pub mod alerts;
pub mod auth;
pub mod commands;
pub mod confirm;
pub mod rate_limiter;

use polybot_common::errors::PolybotError;
use std::sync::Arc;

use crate::config::AppConfig;
use crate::metrics::Metrics;
use crate::risk::RiskEngine;
use crate::state::positions::PositionManager;
use crate::state::reconciliation::Reconciler;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::sync::Mutex;

use teloxide::prelude::*;
use teloxide::utils::command::BotCommands;

#[derive(BotCommands, Clone)]
#[command(rename_rule = "lowercase", description = "PolyBot v2.5 commands", parse_with = "split")]
pub enum Command {
    #[command(description = "Show system status")]
    Status,
    #[command(description = "Show open positions")]
    Positions,
    #[command(description = "Show recent signals")]
    Signals,
    #[command(description = "Pause trading")]
    Pause,
    #[command(description = "Resume trading")]
    Resume,
    #[command(description = "Emergency stop (requires /confirm)")]
    EmergencyStop,
    #[command(description = "Confirm destructive command")]
    Confirm,
    #[command(description = "Show daily/weekly report")]
    Report,
    #[command(description = "Manage followed wallets: /wallet add <address> or /wallet remove <address>")]
    Wallet(String, String),
    #[command(description = "Update runtime risk config: /config <key> <value>")]
    Config(String, String),
    #[command(description = "Force full reconciliation")]
    Reconcile,
    #[command(description = "Show API rate limit usage")]
    Ratelimit,
}

pub async fn start_telegram_bot(
    config: Arc<AppConfig>,
    risk_engine: Arc<RiskEngine>,
    reconciler: Arc<Reconciler>,
    metrics: Arc<Metrics>,
    position_manager: Arc<Mutex<PositionManager>>,
    alert_receiver: UnboundedReceiver<alerts::AlertMessage>,
) -> Result<(), PolybotError> {
    let bot_token = std::env::var("POLYBOT_TELEGRAM_TOKEN")
        .map_err(|_| PolybotError::Telegram("POLYBOT_TELEGRAM_TOKEN not set".to_string()))?;

    let allowed_users = std::env::var("TELEGRAM_ALLOWED_USER_IDS")
        .or_else(|_| std::env::var("POLYBOT_TELEGRAM_ALLOWED_USER_IDS"))
        .ok()
        .map(|s| {
            s.split(',')
                .filter_map(|id| id.trim().parse::<u64>().ok())
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(|| config.telegram.allowed_user_ids.clone());

    if allowed_users.is_empty() {
        tracing::warn!("No Telegram users whitelisted — bot will not respond to anyone");
    }

    let bot = Bot::new(bot_token);
    bot.get_me()
        .send()
        .await
        .map_err(|e| PolybotError::Telegram(format!("Telegram bot authentication failed: {}", e)))?;

    // Shared state for auth, confirm, and rate limiting
    let auth = Arc::new(auth::AuthService::new(allowed_users));
    let confirm_state = Arc::new(confirm::ConfirmState::new());
    let rate_limiter = Arc::new(rate_limiter::CommandRateLimiter::new(
        config.telegram.command_rate_limit_per_min,
        config.telegram.emergency_stop_limit_per_hour,
    ));

    tracing::info!(
        users = auth.allowed_count(),
        "Telegram bot starting with auth"
    );

    alerts::spawn_dispatcher(bot.clone(), auth.allowed_users(), metrics.clone(), alert_receiver);

    let handler =
        Update::filter_message().branch(dptree::entry().filter_command::<Command>().endpoint(
            move |bot, msg, cmd| {
                let auth = auth.clone();
                let confirm_state = confirm_state.clone();
                let rate_limiter = rate_limiter.clone();
                let risk_engine = risk_engine.clone();
                let reconciler = reconciler.clone();
                let config = config.clone();
                let metrics = metrics.clone();
                let position_manager = position_manager.clone();
                async move {
                    commands::handle_command(
                        bot,
                        msg,
                        cmd,
                        auth,
                        confirm_state,
                        rate_limiter,
                        risk_engine,
                        reconciler,
                        config,
                        metrics,
                        position_manager,
                    )
                    .await
                }
            },
        ));

    Dispatcher::builder(bot, handler).build().dispatch().await;

    Ok(())
}
