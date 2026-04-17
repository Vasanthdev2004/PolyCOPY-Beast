use thiserror::Error;

#[derive(Debug, Error)]
pub enum PolybotError {
    #[error("Scanner error: {0}")]
    Scanner(String),

    #[error("Risk engine error: {0}")]
    Risk(String),

    #[error("Execution error: {0}")]
    Execution(String),

    #[error("State error: {0}")]
    State(String),

    #[error("Config error: {0}")]
    Config(String),

    #[error("RPC pool error: {0}")]
    RpcPool(String),

    #[error("Validation error: {0}")]
    Validation(String),

    #[error("Redis error: {0}")]
    Redis(String),

    #[error("Telegram error: {0}")]
    Telegram(String),

    #[error("Channel closed")]
    ChannelClosed,

    #[error("Emergency stop active")]
    EmergencyStop,
}

pub type Result<T> = std::result::Result<T, PolybotError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display() {
        let err = PolybotError::Scanner("file not found".to_string());
        assert_eq!(format!("{}", err), "Scanner error: file not found");
    }

    #[test]
    fn error_conversion() {
        let result: Result<()> = Err(PolybotError::Config("missing field".to_string()));
        assert!(result.is_err());
    }
}
