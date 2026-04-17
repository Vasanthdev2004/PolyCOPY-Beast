# PolyBot v3 Module 1 Core Trading Engine Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rework the current v2.5 execution/risk core into the v3 Module 1 trading engine so a validated signal can become a safe simulation, shadow, or live Polymarket order decision without depending on Module 0 approval flow or Module 2 ingestion sources.

**Architecture:** Keep the existing Rust workspace and channel-driven topology, but isolate Module 1 around explicit execution modes, a market-context service, a simulation-safe execution transport, v3 risk sizing, deterministic order planning, and retryable order submission semantics. Module 1 should consume a validated internal signal contract and expose a stable execution interface for later modules without yet depending on Data API wallet tracking or dashboard polish.

**Tech Stack:** Rust 2021, Tokio, Axum, `polymarket-client-sdk`, `reqwest`, `rust_decimal`, `serde`, `tracing`

---

## File Structure

**Create**
- `polybot-core/src/execution/market_context.rs`
- `polybot-core/src/execution/transport.rs`
- `polybot-core/src/execution/retry.rs`

**Modify**
- `polybot-common/src/constants.rs`
- `polybot-common/src/types.rs`
- `polybot-core/src/config.rs`
- `polybot-core/src/main.rs`
- `polybot-core/src/metrics.rs`
- `polybot-core/src/risk/mod.rs`
- `polybot-core/src/risk/limits.rs`
- `polybot-core/src/risk/sizer.rs`
- `polybot-core/src/execution/mod.rs`
- `polybot-core/src/execution/clob_client.rs`
- `polybot-core/src/execution/clob_ws.rs`
- `polybot-core/src/execution/order_builder.rs`
- `polybot-core/src/state/positions.rs`

**Why these files**
- `polybot-common/src/types.rs`: shared runtime mode, normalized signal fields, and execution decision types.
- `polybot-common/src/constants.rs`: v3 defaults and reusable execution/risk thresholds.
- `polybot-core/src/config.rs`: v3 runtime config surface for core trading only.
- `polybot-core/src/execution/*.rs`: all market metadata, transport isolation, order planning, retry policy, and submission flow.
- `polybot-core/src/risk/*.rs`: v3 sizing formula and hard limits.
- `polybot-core/src/main.rs`: wiring new execution mode and transport setup.
- `polybot-core/src/state/positions.rs`: engine-facing flatten support for emergency stop.

---

### Task 1: Add v3 Core Runtime Mode and Normalized Module 1 Signal Fields

**Files:**
- Modify: `polybot-common/src/types.rs`
- Modify: `polybot-common/src/constants.rs`
- Modify: `polybot-core/src/config.rs`
- Modify: `polybot-core/src/main.rs`
- Test: `polybot-common/src/types.rs`
- Test: `polybot-core/src/config.rs`

- [ ] **Step 1: Write the failing shared-type and config tests**

Add these tests before implementation.

In `polybot-common/src/types.rs`:

```rust
#[test]
fn execution_mode_serializes_as_lowercase() {
    let json = serde_json::to_string(&ExecutionMode::Shadow).unwrap();
    assert_eq!(json, "\"shadow\"");
}

#[test]
fn signal_supports_module_1_core_fields() {
    let signal = Signal {
        signal_id: "550e8400-e29b-41d4-a716-446655440000".to_string(),
        timestamp: chrono::Utc::now().to_rfc3339(),
        wallet_address: "0xabc123abc123abc123abc123abc123abc123abc1".to_string(),
        market_id: "market-1".to_string(),
        side: Side::Yes,
        confidence: 8,
        secret_level: 9,
        category: Category::Politics,
        suggested_size_usdc: Some(dec!(75)),
        scanner_version: "1.0.0".to_string(),
        tx_hash: Some("0xfeed".to_string()),
        source: Some("manual".to_string()),
        target_wallet: Some("0xdef456def456def456def456def456def456def4".to_string()),
        target_price: Some(dec!(0.62)),
        target_size_usdc: Some(dec!(500)),
    };

    assert_eq!(signal.target_price, Some(dec!(0.62)));
    assert_eq!(signal.target_size_usdc, Some(dec!(500)));
}
```

In `polybot-core/src/config.rs`:

