# SuperFast PolyBot v2 — Technical Design Document

**Date:** 2026-04-14
**Status:** Approved Design
**Scope:** Phase 2 — Core Pipeline (Scanner → Risk → Execution → State) + Telegram Bot + Simulation Mode

---

## 1. Architecture: Modular Monolith

Single Rust workspace with 3 crates. The core engine is a monolith where modules communicate via `tokio::mpsc` channels. This minimizes latency (in-process), simplifies deployment (one binary), and keeps the architecture clean for future decomposition.

### 1.1 Workspace Structure

```
superfast-polybot/
├── Cargo.toml                  # Workspace root
├── .env.example                # Template for env vars (secrets)
├── config.toml                 # User-editable risk & system config
├── docker-compose.yml          # Redis + polybot-core stack
│
├── polybot-core/               # Core engine crate (the monolith)
│   ├── Cargo.toml
│   └── src/
│       ├── main.rs             # Tokio runtime, module wiring
│       ├── scanner/            # Signal ingestion
│       │   ├── mod.rs
│       │   ├── schema.rs       # Signal types & JSON parsing
│       │   ├── file_watcher.rs # Filesystem-based ingestion
│       │   ├── http_ingest.rs  # HTTP endpoint ingestion
│       │   └── dedup.rs        # Signal deduplication
│       ├── risk/               # Risk & dynamic sizing engine
│       │   ├── mod.rs
│       │   ├── sizer.rs        # Position size calculation
│       │   ├── limits.rs       # Daily loss, exposure, category limits
│       │   └── drawdown.rs     # Drawdown curve & adjustment
│       ├── execution/          # CLOB order execution
│       │   ├── mod.rs
│       │   ├── order_builder.rs # Order construction & EIP-712 signing
│       │   ├── clob_client.rs  # Polymarket CLOB client (WS + REST)
│       │   └── rpc_pool.rs    # Multi-RPC endpoint failover
│       ├── state/              # Position tracking & PnL
│       │   ├── mod.rs
│       │   ├── positions.rs    # Open position management
│       │   ├── pnl.rs          # Realized/unrealized PnL
│       │   └── reconciliation.rs # On-chain reconciliation
│       ├── telegram_bot/        # Telegram control interface
│       │   ├── mod.rs
│       │   ├── commands.rs     # Command handlers
│       │   └── alerts.rs       # Alert system
│       └── config.rs           # Config loading & validation
│
├── polybot-dashboard/          # Leptos web dashboard (separate crate)
│   ├── Cargo.toml
│   └── src/
│       ├── main.rs
│       ├── app.rs
│       └── components/
│
└── polybot-common/             # Shared types & utilities
    ├── Cargo.toml
    └── src/
        ├── lib.rs
        ├── types.rs            # Signal, Trade, Position, enums
        └── errors.rs           # Error types
```

### 1.2 Config & Secrets Separation

- **`config.toml`**: All tunable parameters — risk limits, multiplier tables, RPC endpoints, dedup windows, category allocations. Parseable, versionable, no secrets.
- **`.env`**: Private key, Polymarket API key, Telegram bot token, Redis URL. Loaded at startup via `dotenvy`, never logged, never persisted in code.
- **Environment variable override**: Any `config.toml` value can be overridden via `POLYBOT_<KEY>` env vars for Docker deployments.

---

## 2. Data Flow

```
Scanner (external) ──JSON──► scanner/ module
                                  │
                                  ▼ (tokio::mpsc::channel, bounded=256)
                            risk/ module
                                  │
                                  ▼ (tokio::mpsc::channel, bounded=128)
                         execution/ module
                                  │
                                  ▼
                          Polymarket CLOB
                                  │
                                  ▼
                           state/ module ──► Redis (hot cache) + SQLite (cold persistence)
                                  │
                    ┌─────────────┼─────────────┐
                    ▼              ▼              ▼
              telegram_bot   Leptos dashboard   reconciliation
```

### 2.1 Channel Strategy

- **Bounded channels** with `tokio::mpsc`. Scanner→Risk: 256 capacity. Risk→Execution: 128 capacity.
- **Backpressure**: If downstream is full, `send()` returns `TryRecvError::Full`. Scanner module buffers and dedupes rather than panicking.
- **Shutdown**: Drop channel senders on graceful shutdown. All modules drain their receiver before exiting.

### 2.2 Event Types (on channels)

