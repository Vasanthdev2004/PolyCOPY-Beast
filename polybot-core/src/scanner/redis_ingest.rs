use polybot_common::errors::PolybotError;
use polybot_common::types::ScannerEvent;
use tokio::sync::mpsc;

use super::dedup::DedupFilter;

/// v2.5: Redis stream ingestion — secondary priority method.
/// Listens to a Redis stream for incoming signals and deduplicates them
/// before sending to the risk engine channel.
pub struct RedisIngest {
    redis_url: String,
    stream_key: String,
}

impl RedisIngest {
    pub fn new(redis_url: &str, stream_key: &str) -> Self {
        Self {
            redis_url: redis_url.to_string(),
            stream_key: stream_key.to_string(),
        }
    }

    /// Start consuming from the Redis stream.
    pub async fn run(&self, sender: mpsc::Sender<ScannerEvent>) -> Result<(), PolybotError> {
        let client = redis::Client::open(self.redis_url.as_str())
            .map_err(|e| PolybotError::Redis(format!("Failed to connect to Redis: {}", e)))?;

        let mut conn = client
            .get_multiplexed_async_connection()
            .await
            .map_err(|e| PolybotError::Redis(format!("Failed to get async connection: {}", e)))?;

        let mut dedup =
            DedupFilter::new(polybot_common::constants::DEFAULT_DEDUP_WINDOW_SECS as u64);

        tracing::info!(stream = %self.stream_key, "Redis stream ingestion started");

        // Use XREAD to consume from the stream
        let mut last_id = "0".to_string();

        loop {
            let result: Result<
                Vec<(String, Vec<(String, Vec<(String, String)>)>)>,
                redis::RedisError,
            > = redis::cmd("XREAD")
                .arg("COUNT")
                .arg(1)
                .arg("BLOCK")
                .arg(5000)
                .arg("STREAMS")
                .arg(&self.stream_key)
                .arg(&last_id)
                .query_async(&mut conn)
                .await;

            match result {
                Ok(streams) => {
                    for (_stream_key, messages) in streams {
                        for (id, fields) in messages {
                            let signal_json = fields
                                .iter()
                                .find(|(k, _)| k == "signal" || k == "data")
                                .map(|(_, v)| v.as_str())
                                .unwrap_or("");

                            match super::schema::validate_and_create_event(signal_json) {
                                Ok(event) => {
                                    let event_id = event.signal.signal_id.clone();

                                    if !dedup.check_and_record(&event) {
                                        tracing::debug!(signal_id = %event_id, "Redis stream: duplicate signal dropped");
                                        continue;
                                    }

                                    if sender.send(event).await.is_err() {
                                        tracing::error!("Channel closed, stopping Redis ingest");
                                        return Err(PolybotError::ChannelClosed);
                                    }
                                }
                                Err(e) => {
                                    tracing::warn!(error = %e, "Redis stream: failed to parse signal");
                                }
                            }
                            last_id = id;
                        }
                    }
                }
                Err(e) => {
                    tracing::error!(error = %e, "Redis stream read error, retrying in 5s");
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redis_ingest_creation() {
        let _ingest = RedisIngest::new("redis://127.0.0.1:6379", "polybot:signals");
    }
}