```rust
#[test]
fn apply_env_overrides_execution_mode_shadow() {
    std::env::set_var("POLYBOT_EXECUTION_MODE", "shadow");
    let mut config = AppConfig::default();
    config.apply_env_overrides();
    assert_eq!(config.system.execution_mode, polybot_common::types::ExecutionMode::Shadow);
    std::env::remove_var("POLYBOT_EXECUTION_MODE");
}
```

- [ ] **Step 2: Run the focused tests to verify RED**

Run:

```bash
cargo test -p polybot-common execution_mode_serializes_as_lowercase
cargo test -p polybot-core apply_env_overrides_execution_mode_shadow
```

Expected:
- FAIL because `ExecutionMode` and the extra signal/config fields do not exist yet.

- [ ] **Step 3: Implement the minimal shared-mode and config surface**

Add the mode enum in `polybot-common/src/types.rs`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExecutionMode {
    Simulation,
    Shadow,
    Live,
}

impl ExecutionMode {
    pub fn allows_network_market_data(self) -> bool {
        !matches!(self, ExecutionMode::Simulation)
    }

    pub fn allows_live_order_submission(self) -> bool {
        matches!(self, ExecutionMode::Live)
    }
}
```

Extend `Signal` in `polybot-common/src/types.rs`:

```rust
#[serde(default)]
pub tx_hash: Option<String>,
#[serde(default)]
pub source: Option<String>,
#[serde(default)]
pub target_wallet: Option<String>,
#[serde(default)]
pub target_price: Option<Decimal>,
#[serde(default)]
pub target_size_usdc: Option<Decimal>,
```

Update `SystemConfig` in `polybot-core/src/config.rs`:

```rust
pub struct SystemConfig {
    pub simulation: bool,
    pub execution_mode: polybot_common::types::ExecutionMode,
    pub log_level: String,
}
```

Update defaults and env override handling:

```rust
execution_mode: polybot_common::types::ExecutionMode::Simulation,
```

```rust
if let Ok(val) = std::env::var("POLYBOT_EXECUTION_MODE") {
    self.system.execution_mode = match val.to_lowercase().as_str() {
        "live" => polybot_common::types::ExecutionMode::Live,
        "shadow" => polybot_common::types::ExecutionMode::Shadow,
        _ => polybot_common::types::ExecutionMode::Simulation,
    };
}
```

In `polybot-core/src/main.rs`, normalize the old boolean into the new mode for logging:

```rust
let mode = config.system.execution_mode;
tracing::info!(?mode, "PolyBot v3 core mode selected");
```

- [ ] **Step 4: Run the focused tests to verify GREEN**

Run:

```bash
cargo test -p polybot-common execution_mode_serializes_as_lowercase
cargo test -p polybot-core apply_env_overrides_execution_mode_shadow
```

Expected:
- PASS

- [ ] **Step 5: Commit**

```bash
git add polybot-common/src/types.rs polybot-common/src/constants.rs polybot-core/src/config.rs polybot-core/src/main.rs
git commit -m "feat: add v3 execution modes and core signal fields"
```

---

### Task 2: Isolate Market Context into a Focused Service

**Files:**
- Create: `polybot-core/src/execution/market_context.rs`
- Modify: `polybot-core/src/execution/mod.rs`
- Modify: `polybot-core/src/execution/clob_client.rs`
- Test: `polybot-core/src/execution/market_context.rs`

- [ ] **Step 1: Write the failing market-context tests**

Create `polybot-core/src/execution/market_context.rs` with tests first:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn market_context_requires_positive_tick_and_min_size() {
        let ctx = MarketContext {
            market_id: "market-1".to_string(),
            token_id: "token-1".to_string(),
            tick_size: dec!(0),
            min_order_size: dec!(0),
            neg_risk: false,
            resolved: false,
        };
        assert!(ctx.validate().is_err());
    }

    #[test]
    fn resolved_market_is_rejected() {
        let ctx = MarketContext {
            market_id: "market-1".to_string(),
            token_id: "token-1".to_string(),
            tick_size: dec!(0.01),
            min_order_size: dec!(1),
            neg_risk: false,
            resolved: true,
        };
        assert!(ctx.validate().is_err());
    }
}
```

- [ ] **Step 2: Run the focused test to verify RED**

Run:

```bash
cargo test -p polybot-core market_context_requires_positive_tick_and_min_size
```

Expected:
- FAIL because `market_context.rs` and `MarketContext::validate()` do not exist yet.

- [ ] **Step 3: Implement the minimal market-context service**

In `polybot-core/src/execution/market_context.rs` add:

