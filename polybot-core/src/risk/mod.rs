pub mod balance;
pub mod drawdown;
pub mod limits;
pub mod sizer;

use polybot_common::constants::{
    confidence_multiplier, drawdown_multiplier as calc_drawdown, secret_level_multiplier,
};
use polybot_common::constants::{MAX_POSITION_USDC, MIN_POSITION_USDC};
use rust_decimal::prelude::ToPrimitive;
use polybot_common::types::{Decision, RiskDecision, Signal};
use rust_decimal::Decimal;
use std::collections::BTreeSet;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex, RwLock};

use crate::config::{AppConfig, RiskConfig};
use crate::metrics::Metrics;
use crate::state::positions::PositionManager;
use crate::state::sqlite::SqliteStore;
use crate::telegram_bot::alerts::AlertBroadcaster;

pub struct RiskEngine {
    config: Arc<AppConfig>,
    metrics: Arc<Metrics>,
    portfolio_drawdown_pct: Arc<Mutex<Decimal>>,
    emergency_stop: Arc<Mutex<bool>>,
    resume_requires_confirm: Arc<Mutex<bool>>,
    position_manager: Arc<Mutex<PositionManager>>,
    runtime_risk: Arc<RwLock<RiskConfig>>,
    followed_wallets: Arc<RwLock<BTreeSet<String>>>,
    alerts: Option<AlertBroadcaster>,
}

impl RiskEngine {
    pub fn new(
        config: Arc<AppConfig>,
        metrics: Arc<Metrics>,
        position_manager: Arc<Mutex<PositionManager>>,
        alerts: Option<AlertBroadcaster>,
    ) -> Self {
        let runtime_risk = config.risk.clone();
        let followed_wallets = std::env::var("POLYBOT_FOLLOW_WALLETS")
            .ok()
            .map(|value| {
                value
                    .split(',')
                    .map(|wallet| wallet.trim().to_lowercase())
                    .filter(|wallet| !wallet.is_empty())
                    .collect::<BTreeSet<_>>()
            })
            .unwrap_or_default();

        Self {
            config,
            metrics,
            portfolio_drawdown_pct: Arc::new(Mutex::new(Decimal::ZERO)),
            emergency_stop: Arc::new(Mutex::new(false)),
            resume_requires_confirm: Arc::new(Mutex::new(false)),
            position_manager,
            runtime_risk: Arc::new(RwLock::new(runtime_risk)),
            followed_wallets: Arc::new(RwLock::new(followed_wallets)),
            alerts,
        }
    }

    fn portfolio_reference_usd(&self, risk_config: &RiskConfig) -> Decimal {
        if risk_config.base_size_pct > Decimal::ZERO {
            risk_config.base_size_usd / risk_config.base_size_pct
        } else {
            risk_config.base_size_usd
        }
    }

