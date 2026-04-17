use polybot_common::errors::PolybotError;
use polybot_common::types::{Side, Trade};
use polymarket_client_sdk::auth::state::{Authenticated, Unauthenticated};
use polymarket_client_sdk::auth::{ExposeSecret, LocalSigner, Normal, Signer as _};
use polymarket_client_sdk::clob::types::request::OrderBookSummaryRequest;
use polymarket_client_sdk::clob::types::{OrderType as SdkOrderType, Side as SdkSide, SignatureType};
use polymarket_client_sdk::clob::{Client as SdkClobClient, Config as SdkClobConfig};
use polymarket_client_sdk::types::U256;
use polymarket_client_sdk::POLYGON;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::str::FromStr as _;
use std::sync::Arc;
use tokio::sync::RwLock;

use super::order_builder::Order;
use super::rate_limiter::ClobRateLimiter;

/// Configuration for the Polymarket CLOB client.
#[derive(Debug, Clone)]
pub struct ClobConfig {
    /// CLOB API endpoint (default: https://clob.polymarket.com)
    pub endpoint: String,
    /// WebSocket endpoint (default: wss://ws-subscriptions-clob.polymarket.com)
    pub ws_endpoint: String,
    /// Chain ID (137 = Polygon mainnet)
    pub chain_id: u64,
    /// Private key for signing (env var: POLYBOT_PRIVATE_KEY)
    pub private_key: String,
    /// API key credentials (derived from L1 auth)
    pub api_key: Option<ApiCredentials>,
    /// Signature type: 0=EOA, 1=POLY_PROXY, 2=GNOSIS_SAFE
    pub signature_type: u8,
    /// Funder address (for proxy/safe wallets)
    pub funder_address: Option<String>,
}

/// L2 API credentials derived from L1 authentication.
#[derive(Debug, Clone)]
pub struct ApiCredentials {
    pub api_key: String,
    pub secret: String,
    pub passphrase: String,
}

/// L2 API credentials alias.
pub type ApiKey = ApiCredentials;

/// CLOB order response from the Polymarket API.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ClobOrderResponse {
    pub success: bool,
    #[serde(default)]
    pub error_msg: String,
    #[serde(default)]
    pub order_id: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub taking_amount: String,
    #[serde(default)]
    pub making_amount: String,
}

/// Orderbook snapshot from the CLOB API.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct OrderBookSnapshot {
    pub market: String,
    pub asset_id: String,
    pub bids: Vec<OrderBookEntry>,
    pub asks: Vec<OrderBookEntry>,
    pub hash: String,
    pub timestamp: u64,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct OrderBookEntry {
    pub price: String,
    pub size: String,
}

