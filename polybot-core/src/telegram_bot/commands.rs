use std::sync::atomic::Ordering;
use teloxide::prelude::*;

use super::alerts;
use super::auth::AuthService;
use super::confirm::{ConfirmAction, ConfirmState};
use super::rate_limiter::CommandRateLimiter;
use super::Command;
use crate::config::AppConfig;
use crate::metrics::Metrics;
use crate::risk::RiskEngine;
use crate::state;
use crate::state::positions::PositionManager;
use crate::state::reconciliation::Reconciler;
use crate::state::redis_store::RedisStore;
use crate::state::sqlite::{SignalLogEntry, SqliteStore};
use polybot_common::types::Position;
use std::sync::Arc;
use tokio::sync::Mutex;

pub async fn handle_command(
    bot: Bot,
    msg: teloxide::types::Message,
    cmd: Command,
    auth: Arc<AuthService>,
    confirm_state: Arc<ConfirmState>,
    rate_limiter: Arc<CommandRateLimiter>,
    risk_engine: Arc<RiskEngine>,
    reconciler: Arc<Reconciler>,
    config: Arc<AppConfig>,
    metrics: Arc<Metrics>,
    position_manager: Arc<Mutex<PositionManager>>,
) -> ResponseResult<()> {
    let user_id = match msg.from {
        Some(user) => user.id.0,
        None => return Ok(()),
    };

    // v2.5: Auth check on every message
    if !auth.is_allowed(user_id) {
        return Ok(());
    }

    // v2.5: Rate limit check (except /confirm)
    if !matches!(cmd, Command::Confirm) {
        if !rate_limiter.check_command(user_id) {
            bot.send_message(msg.chat.id, "Rate limited. Max 30 commands per minute.")
                .await?;
            return Ok(());
        }
    }

    match cmd {
        Command::Status => {
            let mode = if config.system.simulation {
                "SIMULATION"
            } else {
                "LIVE"
            };
            let uptime = metrics.uptime_secs();
            let uptime_fmt = format!(
                "{}h {}m {}s",
                uptime / 3600,
                (uptime % 3600) / 60,
                uptime % 60
            );
            let ws = if metrics.ws_connected.load(Ordering::Relaxed) == 1 {
                "Connected"
            } else {
                "Disconnected"
            };
            let redis = if metrics.redis_connected.load(Ordering::Relaxed) == 1 {
                "Connected"
            } else {
                "Disconnected"
            };
            let rpc = if metrics.rpc_healthy.load(Ordering::Relaxed) == 1 {
                "Healthy"
            } else {
                "Unhealthy"
            };
            let open = metrics.open_positions.load(Ordering::Relaxed);
            let sigs = metrics.signals_received.load(Ordering::Relaxed);
            let wallets = risk_engine.list_followed_wallets().await.len();
            let bot_status = if risk_engine.is_emergency_stop().await {
                "PAUSED"
            } else {
                "ACTIVE"
            };

            bot.send_message(
                msg.chat.id,
                format!(
                    "SuperFast PolyBot v2.5\nMode: {}\nStatus: {}\nUptime: {}\nPositions: {}\nSignals: {}\nFollowed wallets: {}\nWS: {}\nRedis: {}\nRPC: {}",
                    mode, bot_status, uptime_fmt, open, sigs, wallets, ws, redis, rpc
                )
            ).await?;
        }

        Command::Positions => {
            let pnl = metrics.daily_pnl_usd();
            let body = match RedisStore::new(&config.redis.url).await {
                Ok(store) => match store.list_positions().await {
                    Ok(positions) => format_positions_message(&positions, pnl),
                    Err(_) => fallback_positions_message(&metrics),
                },
                Err(_) => fallback_positions_message(&metrics),
            };

            bot.send_message(msg.chat.id, body).await?;
        }

        Command::Signals => {
            let sqlite_path = std::env::var("POLYBOT_SQLITE_PATH")
                .unwrap_or_else(|_| "./polybot.db".to_string());
            let body = match SqliteStore::open(std::path::Path::new(&sqlite_path)) {
                Ok(store) => match store.latest_signals(10) {
                    Ok(signals) => format_signals_message(&signals),
                    Err(_) => fallback_signals_message(&metrics),
                },
                Err(_) => fallback_signals_message(&metrics),
            };
            bot.send_message(msg.chat.id, body).await?;
        }

        Command::Pause => {
            risk_engine.set_emergency_stop(true).await;
            bot.send_message(
                msg.chat.id,
                "Trading paused. Existing positions held. Use /resume to restart.",
            )
            .await?;
        }

        Command::Resume => {
            if risk_engine.resume_requires_confirmation().await && !confirm_state.has_pending(user_id) {
                confirm_state.register(user_id, ConfirmAction::ResumeAfterLoss);
                bot.send_message(
                    msg.chat.id,
                    "Resume after loss breach requires confirmation. Reply /confirm within 30 seconds.",
                )
                .await?;
            } else if confirm_state.has_pending(user_id) {
                risk_engine.set_emergency_stop(false).await;
                risk_engine.clear_resume_confirmation().await;
                confirm_state.cancel(user_id);
                bot.send_message(msg.chat.id, "Trading resumed.").await?;
            } else {
                risk_engine.set_emergency_stop(false).await;
                risk_engine.clear_resume_confirmation().await;
                bot.send_message(msg.chat.id, "Trading resumed.").await?;
            }
        }

        Command::EmergencyStop => {
            if !rate_limiter.check_emergency_stop(user_id) {
                bot.send_message(
                    msg.chat.id,
                    "Emergency stop rate limit reached (max 3/hour). Please wait.",
                )
                .await?;
                return Ok(());
            }
            confirm_state.register(user_id, ConfirmAction::EmergencyStop);
            metrics.record_emergency_stop();
            bot.send_message(
                msg.chat.id,
                "Destructive command: EMERGENCY STOP\nThis will close all positions as market orders.\nReply /confirm within 30 seconds to proceed."
            ).await?;
        }

        Command::Confirm => match confirm_state.confirm(user_id) {
            Some(ConfirmAction::EmergencyStop) => {
                risk_engine.set_emergency_stop(true).await;
                let closed_positions = state::force_flatten_positions(
                    Some(config.redis.url.as_str()),
                    metrics.clone(),
                    position_manager.clone(),
                )
                .await
                .unwrap_or(0);
                bot.send_message(
                    msg.chat.id,
                    format!(
                        "EMERGENCY STOP CONFIRMED. All trading halted. Closed {} open positions.",
                        closed_positions
                    ),
                )
                .await?;
            }
            Some(ConfirmAction::ResumeAfterLoss) => {
                risk_engine.set_emergency_stop(false).await;
                risk_engine.clear_resume_confirmation().await;
                bot.send_message(msg.chat.id, "Resume confirmed. Trading resumed.")
                    .await?;
            }
            Some(ConfirmAction::WalletRemove(addr)) => {
                risk_engine.remove_followed_wallet(&addr).await;
                bot.send_message(msg.chat.id, format!("Wallet {} removal confirmed.", addr))
                    .await?;
            }
            None => {
                bot.send_message(msg.chat.id, "No pending confirmation. Send a destructive command first, then /confirm within 30 seconds.").await?;
            }
        },

        Command::Report => {
            bot.send_message(msg.chat.id, alerts::format_report(&metrics, "Daily")).await?;
        }

        Command::Wallet(action, address) => {
            if !risk_engine.is_emergency_stop().await {
                bot.send_message(msg.chat.id, "Pause trading before changing the followed wallet list.")
                    .await?;
                return Ok(());
            }

            match action.as_str() {
                "add" => match risk_engine.add_followed_wallet(&address).await {
                    Ok(()) => {
                        bot.send_message(msg.chat.id, format!("Wallet {} added to copy list.", address)).await?;
                    }
                    Err(e) => {
                        bot.send_message(msg.chat.id, format!("Wallet add failed: {}", e)).await?;
                    }
                },
                "remove" => {
                    confirm_state.register(user_id, ConfirmAction::WalletRemove(address.clone()));
                    bot.send_message(
                        msg.chat.id,
                        format!("Removing wallet {} is destructive. Reply /confirm within 30 seconds.", address),
                    )
                    .await?;
                }
                "list" => {
                    let wallets = risk_engine.list_followed_wallets().await;
                    let body = if wallets.is_empty() {
                        "No followed wallets configured.".to_string()
                    } else {
                        format!("Followed wallets:\n{}", wallets.join("\n"))
                    };
                    bot.send_message(msg.chat.id, body).await?;
                }
                _ => {
                    bot.send_message(msg.chat.id, "Usage: /wallet add <address> | /wallet remove <address> | /wallet list x")
                        .await?;
                }
            }
        }

        Command::Config(key, value) => {
            match risk_engine.update_runtime_config(&key, &value).await {
                Ok(message) => {
                    let summary = risk_engine.runtime_config_summary().await;
                    bot.send_message(msg.chat.id, format!("{}\n{}", message, summary)).await?;
                }
                Err(e) => {
                    bot.send_message(msg.chat.id, format!("Config update failed: {}", e)).await?;
                }
            }
        }

        Command::Reconcile => match reconciler.force_reconcile().await {
            Ok(result) => {
                bot.send_message(
                    msg.chat.id,
                    format!("Full reconciliation completed.\n{}", result.summary()),
                )
                .await?;
            }
            Err(e) => {
                bot.send_message(msg.chat.id, format!("Full reconciliation failed: {}", e))
                    .await?;
            }
        },

        Command::Ratelimit => {
            let (cmds, es) = rate_limiter.get_stats(user_id);
            bot.send_message(
                msg.chat.id,
                format!("Rate Limit Status:\nCommands this minute: {}/30\nEmergency stops this hour: {}/3", cmds, es)
            ).await?;
        }
    }

    Ok(())
}

