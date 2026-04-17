# SuperFast PolyBot v3.0 — Enhanced PRD

**Project Name:** SuperFast PolyBot v3.0  
**Edition:** Deep Research & Windows-First Edition  
**Date:** April 17, 2026  
**Status:** Final – Ready for Implementation  
**Target:** Windows Native + High Performance Copy Trading on Polymarket CLOB  
**Research Base:** 15+ active repos including `polycopier`, `HyperBuildX/Polymarket-Trading-Bot-Rust`,
`gamma-trade-lab/polymarket-copy-trading-bot`, `Xyryllium/polymarket-tracker-bot`,
`Novus-Tech-LLC/Polymarket-Copytrading-Bot`, official `Polymarket/rs-clob-client` SDK,
Quicknode copy bot guide, and Polymarket rate limit & API documentation (April 2026).

---

## 1. Executive Summary

SuperFast PolyBot v3.0 is a **self-hosted, high-performance copy-trading system** built specifically
for Polymarket's Central Limit Order Book (CLOB) in 2026.

After deep analysis of 15+ active open-source repos, the official Polymarket Rust SDK, live rate
limits, and real-world latency benchmarks, this PRD delivers the most practical yet powerful solution
for a solo developer running on Windows.

### Key Advantages

- Zero heavy dependencies — no Redis, no Docker required on Windows
- Dual signal ingestion: WebSocket (real-time) + Data API polling (fallback)
- Target wallet tracking with performance-scored selection
- Realistic low latency — target 340–550 ms end-to-end (consistent with live bots)
- Intelligent risk engine with dynamic sizing, circuit breakers, and daily loss halt
- Professional Leptos live dashboard with WebSocket health metrics
- Safe Telegram control with chat ID whitelist and 2-step confirmation
- Full simulation mode before going live

---

## 2. Deep Research Summary (April 2026)

| Area | Key Finding | Decision for v3.0 |
|---|---|---|
| Language | Rust dominates serious bots; measurable edge vs Python/TS in live conditions | Keep Rust + Tokio |
| Storage | Most practical bots use SQLite; RocksDB only for extreme throughput | **SQLite only** (`polybot.db`) |
| Signal Source | Data API (2 s poll) + WebSocket user channel — NOT file drop | **Dual-source ingestion** |
| Target Tracking | Data API `/activity` endpoint + tx-hash dedup across multiple wallets | Wallet tracker module |
| Execution | Official `rs-clob-client` SDK + EIP-712 signing; FOK for taker, GTC for maker | Use latest SDK |
| Order Removal of Delay | Polymarket removed artificial 500 ms taker delay in Feb 2026 | FOK orders now realistic |
| Latency (Realistic) | 340–550 ms achievable on laptop (polycopybot.app reports 340 ms median) | Target **< 550 ms** |
| Rate Limits | POST /order: 3,500/10 s burst, 36,000/10 min sustained; Data API: 1,000/10 s | Implement backoff |
| WebSocket Limits | 5 concurrent WS connections per IP; subscribe up to 10 instruments per socket | Pool WS connections |
| Dashboard | Leptos performs excellently for Rust-native reactive UI | Keep Leptos |
| Wallet Selection | ROI alone is misleading; Sharpe ratio + calibration accuracy are better signals | Score wallets on 5+ metrics |

---

## 3. Architecture Overview

