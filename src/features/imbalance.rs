use std::collections::VecDeque;

use bybit::model::WsTrade;
use skeleton::util::{helpers::calculate_exponent, localorderbook::LocalBook};

/// Calculate the imbalance ratio of a LocalBook.
///
/// The imbalance ratio is the difference between the bid and ask quantities
/// divided by their sum.
///
/// If imbalance is negative, increase the bid spread and reduce the opposite.
/// If imbalance is positive, increase the ask spread and reduce the opposite.
/// # Arguments
///
/// * `book` - The LocalBook to calculate the imbalance ratio from.
/// * `depth` - The depth of the bid/ask orders to consider. If `None`, the entire order book is used.
///
/// # Returns
///
/// The imbalance ratio as a `f64`.
pub fn imbalance_ratio(book: &LocalBook, depth: Option<usize>) -> f64 {
    // Extract the best ask and bid from the book.
    let (best_ask, best_bid) = (book.best_ask.qty, book.best_bid.qty);

    // Initialize the weighted bid and ask quantities to the quantities of the best bid and ask.
    let (mut weighted_bid_qty, mut weighted_ask_qty) = (best_bid, best_ask);

    // If a depth is specified, calculate the weighted bid and ask quantities using the specified depth.
    if let Some(depth) = depth {
        // Reset the weighted bid and ask quantities to 0.
        weighted_bid_qty = 0.0;
        weighted_ask_qty = 0.0;

        // Calculate the weighted ask quantity using the specified depth.
        for (i, (_, qty)) in book.asks.iter().take(depth).enumerate() {
            // Calculate the weight using the exponentiation function.
            let weight = calculate_exponent(i as f64);
            // Add the weighted quantity to the weighted ask quantity.
            weighted_ask_qty += weight * qty;
        }

        // Calculate the weighted bid quantity using the specified depth.
        for (i, (_, qty)) in book.bids.iter().rev().take(depth).enumerate() {
            // Calculate the weight using the exponentiation function.
            let weight = calculate_exponent(i as f64);
            // Add the weighted quantity to the weighted bid quantity.
            weighted_bid_qty += weight * qty;
        }
    }

    // Calculate the difference between the weighted bid and ask quantities.
    let diff = weighted_bid_qty - weighted_ask_qty;
    // Calculate the sum of the weighted bid and ask quantities.
    let sum = weighted_bid_qty + weighted_ask_qty;
    // Calculate the imbalance ratio by dividing the difference by the sum.
    let ratio = diff / sum;

    // Return the imbalance ratio, checking for NaN and out-of-range values.
    match ratio {
        x if x.is_nan() => 0.0, // If NaN, return 0.
        x if x > 0.20 => x,     // If positive and greater than 0.20, return the ratio.
        x if x < -0.20 => x,    // If negative and less than -0.20, return the ratio.
        _ => 0.0,               // Otherwise, return 0.
    }
}

/// Calculates the Volume at the Offset (VOI) of a given LocalBook and its previous state.
///
/// # Arguments
///
/// * `book` - The current LocalBook.
/// * `prev_book` - The previous LocalBook.
/// * `depth` - The depth of the bid/ask orders to consider.
///
/// # Returns
///
/// The volume at the offset as a `f64`.
pub fn voi(book: &LocalBook, prev_book: &LocalBook, depth: Option<usize>) -> f64 {
    // Calculate the volume at the bid side
    let bid_v = match book.best_bid.price {
        x if x < prev_book.best_bid.price => 0.0,
        x if x == prev_book.best_bid.price => {
            if let Some(depth) = depth {
                let mut curr_bid_qty = 0.0;
                let mut prev_bid_qty = 0.0;
                // Iterate over the depth bids in the current and previous books
                for (i, (_, qty)) in book.bids.iter().rev().take(depth).enumerate() {
                    curr_bid_qty += qty * calculate_exponent(i as f64);
                }
                for (i, (_, qty)) in prev_book.bids.iter().rev().take(depth).enumerate() {
                    prev_bid_qty += qty * calculate_exponent(i as f64);
                }
                curr_bid_qty - prev_bid_qty
            } else {
                book.best_bid.qty - prev_book.best_bid.qty
            }
        }
        x if x > prev_book.best_bid.price => {
            if let Some(depth) = depth {
                let mut curr_bid = 0.0;
                // Iterate over the depth bids in the current book
                for (i, (_, qty)) in book.bids.iter().rev().take(depth).enumerate() {
                    curr_bid += qty * calculate_exponent(i as f64);
                }
                curr_bid
            } else {
                book.best_bid.qty
            }
        }
        _ => 0.0,
    };

    // Calculate the volume at the ask side
    let ask_v = match book.best_ask.price {
        x if x < prev_book.best_ask.price => {
            if let Some(depth) = depth {
                let mut curr_ask = 0.0;
                // Iterate over the depth asks in the current book
                for (i, (_, qty)) in book.asks.iter().take(depth).enumerate() {
                    curr_ask += qty * calculate_exponent(i as f64);
                }
                curr_ask
            } else {
                book.best_ask.qty
            }
        }
        x if x == prev_book.best_ask.price => {
            if let Some(depth) = depth {
                let mut curr_ask_qty = 0.0;
                let mut prev_ask_qty = 0.0;
                // Iterate over the depth asks in the current and previous books
                for (i, (_, qty)) in book.asks.iter().take(depth).enumerate() {
                    curr_ask_qty += qty * calculate_exponent(i as f64);
                }
                for (i, (_, qty)) in prev_book.bids.iter().take(depth).enumerate() {
                    prev_ask_qty += qty * calculate_exponent(i as f64);
                }
                curr_ask_qty - prev_ask_qty
            } else {
                book.best_ask.qty - prev_book.best_ask.qty
            }
        }
        x if x > prev_book.best_ask.price => 0.0,
        _ => 0.0,
    };

    // Calculate the volume at the offset
    let diff = bid_v - ask_v;
    diff
}

pub fn trade_imbalance(trades: &VecDeque<WsTrade>) -> f64 {
    // Calculate total volume and buy volume
    let (total_volume, buy_volume) = calculate_volumes(trades);
    // Handle empty trade history (optional)
    if total_volume == 0.0 {
        // You can either return an empty tuple or a specific value to indicate no trades
        return 0.0;
    }
    // Calculate buy-sell ratio (avoid division by zero)
    let ratio = buy_volume / total_volume;
    ratio
}

fn calculate_volumes(trades: &VecDeque<WsTrade>) -> (f64, f64) {
    let mut total_volume = 0.0;
    let mut buy_volume = 0.0;
    for trade in trades.iter() {
        total_volume += trade.volume;
        if trade.side == "Buy" {
            buy_volume += trade.volume;
        }
    }
    (total_volume, buy_volume)
}

pub fn map_range(value: f64) -> f64 {
    (value + 1.0) / 2.0
}