    pub async fn evaluate(&self, signal: &Signal) -> RiskDecision {
        let risk_config = self.runtime_risk.read().await.clone();

        // 1. Check emergency stop
        if *self.emergency_stop.lock().await {
            return RiskDecision {
                signal_id: signal.signal_id.clone(),
                market_id: signal.market_id.clone(),
                side: signal.side,
                category: signal.category,
                position_size_usd: Decimal::ZERO,
                confidence_multiplier: Decimal::ZERO,
                secret_level_multiplier: Decimal::ZERO,
                drawdown_factor: Decimal::ZERO,
                blocked: true,
                manual_review: false,
                decision: Decision::EmergencyStop,
            };
        }

        let followed_wallets = self.followed_wallets.read().await;
        if !followed_wallets.is_empty() && !followed_wallets.contains(&signal.wallet_address.to_lowercase()) {
            return RiskDecision {
                signal_id: signal.signal_id.clone(),
                market_id: signal.market_id.clone(),
                side: signal.side,
                category: signal.category,
                position_size_usd: Decimal::ZERO,
                confidence_multiplier: Decimal::ZERO,
                secret_level_multiplier: Decimal::ZERO,
                drawdown_factor: Decimal::ZERO,
                blocked: true,
                manual_review: false,
                decision: Decision::Skip("wallet not in followed list".to_string()),
            };
        }
        drop(followed_wallets);

        // 2. v2.5: Check manual review (confidence < 3 or secret_level < 3)
        let manual_review = signal.requires_manual_review();
        if manual_review {
            if signal.secret_level >= 8 {
                if let Some(alerts) = &self.alerts {
                    alerts.warning(format!(
                        "High-secret signal {} queued for manual review from {}",
                        signal.signal_id, signal.wallet_address
                    ));
                }
            }
            return RiskDecision {
                signal_id: signal.signal_id.clone(),
                market_id: signal.market_id.clone(),
                side: signal.side,
                category: signal.category,
                position_size_usd: Decimal::ZERO,
                confidence_multiplier: confidence_multiplier(signal.confidence),
                secret_level_multiplier: secret_level_multiplier(signal.secret_level),
                drawdown_factor: Decimal::ZERO,
                blocked: false,
                manual_review: true,
                decision: Decision::ManualReview,
            };
        }

        // 3. v2.5: Check per-category thresholds
        if signal.is_blocked_by_category_thresholds() {
            let reason = format!(
                "Blocked by category thresholds: {} requires confidence>={}, secret_level>={}. Got confidence={}, secret_level={}",
                signal.category,
                signal.category.min_confidence_threshold(),
                signal.category.min_secret_level_threshold(),
                signal.confidence,
                signal.secret_level,
            );
            return RiskDecision {
                signal_id: signal.signal_id.clone(),
                market_id: signal.market_id.clone(),
                side: signal.side,
                category: signal.category,
                position_size_usd: Decimal::ZERO,
                confidence_multiplier: confidence_multiplier(signal.confidence),
                secret_level_multiplier: secret_level_multiplier(signal.secret_level),
                drawdown_factor: Decimal::ZERO,
                blocked: true,
                manual_review: false,
                decision: Decision::Skip(reason),
            };
        }

        // 4. v2.5: v2.5 uses confidence (not secret_level) for confidence_multiplier
        let conf_mult = confidence_multiplier(signal.confidence);
        let sl_mult = secret_level_multiplier(signal.secret_level);

        if signal.secret_level >= 8 {
            if let Some(alerts) = &self.alerts {
                alerts.info(format!(
                    "High-secret signal received: {} market={} wallet={} confidence={} secret_level={}",
                    signal.signal_id,
                    signal.market_id,
                    signal.wallet_address,
                    signal.confidence,
                    signal.secret_level
                ));
            }
        }

        // 5. v2.5: Stepped drawdown multiplier
        let current_drawdown = *self.portfolio_drawdown_pct.lock().await;
        let dd_factor = calc_drawdown(current_drawdown);

        // v2.5: if drawdown > 20%, auto-pause
        if dd_factor == Decimal::ZERO {
            tracing::error!("Portfolio drawdown > 20%! Auto-pausing trading.");
            *self.resume_requires_confirm.lock().await = true;
            self.set_emergency_stop(true).await;
            if let Some(alerts) = &self.alerts {
                alerts.critical("Daily loss limit breached. Trading auto-paused.");
            }
            return RiskDecision {
                signal_id: signal.signal_id.clone(),
                market_id: signal.market_id.clone(),
                side: signal.side,
                category: signal.category,
                position_size_usd: Decimal::ZERO,
                confidence_multiplier: conf_mult,
                secret_level_multiplier: sl_mult,
                drawdown_factor: dd_factor,
                blocked: true,
                manual_review: false,
                decision: Decision::EmergencyStop,
            };
        }

        // 6. v2.5: Position sizing
        let mut size = risk_config.base_size_usd * conf_mult * sl_mult * dd_factor;

        // Clamp to [MIN_POSITION_USDC, MAX_POSITION_USDC]
        // Also v2.5: cap by per-category max single position
        let category_max = signal.category.max_single_position_usd();
        let max_size = MAX_POSITION_USDC.min(category_max);

        if size > max_size {
            size = max_size;
        }
        if size < MIN_POSITION_USDC {
            size = MIN_POSITION_USDC;
        }

        let (open_count, market_exposure, category_exposure) = {
            let positions = self.position_manager.lock().await;
            (
                positions.open_position_count(),
                positions.market_exposure(&signal.market_id),
                positions.category_exposure(signal.category),
            )
        };

        // 7. v2.5: Check max concurrent positions
        if open_count >= risk_config.max_concurrent_positions {
            if let Some(alerts) = &self.alerts {
                alerts.warning(format!(
                    "Risk limit breach: max concurrent positions reached ({}/{})",
                    open_count, risk_config.max_concurrent_positions
                ));
            }
            return RiskDecision {
                signal_id: signal.signal_id.clone(),
                market_id: signal.market_id.clone(),
                side: signal.side,
                category: signal.category,
                position_size_usd: Decimal::ZERO,
                confidence_multiplier: conf_mult,
                secret_level_multiplier: sl_mult,
                drawdown_factor: dd_factor,
                blocked: true,
                manual_review: false,
                decision: Decision::Skip(format!(
                    "Max concurrent positions reached ({}/{})",
                    open_count, risk_config.max_concurrent_positions
                )),
            };
        }

        // 8. Check other limits
        if let Some(reason) = limits::check_limits(
            &risk_config,
            signal,
            size,
            current_drawdown,
            market_exposure,
            category_exposure,
            self.portfolio_reference_usd(&risk_config),
        ) {
            if let Some(alerts) = &self.alerts {
                alerts.warning(format!(
                    "Risk limit breach for signal {}: {}",
                    signal.signal_id, reason
                ));
            }
            return RiskDecision {
                signal_id: signal.signal_id.clone(),
                market_id: signal.market_id.clone(),
                side: signal.side,
                category: signal.category,
                position_size_usd: Decimal::ZERO,
                confidence_multiplier: conf_mult,
                secret_level_multiplier: sl_mult,
                drawdown_factor: dd_factor,
                blocked: true,
                manual_review: false,
                decision: Decision::Skip(reason),
            };
        }

        RiskDecision {
            signal_id: signal.signal_id.clone(),
            market_id: signal.market_id.clone(),
            side: signal.side,
            category: signal.category,
            position_size_usd: size,
            confidence_multiplier: conf_mult,
            secret_level_multiplier: sl_mult,
            drawdown_factor: dd_factor,
            blocked: false,
            manual_review: false,
            decision: Decision::Execute,
        }
    }

