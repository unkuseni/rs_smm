use std::collections::VecDeque;

use bybit::WsTrade;
use skeleton::util::localorderbook::{calculate_weighted_ask, calculate_weighted_bid, LocalBook};

/// Calculates the imbalance ratio of a `LocalBook` at the given depth.
///
/// The imbalance ratio is the difference between the weighted bid and ask
/// quantities divided by their sum, and lies in [-1, 1].
///
/// A deadband of ±0.20 is applied: values inside the band are returned as 0.0
/// so that only meaningful buy/sell pressure reaches the skew calculation.
///
/// # Arguments
///
/// * `book` - The order book to calculate the imbalance ratio from.
/// * `depth` - The depth of the bid/ask orders to consider. If `None`, the entire order book is used.
pub fn imbalance_ratio(book: &LocalBook, depth: Option<usize>) -> f64 {
    // Initialize the weighted bid and ask quantities to the quantities of the best bid and ask.
    let (weighted_bid_qty, weighted_ask_qty) = if let Some(depth) = depth {
        (
            calculate_weighted_bid(book, depth),
            calculate_weighted_ask(book, depth),
        )
    } else {
        (book.best_bid.qty, book.best_ask.qty)
    };

    // Calculate the difference between the weighted bid and ask quantities.
    let diff = weighted_bid_qty - weighted_ask_qty;
    // Calculate the sum of the weighted bid and ask quantities.
    let sum = weighted_bid_qty + weighted_ask_qty;
    // Calculate the imbalance ratio by dividing the difference by the sum.
    let ratio = diff / sum;

    // Return the imbalance ratio, guarding against NaN and applying the deadband.
    match ratio {
        x if x.is_nan() => 0.0,
        x if x > 0.20 => x,
        x if x < -0.20 => x,
        _ => 0.0,
    }
}

/// Calculates the Order Flow Imbalance (OFI) between two order book states.
///
/// # Arguments
///
/// * `book` - The current order book.
/// * `prev_book` - The previous order book.
/// * `depth` - The depth of the bid/ask orders to consider. If `None`, only the best bid and ask are used.
pub fn calculate_ofi(book: &LocalBook, prev_book: &LocalBook, depth: Option<usize>) -> f64 {
    let bid_ofi = {
        if book.best_bid.price > prev_book.best_bid.price {
            if let Some(depth) = depth {
                calculate_weighted_bid(book, depth)
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
        } else if let Some(depth) = depth {
            -calculate_weighted_bid(book, depth)
        } else {
            -book.best_bid.qty
        }
    };
    let ask_ofi = {
        if book.best_ask.price < prev_book.best_ask.price {
            if let Some(depth) = depth {
                -calculate_weighted_ask(book, depth)
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
        } else if let Some(depth) = depth {
            calculate_weighted_ask(book, depth)
        } else {
            book.best_ask.qty
        }
    };
    ask_ofi + bid_ofi
}

/// Calculates the Volume of Interest (VOI) between two order book states.
///
/// # Arguments
///
/// * `book` - The current order book.
/// * `prev_book` - The previous order book.
/// * `depth` - The depth of the bid/ask orders to consider.
pub fn voi(book: &LocalBook, prev_book: &LocalBook, depth: Option<usize>) -> f64 {
    // Calculate the volume at the bid side.
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
                calculate_weighted_bid(book, depth)
            } else {
                book.best_bid.qty
            }
        }
        _ => 0.0,
    };

    // Calculate the volume at the ask side.
    let ask_v = match book.best_ask.price {
        x if x < prev_book.best_ask.price => {
            if let Some(depth) = depth {
                calculate_weighted_ask(book, depth)
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

    bid_v - ask_v
}

/// Calculates the trade imbalance over a window of trades.
///
/// Returns a value in [-1, 1]: 1.0 means all volume was on the buy side and
/// -1.0 means all volume was on the sell side. Returns 0.0 when the window is empty.
pub fn trade_imbalance(trades: &VecDeque<WsTrade>) -> f64 {
    // Calculate total volume and buy volume.
    let (total_volume, buy_volume) = calculate_volumes(trades);
    if total_volume == 0.0 {
        return 0.0;
    }
    // Map the buy ratio from [0, 1] to [-1, 1].
    let ratio = buy_volume / total_volume;
    2.0 * ratio - 1.0
}

fn calculate_volumes(trades: &VecDeque<WsTrade>) -> (f64, f64) {
    trades.iter().fold((0.0, 0.0), |(total, buy), trade| {
        let new_total = total + trade.volume;
        let new_buy = if trade.side.eq_ignore_ascii_case("Buy") {
            buy + trade.volume
        } else {
            buy
        };
        (new_total, new_buy)
    })
}
