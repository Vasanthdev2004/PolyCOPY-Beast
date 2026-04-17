# PolyBot Workstream 1 Core Correctness Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the trading core PRD-correct by fixing position identity, market-context-aware order planning, execution lifecycle tracking, and non-placeholder accounting foundations.

**Architecture:** Keep the existing workspace and modular-monolith layout, but tighten the domain model. State becomes keyed by `PositionKey`, execution emits durable order lifecycle events, and accounting reads from real order/fill state instead of placeholder counters.

**Tech Stack:** Rust, Tokio, Axum, Teloxide, Redis, SQLite, `polymarket_client_sdk`, Leptos

---

### Task 1: Add Side-Aware Position Identity

**Files:**
- Modify: `polybot-common/src/types.rs`
- Modify: `polybot-core/src/state/positions.rs`
- Test: `polybot-common/src/types.rs`
- Test: `polybot-core/src/state/positions.rs`

- [ ] **Step 1: Write the failing shared-type tests**

Add tests that require a reusable `PositionKey` with `market_id + side` identity and verify equality semantics for YES/NO positions on the same market.

```rust
#[test]
fn position_key_distinguishes_market_side_pairs() {
    let yes = PositionKey::new("market-1", Side::Yes);
    let no = PositionKey::new("market-1", Side::No);
    assert_ne!(yes, no);
    assert_eq!(yes.market_id, "market-1");
}
```

- [ ] **Step 2: Run the type tests to verify RED**

Run: `cargo test -p polybot-common position_key_distinguishes_market_side_pairs`
Expected: FAIL because `PositionKey` does not exist yet

- [ ] **Step 3: Write the minimal shared-type implementation**

Add a new key type in `polybot-common/src/types.rs`:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PositionKey {
    pub market_id: String,
    pub side: Side,
}

impl PositionKey {
    pub fn new(market_id: impl Into<String>, side: Side) -> Self {
        Self {
            market_id: market_id.into(),
            side,
        }
    }
}
```

- [ ] **Step 4: Run the type tests to verify GREEN**

Run: `cargo test -p polybot-common position_key_distinguishes_market_side_pairs`
Expected: PASS

- [ ] **Step 5: Write the failing position-manager tests**

Add tests requiring separate YES/NO positions in `polybot-core/src/state/positions.rs`:

```rust
#[test]
fn separate_sides_create_separate_positions() {
    let mut pm = PositionManager::new();
    pm.update_from_trade(&test_trade("m1", dec!(0.5), dec!(10), Side::Yes, Category::Politics)).unwrap();
    pm.update_from_trade(&test_trade("m1", dec!(0.6), dec!(10), Side::No, Category::Politics)).unwrap();

    assert!(pm.get_position(&PositionKey::new("m1", Side::Yes)).is_some());
    assert!(pm.get_position(&PositionKey::new("m1", Side::No)).is_some());
    assert_eq!(pm.open_position_count(), 2);
}
```

- [ ] **Step 6: Run the position-manager test to verify RED**

Run: `cargo test -p polybot-core separate_sides_create_separate_positions`
Expected: FAIL because `PositionManager` is keyed only by `market_id`

- [ ] **Step 7: Implement minimal side-aware position storage**

Change the manager storage and accessors in `polybot-core/src/state/positions.rs`:

```rust
pub struct PositionManager {
    positions: HashMap<PositionKey, Position>,
}