    pub async fn set_emergency_stop(&self, stopped: bool) {
        *self.emergency_stop.lock().await = stopped;
        self.metrics.set_paused(stopped);
    }

    pub async fn is_emergency_stop(&self) -> bool {
        *self.emergency_stop.lock().await
    }

    #[allow(dead_code)]
    pub async fn update_drawdown(&self, drawdown_pct: Decimal) {
        *self.portfolio_drawdown_pct.lock().await = drawdown_pct;
        self.metrics
            .update_drawdown(drawdown_pct.to_f64().unwrap_or(0.0));
        if drawdown_pct >= dec!(0.20) {
            tracing::error!("Portfolio drawdown >= 20%! Auto-pausing trading per v2.5 rules.");
            self.set_emergency_stop(true).await;
        }
    }

    #[allow(dead_code)]
    pub async fn reset_daily_loss(&self) {
        *self.portfolio_drawdown_pct.lock().await = Decimal::ZERO;
        *self.resume_requires_confirm.lock().await = false;
    }

    pub async fn resume_requires_confirmation(&self) -> bool {
        *self.resume_requires_confirm.lock().await
    }

    pub async fn clear_resume_confirmation(&self) {
        *self.resume_requires_confirm.lock().await = false;
    }

    pub async fn add_followed_wallet(&self, wallet: &str) -> Result<(), polybot_common::errors::PolybotError> {
        let normalized = wallet.trim().to_lowercase();
        if !normalized.starts_with("0x") || normalized.len() != 42 {
            return Err(polybot_common::errors::PolybotError::Config(
                "wallet address must be a 42-char 0x-prefixed EVM address".to_string(),
            ));
        }
        self.followed_wallets.write().await.insert(normalized);
        Ok(())
    }

    pub async fn remove_followed_wallet(&self, wallet: &str) {
        self.followed_wallets.write().await.remove(&wallet.trim().to_lowercase());
    }

    pub async fn list_followed_wallets(&self) -> Vec<String> {
        self.followed_wallets.read().await.iter().cloned().collect()
    }

