pub mod clob_client;
pub mod clob_ws;
pub mod order_builder;
pub mod rate_limiter;
pub mod rpc_pool;

use polybot_common::constants::MIN_POSITION_USDC;
use polybot_common::errors::PolybotError;
use polybot_common::types::{Decision, RiskDecision, Trade};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};

use crate::config::AppConfig;
use crate::metrics::Metrics;
use crate::risk::limits;
use crate::telegram_bot::alerts::AlertBroadcaster;

pub async fn run_execution_engine(
    config: Arc<AppConfig>,
    metrics: Arc<Metrics>,
    alerts: Option<AlertBroadcaster>,
    market_prices: Arc<RwLock<HashMap<String, Decimal>>>,
    mut receiver: mpsc::Receiver<RiskDecision>,
    state_sender: mpsc::Sender<Trade>,
) -> Result<(), PolybotError> {
    let _rpc_pool = rpc_pool::RpcPool::new(&config.execution.rpc_endpoints);
    let market_data_client = clob_client::ClobClient::public_readonly();
    let ws_manager = Arc::new(clob_ws::ClobWsManager::new(
        clob_client::ClobConfig {
            endpoint: std::env::var("POLYBOT_CLOB_ENDPOINT")
                .unwrap_or_else(|_| "https://clob.polymarket.com".to_string()),
            ws_endpoint: std::env::var("POLYBOT_WS_ENDPOINT")
                .unwrap_or_else(|_| "wss://ws-subscriptions-clob.polymarket.com".to_string()),
            chain_id: 137,
            private_key: String::new(),
            api_key: None,
            signature_type: 0,
            funder_address: None,
        },
        metrics.clone(),
        alerts.clone(),
    ));
    let ws_task = ws_manager.clone();
    tokio::spawn(async move {
        if let Err(e) = ws_task.connect_with_backoff().await {
            tracing::error!(error = %e, "CLOB WebSocket manager stopped");
        }
    });

    // Initialize CLOB client if not in simulation mode
    let clob_client = if !config.system.simulation {
        match clob_client::ClobClient::from_env() {
            Ok(client) => {
                tracing::info!("CLOB client initialized — live trading mode");
                metrics.set_rpc_healthy(true);
                Some(client)
            }
            Err(e) => {
                tracing::error!(error = %e, "Failed to initialize CLOB client — falling back to simulation mode");
                None
            }
        }
    } else {
        tracing::info!("CLOB client skipped — simulation mode");
        None
    };

    while let Some(decision) = receiver.recv().await {
        match decision.decision {
            Decision::Execute => {
                tracing::info!(
                    signal_id = %decision.signal_id,
                    market_id = %decision.market_id,
                    side = ?decision.side,
                    size_usd = %decision.position_size_usd,
                    "Executing trade"
                );

                let mut target_price = dec!(0.50);
                let mut size_usd = decision.position_size_usd;
                let mut market_context =
                    clob_client::MarketContext::simulation(decision.market_id.clone());

                match market_data_client
                    .get_market_context_for_signal(&decision.market_id, decision.side)
                    .await
                {
                    Ok(context) => {
                        market_context = context;
                        ws_manager
                            .subscribe_token(market_context.token_id.clone())
                            .await;
                        let book = match ws_manager.get_cached_orderbook(&market_context.token_id).await {
                            Some(book) => book,
                            None => match market_data_client.get_orderbook(&market_context.token_id).await {
                                Ok(book) => book,
                                Err(e) => {
                                    if config.system.simulation {
                                        tracing::warn!(
                                            error = %e,
                                            signal_id = %decision.signal_id,
                                            market_id = %decision.market_id,
                                            token_id = %market_context.token_id,
                                            "Warm-book fallback unavailable in simulation; using default price"
                                        );
                                        clob_client::OrderBookSnapshot {
                                            market: decision.market_id.clone(),
                                            asset_id: market_context.token_id.clone(),
                                            bids: vec![],
                                            asks: vec![],
                                            hash: String::new(),
                                            timestamp: 0,
                                        }
                                    }

                                    else {
                                        metrics.record_trade_failed();
                                        if let Some(alerts) = &alerts {
                                            alerts.critical(format!(
                                                "Warm-book fallback failed for signal {} token {}: {}",
                                                decision.signal_id, market_context.token_id, e
                                            ));
                                        }
                                        tracing::error!(
                                            error = %e,
                                            signal_id = %decision.signal_id,
                                            token_id = %market_context.token_id,
                                            "Warm-book fallback orderbook fetch failed"
                                        );
                                        continue;
                                    }
                                }
                            },
                        };
                        let midpoint = clob_client::ClobClient::calculate_midpoint(&book)
                            .unwrap_or(target_price);
                        let estimated_fill = clob_client::ClobClient::estimate_fill_price(&book)
                            .unwrap_or(midpoint);
                        market_prices
                            .write()
                            .await
                            .insert(decision.market_id.clone(), midpoint);

                        if !clob_client::ClobClient::check_slippage(
                            midpoint,
                            estimated_fill,
                            config.execution.slippage_threshold,
                        ) {
                            metrics.record_trade_failed();
                            if let Some(alerts) = &alerts {
                                alerts.warning(format!(
                                    "Trade rejected by slippage guard for signal {} market {}",
                                    decision.signal_id, decision.market_id
                                ));
                            }
                            tracing::warn!(
                                signal_id = %decision.signal_id,
                                market_id = %decision.market_id,
                                midpoint = %midpoint,
                                estimated_fill = %estimated_fill,
                                "Trade rejected by slippage guard"
                            );
                            continue;
                        }

                        let visible_liquidity =
                            clob_client::ClobClient::visible_liquidity_usd(&book);
                        size_usd = limits::apply_market_liquidity_cap(size_usd, visible_liquidity);
                        target_price = estimated_fill;
                    }
                    Err(e) => {
                        if config.system.simulation {
                            tracing::warn!(
                                error = %e,
                                signal_id = %decision.signal_id,
                                market_id = %decision.market_id,
                                "Orderbook unavailable in simulation; using fallback price"
                            );
                            market_prices
                                .write()
                                .await
                                .insert(decision.market_id.clone(), target_price);
                        } else {
                            metrics.record_trade_failed();
                            if let Some(alerts) = &alerts {
                                alerts.critical(format!(
                                    "Orderbook fetch failed for signal {} market {}: {}",
                                    decision.signal_id, decision.market_id, e
                                ));
                            }
                            tracing::error!(
                                error = %e,
                                signal_id = %decision.signal_id,
                                market_id = %decision.market_id,
                                "Orderbook fetch failed"
                            );
                            continue;
                        }
                    }
                }

                if size_usd < MIN_POSITION_USDC {
                    metrics.record_trade_failed();
                    if let Some(alerts) = &alerts {
                        alerts.warning(format!(
                            "Trade rejected after liquidity cap for signal {} market {}",
                            decision.signal_id, decision.market_id
                        ));
                    }
                    tracing::warn!(
                        signal_id = %decision.signal_id,
                        market_id = %decision.market_id,
                        capped_size_usd = %size_usd,
                        "Trade rejected after liquidity cap fell below minimum position size"
                    );
                    continue;
                }

                let order =
                    order_builder::build_order(&decision, &market_context, target_price, size_usd);

                if config.system.simulation || clob_client.is_none() {
                    tracing::info!(signal_id = %decision.signal_id, "Simulation mode: creating simulated trade");
                    let trade = order_builder::create_simulated_trade(&decision, &order);
                    metrics.record_trade(true);
                    if let Some(alerts) = &alerts {
                        alerts.info(format!(
                            "Trade executed in simulation: signal={} market={} size_usd={} price={}",
                            decision.signal_id, decision.market_id, trade.size_usd, trade.price
                        ));
                    }
                    if state_sender.send(trade).await.is_err() {
                        tracing::error!("State channel closed");
                        return Err(PolybotError::ChannelClosed);
                    }
                } else if let Some(ref client) = clob_client {
                    match client.submit_order(&order).await {
                        Ok(trade) => {
                            metrics.record_trade(false);
                            if let Some(alerts) = &alerts {
                                alerts.info(format!(
                                    "Live trade executed: signal={} market={} size_usd={} price={}",
                                    decision.signal_id, decision.market_id, trade.size_usd, trade.price
                                ));
                            }
                            if state_sender.send(trade).await.is_err() {
                                tracing::error!("State channel closed");
                                return Err(PolybotError::ChannelClosed);
                            }
                        }
                        Err(e) => {
                            metrics.record_trade_failed();
                            if let Some(alerts) = &alerts {
                                alerts.critical(format!(
                                    "Order submission failed for signal {} market {}: {}",
                                    decision.signal_id, decision.market_id, e
                                ));
                            }
                            tracing::error!(error = %e, "Order submission failed");
                        }
                    }
                }
            }
            Decision::Skip(_reason) => {
                metrics.record_signal_skipped();
            }
            Decision::ManualReview => {
                metrics.record_signal_manual_review();
            }
            Decision::EmergencyStop => {
                metrics.record_emergency_stop();
            }
        }
    }

    Ok(())
}
