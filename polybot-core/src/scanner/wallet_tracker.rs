use std::collections::{BTreeSet, HashMap};

use polybot_common::types::{Category, SignalSource};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

#[derive(Debug, Clone)]
pub struct WalletPollTrigger {
    pub wallet: String,
    pub source: SignalSource,
}

#[derive(Debug, Default, Clone)]
pub struct WalletActivityState {
    last_seen_by_wallet: HashMap<String, i64>,
    wallets_by_asset: HashMap<String, BTreeSet<String>>,
}

impl WalletActivityState {
    pub fn record_activity(&mut self, wallet: &str, asset_id: Option<&str>, timestamp: i64) {
        let normalized_wallet = wallet.to_lowercase();
        self.last_seen_by_wallet
            .entry(normalized_wallet.clone())
            .and_modify(|seen| *seen = (*seen).max(timestamp))
            .or_insert(timestamp);

        if let Some(asset_id) = asset_id.filter(|value| !value.is_empty()) {
            self.wallets_by_asset
                .entry(asset_id.to_string())
                .or_default()
                .insert(normalized_wallet);
        }
    }

    pub fn last_seen_for_wallet(&self, wallet: &str) -> Option<i64> {
        self.last_seen_by_wallet
            .get(&wallet.to_lowercase())
            .copied()
    }

    pub fn wallets_for_asset(&self, asset_id: &str) -> Vec<String> {
        self.wallets_by_asset
            .get(asset_id)
            .map(|wallets| wallets.iter().cloned().collect())
            .unwrap_or_default()
    }

    pub fn tracked_assets(&self) -> Vec<String> {
        self.wallets_by_asset.keys().cloned().collect()
    }

    pub fn retain_wallets(&mut self, wallets: &[String]) {
        let allowed = wallets
            .iter()
            .map(|wallet| wallet.to_lowercase())
            .collect::<BTreeSet<_>>();

        self.last_seen_by_wallet
            .retain(|wallet, _| allowed.contains(wallet));
        self.wallets_by_asset.retain(|_, watching_wallets| {
            watching_wallets.retain(|wallet| allowed.contains(wallet));
            !watching_wallets.is_empty()
        });
    }
}

pub fn category_allowed(category: Category, allowed: &[Category]) -> bool {
    allowed.is_empty() || allowed.contains(&category)
}

pub fn baseline_wallet_score(total_value_usdc: Decimal, recent_trade_count: usize) -> Decimal {
    let capped_value_component = (total_value_usdc / dec!(100)).min(dec!(80));
    let capped_activity_component = Decimal::from(recent_trade_count.min(10) as u64) * dec!(2);
    (capped_value_component + capped_activity_component).min(dec!(100))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wallet_activity_state_tracks_asset_watchers() {
        let mut state = WalletActivityState::default();
        state.record_activity("0xabc", Some("asset-1"), 10);
        state.record_activity("0xdef", Some("asset-1"), 12);

        let wallets = state.wallets_for_asset("asset-1");
        assert_eq!(wallets.len(), 2);
        assert!(wallets.contains(&"0xabc".to_string()));
        assert!(wallets.contains(&"0xdef".to_string()));
        assert_eq!(state.last_seen_for_wallet("0xabc"), Some(10));
    }

    #[test]
    fn baseline_wallet_score_increases_with_value_and_activity() {
        let low = baseline_wallet_score(dec!(100), 1);
        let high = baseline_wallet_score(dec!(1000), 5);

        assert!(high > low);
    }
}
