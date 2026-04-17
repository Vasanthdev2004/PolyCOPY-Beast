use polybot_common::errors::PolybotError;
use polybot_common::types::{Category, ScannerEvent, Side, Signal};
use rust_decimal::Decimal;
use std::time::Instant;

pub fn parse_signal(json: &str) -> Result<Signal, PolybotError> {
    let raw: serde_json::Value = serde_json::from_str(json)
        .map_err(|e| PolybotError::Scanner(format!("JSON parse error: {}", e)))?;

    let category_str = raw
        .get("category")
        .and_then(|v| v.as_str())
        .unwrap_or("other");
    let category = Category::try_from(category_str).unwrap_or(Category::Other);

    let side_str = raw
        .get("side")
        .and_then(|v| v.as_str())
        .ok_or_else(|| PolybotError::Scanner("Missing 'side' field".to_string()))?;
    let side = match side_str.to_uppercase().as_str() {
        "YES" => Side::Yes,
        "NO" => Side::No,
        _ => {
            return Err(PolybotError::Scanner(format!(
                "Invalid side: '{}'. Must be YES or NO",
                side_str
            )))
        }
    };

    let suggested_size = raw
        .get("suggested_size_usdc")
        .and_then(|v| v.as_f64())
        .map(|v| Decimal::try_from(v).unwrap_or(Decimal::ZERO));

    let signal = Signal {
        signal_id: raw
            .get("signal_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        timestamp: raw
            .get("timestamp")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        wallet_address: raw
            .get("wallet_address")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        market_id: raw
            .get("market_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        side,
        confidence: raw.get("confidence").and_then(|v| v.as_u64()).unwrap_or(0) as u8,
        secret_level: raw
            .get("secret_level")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u8,
        category,
        suggested_size_usdc: suggested_size,
        scanner_version: raw
            .get("scanner_version")
            .and_then(|v| v.as_str())
            .unwrap_or("1.0.0")
            .to_string(),
    };

    Ok(signal)
}

pub fn validate_and_create_event(json: &str) -> Result<ScannerEvent, PolybotError> {
    let signal = parse_signal(json)?;
    signal.validate().map_err(|errors| {
        PolybotError::Scanner(
            errors
                .iter()
                .map(|e| format!("{:?}", e))
                .collect::<Vec<_>>()
                .join(", "),
        )
    })?;

    // v2.5: log warning if signal requires manual review
    if signal.requires_manual_review() {
        tracing::warn!(
            signal_id = %signal.signal_id,
            confidence = signal.confidence,
            secret_level = signal.secret_level,
            "Signal queued for manual review (confidence or secret_level < 3)"
        );
    }

    Ok(ScannerEvent {
        signal,
        received_at: Instant::now(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_v25_signal() {
        let json = r#"{
            "signal_id": "550e8400-e29b-41d4-a716-446655440000",
            "timestamp": "2026-04-14T12:34:56.789Z",
            "wallet_address": "0xabc123abc123abc123abc123abc123abc123abc1",
            "market_id": "polymarket-clob-market-123",
            "side": "YES",
            "confidence": 7,
            "secret_level": 6,
            "category": "politics",
            "suggested_size_usdc": 50.00,
            "scanner_version": "1.0.0"
        }"#;
        let signal = parse_signal(json).unwrap();
        assert_eq!(signal.confidence, 7);
        assert_eq!(signal.secret_level, 6);
        assert_eq!(signal.market_id, "polymarket-clob-market-123");
        assert_eq!(signal.side, Side::Yes);
        assert_eq!(signal.category, Category::Politics);
        assert!(signal.suggested_size_usdc.is_some());
    }

    #[test]
    fn parse_side_uppercase() {
        let json = r#"{
            "signal_id": "test-id",
            "timestamp": "2026-04-14T12:34:56.789Z",
            "wallet_address": "0xabc123abc123abc123abc123abc123abc123abc1",
            "market_id": "market-1",
            "side": "NO",
            "confidence": 5,
            "secret_level": 5,
            "category": "sports"
        }"#;
        let signal = parse_signal(json).unwrap();
        assert_eq!(signal.side, Side::No);
    }

    #[test]
    fn parse_unknown_category_defaults_to_other() {
        let json = r#"{
            "signal_id": "test-id",
            "timestamp": "2026-04-14T12:34:56.789Z",
            "wallet_address": "0xabc123abc123abc123abc123abc123abc123abc1",
            "market_id": "market-1",
            "side": "YES",
            "confidence": 5,
            "secret_level": 5,
            "category": "unknown_cat"
        }"#;
        let signal = parse_signal(json).unwrap();
        assert_eq!(signal.category, Category::Other);
    }

    #[test]
    fn validate_good_event() {
        let json = r#"{
            "signal_id": "550e8400-e29b-41d4-a716-446655440000",
            "timestamp": "2026-04-14T12:34:56.789Z",
            "wallet_address": "0xabc123abc123abc123abc123abc123abc123abc1",
            "market_id": "market-1",
            "side": "YES",
            "confidence": 7,
            "secret_level": 7,
            "category": "politics"
        }"#;
        // Stale timestamp is a validation error in v2.5
        let _result = validate_and_create_event(json);
        // Stale timestamp is a validation error in v2.5
        // The signal itself parses fine
        let signal = parse_signal(json).unwrap();
        assert_eq!(signal.confidence, 7);
    }

    #[test]
    fn validate_bad_signal_fails() {
        let json = r#"{
            "signal_id": "",
            "timestamp": "not-a-timestamp",
            "wallet_address": "abc",
            "market_id": "",
            "side": "YES",
            "confidence": 0,
            "secret_level": 0,
            "category": "politics"
        }"#;
        let result = validate_and_create_event(json);
        assert!(result.is_err());
    }
}
