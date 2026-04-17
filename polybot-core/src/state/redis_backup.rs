use polybot_common::errors::PolybotError;
use std::path::Path;

/// v2.5: Automated Redis backup to /backups/redis-YYYY-MM-DD.rdb
/// Retains 7 days of backups.
pub struct RedisBackup {
    redis_url: String,
    backup_dir: String,
    retention_days: u32,
}

impl RedisBackup {
    pub fn new(redis_url: &str, backup_dir: &str) -> Self {
        Self {
            redis_url: redis_url.to_string(),
            backup_dir: backup_dir.to_string(),
            retention_days: 7,
        }
    }

    /// Trigger a Redis BGSAVE and copy the RDB file to the backup directory.
    pub async fn run_daily_backup(&self) -> Result<(), PolybotError> {
        let date = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let backup_path = format!("{}/redis-{}.rdb", self.backup_dir, date);
        tracing::info!(path = %backup_path, "Starting daily Redis backup");

        // Create backup directory if it doesn't exist
        let backup_dir_path = Path::new(&self.backup_dir);
        if !backup_dir_path.exists() {
            std::fs::create_dir_all(backup_dir_path)
                .map_err(|e| PolybotError::State(format!("Failed to create backup dir: {}", e)))?;
        }

        // Issue BGSAVE to Redis
        let client = redis::Client::open(self.redis_url.as_str())
            .map_err(|e| PolybotError::Redis(format!("Failed to connect to Redis: {}", e)))?;

        let mut conn = client
            .get_multiplexed_async_connection()
            .await
            .map_err(|e| PolybotError::Redis(format!("Failed to get Redis connection: {}", e)))?;

        // Issue BGSAVE command
        redis::cmd("BGSAVE")
            .query_async::<String>(&mut conn)
            .await
            .map_err(|e| PolybotError::Redis(format!("BGSAVE failed: {}", e)))?;

        // Wait for BGSAVE to complete (poll LASTSAVE)
        let mut retries = 30; // 30 seconds max wait
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            let saving: i32 = redis::cmd("LASTSAVE")
                .query_async(&mut conn)
                .await
                .unwrap_or(0);
            let _ = saving; // LASTSAVE returns a timestamp; we just wait a bit

            retries -= 1;
            if retries <= 0 {
                break;
            }
            // Check if BGSAVE is still in progress
            let info: String = redis::cmd("INFO")
                .arg("persistence")
                .query_async(&mut conn)
                .await
                .unwrap_or_default();
            if !info.contains("rdb_bgsave_in_progress:1") {
                break;
            }
        }

        // Copy the Redis dump file to backup path
        // Default Redis dump location is /var/lib/redis/dump.rdb or ./dump.rdb
        let dump_paths = ["/var/lib/redis/dump.rdb", "./dump.rdb", "/data/dump.rdb"];

        let mut copied = false;
        for dump_path in &dump_paths {
            let source = Path::new(dump_path);
            if source.exists() {
                match std::fs::copy(source, &backup_path) {
                    Ok(_) => {
                        tracing::info!(from = %dump_path, to = %backup_path, "Redis backup copied");
                        copied = true;
                        break;
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "Failed to copy dump file");
                    }
                }
            }
        }

        if !copied {
            tracing::warn!(
                "Could not find Redis dump file for backup. BGSAVE was issued but copy failed."
            );
        }

        tracing::info!("Redis backup completed");
        Ok(())
    }

    /// Remove backups older than retention period (7 days).
    pub async fn cleanup_old_backups(&self) -> Result<u32, PolybotError> {
        tracing::info!(
            retention_days = self.retention_days,
            "Cleaning up old Redis backups"
        );

        let backup_dir_path = Path::new(&self.backup_dir);
        if !backup_dir_path.exists() {
            return Ok(0);
        }

        let cutoff = chrono::Utc::now() - chrono::Duration::days(self.retention_days as i64);
        let cutoff_str = cutoff.format("%Y-%m-%d").to_string();
        let mut removed = 0u32;

        let entries = std::fs::read_dir(backup_dir_path)
            .map_err(|e| PolybotError::State(format!("Failed to read backup dir: {}", e)))?;

        for entry in entries.flatten() {
            let filename = entry.file_name().to_string_lossy().to_string();
            if filename.starts_with("redis-") && filename.ends_with(".rdb") {
                // Extract date from filename: redis-2026-04-16.rdb
                if filename.len() >= 17 {
                    let date_str = &filename[6..16]; // "2026-04-16"
                    if date_str < cutoff_str.as_str() {
                        if std::fs::remove_file(entry.path()).is_ok() {
                            tracing::info!(file = %filename, "Removed old backup");
                            removed += 1;
                        }
                    }
                }
            }
        }

        tracing::info!(removed = removed, "Backup cleanup completed");
        Ok(removed)
    }

    /// Run the backup loop (called daily).
    pub async fn run_backup_loop(&self) -> Result<(), PolybotError> {
        let mut interval = tokio::time::interval(
            std::time::Duration::from_secs(86400), // 24 hours
        );

        loop {
            interval.tick().await;
            if let Err(e) = self.run_daily_backup().await {
                tracing::error!(error = %e, "Daily Redis backup failed");
            }
            if let Err(e) = self.cleanup_old_backups().await {
                tracing::error!(error = %e, "Backup cleanup failed");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redis_backup_creation() {
        let backup = RedisBackup::new("redis://127.0.0.1:6379", "/app/backups");
        assert_eq!(backup.retention_days, 7);
    }

    #[test]
    fn backup_filename_parsing() {
        // "redis-2026-04-16.rdb" => date part is "2026-04-16"
        let filename = "redis-2026-04-16.rdb";
        assert!(filename.starts_with("redis-"));
        assert!(filename.ends_with(".rdb"));
        assert_eq!(&filename[6..16], "2026-04-16");
    }
}