```rust
// polybot-common/src/types.rs

pub struct ScannerEvent {
    pub signal: Signal,
    pub received_at: Instant,
}

pub struct RiskDecision {
    pub signal: Signal,
    pub position_size_usd: Decimal,
    pub confidence_multiplier: Decimal,
    pub secret_level_multiplier: Decimal,
    pub drawdown_factor: Decimal,
    pub decision: Decision, // Execute, Skip, EmergencyStop
    pub decided_at: Instant,
}

pub enum Decision {
    Execute,
    Skip(String),      // reason
    EmergencyStop,
}
```

---

## 3. Scanner Signal Schema

```json
{
  "signal_id": "uuid-v4",
  "timestamp": "2026-04-14T12:34:56.789Z",
  "wallet_address": "0xabc123...",
  "secret_level": 7,
  "confidence": 0.85,
  "category": "politics",
  "action": "buy",
  "market": {
    "condition_id": "0xdef456...",
    "token_id": "0x789abc...",
    "market_slug": "will-trump-win-2028"
  },
  "side": "yes",
  "price": 0.65,
  "size": 500.0,
  "source": "scanner_v1"
}
```

### 3.1 Validation Rules

| Field | Type | Constraint | On Invalid |
|---|---|---|---|
| `signal_id` | UUID v4 | Must be parseable | Reject + log |
| `timestamp` | ISO 8601 | Must be parseable, not future | Reject + log |
| `wallet_address` | String | 0x-prefixed, 42 chars | Reject + log |
| `secret_level` | u8 | 1–10 inclusive | Reject + log |
| `confidence` | f64 | 0.0–1.0 inclusive | Reject + log |
| `category` | enum | politics, sports, crypto, others | Assign "others" if unknown |
| `action` | enum | buy, sell | Reject + log |
| `market.condition_id` | String | Non-empty | Reject + log |
| `market.token_id` | String | Non-empty | Reject + log |
| `price` | f64 | 0.0–1.0 inclusive | Reject + log |
| `size` | f64 | > 0.0 | Reject + log |

### 3.2 Deduplication

- **Key**: `wallet_address + market.condition_id + action`
- **Window**: 30 seconds (configurable via `config.toml`)
- **Implementation**: HashMap with TTL cleanup task running every 10 seconds
- **Behavior**: Duplicate signals within window are silently discarded with a `tracing::debug!` log

### 3.3 Ingestion Methods (Priority Order)

1. **File watcher** (primary): Watch a directory for new `.json` files. Parse, validate, move to `processed/` archive. Uses `notify` crate.
2. **Redis stream** (secondary): Read from a Redis stream via `XREAD GROUP`. Ack after successful processing.
3. **HTTP endpoint** (tertiary): Simple `axum` POST endpoint at `/signals`. Authenticated via API key header.

All three methods produce the same `ScannerEvent` and push into the same channel.

---

## 4. Risk & Sizing Engine

### 4.1 Position Size Formula

```
position_size_usd = base_size × confidence_mult(secret_level) × secret_level_mult(secret_level) × drawdown_factor
```

`base_size` is set in `config.toml` (e.g., $50 USD).

### 4.2 Multiplier Tables

**Confidence multiplier** (interpolated from secret_level):

| Secret Level | Confidence Multiplier |
|---|---|
| 1 | 0.50 |
| 2 | 0.60 |
| 3 | 0.80 |
| 4 | 0.80 |
| 5 | 0.90 |
| 6 | 1.00 |
| 7 | 1.10 |
| 8 | 1.30 |
| 9 | 1.40 |
| 10 | 1.50 |

**Secret level multiplier**:

| Secret Level | Secret Level Multiplier |
|---|---|
| 1–3 | 0.30 |
| 4–6 | 0.70 |
| 7–8 | 1.00 |
| 9–10 | 1.30 |

### 4.3 Hard Limits

All configurable in `config.toml`:

| Limit | Default | Behavior on Breach |
|---|---|---|
| Daily max loss | 5% of portfolio | Auto-pause all trading, alert via Telegram |
| Per-market exposure | 10% of portfolio | Skip signal, alert |
| Per-category exposure | 25% of portfolio | Skip signal, alert |
| Max position size | $500 USD | Cap at max, proceed |
| Min confidence threshold | 0.60 | Skip signals below threshold |

### 4.4 Drawdown Factor

Linear reduction based on how close current daily loss is to the daily max loss:

```
drawdown_factor = 1.0 - (current_daily_loss / daily_max_loss) × 0.8
```

At 0% daily loss → factor = 1.0 (full size). At 100% of daily max loss → factor = 0.2 (20% size). This provides a smooth de-escalation rather than a hard cliff.

### 4.5 Emergency Stop

