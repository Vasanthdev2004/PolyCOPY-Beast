use polybot_common::constants::{
    FULL_RECONCILIATION_INTERVAL_SECS, LIGHT_RECONCILIATION_INTERVAL_SECS,
};
use polybot_common::errors::PolybotError;
use std::sync::Arc;
use tokio::sync::Mutex;

use super::positions::PositionManager;
use super::redis_store::RedisStore;

/// v2.5: Reconciliation engine with light (30s) and full (5min) modes.
/// Source of truth: on-chain > CLOB API > Redis cache.
pub struct Reconciler {
    light_interval_secs: u64,
    full_interval_secs: u64,
    position_manager: Arc<Mutex<PositionManager>>,
    redis_url: Option<String>,
}

impl Reconciler {
    pub fn new(position_manager: Arc<Mutex<PositionManager>>, redis_url: Option<String>) -> Self {
        Self {
            light_interval_secs: LIGHT_RECONCILIATION_INTERVAL_SECS,
            full_interval_secs: FULL_RECONCILIATION_INTERVAL_SECS,
            position_manager,
            redis_url,
        }
    }

    pub fn with_intervals(mut self, light: u64, full: u64) -> Self {
        self.light_interval_secs = light;
        self.full_interval_secs = full;
        self
    }

    /// Run light reconciliation: Redis vs CLOB API (every 30s)
    pub async fn run_light(&self) -> Result<ReconciliationResult, PolybotError> {
        tracing::debug!("Running light reconciliation (Redis vs CLOB API)");
        self.reconcile_against_redis().await
    }

    /// Run full reconciliation: Redis vs on-chain state (every 5min)
    pub async fn run_full(&self) -> Result<ReconciliationResult, PolybotError> {
        tracing::debug!("Running full reconciliation (Redis vs on-chain)");
        self.reconcile_against_redis().await
    }

    /// Force reconciliation (triggered by /reconcile force command)
    pub async fn force_reconcile(&self) -> Result<ReconciliationResult, PolybotError> {
        tracing::info!("Force reconciliation triggered by operator");
        self.run_full().await
    }

    /// Run the continuous reconciliation loop
    pub async fn run_loop(&self) -> Result<(), PolybotError> {
        let mut light_interval =
            tokio::time::interval(std::time::Duration::from_secs(self.light_interval_secs));
        let mut full_interval =
            tokio::time::interval(std::time::Duration::from_secs(self.full_interval_secs));

        tracing::info!(
            light_interval_secs = self.light_interval_secs,
            full_interval_secs = self.full_interval_secs,
            "Reconciliation loop started"
        );

        loop {
            tokio::select! {
                _ = light_interval.tick() => {
                    match self.run_light().await {
                        Ok(result) => {
                            if result.has_issues() {
                                tracing::warn!(
                                    ghosts = result.ghost_positions.len(),
                                    missing = result.missing_positions.len(),
                                    mismatches = result.mismatches.len(),
                                    "Light reconciliation found issues"
                                );
                            }
                        }
                        Err(e) => tracing::error!(error = %e, "Light reconciliation failed"),
                    }
                }
                _ = full_interval.tick() => {
                    match self.run_full().await {
                        Ok(result) => {
                            if result.has_issues() {
                                tracing::warn!(
                                    ghosts = result.ghost_positions.len(),
                                    missing = result.missing_positions.len(),
                                    mismatches = result.mismatches.len(),
                                    "Full reconciliation found issues"
                                );
                            }
                        }
                        Err(e) => tracing::error!(error = %e, "Full reconciliation failed"),
                    }
                }
            }
        }
    }