```
┌─────────────────────────────────────────────────────────────────┐
│                     SuperFast PolyBot v3.0                      │
│                                                                  │
│  ┌──────────────────┐    ┌───────────────────────────────────┐  │
│  │  Signal Ingestion │    │         Risk & Sizing Engine      │  │
│  │                  │    │                                   │  │
│  │  WS User Channel ├───►│  confidence × secret_level        │  │
│  │  Data API (2s)   │    │  × drawdown multiplier            │  │
│  │  Hash dedup      │    │  → clamp(size, $5, max_pos)       │  │
│  └────────┬─────────┘    └───────────────┬───────────────────┘  │
│           │                              │                      │
│           ▼                              ▼                      │
│  ┌──────────────────┐    ┌───────────────────────────────────┐  │
│  │  Wallet Tracker  │    │       CLOB Execution Engine       │  │
│  │                  │    │                                   │  │
│  │  Multi-target    │    │  Fetch price → size → sign (EIP   │  │
│  │  Perf scoring    │    │  -712) → POST /order (FOK/GTC)    │  │
│  │  Category filter │    │  → confirm fill → update DB       │  │
│  └──────────────────┘    └───────────────┬───────────────────┘  │
│                                          │                      │
│           ┌──────────────────────────────┤                      │
│           ▼                              ▼                      │
│  ┌──────────────────┐    ┌───────────────────────────────────┐  │
│  │  Leptos Dashboard│    │         SQLite (polybot.db)       │  │
│  │                  │    │                                   │  │
│  │  Live PnL        │    │  signals / trades / positions     │  │
│  │  Positions       │    │  targets / config / daily_stats   │  │
│  │  System Health   │    └───────────────────────────────────┘  │
│  └──────────────────┘                                           │
│                                                                  │
│  ┌──────────────────────────────────────────────────────────┐   │
│  │  Telegram Bot  (chat ID whitelist + 2-step confirmation)  │   │
│  └──────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────┘
```

| Component | Technology |
|---|---|
| Language | Rust 1.70+ + Tokio async runtime |
| Storage | SQLite via `sqlx` (`polybot.db`) |
| Signal Ingestion | Polymarket Data API + WebSocket user channel |
| Execution | Official `polymarket-client-sdk` (`rs-clob-client`) |
| Price Feed | CLOB WebSocket `wss://ws-subscriptions-clob.polymarket.com/ws/market` |
| UI | Leptos (reactive Rust/WASM frontend) |
| Control | Telegram Bot (chat ID whitelist + `/confirm` safety) |
| Deployment | Native `cargo run --release` on Windows |

---

## 4. Polymarket API Layer (Research-Backed)

### 4.1 Three APIs in Play

| API | Base URL | Auth Needed | Purpose |
|---|---|---|---|
| Gamma API | `https://gamma-api.polymarket.com` | No | Market discovery, metadata, prices |
| CLOB API | `https://clob.polymarket.com` | Yes (EIP-712 + HMAC) | Order placement, cancellation, orderbook |
| Data API | `https://data-api.polymarket.com` | No | Wallet activity, positions, leaderboard |

### 4.2 Rate Limits (March 2026 — Cloudflare Throttle-Based)

Polymarket uses Cloudflare throttling — requests over the limit are **queued and delayed**, not immediately rejected. A `429` only appears if the queue itself is full.

| Endpoint | Burst Limit | Sustained Limit | Notes |
|---|---|---|---|
| General REST | 15,000 / 10 s | — | Global cap |
| CLOB General | 9,000 / 10 s | — | |
| `POST /order` | 3,500 / 10 s | 36,000 / 10 min | Primary trading endpoint |
| `DELETE /order` | 3,000 / 10 s | 30,000 / 10 min | |
| `POST /orders` (batch) | 1,000 / 10 s | 15,000 / 10 min | Prefer for multi-order flows |
| Gamma API | 4,000 / 10 s | — | |
| Data API | 1,000 / 10 s | — | Target wallet polling lives here |
| WebSocket | 5 concurrent / IP | ≤ 10 instruments / socket | Pool connections carefully |

**Retry strategy:** Exponential backoff with jitter. Start at 1 s, double on each retry, cap at 60 s. Add random jitter ± 20% to prevent thundering-herd from multiple bots.

### 4.3 WebSocket Endpoints

| Channel | URL | Use |
|---|---|---|
| Market channel | `wss://ws-subscriptions-clob.polymarket.com/ws/market` | Real-time orderbook + price feed |
| User channel | `wss://ws-subscriptions-clob.polymarket.com/ws/user` | Fill confirmations for our own orders |

Subscribe message format:
```json
{
  "type": "market",
  "assets_id": "<token_id>"
}
```

The bot maintains heartbeat messages automatically via the SDK's built-in `clob` feature — if the client disconnects, all open orders are cancelled server-side as a safety measure.

---

## 5. Authentication & Wallet Setup

### 5.1 Wallet Types

| Type | Signature Type | When to Use |
|---|---|---|
| EOA (standard wallet) | `SignatureType::EOA` (0) | Simplest setup; private key in `.env` |
| Proxy wallet (Magic/email) | `SignatureType::Proxy` (1) | If using Polymarket's email login |
| Gnosis Safe | `SignatureType::GnosisSafe` (2) | Advanced; funder address must be set explicitly |