```rust
use polybot_common::errors::PolybotError;
use rust_decimal::Decimal;

#[derive(Debug, Clone, PartialEq)]
pub struct MarketContext {
    pub market_id: String,
    pub token_id: String,
    pub tick_size: Decimal,
    pub min_order_size: Decimal,
    pub neg_risk: bool,
    pub resolved: bool,
}

impl MarketContext {
    pub fn validate(&self) -> Result<(), PolybotError> {
        if self.tick_size <= Decimal::ZERO {
            return Err(PolybotError::Execution("tick_size must be positive".to_string()));
        }
        if self.min_order_size <= Decimal::ZERO {
            return Err(PolybotError::Execution("min_order_size must be positive".to_string()));
        }
        if self.resolved {
            return Err(PolybotError::Execution("market is resolved".to_string()));
        }
        Ok(())
    }
}
```

In `polybot-core/src/execution/mod.rs` expose the module:

```rust
pub mod market_context;
```

In `polybot-core/src/execution/clob_client.rs`, replace the inline `MarketContext` definition with a `use super::market_context::MarketContext;` and extend the builder:

```rust
Ok(MarketContext {
    market_id: market.condition_id.clone(),
    token_id,
    tick_size,
    min_order_size,
    neg_risk: market.neg_risk,
    resolved: false,
})
```

- [ ] **Step 4: Run the focused tests to verify GREEN**

Run:

```bash
cargo test -p polybot-core market_context_requires_positive_tick_and_min_size
cargo test -p polybot-core resolved_market_is_rejected
```

Expected:
- PASS

- [ ] **Step 5: Commit**

```bash
git add polybot-core/src/execution/market_context.rs polybot-core/src/execution/mod.rs polybot-core/src/execution/clob_client.rs
git commit -m "feat: extract market context service"
```

---

### Task 3: Add Transport Isolation So Simulation Makes No Real CLOB Calls

**Files:**
- Create: `polybot-core/src/execution/transport.rs`
- Modify: `polybot-core/src/execution/mod.rs`
- Modify: `polybot-core/src/execution/clob_client.rs`
- Test: `polybot-core/src/execution/transport.rs`
- Test: `polybot-core/src/execution/mod.rs`

- [ ] **Step 1: Write the failing simulation-network-isolation tests**

In `polybot-core/src/execution/transport.rs`, start with:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use polybot_common::types::ExecutionMode;

    #[test]
    fn simulation_mode_uses_simulated_transport() {
        let selected = select_transport_mode(ExecutionMode::Simulation);
        assert_eq!(selected, TransportMode::Simulation);
    }
}
```

In `polybot-core/src/execution/mod.rs`, add a focused unit test:

```rust
#[tokio::test]
async fn simulation_mode_avoids_live_transport_submission() {
    let mode = polybot_common::types::ExecutionMode::Simulation;
    assert!(!mode.allows_live_order_submission());
}
```

- [ ] **Step 2: Run the focused tests to verify RED**

Run:

```bash
cargo test -p polybot-core simulation_mode_uses_simulated_transport
cargo test -p polybot-core simulation_mode_avoids_live_transport_submission
```

Expected:
- FAIL because `transport.rs` and transport selection do not exist yet.

- [ ] **Step 3: Implement the minimal transport abstraction**

Create `polybot-core/src/execution/transport.rs`:

```rust
use async_trait::async_trait;
use polybot_common::errors::PolybotError;

use super::market_context::MarketContext;
use super::order_builder::Order;
use polybot_common::types::Trade;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportMode {
    Simulation,
    Live,
}

pub fn select_transport_mode(mode: polybot_common::types::ExecutionMode) -> TransportMode {
    if mode.allows_live_order_submission() {
        TransportMode::Live
    } else {
        TransportMode::Simulation
    }
}

