#![allow(dead_code)]

mod config;
mod execution;
mod health;
mod metrics;
mod risk;
mod scanner;
mod setup;
mod state;
mod telegram_bot;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::SystemTime;
use tokio::sync::{mpsc, RwLock};

use config::AppConfig;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .json()
        .init();

    let mut config = AppConfig::load()?;
    config.apply_env_overrides();

    let setup_only = std::env::args().any(|arg| arg == "--setup-check");
    let preflight = setup::run_startup_preflight(&config).await?;

    tracing::info!(
        execution_mode = config.system.execution_mode.as_str(),
        preflight = %preflight.summary(),
        "SuperFast PolyBot v3 starting"
    );

    if setup_only {
        tracing::info!(
            "Startup preflight completed successfully; exiting because --setup-check was requested"
        );
        return Ok(());
    }

    match config.system.execution_mode {
        polybot_common::types::ExecutionMode::Simulation => {
            tracing::info!("Running in SIMULATION mode - no Polymarket CLOB HTTP/WebSocket calls or live orders will be made");
        }
        polybot_common::types::ExecutionMode::Shadow => {
            tracing::info!("Running in SHADOW mode - live market data allowed, order submission disabled");
        }
        polybot_common::types::ExecutionMode::Live => {
            tracing::info!("Running in LIVE mode - live market data and order submission enabled");
        }
    }

    let config = Arc::new(config);
    let sqlite_path =
        std::env::var("POLYBOT_SQLITE_PATH").unwrap_or_else(|_| "./polybot.db".to_string());
    let (alert_tx, alert_rx) = tokio::sync::mpsc::unbounded_channel();
    let alert_broadcaster = telegram_bot::alerts::AlertBroadcaster::new(alert_tx);

    let metrics = Arc::new(metrics::Metrics::new());
    let position_manager = Arc::new(tokio::sync::Mutex::new(
        state::positions::PositionManager::new(),
    ));

    if let Ok(store) = state::sqlite::SqliteStore::open(std::path::Path::new(&sqlite_path)) {
        if let Err(e) =
            state::recover_from_sqlite(&store, metrics.clone(), position_manager.clone()).await
        {
            tracing::error!(error = %e, "Failed to recover state from SQLite");
        }
    }

    let risk_engine = Arc::new(risk::RiskEngine::new(
        config.clone(),
        metrics.clone(),
        position_manager.clone(),
        Some(alert_broadcaster.clone()),
    ));

    if let Ok(store) = state::sqlite::SqliteStore::open(std::path::Path::new(&sqlite_path)) {
        if let Ok(targets) = store.list_active_targets() {
            for target in targets {
                let _ = risk_engine.add_followed_wallet(&target.wallet_address).await;
            }
        }

        for wallet in risk_engine.list_followed_wallets().await {
            if let Err(e) = store.upsert_target(&wallet, None, &config.scanner.target_categories, None)
            {
                tracing::error!(
                    error = %e,
                    wallet = %wallet,
                    "Failed to persist followed wallet to SQLite targets table"
                );
            }
        }
    }

    let reconciler = Arc::new(
        state::reconciliation::Reconciler::new(
            position_manager.clone(),
            config.redis.enabled.then(|| config.redis.url.clone()),
        )
        .with_alerts(Some(alert_broadcaster.clone())),
    );
    let market_prices = Arc::new(RwLock::new(HashMap::new()));
    let wallet_activity_state =
        Arc::new(RwLock::new(scanner::wallet_tracker::WalletActivityState::default()));

    let (raw_signal_tx, raw_signal_rx) = mpsc::channel(512);
    let (dedup_signal_tx, dedup_signal_rx) = mpsc::channel(256);
    let (risk_decision_tx, risk_decision_rx) = mpsc::channel(128);
    let (trade_tx, trade_rx) = mpsc::channel(128);
    let (wallet_trigger_tx, wallet_trigger_rx) = mpsc::channel(256);

    let health_state = Arc::new(health::HealthState {
        start_time: SystemTime::now(),
        simulation_mode: config.system.simulation,
        execution_mode: config.system.execution_mode,
        paused: false,
        metrics: metrics.clone(),
        redis_enabled: config.redis.enabled,
        redis_url: config.redis.url.clone(),
        sqlite_path: sqlite_path.clone(),
        starting_balance: if config.risk.base_size_pct > rust_decimal::Decimal::ZERO {
            config.risk.base_size_usd / config.risk.base_size_pct
        } else {
            config.risk.base_size_usd
        },
        risk_engine: risk_engine.clone(),
        position_manager: position_manager.clone(),
    });

    let dedup_metrics = metrics.clone();
    let dedup_window = config.scanner.dedup_window_secs;
    let dedup_handle = tokio::spawn(async move {
        if let Err(e) =
            scanner::dedup::run_dedup_task(raw_signal_rx, dedup_signal_tx, dedup_window).await
        {
            tracing::error!(error = %e, "Dedup task failed");
        }
    });
    let _ = dedup_metrics;

    let poller_config = config.clone();
    let poller_risk = risk_engine.clone();
    let poller_signal_tx = raw_signal_tx.clone();
    let poller_state = wallet_activity_state.clone();
    let poller_handle = tokio::spawn(async move {
        match scanner::data_api::DataApiPoller::new(&poller_config, poller_state) {
            Ok(poller) => {
                if let Err(e) = poller.run(poller_risk, poller_signal_tx, wallet_trigger_rx).await {
                    tracing::error!(error = %e, "Data API poller failed");
                }
            }
            Err(e) => tracing::error!(error = %e, "Failed to initialize Data API poller"),
        }
    });

    let fast_path_handle = if config.scanner.use_websocket {
        let ws_config = config.clone();
        let ws_state = wallet_activity_state.clone();
        Some(tokio::spawn(async move {
            let fast_path = scanner::market_ws::MarketFastPath::new(&ws_config, ws_state);
            if let Err(e) = fast_path.run(wallet_trigger_tx).await {
                tracing::error!(error = %e, "Market fast-path failed");
            }
        }))
    } else {
        None
    };

    let risk_engine_task = risk_engine.clone();
    let risk_metrics = metrics.clone();
    let risk_handle = tokio::spawn(async move {
        if let Err(e) =
            risk::run_risk_engine(risk_engine_task, risk_metrics, dedup_signal_rx, risk_decision_tx)
                .await
        {
            tracing::error!(error = %e, "Risk engine failed");
        }
    });

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

    let state_config = config.clone();
    let state_metrics = metrics.clone();
    let state_positions = position_manager.clone();
    let state_market_prices = market_prices.clone();
    let state_handle = tokio::spawn(async move {
        if let Err(e) = state::run_state_manager(
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

    let reconciler_task = reconciler.clone();
    let recon_handle = tokio::spawn(async move {
        if let Err(e) = reconciler_task.run_loop().await {
            tracing::error!(error = %e, "Reconciliation task failed");
        }
    });

    let http_config = config.clone();
    let http_signal_tx = raw_signal_tx.clone();
    let http_handle = tokio::spawn(async move {
        if let Err(e) = scanner::http_ingest::start_http_server(&http_config, http_signal_tx).await
        {
            tracing::error!(error = %e, "HTTP server failed");
        }
    });

    let redis_ingest_handle = if config.redis.enabled {
        let redis_signal_tx = raw_signal_tx.clone();
        let redis_url = config.redis.url.clone();
        let redis_signal_max_age_secs = config.scanner.signal_max_age_secs;
        tokio::spawn(async move {
            let stream_key = std::env::var("POLYBOT_SIGNAL_STREAM")
                .unwrap_or_else(|_| "polybot:signals".to_string());
            let redis_ingest = scanner::redis_ingest::RedisIngest::new(
                &redis_url,
                &stream_key,
                redis_signal_max_age_secs,
            );
            if let Err(e) = redis_ingest.run(redis_signal_tx).await {
                tracing::error!(error = %e, "Redis ingest failed");
            }
        })
    } else {
        tracing::info!("Redis signal ingestion disabled");
        tokio::spawn(async {})
    };

    let backup_handle = if config.redis.enabled {
        let redis_backup_url = config.redis.url.clone();
        tokio::spawn(async move {
            let backup_dir =
                std::env::var("POLYBOT_BACKUP_DIR").unwrap_or_else(|_| "./backups".to_string());
            let backup = state::redis_backup::RedisBackup::new(&redis_backup_url, &backup_dir);
            if let Err(e) = backup.run_backup_loop().await {
                tracing::error!(error = %e, "Redis backup loop failed");
            }
        })
    } else {
        tracing::info!("Redis backup loop disabled");
        tokio::spawn(async {})
    };

    let health_state_clone = health_state.clone();
    let health_port = config.dashboard.port;
    let health_handle = tokio::spawn(async move {
        if let Err(e) = health::start_health_server(health_state_clone, health_port).await {
            tracing::error!(error = %e, "Health server failed");
        }
    });

    let tg_config = config.clone();
    let tg_risk = risk_engine.clone();
    let tg_reconciler = reconciler.clone();
    let tg_metrics = metrics.clone();
    let tg_handle = tokio::spawn(async move {
        if let Err(e) = telegram_bot::start_telegram_bot(
            tg_config,
            tg_risk,
            tg_reconciler,
            tg_metrics,
            position_manager.clone(),
            alert_rx,
        )
        .await
        {
            tracing::warn!(
                error = %e,
                "Telegram bot not started (token may not be configured)"
            );
        }
    });

    let fw_config = config.clone();
    let fw_handle = tokio::spawn(async move {
        let file_watcher = scanner::file_watcher::FileWatcher::new(&fw_config, raw_signal_tx);
        if let Err(e) = file_watcher.watch().await {
            tracing::error!(error = %e, "File watcher failed");
        }
    });

    tracing::info!("All subsystems started. Watching for signals...");

    tokio::signal::ctrl_c().await?;
    tracing::info!("Shutdown signal received, stopping...");

    let _ = tokio::join!(
        dedup_handle,
        poller_handle,
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

    if let Some(handle) = fast_path_handle {
        let _ = handle.await;
    }

    Ok(())
}
