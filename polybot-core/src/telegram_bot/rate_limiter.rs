use std::collections::HashMap;
use std::sync::Mutex;
use std::time::SystemTime;

/// v2.5: Command rate limiting per user.
/// Max 30 commands/min per user. Max 3 /emergency_stop per hour.
pub struct CommandRateLimiter {
    max_per_min: u32,
    emergency_max_per_hour: u32,
    state: Mutex<HashMap<u64, UserRateState>>,
}

struct UserRateState {
    minute_bucket: Vec<u64>,         // epoch millis
    emergency_stop_bucket: Vec<u64>, // epoch millis
}

fn epoch_millis() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

impl CommandRateLimiter {
    pub fn new(max_per_min: u32, emergency_max_per_hour: u32) -> Self {
        Self {
            max_per_min,
            emergency_max_per_hour,
            state: Mutex::new(HashMap::new()),
        }
    }

    /// Check if a regular command is allowed. Returns false if rate limited.
    /// Also records the command if allowed.
    pub fn check_command(&self, user_id: u64) -> bool {
        let mut state = self.state.lock().unwrap();
        let user = state.entry(user_id).or_insert_with(|| UserRateState {
            minute_bucket: Vec::new(),
            emergency_stop_bucket: Vec::new(),
        });

        let now = epoch_millis();
        let one_min_ago = now.saturating_sub(60_000);

        // Clean old entries
        user.minute_bucket.retain(|t| *t > one_min_ago);

        if user.minute_bucket.len() >= self.max_per_min as usize {
            return false;
        }

        user.minute_bucket.push(now);
        true
    }

    /// Check if /emergency_stop is allowed. Also records if allowed.
    pub fn check_emergency_stop(&self, user_id: u64) -> bool {
        let mut state = self.state.lock().unwrap();
        let user = state.entry(user_id).or_insert_with(|| UserRateState {
            minute_bucket: Vec::new(),
            emergency_stop_bucket: Vec::new(),
        });

        let now = epoch_millis();
        let one_hour_ago = now.saturating_sub(3_600_000);

        user.emergency_stop_bucket.retain(|t| *t > one_hour_ago);

        if user.emergency_stop_bucket.len() >= self.emergency_max_per_hour as usize {
            return false;
        }

        user.emergency_stop_bucket.push(now);
        true
    }

    /// Get current rate limit stats for a user (commands this minute, emergency stops this hour)
    pub fn get_stats(&self, user_id: u64) -> (u32, u32) {
        let mut state = self.state.lock().unwrap();
        let user = state.entry(user_id).or_insert_with(|| UserRateState {
            minute_bucket: Vec::new(),
            emergency_stop_bucket: Vec::new(),
        });

        let now = epoch_millis();
        let one_min_ago = now.saturating_sub(60_000);
        let one_hour_ago = now.saturating_sub(3_600_000);

        user.minute_bucket.retain(|t| *t > one_min_ago);
        user.emergency_stop_bucket.retain(|t| *t > one_hour_ago);

        (
            user.minute_bucket.len() as u32,
            user.emergency_stop_bucket.len() as u32,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_under_limit() {
        let limiter = CommandRateLimiter::new(30, 3);
        assert!(limiter.check_command(123));
    }

    #[test]
    fn blocks_over_minute_limit() {
        let limiter = CommandRateLimiter::new(2, 3);
        assert!(limiter.check_command(123));
        assert!(limiter.check_command(123));
        assert!(!limiter.check_command(123)); // 3rd blocked
    }

    #[test]
    fn emergency_stop_hourly_limit() {
        let limiter = CommandRateLimiter::new(30, 2);
        assert!(limiter.check_emergency_stop(123));
        assert!(limiter.check_emergency_stop(123));
        assert!(!limiter.check_emergency_stop(123)); // 3rd blocked
    }

    #[test]
    fn get_stats() {
        let limiter = CommandRateLimiter::new(30, 3);
        limiter.check_command(123);
        limiter.check_command(123);
        let (cmds, es) = limiter.get_stats(123);
        assert_eq!(cmds, 2);
        assert_eq!(es, 0);
    }
}