**Recommendation for v3.0:** Use EOA (`signature_type = 0`) for simplicity. The SDK auto-derives the Polymarket proxy wallet address via `CREATE2` from your EOA — no manual address entry needed.

### 5.2 First-Run Approval Transactions

On first run in EOA mode, the bot must send two Polygon transactions:
1. Approve USDC.e spender (CTF Exchange contract)
2. Approve CTF token spender

These are one-time on-chain approvals. The bot should detect and execute them automatically at startup if not already present. Require `MIN_PRIORITY_FEE_GWEI` and `MIN_MAX_FEE_GWEI` config values to ensure they go through.

### 5.3 API Key Derivation

CLOB API keys (key, secret, passphrase) are derived deterministically from the private key via a signed challenge. The SDK handles this at startup:

```rust
let client = Client::new("https://clob.polymarket.com", Config::default())?
    .authentication_builder(&signer)
    .authenticate()
    .await?;

let api_keys = client.api_keys().await?;
// Store these in SQLite config table for reuse across restarts
```

---

## 6. Environment Variables (Complete `.env` Reference)

```env
# === Core Identity ===
POLYMARKET_PRIVATE_KEY=0xYOUR_EOA_PRIVATE_KEY
FUNDER_ADDRESS=                          # Optional: only for GnosisSafe/Proxy mode

# === Polymarket Endpoints ===
CLOB_API_URL=https://clob.polymarket.com
GAMMA_API_URL=https://gamma-api.polymarket.com
DATA_API_URL=https://data-api.polymarket.com
WS_CLOB_URL=wss://ws-subscriptions-clob.polymarket.com/ws/market
WS_USER_URL=wss://ws-subscriptions-clob.polymarket.com/ws/user

# === Polygon RPC ===
POLYGON_RPC_URL=https://polygon-mainnet.your-provider.com/YOUR_KEY
POLYGON_CHAIN_ID=137

# === Signal Ingestion (Target Wallets) ===
TARGET_WALLETS=0xWALLET1,0xWALLET2,0xWALLET3
POLL_INTERVAL_MS=2000                    # Data API poll interval (2000 ms recommended)
USE_WEBSOCKET=true                       # Enable WS user channel for faster detection
SIGNAL_MAX_AGE_SECS=30                   # Ignore signals older than this (stale protection)

# === Order Execution ===
ORDER_TYPE=FOK                           # FOK (taker) or GTC (maker)
SLIPPAGE_TOLERANCE=0.02                  # 2% max price deviation before rejecting
PRICE_BUFFER=0.01                        # Extra 1% buffer on FOK price to improve fill rate
POSITION_MULTIPLIER=0.1                  # Copy at 10% of target's size (scale to your balance)

# === Risk Limits ===
MAX_TRADE_SIZE_USDC=150.0
MIN_TRADE_SIZE_USDC=5.0
MAX_CONCURRENT_POSITIONS=20
MAX_DAILY_LOSS_PCT=5.0                   # Auto-pause when daily drawdown hits 5%
MAX_DAILY_VOLUME_USDC=0                  # 0 = disabled
MAX_CONSECUTIVE_LOSSES=5                 # Circuit breaker; 0 = disabled
LOSS_COOLDOWN_SECS=3600                  # Pause duration after circuit breaker fires
MIN_USDC_BALANCE=20.0                    # Bot pauses if balance falls below this

# === Category Caps ===
MAX_POSITION_POLITICS_USDC=250.0
MAX_POSITION_CRYPTO_USDC=150.0
MAX_POSITION_SPORTS_USDC=200.0
MAX_POSITION_OTHER_USDC=100.0

# === Gas (Polygon) ===
MIN_PRIORITY_FEE_GWEI=30
MIN_MAX_FEE_GWEI=60

# === Telegram Bot ===
TELEGRAM_BOT_TOKEN=YOUR_BOT_TOKEN
TELEGRAM_ALLOWED_CHAT_IDS=123456789,987654321   # Comma-separated chat IDs

# === Mode ===
SIMULATION_MODE=true                     # MUST start true; flip to false after validation
LOG_LEVEL=info                           # trace | debug | info | warn | error
```

