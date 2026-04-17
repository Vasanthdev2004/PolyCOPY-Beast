use polybot_common::constants as C;
use polybot_common::errors::PolybotError;
use rust_decimal::Decimal;

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub system: SystemConfig,
    pub risk: RiskConfig,
    pub scanner: ScannerConfig,
    pub execution: ExecutionConfig,
    pub telegram: TelegramConfig,
    pub redis: RedisConfig,
    pub dashboard: DashboardConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemConfig {
    pub simulation: bool,
    pub log_level: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskConfig {
    pub base_size_usd: Decimal,
    pub base_size_pct: Decimal,
    pub daily_max_loss_pct: Decimal,
    pub per_market_exposure_pct: Decimal,
    pub per_category_exposure_pct: Decimal,
    pub max_position_size_usd: Decimal,
    pub max_concurrent_positions: u32,
    pub max_market_liquidity_pct: Decimal,
    pub min_confidence: u8,
    pub min_secret_level: u8,
    pub slippage_threshold: Decimal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScannerConfig {
    pub watch_dir: String,
    pub processed_dir: String,
    pub dedup_window_secs: u64,
    pub http_port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionConfig {
    pub slippage_threshold: Decimal,
    pub rpc_endpoints: Vec<String>,
    pub ws_reconnect_max_wait_secs: u64,
    pub heartbeat_interval_secs: u64,
    pub order_timeout_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelegramConfig {
    pub allowed_user_ids: Vec<u64>,
    pub command_rate_limit_per_min: u32,
    pub emergency_stop_limit_per_hour: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedisConfig {
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardConfig {
    pub host: String,
    pub port: u16,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            system: SystemConfig {
                simulation: true,
                log_level: "info".to_string(),
            },
            risk: RiskConfig {
                base_size_usd: rust_decimal_macros::dec!(50),
                base_size_pct: C::DEFAULT_BASE_SIZE_PCT,
                daily_max_loss_pct: C::DEFAULT_DAILY_MAX_LOSS_PCT,
                per_market_exposure_pct: C::DEFAULT_PER_MARKET_EXPOSURE_PCT,
                per_category_exposure_pct: C::DEFAULT_PER_CATEGORY_EXPOSURE_PCT,
                max_position_size_usd: C::MAX_POSITION_USDC,
                max_concurrent_positions: C::MAX_CONCURRENT_POSITIONS,
                max_market_liquidity_pct: C::MAX_MARKET_LIQUIDITY_PCT,
                min_confidence: C::DEFAULT_MIN_CONFIDENCE,
                min_secret_level: C::DEFAULT_MIN_SECRET_LEVEL,
                slippage_threshold: C::DEFAULT_SLIPPAGE_THRESHOLD,
            },
            scanner: ScannerConfig {
                watch_dir: "./signals".to_string(),
                processed_dir: "./signals/processed".to_string(),
                dedup_window_secs: C::DEFAULT_DEDUP_WINDOW_SECS,
                http_port: 8081,
            },
            execution: ExecutionConfig {
                slippage_threshold: C::DEFAULT_SLIPPAGE_THRESHOLD,
                rpc_endpoints: vec!["https://polygon-rpc.com".to_string()],
                ws_reconnect_max_wait_secs: 60,
                heartbeat_interval_secs: C::WS_HEARTBEAT_SECS,
                order_timeout_secs: C::ORDER_TIMEOUT_SECS,
            },
            telegram: TelegramConfig {
                allowed_user_ids: vec![],
                command_rate_limit_per_min: 30,
                emergency_stop_limit_per_hour: 3,
            },
            redis: RedisConfig {
                url: "redis://127.0.0.1:6379".to_string(),
            },
            dashboard: DashboardConfig {
                host: "0.0.0.0".to_string(),
                port: 8080,
            },
        }
    }
}

impl AppConfig {
    pub fn load_from_file(path: &Path) -> Result<Self, PolybotError> {
        let content = std::fs::read_to_string(path).map_err(|e| {
            PolybotError::Config(format!(
                "Failed to read config file {}: {}",
                path.display(),
                e
            ))
        })?;
        let config: AppConfig = toml::from_str(&content)
            .map_err(|e| PolybotError::Config(format!("Failed to parse config: {}", e)))?;
        config.validate()?;
        Ok(config)
    }

    pub fn load() -> Result<Self, PolybotError> {
        let env_path = Path::new("config.toml");
        if env_path.exists() {
            Self::load_from_file(env_path)
        } else {
            tracing::info!("No config.toml found, using defaults");
            let config = Self::default();
            config.validate()?;
            Ok(config)
        }
    }

    pub fn validate(&self) -> Result<(), PolybotError> {
        if self.risk.base_size_usd <= Decimal::ZERO && self.risk.base_size_pct <= Decimal::ZERO {
            return Err(PolybotError::Config(
                "base_size_usd or base_size_pct must be positive".to_string(),
            ));
        }
        if self.risk.daily_max_loss_pct <= Decimal::ZERO
            || self.risk.daily_max_loss_pct > Decimal::ONE
        {
            return Err(PolybotError::Config(
                "daily_max_loss_pct must be between 0 and 1".to_string(),
            ));
        }
        if self.risk.min_confidence < 1 || self.risk.min_confidence > 10 {
            return Err(PolybotError::Config(
                "min_confidence must be between 1 and 10".to_string(),
            ));
        }
        if self.risk.max_concurrent_positions == 0 {
            return Err(PolybotError::Config(
                "max_concurrent_positions must be > 0".to_string(),
            ));
        }
        if self.execution.rpc_endpoints.len() < 2 {
            tracing::warn!(
                "v2.5 requires minimum 2 RPC endpoints. Only {} configured.",
                self.execution.rpc_endpoints.len()
            );
        }
        if self.execution.rpc_endpoints.is_empty() {
            return Err(PolybotError::Config(
                "At least one RPC endpoint required".to_string(),
            ));
        }
        if self.scanner.dedup_window_secs == 0 {
            return Err(PolybotError::Config(
                "dedup_window_secs must be > 0".to_string(),
            ));
        }
        Ok(())
    }

    pub fn apply_env_overrides(&mut self) {
        if let Ok(val) = std::env::var("POLYBOT_SIMULATION") {
            self.system.simulation = val.to_lowercase() == "true" || val == "1";
        }
        if let Ok(val) = std::env::var("POLYBOT_LOG_LEVEL") {
            self.system.log_level = val;
        }
        if let Ok(val) = std::env::var("POLYBOT_REDIS_URL") {
            self.redis.url = val;
        }
        if let Ok(val) = std::env::var("POLYBOT_BASE_SIZE_USD") {
            if let Ok(d) = val.parse::<Decimal>() {
                self.risk.base_size_usd = d;
            }
        }
        if let Ok(val) = std::env::var("TELEGRAM_ALLOWED_USER_IDS")
            .or_else(|_| std::env::var("POLYBOT_TELEGRAM_ALLOWED_USER_IDS"))
        {
            self.telegram.allowed_user_ids = val
                .split(',')
                .filter_map(|s| s.trim().parse::<u64>().ok())
                .collect();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_valid() {
        let config = AppConfig::default();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn invalid_base_size_rejected() {
        let mut config = AppConfig::default();
        config.risk.base_size_usd = Decimal::ZERO;
        config.risk.base_size_pct = Decimal::ZERO;
        assert!(config.validate().is_err());
    }

    #[test]
    fn empty_rpc_endpoints_rejected() {
        let mut config = AppConfig::default();
        config.execution.rpc_endpoints = vec![];
        assert!(config.validate().is_err());
    }

    #[test]
    fn max_concurrent_positions_default() {
        let config = AppConfig::default();
        assert_eq!(config.risk.max_concurrent_positions, 20);
    }

    #[test]
    fn apply_env_overrides_simulation() {
        std::env::set_var("POLYBOT_SIMULATION", "true");
        let mut config = AppConfig::default();
        config.apply_env_overrides();
        assert!(config.system.simulation);
        std::env::remove_var("POLYBOT_SIMULATION");
    }

    #[test]
    fn apply_env_telegram_user_ids() {
        std::env::set_var("POLYBOT_TELEGRAM_ALLOWED_USER_IDS", "123,456,789");
        let mut config = AppConfig::default();
        config.apply_env_overrides();
        assert_eq!(config.telegram.allowed_user_ids, vec![123, 456, 789]);
        std::env::remove_var("POLYBOT_TELEGRAM_ALLOWED_USER_IDS");
    }
}
