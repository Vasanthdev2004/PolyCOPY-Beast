use polybot_common::errors::PolybotError;
use polybot_common::types::Trade;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignalLogEntry {
    pub signal_id: String,
    pub timestamp: String,
    pub wallet_address: String,
    pub market_id: String,
    pub confidence: u8,
    pub secret_level: u8,
    pub category: String,
    pub side: String,
    pub disposition: String,
    pub received_at: String,
}

/// v2.5: SQLite cold persistence for trade history and audit trail.
pub struct SqliteStore {
    conn: rusqlite::Connection,
}

impl SqliteStore {
    pub fn open(db_path: &Path) -> Result<Self, PolybotError> {
        let conn = rusqlite::Connection::open(db_path)
            .map_err(|e| PolybotError::State(format!("Failed to open SQLite: {}", e)))?;
        let store = Self { conn };
        store.create_tables()?;
        Ok(store)
    }

    pub fn open_in_memory() -> Result<Self, PolybotError> {
        let conn = rusqlite::Connection::open_in_memory()
            .map_err(|e| PolybotError::State(format!("Failed to open in-memory SQLite: {}", e)))?;
        let store = Self { conn };
        store.create_tables()?;
        Ok(store)
    }

    fn create_tables(&self) -> Result<(), PolybotError> {
        self.conn
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS trades (
                id TEXT PRIMARY KEY,
                signal_id TEXT NOT NULL,
                market_id TEXT NOT NULL,
                side TEXT NOT NULL,
                price TEXT NOT NULL,
                size TEXT NOT NULL,
                size_usd TEXT NOT NULL,
                filled_size TEXT NOT NULL,
                order_type TEXT NOT NULL,
                status TEXT NOT NULL,
                placed_at TEXT NOT NULL,
                filled_at TEXT,
                simulated INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE IF NOT EXISTS positions (
                id TEXT PRIMARY KEY,
                market_id TEXT NOT NULL,
                side TEXT NOT NULL,
                entry_price TEXT NOT NULL,
                current_size TEXT NOT NULL,
                average_price TEXT NOT NULL,
                opened_at TEXT NOT NULL,
                status TEXT NOT NULL,
                category TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS signal_log (
                signal_id TEXT PRIMARY KEY,
                timestamp TEXT NOT NULL,
                wallet_address TEXT NOT NULL,
                market_id TEXT NOT NULL,
                confidence INTEGER NOT NULL,
                secret_level INTEGER NOT NULL,
                category TEXT NOT NULL,
                side TEXT NOT NULL,
                disposition TEXT NOT NULL,
                received_at TEXT NOT NULL
            );",
            )
            .map_err(|e| PolybotError::State(format!("Failed to create tables: {}", e)))?;
        Ok(())
    }

    pub fn insert_trade(&self, trade: &Trade) -> Result<(), PolybotError> {
        self.conn.execute(
            "INSERT OR REPLACE INTO trades (id, signal_id, market_id, side, price, size, size_usd, filled_size, order_type, status, placed_at, filled_at, simulated)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            rusqlite::params![
                trade.id,
                trade.signal_id,
                trade.market_id,
                format!("{:?}", trade.side),
                trade.price.to_string(),
                trade.size.to_string(),
                trade.size_usd.to_string(),
                trade.filled_size.to_string(),
                format!("{:?}", trade.order_type),
                format!("{:?}", trade.status),
                trade.placed_at.to_rfc3339(),
                trade.filled_at.map(|t| t.to_rfc3339()),
                trade.simulated as i32,
            ],
        ).map_err(|e| PolybotError::State(format!("Failed to insert trade: {}", e)))?;
        Ok(())
    }

    pub fn insert_signal_log(
        &self,
        signal_id: &str,
        timestamp: &str,
        wallet_address: &str,
        market_id: &str,
        confidence: u8,
        secret_level: u8,
        category: &str,
        side: &str,
        disposition: &str,
    ) -> Result<(), PolybotError> {
        self.conn.execute(
            "INSERT OR IGNORE INTO signal_log (signal_id, timestamp, wallet_address, market_id, confidence, secret_level, category, side, disposition, received_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, datetime('now'))",
            rusqlite::params![signal_id, timestamp, wallet_address, market_id, confidence, secret_level, category, side, disposition],
        ).map_err(|e| PolybotError::State(format!("Failed to insert signal log: {}", e)))?;
        Ok(())
    }

    pub fn get_trade_count(&self) -> Result<u64, PolybotError> {
        let count: u64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM trades", [], |row| row.get(0))
            .map_err(|e| PolybotError::State(format!("Failed to count trades: {}", e)))?;
        Ok(count)
    }

    pub fn latest_signals(&self, limit: usize) -> Result<Vec<SignalLogEntry>, PolybotError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT signal_id, timestamp, wallet_address, market_id, confidence, secret_level, category, side, disposition, received_at
                 FROM signal_log
                 ORDER BY received_at DESC
                 LIMIT ?1",
            )
            .map_err(|e| PolybotError::State(format!("Failed to prepare signal query: {}", e)))?;

        let rows = stmt
            .query_map([limit as i64], |row| {
                Ok(SignalLogEntry {
                    signal_id: row.get(0)?,
                    timestamp: row.get(1)?,
                    wallet_address: row.get(2)?,
                    market_id: row.get(3)?,
                    confidence: row.get(4)?,
                    secret_level: row.get(5)?,
                    category: row.get(6)?,
                    side: row.get(7)?,
                    disposition: row.get(8)?,
                    received_at: row.get(9)?,
                })
            })
            .map_err(|e| PolybotError::State(format!("Failed to query signals: {}", e)))?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| PolybotError::State(format!("Failed to read signals: {}", e)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use polybot_common::types::*;
    use rust_decimal_macros::dec;

    #[test]
    fn open_in_memory() {
        let store = SqliteStore::open_in_memory().unwrap();
        assert_eq!(store.get_trade_count().unwrap(), 0);
    }

    #[test]
    fn insert_and_count_trade() {
        let store = SqliteStore::open_in_memory().unwrap();
        let trade = Trade {
            id: "t1".to_string(),
            signal_id: "s1".to_string(),
            market_id: "m1".to_string(),
            category: Category::Politics,
            side: Side::Yes,
            price: dec!(0.65),
            size: dec!(100),
            size_usd: dec!(65),
            filled_size: dec!(100),
            order_type: OrderType::Limit,
            status: TradeStatus::Filled,
            placed_at: chrono::Utc::now(),
            filled_at: Some(chrono::Utc::now()),
            simulated: true,
        };
        store.insert_trade(&trade).unwrap();
        assert_eq!(store.get_trade_count().unwrap(), 1);
    }

    #[test]
    fn insert_signal_log() {
        let store = SqliteStore::open_in_memory().unwrap();
        store
            .insert_signal_log(
                "sig-1",
                "2026-04-14T12:00:00Z",
                "0xabc",
                "m1",
                7,
                6,
                "politics",
                "YES",
                "execute",
            )
            .unwrap();
    }

    #[test]
    fn latest_signals_returns_rows() {
        let store = SqliteStore::open_in_memory().unwrap();
        store
            .insert_signal_log(
                "sig-1",
                "2026-04-14T12:00:00Z",
                "0xabc",
                "m1",
                7,
                6,
                "politics",
                "YES",
                "execute",
            )
            .unwrap();

        let signals = store.latest_signals(10).unwrap();
        assert_eq!(signals.len(), 1);
        assert_eq!(signals[0].signal_id, "sig-1");
    }
}