- Triggered by `/emergency_stop` Telegram command or by risk engine detecting catastrophic conditions (e.g., daily loss exceeds 150% of limit due to rapid market move).
- Sets global atomic flag. All channels drain. No new orders submitted. Existing orders may be cancelled (configurable).
- Requires explicit `/resume` to resume trading.

### 4.6 Per-Category Risk Profiles

| Category | Default Max Exposure | Confidence Override |
|---|---|---|
| politics | 25% | None |
| sports | 20% | −0.10 (higher bar) |
| crypto | 15% | −0.10 (higher bar) |
| others | 10% | −0.05 |

---

## 5. Execution Engine

### 5.1 CLOB Connection

- **WebSocket**: Persistent connection to Polymarket CLOB WS endpoint for order placement and real-time book data.
- **Auto-reconnect**: Exponential backoff (1s, 2s, 4s, 8s, max 30s). Circuit breaker after 5 consecutive failures (open for 60s).
- **Heartbeat**: Ping/pong every 15 seconds to keep connection alive.

### 5.2 Order Types

| Type | Use Case |
|---|---|
| Limit | Default — place at signal price, wait for fill |
| IOC | Time-sensitive signals — partial fill acceptable |
| FOK | High-confidence signals — all or nothing |
| Post-Only | Manual only — set via `config.toml` per-market or via Telegram `/config set` |

Order type selection is based on `secret_level` and `confidence` (auto-select):

| Secret Level | Confidence | Order Type |
|---|---|---|
| 7+ | 0.90+ | FOK |
| 5–6 | 0.80+ | IOC |
| Default | Any | Limit |

Configurable in `config.toml`.

### 5.3 Slippage Protection

Before placing an order, fetch current best bid/ask from book. Reject if:

```
|signal_price - best_book_price| / best_book_price > slippage_threshold
```

Default `slippage_threshold` = 0.02 (2%). Configurable per category.

### 5.4 EIP-712 Signing

- Uses `ethers-rs` crate for signing.
- Private key loaded from `POLYBOT_PRIVATE_KEY` env var.
- Key held in memory only, zeroed on drop via `zeroize` crate.
- Signing function is a pure function: takes (order_params, private_key) → signed_order. Maximum testability.

### 5.5 RPC Pool

- 3+ RPC endpoints configured in `config.toml`.
- Round-robin with circuit breaker per endpoint.
- 3 consecutive failures → circuit open for 30s, try next endpoint.
- All endpoints failed → alert via Telegram, queue decisions for retry (up to 60s).

---

## 6. State & Reconciliation

### 6.1 Storage Layers

| Layer | Technology | Data | Access Pattern |
|---|---|---|---|
| Hot cache | Redis | Current positions, daily PnL, exposure, signal dedup map | Sub-ms reads, frequent writes |
| Cold persistence | SQLite (via `rusqlite`) | Trade history, signal log, audit trail, configuration snapshots | Append-heavy, occasional reads for reporting |

### 6.2 Position Tracking

- Every executed order creates/updates a `Position` record in Redis.
- Position fields: `condition_id`, `token_id`, `side`, `entry_price`, `current_size`, `average_price`, `opened_at`.
- Positions are indexed by `condition_id` for fast per-market exposure queries.

### 6.3 PnL Calculation

- **Unrealized PnL**: `(current_book_price - average_entry_price) × position_size`. Refreshed every 10 seconds from book snapshots.
- **Realized PnL**: Computed on fill confirmation. Locked and written to SQLite trade history.
- **Daily PnL**: Sum of realized + unrealized for current UTC day. Stored in Redis with TTL of 48 hours.

### 6.4 Reconciliation

- Runs every 60 seconds.
- Fetches on-chain positions via RPC call to Polymarket contracts.
- Compares against local Redis state.
- Discrepancies logged as warnings and sent as Telegram alerts.
- Auto-correction is OFF by default (requires manual review). Can be enabled in config.

---

## 7. Telegram Bot

### 7.1 Commands

| Command | Description |
|---|---|
| `/status` | System status: running/paased/stopped, uptime, connection health |
| `/positions` | All open positions with unrealized PnL |
| `/signals` | Last 10 processed signals |
| `/pause` | Pause new trade execution (existing positions held) |
| `/resume` | Resume trading after pause or emergency stop |
| `/emergency_stop` | Immediate halt, cancel configurable |
| `/wallet add <addr>` | Add wallet to follow list |
| `/wallet remove <addr>` | Remove wallet from follow list |
| `/report` | Daily PnL report with category breakdown |
| `/config get <key>` | Show current config value |
| `/config set <key> <value>` | Update config value at runtime |

