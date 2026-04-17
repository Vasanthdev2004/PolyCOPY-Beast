# SUPERFAST_POLY_BOT v2 — Complete Product Requirements Document (PRD)

**Document Version:** 2.5  
**Edition:** Hardened & Gap-Resolved Edition  
**Date:** April 14, 2026  
**Author:** vasanthmaster100 + AI Co-Author  
**Status:** Final Consolidated PRD — Single Source of Truth  
**Classification:** Confidential — For Internal Development Only

---

## 1. Executive Summary

SuperFast PolyBot v2 is a high-performance, self-hosted copy-trading system designed specifically for Polymarket's Central Limit Order Book (CLOB). The product's core competitive advantage is the user's proprietary "secret wallet" scanner, which identifies high-alpha on-chain traders before they appear on public leaderboards.

Unlike existing Telegram-based copy-trading bots that suffer from high latency (1.5–4 seconds), blind mirroring, poor risk management, and survivor bias, SuperFast PolyBot v2 introduces intelligent dynamic position sizing driven by scanner confidence scores and secret levels, combined with sub-400ms execution using official Rust SDKs.

The system will consist of four major user-facing components:
1. Real-time scanner signal ingestion
2. Intelligent risk and dynamic sizing engine
3. Ultra-low latency execution core
4. Professional monitoring suite (Leptos dashboard + Telegram control bot)

**Primary Objective:** Achieve sustainable positive expected value (+EV) by converting scanner alpha into risk-adjusted trades with superior speed and discipline.

**Target Latency:** Average end-to-end latency from scanner signal receipt to CLOB order placement must not exceed 400ms, with a stretch goal of under 250ms.

**Target User:** Sophisticated crypto trader who already operates a private scanner and seeks institutional-grade execution and risk infrastructure.

---

## 2. Business Objectives

### 2.1 Primary Objectives
- Transform raw scanner signals into intelligently sized, risk-controlled trades on Polymarket
- Achieve consistent outperformance versus naive copy-trading strategies
- Maintain full transparency and self-custody (non-custodial architecture)
- Provide professional-grade monitoring and control interfaces
- Establish an extensible platform for future cross-market arbitrage and advanced trading strategies

### 2.2 Success Metrics (KPIs)
- Execution latency: P95 < 400ms, average < 300ms
- Risk compliance: Zero daily loss breaches in simulation, < 2% live
- Sharpe ratio target: > 2.5 on validated secret wallets
- System uptime: 99.5% over 30-day rolling periods
- User control: All major functions accessible via both dashboard and Telegram
- Backtesting accuracy: > 92% match between simulation and live execution outcomes

---

## 3. Scope

### 3.1 In Scope
- Real-time ingestion of scanner signals via multiple methods (file system watcher, Redis stream, HTTP endpoint)
- Multi-factor risk engine using confidence, secret level, category, market exposure, and global drawdown
- Dynamic position sizing engine with configurable multipliers and safety bounds
- Polymarket CLOB execution using official Rust SDK with EIP-712 signing
- Position reconciliation and real-time PnL tracking
- Leptos-based web dashboard with live updates
- Telegram bot with full command set and alert system
- Simulation/replay mode for safe strategy validation
- Comprehensive logging, tracing, and performance monitoring
- Docker-based deployment architecture

### 3.2 Out of Scope (Phase 1–2)
- Kalshi or other prediction market integration
- Machine learning signal validation layer
- Mobile native applications
- Social features or community sharing
- Custodial wallet management
- Advanced derivatives or options trading

---

## 4. Functional Requirements

### 4.1 Scanner Integration Module

#### 4.1.1 Signal JSON Schema (NEW — v2.5)

All signals must conform to the following strict schema. The system must reject malformed signals and log validation errors.

