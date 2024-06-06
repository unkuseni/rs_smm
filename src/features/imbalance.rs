use std::collections::VecDeque;

use bybit::model::WsTrade;
use skeleton::util::{helpers::calculate_exponent, localorderbook::LocalBook};

/// Calculate the imbalance ratio of a LocalBook.
///
/// The imbalance ratio is the difference between the bid and ask quantities
/// divided by their sum.
///
/// # Arguments
///
/// * `book` - The LocalBook to calculate the imbalance ratio from.
///
/// # Returns
///
/// The imbalance ratio as a `f64`.
pub fn imbalance_ratio(book: LocalBook) -> f64 {
    // Get the best ask and best bid quantities from the LocalBook.
    let (best_ask, best_bid) = (book.best_ask, book.best_bid);

    // Calculate the difference between the bid and ask quantities.
    let diff = best_bid.qty - best_ask.qty;

    // Calculate the sum of the bid and ask quantities.
    let sum = best_bid.qty + best_ask.qty;

    // Calculate and return the imbalance ratio.
    let ratio = diff / sum;
    match ratio {
        x if x.is_nan() => 0.0,
        x if x > 0.20 => x,
        x if x < -0.20 => x,
        _ => 0.0,
    }
}

/// Calculate the imbalance ratio of a LocalBook at a specified depth.
///
/// The imbalance ratio is the difference between the weighted bid quantity and
/// the weighted ask quantity divided by their sum.
///
/// # Arguments
///
/// * `book` - The LocalBook to calculate the imbalance ratio from.
/// * `depth` - The number of orders to consider at each side of the book.
///
/// # Returns
///
/// The imbalance ratio as a `f64`.
pub fn imbalance_ratio_at_depth(book: LocalBook, depth: usize) -> f64 {
    // Initialize variables to store the weighted bid and ask quantities.
    let mut weighted_bid_qty = 0.0;
    let mut weighted_ask_qty = 0.0;

    // Calculate the weighted ask quantity.
    for (i, (_, qty)) in book.asks.iter().take(depth).enumerate() {
        // Calculate the weight based on the order's index.
        let weight = calculate_exponent(i as f64);
        // Update the weighted ask quantity.
        weighted_ask_qty += weight * qty;
    }

    // Calculate the weighted bid quantity.
    for (i, (_, qty)) in book.bids.iter().rev().take(depth).enumerate() {
        // Calculate the weight based on the order's index.
        let weight = calculate_exponent(i as f64);
        // Update the weighted bid quantity.
        weighted_bid_qty += weight * qty;
    }

    // Calculate and return the imbalance ratio.
    let ratio = (weighted_bid_qty - weighted_ask_qty) / (weighted_bid_qty + weighted_ask_qty);
    match ratio {
        x if x.is_nan() => 0.0,
        x if x > 0.20 => x,
        x if x < -0.20 => x,
        _ => 0.0,
    }
}

pub fn voi(book: LocalBook, prev_book: LocalBook) -> f64 {
    let bid_v = match book.best_bid.price {
        x if x < prev_book.best_bid.price => 0.0,
        x if x == prev_book.best_bid.price => book.best_bid.qty - prev_book.best_bid.qty,
        x if x > prev_book.best_bid.price => book.best_bid.qty,
        _ => 0.0,
    };

    let ask_v = match book.best_ask.price {
        x if x < prev_book.best_ask.price => book.best_ask.qty,
        x if x == prev_book.best_ask.price => book.best_ask.qty - prev_book.best_ask.qty,
        x if x > prev_book.best_ask.price => 0.0,
        _ => 0.0,
    };

    let diff = bid_v - ask_v;
    diff
}

pub fn trade_imbalance(trades: (String, VecDeque<WsTrade>)) -> (String, f64) {
    // Calculate total volume and buy volume
    let (total_volume, buy_volume) = calculate_volumes(&trades.1);
    // Handle empty trade history (optional)
    if total_volume == 0.0 {
        // You can either return an empty tuple or a specific value to indicate no trades
        return (trades.0, 0.0);
    }
    // Calculate buy-sell ratio (avoid division by zero)
    let ratio = buy_volume / total_volume;
    (trades.0, ratio)
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
