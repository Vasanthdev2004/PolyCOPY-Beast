use axum::{extract::{Query, State}, response::{Html, Json}, routing::get, Router};
use serde::Serialize;
use std::sync::Arc;
use std::time::SystemTime;

use crate::metrics::Metrics;
use crate::state::{redis_store::RedisStore, sqlite::{SignalLogEntry, SqliteStore}};
use polybot_common::types::Position;

const DASHBOARD_HTML: &str = include_str!("dashboard_page.html");

#[derive(Clone)]
pub struct HealthState {
    pub start_time: SystemTime,
    pub simulation_mode: bool,
    pub paused: bool,
    pub metrics: Arc<Metrics>,
    pub redis_url: String,
    pub sqlite_path: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct SignalsQuery {
    pub limit: Option<usize>,
}

#[derive(Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub uptime_secs: u64,
    pub simulation: bool,
    pub ws_connected: bool,
    pub rpc_status: String,
    pub redis_connected: bool,
    pub last_signal_at: Option<String>,
    pub daily_pnl: String,
    pub paused: bool,
    pub open_positions: u64,
    pub signals_received: u64,
    pub signals_processed: u64,
    pub emergency_stops: u64,
}

pub async fn health_check(State(state): State<Arc<HealthState>>) -> Json<HealthResponse> {
    let metrics = &state.metrics;
    let uptime = state
        .start_time
        .elapsed()
        .unwrap_or(std::time::Duration::from_secs(0))
        .as_secs();

    let last_signal = metrics
        .last_signal_at
        .lock()
        .ok()
        .and_then(|guard| guard.clone());

    let redis_connected = metrics
        .redis_connected
        .load(std::sync::atomic::Ordering::Relaxed)
        == 1;
    let ws_connected = metrics
        .ws_connected
        .load(std::sync::atomic::Ordering::Relaxed)
        == 1;
    let rpc_healthy = metrics
        .rpc_healthy
        .load(std::sync::atomic::Ordering::Relaxed)
        == 1;
    let rpc_status = if rpc_healthy { "healthy" } else { "unhealthy" }.to_string();
    let paused = metrics.is_paused() || state.paused;

    Json(HealthResponse {
        status: if paused {
            "paused".to_string()
        } else {
            "ok".to_string()
        },
        uptime_secs: uptime,
        simulation: state.simulation_mode,
        ws_connected,
        rpc_status,
        redis_connected,
        last_signal_at: last_signal,
        daily_pnl: format!("{:.2}", metrics.daily_pnl_usd()),
        paused,
        open_positions: metrics
            .open_positions
            .load(std::sync::atomic::Ordering::Relaxed),
        signals_received: metrics
            .signals_received
            .load(std::sync::atomic::Ordering::Relaxed),
        signals_processed: metrics
            .signals_processed
            .load(std::sync::atomic::Ordering::Relaxed),
        emergency_stops: metrics
            .emergency_stops_triggered
            .load(std::sync::atomic::Ordering::Relaxed),
    })
}

