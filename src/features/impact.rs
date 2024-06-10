use std::collections::VecDeque;

use bybit::model::WsTrade;
use skeleton::util::localorderbook::LocalBook;

/// Calculates the price impact of a trade based on the old and current order book state.
///
/// # Arguments
///
/// * `old_book` - The old order book state.
/// * `new_book` - The current order book state.
/// * `depth` - The depth of the order book to consider, if any.
///
/// # Returns
///
/// The price impact of the trade.
pub fn price_impact(new_book: LocalBook, old_book: LocalBook, depth: Option<usize>) -> f64 {
    // Calculate the volume at the bid and ask offsets
    let (mut old_bid_vol, mut curr_bid_vol, old_bid_price, curr_bid_price) = (
        old_book.best_bid.qty,
        new_book.best_bid.qty,
        old_book.best_bid.price,
        new_book.best_bid.price,
    );
    let (mut old_ask_vol, mut curr_ask_vol, old_ask_price, curr_ask_price) = (
        old_book.best_ask.qty,
        new_book.best_ask.qty,
        old_book.best_ask.price,
        new_book.best_ask.price,
    );

    // Calculate the volume at the depth, if provided
    if let Some(depth) = depth {
        old_bid_vol = 0.0;
        curr_bid_vol = 0.0;
        old_ask_vol = 0.0;
        curr_ask_vol = 0.0;

        // Iterate over the depth asks and bids in the old and new order books
        for (_, (_, qty)) in old_book.asks.iter().take(depth).enumerate() {
            old_ask_vol += qty;
        }
        for (_, (_, qty)) in new_book.asks.iter().take(depth).enumerate() {
            curr_ask_vol += qty;
        }
        for (_, (_, qty)) in old_book.bids.iter().rev().take(depth).enumerate() {
            old_bid_vol += qty;
        }
        for (_, (_, qty)) in new_book.bids.iter().rev().take(depth).enumerate() {
            curr_bid_vol += qty;
        }
    }

    // Calculate the volume at the bid and ask offsets
    let bid_impact = if curr_bid_price > old_bid_price || curr_bid_vol > old_bid_vol {
        curr_bid_vol - old_bid_vol
    } else if curr_bid_price < old_bid_price || curr_bid_vol < old_bid_vol {
        curr_bid_vol - old_bid_vol
    } else {
        0.0
    };
    let ask_impact = if curr_ask_price < old_ask_price || curr_ask_vol > old_ask_vol {
        curr_ask_vol - old_ask_vol
    } else if curr_ask_price > old_ask_price || curr_ask_vol < old_ask_vol {
        curr_ask_vol - old_ask_vol
    } else {
        0.0
    };

    // Return the sum of the bid and ask impacts
    bid_impact + ask_impact
}

/// Calculates the expected value of a trade based on the old price, current price, and imbalance.
///
/// # Arguments
///
/// * `old_price` - The old price of the trade.
/// * `curr_price` - The current price of the trade.
/// * `imbalance` - The imbalance of the trade.
///
/// # Returns
///
/// The expected value of the trade.
pub fn expected_value(old_price: f64, curr_price: f64, imbalance: f64) -> f64 {
    // Calculate the difference in price between the old and current prices.
    let diff = curr_price - old_price;

    // Calculate the expected value of the trade by multiplying the imbalance by the price difference.
    imbalance.abs() * diff
}

/// Calculates the change in the mid price relative to the average spread.
///
/// # Arguments
///
/// * `old_price` - The old price of the mid price.
/// * `curr_price` - The current price of the mid price.
/// * `avg_spread` - The average spread.
///
/// # Returns
///
/// The change in the mid price relative to the average spread.
pub fn mid_price_change(old_price: f64, curr_price: f64, avg_spread: f64) -> f64 {
    // Calculate the difference in price between the old and current prices.
    let diff = curr_price - old_price;

    // Calculate the change in the mid price relative to the average spread.
    diff / avg_spread
}

/// Calculates the difference between the current price and the old price.
///
/// # Arguments
///
/// * `old_price` - The old price.
/// * `curr_price` - The current price.
///
/// # Returns
///
/// The difference between the current price and the old price.
pub fn mid_price_diff(old_price: f64, curr_price: f64) -> f64 {
    // Calculate the difference between the current price and the old price.
    curr_price - old_price
}

/// Calculates the average of two prices.
///
/// # Arguments
///
/// * `old_price` - The first price.
/// * `curr_price` - The second price.
///
/// # Returns
///
/// The average of the two prices.
pub fn mid_price_avg(old_price: f64, curr_price: f64) -> f64 {
    // Calculate the average of the two prices by adding them together and dividing by two.
    (old_price + curr_price) / 2.0
}

/// Calculates the basis of the average trade price relative to the mid price.
///
/// The basis is the difference between the average trade price and the mid price.
///
/// # Arguments
///
/// * `old_price` - The old price of the mid price.
/// * `curr_price` - The current price of the mid price.
/// * `avg_trade_price` - The average trade price.
///
/// # Returns
///
/// The basis of the average trade price relative to the mid price.
/// Good predictor of midprice because of its reversion back to 0.
///
/// Using a time series on this value
/// If the basis is negative, recent trades were closer to the bid price as such midprice will decrease and revert to the avg trade price.
/// If the basis is positive, recent trades were closer to the ask price as such midprice will increase and revert to the avg trade price.
pub fn mid_price_basis(old_price: f64, curr_price: f64, avg_trade_price: f64) -> f64 {
    // Calculate the basis of the average trade price relative to the mid price.
    // The basis is the difference between the average trade price and the mid price.
    avg_trade_price - mid_price_avg(old_price, curr_price)
}

/// Calculates the average trade price based on the current mid price, the old trades,
/// the current trades, the previous average trade price, and the tick window.
///
/// # Arguments
///
/// * `curr_mid` - The current mid price.
/// * `old_trades` - The old trades.
/// * `curr_trades` - The current trades.
/// * `prev_avg` - The previous average trade price.
/// * `tick_window` - The tick window.
///
/// # Returns
///
/// The average trade price.
pub fn avg_trade_price(
    curr_mid: f64,
    old_trades: Option<&VecDeque<WsTrade>>,
    curr_trades: &VecDeque<WsTrade>,
    prev_avg: f64,
    tick_window: usize,
) -> f64 {
    // If there are no old trades, return the current mid price.
    if old_trades.is_none() {
        return curr_mid;
    }

    let mut old_volume = 0.0;
    let mut curr_volume = 0.0;
    let mut old_turnover = 0.0;
    let mut curr_turnover = 0.0;

    // Iterate over the old trades and calculate the cumulative volume and turnover.
    for v in old_trades.unwrap() {
        old_volume += v.volume;
        old_turnover += v.volume * v.price;
    }
    // Iterate over the current trades and calculate the cumulative volume and turnover.
    for v in curr_trades {
        curr_volume += v.volume;
        curr_turnover += v.volume * v.price;
    }

    // If the cumulative volume of the old trades is not equal to the cumulative volume of the
    // current trades, calculate the average trade price and return it.
    if old_volume != curr_volume {
        let inv_tick = 1.0 / tick_window as f64;
        ((old_turnover + curr_turnover) / (old_volume + curr_volume)) * inv_tick
    } else {
        // Otherwise, return the previous average trade price.
        prev_avg
    }
}
