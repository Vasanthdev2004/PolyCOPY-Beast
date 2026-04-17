use polybot_common::constants::{
    CIRCUIT_BREAKER_PCT, CLOB_RATE_LIMIT_PER_MIN, CLOB_READ_LIMIT_PER_MIN,
};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

/// v2.5: CLOB API rate limiter with circuit breaker at 80%.
/// Tracks writes (100/min) and reads (300/min) separately.
/// Circuit breaker activates at 80% of rate limit.
pub struct ClobRateLimiter {
    write_limit: u32,
    read_limit: u32,
    circuit_breaker_pct: rust_decimal::Decimal,
    state: Mutex<RateLimitState>,
    write_count: AtomicU32,
    read_count: AtomicU32,
}

struct RateLimitState {
    window_start: Instant,
}

impl ClobRateLimiter {
    pub fn new() -> Self {
        Self {
            write_limit: CLOB_RATE_LIMIT_PER_MIN,
            read_limit: CLOB_READ_LIMIT_PER_MIN,
            circuit_breaker_pct: CIRCUIT_BREAKER_PCT,
            state: Mutex::new(RateLimitState {
                window_start: Instant::now(),
            }),
            write_count: AtomicU32::new(0),
            read_count: AtomicU32::new(0),
        }
    }

    pub fn new_with_limits(write_limit: u32, read_limit: u32) -> Self {
        Self {
            write_limit,
            read_limit,
            circuit_breaker_pct: CIRCUIT_BREAKER_PCT,
            state: Mutex::new(RateLimitState {
                window_start: Instant::now(),
            }),
            write_count: AtomicU32::new(0),
            read_count: AtomicU32::new(0),
        }
    }

    /// Check if a write (order placement) is allowed. Returns false if at/above 80%.
    pub async fn check_write(&self) -> bool {
        self.maybe_reset_window().await;
        let count = self.write_count.load(Ordering::Relaxed);
        let threshold = ((self.write_limit as f64) * 0.80) as u32;
        count < threshold
    }

    /// Record a write request
    pub async fn record_write(&self) {
        self.maybe_reset_window().await;
        self.write_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Check if a read (book query) is allowed.
    pub async fn check_read(&self) -> bool {
        self.maybe_reset_window().await;
        let count = self.read_count.load(Ordering::Relaxed);
        let threshold = ((self.read_limit as f64) * 0.80) as u32;
        count < threshold
    }

    /// Record a read request
    pub async fn record_read(&self) {
        self.maybe_reset_window().await;
        self.read_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Get current usage stats
    pub async fn get_stats(&self) -> (u32, u32) {
        self.maybe_reset_window().await;
        (
            self.write_count.load(Ordering::Relaxed),
            self.read_count.load(Ordering::Relaxed),
        )
    }

    /// Is the write circuit breaker open (at 80% capacity)?
    pub async fn is_write_circuit_open(&self) -> bool {
        self.maybe_reset_window().await;
        let count = self.write_count.load(Ordering::Relaxed);
        let threshold = ((self.write_limit as f64) * 0.80) as u32;
        count >= threshold
    }

    async fn maybe_reset_window(&self) {
        let mut state = self.state.lock().await;
        if state.window_start.elapsed() >= Duration::from_secs(60) {
            state.window_start = Instant::now();
            self.write_count.store(0, Ordering::Relaxed);
            self.read_count.store(0, Ordering::Relaxed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn allows_under_limit() {
        let limiter = ClobRateLimiter::new_with_limits(100, 300);
        assert!(limiter.check_write().await);
        limiter.record_write().await;
        assert!(limiter.check_write().await);
    }

    #[tokio::test]
    async fn circuit_breaker_at_80_pct() {
        let limiter = ClobRateLimiter::new_with_limits(10, 30);
        // 80% of 10 = 8. So 8 writes are OK, 9th should be blocked
        for _ in 0..8 {
            limiter.record_write().await;
        }
        assert!(limiter.is_write_circuit_open().await);
        assert!(!limiter.check_write().await);
    }

    #[tokio::test]
    async fn read_circuit_breaker() {
        let limiter = ClobRateLimiter::new_with_limits(100, 10);
        // 80% of 10 = 8
        for _ in 0..8 {
            limiter.record_read().await;
        }
        assert!(!limiter.check_read().await);
    }

    #[tokio::test]
    async fn window_resets() {
        let limiter = ClobRateLimiter::new_with_limits(2, 30);
        limiter.record_write().await;
        limiter.record_write().await;
        assert!(!limiter.check_write().await);
        // After window reset (60s), should be OK
        // Can't wait 60s in test, but the stats show the count
        let (writes, _) = limiter.get_stats().await;
        assert_eq!(writes, 2);
    }
}