---

## 7. Signal Ingestion Architecture (Dual-Source)

The original PRD described a file-system watcher which is insufficient for copy trading latency requirements. Real-world bots use a **dual-source architecture**:

### 7.1 Primary Source — Data API Polling

```
Loop every POLL_INTERVAL_MS (default 2000 ms):
  GET https://data-api.polymarket.com/activity
    ?user=<target_wallet>
    &type=TRADE
    &limit=100
    &sortBy=TIMESTAMP
    &sortDirection=DESC
    &start=<last_seen_timestamp>

  For each trade returned:
    → Hash-dedup: skip if tx_hash already in signals table
    → Age filter: skip if timestamp < (now - SIGNAL_MAX_AGE_SECS)
    → Enqueue to signal_channel (mpsc::Sender<TradeEvent>)
```

### 7.2 Secondary Source — WebSocket User Channel

When `USE_WEBSOCKET=true`, the bot also subscribes to the market-level WebSocket channel for each token the target wallet holds. This provides sub-100 ms event detection vs the 2 s polling window.

```
Connect: wss://ws-subscriptions-clob.polymarket.com/ws/market
Subscribe to each token_id the target holds
On event received → same hash-dedup → enqueue to signal_channel
```

### 7.3 Deduplication

Both sources feed the same `signal_channel`. A `HashSet<TxHash>` in memory (with SQLite persistence for restart recovery) ensures each trade event is processed exactly once regardless of which source detected it first.

### 7.4 Signal Staleness Guard

A signal is **rejected** if:
- Its `timestamp` is older than `SIGNAL_MAX_AGE_SECS` (configurable, default 30 s)
- The market's `end_date` has already passed (resolved market)
- The market's `redeemable = true` flag is set (settled on-chain)
- The same `(target_wallet, market_id, side)` triple is already tracked as an open position

---

## 8. Signal JSON Schema (Final)

This schema is used internally when signals arrive from the wallet tracker. For external/manual injection the file watcher on `./signals/` can also parse this format.

```json
{
  "signal_id": "uuid-v4",
  "source": "websocket" | "polling" | "manual",
  "tx_hash": "0x...",
  "timestamp": "2026-04-17T13:45:22.123Z",
  "target_wallet": "0x...",
  "market_id": "clob-market-id",
  "token_id": "erc1155-token-id",
  "side": "BUY" | "SELL",
  "outcome": "YES" | "NO",
  "target_size_usdc": 500.0,
  "target_price": 0.62,
  "confidence": 8,
  "secret_level": 9,
  "category": "politics" | "crypto" | "sports" | "other",
  "suggested_size_usdc": 75.0
}
```

---

## 9. Order Execution Flow (Step-by-Step)

This was entirely missing from the original PRD. Every copy trade follows this sequence:

```
Step 1 — Signal received from signal_channel
         ↓
Step 2 — Staleness check (age, market resolved, duplicate)
         ↓ PASS
Step 3 — Risk engine: compute final_size
         ↓ final_size >= MIN_TRADE_SIZE_USDC
Step 4 — Fetch current best price
         GET /price?token_id=<id>&side=BUY  (parallel with Step 3)
         ↓
Step 5 — Slippage check
         |fetched_price - signal_price| / signal_price <= SLIPPAGE_TOLERANCE
         ↓ PASS
Step 6 — Build order payload
         token_id, side, price = fetched_price + PRICE_BUFFER,
         size = final_size, type = FOK (or GTC)
         ↓
Step 7 — Sign order with EIP-712 via rs-clob-client
         ↓
Step 8 — POST /order → CLOB API
         ↓
Step 9a — 200 OK + fill_id received
          → Write to trades table (status = FILLED)
          → Update positions table
          → Update daily_stats table (volume, pnl_unrealized)
          ↓
Step 9b — 200 OK but NOT_MATCHED (FOK rejected)
          → Write to trades table (status = REJECTED)
          → Log: "FOK not matched at price X"
          → Optionally retry as GTC with tighter size
          ↓
Step 9c — 429 / 5xx
          → Exponential backoff (1 s × 2^n, max 60 s, ± 20% jitter)
          → Max 3 retries before dropping signal
          → Write to trades table (status = FAILED)
          → Emit alert if 3+ consecutive failures
```