fn format_positions_message(positions: &[Position], pnl: f64) -> String {
    if positions.is_empty() {
        return format!("Positions:\nNo open positions\nDaily PnL: ${:.2}", pnl);
    }

    let lines = positions
        .iter()
        .map(|position| {
            format!(
                "{} {:?} size={} avg={} status={:?}",
                position.market_id,
                position.side,
                position.current_size,
                position.average_price,
                position.status
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        "Positions:\n{}\nOpen: {}\nDaily PnL: ${:.2}",
        lines,
        positions.len(),
        pnl
    )
}

fn fallback_positions_message(metrics: &Metrics) -> String {
    let open = metrics.open_positions.load(Ordering::Relaxed);
    let total_opened = metrics.total_positions_opened.load(Ordering::Relaxed);
    let total_closed = metrics.total_positions_closed.load(Ordering::Relaxed);
    let pnl = metrics.daily_pnl_usd();

    format!(
        "Positions:\nOpen: {}\nTotal opened: {}\nTotal closed: {}\nDaily PnL: ${:.2}",
        open, total_opened, total_closed, pnl
    )
}

fn format_signals_message(signals: &[SignalLogEntry]) -> String {
    if signals.is_empty() {
        return "Signals:\nNo recent signals".to_string();
    }

    let lines = signals
        .iter()
        .map(|signal| {
            format!(
                "{} {} conf={} secret={} {}",
                signal.market_id,
                signal.side,
                signal.confidence,
                signal.secret_level,
                signal.disposition
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    format!("Signals:\n{}", lines)
}

fn fallback_signals_message(metrics: &Metrics) -> String {
    let received = metrics.signals_received.load(Ordering::Relaxed);
    let processed = metrics.signals_processed.load(Ordering::Relaxed);
    let skipped = metrics.signals_skipped.load(Ordering::Relaxed);
    let manual = metrics.signals_manual_review.load(Ordering::Relaxed);
    let last = metrics
        .last_signal_at
        .lock()
        .ok()
        .and_then(|g| g.clone())
        .unwrap_or_else(|| "Never".to_string());

    format!(
        "Signals:\nReceived: {}\nProcessed: {}\nSkipped: {}\nManual review: {}\nLast signal: {}",
        received, processed, skipped, manual, last
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use polybot_common::types::{Category, PositionStatus, Side};
    use rust_decimal_macros::dec;

    fn sample_position() -> Position {
        Position {
            id: "p1".to_string(),
            market_id: "market-1".to_string(),
            side: Side::Yes,
            entry_price: dec!(0.50),
            current_size: dec!(100),
            average_price: dec!(0.52),
            opened_at: Utc::now(),
            status: PositionStatus::Open,
            category: Category::Politics,
        }
    }

    fn sample_signal() -> SignalLogEntry {
        SignalLogEntry {
            signal_id: "signal-1".to_string(),
            timestamp: "2026-04-17T12:00:00Z".to_string(),
            wallet_address: "0xabc123abc123abc123abc123abc123abc123abc1".to_string(),
            market_id: "market-1".to_string(),
            confidence: 8,
            secret_level: 9,
            category: "politics".to_string(),
            side: "YES".to_string(),
            disposition: "execute".to_string(),
            received_at: "2026-04-17T12:00:01Z".to_string(),
        }
    }

    #[test]
    fn format_positions_message_lists_open_positions() {
        let output = format_positions_message(&[sample_position()], 12.34);
        assert!(output.contains("market-1"));
        assert!(output.contains("Open: 1"));
        assert!(output.contains("Daily PnL: $12.34"));
    }

    #[test]
    fn format_signals_message_lists_recent_signals() {
        let output = format_signals_message(&[sample_signal()]);
        assert!(output.contains("market-1"));
        assert!(output.contains("conf=8"));
        assert!(output.contains("execute"));
    }
}
