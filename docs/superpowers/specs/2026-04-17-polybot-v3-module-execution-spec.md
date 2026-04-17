# SuperFast PolyBot v3.0 Module Execution Spec

**Date:** 2026-04-17  
**Status:** Drafted for review  
**Parent PRD:** [SuperFast_PolyBot_v3_PRD_Enhanced.md](D:/Copy%20bot%20test/Glm%20copy/SuperFast_PolyBot_v3_PRD_Enhanced.md)  
**Scope:** Turn the v3.0 master PRD into an implementation-safe module plan with launch priorities, deliverables, and acceptance criteria.

---

## 1. Purpose

The v3.0 PRD is the master vision and technical direction document. This execution spec exists to make that PRD buildable without ambiguity.

It does three things:

1. Splits the PRD into 5 implementation modules plus one setup prerequisite slice.
2. Defines exact deliverables and acceptance criteria for each module.
3. Separates `Must-have for first safe live deployment` from `Should-have for v3` and `Later`.

This document is the execution bridge between the v3 PRD and the eventual module-by-module implementation plans.

---

## 2. Delivery Strategy

### 2.1 Recommended Model

Use a **hybrid strategy**:

- Keep the system decomposed by clean subsystem boundaries.
- Implement in launch-driven order so we reach a safe first live deployment as early as possible.

### 2.2 Final Module Set

The project will be executed as:

1. **Module 1:** Core Trading Engine
2. **Module 2:** Signal Ingestion & Wallet Tracker
3. **Module 3:** State, SQLite, and Reconciliation
4. **Module 4:** Telegram Operator Layer
5. **Module 5:** Dashboard / Operator Panel

Additionally:

- **Module 0:** Setup & Deployment Prerequisites

Module 0 is a prerequisite slice, not one of the 5 core product modules.

### 2.3 Implementation Order

Implementation follows this order:

1. Module 1: Core Trading Engine
2. Module 2: Signal Ingestion & Wallet Tracker
3. Module 3: State, SQLite, and Reconciliation
4. Module 4: Telegram Operator Layer
5. Module 5: Dashboard / Operator Panel

This order is mandatory for launch safety because monitoring and controls must sit on top of trustworthy execution and state.

---

## 3. Launch Readiness Definition

### 3.1 Must-Have For First Safe Live Deployment

The system is eligible for first safe live deployment only when all of the following are complete:

- Module 0 complete
- Module 1 `Must-have` complete
- Module 2 `Must-have` complete
- Module 3 `Must-have` complete
- Module 4 `Must-have` complete
- Module 5 minimal live dashboard complete
- Simulation mode is fully validated
- All critical paths have been tested in both simulation mode and shadow mode before M3
- Small-capital deployment safeguards are active

### 3.2 Should-Have For v3

These should still be delivered within v3, but they are not blockers for the first limited live deployment:

- richer wallet scoring
- enhanced dashboard polish
- richer reports
- more advanced analytics and charts
- more resilient operator UX

### 3.3 Later

Items categorized as `Later` are explicitly deferred and should not block first live deployment or first public v3 milestone.

---

## 4. Current Repo To v3 Migration Decisions

The existing repo and the v3 PRD diverge in several important ways. These are not optional cleanup tasks; they are part of the v3 rework.

### 4.1 Storage

- **Current repo:** hybrid Redis + SQLite assumptions
- **v3 target:** SQLite-only as the system of record

Decision:

- Redis becomes optional or fully removed from the required runtime path.
- SQLite becomes the sole required local persistence layer for signals, trades, positions, targets, daily stats, and config.

### 4.2 Signal Source

- **Current repo:** file watcher, HTTP, Redis stream
- **v3 target:** Data API polling + WebSocket as primary ingestion

Decision:

- Data API polling and wallet activity tracking become the default path.
- WebSocket-based fast detection becomes the low-latency path.
- File-based/manual signal injection is retained only as a debug/manual input path.

### 4.3 Deployment

- **Current repo:** still carries Docker/Redis-era assumptions
- **v3 target:** Windows-native first, no Docker required

Decision:

- Windows-native local run is the primary deployment mode.
- Docker remains optional, not required.

### 4.4 Frontend

- **Current repo:** starter dashboard, mixed delivery path, partly served from backend
- **v3 target:** clean modern operator control panel

Decision:

- The dashboard is not a dense trading terminal.
- It must prioritize clarity, trust, health visibility, and safe controls.

---

## 5. Module 0: Setup & Deployment Prerequisites

This module is intentionally separate from the 5 product modules. It covers one-time onboarding/setup concerns that should not contaminate the core trading logic.

### 5.1 Deliverables

- `.env` and config validation at startup
- Polygon RPC connectivity validation
- EOA/proxy/safe wallet mode validation
- First-run approval detection and guided execution flow
- Derived CLOB API key generation and persistence
- Windows-native runbook
- Local health verification commands

### 5.2 Acceptance Criteria

