use polybot_common::constants::{
    confidence_multiplier, secret_level_multiplier, MAX_POSITION_USDC, MIN_POSITION_USDC,
};
use rust_decimal::Decimal;

/// v2.5 Position sizing: base_size × confidence_mult × secret_level_mult × drawdown_factor
/// Confidence multiplier uses the confidence score (1-10).
/// Secret level multiplier uses the secret_level (1-10).
/// Result is clamped to [MIN_POSITION_USDC, max_by_category].
#[allow(dead_code)]
pub fn calculate_position_size(
    base_size_usd: Decimal,
    confidence: u8,
    secret_level: u8,
    drawdown_factor: Decimal,
    category_max_usd: Decimal,
) -> Decimal {
    let conf = confidence_multiplier(confidence);
    let sl = secret_level_multiplier(secret_level);

    // If either multiplier is 0.0 (blocked), return 0
    if conf == Decimal::ZERO || sl == Decimal::ZERO {
        return Decimal::ZERO;
    }

    let mut size = base_size_usd * conf * sl * drawdown_factor;

    // v2.5: Cap by per-category max single position
    let max_allowed = MAX_POSITION_USDC.min(category_max_usd);
    if size > max_allowed {
        size = max_allowed;
    }

    // v2.5: Hard minimum of $5
    if size < MIN_POSITION_USDC && size > Decimal::ZERO {
        size = MIN_POSITION_USDC;
    }

    size
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn position_size_high_confidence_high_secret() {
        // confidence=8: mult=1.5, secret_level=7: mult=1.3, drawdown=1.0
        let size = calculate_position_size(dec!(50), 8, 7, dec!(1.0), dec!(250));
        assert_eq!(size, dec!(97.5)); // 50 * 1.5 * 1.3 * 1.0
    }

    #[test]
    fn position_size_low_confidence_blocked() {
        // confidence=3: mult=0.0 (blocked)
        let size = calculate_position_size(dec!(50), 3, 7, dec!(1.0), dec!(250));
        assert_eq!(size, dec!(0));
    }

    #[test]
    fn position_size_low_secret_blocked() {
        // secret_level=2: mult=0.0 (blocked)
        let size = calculate_position_size(dec!(50), 7, 2, dec!(1.0), dec!(250));
        assert_eq!(size, dec!(0));
    }

    #[test]
    fn position_size_capped_by_category() {
        // crypto max = $150
        let size = calculate_position_size(dec!(50), 10, 10, dec!(1.0), dec!(150));
        assert_eq!(size, dec!(150)); // capped by category
    }

    #[test]
    fn position_size_medium_confidence() {
        // confidence=6: mult=1.0, secret_level=6: mult=1.0, drawdown=0.75
        let size = calculate_position_size(dec!(50), 6, 6, dec!(0.75), dec!(250));
        assert_eq!(size, dec!(37.5)); // 50 * 1.0 * 1.0 * 0.75
    }

    #[test]
    fn position_size_drawdown_zero_blocks() {
        let size = calculate_position_size(dec!(50), 7, 7, dec!(0.0), dec!(250));
        assert_eq!(size, dec!(0));
    }
}
