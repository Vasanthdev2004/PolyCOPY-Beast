use rust_decimal::Decimal;

use super::positions::PositionManager;

/// Calculate unrealized PnL across all open positions.
/// In production, this would use real-time market prices from the CLOB orderbook.
/// For simulation mode, it estimates based on position entry data.
pub fn calculate_unrealized_pnl(
    manager: &PositionManager,
    current_prices: &std::collections::HashMap<String, Decimal>,
) -> Decimal {
    let positions = manager.get_positions_vec();
    positions
        .iter()
        .map(|pos| {
            if let Some(current_price) = current_prices.get(&pos.market_id) {
                // PnL = (current_price - average_entry_price) * current_size
                match pos.side {
                    polybot_common::types::Side::Yes => {
                        (current_price - pos.average_price) * pos.current_size
                    }
                    polybot_common::types::Side::No => {
                        (pos.average_price - current_price) * pos.current_size
                    }
                }
            } else {
                // No price data available — estimate 0 PnL for this position
                Decimal::ZERO
            }
        })
        .fold(Decimal::ZERO, |acc, pnl| acc + pnl)
}

pub fn calculate_realized_pnl_for_position(
    position: &polybot_common::types::Position,
    exit_price: Decimal,
) -> Decimal {
    match position.side {
        polybot_common::types::Side::Yes => {
            (exit_price - position.average_price) * position.current_size
        }
        polybot_common::types::Side::No => {
            (position.average_price - exit_price) * position.current_size
        }
    }
}

/// Calculate realized PnL from closed trades.
/// Realized PnL = sum of (exit_value - entry_value) for all closed trades.
#[allow(dead_code)]
pub fn calculate_realized_pnl(trades: &[polybot_common::types::Trade]) -> Decimal {
    trades
        .iter()
        .map(|_trade| {
            // For BUY trades: PnL = (filled_value - size_usd) if fully filled
            // Simplified: use the trade's filled_size * price as value
            // Real calculation needs matched entry/exit pairs
            Decimal::ZERO // Placeholder until full trade matching is implemented
        })
        .fold(Decimal::ZERO, |acc, pnl| acc + pnl)
}

/// Calculate total portfolio value = unrealized PnL + realized PnL + cash balance.
/// Returns (total_value, unrealized_pnl, realized_pnl).
#[allow(dead_code)]
pub fn calculate_portfolio_value(
    manager: &PositionManager,
    current_prices: &std::collections::HashMap<String, Decimal>,
    cash_balance: Decimal,
) -> (Decimal, Decimal, Decimal) {
    let unrealized = calculate_unrealized_pnl(manager, current_prices);
    let realized = Decimal::ZERO; // from trade history
    let total = cash_balance + unrealized + realized;
    (total, unrealized, realized)
}

/// Calculate current drawdown percentage from peak portfolio value.
#[allow(dead_code)]
pub fn calculate_drawdown_pct(current_value: Decimal, peak_value: Decimal) -> Decimal {
    if peak_value == Decimal::ZERO {
        return Decimal::ZERO;
    }
    let drawdown = (peak_value - current_value) / peak_value;
    drawdown.max(Decimal::ZERO) // Can't have negative drawdown
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn unrealized_pnl_empty() {
        let manager = PositionManager::new();
        let prices = std::collections::HashMap::new();
        assert_eq!(calculate_unrealized_pnl(&manager, &prices), Decimal::ZERO);
    }

    #[test]
    fn calculate_drawdown() {
        assert_eq!(calculate_drawdown_pct(dec!(9000), dec!(10000)), dec!(0.1)); // 10% drawdown
        assert_eq!(calculate_drawdown_pct(dec!(10000), dec!(10000)), dec!(0)); // no drawdown
        assert_eq!(calculate_drawdown_pct(dec!(11000), dec!(10000)), dec!(0)); // gain, not drawdown
    }

    #[test]
    fn portfolio_value() {
        let manager = PositionManager::new();
        let prices = std::collections::HashMap::new();
        let (total, unrealized, realized) =
            calculate_portfolio_value(&manager, &prices, dec!(5000));
        assert_eq!(total, dec!(5000));
        assert_eq!(unrealized, Decimal::ZERO);
        assert_eq!(realized, Decimal::ZERO);
    }

    #[test]
    fn realized_pnl_for_yes_position_uses_exit_price() {
        let position = polybot_common::types::Position {
            id: "p1".to_string(),
            market_id: "m1".to_string(),
            side: polybot_common::types::Side::Yes,
            entry_price: dec!(0.50),
            current_size: dec!(100),
            average_price: dec!(0.50),
            opened_at: chrono::Utc::now(),
            status: polybot_common::types::PositionStatus::Open,
            category: polybot_common::types::Category::Politics,
        };

        assert_eq!(
            calculate_realized_pnl_for_position(&position, dec!(0.70)),
            dec!(20)
        );
    }
}