### Order Types Explained

| Type | Behaviour | When to Use |
|---|---|---|
| FOK (Fill or Kill) | Must fill 100% immediately at price or cancel | Copy trades — you want the same entry or nothing |
| GTC (Good Till Cancelled) | Rests in the book until filled or manually cancelled | Maker position sizing when not urgent |

**Default for v3.0:** `FOK` for all copy trades. If a FOK is rejected due to price slippage, the signal is discarded — never chase with a worse GTC entry.

---

## 10. Risk & Dynamic Sizing Engine

### 10.1 Formula

```rust
final_size = base_size
           × confidence_multiplier(signal.confidence)
           × secret_level_multiplier(signal.secret_level)
           × drawdown_multiplier(current_drawdown_pct)

final_size = clamp(final_size, MIN_TRADE_SIZE_USDC, category_max_position)
```

Where `base_size = target_size_usdc × POSITION_MULTIPLIER`.

### 10.2 Confidence Multiplier

| Confidence | Multiplier |
|---|---|
| 1–3 | 0.00 (signal rejected) |
| 4 | 0.50 |
| 5 | 0.75 |
| 6 | 1.00 |
| 7 | 1.25 |
| 8 | 1.50 |
| 9 | 1.75 |
| 10 | 2.00 |

### 10.3 Secret Level Multiplier

| Secret Level | Multiplier |
|---|---|
| 1–3 | 0.00 (signal rejected) |
| 4 | 0.60 |
| 5 | 0.80 |
| 6 | 1.00 |
| 7 | 1.30 |
| 8 | 1.60 |
| 9 | 1.90 |
| 10 | 2.50 |

### 10.4 Drawdown Multiplier

| Daily Drawdown | Multiplier |
|---|---|
| 0–5% | 1.00 |
| 5–10% | 0.75 |
| 10–15% | 0.50 |
| 15–20% | 0.25 |
| > 20% | 0.00 (full pause) |

### 10.5 Hard Limits

| Limit | Default | Behaviour on Breach |
|---|---|---|
| Daily loss limit | 5% of balance | Auto-pause all trading |
| Max concurrent positions | 20 | New signals dropped until a position closes |
| Consecutive loss circuit breaker | 5 losses | Pause for `LOSS_COOLDOWN_SECS` (1 hour) |
| Min USDC balance | $20 | Auto-pause; alert via Telegram |
| Micro-trade filter | < $1.00 notional | Reject — likely spoofing signal |
| Politics position cap | $250 per market | |
| Crypto position cap | $150 per market | |
| Sports position cap | $200 per market | |
| Other position cap | $100 per market | |

### 10.6 Anti-Duplication Rule

The bot **never enters the same token from two different target wallets**. The first target to trigger a signal for a given `token_id` "owns" that position in the ledger. All subsequent signals for the same token are ignored until the position is fully closed. This prevents double-sizing a position from correlated traders.

---

## 11. Target Wallet Selection & Scoring

Blindly following any wallet is fragile. The bot scores candidate target wallets on these criteria before adding them to the watchlist:

| Signal | Why It Matters |
|---|---|
| ROI (30-day) | Raw return — baseline measure |
| Win rate | % of positions that closed profitably |
| Sharpe ratio | Risk-adjusted return; distinguishes consistent edge from lucky streaks |
| Calibration accuracy | Does the wallet's position size reflect accurate probability estimates? |
| Category specialisation | Best to follow a wallet in its historically strong categories only |
| Trade frequency | Very low frequency = fragile sample; very high = may be bot, not alpha |

The Telegram `/wallet score [address]` command triggers an on-demand scoring run via the Data API leaderboard endpoint.

---

## 12. SQLite Database Schema

### 12.1 `signals` Table

