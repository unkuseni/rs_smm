use bybit::model::{Ask, Bid};
use ordered_float::OrderedFloat;
use std::collections::BTreeMap;

use super::helpers::generate_timestamp;

#[derive(Debug, Clone)]
pub struct LocalBook {
    pub size: usize,
    pub asks: BTreeMap<OrderedFloat<f64>, f64>,
    pub bids: BTreeMap<OrderedFloat<f64>, f64>,
    pub last_update: u64,
}

impl LocalBook {
    pub fn new(size: usize) -> Self {
        Self {
            size,
            asks: BTreeMap::new(),
            bids: BTreeMap::new(),
            last_update: generate_timestamp(),
        }
    }

    pub fn update(&mut self, bids: Vec<Bid>, asks: Vec<Ask>, timestamp: u64) {
        if timestamp <= self.last_update {
            return;
        }

        self.bids.retain(|_, v| *v != 0.0);
        self.asks.retain(|_, v| *v != 0.0);

        for bid in bids {
            if bid.qty != 0.0 {
                self.bids
                    .entry(OrderedFloat::<f64>::from(bid.price))
                    .and_modify(|e| *e = bid.qty)
                    .or_insert(bid.qty);
            }
        }

        for ask in asks {
            if ask.qty != 0.0 {
                self.asks
                    .entry(OrderedFloat::<f64>::from(ask.price))
                    .and_modify(|e| *e = ask.qty)
                    .or_insert(ask.qty);
            }
        }

        self.last_update = timestamp;
    }
    
    pub fn update_bba(&mut self, bids: Vec<Bid>, asks: Vec<Ask>, timestamp: u64) {
        if timestamp <= self.last_update {
            return;
        }

        self.bids.retain(|_, v| *v != 0.0);
        self.asks.retain(|_, v| *v != 0.0);

        for bid in bids {
            if bid.qty != 0.0 {
                self.bids.retain(|&k, _| k <= OrderedFloat::from(bid.price));
                self.bids
                    .entry(OrderedFloat::<f64>::from(bid.price))
                    .and_modify(|e| *e = bid.qty)
                    .or_insert(bid.qty);
            }
        }

        for ask in asks {
            if ask.qty != 0.0 {
                self.asks.retain(|&k, _| k >= OrderedFloat::from(ask.price));
                self.asks
                    .entry(OrderedFloat::<f64>::from(ask.price))
                    .and_modify(|e| *e = ask.qty)
                    .or_insert(ask.qty);
            }
        }

        self.last_update = timestamp;
    }
}