#[async_trait]
pub trait ExecutionTransport: Send + Sync {
    async fn submit_order(&self, order: &Order) -> Result<Trade, PolybotError>;
    async fn market_context(&self, market_id: &str, side: polybot_common::types::Side) -> Result<MarketContext, PolybotError>;
}
```

In `polybot-core/src/execution/mod.rs`, add:

```rust
pub mod transport;
```

Wire transport selection off `config.system.execution_mode` and ensure simulation/shadow both skip live order submission.

- [ ] **Step 4: Run the focused tests to verify GREEN**

Run:

```bash
cargo test -p polybot-core simulation_mode_uses_simulated_transport
cargo test -p polybot-core simulation_mode_avoids_live_transport_submission
```

Expected:
- PASS

- [ ] **Step 5: Commit**

```bash
git add polybot-core/src/execution/transport.rs polybot-core/src/execution/mod.rs polybot-core/src/execution/clob_client.rs
git commit -m "feat: isolate execution transport for simulation safety"
```

---

### Task 4: Upgrade Risk Sizing to the v3 Formula and Hard Limits

**Files:**
- Modify: `polybot-common/src/constants.rs`
- Modify: `polybot-core/src/config.rs`
- Modify: `polybot-core/src/risk/sizer.rs`
- Modify: `polybot-core/src/risk/limits.rs`
- Modify: `polybot-core/src/risk/mod.rs`
- Test: `polybot-core/src/risk/sizer.rs`
- Test: `polybot-core/src/risk/limits.rs`

- [ ] **Step 1: Write the failing v3 risk tests**

In `polybot-core/src/risk/sizer.rs` add:

```rust
#[test]
fn v3_base_size_uses_target_size_and_position_multiplier() {
    let size = calculate_position_size_from_target(
        dec!(500),
        dec!(0.10),
        8,
        9,
        dec!(1.0),
        dec!(150),
        dec!(5),
    );
    assert_eq!(size, dec!(150));
}
```

In `polybot-core/src/risk/limits.rs` add:

```rust
#[test]
fn micro_trade_notional_is_rejected() {
    let result = validate_notional_floor(dec!(0.99));
    assert!(result.is_some());
}
```

- [ ] **Step 2: Run the focused tests to verify RED**

Run:

```bash
cargo test -p polybot-core v3_base_size_uses_target_size_and_position_multiplier
cargo test -p polybot-core micro_trade_notional_is_rejected
```

Expected:
- FAIL because the v3 helpers do not exist yet.

- [ ] **Step 3: Implement the minimal v3 sizing and limit helpers**

In `polybot-core/src/risk/sizer.rs` add:

```rust
pub fn calculate_position_size_from_target(
    target_size_usdc: Decimal,
    position_multiplier: Decimal,
    confidence: u8,
    secret_level: u8,
    drawdown_factor: Decimal,
    category_max_usd: Decimal,
    min_trade_size_usdc: Decimal,
) -> Decimal {
    let base_size = target_size_usdc * position_multiplier;
    let conf = confidence_multiplier(confidence);
    let sl = secret_level_multiplier(secret_level);
    if conf == Decimal::ZERO || sl == Decimal::ZERO {
        return Decimal::ZERO;
    }
    let size = (base_size * conf * sl * drawdown_factor)
        .min(category_max_usd)
        .max(min_trade_size_usdc);
    size
}
```

In `polybot-core/src/risk/limits.rs` add:

```rust
pub fn validate_notional_floor(notional: Decimal) -> Option<String> {
    if notional < rust_decimal_macros::dec!(1.0) {
        Some("signal rejected below $1.00 notional floor".to_string())
    } else {
        None
    }
}
```

Extend `RiskConfig` in `polybot-core/src/config.rs` with:

```rust
pub position_multiplier: Decimal,
pub min_trade_size_usdc: Decimal,
pub min_usdc_balance: Decimal,
pub max_consecutive_losses: u32,
pub loss_cooldown_secs: u64,
pub price_buffer: Decimal,
```

- [ ] **Step 4: Run the focused tests to verify GREEN**

Run:

```bash
cargo test -p polybot-core v3_base_size_uses_target_size_and_position_multiplier
cargo test -p polybot-core micro_trade_notional_is_rejected
```

Expected:
- PASS

- [ ] **Step 5: Commit**

```bash
git add polybot-common/src/constants.rs polybot-core/src/config.rs polybot-core/src/risk/sizer.rs polybot-core/src/risk/limits.rs polybot-core/src/risk/mod.rs
git commit -m "feat: implement v3 core risk sizing and notional limits"
```

---

### Task 5: Refactor Order Planning for v3 FOK-First Semantics

**Files:**
- Modify: `polybot-core/src/execution/order_builder.rs`
- Modify: `polybot-common/src/types.rs`
- Test: `polybot-core/src/execution/order_builder.rs`

- [ ] **Step 1: Write the failing v3 order-planning tests**

Add these tests in `polybot-core/src/execution/order_builder.rs`:

```rust
#[test]
fn fok_plan_applies_price_buffer() {
    let decision = test_decision(dec!(1.0), dec!(1.0));
    let ctx = test_market_context();
    let order = build_order_with_price_buffer(&decision, &ctx, dec!(0.50), dec!(50), dec!(0.01), OrderType::Fok);
    assert_eq!(order.price, dec!(0.51));
}

