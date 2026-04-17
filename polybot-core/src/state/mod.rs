pub mod pnl;
pub mod positions;
pub mod reconciliation;
pub mod redis_backup;
pub mod redis_store;
pub mod sqlite;

use polybot_common::errors::PolybotError;
use polybot_common::types::{Category, OrderType, PositionKey, Side, Trade, TradeStatus};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex, RwLock};

use crate::config::AppConfig;
use crate::metrics::Metrics;

pub async fn force_flatten_positions(
    redis_url: Option<&str>,
    metrics: Arc<Metrics>,
    position_manager: Arc<Mutex<positions::PositionManager>>,
) -> Result<usize, PolybotError> {
    let closed_positions = {
        let mut manager = position_manager.lock().await;
        manager.close_all_positions()
    };

    if let Some(redis_url) = redis_url {
        if let Ok(store) = redis_store::RedisStore::new(redis_url).await {
            for position in &closed_positions {
                if let Err(e) = store.remove_position(position).await {
                    tracing::error!(error = %e, "Failed to remove flattened position from Redis");
                }
            }
        }
    }

    metrics.set_open_positions(0);
    for _ in 0..closed_positions.len() {
        metrics.total_positions_closed.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    Ok(closed_positions.len())
}

pub async fn run_state_manager(
    config: Arc<AppConfig>,
    metrics: Arc<Metrics>,
    position_manager: Arc<Mutex<positions::PositionManager>>,
    market_prices: Arc<RwLock<HashMap<String, Decimal>>>,
    receiver: mpsc::Receiver<Trade>,
) -> Result<(), PolybotError> {
    let sqlite_path = std::env::var("POLYBOT_SQLITE_PATH")
        .unwrap_or_else(|_| "./polybot.db".to_string());
    let sqlite_path = sqlite::SqliteStore::open(std::path::Path::new(&sqlite_path))
        .map(|_| sqlite_path)
        .ok();
    let redis_store = redis_store::RedisStore::new(&config.redis.url).await;

    match redis_store {
        Ok(store) => {
            tracing::info!("Connected to Redis for state persistence");
            metrics.set_redis_connected(true);
            run_with_redis(
                receiver,
                metrics,
                position_manager,
                market_prices,
                &store,
                sqlite_path.as_deref(),
            )
            .await
        }
        Err(e) => {
            tracing::warn!("Redis unavailable ({}), running in memory-only mode", e);
            metrics.set_redis_connected(false);
            run_in_memory(receiver, metrics, position_manager, market_prices, sqlite_path.as_deref()).await
        }
    }
}

async fn run_with_redis(
    mut receiver: mpsc::Receiver<Trade>,
    metrics: Arc<Metrics>,
    position_manager: Arc<Mutex<positions::PositionManager>>,
    market_prices: Arc<RwLock<HashMap<String, Decimal>>>,
    redis_store: &redis_store::RedisStore,
    sqlite_path: Option<&str>,
) -> Result<(), PolybotError> {
    while let Some(trade) = receiver.recv().await {
        tracing::info!(
            trade_id = %trade.id,
            signal_id = %trade.signal_id,
            status = ?trade.status,
            simulated = trade.simulated,
            "Processing trade"
        );

        // Persist trade to Redis
        if let Err(e) = redis_store.store_trade(&trade).await {
            tracing::error!(error = %e, "Failed to persist trade to Redis");
        }

        if let Some(sqlite_path) = sqlite_path {
            if let Err(e) = sqlite::SqliteStore::open(std::path::Path::new(sqlite_path))
                .and_then(|store| store.insert_trade(&trade))
            {
                tracing::error!(error = %e, "Failed to persist trade to SQLite");
            }
        }

        let (position_snapshot, open_positions, unrealized) = {
            let mut position_manager = position_manager.lock().await;
            if let Err(e) = position_manager.update_from_trade(&trade) {
                tracing::error!(error = %e, "Failed to update position from trade");
            }

            let current_prices = market_prices.read().await.clone();
            (
                position_manager
                    .get_position(&PositionKey::new(trade.market_id.clone(), trade.side))
                    .cloned(),
                position_manager.open_position_count(),
                pnl::calculate_unrealized_pnl(&position_manager, &current_prices),
            )
        };

        // Update position in Redis
        if let Some(pos) = position_snapshot.as_ref() {
            if let Err(e) = redis_store.store_position(pos).await {
                tracing::error!(error = %e, "Failed to persist position to Redis");
            }
        }

        metrics.set_open_positions(open_positions);
        metrics.update_daily_pnl(unrealized.to_f64().unwrap_or(0.0));
        tracing::info!(unrealized_pnl = %unrealized, "Unrealized PnL updated");
    }

    tracing::info!("State manager shutting down");
    Ok(())
}

async fn run_in_memory(
    mut receiver: mpsc::Receiver<Trade>,
    metrics: Arc<Metrics>,
    position_manager: Arc<Mutex<positions::PositionManager>>,
    market_prices: Arc<RwLock<HashMap<String, Decimal>>>,
    sqlite_path: Option<&str>,
) -> Result<(), PolybotError> {
    while let Some(trade) = receiver.recv().await {
        tracing::info!(
            trade_id = %trade.id,
            signal_id = %trade.signal_id,
            status = ?trade.status,
            simulated = trade.simulated,
            "Processing trade (in-memory)"
        );

        if let Some(sqlite_path) = sqlite_path {
            if let Err(e) = sqlite::SqliteStore::open(std::path::Path::new(sqlite_path))
                .and_then(|store| store.insert_trade(&trade))
            {
                tracing::error!(error = %e, "Failed to persist trade to SQLite");
            }
        }

        let (open_positions, unrealized) = {
            let mut position_manager = position_manager.lock().await;
            if let Err(e) = position_manager.update_from_trade(&trade) {
                tracing::error!(error = %e, "Failed to update position from trade");
            }

            let current_prices_in_memory = market_prices.read().await.clone();
            (
                position_manager.open_position_count(),
                pnl::calculate_unrealized_pnl(&position_manager, &current_prices_in_memory),
            )
        };

        metrics.set_open_positions(open_positions);
        metrics.update_daily_pnl(unrealized.to_f64().unwrap_or(0.0));
        tracing::info!(unrealized_pnl = %unrealized, "Unrealized PnL updated");
    }

    tracing::info!("State manager shutting down (in-memory)");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn test_trade(market_id: &str, side: Side, category: Category) -> Trade {
        Trade {
            id: format!("trade-{market_id}-{side:?}"),
            signal_id: "signal-1".to_string(),
            market_id: market_id.to_string(),
            category,
            side,
            price: Decimal::new(50, 2),
            size: Decimal::new(10, 0),
            size_usd: Decimal::new(500, 2),
            filled_size: Decimal::new(10, 0),
            order_type: OrderType::Limit,
            status: TradeStatus::Filled,
            placed_at: Utc::now(),
            filled_at: Some(Utc::now()),
            simulated: true,
        }
    }

    #[tokio::test]
    async fn force_flatten_positions_clears_positions_and_metrics() {
        let metrics = Arc::new(Metrics::new());
        let position_manager = Arc::new(Mutex::new(positions::PositionManager::new()));
        {
            let mut manager = position_manager.lock().await;
            manager
                .update_from_trade(&test_trade("m1", Side::Yes, Category::Politics))
                .unwrap();
            manager
                .update_from_trade(&test_trade("m2", Side::No, Category::Crypto))
                .unwrap();
        }
        metrics.set_open_positions(2);

        let closed = force_flatten_positions(None, metrics.clone(), position_manager.clone())
            .await
            .unwrap();

        assert_eq!(closed, 2);
        assert_eq!(metrics.open_positions.load(std::sync::atomic::Ordering::Relaxed), 0);
        assert_eq!(
            metrics
                .total_positions_closed
                .load(std::sync::atomic::Ordering::Relaxed),
            2
        );
        assert_eq!(position_manager.lock().await.open_position_count(), 0);
    }
}
