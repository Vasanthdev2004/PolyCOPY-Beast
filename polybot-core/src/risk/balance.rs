use polybot_common::constants::{DEFAULT_BASE_SIZE_PCT, MAX_POSITION_USDC, MIN_POSITION_USDC};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Convert Decimal dollars to u64 cents (multiply by 100, round)
fn dollars_to_cents(d: Decimal) -> u64 {
    let cents = d * dec!(100);
    cents.try_into().unwrap_or(0)
}

/// v2.5: Manages dynamic base size calculation.
/// Base size = 1.5% of portfolio balance, recalculated every 15 minutes.
/// Hard minimum: $5, hard maximum: $500.
pub struct DynamicBaseSize {
    /// Current portfolio balance in USDC cents (for atomic ops)
    portfolio_balance: Arc<AtomicU64>,
    /// Last time balance was fetched
    last_update: Arc<tokio::sync::Mutex<std::time::Instant>>,
    /// Update interval (default 15 minutes)
    update_interval_secs: u64,
}

impl DynamicBaseSize {
    pub fn new(initial_balance_usd: Decimal) -> Self {
        Self {
            portfolio_balance: Arc::new(AtomicU64::new(dollars_to_cents(initial_balance_usd))),
            last_update: Arc::new(tokio::sync::Mutex::new(std::time::Instant::now())),
            update_interval_secs: 900, // 15 minutes
        }
    }

    /// Get the current base size (1.5% of portfolio, clamped to [$5, $500])
    pub fn current_base_size(&self) -> Decimal {
        let balance_cents = self.portfolio_balance.load(Ordering::Relaxed);
        let balance_usd = Decimal::from(balance_cents) / dec!(100);
        let base = balance_usd * DEFAULT_BASE_SIZE_PCT;
        base.min(MAX_POSITION_USDC).max(MIN_POSITION_USDC)
    }

    /// Update the portfolio balance
    pub fn update_balance(&self, new_balance_usd: Decimal) {
        let cents = dollars_to_cents(new_balance_usd);
        self.portfolio_balance.store(cents, Ordering::Relaxed);
        tracing::info!(balance_usd = %new_balance_usd, "Portfolio balance updated");
    }

    /// Check if balance needs updating (every 15 minutes)
    pub async fn maybe_refresh(&self) -> bool {
        let mut last = self.last_update.lock().await;
        if last.elapsed().as_secs() >= self.update_interval_secs {
            *last = std::time::Instant::now();
            true
        } else {
            false
        }
    }

    /// Get current portfolio balance
    pub fn current_balance(&self) -> Decimal {
        let cents = self.portfolio_balance.load(Ordering::Relaxed);
        Decimal::from(cents) / dec!(100)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_size_from_balance() {
        let dbs = DynamicBaseSize::new(dec!(10000));
        // 1.5% of 10000 = 150
        assert_eq!(dbs.current_base_size(), dec!(150));
    }

    #[test]
    fn base_size_clamped_at_max() {
        let dbs = DynamicBaseSize::new(dec!(100000));
        // 1.5% of 100000 = 1500, clamped to 500
        assert_eq!(dbs.current_base_size(), dec!(500));
    }

    #[test]
    fn base_size_clamped_at_min() {
        let dbs = DynamicBaseSize::new(dec!(100));
        // 1.5% of 100 = 1.5, clamped to 5
        assert_eq!(dbs.current_base_size(), dec!(5));
    }

    #[test]
    fn update_balance() {
        let dbs = DynamicBaseSize::new(dec!(5000));
        assert_eq!(dbs.current_base_size(), dec!(75)); // 1.5% of 5000
        dbs.update_balance(dec!(20000));
        assert_eq!(dbs.current_base_size(), dec!(300)); // 1.5% of 20000
    }

    #[test]
    fn current_balance() {
        let dbs = DynamicBaseSize::new(dec!(12345));
        let balance = dbs.current_balance();
        assert_eq!(balance, dec!(12345));
    }
}
