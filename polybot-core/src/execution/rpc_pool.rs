use polybot_common::errors::PolybotError;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

const MAX_CONSECUTIVE_FAILURES: usize = 3;
const CIRCUIT_OPEN_DURATION_SECS: u64 = 30;

#[derive(Debug, Clone)]
struct EndpointState {
    url: String,
    consecutive_failures: Arc<AtomicUsize>,
    circuit_open_until: Arc<Mutex<Option<tokio::time::Instant>>>,
}

pub struct RpcPool {
    endpoints: Vec<EndpointState>,
    current: Arc<AtomicUsize>,
}

impl RpcPool {
    pub fn new(urls: &[String]) -> Self {
        let endpoints = urls
            .iter()
            .map(|url| EndpointState {
                url: url.clone(),
                consecutive_failures: Arc::new(AtomicUsize::new(0)),
                circuit_open_until: Arc::new(Mutex::new(None)),
            })
            .collect();

        Self {
            endpoints,
            current: Arc::new(AtomicUsize::new(0)),
        }
    }

    pub async fn get_endpoint(&self) -> Result<String, PolybotError> {
        let len = self.endpoints.len();
        if len == 0 {
            return Err(PolybotError::RpcPool(
                "No RPC endpoints configured".to_string(),
            ));
        }

        for _ in 0..len {
            let idx = self.current.fetch_add(1, Ordering::Relaxed) % len;
            let ep = &self.endpoints[idx];

            let is_open = {
                let open_until = ep.circuit_open_until.lock().await;
                if let Some(until) = *open_until {
                    tokio::time::Instant::now() < until
                } else {
                    false
                }
            };

            if !is_open {
                return Ok(ep.url.clone());
            }
        }

        Err(PolybotError::RpcPool(
            "All RPC endpoints have open circuits".to_string(),
        ))
    }

    pub async fn report_success(&self, url: &str) {
        if let Some(ep) = self.endpoints.iter().find(|e| e.url == url) {
            ep.consecutive_failures.store(0, Ordering::Relaxed);
            let mut open_until = ep.circuit_open_until.lock().await;
            *open_until = None;
        }
    }

    pub async fn report_failure(&self, url: &str) {
        if let Some(ep) = self.endpoints.iter().find(|e| e.url == url) {
            let failures = ep.consecutive_failures.fetch_add(1, Ordering::Relaxed) + 1;
            if failures >= MAX_CONSECUTIVE_FAILURES {
                let mut open_until = ep.circuit_open_until.lock().await;
                *open_until = Some(
                    tokio::time::Instant::now() + Duration::from_secs(CIRCUIT_OPEN_DURATION_SECS),
                );
                tracing::warn!(url = %url, "Circuit breaker opened for RPC endpoint");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pool_returns_endpoint() {
        let pool = RpcPool::new(&[
            "https://rpc1.example.com".to_string(),
            "https://rpc2.example.com".to_string(),
        ]);
        // Should return one of the endpoints
        let rt = tokio::runtime::Runtime::new().unwrap();
        let url = rt.block_on(pool.get_endpoint()).unwrap();
        assert!(url.starts_with("https://rpc"));
    }

    #[tokio::test]
    async fn empty_pool_returns_error() {
        let pool = RpcPool::new(&[]);
        let result = pool.get_endpoint().await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn report_success_resets_failures() {
        let pool = RpcPool::new(&["https://rpc1.example.com".to_string()]);
        pool.report_failure("https://rpc1.example.com").await;
        pool.report_success("https://rpc1.example.com").await;
        assert_eq!(
            pool.endpoints[0]
                .consecutive_failures
                .load(Ordering::Relaxed),
            0
        );
    }
}
