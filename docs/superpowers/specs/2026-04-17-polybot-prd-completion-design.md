# PolyBot PRD Completion Design

**Date:** 2026-04-17
**Status:** Approved for implementation
**Scope:** Complete `project_prd_v2.5.md` using the existing Rust workspace, staged in recommended delivery order.

---

## 1. Goal

Finish the project to the PRD end state without rewriting the codebase from scratch. The existing workspace already contains the correct top-level shape:

- `polybot-common` for shared domain types
- `polybot-core` for trading, state, reconciliation, health, and Telegram operations
- `polybot-dashboard` for the Leptos operator UI

The design preserves that structure and closes the gaps between the current implementation and the PRD.

---

## 2. Delivery Order

The full PRD is too large to execute as a single undifferentiated implementation push, so delivery is staged in this order:

1. Trading correctness and live execution safety
2. State, PnL, and reconciliation correctness
3. Telegram/operator controls and alerting
4. Dashboard, analytics, and replay/backtesting interface
5. Production hardening, deployment, and runbook proof

This order is mandatory because later workstreams depend on the correctness of the earlier ones. In particular, the dashboard and Telegram surfaces must not be treated as complete until they are reading from trustworthy execution, ledger, and reconciliation data.

---

## 3. Current State Summary

The current codebase already implements:

- Multi-source scanner ingestion via file watcher, HTTP, and Redis
- Risk evaluation with multiplier tables, category thresholds, and exposure checks
- A Polymarket SDK integration layer with market-data reads and order submission
- Redis and SQLite persistence scaffolding
- Reconciliation scaffolding
- Telegram auth, confirmation, rate limiting, and command routing
- A starter Leptos dashboard with health, risk, positions, and signals views

The main blockers to PRD completion are not the absence of modules, but incorrect or partial domain behavior inside existing modules.

---

## 4. End-State Architecture

The final system remains a modular monolith inside `polybot-core`, but the internal contracts are tightened around six durable subsystems:

### 4.1 Signal Intake

Responsibilities:

- Strict schema validation
- Timestamp skew enforcement
- Wallet and market identifier validation
- Deduplication by signal identity
- Source tagging and normalized `SignalEnvelope` creation
- Manual-review routing for low-confidence or low-secret signals

Key result:

All downstream modules receive one normalized event model instead of re-validating ad hoc fields.

### 4.2 Market Context

Responsibilities:

- Resolve condition IDs and token IDs
- Fetch and cache order books
- Fetch and cache tick size and min order size
- Resolve `neg_risk` requirements
- Verify balance and allowance readiness for buy/sell paths
- Maintain websocket-backed warm data where possible

Key result:

Execution no longer guesses order precision or market shape. Every order is created from current market metadata.

### 4.3 Risk Engine

Responsibilities:

- Base size calculation and periodic refresh
- Confidence, secret-level, and drawdown multipliers
- Category-specific thresholds and caps
- Per-market and per-category exposure enforcement
- Followed-wallet controls
- Pause/resume/emergency-stop policy

Key result:

Risk decisions become deterministic and auditable, with each rejection path carrying a clear reason.

### 4.4 Execution Engine

Responsibilities:

- Turn `RiskDecision` + `MarketContext` into `OrderIntent`
- Select valid order type and precision
- Submit via `polymarket_client_sdk`
- Track lifecycle through websocket user channel plus polling fallback
- Cancel, timeout, retry, or flatten when required
- Emit latency measurements and durable order/trade events

Key result:

Execution becomes a true lifecycle manager instead of a submit-and-assume-success adapter.

### 4.5 State and Accounting

Responsibilities:

- Position ledger keyed by `PositionKey`
- Durable orders, trades, fills, and equity snapshots
- Realized and unrealized PnL with signed values
- Drawdown and daily-loss tracking
- Redis hot state plus SQLite audit persistence
- Reconciliation against authoritative sources

Key result:

All operator surfaces read from one consistent accounting model.

### 4.6 Operator Surfaces

Responsibilities:

- Health and metrics API
- Telegram commands and confirmations
- Dashboard data APIs
- Scheduled digests and critical alerts
- Manual override actions

Key result:

The dashboard and Telegram bot become thin operator surfaces over real state, not alternate business-logic sources.

---

## 5. Core Domain Corrections

### 5.1 Position Identity

Current behavior keys positions by `market_id` only. That is insufficient because YES and NO sides on the same market can collide.

The final design introduces:

- `PositionKey { market_id, side }`
- `PositionLot` or equivalent fill-aware position accumulation
- Exposure and reconciliation indexed by `PositionKey`