fn key_for_trade(trade: &Trade) -> PositionKey {
    PositionKey::new(trade.market_id.clone(), trade.side)
}
```

Update `update_from_trade`, `get_position`, `close_position`, `market_exposure`, and `category_exposure` to work with `PositionKey`.

- [ ] **Step 8: Run the focused core tests to verify GREEN**

Run: `cargo test -p polybot-core separate_sides_create_separate_positions`
Expected: PASS

- [ ] **Step 9: Commit**

```bash
git add polybot-common/src/types.rs polybot-core/src/state/positions.rs
git commit -m "fix: key positions by market and side"
```

---

### Task 2: Fix Signed Accounting and State Persistence Foundations

**Files:**
- Modify: `polybot-core/src/metrics.rs`
- Modify: `polybot-core/src/state/pnl.rs`
- Modify: `polybot-core/src/state/mod.rs`
- Modify: `polybot-core/src/state/sqlite.rs`
- Modify: `polybot-core/src/state/redis_store.rs`
- Test: `polybot-core/src/metrics.rs`
- Test: `polybot-core/src/state/pnl.rs`

- [ ] **Step 1: Write the failing signed-PnL metrics test**

Add a metrics test requiring negative daily PnL support:

```rust
#[test]
fn daily_pnl_supports_losses() {
    let m = Metrics::new();
    m.update_daily_pnl(-12.34);
    assert!((m.daily_pnl_usd() + 12.34).abs() < 0.01);
}
```

- [ ] **Step 2: Run the metrics test to verify RED**

Run: `cargo test -p polybot-core daily_pnl_supports_losses`
Expected: FAIL because the current storage uses unsigned atomics

- [ ] **Step 3: Implement minimal signed-PnL metric storage**

Replace unsigned storage with signed cents:

```rust
use std::sync::atomic::AtomicI64;

