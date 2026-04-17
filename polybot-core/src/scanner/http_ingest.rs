use axum::{extract::State, http::StatusCode, routing::post, Router};
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing;

use crate::config::AppConfig;
use crate::scanner::schema::validate_and_create_event;
use polybot_common::errors::PolybotError;
use polybot_common::types::ScannerEvent;

#[derive(Clone)]
pub struct AppState {
    pub signal_sender: mpsc::Sender<ScannerEvent>,
    pub api_key: String,
}

async fn ingest_signal(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    body: String,
) -> Result<StatusCode, StatusCode> {
    // Validate API key
    if let Some(key) = headers.get("X-API-Key") {
        if key.to_str().unwrap_or("") != state.api_key {
            tracing::warn!("Invalid API key from HTTP ingestion");
            return Err(StatusCode::UNAUTHORIZED);
        }
    } else {
        return Err(StatusCode::UNAUTHORIZED);
    }

    match validate_and_create_event(&body) {
        Ok(event) => {
            tracing::info!(
                signal_id = %event.signal.signal_id,
                "Received signal via HTTP"
            );
            state
                .signal_sender
                .send(event)
                .await
                .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
            Ok(StatusCode::OK)
        }
        Err(e) => {
            tracing::error!(error = %e, "Invalid signal received via HTTP");
            Err(StatusCode::BAD_REQUEST)
        }
    }
}

pub async fn start_http_server(
    config: &AppConfig,
    signal_sender: mpsc::Sender<ScannerEvent>,
) -> Result<(), PolybotError> {
    let api_key =
        std::env::var("POLYBOT_API_KEY").unwrap_or_else(|_| "default-api-key".to_string());

    let state = Arc::new(AppState {
        signal_sender,
        api_key,
    });

    let app = Router::new()
        .route("/signals", post(ingest_signal))
        .with_state(state);

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], config.scanner.http_port));
    tracing::info!("HTTP ingestion server starting on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|e| PolybotError::Scanner(format!("Failed to bind HTTP server: {}", e)))?;

    axum::serve(listener, app)
        .await
        .map_err(|e| PolybotError::Scanner(format!("HTTP server error: {}", e)))?;

    Ok(())
}