```sql
CREATE TABLE signals (
    id            TEXT PRIMARY KEY,    -- uuid-v4
    source        TEXT NOT NULL,       -- 'websocket' | 'polling' | 'manual'
    tx_hash       TEXT UNIQUE,         -- Dedup key
    received_at   TEXT NOT NULL,       -- ISO8601 timestamp
    target_wallet TEXT NOT NULL,
    market_id     TEXT NOT NULL,
    token_id      TEXT NOT NULL,
    side          TEXT NOT NULL,       -- 'BUY' | 'SELL'
    outcome       TEXT NOT NULL,       -- 'YES' | 'NO'
    target_price  REAL NOT NULL,
    target_size   REAL NOT NULL,
    confidence    INTEGER NOT NULL,
    secret_level  INTEGER NOT NULL,
    category      TEXT NOT NULL,
    status        TEXT NOT NULL        -- 'pending' | 'executed' | 'rejected' | 'stale'
);
```

### 12.2 `trades` Table

```sql
CREATE TABLE trades (
    id            TEXT PRIMARY KEY,
    signal_id     TEXT NOT NULL REFERENCES signals(id),
    order_id      TEXT,               -- CLOB order ID from Polymarket
    placed_at     TEXT NOT NULL,
    filled_at     TEXT,
    market_id     TEXT NOT NULL,
    token_id      TEXT NOT NULL,
    side          TEXT NOT NULL,
    order_type    TEXT NOT NULL,      -- 'FOK' | 'GTC'
    requested_size REAL NOT NULL,
    filled_size   REAL,
    requested_price REAL NOT NULL,
    fill_price    REAL,
    fee_usdc      REAL DEFAULT 0,
    status        TEXT NOT NULL,      -- 'pending' | 'filled' | 'rejected' | 'failed'
    retry_count   INTEGER DEFAULT 0,
    error_msg     TEXT
);
```

### 12.3 `positions` Table

```sql
CREATE TABLE positions (
    token_id       TEXT PRIMARY KEY,
    market_id      TEXT NOT NULL,
    outcome        TEXT NOT NULL,     -- 'YES' | 'NO'
    category       TEXT NOT NULL,
    size_usdc      REAL NOT NULL,
    avg_entry_price REAL NOT NULL,
    current_price  REAL,
    unrealized_pnl REAL,
    opened_at      TEXT NOT NULL,
    last_updated   TEXT NOT NULL,
    owned_by_wallet TEXT NOT NULL,    -- Which target wallet triggered this
    status         TEXT NOT NULL      -- 'open' | 'closed'
);
```

### 12.4 `targets` Table

```sql
CREATE TABLE targets (
    wallet_address TEXT PRIMARY KEY,
    label          TEXT,              -- Human-readable name
    added_at       TEXT NOT NULL,
    active         INTEGER NOT NULL DEFAULT 1,
    roi_30d        REAL,
    win_rate       REAL,
    sharpe_ratio   REAL,
    last_scored_at TEXT,
    categories     TEXT,              -- JSON array of active categories to copy
    notes          TEXT
);
```

### 12.5 `daily_stats` Table

```sql
CREATE TABLE daily_stats (
    date              TEXT PRIMARY KEY,   -- 'YYYY-MM-DD'
    starting_balance  REAL NOT NULL,
    realized_pnl      REAL DEFAULT 0,
    unrealized_pnl    REAL DEFAULT 0,
    volume_traded     REAL DEFAULT 0,
    trades_placed     INTEGER DEFAULT 0,
    trades_filled     INTEGER DEFAULT 0,
    trades_rejected   INTEGER DEFAULT 0,
    drawdown_pct      REAL DEFAULT 0,
    paused_at         TEXT,
    notes             TEXT
);
```

### 12.6 `config` Table

```sql
CREATE TABLE config (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
-- Stores derived API keys, last known balances, heartbeat timestamps, etc.
```

---

## 13. Error Handling & Resilience

This section was entirely missing from the original PRD.

### 13.1 Error Categories

| Error Type | Trigger | Response |
|---|---|---|
| API 429 (rate limited) | Too many requests | Exponential backoff with jitter (1 s → max 60 s) |
| API 5xx (server error) | Polymarket infra issues | Retry up to 3 times, then mark signal as FAILED |
| FOK not matched | Insufficient liquidity at price | Discard signal — do NOT retry as GTC |
| Partial fill (GTC) | Maker order partially filled | Track in positions with partial fill flag |
| Network timeout (> 5 s) | RPC/API unreachable | Fall back to cached price; re-attempt after backoff |
| WebSocket disconnect | Network drop | Auto-reconnect with exponential backoff; resubscribe all tokens |
| USDC approval missing | First run | Auto-trigger on-chain approval transactions |
| Polygon RPC down | Node issues | Switch to backup RPC URL from `POLYGON_RPC_BACKUP_URL` |
| Balance too low | Below `MIN_USDC_BALANCE` | Auto-pause; Telegram alert |
| Daily loss limit hit | Drawdown ≥ 5% | Auto-pause all trading; Telegram alert; require manual `/resume` |

