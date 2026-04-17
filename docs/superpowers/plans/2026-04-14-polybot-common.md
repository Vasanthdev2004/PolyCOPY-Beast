# Plan 1: polybot-common — Shared Types & Errors

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Create the foundational shared crate with all type definitions, error types, and constants that the rest of the workspace depends on.

**Architecture:** A pure Rust library crate (`polybot-common`) with no external service dependencies. Contains Serde-serializable types for signals, trades, positions, risk decisions, and a unified error enum. All other crates depend on this.

**Tech Stack:** Rust, `serde`, `serde_json`, `rust_decimal`, `uuid`, `thiserror`, `chrono`

---

### Task 1: Initialize Workspace & polybot-common Crate

**Files:**
- Create: `Cargo.toml` (workspace root)
- Create: `polybot-common/Cargo.toml`
- Create: `polybot-common/src/lib.rs`

- [ ] **Step 1: Create workspace root Cargo.toml**

```toml
[workspace]
members = [
    "polybot-common",
    "polybot-core",
    "polybot-dashboard",
]
resolver = "2"
```

- [ ] **Step 2: Create polybot-common Cargo.toml**

```toml
[package]
name = "polybot-common"
version = "0.1.0"
edition = "2021"

[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
rust_decimal = { version = "1", features = ["serde"] }
rust_decimal_macros = "1"
uuid = { version = "1", features = ["v4", "serde"] }
chrono = { version = "0.4", features = ["serde"] }
thiserror = "1"
```

- [ ] **Step 3: Create lib.rs stub**

```rust
pub mod types;
pub mod errors;
```

- [ ] **Step 4: Verify workspace compiles**