    pub async fn update_runtime_config(
        &self,
        key: &str,
        value: &str,
    ) -> Result<String, polybot_common::errors::PolybotError> {
        let mut risk = self.runtime_risk.write().await;
        match key {
            "base_size_usd" => risk.base_size_usd = value.parse().map_err(|_| polybot_common::errors::PolybotError::Config("invalid decimal for base_size_usd".to_string()))?,
            "daily_max_loss_pct" => risk.daily_max_loss_pct = value.parse().map_err(|_| polybot_common::errors::PolybotError::Config("invalid decimal for daily_max_loss_pct".to_string()))?,
            "per_market_exposure_pct" => risk.per_market_exposure_pct = value.parse().map_err(|_| polybot_common::errors::PolybotError::Config("invalid decimal for per_market_exposure_pct".to_string()))?,
            "per_category_exposure_pct" => risk.per_category_exposure_pct = value.parse().map_err(|_| polybot_common::errors::PolybotError::Config("invalid decimal for per_category_exposure_pct".to_string()))?,
            "max_position_size_usd" => risk.max_position_size_usd = value.parse().map_err(|_| polybot_common::errors::PolybotError::Config("invalid decimal for max_position_size_usd".to_string()))?,
            "max_concurrent_positions" => risk.max_concurrent_positions = value.parse().map_err(|_| polybot_common::errors::PolybotError::Config("invalid integer for max_concurrent_positions".to_string()))?,
            "min_confidence" => risk.min_confidence = value.parse().map_err(|_| polybot_common::errors::PolybotError::Config("invalid integer for min_confidence".to_string()))?,
            "min_secret_level" => risk.min_secret_level = value.parse().map_err(|_| polybot_common::errors::PolybotError::Config("invalid integer for min_secret_level".to_string()))?,
            "slippage_threshold" => risk.slippage_threshold = value.parse().map_err(|_| polybot_common::errors::PolybotError::Config("invalid decimal for slippage_threshold".to_string()))?,
            other => {
                return Err(polybot_common::errors::PolybotError::Config(format!(
                    "unsupported runtime config key: {}",
                    other
                )))
            }
        }
        Ok(format!("{} updated to {}", key, value))
    }

    pub async fn runtime_config_summary(&self) -> String {
        let risk = self.runtime_risk.read().await;
        format!(
            "base_size_usd={} daily_max_loss_pct={} per_market_exposure_pct={} per_category_exposure_pct={} max_position_size_usd={} max_concurrent_positions={} min_confidence={} min_secret_level={} slippage_threshold={}",
            risk.base_size_usd,
            risk.daily_max_loss_pct,
            risk.per_market_exposure_pct,
            risk.per_category_exposure_pct,
            risk.max_position_size_usd,
            risk.max_concurrent_positions,
            risk.min_confidence,
            risk.min_secret_level,
            risk.slippage_threshold,
        )
    }
}

use rust_decimal_macros::dec;

pub async fn run_risk_engine(
    engine: Arc<RiskEngine>,
    metrics: Arc<Metrics>,
    mut receiver: mpsc::Receiver<polybot_common::types::ScannerEvent>,
    sender: mpsc::Sender<RiskDecision>,
) -> Result<(), polybot_common::errors::PolybotError> {
    while let Some(event) = receiver.recv().await {
        metrics.record_signal_received();
        let decision = engine.evaluate(&event.signal).await;

        if matches!(decision.decision, Decision::Execute) {
            metrics.record_signal_processed();
        }

        let sqlite_path = std::env::var("POLYBOT_SQLITE_PATH")
            .unwrap_or_else(|_| "./polybot.db".to_string());
        if let Ok(store) = SqliteStore::open(std::path::Path::new(&sqlite_path)) {
            let disposition = match &decision.decision {
                Decision::Execute => "execute".to_string(),
                Decision::ManualReview => "manual_review".to_string(),
                Decision::EmergencyStop => "emergency_stop".to_string(),
                Decision::Skip(reason) => format!("skip:{}", reason),
            };
            if let Err(e) = store.insert_signal_log(
                &event.signal.signal_id,
                &event.signal.timestamp,
                &event.signal.wallet_address,
                &event.signal.market_id,
                event.signal.confidence,
                event.signal.secret_level,
                &event.signal.category.to_string(),
                match event.signal.side {
                    polybot_common::types::Side::Yes => "YES",
                    polybot_common::types::Side::No => "NO",
                },
                &disposition,
            ) {
                tracing::error!(error = %e, "Failed to persist signal log to SQLite");
            }
        }

        tracing::info!(
            signal_id = %decision.signal_id,
            decision = ?decision.decision,
            size = %decision.position_size_usd,
            blocked = decision.blocked,
            manual_review = decision.manual_review,
            "Risk decision made"
        );

        if sender.send(decision).await.is_err() {
            tracing::error!("Execution channel closed");
            return Err(polybot_common::errors::PolybotError::ChannelClosed);
        }
    }

    Ok(())
}