- Startup fails fast with actionable error messages if required env/config is missing.
- Wallet type and signer configuration are validated before trading starts.
- Missing USDC/CTF approvals are detected before trading mode is allowed.
- Derived API credentials can be generated and persisted for reuse.
- A new user on Windows can follow the documented steps and reach a healthy simulation boot.

### 5.3 Must-Have For First Safe Live Deployment

- env/config validation
- wallet mode validation
- approval detection
- API key derivation and reuse
- Windows-native onboarding/run instructions

### 5.4 Should-Have For v3

- guided setup subcommand
- startup diagnostics report
- approval-status visibility in dashboard/Telegram

### 5.5 Later

- interactive setup wizard
- automatic RPC benchmarking and selection

---

## 6. Module 1: Core Trading Engine

### 6.1 Objective

Turn a validated signal into a correctly risk-checked, correctly-priced, correctly-signed Polymarket order with safe simulation and live execution modes.

### 6.2 Deliverables

- market context fetcher for token ID, tick size, min order size, `neg_risk`, market resolution state
- risk engine using:
  - confidence multiplier
  - secret-level multiplier
  - drawdown multiplier
  - category caps
  - max concurrent positions
  - min balance guard
  - circuit breaker logic
- order planner:
  - FOK default
  - GTC optional
  - slippage and price-buffer logic
  - final size clamping
- order signing and placement through official Rust SDK
- execution retry/backoff for retryable failures
- simulation mode with zero live-order side effects
- execution latency instrumentation
- emergency-stop hook integration

### 6.3 Acceptance Criteria

- Given a valid signal, the engine produces one deterministic execution decision.
- Orders respect live market metadata including token ID, tick size, and minimum order size.
- FOK orders are rejected rather than silently degraded into worse entries.
- Retryable failures follow capped exponential backoff with jitter.
- Non-retryable failures are recorded immediately.
- Simulation mode never places a real order.
- In simulation mode, no real HTTP/WebSocket calls are made to the Polymarket CLOB API.
- Daily loss, min balance, and circuit breaker conditions prevent new execution.
- Execution timestamps and latency metrics are emitted for every trade attempt.

### 6.4 Must-Have For First Safe Live Deployment

- market metadata lookup
- pricing and slippage guard
- risk sizing and hard limits
- FOK placement
- signing/auth via official SDK
- simulation/live split
- retry/backoff for 429 and 5xx
- emergency-stop integration

### 6.5 Should-Have For v3

- optional GTC path for non-urgent flows
- backup RPC selection logic
- partial-fill aware live lifecycle expansion
- order aggregation window for very small clustered signals

### 6.6 Later

- adaptive execution strategy
- stop-loss and take-profit automation
- advanced maker tactics
- multi-order batching heuristics

---

## 7. Module 2: Signal Ingestion & Wallet Tracker

### 7.1 Objective

Detect copyable target-wallet actions reliably and convert them into deduplicated, bounded-age internal signals.

### 7.2 Deliverables

- Data API poller for target wallet activity
- WebSocket fast-path ingestion for supported real-time events
- normalized internal signal model
- tx-hash deduplication
- staleness guard
- resolved-market and redeemable-market filtering
- category tagging
- target wallet config management
- target wallet category filtering
- baseline wallet scoring model

### 7.3 Acceptance Criteria

- A trade detected by both polling and WebSocket is processed only once.
- Signals older than `SIGNAL_MAX_AGE_SECS` are rejected.
- Signals for resolved or redeemable markets are rejected.
- Signals for already-owned positions are rejected according to the anti-duplication rule.
- Polling interval is configurable and safe by default.
- Target wallet additions/removals update the active watchlist correctly.

### 7.4 Must-Have For First Safe Live Deployment

- Data API polling
- normalized signal conversion
- deduplication by tx hash
- staleness guard
- target wallet list
- category filter support

### 7.5 Should-Have For v3

- WebSocket fast path
- baseline wallet scoring
- wallet score command integration

### 7.6 Later

- historical wallet calibration analytics
- automated wallet promotion/demotion
- leaderboard-assisted discovery workflow

---

## 8. Module 3: State, SQLite, and Reconciliation

### 8.1 Objective

Make local state trustworthy, recoverable, and auditable through SQLite-backed persistence and reconciliation.

### 8.2 Deliverables

- SQLite schema and migration system for:
  - `signals`
  - `trades`
  - `positions`
  - `targets`
  - `daily_stats`
  - `config`
- repository layer for reads/writes
- position model with anti-duplication ownership rule
- realized and unrealized PnL calculation
- balance and drawdown tracking
- startup recovery from SQLite
- reconciliation loop
- orphaned-position detection
- current-price refresh for open positions
- daily stats updates

### 8.3 Acceptance Criteria

- Signals, trades, and positions survive restart.
- Recovered state matches the last committed SQLite state.
- Open positions use current market prices for unrealized PnL.
- Closed positions move into realized PnL correctly.
- Reconciliation identifies:
  - on-chain/CLOB positions missing locally
  - local positions missing on-chain/CLOB
  - stale price/PnL state
- Daily stats accurately reflect balance, volume, drawdown, and trade counts for the current day.

### 8.4 Must-Have For First Safe Live Deployment