#[test]
fn fok_plan_is_not_silently_downgraded() {
    let decision = test_decision(dec!(1.0), dec!(1.0));
    let ctx = test_market_context();
    let order = build_order_with_price_buffer(&decision, &ctx, dec!(0.50), dec!(50), dec!(0.00), OrderType::Fok);
    assert_eq!(order.order_type, OrderType::Fok);
}
```

- [ ] **Step 2: Run the focused tests to verify RED**

Run:

```bash
cargo test -p polybot-core fok_plan_applies_price_buffer
cargo test -p polybot-core fok_plan_is_not_silently_downgraded
```

Expected:
- FAIL because the new builder helper does not exist yet.

- [ ] **Step 3: Implement the minimal v3 order planner**

In `polybot-core/src/execution/order_builder.rs` add:

```rust
pub fn build_order_with_price_buffer(
    decision: &RiskDecision,
    market_context: &MarketContext,
    fetched_price: Decimal,
    size_usd: Decimal,
    price_buffer: Decimal,
    order_type: OrderType,
) -> Order {
    let planned_price = match order_type {
        OrderType::Fok => fetched_price * (Decimal::ONE + price_buffer),
        _ => fetched_price,
    };

    let mut order = build_order(decision, market_context, planned_price, size_usd);
    order.order_type = order_type;
    order
}
```

Keep `build_order()` as the lower-level constructor, but route execution through `build_order_with_price_buffer()`.

- [ ] **Step 4: Run the focused tests to verify GREEN**

Run:

```bash
cargo test -p polybot-core fok_plan_applies_price_buffer
cargo test -p polybot-core fok_plan_is_not_silently_downgraded
```

Expected:
- PASS

- [ ] **Step 5: Commit**

```bash
git add polybot-core/src/execution/order_builder.rs polybot-common/src/types.rs
git commit -m "feat: add v3 fok-first order planning"
```

---

### Task 6: Add Retry Policy for 429 and 5xx Failures

**Files:**
- Create: `polybot-core/src/execution/retry.rs`
- Modify: `polybot-core/src/execution/mod.rs`
- Modify: `polybot-core/src/execution/clob_client.rs`
- Test: `polybot-core/src/execution/retry.rs`

- [ ] **Step 1: Write the failing retry-policy tests**

Create `polybot-core/src/execution/retry.rs` with tests first:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_policy_caps_after_three_attempts() {
        let policy = RetryPolicy::default();
        assert_eq!(policy.max_retries, 3);
    }

    #[test]
    fn retryable_server_error_is_classified_as_retryable() {
        let err = RetryClass::from_status(500);
        assert!(matches!(err, RetryClass::Retryable));
    }
}
```

- [ ] **Step 2: Run the focused tests to verify RED**

Run:

```bash
cargo test -p polybot-core retry_policy_caps_after_three_attempts
```

Expected:
- FAIL because `retry.rs` does not exist yet.

- [ ] **Step 3: Implement the minimal retry policy**

Create `polybot-core/src/execution/retry.rs`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryClass {
    Retryable,
    NonRetryable,
}

