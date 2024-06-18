use std::collections::VecDeque;

use bybit::model::WsTrade;
use skeleton::util::localorderbook::LocalBook;

use super::{
    imbalance::{imbalance_ratio, trade_imbalance, voi},
    impact::{
        avg_trade_price, expected_return, expected_value, mid_price_avg, mid_price_basis,
        mid_price_change, mid_price_diff, price_flu, price_impact,
    },
};

pub struct Engine {
    pub imbalance_ratio: f64,
    pub voi: f64,
    pub trade_imb: f64,
    pub price_impact: f64,
    pub expected_return: f64,
    pub price_flu: f64,
    pub expected_value: (VecDeque<f64>, f64),
    pub mid_price_change: f64,
    pub mid_price_diff: f64,
    pub mid_price_avg: f64,
    pub mid_price_basis: f64,
    pub avg_trade_price: f64,
    pub avg_spread: (VecDeque<f64>, f64),
}

impl Engine {
    pub fn new() -> Self {
        Self {
            imbalance_ratio: 0.0,
            voi: 0.0,
            trade_imb: 0.0,
            price_impact: 0.0,
            expected_return: 0.0,
            price_flu: 0.0,
            expected_value: (VecDeque::new(), 0.0),
            mid_price_change: 0.0,
            mid_price_diff: 0.0,
            mid_price_avg: 0.0,
            avg_trade_price: 0.0,
            mid_price_basis: 0.0,
            avg_spread: (VecDeque::new(), 0.0),
        }
    }

    pub fn update(
        &mut self,
        curr_book: &LocalBook,
        prev_book: &LocalBook,
        curr_trades: &VecDeque<WsTrade>,
        prev_trades: &VecDeque<WsTrade>,
        prev_avg: &f64,
        depth: Option<usize>,
    ) {
        self.imbalance_ratio = imbalance_ratio(curr_book, depth);
        self.voi = voi(curr_book, prev_book, depth);
        self.trade_imb = trade_imbalance(curr_trades);
        self.price_impact = price_impact(curr_book, prev_book, depth);
        self.price_flu = price_flu(prev_book.mid_price, curr_book.mid_price);
        self.expected_return = expected_return(prev_book.mid_price, curr_book.mid_price);
        self.expected_value.0.push_back(expected_value(
            prev_book.get_mid_price(),
            curr_book.get_mid_price(),
            imbalance_ratio(curr_book, depth),
        ));
        self.expected_value.1 = self.avg_exp_value();
        self.mid_price_change = mid_price_change(
            prev_book.get_mid_price(),
            curr_book.get_mid_price(),
            curr_book.tick_size,
        );
        self.mid_price_diff = mid_price_diff(prev_book.get_mid_price(), curr_book.get_mid_price());
        self.mid_price_avg = mid_price_avg(prev_book.get_mid_price(), curr_book.get_mid_price());
        self.avg_trade_price = avg_trade_price(
            curr_book.get_mid_price(),
            Some(prev_trades),
            curr_trades,
            *prev_avg,
            300,
        );
        self.mid_price_basis = mid_price_basis(
            prev_book.get_mid_price(),
            curr_book.get_mid_price(),
            self.avg_trade_price,
        );
        self.avg_spread.0.push_back(curr_book.get_spread());
        self.avg_spread.1 = self.avg_spread();
    }

    fn avg_spread(&mut self) -> f64 {
        if self.avg_spread.0.is_empty() {
            0.0
        } else {
            remove_elements_at_capacity(&mut self.avg_spread.0, 1500);
            self.avg_spread.0.iter().sum::<f64>() / self.avg_spread.0.len() as f64
        }
    }

    fn avg_exp_value(&mut self) -> f64 {
        if self.expected_value.0.is_empty() {
            0.0
        } else {
            remove_elements_at_capacity(&mut self.expected_value.0, 1500);
            self.expected_value.0.iter().sum::<f64>() / self.expected_value.0.len() as f64
        }
    }
}

fn remove_elements_at_capacity<T>(data: &mut VecDeque<T>, capacity: usize) {
    while data.len() > capacity {
        data.pop_front();
    }
}