### 13.2 Retry Logic (Rust pseudocode)

```rust
const MAX_RETRIES: u32 = 3;
const BASE_DELAY_MS: u64 = 1000;
const MAX_DELAY_MS: u64 = 60_000;

async fn place_with_retry(order: &Order) -> Result<Fill, BotError> {
    let mut delay = BASE_DELAY_MS;
    for attempt in 0..MAX_RETRIES {
        match clob_client.post_order(order).await {
            Ok(fill) => return Ok(fill),
            Err(BotError::RateLimit) | Err(BotError::ServerError(_)) => {
                let jitter = rand::thread_rng().gen_range(0..delay / 5);
                tokio::time::sleep(Duration::from_millis(delay + jitter)).await;
                delay = (delay * 2).min(MAX_DELAY_MS);
            }
            Err(e) => return Err(e),  // Non-retryable: propagate immediately
        }
    }
    Err(BotError::MaxRetriesExceeded)
}
```

### 13.3 State Reconciliation

Every 30 seconds (adaptive: 10–60 s based on activity), the bot runs a reconciliation pass:

1. Fetch open positions from CLOB API (`GET /positions`)
2. Compare against `positions` table in SQLite
3. For any mismatch:
   - Position exists on-chain but not in DB → insert as orphaned position, alert
   - Position in DB but closed on-chain → mark as closed, compute realized PnL
4. Update `current_price` and `unrealized_pnl` for all open positions from live orderbook
5. Write reconciliation timestamp to `config` table

---

## 14. Functional Requirements

### Must Have

- Full Simulation Mode with virtual balance tracking
- Live Mode toggle (requires `SIMULATION_MODE=false` in `.env`)
- Dual-source signal ingestion (WebSocket + Data API polling)
- Target wallet tracker with configurable wallet list
- Transaction hash deduplication across all sources
- Signal staleness guard (age, market resolution, duplicate position)
- Complete risk + sizing engine with all multipliers
- Anti-duplication rule (one owner per token)
- CLOB order placement (FOK default) with price + slippage check
- CLOB order cancellation
- Automatic retry with exponential backoff + jitter
- Circuit breaker (consecutive losses)
- Daily loss limit auto-pause
- Real-time Leptos dashboard
- Telegram bot with whitelisted chat IDs
- Full SQLite persistence (signals, trades, positions, targets, daily_stats)
- State reconciliation loop (every 30 s)
- First-run USDC approval transaction handler

### Nice to Have

- Target wallet performance scoring via `/wallet score`
- Order aggregation window (15–60 s) for batching small signals
- Backup Polygon RPC failover
- Stop-loss execution via WebSocket orderbook monitoring
- Take-profit auto-exit at configurable price target

---

## 15. User Interfaces

### 15.1 Leptos Dashboard

| Panel | Metrics |
|---|---|
| Status Bar | Mode (SIM/LIVE), daily PnL, balance, drawdown % |
| Open Positions | Token, outcome, size, entry price, current price, unrealised PnL |
| System Health | WebSocket status, Data API latency, SQLite OK, last reconciliation |
| Signal Feed | Last 20 signals with source, status, wallet, size |
| Execution Log | Last 20 trades with order type, fill price, latency |
| Daily Stats | PnL chart, volume chart, win/loss ratio |

### 15.2 WebSocket Events Streamed to Dashboard

The Leptos frontend connects to a local WebSocket server (Axum) running on `ws://localhost:9001`:

| Event Type | Payload |
|---|---|
| `signal_received` | Signal ID, source, wallet, market, side, size |
| `trade_placed` | Trade ID, order type, price, size |
| `trade_filled` | Trade ID, fill price, latency ms |
| `trade_failed` | Trade ID, error message, retry count |
| `position_updated` | Token ID, new price, unrealised PnL |
| `system_alert` | Level (warn/error), message |
| `daily_stats` | Balance, PnL, drawdown % — emitted every 60 s |