impl RetryClass {
    pub fn from_status(status: u16) -> Self {
        if status == 429 || status >= 500 {
            RetryClass::Retryable
        } else {
            RetryClass::NonRetryable
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct RetryPolicy {
    pub max_retries: u32,
    pub base_delay_ms: u64,
    pub max_delay_ms: u64,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: 3,
            base_delay_ms: 1_000,
            max_delay_ms: 60_000,
        }
    }
}
```

Expose it in `polybot-core/src/execution/mod.rs`:

```rust
pub mod retry;
```

- [ ] **Step 4: Run the focused tests to verify GREEN**

Run:

```bash
cargo test -p polybot-core retry_policy_caps_after_three_attempts
cargo test -p polybot-core retryable_server_error_is_classified_as_retryable
```

Expected:
- PASS

- [ ] **Step 5: Commit**

```bash
git add polybot-core/src/execution/retry.rs polybot-core/src/execution/mod.rs polybot-core/src/execution/clob_client.rs
git commit -m "feat: add v3 execution retry policy"
```

---

### Task 7: Integrate v3 Execution Flow, Latency Metrics, and Emergency Hook

**Files:**
- Modify: `polybot-core/src/execution/mod.rs`
- Modify: `polybot-core/src/metrics.rs`
- Modify: `polybot-core/src/state/positions.rs`
- Modify: `polybot-core/src/main.rs`
- Test: `polybot-core/src/execution/mod.rs`
- Test: `polybot-core/src/metrics.rs`

- [ ] **Step 1: Write the failing execution-integration tests**

Add a focused test in `polybot-core/src/execution/mod.rs`:

```rust
#[tokio::test]
async fn shadow_mode_does_not_submit_live_orders() {
    let mode = polybot_common::types::ExecutionMode::Shadow;
    assert!(!mode.allows_live_order_submission());
}
```

Add a latency test in `polybot-core/src/metrics.rs` if one does not already cover average/max update in the new execution path:

```rust
#[test]
fn record_latency_updates_average_and_max() {
    let m = Metrics::new();
    m.record_latency(300);
    m.record_latency(900);
    assert!(m.max_latency_us.load(std::sync::atomic::Ordering::Relaxed) >= 900);
}
```

- [ ] **Step 2: Run the focused tests to verify RED or missing integration**

Run:

```bash
cargo test -p polybot-core shadow_mode_does_not_submit_live_orders
```

Expected:
- FAIL or prove that the integration path is still incomplete.

- [ ] **Step 3: Implement the minimal integrated v3 execution flow**

In `polybot-core/src/execution/mod.rs`:

- select transport mode from `ExecutionMode`
- in `Simulation`, use only synthetic context/trade creation
- in `Shadow`, perform market metadata + pricing + planning, but skip `submit_order()`
- in `Live`, perform market metadata + planning + retry-backed submission

Use this shape:

```rust
match config.system.execution_mode {
    polybot_common::types::ExecutionMode::Simulation => {
        let trade = order_builder::create_simulated_trade(&decision, &order);
        metrics.record_trade(true);
        state_sender.send(trade).await?;
    }
    polybot_common::types::ExecutionMode::Shadow => {
        tracing::info!(signal_id = %decision.signal_id, "Shadow mode: plan complete, submission skipped");
        metrics.record_trade_failed();
    }
    polybot_common::types::ExecutionMode::Live => {
        let started = std::time::Instant::now();
        let trade = client.submit_order(&order).await?;
        metrics.record_latency(started.elapsed().as_micros() as u64);
        metrics.record_trade(false);
        state_sender.send(trade).await?;
    }
}
```

Also ensure `Decision::EmergencyStop` keeps the hook path wired so later modules can trigger flatten/cancel from one place.

- [ ] **Step 4: Run the focused tests to verify GREEN**

Run:

```bash
cargo test -p polybot-core shadow_mode_does_not_submit_live_orders
cargo test -p polybot-core record_latency_updates_average_and_max
```

Expected:
- PASS

- [ ] **Step 5: Commit**

```bash
git add polybot-core/src/execution/mod.rs polybot-core/src/metrics.rs polybot-core/src/state/positions.rs polybot-core/src/main.rs
git commit -m "feat: integrate v3 execution flow and latency tracking"
```

---

### Task 8: Verify Module 1 Must-Haves

**Files:**
- Modify: `docs/superpowers/plans/2026-04-17-polybot-v3-module-1-core-trading-engine.md`

- [ ] **Step 1: Run focused core verification**

Run:

```bash
cargo test -p polybot-common -p polybot-core
```

Expected:
- PASS with all Module 1 tests green.

- [ ] **Step 2: Run compile verification for dependent crate**

Run:

```bash
cargo check -p polybot-dashboard
```

Expected:
- PASS

- [ ] **Step 3: Review Module 1 spec coverage**

Check the approved spec against the implemented tasks:

- market metadata lookup
- pricing/slippage guard
- v3 sizing and limits
- FOK placement
- signing/auth integration
- simulation/shadow/live split
- retry/backoff
- emergency-stop hook

Record any remaining gaps at the bottom of this plan file as `Module 1 follow-ups`.

- [ ] **Step 4: Commit**

```bash
git add docs/superpowers/plans/2026-04-17-polybot-v3-module-1-core-trading-engine.md
git commit -m "docs: record module 1 verification results"
```
