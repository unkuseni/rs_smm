use bybit::model::{Ask, Bid};
use ordered_float::OrderedFloat;
use std::collections::BTreeMap;

use super::helpers::spread_price_in_bps;
#[derive(Debug, Clone)]
pub struct LocalBook {
    pub asks: BTreeMap<OrderedFloat<f64>, f64>,
    pub bids: BTreeMap<OrderedFloat<f64>, f64>,
    pub best_ask: Ask,
    pub best_bid: Bid,
    pub mid_price: f64,
    pub tick_size: f64,
    pub lot_size: f64,
    pub min_order_size: f64,
    pub post_only_max: f64,
    pub last_update: u64,
}

impl LocalBook {
    pub fn new() -> Self {
        Self {
            last_update: 0,
            asks: BTreeMap::new(),
            bids: BTreeMap::new(),
            best_ask: Ask {
                price: 0.0,
                qty: 0.0,
            },
            mid_price: 0.0,
            lot_size: 0.0,
            min_order_size: 0.0,
            best_bid: Bid {
                price: 0.0,
                qty: 0.0,
            },
            tick_size: 0.0,
            post_only_max: 0.0,
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
        if timestamp == self.last_update {
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

        // Calculate the mid price
        self.set_mid_price();
        // Update the last update timestamp
        self.last_update = timestamp;
    }

    pub fn update_binance_bba(&mut self, bids: Vec<Bid>, asks: Vec<Ask>, timestamp: u64) {
        // If the timestamp is not newer than the last update, return early
        if timestamp <= self.last_update {
            return;
        }

        // Update the bids in the order book
        let prices_iter = bids.iter().map(|bid| OrderedFloat::from(bid.price));
        for bid in bids.iter() {
            let price = OrderedFloat::from(bid.price);

            // Modify or insert the bid price and quantity into the bids HashMap
            self.bids
                .entry(price)
                .and_modify(|qty| *qty = bid.qty)
                .or_insert(bid.qty);
            // Remove bids with prices higher than the current bid price
        }
        if let Some(highest_bid_price) = prices_iter.max() {
            self.bids.retain(|&key, _| key <= highest_bid_price);
        }

        let ask_prices_iter = asks.iter().map(|ask| OrderedFloat::from(ask.price));
        for ask in asks.iter() {
            let price = OrderedFloat::from(ask.price);
            // Modify or insert the ask price and quantity into the asks HashMap
            self.asks
                .entry(price)
                .and_modify(|qty| *qty = ask.qty)
                .or_insert(ask.qty);
            // Remove asks with prices lower than the current ask price
        }
        if let Some(lowest_ask_price) = ask_prices_iter.min() {
            self.asks.retain(|&key, _| key >= lowest_ask_price);
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

        // Set the mid price
        self.set_mid_price();
        // Update the last update timestamp
        self.last_update = timestamp;
    }

    fn set_mid_price(&mut self) {
        let avg = (self.best_ask.price + self.best_bid.price) / 2.0;
        self.mid_price = avg;
    }
    /// Get the tick size of the order book.
    ///
    /// # Returns
    ///
    /// The tick size as a `f64`.
    pub fn get_tick_size(&self) -> f64 {
        // Returns the tick size of the order book. Tick size is the minimum price
        // increment for the market.
        self.tick_size
    }

    pub fn get_lot_size(&self) -> f64 {
        self.lot_size
    }

    pub fn get_min_order_value(&self) -> f64 {
        self.min_order_size
    }

    pub fn get_post_only_max(&self) -> f64 {
        self.post_only_max
    }

    /// Get the best ask prices and quantities in the order book.
    pub fn get_best_ask(&self) -> Ask {
        self.best_ask.clone()
    }

    pub fn get_mid_price(&self) -> f64 {
        self.mid_price
    }

    /// Get the best bid prices and quantities in the order book.
    pub fn get_best_bid(&self) -> Bid {
        self.best_bid.clone()
    }

    /// Get the best ask and bid prices and quantities in the order book.
    pub fn get_bba(&self) -> (Ask, Bid) {
        (self.best_ask.clone(), self.best_bid.clone())
    }

    /// Get the spread between the best ask and best bid prices.
    ///
    /// # Returns
    ///
    /// The spread as a `f64`.
    pub fn get_spread(&self) -> f64 {
        // Calculate the spread between the best ask price and the best bid price.
        // The spread represents the difference between the best ask and best bid prices.
        // The spread is positive if the best ask price is higher than the best bid price,
        // and negative if the best ask price is lower than the best bid price.
        self.best_ask.price - self.best_bid.price
    }

    pub fn get_spread_in_bps(&self) -> f64 {
        spread_price_in_bps(self.get_spread(), self.mid_price)
    }

    /// Get the bids and asks in the order book at the specified depth.
    pub fn get_book_depth(&self, depth: usize) -> (Vec<Ask>, Vec<Bid>) {
        let asks: Vec<Ask> = {
            let mut ask_vec = Vec::new();
            for (p, q) in self.asks.iter().take(depth) {
                ask_vec.push(Ask {
                    price: **p,
                    qty: *q,
                })
            }
            ask_vec.reverse();
            ask_vec
        };

        let bids: Vec<Bid> = {
            let mut bid_vec = Vec::new();
            for (p, q) in self.bids.iter().rev().take(depth) {
                bid_vec.push(Bid {
                    price: **p,
                    qty: *q,
                })
            }
            bid_vec
        };
        (asks, bids)
    }
    pub fn get_wmid(&self) -> f64 {
        let imb = self.best_bid.qty / (self.best_bid.qty + self.best_ask.qty);
        self.best_bid.price * imb + self.best_ask.price * (1.0 - imb)
    }
}

unsafe impl Send for LocalBook {}

pub trait ProcessAsks {
    fn process_asks(ask: Self) -> Ask;
}

pub trait ProcessBids {
    fn process_bids(bid: Self) -> Bid;
}

impl ProcessAsks for binance::model::Asks {
    fn process_asks(ask: Self) -> Ask {
        Ask {
            price: ask.price,
            qty: ask.qty,
        }
    }
}

impl ProcessBids for binance::model::Bids {
    fn process_bids(bid: Self) -> Bid {
        Bid {
            price: bid.price,
            qty: bid.qty,
        }
    }
}