pub async fn metrics_handler(State(state): State<Arc<HealthState>>) -> String {
    let m = &state.metrics;
    let uptime = state
        .start_time
        .elapsed()
        .unwrap_or(std::time::Duration::from_secs(0))
        .as_secs();

    format!(
        "# HELP polybot_uptime_seconds Bot uptime\n# TYPE polybot_uptime_seconds gauge\npolybot_uptime_seconds {}\n\
         # HELP polybot_signals_received_total Total signals received\n# TYPE polybot_signals_received_total counter\npolybot_signals_received_total {}\n\
         # HELP polybot_signals_processed_total Signals processed by risk engine\n# TYPE polybot_signals_processed_total counter\npolybot_signals_processed_total {}\n\
         # HELP polybot_signals_skipped_total Signals skipped\n# TYPE polybot_signals_skipped_total counter\npolybot_signals_skipped_total {}\n\
         # HELP polybot_signals_manual_review_total Signals queued for manual review\n# TYPE polybot_signals_manual_review_total counter\npolybot_signals_manual_review_total {}\n\
         # HELP polybot_trades_executed_total Live trades executed\n# TYPE polybot_trades_executed_total counter\npolybot_trades_executed_total {}\n\
         # HELP polybot_trades_simulated_total Simulated trades\n# TYPE polybot_trades_simulated_total counter\npolybot_trades_simulated_total {}\n\
         # HELP polybot_trades_failed_total Failed trade attempts\n# TYPE polybot_trades_failed_total counter\npolybot_trades_failed_total {}\n\
         # HELP polybot_open_positions Current open positions\n# TYPE polybot_open_positions gauge\npolybot_open_positions {}\n\
         # HELP polybot_daily_pnl_usd Daily PnL in USD\n# TYPE polybot_daily_pnl_usd gauge\npolybot_daily_pnl_usd {:.2}\n\
         # HELP polybot_drawdown_pct Current drawdown percentage\n# TYPE polybot_drawdown_pct gauge\npolybot_drawdown_pct {:.4}\n\
         # HELP polybot_avg_latency_us Average execution latency in microseconds\n# TYPE polybot_avg_latency_us gauge\npolybot_avg_latency_us {}\n\
         # HELP polybot_max_latency_us Maximum execution latency in microseconds\n# TYPE polybot_max_latency_us gauge\npolybot_max_latency_us {}\n\
         # HELP polybot_emergency_stops_total Emergency stops triggered\n# TYPE polybot_emergency_stops_total counter\npolybot_emergency_stops_total {}\n\
         # HELP polybot_health Bot health (1=ok, 0=error)\n# TYPE polybot_health gauge\npolybot_health {}\n\
         # HELP polybot_ws_connected WebSocket connection (1=connected)\n# TYPE polybot_ws_connected gauge\npolybot_ws_connected {}\n\
         # HELP polybot_redis_connected Redis connection (1=connected)\n# TYPE polybot_redis_connected gauge\npolybot_redis_connected {}\n",
        uptime,
        m.signals_received.load(std::sync::atomic::Ordering::Relaxed),
        m.signals_processed.load(std::sync::atomic::Ordering::Relaxed),
        m.signals_skipped.load(std::sync::atomic::Ordering::Relaxed),
        m.signals_manual_review.load(std::sync::atomic::Ordering::Relaxed),
        m.trades_executed.load(std::sync::atomic::Ordering::Relaxed),
        m.trades_simulated.load(std::sync::atomic::Ordering::Relaxed),
        m.trades_failed.load(std::sync::atomic::Ordering::Relaxed),
        m.open_positions.load(std::sync::atomic::Ordering::Relaxed),
        m.daily_pnl_usd(),
        m.current_drawdown_pct(),
        m.avg_latency_us.load(std::sync::atomic::Ordering::Relaxed),
        m.max_latency_us.load(std::sync::atomic::Ordering::Relaxed),
        m.emergency_stops_triggered.load(std::sync::atomic::Ordering::Relaxed),
         if m.is_paused() || state.paused { 0 } else { 1 },
         m.ws_connected.load(std::sync::atomic::Ordering::Relaxed),
         m.redis_connected.load(std::sync::atomic::Ordering::Relaxed),
    )
}

pub async fn positions_handler(
    State(state): State<Arc<HealthState>>,
) -> Json<Vec<Position>> {
    match RedisStore::new(&state.redis_url).await {
        Ok(store) => Json(store.list_positions().await.unwrap_or_default()),
        Err(_) => Json(Vec::new()),
    }
}

pub async fn signals_handler(
    State(state): State<Arc<HealthState>>,
    Query(query): Query<SignalsQuery>,
) -> Json<Vec<SignalLogEntry>> {
    let limit = query.limit.unwrap_or(20);
    match SqliteStore::open(std::path::Path::new(&state.sqlite_path)) {
        Ok(store) => Json(store.latest_signals(limit).unwrap_or_default()),
        Err(_) => Json(Vec::new()),
    }
}

pub async fn dashboard_handler() -> Html<&'static str> {
    Html(DASHBOARD_HTML)
}

pub fn create_health_router(state: Arc<HealthState>) -> Router {
    Router::new()
        .route("/", get(dashboard_handler))
        .route("/dashboard", get(dashboard_handler))
        .route("/health", get(health_check))
        .route("/metrics", get(metrics_handler))
        .route("/positions", get(positions_handler))
        .route("/signals", get(signals_handler))
        .with_state(state)
}

pub async fn start_health_server(
    state: Arc<HealthState>,
    port: u16,
) -> Result<(), polybot_common::errors::PolybotError> {
    let app = create_health_router(state);

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!("Health/metrics server starting on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.map_err(|e| {
        polybot_common::errors::PolybotError::Config(format!("Failed to bind health server: {}", e))
    })?;

    axum::serve(listener, app).await.map_err(|e| {
        polybot_common::errors::PolybotError::Config(format!("Health server error: {}", e))
    })?;

    Ok(())
}