```json
{
  "signal_id": "uuid-v4",
  "timestamp": "2026-04-14T10:00:00.000Z",
  "wallet_address": "0xABCDEF...",
  "market_id": "polymarket-clob-market-id",
  "side": "YES | NO",
  "confidence": 7,
  "secret_level": 6,
  "category": "politics | sports | crypto | other",
  "suggested_size_usdc": 50.00,
  "scanner_version": "1.0.0"
}
```

**Field Definitions:**

| Field | Type | Required | Constraints | Description |
|---|---|---|---|---|
| signal_id | string (UUID v4) | Yes | Must be unique | Deduplication key |
| timestamp | ISO 8601 string | Yes | Must be within ±30s of system clock | Signal creation time |
| wallet_address | string | Yes | Must be valid EVM address | Source wallet being copied |
| market_id | string | Yes | Must match active CLOB market | Target Polymarket market |
| side | enum | Yes | "YES" or "NO" only | Position direction |
| confidence | integer | Yes | 1–10 inclusive | Scanner confidence score |
| secret_level | integer | Yes | 1–10 inclusive | Wallet alpha score |
| category | enum | Yes | See allowed values | Market category tag |
| suggested_size_usdc | float | No | > 0, <= 10000.00 | Scanner size hint (overridden by risk engine) |
| scanner_version | string | No | Semver format | For schema evolution tracking |

**Validation Rules:**
- Signals with confidence < 3 OR secret_level < 3 must be queued for manual review, not auto-executed
- Signals with timestamp older than 30 seconds must be discarded with a warning log
- Duplicate signal_id within a 5-minute rolling window must be silently dropped

#### 4.1.2 Ingestion Methods
- Must support multiple ingestion methods (priority order: file watcher → Redis stream → HTTP endpoint)
- Must handle high-frequency signals (100+ per minute) without backpressure
- Must assign default category "other" if field is missing or unrecognized

---

### 4.2 Risk & Dynamic Sizing Engine

#### 4.2.1 Position Sizing Formula (NEW — v2.5)

```
final_size = base_size × confidence_multiplier × secret_level_multiplier × drawdown_multiplier
final_size = clamp(final_size, MIN_POSITION_USDC, MAX_POSITION_USDC)
```

**Base Size:**
- Default: 1.5% of total portfolio USDC balance at session start
- Recalculated every 15 minutes against live balance
- Hard minimum: $5.00 USDC
- Hard maximum: $500.00 USDC per position (overrideable via config)

**Confidence Multiplier Table:**

| Confidence Score | Multiplier |
|---|---|
| 1–3 | 0.0 (blocked — manual review queue) |
| 4 | 0.5 |
| 5 | 0.75 |
| 6 | 1.0 (baseline) |
| 7 | 1.25 |
| 8 | 1.5 |
| 9 | 1.75 |
| 10 | 2.0 |

**Secret Level Multiplier Table:**

| Secret Level | Multiplier |
|---|---|
| 1–3 | 0.0 (blocked — manual review queue) |
| 4 | 0.6 |
| 5 | 0.8 |
| 6 | 1.0 (baseline) |
| 7 | 1.3 |
| 8 | 1.6 |
| 9 | 1.9 |
| 10 | 2.5 |

**Drawdown Multiplier Curve:**

| Portfolio Drawdown | Multiplier |
|---|---|
| 0–5% | 1.0 (no reduction) |
| 5–10% | 0.75 |
| 10–15% | 0.5 |
| 15–20% | 0.25 |
| > 20% | 0.0 (auto-pause, requires manual resume) |

#### 4.2.2 Exposure Limits

| Limit Type | Default Value | Configurable |
|---|---|---|
| Global daily loss limit | 5% of portfolio | Yes |
| Per-market max exposure | 10% of portfolio | Yes |
| Per-category max exposure | 25% of portfolio | Yes |
| Max concurrent open positions | 20 | Yes |
| Max position size as % of market liquidity | 2% | Yes (critical for slippage) |

- System must auto-pause trading when global daily loss limit is breached
- System must resume only on explicit `/resume` command after daily loss breach
- Emergency stop must close all open positions as market orders within 10 seconds

