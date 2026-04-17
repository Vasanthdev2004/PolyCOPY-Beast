#![allow(dead_code)]

mod config;
mod execution;
mod health;
mod metrics;
mod risk;
mod scanner;
mod state;
mod telegram_bot;

use std::sync::Arc;
use std::collections::HashMap;
use std::time::SystemTime;
use tokio::sync::{mpsc, RwLock};

use config::AppConfig;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load .env
    dotenvy::dotenv().ok();

    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .json()
        .init();

    // Load config
    let mut config = AppConfig::load()?;
    config.apply_env_overrides();

    tracing::info!(
        simulation = config.system.simulation,
        "SuperFast PolyBot v2.5 starting"
    );

    if config.system.simulation {
        tracing::info!("Running in SIMULATION mode — no real orders will be placed");
    }

    let config = Arc::new(config);
    let sqlite_path = std::env::var("POLYBOT_SQLITE_PATH").unwrap_or_else(|_| "./polybot.db".to_string());
    let (alert_tx, alert_rx) = tokio::sync::mpsc::unbounded_channel();
    let alert_broadcaster = telegram_bot::alerts::AlertBroadcaster::new(alert_tx);

    // Shared metrics — accessible from all subsystems
    let metrics = Arc::new(metrics::Metrics::new());
    let position_manager = Arc::new(tokio::sync::Mutex::new(
        state::positions::PositionManager::new(),
    ));
    let risk_engine = Arc::new(risk::RiskEngine::new(
        config.clone(),
        metrics.clone(),
        position_manager.clone(),
        Some(alert_broadcaster.clone()),
    ));
    let reconciler = Arc::new(state::reconciliation::Reconciler::new(
        position_manager.clone(),
        Some(config.redis.url.clone()),
    ));
    let market_prices = Arc::new(RwLock::new(HashMap::new()));

    // Create channels
    // Scanner -> Dedup -> Risk -> Execution -> State
    let (raw_signal_tx, raw_signal_rx) = mpsc::channel(512);
    let (dedup_signal_tx, dedup_signal_rx) = mpsc::channel(256);
    let (risk_decision_tx, risk_decision_rx) = mpsc::channel(128);
    let (trade_tx, trade_rx) = mpsc::channel(128);

    // Health state — now backed by shared Metrics
    let health_state = Arc::new(health::HealthState {
        start_time: SystemTime::now(),
        simulation_mode: config.system.simulation,
        paused: false,
        metrics: metrics.clone(),
        redis_url: config.redis.url.clone(),
        sqlite_path: sqlite_path.clone(),
    });

    // Spawn dedup filter task
    let dedup_metrics = metrics.clone();
    let dedup_window = config.scanner.dedup_window_secs;
    let dedup_handle = tokio::spawn(async move {
        if let Err(e) =
            scanner::dedup::run_dedup_task(raw_signal_rx, dedup_signal_tx, dedup_window).await
        {
            tracing::error!(error = %e, "Dedup task failed");
        }
    });
    let _ = dedup_metrics; // available for future dedup metrics

    // Spawn risk engine task
    let risk_engine_task = risk_engine.clone();
    let risk_metrics = metrics.clone();
    let risk_handle = tokio::spawn(async move {
        if let Err(e) = risk::run_risk_engine(
            risk_engine_task,
            risk_metrics,
            dedup_signal_rx,
            risk_decision_tx,
        )
        .await
        {
            tracing::error!(error = %e, "Risk engine failed");
        }
    });

    // Spawn execution engine task
    let exec_config = config.clone();
    let exec_metrics = metrics.clone();
    let exec_alerts = alert_broadcaster.clone();
    let exec_market_prices = market_prices.clone();
    let exec_handle = tokio::spawn(async move {
        if let Err(e) = execution::run_execution_engine(
            exec_config,
            exec_metrics,
            Some(exec_alerts),
            exec_market_prices,
            risk_decision_rx,
            trade_tx,
        )
        .await
        {
            tracing::error!(error = %e, "Execution engine failed");
        }
    });
    let _ = exec_metrics; // available for future execution metrics

    // Spawn state manager task
    let state_config = config.clone();
    let state_metrics = metrics.clone();
    let state_positions = position_manager.clone();
    let state_market_prices = market_prices.clone();
    let state_handle = tokio::spawn(async move {
        if let Err(e) =
            state::run_state_manager(
                state_config,
                state_metrics,
                state_positions,
                state_market_prices,
                trade_rx,
            )
            .await
        {
            tracing::error!(error = %e, "State manager failed");
        }
    });

    // Spawn reconciliation task (v2.5: light 30s + full 5min)
    let reconciler_task = reconciler.clone();
    let recon_handle = tokio::spawn(async move {
        if let Err(e) = reconciler_task.run_loop().await {
            tracing::error!(error = %e, "Reconciliation task failed");
        }
    });

    // Spawn HTTP ingestion server
    let http_config = config.clone();
    let http_signal_tx = raw_signal_tx.clone();
    let http_handle = tokio::spawn(async move {
        if let Err(e) = scanner::http_ingest::start_http_server(&http_config, http_signal_tx).await
        {
            tracing::error!(error = %e, "HTTP server failed");
        }
    });

    // Spawn Redis stream ingestion
    let redis_signal_tx = raw_signal_tx.clone();
    let redis_url = config.redis.url.clone();
    let redis_ingest_handle = tokio::spawn(async move {
        let stream_key = std::env::var("POLYBOT_SIGNAL_STREAM")
            .unwrap_or_else(|_| "polybot:signals".to_string());
        let redis_ingest = scanner::redis_ingest::RedisIngest::new(&redis_url, &stream_key);
        if let Err(e) = redis_ingest.run(redis_signal_tx).await {
            tracing::error!(error = %e, "Redis ingest failed");
        }
    });

    // Spawn Redis backup loop
    let redis_backup_url = config.redis.url.clone();
    let backup_handle = tokio::spawn(async move {
        let backup_dir = std::env::var("POLYBOT_BACKUP_DIR")
            .unwrap_or_else(|_| "./backups".to_string());
        let backup = state::redis_backup::RedisBackup::new(&redis_backup_url, &backup_dir);
        if let Err(e) = backup.run_backup_loop().await {
            tracing::error!(error = %e, "Redis backup loop failed");
        }
    });

    // Spawn health/metrics server
    let health_state_clone = health_state.clone();
    let health_port = config.dashboard.port;
    let health_handle = tokio::spawn(async move {
        if let Err(e) = health::start_health_server(health_state_clone, health_port).await {
            tracing::error!(error = %e, "Health server failed");
        }
    });

    // Spawn telegram bot (if token is configured)
    let tg_config = config.clone();
    let tg_risk = risk_engine.clone();
    let tg_reconciler = reconciler.clone();
    let tg_metrics = metrics.clone();
    let tg_handle = tokio::spawn(async move {
        if let Err(e) =
            telegram_bot::start_telegram_bot(
                tg_config,
                tg_risk,
                tg_reconciler,
                tg_metrics,
                position_manager.clone(),
                alert_rx,
            )
                .await
        {
            tracing::warn!(error = %e, "Telegram bot not started (token may not be configured)");
        }
    });

    // File watcher (primary signal source)
    let fw_config = config.clone();
    let fw_handle = tokio::spawn(async move {
        let file_watcher = scanner::file_watcher::FileWatcher::new(&fw_config, raw_signal_tx);
        if let Err(e) = file_watcher.watch().await {
            tracing::error!(error = %e, "File watcher failed");
        }
    });

    tracing::info!("All subsystems started. Watching for signals...");

    // Wait for shutdown signal (Ctrl+C)
    tokio::signal::ctrl_c().await?;
    tracing::info!("Shutdown signal received, stopping...");

    // Wait for all tasks
    let _ = tokio::join!(
        dedup_handle,
        risk_handle,
        exec_handle,
        state_handle,
        recon_handle,
        http_handle,
        redis_ingest_handle,
        backup_handle,
        health_handle,
        tg_handle,
        fw_handle,
    );

    Ok(())
}