### 15.3 Telegram Bot Commands

| Command | Description | Confirmation Required |
|---|---|---|
| `/status` | Bot mode, balance, daily PnL, drawdown | No |
| `/positions` | All open positions with PnL | No |
| `/signals` | Last 10 signals (source, wallet, status) | No |
| `/wallets` | List all tracked target wallets | No |
| `/wallet add [address]` | Add wallet to tracker | No |
| `/wallet remove [address]` | Remove wallet | Yes |
| `/wallet score [address]` | Score wallet (ROI, win rate, Sharpe) | No |
| `/pause` | Pause all new signal processing | No |
| `/resume` | Resume signal processing | No |
| `/mode sim` | Switch to simulation mode | Yes → `/confirm` |
| `/mode live` | Switch to live mode | Yes → `/confirm` |
| `/emergency_stop` | Cancel all orders + pause immediately | Yes → `/confirm` |
| `/report daily` | Full daily PnL report | No |
| `/report weekly` | 7-day performance summary | No |

**Telegram security:** Only chat IDs listed in `TELEGRAM_ALLOWED_CHAT_IDS` receive responses. All other messages are silently ignored and logged. `/confirm` is required for any destructive or mode-changing action — confirmation expires after 60 seconds.

---

## 16. Non-Functional Requirements

| Requirement | Target | Notes |
|---|---|---|
| Signal-to-order latency | < 550 ms average | WebSocket path targets ~340 ms; polling path up to 2.5 s |
| Reconciliation interval | 30 s (adaptive 10–60 s) | Scales with activity level |
| SQLite write latency | < 10 ms | WAL mode enabled |
| Dashboard refresh | Real-time via local WebSocket | No polling in UI |
| Uptime | Bot must auto-restart on panic | Use Windows Task Scheduler or NSSM for service wrapping |
| Private key security | Only in `.env`, never logged | `secrecy::SecretString` wrapper in Rust |
| Windows native | No Redis, no Docker | Pure `cargo run --release` |
| Simulation safety | 100% no-op — zero real orders | Verified by unit test suite |
| Rust edition | 2021, stable toolchain 1.70+ | |

---

## 17. Implementation Phases

| Phase | Scope | Key Deliverables |
|---|---|---|
| **Phase 1** | Foundation | SQLite schema + migrations, `.env` loader, `config` table, simulation mode shell |
| **Phase 2** | Signal ingestion | Data API wallet poller, WebSocket connector, tx-hash dedup, staleness guard |
| **Phase 3** | Risk engine | Sizing formula, all multipliers, hard limits, anti-duplication rule |
| **Phase 4** | CLOB execution | EIP-712 signing, FOK/GTC order placement, retry logic, fill tracking |
| **Phase 5** | Telegram bot | Chat ID whitelist, all commands, 2-step confirmation flow |
| **Phase 6** | Leptos dashboard | All panels, local WebSocket event stream, daily stats charts |
| **Phase 7** | Reconciliation | Position sync loop, PnL calculation, orphan detection |
| **Phase 8** | Live deployment | Flip `SIMULATION_MODE=false`, start with $100–200, monitor for 48 h |

---

## 18. Post-February 2026 CLOB Change

> **Important context for latency expectations.**

In early-to-mid February 2026, Polymarket quietly removed the artificial ~500 ms delay on taker (market/FOK) orders that had previously been applied to crypto markets. This delay was originally introduced to reduce latency arbitrage and deter delay-exploiting bots.

**Impact on v3.0:**
- FOK orders now resolve significantly faster — the 340–550 ms latency target is now realistic end-to-end
- Pure HFT-style arbitrage and low-risk spread farming became much harder or unprofitable as a result
- Copy trading became more competitive: fast detection + fast signing + fast submission now determines who gets the better fill price
- This reinforces the WebSocket-first detection strategy over polling

---

## 19. Risk Disclaimer

This software executes real financial transactions on the Polygon blockchain. All trading involves risk of loss. Features like `SIMULATION_MODE`, the daily loss limit, circuit breaker, and minimum balance guard are provided to reduce unintended exposure — but they do not eliminate risk. Start with the minimum viable capital ($100–200) and validate all behaviour in simulation mode before going live.