#### 4.2.3 Risk Profiles Per Category

| Category | Max Single Position | Confidence Threshold | Secret Level Threshold |
|---|---|---|---|
| politics | $250 USDC | 6 | 5 |
| crypto | $150 USDC | 7 | 6 |
| sports | $200 USDC | 6 | 5 |
| other | $100 USDC | 7 | 7 |

---

### 4.3 Execution Engine

#### 4.3.1 Polymarket CLOB API — Verified Constraints (NEW — v2.5)

Based on current CLOB API documentation and behavior:

- **Supported order types:** Limit (GTC), IOC, FOK. Post-only is supported but must be validated against current SDK version before use.
- **Rate limits:** 100 requests/minute per API key on order placement; 300 requests/minute on reads. Circuit breaker must activate at 80% of rate limit.
- **Minimum order size:** $1.00 USDC equivalent
- **Price precision:** 2 decimal places (e.g., 0.65, not 0.6523)
- **Size precision:** 2 decimal places in USDC
- **WebSocket heartbeat:** Must send ping every 30 seconds; reconnect if no pong within 10 seconds

#### 4.3.2 Execution Flow

1. Signal received and validated
2. Risk engine calculates final_size and checks all exposure limits
3. Fetch current orderbook snapshot via WebSocket
4. Validate slippage: if estimated fill price deviates > 2% from mid, discard and log
5. Construct order with EIP-712 signing
6. Submit to CLOB via Rust SDK
7. Track order state until filled, cancelled, or timed out (30s timeout)
8. Emit execution event to state module

#### 4.3.3 Failover & Resilience
- Must support minimum 2 RPC endpoints with automatic failover on timeout (> 2s)
- Warm WebSocket connection must be maintained with exponential backoff reconnection (1s, 2s, 4s, 8s, max 60s)
- All external API calls must have a 5-second hard timeout

---

### 4.4 State & Reconciliation Module

#### 4.4.1 Reconciliation Policy (NEW — v2.5)

**Source of Truth Hierarchy:**
1. On-chain state (Polygon) — always authoritative
2. CLOB API order status — secondary confirmation
3. Redis cache — fast-access view, not authoritative

**Reconciliation Intervals:**
- Light reconciliation (Redis vs CLOB API): every 30 seconds
- Full reconciliation (Redis vs on-chain): every 5 minutes
- Emergency reconciliation: triggered on any execution error

**Divergence Handling:**
- If Redis shows a position that CLOB/on-chain does not: mark as "ghost position," alert operator, do not trade against it
- If on-chain shows a position not in Redis: rebuild Redis entry from on-chain data, emit alert
- Manual override command: `/reconcile force` available via Telegram and dashboard

#### 4.4.2 General Requirements
- Must calculate realized and unrealized PnL with high precision (6 decimal places)
- Must persist all trade history to disk (SQLite or append-only log) for auditing
- Must support Redis as primary fast cache with persistent storage fallback

---

### 4.5 User Interface Requirements

#### 4.5.1 Leptos Dashboard
- Live signal feed with color-coded confidence and secret level
- Real-time portfolio equity curve
- Active positions table with unrealized PnL and latency metrics
- Risk dashboard showing current exposure, daily P&L, drawdown, and category allocation
- Latency heatmap and performance metrics
- Backtesting replay interface with adjustable speed
- System health monitoring (connections, latency, error rates)
- Manual override controls (pause, emergency stop, single trade approval)

#### 4.5.2 Telegram Bot Interface (NEW — v2.5 Security Spec)

**Authentication:**
- Whitelist of allowed Telegram user IDs stored in environment variable `TELEGRAM_ALLOWED_USER_IDS` (comma-separated)
- All commands silently ignored if user ID not in whitelist
- Whitelist checked on every message, not just at bot startup

**Destructive Commands — Two-Step Confirmation Required:**

