use chrono::{DateTime, Utc};
use polybot_common::errors::PolybotError;
use polybot_common::types::{Category, Position, PositionStatus, Side, Trade};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::str::FromStr as _;
use std::time::Duration;

const REDIS_PREFIX: &str = "polybot";

#[derive(Debug, Serialize, Deserialize)]
struct PositionSnapshot {
    pub id: String,
    pub market_id: String,
    pub side: String,
    pub entry_price: String,
    pub current_size: String,
    pub average_price: String,
    pub opened_at: String,
    pub status: String,
    pub category: String,
}

impl From<&Position> for PositionSnapshot {
    fn from(pos: &Position) -> Self {
        Self {
            id: pos.id.clone(),
            market_id: pos.market_id.clone(),
            side: format!("{:?}", pos.side),
            entry_price: pos.entry_price.to_string(),
            current_size: pos.current_size.to_string(),
            average_price: pos.average_price.to_string(),
            opened_at: pos.opened_at.to_rfc3339(),
            status: format!("{:?}", pos.status),
            category: format!("{:?}", pos.category),
        }
    }
}

impl TryFrom<PositionSnapshot> for Position {
    type Error = PolybotError;

    fn try_from(snapshot: PositionSnapshot) -> Result<Self, Self::Error> {
        let side = match snapshot.side.as_str() {
            "Yes" => Side::Yes,
            "No" => Side::No,
            other => {
                return Err(PolybotError::Redis(format!(
                    "Unknown position side in Redis snapshot: {}",
                    other
                )));
            }
        };

        let status = match snapshot.status.as_str() {
            "Open" => PositionStatus::Open,
            "Closed" => PositionStatus::Closed,
            "Ghost" => PositionStatus::Ghost,
            other => {
                return Err(PolybotError::Redis(format!(
                    "Unknown position status in Redis snapshot: {}",
                    other
                )));
            }
        };

        let category = match snapshot.category.as_str() {
            "Politics" => Category::Politics,
            "Sports" => Category::Sports,
            "Crypto" => Category::Crypto,
            "Other" => Category::Other,
            other => {
                return Err(PolybotError::Redis(format!(
                    "Unknown position category in Redis snapshot: {}",
                    other
                )));
            }
        };

        let opened_at = DateTime::parse_from_rfc3339(&snapshot.opened_at)
            .map_err(|e| PolybotError::Redis(format!("Invalid opened_at in Redis snapshot: {}", e)))?
            .with_timezone(&Utc);

        Ok(Position {
            id: snapshot.id,
            market_id: snapshot.market_id,
            side,
            entry_price: Decimal::from_str(&snapshot.entry_price)
                .map_err(|e| PolybotError::Redis(format!("Invalid entry_price in Redis snapshot: {}", e)))?,
            current_size: Decimal::from_str(&snapshot.current_size)
                .map_err(|e| PolybotError::Redis(format!("Invalid current_size in Redis snapshot: {}", e)))?,
            average_price: Decimal::from_str(&snapshot.average_price)
                .map_err(|e| PolybotError::Redis(format!("Invalid average_price in Redis snapshot: {}", e)))?,
            opened_at,
            status,
            category,
        })
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct TradeSnapshot {
    pub id: String,
    pub signal_id: String,
    pub market_id: String,
    pub side: String,
    pub price: String,
    pub size: String,
    pub size_usd: String,
    pub filled_size: String,
    pub order_type: String,
    pub status: String,
    pub placed_at: String,
    pub simulated: bool,
}

impl From<&Trade> for TradeSnapshot {
    fn from(trade: &Trade) -> Self {
        Self {
            id: trade.id.clone(),
            signal_id: trade.signal_id.clone(),
            market_id: trade.market_id.clone(),
            side: format!("{:?}", trade.side),
            price: trade.price.to_string(),
            size: trade.size.to_string(),
            size_usd: trade.size_usd.to_string(),
            filled_size: trade.filled_size.to_string(),
            order_type: format!("{:?}", trade.order_type),
            status: format!("{:?}", trade.status),
            placed_at: trade.placed_at.to_rfc3339(),
            simulated: trade.simulated,
        }
    }
}

pub struct RedisStore {
    client: redis::Client,
    conn: Option<redis::aio::MultiplexedConnection>,
}

impl RedisStore {
    fn positions_index_key() -> String {
        format!("{}:positions:_index", REDIS_PREFIX)
    }

    fn position_storage_key(position: &Position) -> String {
        format!(
            "{}:positions:{}:{}",
            REDIS_PREFIX,
            position.market_id,
            match position.side {
                Side::Yes => "yes",
                Side::No => "no",
            }
        )
    }

    pub async fn new(url: &str) -> Result<Self, PolybotError> {
        let client = redis::Client::open(url)
            .map_err(|e| PolybotError::Redis(format!("Failed to create Redis client: {}", e)))?;

        let conn = Self::connect_with_retry(&client, 3, Duration::from_secs(2)).await?;

        Ok(Self {
            client,
            conn: Some(conn),
        })
    }

    async fn connect_with_retry(
        client: &redis::Client,
        max_retries: u32,
        delay: Duration,
    ) -> Result<redis::aio::MultiplexedConnection, PolybotError> {
        let mut attempts = 0;
        loop {
            match client.get_multiplexed_async_connection().await {
                Ok(conn) => {
                    tracing::info!("Redis connection established");
                    return Ok(conn);
                }
                Err(e) => {
                    attempts += 1;
                    if attempts >= max_retries {
                        return Err(PolybotError::Redis(format!(
                            "Failed to connect to Redis after {} attempts: {}",
                            attempts, e
                        )));
                    }
                    tracing::warn!(
                        "Redis connection attempt {} failed: {}, retrying...",
                        attempts,
                        e
                    );
                    tokio::time::sleep(delay).await;
                }
            }
        }
    }

    pub async fn store_position(&self, position: &Position) -> Result<(), PolybotError> {
        let mut conn = self
            .conn
            .as_ref()
            .ok_or_else(|| PolybotError::Redis("No Redis connection".to_string()))?
            .clone();

        let key = Self::position_storage_key(position);
        let snapshot = PositionSnapshot::from(position);
        let json = serde_json::to_string(&snapshot)
            .map_err(|e| PolybotError::Redis(format!("Failed to serialize position: {}", e)))?;

        redis::cmd("SET")
            .arg(&key)
            .arg(&json)
            .exec_async(&mut conn)
            .await
            .map_err(|e| PolybotError::Redis(format!("Failed to store position: {}", e)))?;

        let index_key = Self::positions_index_key();
        redis::cmd("SADD")
            .arg(&index_key)
            .arg(&key)
            .exec_async(&mut conn)
            .await
            .map_err(|e| PolybotError::Redis(format!("Failed to update positions index: {}", e)))?;

        Ok(())
    }

    pub async fn remove_position(&self, position: &Position) -> Result<(), PolybotError> {
        let mut conn = self
            .conn
            .as_ref()
            .ok_or_else(|| PolybotError::Redis("No Redis connection".to_string()))?
            .clone();

        let key = Self::position_storage_key(position);
        redis::cmd("DEL")
            .arg(&key)
            .exec_async(&mut conn)
            .await
            .map_err(|e| PolybotError::Redis(format!("Failed to delete position {}: {}", key, e)))?;

        let index_key = Self::positions_index_key();
        redis::cmd("SREM")
            .arg(&index_key)
            .arg(&key)
            .exec_async(&mut conn)
            .await
            .map_err(|e| PolybotError::Redis(format!("Failed to update positions index: {}", e)))?;

        Ok(())
    }

    pub async fn store_trade(&self, trade: &Trade) -> Result<(), PolybotError> {
        let mut conn = self
            .conn
            .as_ref()
            .ok_or_else(|| PolybotError::Redis("No Redis connection".to_string()))?
            .clone();

        let key = format!("{}:trades:{}", REDIS_PREFIX, trade.id);
        let snapshot = TradeSnapshot::from(trade);
        let json = serde_json::to_string(&snapshot)
            .map_err(|e| PolybotError::Redis(format!("Failed to serialize trade: {}", e)))?;

        redis::cmd("SET")
            .arg(&key)
            .arg(&json)
            .exec_async(&mut conn)
            .await
            .map_err(|e| PolybotError::Redis(format!("Failed to store trade: {}", e)))?;

        let index_key = format!("{}:trades:_index", REDIS_PREFIX);
        let score = trade.placed_at.timestamp();
        redis::cmd("ZADD")
            .arg(&index_key)
            .arg(score)
            .arg(&trade.id)
            .exec_async(&mut conn)
            .await
            .map_err(|e| PolybotError::Redis(format!("Failed to update trades index: {}", e)))?;

        Ok(())
    }

    pub async fn update_daily_pnl(&self, pnl: Decimal) -> Result<(), PolybotError> {
        let mut conn = self
            .conn
            .as_ref()
            .ok_or_else(|| PolybotError::Redis("No Redis connection".to_string()))?
            .clone();

        let key = format!("{}:daily_pnl", REDIS_PREFIX);
        let ttl: u64 = 86400 * 2;

        redis::cmd("SETEX")
            .arg(&key)
            .arg(ttl)
            .arg(pnl.to_string())
            .exec_async(&mut conn)
            .await
            .map_err(|e| PolybotError::Redis(format!("Failed to update daily PnL: {}", e)))?;

        Ok(())
    }

    pub async fn set_system_status(&self, key: &str, value: &str) -> Result<(), PolybotError> {
        let mut conn = self
            .conn
            .as_ref()
            .ok_or_else(|| PolybotError::Redis("No Redis connection".to_string()))?
            .clone();

        let redis_key = format!("{}:status:{}", REDIS_PREFIX, key);
        redis::cmd("SET")
            .arg(&redis_key)
            .arg(value)
            .exec_async(&mut conn)
            .await
            .map_err(|e| PolybotError::Redis(format!("Failed to set status: {}", e)))?;

        Ok(())
    }

    pub async fn is_healthy(&self) -> bool {
        if let Some(conn) = self.conn.as_ref() {
            let mut conn = conn.clone();
            redis::cmd("PING")
                .query_async::<String>(&mut conn)
                .await
                .is_ok()
        } else {
            false
        }
    }

    pub async fn list_positions(&self) -> Result<Vec<Position>, PolybotError> {
        let mut conn = self
            .conn
            .as_ref()
            .ok_or_else(|| PolybotError::Redis("No Redis connection".to_string()))?
            .clone();

        let index_key = Self::positions_index_key();
        let keys: Vec<String> = redis::cmd("SMEMBERS")
            .arg(&index_key)
            .query_async(&mut conn)
            .await
            .map_err(|e| PolybotError::Redis(format!("Failed to load positions index: {}", e)))?;

        let mut positions = Vec::new();
        for key in keys {
            let json: Option<String> = redis::cmd("GET")
                .arg(&key)
                .query_async(&mut conn)
                .await
                .map_err(|e| PolybotError::Redis(format!("Failed to load position {}: {}", key, e)))?;

            if let Some(json) = json {
                let snapshot: PositionSnapshot = serde_json::from_str(&json)
                    .map_err(|e| PolybotError::Redis(format!("Failed to deserialize position {}: {}", key, e)))?;
                positions.push(snapshot.try_into()?);
            }
        }

        Ok(positions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn test_position(side: Side) -> Position {
        Position {
            id: format!("test-{side:?}"),
            market_id: "market-1".to_string(),
            side,
            entry_price: dec!(0.65),
            current_size: dec!(100),
            average_price: dec!(0.65),
            opened_at: Utc::now(),
            status: PositionStatus::Open,
            category: Category::Politics,
        }
    }

    #[test]
    fn position_snapshot_serialization() {
        let pos = PositionSnapshot {
            id: "test".to_string(),
            market_id: "market-1".to_string(),
            side: "Yes".to_string(),
            entry_price: "0.65".to_string(),
            current_size: "100".to_string(),
            average_price: "0.65".to_string(),
            opened_at: "2026-04-14T00:00:00Z".to_string(),
            status: "Open".to_string(),
            category: "Politics".to_string(),
        };
        let json = serde_json::to_string(&pos).unwrap();
        assert!(json.contains("market-1"));
    }

    #[test]
    fn trade_snapshot_serialization() {
        let trade = TradeSnapshot {
            id: "t1".to_string(),
            signal_id: "s1".to_string(),
            market_id: "m1".to_string(),
            side: "Yes".to_string(),
            price: "0.65".to_string(),
            size: "100".to_string(),
            size_usd: "65".to_string(),
            filled_size: "100".to_string(),
            order_type: "Limit".to_string(),
            status: "Filled".to_string(),
            placed_at: "2026-04-14T12:00:00Z".to_string(),
            simulated: true,
        };
        let json = serde_json::to_string(&trade).unwrap();
        assert!(json.contains("s1"));
        assert!(json.contains("\"simulated\":true"));
    }

    #[test]
    fn position_storage_key_distinguishes_sides() {
        let yes = test_position(Side::Yes);
        let no = test_position(Side::No);

        assert_ne!(
            RedisStore::position_storage_key(&yes),
            RedisStore::position_storage_key(&no)
        );
    }
}