- schema + migrations
- durable signal/trade/position persistence
- startup recovery
- realized/unrealized PnL
- drawdown tracking
- reconciliation loop

### 8.5 Should-Have For v3

- richer orphan resolution flows
- explicit reconciliation audit log
- config table support for derived credentials and heartbeat metadata

### 8.6 Later

- more detailed attribution analytics
- archival compaction strategy
- richer performance history tools

---

## 9. Module 4: Telegram Operator Layer

### 9.1 Objective

Provide a safe remote operator interface for control, visibility, and emergency intervention.

### 9.2 Deliverables

- chat ID whitelist enforcement
- command parser and router
- 2-step confirmation for destructive commands
- status/report formatting
- target wallet management commands
- score command wiring
- pause/resume
- live/simulation mode switching with confirmation
- emergency stop
- daily/weekly reporting
- alerts for:
  - daily loss halt
  - repeated failures
  - startup problems
  - reconciliation mismatches

### 9.3 Acceptance Criteria

- Only whitelisted chat IDs receive responses.
- Destructive commands expire if not confirmed in time.
- `/status` reflects true system state.
- `/positions` reflects persisted open positions.
- `/signals` reflects recent real signals.
- `/emergency_stop` halts new signals and triggers flatten/cancel behavior.
- `/mode live` cannot succeed unless setup prerequisites are satisfied.

### 9.4 Must-Have For First Safe Live Deployment

- whitelist enforcement
- `/status`
- `/positions`
- `/signals`
- `/pause`
- `/resume`
- `/emergency_stop`
- `/wallet add`
- `/wallet remove`
- confirmation flow

### 9.5 Should-Have For v3

- `/wallet score`
- `/report daily`
- `/report weekly`
- `/mode sim`
- `/mode live`

### 9.6 Later

- richer report formatting
- alert routing preferences
- incident drill shortcuts

---

## 10. Module 5: Dashboard / Operator Panel

### 10.1 Objective

Ship a clean, modern operator control panel optimized for trust, monitoring, and fast safe intervention.

### 10.2 UI Direction

This is **not** a professional manual trading terminal.

It should feel like:

- calm
- modern
- legible
- reliable
- operationally focused
- trustworthy
- minimal design
- high contrast
- green = healthy, amber/red = alert
- clarity and quick scanning over dense information

It should avoid:

- noisy, dense terminal aesthetics
- unnecessary chart overload
- overwhelming simultaneous controls

### 10.3 Deliverables

#### Minimal Live Dashboard (launch blocker)

- status bar:
  - mode
  - balance
  - daily PnL
  - drawdown
- open positions panel
- recent signals panel
- system health panel
- execution log panel
- basic operator controls:
  - pause
  - resume
  - emergency stop

#### Full v3 Dashboard

- local WebSocket event stream from backend
- live updates without manual refresh
- daily stats charts
- target wallet panel
- richer execution telemetry
- reconciliation/alerts panel

### 10.4 Acceptance Criteria

- A user can understand whether the bot is healthy within 5 seconds of opening the dashboard.
- Open positions, recent signals, and system health are visible without navigation depth.
- Destructive controls are visually separated and safe.
- Live dashboard state matches backend operator state.
- The panel remains readable on laptop-sized screens.

### 10.5 Must-Have For First Safe Live Deployment

- minimal live dashboard
- health
- positions
- recent signals
- PnL + drawdown summary
- basic controls

### 10.6 Should-Have For v3

- live event streaming
- daily stats charts
- richer execution log
- improved visual polish

### 10.7 Later

- dense terminal mode
- replay tools
- advanced charting
- deep analytics workspace

---

## 11. Safe First Live Deployment Bundle

The exact launch bundle is:

- Module 0 must-have complete
- Module 1 must-have complete
- Module 2 must-have complete
- Module 3 must-have complete
- Module 4 must-have complete
- Module 5 minimal live dashboard complete

Additionally:

- Simulation mode must be proven stable.
- A limited-capital launch profile must exist.
- Emergency stop must be tested before live trading.
- Startup diagnostics must be understandable without code inspection.

---

## 12. Suggested Milestones

### M0: Setup Readiness

Goal:

- New machine can boot simulation mode successfully.

### M1: Simulation-Complete Backend

Goal:

- Modules 1, 2, and 3 must-have complete in simulation mode.

### M2: Operator-Complete Control Layer

Goal:

- Module 4 must-have complete, minimal Module 5 complete.

### M3: First Safe Live Deployment

Goal:

- small-capital live deployment with operational safeguards enabled.

### M4: Full v3 Completion

Goal:

- all `Should-have for v3` complete across all modules.

### M5: Post-v3 Expansion

Goal:

- `Later` features evaluated for v4.

---

## 13. Execution Rule

Implementation planning must now be written **module by module**, starting with Module 1.

No implementation plan should attempt to cover all 5 modules at once.

The next implementation planning step should produce:

- a dedicated Module 1 plan
- exact file-level scope
- acceptance-test-first execution order

Only after Module 1 is accepted should Module 2 planning begin.