| Command | Confirmation Required |
|---|---|
| /emergency_stop | Must reply /confirm within 30 seconds |
| /wallet remove | Must reply /confirm within 30 seconds |
| /resume (after daily loss breach) | Must reply /confirm within 30 seconds |

**Full Command Set:**

| Command | Description |
|---|---|
| /status | System health, uptime, active positions count |
| /positions | Table of open positions with PnL |
| /signals | Last 10 signals received and their disposition |
| /pause | Pause signal processing (positions remain open) |
| /resume | Resume signal processing (requires /confirm if after loss breach) |
| /emergency_stop | Close all positions as market orders (requires /confirm) |
| /wallet add [address] | Add wallet to copy list |
| /wallet remove [address] | Remove wallet (requires /confirm) |
| /report [daily/weekly] | Performance summary |
| /config [key] [value] | Modify risk parameters at runtime |
| /reconcile force | Force full state reconciliation |
| /ratelimit | Show current API rate limit usage |

**Rate Limiting on Commands:**
- Maximum 30 commands per minute per user
- Maximum 3 /emergency_stop commands per hour (prevent accidental spam)

**Alerts (Automatic):**
- Signal received with secret_level >= 8
- Trade executed (with latency and size)
- Risk limit breach (with details)
- Daily loss limit breach
- WebSocket disconnection lasting > 30 seconds
- System error (with stack trace summary)
- Daily and weekly performance digest (scheduled)

---

## 5. Non-Functional Requirements

### 5.1 Performance
- 99th percentile end-to-end latency < 400ms
- Scanner ingestion to risk decision < 50ms
- Risk decision to signed order < 150ms
- Signed order to CLOB submission < 200ms
- System must handle 100+ signals per minute without degradation

### 5.2 Reliability & Resilience
- Automatic reconnection to all external services (WebSocket, RPC, Redis)
- Graceful degradation when individual components fail
- Circuit breakers on all external API calls (activate at 80% rate limit or 3 consecutive failures)
- Comprehensive error handling and retry logic with exponential backoff
- Daily automated backup of all state and trade history

### 5.3 Security
- All private keys and API credentials must be loaded exclusively from environment variables
- No private keys may ever be written to disk or logs — enforced via log scrubbing middleware
- All external API calls must use HTTPS and validated certificates
- Rate limiting on all external services
- Secure session management for dashboard and Telegram bot
- Telegram auth via user ID whitelist (see 4.5.2)

### 5.4 Scalability
- System must support scaling from 1 to 50 followed wallets without architectural changes
- Database and cache layers must handle at least 1M trade records efficiently

---

## 6. Architecture Overview

The system follows a modular, event-driven architecture using Tokio for async runtime. All hot-path components are implemented in Rust for maximum performance. The dashboard uses Leptos for reactive Rust-based frontend. State is shared via Redis for both speed and persistence.

Major components communicate via typed channels and events. All business logic is isolated in pure functions where possible to maximize testability and auditability.

---

## 7. Assumptions & Dependencies

### 7.1 Assumptions
- User's scanner produces clean, well-formed JSON signals conforming to the schema in Section 4.1.1
- User has access to a dedicated Polymarket trading wallet with sufficient USDC balance (minimum $500 recommended for live deployment)
- Post-only order type availability must be verified against current Polymarket Rust SDK before Phase 3 implementation
- User has valid xAI API access for any auxiliary AI features (future)
- User has basic Rust development environment for initial setup

### 7.2 External Dependencies
- Polymarket CLOB API and WebSocket endpoints (rate limits: 100 writes/min, 300 reads/min)
- Polygon RPC providers — minimum 2 providers required (Alchemy + Infura recommended)
- Redis instance for state management
- Telegram Bot API

---

## 8. Operational Runbook (NEW — v2.5)

### 8.1 Go-Live Capital Allocation Strategy
- Week 1 (live): Maximum $200 USDC total capital at risk. Max position size $10 USDC.
- Week 2–3: Scale to $1,000 USDC if Sharpe > 1.5 and zero risk breaches in Week 1
- Week 4+: Scale to full capital if Week 2–3 metrics are met

