use std::collections::VecDeque;

use bybit::WsTrade;

/// Calculates the expected price given the old price, current price, and an
/// imbalance signal in [-1, 1].
pub fn expected_value(old_price: f64, curr_price: f64, imbalance: f64) -> f64 {
    let norm_imb = imbalance.abs();
    let price_change = (curr_price - old_price) * norm_imb;
    curr_price + price_change.copysign(imbalance)
}

/// Calculates the average of two prices.
pub fn mid_price_avg(old_mid: f64, curr_mid: f64) -> f64 {
    (old_mid + curr_mid) / 2.0
}

/// Calculates the basis of the average trade price relative to the mid price.
///
/// The basis is the difference between the average trade price and the mid
/// price. It is a good predictor of mid-price direction because of its
/// reversion back to 0: a negative basis means recent trades were closer to
/// the bid, so the mid price tends to decrease toward the average trade price,
/// and vice versa.
pub fn mid_price_basis(old_price: f64, curr_price: f64, avg_trade_price: f64) -> f64 {
    avg_trade_price - mid_price_avg(old_price, curr_price)
}

/// Calculates the logarithmic return between an old price and a current price.
///
/// Returns 0.0 if either price is non-positive to avoid NaN/inf propagation.
pub fn expected_return(old_price: f64, curr_price: f64) -> f64 {
    if old_price <= 0.0 || curr_price <= 0.0 {
        return 0.0;
    }
    (curr_price / old_price).ln()
}

/// Calculates the price fluctuation between the old and current price in
/// basis points (absolute difference divided by the current price).
pub fn price_flu(old_price: f64, curr_price: f64) -> f64 {
    if curr_price == 0.0 {
        return 0.0;
    }
    (curr_price - old_price).abs() * 10000.0 / curr_price
}

/// Calculates the incremental volume-weighted average price (VWAP) of the
/// trades received since the last update, normalized by the USD value of one
/// tick.
///
/// * `prev_avg` is returned unchanged when no new volume has traded.
/// * `tick_value` is the USD value of a one-tick price move. For
///   USDT-margined linear contracts the caller passes the book's tick size
///   as an approximation of the per-contract tick value.
///
/// The result is the VWAP of the new trades expressed in tick units.
pub fn avg_trade_price(
    curr_mid: f64,
    old_trades: Option<&VecDeque<WsTrade>>,
    curr_trades: &VecDeque<WsTrade>,
    prev_avg: f64,
    tick_value: f64,
) -> f64 {
    let Some(old_trades) = old_trades else {
        return curr_mid;
    };

    let mut old_volume = 0.0;
    let mut curr_volume = 0.0;
    let mut old_turnover = 0.0;
    let mut curr_turnover = 0.0;

    for v in old_trades {
        old_volume += v.volume;
        old_turnover += v.volume * v.price;
    }
    for v in curr_trades {
        curr_volume += v.volume;
        curr_turnover += v.volume * v.price;
    }

    if (old_volume - curr_volume).abs() > f64::EPSILON && tick_value > 0.0 {
        let vwap = (curr_turnover - old_turnover) / (curr_volume - old_volume);
        vwap / tick_value
    } else {
        prev_avg
    }
}