/// Market info from the CLOB API.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct MarketInfo {
    pub condition_id: String,
    pub question_id: String,
    pub tokens: Vec<TokenInfo>,
    pub minimum_order_size: String,
    pub minimum_tick_size: String,
    #[serde(default)]
    pub neg_risk: bool,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct TokenInfo {
    pub token_id: String,
    pub outcome: String,
    pub price: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MarketContext {
    pub token_id: String,
    pub tick_size: Decimal,
    pub min_order_size: Decimal,
    pub neg_risk: bool,
}

impl MarketContext {
    pub fn simulation(token_id: impl Into<String>) -> Self {
        Self {
            token_id: token_id.into(),
            tick_size: dec!(0.01),
            min_order_size: dec!(1),
            neg_risk: false,
        }
    }
}

/// Polymarket CLOB client — wraps the official Rust SDK for order placement,
/// orderbook access, and authentication.
pub struct ClobClient {
    config: ClobConfig,
    rate_limiter: Arc<ClobRateLimiter>,
    http_client: reqwest::Client,
    public_client: SdkClobClient<Unauthenticated>,
    authenticated_client: RwLock<Option<SdkClobClient<Authenticated<Normal>>>>,
    /// Whether we have valid L2 credentials
    authenticated: RwLock<bool>,
}

impl ClobClient {
    /// Create a new CLOB client with the given configuration.
    pub fn new(config: ClobConfig) -> Self {
        let rate_limiter = Arc::new(ClobRateLimiter::new_with_limits(80, 240)); // 80% of 100/300
        let public_client = SdkClobClient::new(&config.endpoint, SdkClobConfig::default())
            .expect("Failed to create SDK CLOB client");
        Self {
            config,
            rate_limiter,
            http_client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(5))
                .build()
                .expect("Failed to create HTTP client"),
            public_client,
            authenticated_client: RwLock::new(None),
            authenticated: RwLock::new(false),
        }
    }

    /// Create a client from environment variables.
    pub fn from_env() -> Result<Self, PolybotError> {
        let private_key = std::env::var("POLYBOT_PRIVATE_KEY")
            .or_else(|_| std::env::var("POLYMARKET_PRIVATE_KEY"))
            .map_err(|_| PolybotError::Config("POLYBOT_PRIVATE_KEY not set".to_string()))?;

        let endpoint = std::env::var("POLYBOT_CLOB_ENDPOINT")
            .unwrap_or_else(|_| "https://clob.polymarket.com".to_string());

        let ws_endpoint = std::env::var("POLYBOT_WS_ENDPOINT")
            .unwrap_or_else(|_| "wss://ws-subscriptions-clob.polymarket.com".to_string());

        let signature_type = std::env::var("POLYBOT_SIGNATURE_TYPE")
            .ok()
            .and_then(|v| v.parse::<u8>().ok())
            .unwrap_or(0); // EOA by default

        let funder_address = std::env::var("POLYBOT_FUNDER_ADDRESS").ok();

        let config = ClobConfig {
            endpoint,
            ws_endpoint,
            chain_id: 137, // Polygon mainnet
            private_key,
            api_key: None,
            signature_type,
            funder_address,
        };

        Ok(Self::new(config))
    }

    /// Create a public, read-only client for market data requests.
    pub fn public_readonly() -> Self {
        let endpoint = std::env::var("POLYBOT_CLOB_ENDPOINT")
            .unwrap_or_else(|_| "https://clob.polymarket.com".to_string());
        let ws_endpoint = std::env::var("POLYBOT_WS_ENDPOINT")
            .unwrap_or_else(|_| "wss://ws-subscriptions-clob.polymarket.com".to_string());

        Self::new(ClobConfig {
            endpoint,
            ws_endpoint,
            chain_id: 137,
            private_key: String::new(),
            api_key: None,
            signature_type: 0,
            funder_address: None,
        })
    }

    /// Authenticate with the CLOB — derive L2 API credentials from L1 private key.
    /// This performs EIP-712 signing to create or derive API credentials.
    pub async fn authenticate(&self) -> Result<ApiCredentials, PolybotError> {
        tracing::info!("Authenticating with Polymarket CLOB (L1 -> L2)");

        let signer = self.local_signer()?;

        let mut builder = self.public_client.clone().authentication_builder(&signer);
        builder = builder.signature_type(self.sdk_signature_type());

        if let Some(funder) = self.config.funder_address.as_ref() {
            let funder = funder
                .parse()
                .map_err(|e| PolybotError::Execution(format!("Invalid funder address: {}", e)))?;
            builder = builder.funder(funder);
        }

        let client = builder
            .authenticate()
            .await
            .map_err(|e| PolybotError::Execution(format!("CLOB authentication failed: {}", e)))?;

        let credentials = client.credentials().clone();
        *self.authenticated_client.write().await = Some(client);
        *self.authenticated.write().await = true;
        Ok(ApiCredentials {
            api_key: credentials.key().to_string(),
            secret: credentials.secret().expose_secret().to_string(),
            passphrase: credentials.passphrase().expose_secret().to_string(),
        })
    }

    fn sdk_signature_type(&self) -> SignatureType {
        match self.config.signature_type {
            1 => SignatureType::Proxy,
            2 => SignatureType::GnosisSafe,
            _ => SignatureType::Eoa,
        }
    }

    fn local_signer(&self) -> Result<impl polymarket_client_sdk::auth::Signer, PolybotError> {
        if self.config.private_key.is_empty() {
            return Err(PolybotError::Config(
                "No private key configured for live trading".to_string(),
            ));
        }

        LocalSigner::from_str(&self.config.private_key)
            .map(|signer| signer.with_chain_id(Some(POLYGON)))
            .map_err(|e| PolybotError::Config(format!("Invalid private key: {}", e)))
    }

    /// Submit a signed order to the CLOB.
    /// This is the main entry point for live trading.
    pub async fn submit_order(&self, order: &Order) -> Result<Trade, PolybotError> {
        // Check rate limiter
        if !self.rate_limiter.check_write().await {
            return Err(PolybotError::Execution(
                "CLOB rate limit circuit breaker is open — too many requests".to_string(),
            ));
        }
        self.rate_limiter.record_write().await;

        // In production with the SDK:
        // 1. Create limit order via client.limit_order()
        // 2. Sign with the signer
        // 3. Post order via client.post_order()
        // 4. Track order status

        tracing::info!(
            signal_id = %order.signal_id,
            order_type = ?order.order_type,
            size_usd = %order.size_usd,
            "Submitting order to CLOB"
        );

        if !*self.authenticated.read().await {
            self.authenticate().await?;
        }

        let client = self
            .authenticated_client
            .read()
            .await
            .as_ref()
            .cloned()
            .ok_or_else(|| PolybotError::Execution("Authenticated CLOB client unavailable".to_string()))?;
        let signer = self.local_signer()?;
        let token_id = U256::from_str(&order.token_id)
            .map_err(|e| PolybotError::Execution(format!("Invalid token id for order: {}", e)))?;

        let sdk_order_type = match order.order_type {
            polybot_common::types::OrderType::Fok => SdkOrderType::FOK,
            polybot_common::types::OrderType::Ioc => SdkOrderType::FAK,
            _ => SdkOrderType::GTC,
        };

        let signable_order = client
            .limit_order()
            .token_id(token_id)
            .side(SdkSide::Buy)
            .price(order.price)
            .size(order.size)
            .order_type(sdk_order_type)
            .post_only(matches!(order.order_type, polybot_common::types::OrderType::PostOnly))
            .build()
            .await
            .map_err(|e| PolybotError::Execution(format!("Failed to build CLOB order: {}", e)))?;

        let signed_order = client
            .sign(&signer, signable_order)
            .await
            .map_err(|e| PolybotError::Execution(format!("Failed to sign CLOB order: {}", e)))?;
        let response = client
            .post_order(signed_order)
            .await
            .map_err(|e| PolybotError::Execution(format!("Failed to submit CLOB order: {}", e)))?;

        let status = if !response.success {
            polybot_common::types::TradeStatus::Failed(
                response
                    .error_msg
                    .clone()
                    .unwrap_or_else(|| "Order rejected by CLOB".to_string()),
            )
        } else {
            match response.status {
                polymarket_client_sdk::clob::types::OrderStatusType::Matched => {
                    polybot_common::types::TradeStatus::Filled
                }
                polymarket_client_sdk::clob::types::OrderStatusType::Live
                | polymarket_client_sdk::clob::types::OrderStatusType::Delayed => {
                    polybot_common::types::TradeStatus::Pending
                }
                polymarket_client_sdk::clob::types::OrderStatusType::Canceled => {
                    polybot_common::types::TradeStatus::Cancelled
                }
                polymarket_client_sdk::clob::types::OrderStatusType::Unmatched => {
                    polybot_common::types::TradeStatus::TimedOut
                }
                polymarket_client_sdk::clob::types::OrderStatusType::Unknown(raw) => {
                    polybot_common::types::TradeStatus::Failed(format!("Unknown CLOB order status: {}", raw))
                }
                _ => polybot_common::types::TradeStatus::Pending,
            }
        };

        let filled_size = if matches!(status, polybot_common::types::TradeStatus::Filled) {
            order.size
        } else {
            Decimal::ZERO
        };

        Ok(Trade {
            id: response.order_id,
            signal_id: order.signal_id.clone(),
            market_id: order.market_id.clone(),
            category: order.category,
            side: order.side,
            price: order.price,
            size: order.size,
            size_usd: order.size_usd,
            filled_size,
            order_type: order.order_type,
            status,
            placed_at: chrono::Utc::now(),
            filled_at: None,
            simulated: false,
        })
    }

    /// Fetch the orderbook for a given market (token_id).
    pub async fn get_orderbook(&self, token_id: &str) -> Result<OrderBookSnapshot, PolybotError> {
        if !self.rate_limiter.check_read().await {
            return Err(PolybotError::Execution(
                "CLOB read rate limit circuit breaker open".to_string(),
            ));
        }
        self.rate_limiter.record_read().await;

        let token_id = U256::from_str(token_id)
            .map_err(|e| PolybotError::Execution(format!("Invalid token id: {}", e)))?;
        let request = OrderBookSummaryRequest::builder().token_id(token_id).build();
        let book = self
            .public_client
            .order_book(&request)
            .await
            .map_err(|e| PolybotError::Execution(format!("Orderbook fetch failed: {}", e)))?;

        Ok(OrderBookSnapshot {
            market: book.market.to_string(),
            asset_id: book.asset_id.to_string(),
            bids: book
                .bids
                .into_iter()
                .map(|entry| OrderBookEntry {
                    price: entry.price.to_string(),
                    size: entry.size.to_string(),
                })
                .collect(),
            asks: book
                .asks
                .into_iter()
                .map(|entry| OrderBookEntry {
                    price: entry.price.to_string(),
                    size: entry.size.to_string(),
                })
                .collect(),
            hash: book.hash.unwrap_or_default(),
            timestamp: book.timestamp.timestamp_millis() as u64,
        })
    }

    /// Get market info for a given condition_id.
    pub async fn get_market(&self, condition_id: &str) -> Result<MarketInfo, PolybotError> {
        let market = self
            .public_client
            .market(condition_id)
            .await
            .map_err(|e| PolybotError::Execution(format!("Market fetch failed: {}", e)))?;

        Ok(MarketInfo {
            condition_id: market.condition_id.map(|value| value.to_string()).unwrap_or_default(),
            question_id: market.question_id.map(|value| value.to_string()).unwrap_or_default(),
            tokens: market
                .tokens
                .into_iter()
                .map(|token| TokenInfo {
                    token_id: token.token_id.to_string(),
                    outcome: token.outcome,
                    price: token.price.to_string(),
                })
                .collect(),
            minimum_order_size: market.minimum_order_size.to_string(),
            minimum_tick_size: market.minimum_tick_size.to_string(),
            neg_risk: market.neg_risk,
        })
    }

    pub async fn resolve_token_id_for_signal(
        &self,
        market_id: &str,
        side: Side,
    ) -> Result<String, PolybotError> {
        if !Self::looks_like_condition_id(market_id) {
            if U256::from_str(market_id).is_ok() {
                return Ok(market_id.to_string());
            }
        }

        let market = self.get_market(market_id).await?;
        let outcome = match side {
            Side::Yes => "yes",
            Side::No => "no",
        };

        market
            .tokens
            .into_iter()
            .find(|token| token.outcome.eq_ignore_ascii_case(outcome))
            .map(|token| token.token_id)
            .ok_or_else(|| {
                PolybotError::Execution(format!(
                    "Unable to resolve token for outcome {} in market {}",
                    outcome, market_id
                ))
            })
    }

    pub fn market_context_from_market_info(
        market: &MarketInfo,
        side: Side,
    ) -> Result<MarketContext, PolybotError> {
        let outcome = match side {
            Side::Yes => "yes",
            Side::No => "no",
        };

        let token_id = market
            .tokens
            .iter()
            .find(|token| token.outcome.eq_ignore_ascii_case(outcome))
            .map(|token| token.token_id.clone())
            .ok_or_else(|| {
                PolybotError::Execution(format!(
                    "Unable to resolve token for outcome {} in market {}",
                    outcome, market.condition_id
                ))
            })?;

        let tick_size = Decimal::from_str(&market.minimum_tick_size).map_err(|e| {
            PolybotError::Execution(format!(
                "Invalid market tick size {}: {}",
                market.minimum_tick_size, e
            ))
        })?;
        let min_order_size = Decimal::from_str(&market.minimum_order_size).map_err(|e| {
            PolybotError::Execution(format!(
                "Invalid market minimum order size {}: {}",
                market.minimum_order_size, e
            ))
        })?;

        Ok(MarketContext {
            token_id,
            tick_size,
            min_order_size,
            neg_risk: market.neg_risk,
        })
    }

    pub async fn get_market_context_for_signal(
        &self,
        market_id: &str,
        side: Side,
    ) -> Result<MarketContext, PolybotError> {
        if !Self::looks_like_condition_id(market_id) && U256::from_str(market_id).is_ok() {
            return Ok(MarketContext::simulation(market_id.to_string()));
        }

        let market = self.get_market(market_id).await?;
        Self::market_context_from_market_info(&market, side)
    }

    pub async fn get_orderbook_for_signal(
        &self,
        market_id: &str,
        side: Side,
    ) -> Result<(String, OrderBookSnapshot), PolybotError> {
        let context = self.get_market_context_for_signal(market_id, side).await?;
        let book = self.get_orderbook(&context.token_id).await?;
        Ok((context.token_id, book))
    }

    fn looks_like_condition_id(market_id: &str) -> bool {
        market_id.starts_with("0x") && market_id.len() == 66
    }

    /// Calculate the midpoint price from an orderbook for slippage validation.
    pub fn calculate_midpoint(book: &OrderBookSnapshot) -> Option<Decimal> {
        let best_bid = book
            .bids
            .first()
            .and_then(|b| b.price.parse::<Decimal>().ok());
        let best_ask = book
            .asks
            .first()
            .and_then(|a| a.price.parse::<Decimal>().ok());

        match (best_bid, best_ask) {
            (Some(bid), Some(ask)) => Some((bid + ask) / dec!(2)),
            (Some(bid), None) => Some(bid),
            (None, Some(ask)) => Some(ask),
            (None, None) => None,
        }
    }

    pub fn estimate_fill_price(book: &OrderBookSnapshot) -> Option<Decimal> {
        book.asks
            .first()
            .and_then(|entry| entry.price.parse::<Decimal>().ok())
            .or_else(|| Self::calculate_midpoint(book))
    }

    pub fn visible_liquidity_usd(book: &OrderBookSnapshot) -> Decimal {
        book.bids
            .iter()
            .chain(book.asks.iter())
            .filter_map(|entry| {
                let price = entry.price.parse::<Decimal>().ok()?;
                let size = entry.size.parse::<Decimal>().ok()?;
                Some(price * size)
            })
            .fold(Decimal::ZERO, |acc, value| acc + value)
    }

    /// Check if slippage is within acceptable bounds (PRD: max 2% deviation).
    pub fn check_slippage(
        midpoint: Decimal,
        target_price: Decimal,
        max_deviation_pct: Decimal,
    ) -> bool {
        if midpoint == Decimal::ZERO {
            return false;
        }
        let deviation = ((target_price - midpoint).abs() / midpoint).abs();
        deviation <= max_deviation_pct
    }

    /// Send a heartbeat to keep the trading session alive.
    /// Per PRD: must send every 5 seconds, else all orders cancelled after 10s.
    pub async fn send_heartbeat(&self) -> Result<String, PolybotError> {
        let url = format!("{}/heartbeat", self.config.endpoint);
        // In production, this would include L2 auth headers
        let response = self
            .http_client
            .post(&url)
            .send()
            .await
            .map_err(|e| PolybotError::Execution(format!("Heartbeat failed: {}", e)))?;

        let body: serde_json::Value = response
            .json()
            .await
            .map_err(|e| PolybotError::Execution(format!("Heartbeat parse failed: {}", e)))?;

        body.get("heartbeat_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| PolybotError::Execution("No heartbeat_id in response".to_string()))
    }

    /// Get the current rate limiter stats
    pub async fn rate_limit_stats(&self) -> (u32, u32) {
        self.rate_limiter.get_stats().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> ClobConfig {
        ClobConfig {
            endpoint: "https://clob.polymarket.com".to_string(),
            ws_endpoint: "wss://ws-subscriptions-clob.polymarket.com".to_string(),
            chain_id: 137,
            private_key: "0x0000000000000000000000000000000000000000000000000000000000000001"
                .to_string(),
            api_key: None,
            signature_type: 0,
            funder_address: None,
        }
    }

    #[test]
    fn clob_client_creation() {
        let client = ClobClient::new(test_config());
        assert!(!client.config.endpoint.is_empty());
    }

    #[test]
    fn midpoint_calculation() {
        let book = OrderBookSnapshot {
            market: "test".to_string(),
            asset_id: "test-token".to_string(),
            bids: vec![OrderBookEntry {
                price: "0.60".to_string(),
                size: "100".to_string(),
            }],
            asks: vec![OrderBookEntry {
                price: "0.62".to_string(),
                size: "100".to_string(),
            }],
            hash: "abc".to_string(),
            timestamp: 0,
        };
        let mid = ClobClient::calculate_midpoint(&book);
        assert_eq!(mid, Some(dec!(0.61)));
    }

    #[test]
    fn midpoint_empty_book() {
        let book = OrderBookSnapshot {
            market: "test".to_string(),
            asset_id: "test-token".to_string(),
            bids: vec![],
            asks: vec![],
            hash: "abc".to_string(),
            timestamp: 0,
        };
        assert_eq!(ClobClient::calculate_midpoint(&book), None);
    }

    #[test]
    fn slippage_check_within_bounds() {
        let mid = dec!(0.60);
        let target = dec!(0.61);
        assert!(ClobClient::check_slippage(mid, target, dec!(0.02))); // 1.67% < 2%
    }

    #[test]
    fn slippage_check_exceeds_bounds() {
        let mid = dec!(0.60);
        let target = dec!(0.65);
        assert!(!ClobClient::check_slippage(mid, target, dec!(0.02))); // 8.3% > 2%
    }

    #[test]
    fn estimate_fill_price_uses_best_ask() {
        let book = OrderBookSnapshot {
            market: "test".to_string(),
            asset_id: "test-token".to_string(),
            bids: vec![OrderBookEntry {
                price: "0.58".to_string(),
                size: "50".to_string(),
            }],
            asks: vec![OrderBookEntry {
                price: "0.62".to_string(),
                size: "60".to_string(),
            }],
            hash: "abc".to_string(),
            timestamp: 0,
        };

        assert_eq!(ClobClient::estimate_fill_price(&book), Some(dec!(0.62)));
    }

    #[test]
    fn visible_liquidity_sums_book_depth() {
        let book = OrderBookSnapshot {
            market: "test".to_string(),
            asset_id: "test-token".to_string(),
            bids: vec![OrderBookEntry {
                price: "0.58".to_string(),
                size: "50".to_string(),
            }],
            asks: vec![OrderBookEntry {
                price: "0.62".to_string(),
                size: "60".to_string(),
            }],
            hash: "abc".to_string(),
            timestamp: 0,
        };

        assert_eq!(ClobClient::visible_liquidity_usd(&book), dec!(66.2));
    }

    #[test]
    fn from_env_missing_key() {
        // Clear the env var if set
        std::env::remove_var("POLYBOT_PRIVATE_KEY");
        let result = ClobClient::from_env();
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn rate_limiter_integration() {
        let client = ClobClient::new(test_config());
        let (writes, reads) = client.rate_limit_stats().await;
        assert_eq!(writes, 0);
        assert_eq!(reads, 0);
    }

    #[test]
    fn market_context_from_market_info_uses_market_metadata() {
        let market = MarketInfo {
            condition_id: "condition-1".to_string(),
            question_id: "question-1".to_string(),
            tokens: vec![
                TokenInfo {
                    token_id: "token-yes".to_string(),
                    outcome: "yes".to_string(),
                    price: "0.52".to_string(),
                },
                TokenInfo {
                    token_id: "token-no".to_string(),
                    outcome: "no".to_string(),
                    price: "0.48".to_string(),
                },
            ],
            minimum_order_size: "5".to_string(),
            minimum_tick_size: "0.001".to_string(),
            neg_risk: true,
        };

        let ctx = ClobClient::market_context_from_market_info(&market, Side::No).unwrap();
        assert_eq!(ctx.token_id, "token-no");
        assert_eq!(ctx.tick_size, dec!(0.001));
        assert_eq!(ctx.min_order_size, dec!(5));
        assert!(ctx.neg_risk);
    }
}