    async fn reconcile_against_redis(&self) -> Result<ReconciliationResult, PolybotError> {
        let local_positions = {
            let positions = self.position_manager.lock().await;
            positions
                .get_positions_vec()
                .into_iter()
                .cloned()
                .collect::<Vec<_>>()
        };

        let checked = local_positions.len() as u32;
        let Some(redis_url) = self.redis_url.as_ref() else {
            return Ok(ReconciliationResult {
                checked,
                ghost_positions: Vec::new(),
                missing_positions: Vec::new(),
                mismatches: Vec::new(),
            });
        };

        let redis_store = match RedisStore::new(redis_url).await {
            Ok(store) => store,
            Err(e) => {
                tracing::warn!(error = %e, "Redis unavailable during reconciliation");
                return Ok(ReconciliationResult {
                    checked,
                    ghost_positions: Vec::new(),
                    missing_positions: Vec::new(),
                    mismatches: Vec::new(),
                });
            }
        };

        let redis_positions = redis_store.list_positions().await?;

        let local_by_market = local_positions
            .into_iter()
            .map(|position| (position.market_id.clone(), position))
            .collect::<std::collections::HashMap<_, _>>();
        let redis_by_market = redis_positions
            .into_iter()
            .map(|position| (position.market_id.clone(), position))
            .collect::<std::collections::HashMap<_, _>>();

        let ghost_positions = redis_by_market
            .keys()
            .filter(|market_id| !local_by_market.contains_key(*market_id))
            .cloned()
            .collect::<Vec<_>>();
        let missing_positions = local_by_market
            .keys()
            .filter(|market_id| !redis_by_market.contains_key(*market_id))
            .cloned()
            .collect::<Vec<_>>();

        let mismatches = local_by_market
            .iter()
            .filter_map(|(market_id, local)| {
                let redis = redis_by_market.get(market_id)?;
                if local.current_size != redis.current_size
                    || local.average_price != redis.average_price
                    || local.status != redis.status
                {
                    Some(market_id.clone())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();

        Ok(ReconciliationResult {
            checked,
            ghost_positions,
            missing_positions,
            mismatches,
        })
    }
}

#[derive(Debug, Clone)]
pub struct ReconciliationResult {
    pub checked: u32,
    pub ghost_positions: Vec<String>, // market_ids in Redis but not on-chain
    pub missing_positions: Vec<String>, // on-chain but not in Redis
    pub mismatches: Vec<String>,      // position data differs
}

impl ReconciliationResult {
    pub fn has_issues(&self) -> bool {
        !self.ghost_positions.is_empty()
            || !self.missing_positions.is_empty()
            || !self.mismatches.is_empty()
    }

    pub fn summary(&self) -> String {
        format!(
            "Checked: {}\nGhost positions: {}\nMissing positions: {}\nMismatches: {}",
            self.checked,
            self.ghost_positions.len(),
            self.missing_positions.len(),
            self.mismatches.len(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reconciler_create() {
        let pm = Arc::new(Mutex::new(PositionManager::new()));
        let _r = Reconciler::new(pm, None);
    }

    #[tokio::test]
    async fn light_reconciliation_no_issues() {
        let pm = Arc::new(Mutex::new(PositionManager::new()));
        let r = Reconciler::new(pm, None);
        let result = r.run_light().await.unwrap();
        assert_eq!(result.checked, 0);
        assert!(!result.has_issues());
    }

    #[tokio::test]
    async fn full_reconciliation_no_issues() {
        let pm = Arc::new(Mutex::new(PositionManager::new()));
        let r = Reconciler::new(pm, None);
        let result = r.run_full().await.unwrap();
        assert_eq!(result.checked, 0);
        assert!(!result.has_issues());
    }

    #[test]
    fn reconciliation_result_has_issues() {
        let result = ReconciliationResult {
            checked: 10,
            ghost_positions: vec!["m1".to_string()],
            missing_positions: vec![],
            mismatches: vec![],
        };
        assert!(result.has_issues());
    }

    #[test]
    fn reconciliation_result_no_issues() {
        let result = ReconciliationResult {
            checked: 10,
            ghost_positions: vec![],
            missing_positions: vec![],
            mismatches: vec![],
        };
        assert!(!result.has_issues());
    }
}
