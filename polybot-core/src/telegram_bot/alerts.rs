use std::sync::Arc;

use teloxide::prelude::*;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use crate::metrics::Metrics;

#[derive(Debug, Clone)]
pub enum AlertLevel {
    Critical,
    Warning,
    Info,
}

#[derive(Debug, Clone)]
pub struct AlertMessage {
    pub level: AlertLevel,
    pub message: String,
}

#[derive(Clone)]
pub struct AlertBroadcaster {
    sender: UnboundedSender<AlertMessage>,
}

impl AlertBroadcaster {
    pub fn new(sender: UnboundedSender<AlertMessage>) -> Self {
        Self { sender }
    }

    pub fn critical(&self, message: impl Into<String>) {
        let _ = self.sender.send(AlertMessage {
            level: AlertLevel::Critical,
            message: message.into(),
        });
    }

    pub fn warning(&self, message: impl Into<String>) {
        let _ = self.sender.send(AlertMessage {
            level: AlertLevel::Warning,
            message: message.into(),
        });
    }

    pub fn info(&self, message: impl Into<String>) {
        let _ = self.sender.send(AlertMessage {
            level: AlertLevel::Info,
            message: message.into(),
        });
    }
}

pub fn format_report(metrics: &Metrics, period: &str) -> String {
    let uptime = metrics.uptime_secs();
    format!(
        "{} Report\nUptime: {}h {}m\nSignals: {}\nLive trades: {}\nSim trades: {}\nFailed: {}\nOpen positions: {}\nDaily PnL: ${:.2}\nDrawdown: {:.2}%\nAvg latency: {}us\nMax latency: {}us",
        period,
        uptime / 3600,
        (uptime % 3600) / 60,
        metrics.signals_received.load(std::sync::atomic::Ordering::Relaxed),
        metrics.trades_executed.load(std::sync::atomic::Ordering::Relaxed),
        metrics.trades_simulated.load(std::sync::atomic::Ordering::Relaxed),
        metrics.trades_failed.load(std::sync::atomic::Ordering::Relaxed),
        metrics.open_positions.load(std::sync::atomic::Ordering::Relaxed),
        metrics.daily_pnl_usd(),
        metrics.current_drawdown_pct() * 100.0,
        metrics.avg_latency_us.load(std::sync::atomic::Ordering::Relaxed),
        metrics.max_latency_us.load(std::sync::atomic::Ordering::Relaxed),
    )
}

pub fn spawn_dispatcher(
    bot: Bot,
    allowed_users: Vec<u64>,
    metrics: Arc<Metrics>,
    mut receiver: UnboundedReceiver<AlertMessage>,
) {
    let daily_secs = std::env::var("POLYBOT_DAILY_DIGEST_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(86_400);
    let weekly_secs = std::env::var("POLYBOT_WEEKLY_DIGEST_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(604_800);

    tokio::spawn(async move {
        let mut daily = tokio::time::interval(std::time::Duration::from_secs(daily_secs));
        let mut weekly = tokio::time::interval(std::time::Duration::from_secs(weekly_secs));

        loop {
            tokio::select! {
                Some(alert) = receiver.recv() => {
                    let prefix = match alert.level {
                        AlertLevel::Critical => "CRITICAL",
                        AlertLevel::Warning => "WARNING",
                        AlertLevel::Info => "INFO",
                    };
                    let text = format!("{}: {}", prefix, alert.message);
                    for user_id in &allowed_users {
                        if let Err(e) = bot.send_message(ChatId(*user_id as i64), text.clone()).await {
                            tracing::error!(error = %e, "Failed to send Telegram alert");
                        }
                    }
                }
                _ = daily.tick() => {
                    let text = format_report(&metrics, "Daily");
                    for user_id in &allowed_users {
                        if let Err(e) = bot.send_message(ChatId(*user_id as i64), text.clone()).await {
                            tracing::error!(error = %e, "Failed to send daily digest");
                        }
                    }
                }
                _ = weekly.tick() => {
                    let text = format_report(&metrics, "Weekly");
                    for user_id in &allowed_users {
                        if let Err(e) = bot.send_message(ChatId(*user_id as i64), text.clone()).await {
                            tracing::error!(error = %e, "Failed to send weekly digest");
                        }
                    }
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc::unbounded_channel;

    #[test]
    fn alert_sender_creation() {
        let (tx, _rx) = unbounded_channel();
        let _sender = AlertBroadcaster::new(tx);
    }

    #[test]
    fn report_format_includes_metrics() {
        let metrics = Metrics::new();
        metrics.record_signal_received();
        let report = format_report(&metrics, "Daily");
        assert!(report.contains("Daily Report"));
        assert!(report.contains("Signals: 1"));
    }
}
