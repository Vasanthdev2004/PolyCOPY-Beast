use std::time::Duration;

use polymarket_client_sdk::error::{Error as SdkError, Status, StatusCode};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryClass {
    Retryable,
    NonRetryable,
}

impl RetryClass {
    pub fn from_status(status: StatusCode) -> Self {
        if status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error() {
            Self::Retryable
        } else {
            Self::NonRetryable
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct RetryPolicy {
    pub max_retries: u32,
    pub base_delay_ms: u64,
    pub max_delay_ms: u64,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: 3,
            base_delay_ms: 1_000,
            max_delay_ms: 60_000,
        }
    }
}

impl RetryPolicy {
    pub fn should_retry(&self, attempt: u32, retry_class: RetryClass) -> bool {
        matches!(retry_class, RetryClass::Retryable) && attempt < self.max_retries
    }

    pub fn backoff_delay(&self, attempt: u32) -> Duration {
        let multiplier = 2u64.saturating_pow(attempt);
        let delay_ms = self
            .base_delay_ms
            .saturating_mul(multiplier)
            .min(self.max_delay_ms);
        Duration::from_millis(delay_ms)
    }
}

pub fn classify_sdk_error(error: &SdkError) -> RetryClass {
    error
        .downcast_ref::<Status>()
        .map(|status| RetryClass::from_status(status.status_code))
        .unwrap_or(RetryClass::NonRetryable)
}

#[cfg(test)]
mod tests {
    use super::*;
    use polymarket_client_sdk::error::Method;

    #[test]
    fn retry_policy_caps_after_three_attempts() {
        let policy = RetryPolicy {
            max_retries: 3,
            base_delay_ms: 1_000,
            max_delay_ms: 4_000,
        };

        assert_eq!(policy.backoff_delay(0), Duration::from_secs(1));
        assert_eq!(policy.backoff_delay(1), Duration::from_secs(2));
        assert_eq!(policy.backoff_delay(2), Duration::from_secs(4));
        assert_eq!(policy.backoff_delay(3), Duration::from_secs(4));
        assert!(policy.should_retry(2, RetryClass::Retryable));
        assert!(!policy.should_retry(3, RetryClass::Retryable));
    }

    #[test]
    fn retryable_server_error_is_classified_as_retryable() {
        let error = SdkError::status(
            StatusCode::SERVICE_UNAVAILABLE,
            Method::POST,
            "/order".to_string(),
            "temporarily unavailable",
        );

        assert_eq!(classify_sdk_error(&error), RetryClass::Retryable);
    }

    #[test]
    fn rate_limit_error_is_classified_as_retryable() {
        let error = SdkError::status(
            StatusCode::TOO_MANY_REQUESTS,
            Method::POST,
            "/order".to_string(),
            "rate limited",
        );

        assert_eq!(classify_sdk_error(&error), RetryClass::Retryable);
    }

    #[test]
    fn client_error_is_classified_as_non_retryable() {
        let error = SdkError::status(
            StatusCode::BAD_REQUEST,
            Method::POST,
            "/order".to_string(),
            "invalid order",
        );

        assert_eq!(classify_sdk_error(&error), RetryClass::NonRetryable);
    }
}