All position, persistence, dashboard, and reconciliation code must migrate to this identity.

### 5.2 Order Lifecycle

The current implementation is too close to "submit order, then treat the result as execution." The final design introduces explicit state models:

- `OrderIntent`
- `OrderRecord`
- `OrderStatus`
- `TradeFill`

Orders must remain first-class until they are matched, partially filled, cancelled, timed out, or flattened.

### 5.3 Ledger and PnL

The current PnL path is placeholder-driven and cannot represent losses safely enough for production.

The final design requires:

- Signed PnL representation
- Realized PnL from matched exit events
- Unrealized PnL from current market prices
- Equity snapshots for the dashboard
- Drawdown derived from ledger/equity, not placeholder counters

### 5.4 Market Precision

Execution must use live market metadata rather than hardcoded assumptions:

- Tick size can vary by market
- Min order size is market-provided
- Negative-risk markets require explicit handling

The market-context layer owns this logic so the order builder receives fully validated inputs.

---

## 6. Workstream Deliverables

## 6.1 Workstream 1: Trading Correctness and Live Execution

Deliverables:

- Strict signal validation aligned with the PRD
- Side-aware positions and exposure checks
- Market context service for token resolution, tick size, min size, and neg-risk
- Order planning that honors tick size and valid order semantics
- Live order lifecycle tracking through user-channel updates
- Real emergency flatten flow for open positions and active orders
- Latency instrumentation across scanner -> risk -> plan -> submit -> fill transitions

Exit criteria:

- Simulation and live-mode order flows use the same lifecycle model
- Emergency stop closes or cancels outstanding exposure, not just toggles a flag
- Execution surfaces enough state for reconciliation and operator views

## 6.2 Workstream 2: State, PnL, and Reconciliation

Deliverables:

- Signed ledger-backed PnL
- Equity snapshots and drawdown tracking
- Redis/SQLite persistence for orders, fills, positions, and snapshots
- Light reconciliation against CLOB state
- Full reconciliation using authoritative sources
- Ghost, missing, and mismatch handling

Exit criteria:

- Dashboard and Telegram no longer rely on placeholder PnL
- Reconciliation can rebuild or quarantine divergent state deterministically

## 6.3 Workstream 3: Telegram and Operator Controls

Deliverables:

- Full PRD command semantics
- Destructive command confirmation flows
- Real positions and recent signal views
- Runtime config management with validation
- Digest scheduling and critical alerts
- Reconcile and flatten actions surfaced safely

Exit criteria:

- Telegram actions operate on real system state, not summary counters alone

## 6.4 Workstream 4: Dashboard and Replay/Backtesting UI

Deliverables:

- Live signal feed with confidence/secret-level visual treatment
- Active positions with unrealized PnL and latency details
- Equity curve and drawdown visualization
- Risk dashboard with exposure and category allocation
- Latency and performance panels
- Manual override controls
- Replay/backtesting screens and adjustable playback controls

Exit criteria:

- Dashboard matches the PRD feature set and is backed by server APIs rather than hardcoded assumptions

## 6.5 Workstream 5: Production Hardening

Deliverables:

- Deployment-safe config and secret handling
- Better repository hygiene and ignore rules
- Failure drills and backup/restore verification
- Improved retry, timeout, and rate-limit behavior
- Go-live checklist matching the runbook

Exit criteria:

- The repo can be operated through the documented runbook with credible live-readiness evidence

---

## 7. Verification Strategy

Every workstream must be delivered with evidence:

- Unit tests for new logic and corrected edge cases
- Integration-style tests for core state transitions
- `cargo test` verification for touched crates
- `cargo check` verification for the dashboard whenever UI contracts change
- Manual command-path verification for Telegram-facing logic where automated coverage is insufficient

No feature is considered complete until the corresponding verification exists and passes fresh.

---

## 8. Milestones

### Milestone A

Trading core and accounting are trustworthy enough that:

- positions are side-aware
- PnL is signed and non-placeholder
- emergency stop can flatten
- reconciliation uses real order/position state

### Milestone B

Telegram and health/operator endpoints are fully PRD-aligned and trustworthy.

### Milestone C

Dashboard and replay/backtesting interfaces reach PRD feature parity.

### Milestone D

Production hardening and deployment verification close the remaining runbook gaps.

---

## 9. Non-Goals

This design does not add out-of-scope markets, social features, custody features, or unrelated refactors. The implementation must stay tightly aligned to the PRD and the current workspace.
