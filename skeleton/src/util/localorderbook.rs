use bybit::model::{Ask, Bid};
use ordered_float::OrderedFloat;
use std::collections::BTreeMap;

use super::helpers::generate_timestamp;

#[derive(Debug, Clone)]
pub struct LocalBook {
    pub asks: BTreeMap<OrderedFloat<f64>, f64>,
    pub bids: BTreeMap<OrderedFloat<f64>, f64>,
    pub best_ask: Ask,
    pub best_bid: Bid,
    pub last_update: u64,
}

impl LocalBook {
    pub fn new() -> Self {
        Self {
            asks: BTreeMap::new(),
            bids: BTreeMap::new(),
            last_update: generate_timestamp(),
            best_ask: Ask {
                price: 0.0,
                qty: 0.0,
            },
            best_bid: Bid {
                price: 0.0,
                qty: 0.0,
            },
        }
    }

    /// Updates the order book with the given list of bids and asks and a timestamp.
    /// If the timestamp is not newer than the last update, the function returns early.
    ///
    /// For each bid in the input list, it updates the bid quantity at the corresponding price in the order book.
    /// If the price does not exist in the order book, it adds a new entry for the bid price and quantity.
    ///
    /// For each ask in the input list, it updates the ask quantity at the corresponding price in the order book.
    /// If the price does not exist in the order book, it adds a new entry for the ask price and quantity.
    ///
    /// After updating the bids and asks, it removes any entries with a quantity of 0 from both the bid and ask order books.
    ///
    /// Finally, it updates the last_update timestamp to the input timestamp.
    pub fn update(&mut self, bids: Vec<Bid>, asks: Vec<Ask>, timestamp: u64) {
        if timestamp <= self.last_update {
            return;
        }

        for bid in bids.iter() {
            let price = OrderedFloat::from(bid.price);
            self.bids
                .entry(price)
                .and_modify(|qty| *qty = bid.qty)
                .or_insert(bid.qty);
        }

        for ask in asks.iter() {
            let price = OrderedFloat::from(ask.price);
            self.asks
                .entry(price)
                .and_modify(|qty| *qty = ask.qty)
                .or_insert(ask.qty);
        }

        self.bids.retain(|_, &mut v| v != 0.0);
        self.asks.retain(|_, &mut v| v != 0.0);

        self.last_update = timestamp;
    }

    /// Update the order book with the given bids, asks, and timestamp.
    pub fn update_bba(&mut self, bids: Vec<Bid>, asks: Vec<Ask>, timestamp: u64) {
        // If the timestamp is not newer than the last update, return early
        if timestamp <= self.last_update {
            return;
        }

        // Update the bids in the order book
        for bid in bids.iter() {
            let price = OrderedFloat::from(bid.price);
            // Modify or insert the bid price and quantity into the bids HashMap
            self.bids
                .entry(price)
                .and_modify(|qty| *qty = bid.qty)
                .or_insert(bid.qty);
            // Remove bids with prices higher than the current bid price
            self.bids.retain(|&key, _| key <= price);
        }

        for ask in asks.iter() {
            let price = OrderedFloat::from(ask.price);
            // Modify or insert the ask price and quantity into the asks HashMap
            self.asks
                .entry(price)
                .and_modify(|qty| *qty = ask.qty)
                .or_insert(ask.qty);
            // Remove asks with prices lower than the current ask price
            self.asks.retain(|&key, _| key >= price);
        }

        // Remove any bids with quantity equal to 0
        self.bids.retain(|_, &mut v| v != 0.0);
        // Remove any asks with quantity equal to 0
        self.asks.retain(|_, &mut v| v != 0.0);

        // Set the best bid based on the highest bid price and quantity in the order book
        self.best_bid = self
            .bids
            .iter()
            .next_back()
            .map(|(price, qty)| Bid {
                price: **price,
                qty: *qty,
            })
            .unwrap_or_else(|| Bid {
                price: 0.0,
                qty: 0.0,
            });
        // Set the best ask based on the lowest ask price and quantity in the order boo
        self.best_ask = self
            .asks
            .iter()
            .next()
            .map(|(price, qty)| Ask {
                price: **price,
                qty: *qty,
            })
            .unwrap_or_else(|| Ask {
                price: 0.0,
                qty: 0.0,
            });
        // Update the last update timestamp
        self.last_update = timestamp;
    }

    /// Get the best ask prices and quantities in the order book.
    pub fn get_best_ask(&self) -> Ask {
        self.best_ask.clone()
    }

    /// Get the best bid prices and quantities in the order book.
    pub fn get_best_bid(&self) -> Bid {
        self.best_bid.clone()
    }

    /// Get the best ask and bid prices and quantities in the order book.
    pub fn get_bba(&self) -> (Bid, Ask) {
        (self.best_bid.clone(), self.best_ask.clone())
    }
}
unsafe impl Send for LocalBook {}
