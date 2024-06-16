use std::collections::VecDeque;

use bybit::model::WsTrade;
use ndarray::{Array1, Array2, ShapeError};
use skeleton::util::localorderbook::LocalBook;

use super::{
    imbalance::{imbalance_ratio, trade_imbalance, voi},
    impact::{
        avg_trade_price, expected_value, mid_price_avg, mid_price_basis, mid_price_change,
        mid_price_diff, price_impact,
    },
    linear_reg::mid_price_regression,
};

pub struct Engine {
    pub imbalance_ratio: f64,
    pub voi: f64,
    pub trade_imb: f64,
    pub price_impact: f64,
    pub expected_value: (VecDeque<f64>, f64),
    pub mid_price_change: f64,
    pub mid_price_diff: f64,
    pub mid_price_avg: f64,
    pub mid_price_basis: f64,
    pub avg_trade_price: f64,
    pub target_dataset: Vec<f64>,
    pub record_dataset: Vec<Vec<f64>>,
    pub regression_pred: f64,
    pub avg_spread: (VecDeque<f64>, f64),
}

impl Engine {
    pub fn new() -> Self {
        Self {
            imbalance_ratio: 0.0,
            voi: 0.0,
            trade_imb: 0.0,
            price_impact: 0.0,
            expected_value: (VecDeque::new(), 0.0),
            mid_price_change: 0.0,
            mid_price_diff: 0.0,
            mid_price_avg: 0.0,
            avg_trade_price: 0.0,
            mid_price_basis: 0.0,
            target_dataset: Vec::new(),
            record_dataset: Vec::new(),
            regression_pred: 0.0,
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
        self.target_dataset.push(curr_book.get_mid_price());
        self.imbalance_ratio = imbalance_ratio(curr_book, depth);
        self.voi = voi(curr_book, prev_book, depth);
        self.trade_imb = trade_imbalance(curr_trades);
        self.price_impact = price_impact(curr_book, prev_book, depth);
        self.expected_value.0.push_back(expected_value(
            prev_book.get_mid_price(),
            curr_book.get_mid_price(),
            imbalance_ratio(curr_book, depth),
        ));
        self.expected_value.1 = self.avg_exp_value();
        self.mid_price_change = mid_price_change(
            prev_book.get_mid_price(),
            curr_book.get_mid_price(),
            self.avg_spread(),
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
        
        // if self.target_dataset.len() >= 1500 {
        //     remove_elements_at_capacity(&mut self.target_dataset, 1500);
        // } else {
        //     self.target_dataset.push(curr_book.get_mid_price());
        // }
        // if self.record_dataset.len() >= 1500 {
        //     remove_elements_at_capacity(&mut self.record_dataset, 1500);
        // } else {
        //     self.record_dataset
        //         .push(vec![self.voi, self.imbalance_ratio, self.mid_price_basis]);
        // }

        // self.update_regression_data(curr_book.get_spread());
    }

    fn avg_spread(&mut self) -> f64 {
        if self.avg_spread.0.is_empty() {
            0.0
        } else {
            while self.avg_spread.0.len() >= 1500 {
                self.avg_spread.0.pop_front();
            }
            self.avg_spread.0.iter().sum::<f64>() / self.avg_spread.0.len() as f64
        }
    }

    fn avg_exp_value(&mut self) -> f64 {
        if self.expected_value.0.is_empty() {
            0.0
        } else {
            while self.expected_value.0.len() >= 1500 {
                self.expected_value.0.pop_front();
            }
            self.expected_value.0.iter().sum::<f64>() / self.expected_value.0.len() as f64
        }
    }

    fn update_regression_data(&mut self, avg: f64) {
        let mut record_arr = vecs_to_array2(self.record_dataset.clone()).unwrap();
        let target_arr = Array1::from(self.target_dataset.clone());
        self.regression_pred = mid_price_regression(target_arr, record_arr, avg);
    }
}

fn remove_elements_at_capacity<T>(data: &mut Vec<T>, capacity: usize) {
    while data.len() > capacity {
        data.remove(0);
    }
}

fn vecs_to_array2(data: Vec<Vec<f64>>) -> Result<Array2<f64>, ShapeError> {
    let rows = data.len();
    let cols = data.get(0).map_or(0, Vec::len); // Assumes all rows have the same number of columns
    let flat_data: Vec<f64> = data.into_iter().flatten().collect();
    Array2::from_shape_vec((rows, cols), flat_data)
}