Run: `cargo check`
Expected: Compiles with warnings about empty modules (that's fine for now)

- [ ] **Step 5: Commit**

```bash
git init
git add Cargo.toml polybot-common/
git commit -m "feat: initialize workspace and polybot-common crate"
```

---

### Task 2: Define Market & Category Types

**Files:**
- Create: `polybot-common/src/types.rs`

- [ ] **Step 1: Write failing tests for category and market types**

Create `polybot-common/src/types.rs`:

```rust
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    Politics,
    Sports,
    Crypto,
    Others,
}

impl Category {
    pub fn max_exposure_pct(&self) -> Decimal {
        match self {
            Category::Politics => dec!(0.25),
            Category::Sports => dec!(0.20),
            Category::Crypto => dec!(0.15),
            Category::Others => dec!(0.10),
        }
    }

    pub fn confidence_offset(&self) -> Decimal {
        match self {
            Category::Politics => dec!(0),
            Category::Sports => dec!(-0.10),
            Category::Crypto => dec!(-0.10),
            Category::Others => dec!(-0.05),
        }
    }
}

impl fmt::Display for Category {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Category::Politics => write!(f, "politics"),
            Category::Sports => write!(f, "sports"),
            Category::Crypto => write!(f, "crypto"),
            Category::Others => write!(f, "others"),
        }
    }
}

impl TryFrom<&str> for Category {
    type Error = String;
    fn try_from(s: &str) -> Result<Self, Self::Error> {
        match s.to_lowercase().as_str() {
            "politics" => Ok(Category::Politics),
            "sports" => Ok(Category::Sports),
            "crypto" => Ok(Category::Crypto),
            _ => Ok(Category::Others),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MarketRef {
    pub condition_id: String,
    pub token_id: String,
    #[serde(default)]
    pub market_slug: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    Buy,
    Sell,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Yes,
    No,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderType {
    Limit,
    Ioc,
    Fok,
    PostOnly,
}
```

- [ ] **Step 2: Write unit tests for Category**

Append to `polybot-common/src/types.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn category_max_exposure() {
        assert_eq!(Category::Politics.max_exposure_pct(), dec!(0.25));
        assert_eq!(Category::Sports.max_exposure_pct(), dec!(0.20));
        assert_eq!(Category::Crypto.max_exposure_pct(), dec!(0.15));
        assert_eq!(Category::Others.max_exposure_pct(), dec!(0.10));
    }

    #[test]
    fn category_confidence_offset() {
        assert_eq!(Category::Politics.confidence_offset(), dec!(0));
        assert_eq!(Category::Sports.confidence_offset(), dec!(-0.10));
        assert_eq!(Category::Crypto.confidence_offset(), dec!(-0.10));
        assert_eq!(Category::Others.confidence_offset(), dec!(-0.05));
    }

    #[test]
    fn category_try_from_str() {
        assert_eq!(Category::try_from("politics").unwrap(), Category::Politics);
        assert_eq!(Category::try_from("SPORTS").unwrap(), Category::Sports);
        assert_eq!(Category::try_from("unknown").unwrap(), Category::Others);
    }
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p polybot-common`
Expected: All tests pass

- [ ] **Step 4: Commit**

```bash
git add polybot-common/src/types.rs
git commit -m "feat: add market and category types"
```

---

### Task 3: Define Signal Type & Validation

**Files:**
- Modify: `polybot-common/src/types.rs`

- [ ] **Step 1: Add Signal struct and ValidationOutcome**

Append to `polybot-common/src/types.rs` (before the `#[cfg(test)]` module):

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Signal {
    pub signal_id: String,
    pub timestamp: String,
    pub wallet_address: String,
    pub secret_level: u8,
    pub confidence: f64,
    pub category: Category,
    pub action: Action,
    pub market: MarketRef,
    pub side: Side,
    pub price: f64,
    pub size: f64,
    #[serde(default = "default_source")]
    pub source: String,
}

fn default_source() -> String {
    "unknown".to_string()
}

#[derive(Debug, Clone, PartialEq)]
pub enum ValidationError {
    InvalidSignalId(String),
    InvalidTimestamp(String),
    InvalidWalletAddress(String),
    SecretLevelOutOfRange(u8),
    ConfidenceOutOfRange(f64),
    InvalidAction(String),
    EmptyConditionId,
    EmptyTokenId,
    PriceOutOfRange(f64),
    SizeOutOfRange(f64),
}

impl Signal {
    pub fn validate(&self) -> Result<(), Vec<ValidationError>> {
        let mut errors = Vec::new();

        if self.signal_id.is_empty() {
            errors.push(ValidationError::InvalidSignalId(self.signal_id.clone()));
        }

        if DateTime::parse_from_rfc3339(&self.timestamp).is_err() {
            errors.push(ValidationError::InvalidTimestamp(self.timestamp.clone()));
        }

        if !self.wallet_address.starts_with("0x") || self.wallet_address.len() != 42 {
            errors.push(ValidationError::InvalidWalletAddress(self.wallet_address.clone()));
        }

        if self.secret_level < 1 || self.secret_level > 10 {
            errors.push(ValidationError::SecretLevelOutOfRange(self.secret_level));
        }

        if self.confidence < 0.0 || self.confidence > 1.0 {
            errors.push(ValidationError::ConfidenceOutOfRange(self.confidence));
        }

        if self.market.condition_id.is_empty() {
            errors.push(ValidationError::EmptyConditionId);
        }

        if self.market.token_id.is_empty() {
            errors.push(ValidationError::EmptyTokenId);
        }

        if self.price < 0.0 || self.price > 1.0 {
            errors.push(ValidationError::PriceOutOfRange(self.price));
        }

        if self.size <= 0.0 {
            errors.push(ValidationError::SizeOutOfRange(self.size));
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}
```

- [ ] **Step 2: Add validation tests**

Append to the `#[cfg(test)] mod tests` block in `types.rs`:

```rust
    #[test]
    fn signal_valid() {
        let signal = Signal {
            signal_id: "550e8400-e29b-41d4-a716-446655440000".to_string(),
            timestamp: "2026-04-14T12:34:56.789Z".to_string(),
            wallet_address: "0xabc123abc123abc123abc123abc123abc123abc1".to_string(),
            secret_level: 7,
            confidence: 0.85,
            category: Category::Politics,
            action: Action::Buy,
            market: MarketRef {
                condition_id: "0xdef456".to_string(),
                token_id: "0x789abc".to_string(),
                market_slug: Some("will-trump-win-2028".to_string()),
            },
            side: Side::Yes,
            price: 0.65,
            size: 500.0,
            source: "scanner_v1".to_string(),
        };
        assert!(signal.validate().is_ok());
    }

    #[test]
    fn signal_invalid_secret_level() {
        let mut signal = create_test_signal();
        signal.secret_level = 0;
        let err = signal.validate().unwrap_err();
        assert!(err.iter().any(|e| matches!(e, ValidationError::SecretLevelOutOfRange(0))));
    }

    #[test]
    fn signal_invalid_confidence() {
        let mut signal = create_test_signal();
        signal.confidence = 1.5;
        let err = signal.validate().unwrap_err();
        assert!(err.iter().any(|e| matches!(e, ValidationError::ConfidenceOutOfRange(1.5))));
    }

    #[test]
    fn signal_invalid_wallet_address() {
        let mut signal = create_test_signal();
        signal.wallet_address = "abc".to_string();
        let err = signal.validate().unwrap_err();
        assert!(err.iter().any(|e| matches!(e, ValidationError::InvalidWalletAddress(_))));
    }

    fn create_test_signal() -> Signal {
        Signal {
            signal_id: "550e8400-e29b-41d4-a716-446655440000".to_string(),
            timestamp: "2026-04-14T12:34:56.789Z".to_string(),
            wallet_address: "0xabc123abc123abc123abc123abc123abc123abc1".to_string(),
            secret_level: 7,
            confidence: 0.85,
            category: Category::Politics,
            action: Action::Buy,
            market: MarketRef {
                condition_id: "0xdef456".to_string(),
                token_id: "0x789abc".to_string(),
                market_slug: Some("test-market".to_string()),
            },
            side: Side::Yes,
            price: 0.65,
            size: 500.0,
            source: "scanner_v1".to_string(),
        }
    }
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p polybot-common`
Expected: All tests pass

- [ ] **Step 4: Commit**

```bash
git add polybot-common/src/types.rs
git commit -m "feat: add Signal type with validation"
```

---

### Task 4: Define Risk Types (Decision, Position, Trade)

**Files:**
- Modify: `polybot-common/src/types.rs`

- [ ] **Step 1: Add RiskDecision, Position, Trade types**

Insert before the `#[cfg(test)]` module in `types.rs`:

```rust
use std::time::Instant;

#[derive(Debug, Clone, PartialEq)]
pub enum Decision {
    Execute,
    Skip(String),
    EmergencyStop,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskDecision {
    pub signal_id: String,
    pub position_size_usd: Decimal,
    pub confidence_multiplier: Decimal,
    pub secret_level_multiplier: Decimal,
    pub drawdown_factor: Decimal,
    pub decision: Decision,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PositionStatus {
    Open,
    Closed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Position {
    pub id: String,
    pub condition_id: String,
    pub token_id: String,
    pub side: Side,
    pub entry_price: Decimal,
    pub current_size: Decimal,
    pub average_price: Decimal,
    pub opened_at: DateTime<Utc>,
    pub status: PositionStatus,
    pub category: Category,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Trade {
    pub id: String,
    pub signal_id: String,
    pub condition_id: String,
    pub token_id: String,
    pub side: Side,
    pub action: Action,
    pub price: Decimal,
    pub size: Decimal,
    pub size_usd: Decimal,
    pub filled_size: Decimal,
    pub order_type: OrderType,
    pub status: TradeStatus,
    pub placed_at: DateTime<Utc>,
    pub filled_at: Option<DateTime<Utc>>,
    pub simulated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TradeStatus {
    Pending,
    PartiallyFilled,
    Filled,
    Cancelled,
    Failed(String),
}
```

- [ ] **Step 2: Add Trade creation test**

Append to test module:

```rust
    #[test]
    fn trade_default_not_simulated() {
        let trade = Trade {
            id: "t1".to_string(),
            signal_id: "s1".to_string(),
            condition_id: "c1".to_string(),
            token_id: "tk1".to_string(),
            side: Side::Yes,
            action: Action::Buy,
            price: dec!(0.65),
            size: dec!(100),
            size_usd: dec!(65),
            filled_size: dec!(0),
            order_type: OrderType::Limit,
            status: TradeStatus::Pending,
            placed_at: Utc::now(),
            filled_at: None,
            simulated: false,
        };
        assert!(!trade.simulated);
    }
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p polybot-common`
Expected: All tests pass

- [ ] **Step 4: Commit**

```bash
git add polybot-common/src/types.rs
git commit -m "feat: add RiskDecision, Position, Trade types"
```

---

### Task 5: Define Error Types

**Files:**
- Create: `polybot-common/src/errors.rs`

- [ ] **Step 1: Write error types**

```rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum PolybotError {
    #[error("Scanner error: {0}")]
    Scanner(String),

    #[error("Risk engine error: {0}")]
    Risk(String),

    #[error("Execution error: {0}")]
    Execution(String),

    #[error("State error: {0}")]
    State(String),

    #[error("Config error: {0}")]
    Config(String),

    #[error("RPC pool error: {0}")]
    RpcPool(String),

    #[error("Validation error: {0}")]
    Validation(String),

    #[error("Redis error: {0}")]
    Redis(String),

    #[error("Telegram error: {0}")]
    Telegram(String),

    #[error("Channel closed")]
    ChannelClosed,

    #[error("Emergency stop active")]
    EmergencyStop,
}

pub type Result<T> = std::result::Result<T, PolybotError>;
```

- [ ] **Step 2: Write error tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display() {
        let err = PolybotError::Scanner("file not found".to_string());
        assert_eq!(format!("{}", err), "Scanner error: file not found");
    }

    #[test]
    fn error_conversion() {
        let result: Result<()> = Err(PolybotError::Config("missing field".to_string()));
        assert!(result.is_err());
    }
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p polybot-common`
Expected: All tests pass

- [ ] **Step 4: Commit**

```bash
git add polybot-common/src/errors.rs
git commit -m "feat: add PolybotError error types"
```

---

### Task 6: Define Constants & Multiplier Tables

**Files:**
- Modify: `polybot-common/src/lib.rs`

- [ ] **Step 1: Add constants module to lib.rs**

```rust
pub mod types;
pub mod errors;
pub mod constants;
```

Create `polybot-common/src/constants.rs`:

```rust
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::collections::HashMap;

pub fn confidence_multiplier(secret_level: u8) -> Decimal {
    let table: [(u8, Decimal); 10] = [
        (1, dec!(0.50)),
        (2, dec!(0.60)),
        (3, dec!(0.80)),
        (4, dec!(0.80)),
        (5, dec!(0.90)),
        (6, dec!(1.00)),
        (7, dec!(1.10)),
        (8, dec!(1.30)),
        (9, dec!(1.40)),
        (10, dec!(1.50)),
    ];
    table.iter()
        .find(|(level, _)| *level == secret_level)
        .map(|(_, mult)| *mult)
        .unwrap_or(dec!(0.50))
}

pub fn secret_level_multiplier(secret_level: u8) -> Decimal {
    match secret_level {
        1..=3 => dec!(0.30),
        4..=6 => dec!(0.70),
        7..=8 => dec!(1.00),
        9..=10 => dec!(1.30),
        _ => dec!(0.30),
    }
}

pub const DEFAULT_BASE_SIZE_USD: Decimal = dec!(50);
pub const DEFAULT_DAILY_MAX_LOSS_PCT: Decimal = dec!(0.05);
pub const DEFAULT_PER_MARKET_EXPOSURE_PCT: Decimal = dec!(0.10);
pub const DEFAULT_PER_CATEGORY_EXPOSURE_PCT: Decimal = dec!(0.25);
pub const DEFAULT_MAX_POSITION_SIZE_USD: Decimal = dec!(500);
pub const DEFAULT_MIN_CONFIDENCE: f64 = 0.60;
pub const DEFAULT_SLIPPAGE_THRESHOLD: Decimal = dec!(0.02);
pub const DEFAULT_DEDUP_WINDOW_SECS: u64 = 30;
pub const DEFAULT_RECONCILIATION_INTERVAL_SECS: u64 = 60;
pub const DEFAULT_DRAWDOWN_REDUCTION_FACTOR: Decimal = dec!(0.80);
```

- [ ] **Step 2: Add multiplier tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn confidence_multiplier_table() {
        assert_eq!(confidence_multiplier(1), dec!(0.50));
        assert_eq!(confidence_multiplier(5), dec!(0.90));
        assert_eq!(confidence_multiplier(7), dec!(1.10));
        assert_eq!(confidence_multiplier(10), dec!(1.50));
    }

    #[test]
    fn confidence_multiplier_out_of_range() {
        assert_eq!(confidence_multiplier(0), dec!(0.50));
        assert_eq!(confidence_multiplier(15), dec!(0.50));
    }

    #[test]
    fn secret_level_multiplier_table() {
        assert_eq!(secret_level_multiplier(2), dec!(0.30));
        assert_eq!(secret_level_multiplier(5), dec!(0.70));
        assert_eq!(secret_level_multiplier(8), dec!(1.00));
        assert_eq!(secret_level_multiplier(10), dec!(1.30));
    }
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p polybot-common`
Expected: All tests pass

- [ ] **Step 4: Commit**

```bash
git add polybot-common/src/constants.rs polybot-common/src/lib.rs
git commit -m "feat: add multiplier tables and constants"
```