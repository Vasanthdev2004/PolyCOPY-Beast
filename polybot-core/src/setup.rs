use std::time::Duration;

use polybot_common::errors::PolybotError;
use polybot_common::types::ExecutionMode;
use serde::Deserialize;

use crate::config::AppConfig;
use crate::execution::clob_client::{ClobClient, WalletMode};

const POLYGON_MAINNET_CHAIN_ID: u64 = 137;

#[derive(Debug, Clone)]
pub struct StartupPreflightReport {
    pub execution_mode: ExecutionMode,
    pub verified_rpc_endpoint: String,
    pub wallet_mode: Option<WalletMode>,
    pub approvals_ready: Option<bool>,
}

impl StartupPreflightReport {
    pub fn summary(&self) -> String {
        match (self.wallet_mode, self.approvals_ready) {
            (Some(wallet_mode), Some(approvals_ready)) => format!(
                "mode={:?} rpc={} wallet_mode={} approvals_ready={}",
                self.execution_mode, self.verified_rpc_endpoint, wallet_mode, approvals_ready
            ),
            _ => format!(
                "mode={:?} rpc={} simulation_preflight=true",
                self.execution_mode, self.verified_rpc_endpoint
            ),
        }
    }
}

#[derive(Debug, Deserialize)]
struct RpcChainIdResponse {
    result: Option<String>,
}

pub async fn run_startup_preflight(
    config: &AppConfig,
) -> Result<StartupPreflightReport, PolybotError> {
    let verified_rpc_endpoint = validate_rpc_connectivity(&config.execution.rpc_endpoints).await?;

    let mut report = StartupPreflightReport {
        execution_mode: config.system.execution_mode,
        verified_rpc_endpoint,
        wallet_mode: None,
        approvals_ready: None,
    };

    if matches!(config.system.execution_mode, ExecutionMode::Simulation) {
        return Ok(report);
    }

    let client = ClobClient::from_env()?;
    let wallet_mode = client.validate_wallet_mode()?;
    let _credentials = client.authenticate().await?;
    let approvals = client.check_approvals().await?;

    report.wallet_mode = Some(wallet_mode);
    report.approvals_ready = Some(approvals.ready_for_live_trading);

    if !approvals.ready_for_live_trading {
        return Err(PolybotError::Config(approvals.guidance_message()));
    }

    Ok(report)
}

async fn validate_rpc_connectivity(endpoints: &[String]) -> Result<String, PolybotError> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .map_err(|e| PolybotError::Config(format!("Failed to build RPC validation client: {}", e)))?;

    let mut failures = Vec::new();

    for endpoint in endpoints {
        match validate_rpc_endpoint(&client, endpoint).await {
            Ok(()) => return Ok(endpoint.clone()),
            Err(error) => failures.push(format!("{} ({})", endpoint, error)),
        }
    }

    Err(PolybotError::Config(format!(
        "Polygon RPC connectivity validation failed for all configured endpoints: {}",
        failures.join("; ")
    )))
}

async fn validate_rpc_endpoint(client: &reqwest::Client, endpoint: &str) -> Result<(), String> {
    let response = client
        .post(endpoint)
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "method": "eth_chainId",
            "params": [],
            "id": 1
        }))
        .send()
        .await
        .map_err(|e| format!("request failed: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("unexpected HTTP status {}", response.status()));
    }

    let payload: RpcChainIdResponse = response
        .json()
        .await
        .map_err(|e| format!("invalid JSON-RPC response: {}", e))?;

    let chain_id_hex = payload
        .result
        .ok_or_else(|| "missing eth_chainId result".to_string())?;
    let chain_id = parse_chain_id_hex(&chain_id_hex).map_err(|e| e.to_string())?;

    if chain_id != POLYGON_MAINNET_CHAIN_ID {
        return Err(format!(
            "expected Polygon mainnet chain id 137, got {}",
            chain_id
        ));
    }

    Ok(())
}

fn parse_chain_id_hex(value: &str) -> Result<u64, PolybotError> {
    let trimmed = value.trim();
    let raw = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
        .ok_or_else(|| PolybotError::Config(format!("Invalid hex chain id: {}", value)))?;

    u64::from_str_radix(raw, 16)
        .map_err(|e| PolybotError::Config(format!("Invalid hex chain id {}: {}", value, e)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_chain_id_hex_accepts_polygon() {
        assert_eq!(parse_chain_id_hex("0x89").unwrap(), 137);
    }

    #[test]
    fn parse_chain_id_hex_rejects_invalid_values() {
        assert!(parse_chain_id_hex("137").is_err());
        assert!(parse_chain_id_hex("0xzz").is_err());
    }
}