### 8.2 Adding / Removing Wallets Mid-Session
1. Use `/pause` command to halt new signal processing
2. Use `/wallet add [address]` or `/wallet remove [address]` (with /confirm)
3. Wait for confirmation message from bot
4. Use `/resume` to restart signal processing
5. Do NOT modify wallet list while positions from that wallet are open

### 8.3 Incident Response — Runaway Position
1. Immediately issue `/emergency_stop` followed by `/confirm`
2. Bot will attempt to close all positions as market orders within 10 seconds
3. If bot is unresponsive, manually access Polymarket UI and close positions
4. After incident, run `/reconcile force` to verify state integrity
5. Review logs for root cause before resuming

### 8.4 Redis State Backup & Restore
- Daily automated backup of Redis snapshot to `/backups/redis-YYYY-MM-DD.rdb`
- Retain 7 days of backups
- Restore procedure: `redis-cli RESTORE` from latest `.rdb`, then run `/reconcile force`

### 8.5 Monitoring Checklist (Daily)
- Check WebSocket connection uptime in dashboard
- Review daily P&L against target
- Verify no ghost positions in reconciliation log
- Confirm API rate limit headroom (should be < 60% of limit on average)

---

## 9. Risks & Mitigation Strategies

### High Priority Risks
- **Latency creep:** Mitigation — continuous performance monitoring + latency tracing at every stage
- **Risk engine bypass:** Mitigation — strict validation at multiple layers with immutable audit log
- **WebSocket disconnections:** Mitigation — automatic reconnection with exponential backoff and circuit breaker
- **Scanner signal quality degradation:** Mitigation — minimum confidence/secret level thresholds with manual review mode
- **Reconciliation divergence:** Mitigation — on-chain always authoritative; ghost position alerts (NEW)

### Medium Priority Risks
- RPC provider instability — mitigated by minimum 2 providers with failover
- Regulatory changes in prediction markets
- Polymarket API or CLOB format changes — mitigated by SDK version pinning and change log monitoring
- Slippage on large positions — mitigated by max 2% of market liquidity per position cap (NEW)

---

## 10. Development Roadmap

**Phase 0:** Research & PRD Finalization (Completed)  
**Phase 1:** Core Rust foundation, scanner ingestion, risk engine, basic execution, simulation mode (Completed)  
**Phase 2:** Real-time file watcher, full Telegram bot, Redis state layer, initial Leptos dashboard (In Progress)  
**Phase 3:** Advanced latency optimization, orderbook integration, comprehensive backtesting suite, production hardening (Weeks 5-8)  
**Phase 4:** Live deployment with small capital per runbook Section 8.1, monitoring refinement, documentation, performance tuning (Weeks 9-10)

**Total Projected Timeline:** 10–12 weeks to initial live deployment with conservative capital.

---

## 11. Appendix

### 11.1 Glossary
- **CLOB:** Central Limit Order Book — Polymarket's order matching engine
- **Secret Wallet:** High-alpha trader address detected by user's private scanner
- **Secret Level:** Proprietary 1-10 score indicating confidence in the wallet's edge
- **FOK/IOC:** Fill-Or-Kill / Immediate-Or-Cancel order types
- **Ghost Position:** A position recorded in Redis that has no corresponding on-chain state
- **EIP-712:** Ethereum typed structured data signing standard used for CLOB order authentication

### 11.2 Version History
- v2.0 — Initial massive research document
- v2.1 — Added UI/UX and Telegram specifications
- v2.2 — Incorporated Phase 1 implementation feedback
- v2.3 — Added detailed non-functional requirements and risk section
- v2.4 — Consolidated into final clean PRD format
- v2.5 — Added signal JSON schema, sizing multiplier tables, CLOB API constraints, reconciliation policy, Telegram auth spec, operational runbook

**End of Document**