pub daily_pnl_cents: AtomicI64,
pub total_pnl_cents: AtomicI64,
```

Update:

```rust
pub fn update_daily_pnl(&self, pnl_usd: f64) {
    let cents = (pnl_usd * 100.0).round() as i64;
    self.daily_pnl_cents.store(cents, Ordering::Relaxed);
}
```

- [ ] **Step 4: Run the metrics test to verify GREEN**

Run: `cargo test -p polybot-core daily_pnl_supports_losses`
Expected: PASS

- [ ] **Step 5: Write the failing unrealized-PnL regression test**

Add a test in `polybot-core/src/state/pnl.rs` that requires non-zero unrealized PnL when live prices differ from average price:

```rust
#[test]
fn unrealized_pnl_uses_market_prices() {
    let mut manager = PositionManager::new();
    manager.update_from_trade(&test_trade("m1", dec!(0.50), dec!(100), Side::Yes, Category::Politics)).unwrap();

    let mut prices = std::collections::HashMap::new();
    prices.insert("m1".to_string(), dec!(0.70));

    assert_eq!(calculate_unrealized_pnl(&manager, &prices), dec!(20));
}
```

- [ ] **Step 6: Run the PnL regression to verify RED or incorrect behavior**

Run: `cargo test -p polybot-core unrealized_pnl_uses_market_prices`
Expected: FAIL until helper fixtures and position access are updated for `PositionKey`

- [ ] **Step 7: Implement minimal PnL/accounting fixes**

Update tests/helpers and `calculate_unrealized_pnl` consumers so state-manager paths no longer pass empty price maps by default when priced data is available. Introduce a narrow seam in `state/mod.rs` for injected current prices rather than hardcoding `HashMap::new()`.

- [ ] **Step 8: Run focused accounting tests**

Run: `cargo test -p polybot-core daily_pnl_supports_losses unrealized_pnl_uses_market_prices`
Expected: PASS

- [ ] **Step 9: Commit**

```bash
git add polybot-core/src/metrics.rs polybot-core/src/state/pnl.rs polybot-core/src/state/mod.rs polybot-core/src/state/sqlite.rs polybot-core/src/state/redis_store.rs
git commit -m "fix: support signed pnl and accounting foundations"
```

---

### Task 3: Introduce Market Context for Order Planning

**Files:**
- Modify: `polybot-core/src/execution/clob_client.rs`
- Modify: `polybot-core/src/execution/order_builder.rs`
- Modify: `polybot-core/src/execution/mod.rs`
- Test: `polybot-core/src/execution/order_builder.rs`
- Test: `polybot-core/src/execution/clob_client.rs`

- [ ] **Step 1: Write the failing order-precision test**

Add an order-builder test that requires rounding by tick size instead of a hardcoded 2-decimal assumption:

```rust
#[test]
fn build_order_respects_tick_size() {
    let decision = test_decision(dec!(1.0), dec!(1.0));
    let ctx = MarketContext {
        token_id: "token-1".to_string(),
        tick_size: dec!(0.001),
        min_order_size: dec!(1),
        neg_risk: false,
    };

    let order = build_order(&decision, &ctx, dec!(0.537), dec!(50));
    assert_eq!(order.price, dec!(0.537));
}
```

- [ ] **Step 2: Run the order-builder test to verify RED**

Run: `cargo test -p polybot-core build_order_respects_tick_size`
Expected: FAIL because `MarketContext` and tick-size-aware pricing do not exist

- [ ] **Step 3: Implement minimal market-context type and order-builder changes**

Add a context model in `clob_client.rs` or a dedicated execution module:

```rust
#[derive(Debug, Clone)]
pub struct MarketContext {
    pub token_id: String,
    pub tick_size: Decimal,
    pub min_order_size: Decimal,
    pub neg_risk: bool,
}
```

Change `build_order` to take `&MarketContext` and round using `tick_size`.

- [ ] **Step 4: Run the order-builder test to verify GREEN**

Run: `cargo test -p polybot-core build_order_respects_tick_size`
Expected: PASS

- [ ] **Step 5: Write the failing market-context fetch test**

Add a focused test around mapping market metadata into `MarketContext` with current market fields.

- [ ] **Step 6: Implement minimal metadata resolution**

Extend `ClobClient` helpers to return token ID, tick size, min order size, and neg-risk from market/orderbook lookups.

- [ ] **Step 7: Run focused execution tests**

Run: `cargo test -p polybot-core build_order_respects_tick_size`
Expected: PASS with no regressions in existing execution tests

- [ ] **Step 8: Commit**

```bash
git add polybot-core/src/execution/clob_client.rs polybot-core/src/execution/order_builder.rs polybot-core/src/execution/mod.rs
git commit -m "feat: add market-context-aware order planning"
```

---

### Task 4: Add Order Lifecycle Tracking and Emergency Flatten

**Files:**
- Modify: `polybot-core/src/execution/mod.rs`
- Modify: `polybot-core/src/execution/clob_ws.rs`
- Modify: `polybot-core/src/telegram_bot/commands.rs`
- Modify: `polybot-core/src/state/mod.rs`
- Test: `polybot-core/src/execution/mod.rs`
- Test: `polybot-core/src/telegram_bot/commands.rs`

- [ ] **Step 1: Write the failing emergency-stop behavior test**

Add a test that requires emergency stop to emit flatten/cancel actions instead of only toggling pause state.

- [ ] **Step 2: Run the focused emergency-stop test to verify RED**

Run: `cargo test -p polybot-core emergency_stop`
Expected: FAIL because the current command flow only sets the pause flag

- [ ] **Step 3: Implement minimal flatten orchestration**

Introduce an execution path that:

- cancels active orders
- enumerates open positions
- submits offsetting liquidation intents
- emits state events for the resulting lifecycle

- [ ] **Step 4: Add lifecycle tracking seam**

Persist `Pending`, `PartiallyFilled`, `Filled`, `Cancelled`, and `TimedOut` transitions as durable events instead of inferring completion only from submit responses.

- [ ] **Step 5: Run focused execution tests**

Run: `cargo test -p polybot-core emergency_stop`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add polybot-core/src/execution/mod.rs polybot-core/src/execution/clob_ws.rs polybot-core/src/telegram_bot/commands.rs polybot-core/src/state/mod.rs
git commit -m "feat: add order lifecycle tracking and emergency flatten"
```

---

### Task 5: Verify the Milestone

**Files:**
- Modify: `docs/superpowers/plans/2026-04-17-polybot-workstream-1-core-correctness.md`

- [ ] **Step 1: Run targeted core verification**

Run: `cargo test -p polybot-common -p polybot-core`
Expected: PASS

- [ ] **Step 2: Run dashboard contract verification**

Run: `cargo check -p polybot-dashboard`
Expected: PASS

- [ ] **Step 3: Record milestone notes**

Update this plan file with a short completion note at the bottom summarizing what changed and what remains for Workstream 2.

- [ ] **Step 4: Commit**

```bash
git add docs/superpowers/plans/2026-04-17-polybot-workstream-1-core-correctness.md
git commit -m "docs: record workstream 1 verification notes"
```
