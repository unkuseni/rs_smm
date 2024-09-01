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
        // Calculate the weighted bid quantity using the specified depth.
        weighted_bid_qty = calculate_weighted_bid(book, depth);
        // Calculate the weighted ask quantity using the specified depth.
        weighted_ask_qty = calculate_weighted_bid(book, depth);
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

pub fn calculate_ofi(book: &LocalBook, prev_book: &LocalBook, depth: Option<usize>) -> f64 {
    let bid_ofi = {
        if book.best_bid.price > prev_book.best_bid.price {
            if let Some(depth) = depth {
                let weighted_bid = calculate_weighted_bid(book, depth);
                weighted_bid
            } else {
                book.best_bid.qty
            }
        } else if book.best_bid.price == prev_book.best_bid.price {
            if let Some(depth) = depth {
                let weighted_bid = calculate_weighted_bid(book, depth);
                let prev_weighted_bid = calculate_weighted_bid(prev_book, depth);
                weighted_bid - prev_weighted_bid
            } else {
                book.best_bid.qty - prev_book.best_bid.qty
            }
        } else {
            if let Some(depth) = depth {
                let weighted_bid = calculate_weighted_bid(book, depth);
                -weighted_bid
            } else {
                -book.best_bid.qty
            }
        }
    };
    let ask_ofi = {
        if book.best_ask.price < prev_book.best_ask.price {
            if let Some(depth) = depth {
                let weighted_ask = calculate_weighted_ask(book, depth);
                -weighted_ask
            } else {
                -book.best_ask.qty
            }
        } else if book.best_ask.price == prev_book.best_ask.price {
            if let Some(depth) = depth {
                let weighted_ask = calculate_weighted_ask(book, depth);
                let prev_weighted_ask = calculate_weighted_ask(prev_book, depth);
                prev_weighted_ask - weighted_ask
            } else {
                prev_book.best_ask.qty - book.best_ask.qty
            }
        } else {
            if let Some(depth) = depth {
                let weighted_ask = calculate_weighted_ask(book, depth);
                weighted_ask
            } else {
                book.best_ask.qty
            }
        }
    };
    let ofi = ask_ofi + bid_ofi;

    ofi
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
                let curr_bid_qty = calculate_weighted_bid(book, depth);
                let prev_bid_qty = calculate_weighted_bid(prev_book, depth);
                curr_bid_qty - prev_bid_qty
            } else {
                book.best_bid.qty - prev_book.best_bid.qty
            }
        }
        x if x > prev_book.best_bid.price => {
            if let Some(depth) = depth {
                let curr_bid = calculate_weighted_bid(book, depth);
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
                let curr_ask = calculate_weighted_ask(book, depth);
                curr_ask
            } else {
                book.best_ask.qty
            }
        }
        x if x == prev_book.best_ask.price => {
            if let Some(depth) = depth {
                let curr_ask_qty = calculate_weighted_ask(book, depth);
                let prev_ask_qty = calculate_weighted_ask(prev_book, depth);
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
    let (total_volume, buy_volume) = trades.iter().fold((0.0, 0.0), |(total, buy), trade| {
        let new_total = total + trade.volume;
        let new_buy = if trade.side == "Buy" {
            buy + trade.volume
        } else {
            buy
        };
        (new_total, new_buy)
    });
    (total_volume, buy_volume)
}

pub fn map_range(value: f64) -> f64 {
    (value + 1.0) / 2.0
}

/// Calculates the weighted ask quantity using the specified depth.
///
/// The weighted ask quantity is the sum of the ask quantities multiplied by a weight that
/// decreases as the price moves further away from the best ask.
///
/// # Arguments
///
/// * `book`: The order book to calculate the weighted ask quantity from.
/// * `depth`: The number of levels to calculate the weighted ask quantity from.
///
/// # Returns
///
/// The weighted ask quantity as a `f64`.
fn calculate_weighted_ask(book: &LocalBook, depth: usize) -> f64 {
    book.asks
        .iter()
        .take(depth)
        .enumerate()
        .map(|(i, (_, qty))| (calculate_exponent(i as f64) * qty) as f64)
        .sum::<f64>()
}

/// Calculates the weighted bid quantity using the specified depth.
///
/// The weighted bid quantity is the sum of the bid quantities multiplied by a weight that
/// decreases as the price moves further away from the best bid.
///
/// # Arguments
///
/// * `book`: The order book to calculate the weighted bid quantity from.
/// * `depth`: The number of levels to calculate the weighted bid quantity from.
///
/// # Returns
///
/// The weighted bid quantity as a `f64`.
fn calculate_weighted_bid(book: &LocalBook, depth: usize) -> f64 {
    book.bids
        .iter()
        .rev()
        .take(depth)
        .enumerate()
        .map(|(i, (_, qty))| (calculate_exponent(i as f64) * qty) as f64)
        .sum::<f64>()
}