### 7.2 Push Alerts

| Alert | Trigger | Priority |
|---|---|---|
| High-secret signal | secret_level >= 8 | Info |
| Trade executed | Order filled | Info |
| Daily loss breach | Loss exceeds threshold | Critical |
| Emergency stop triggered | Manual or automatic | Critical |
| WS/RPC disconnect | Connection lost | Warning |
| Reconciliation mismatch | On-chain vs local differs | Warning |

### 7.3 Authentication

- Whitelist of Telegram user IDs in `config.toml` under `[telegram]` section.
- Non-whitelisted users get no response.
- Bot token loaded from `POLYBOT_TELEGRAM_TOKEN` env var.

---

## 8. Simulation Mode

- Enabled via `simulation = true` in `config.toml` or `POLYBOT_SIMULATION=true` env var.
- All signals processed through scanner → risk pipeline normally.
- Risk decisions logged with full detail (multipliers, limits, drawdown).
- Orders are **not** sent to CLOB. Instead, simulated fills recorded against real book snapshots.
- Simulated trades logged to SQLite with `simulated = true` flag, identical schema to live trades.
- PnL tracking works in simulation against real prices.
- Purpose: Validate risk engine behavior and scanner alpha before risking capital.

---

## 9. Deployment

### 9.1 Docker Compose

```yaml
services:
  redis:
    image: redis:7-alpine
    ports:
      - "6379:6379"
    volumes:
      - redis_data:/data
    healthcheck:
      test: ["CMD", "redis-cli", "ping"]
      interval: 10s

  polybot-core:
    build: .
    depends_on:
      redis:
        condition: service_healthy
    env_file: .env
    volumes:
      - ./config.toml:/app/config.toml:ro
      - ./signals:/app/signals        # For file watcher
      - polybot_data:/app/data        # SQLite persistence
    healthcheck:
      test: ["CMD", "curl", "-f", "http://localhost:8080/health"]
      interval: 30s

volumes:
  redis_data:
  polybot_data:
```

### 9.2 Monitoring

- **Health endpoint**: `GET /health` on port 8080. Returns JSON with: uptime, ws_connected, rpc_status, redis_connected, last_signal_at, daily_pnl, paused.
- **Metrics**: `tracing` structured JSON logs. All hot-path spans instrumented with `tracing::instrument` including timing.
- **Latency heatmap**: Scanner receipt → risk decision → order submission timestamps recorded for every signal.

---

## 10. Key Rust Dependencies

| Crate | Purpose |
|---|---|
| `tokio` | Async runtime (full features) |
| `serde` + `serde_json` | JSON serialization |
| `ethers` | Ethereum signing & transactions |
| `redis` | Redis client (via `tokio` feature) |
| `rusqlite` | SQLite persistence |
| `teloxide` | Telegram bot framework |
| `notify` | File system watcher |
| `axum` | HTTP ingestion endpoint + health check |
| `tracing` + `tracing-subscriber` | Structured logging |
| `dotenvy` | .env loading |
| `toml` + `serde` | Config parsing |
| `rust_decimal` | Precise decimal math for prices/sizes |
| `uuid` | Signal ID generation |
| `zeroize` | Secure key zeroing |
| `thiserror` | Error type derivation |

Dashboard (separate crate):

| Crate | Purpose |
|---|---|
| `leptos` | Reactive web frontend |
| `leptos_axum` | SSR + hydration integration |

---

## 11. Error Handling Strategy

- All fallible operations return `Result<T, polybot_common::errors::Error>`.
- `Error` is an enum with variants: `Scanner`, `Risk`, `Execution`, `State`, `Config`, `RpcPool`.
- Each variant wraps a context string + source error chain.
- **No panics** in production code. All `.unwrap()` replaced with `?` or explicit handling.
- **Circuit breakers** wrap all external service calls (CLOB, RPC, Telegram API).
- **Retry with backoff** for transient errors. No retry for validation or auth errors.

---

## 12. Testing Strategy

| Layer | Tool | Target |
|---|---|---|
| Unit tests | `#[cfg(test)]` in each module | Pure functions: sizer, limits, dedup, validation |
| Integration tests | `tests/` directory | Channel flow: scanner→risk→execution with mock CLOB |
| Simulation mode | Manual + CI | Full pipeline with real signals, no real orders |
| Latency benchmarks | `criterion` | Critical path timing assertions |

Minimum coverage targets:
- Risk engine: 95% (pure functions, easily testable)
- Scanner parsing: 90%
- Execution order building: 85%
- State/reconciliation: 80%