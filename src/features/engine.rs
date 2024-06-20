use std::collections::VecDeque;

use bybit::model::WsTrade;
use skeleton::util::localorderbook::LocalBook;

use super::{
    imbalance::{imbalance_ratio, trade_imbalance, voi, wmid},
    impact::{
        avg_trade_price, expected_return, expected_value, mid_price_basis, mid_price_change,
        price_flu, price_impact,
    },
};

const IMB_WEIGHT: f64 = 0.25;
const TRADE_IMB_WEIGHT: f64 = 0.25;
const EXP_RET_WEIGHT: f64 = 0.10;
const DEEP_IMB_WEIGHT: f64 = 0.20;
const MID_BASIS_WEIGHT: f64 = 0.10;
const VOI_WEIGHT: f64 = 0.10;

#[derive(Clone, Debug)]
pub struct Engine {
    pub imbalance_ratio: f64,
    pub deep_imbalance_ratio: f64,
    pub wmid: f64,
    pub voi: f64,
    pub trade_imb: f64,
    pub price_impact: f64,
    pub expected_return: f64,
    pub price_flu: f64, // in bps
    pub expected_value: (VecDeque<f64>, f64),
    pub mid_price_change: f64,
    pub mid_price_basis: f64,
    pub avg_trade_price: f64,
    pub avg_spread: (VecDeque<f64>, f64),
    pub skew: f64,
}

impl Engine {
    pub fn new() -> Self {
        Self {
            imbalance_ratio: 0.0,
            deep_imbalance_ratio: 0.0,
            wmid: 0.0,
            voi: 0.0,
            trade_imb: 0.0,
            price_impact: 0.0,
            expected_return: 0.0,
            price_flu: 0.0,
            expected_value: (VecDeque::new(), 0.0),
            mid_price_change: 0.0,
            avg_trade_price: 0.0,
            mid_price_basis: 0.0,
            avg_spread: (VecDeque::new(), 0.0),
            skew: 0.0,
        }
    }

    /// Updates the engine's features with the current order book and trades data.
    ///
    /// # Arguments
    ///
    /// * `curr_book` - The current order book.
    /// * `prev_book` - The previous order book.
    /// * `curr_trades` - The current trades data.
    /// * `prev_trades` - The previous trades data.
    /// * `prev_avg` - The average trade price of the previous order book.
    /// * `depth` - The depths at which to calculate imbalance and spread.
    /// * `tick_window` - The number of ticks to consider when calculating `avg_trade_price`.
    pub fn update(
        &mut self,
        curr_book: &LocalBook,
        prev_book: &LocalBook,
        curr_trades: &VecDeque<WsTrade>,
        prev_trades: &VecDeque<WsTrade>,
        prev_avg: &f64,
        depth: Vec<usize>,
        tick_window: usize,
    ) {
        // Update imbalance ratio
        self.imbalance_ratio = imbalance_ratio(curr_book, Some(depth[0]));
        // Update deep imbalance ratio
        self.deep_imbalance_ratio = imbalance_ratio(curr_book, Some(depth[1]));
        // Update volume of interest
        self.voi = voi(curr_book, prev_book, Some(depth[0]));
        // Update trade imbalance
        self.trade_imb = trade_imbalance(curr_trades);
        // Update price impact
        self.price_impact = price_impact(curr_book, prev_book, Some(depth[0]));
        // Update price flu
        self.price_flu = price_flu(prev_book.mid_price, curr_book.mid_price);
        // Update expected return
        self.expected_return = expected_return(prev_book.mid_price, curr_book.mid_price);
        // Update expected value
        self.expected_value.0.push_back(expected_value(
            prev_book.get_mid_price(),
            curr_book.get_mid_price(),
            imbalance_ratio(curr_book, Some(depth[0])),
        ));
        // Update weighted mid price
        self.wmid = (wmid(curr_book, self.imbalance_ratio) / curr_book.mid_price).ln();
        // Update average expected value
        self.expected_value.1 = self.avg_exp_value();
        // Update mid price change
        self.mid_price_change = mid_price_change(
            prev_book.get_mid_price(),
            curr_book.get_mid_price(),
            curr_book.tick_size,
        );
        // Update average trade price
        self.avg_trade_price = avg_trade_price(
            curr_book.get_mid_price(),
            Some(prev_trades),
            curr_trades,
            *prev_avg,
            tick_window,
        );
        // Update mid price basis
        self.mid_price_basis = mid_price_basis(
            prev_book.get_mid_price(),
            curr_book.get_mid_price(),
            self.avg_trade_price,
        );
        // Update average spread
        self.avg_spread.0.push_back(curr_book.get_spread());
        self.avg_spread.1 = self.avg_spread();
        // Generate skew
        self.generate_skew()
    }

    /// Calculates the average spread over the last 1500 values.
    ///
    /// If the VecDeque is empty, returns 0.0.
    /// Otherwise, removes elements from the VecDeque until its length is less than or equal to 1500,
    /// then calculates the average of the remaining elements, but only considering values greater than 0.0.
    ///
    /// # Returns
    /// The average spread over the last 1500 values, or 0.0 if the VecDeque is empty.
    fn avg_spread(&mut self) -> f64 {
        // Check if the VecDeque is empty
        if self.avg_spread.0.is_empty() {
            // Return 0.0 if the VecDeque is empty
            0.0
        } else {
            // Remove elements from the VecDeque until its length is less than or equal to 1500
            remove_elements_at_capacity(&mut self.avg_spread.0, 1500);

            // Calculate the average of the remaining elements, but only considering values greater than 0.0
            self.avg_spread.0.iter().map(|x| x.max(0.0)).sum::<f64>()
                / self.avg_spread.0.len() as f64
        }
    }

    /// Calculates the average value of the expected values.
    ///
    /// Removes elements from the `expected_value.0` VecDeque until its length is
    /// less than or equal to 1500 and then calculates the average value of the
    /// remaining elements.
    ///
    /// # Returns
    /// The average value of the expected values.
    fn avg_exp_value(&mut self) -> f64 {
        // Check if the VecDeque is empty
        if self.expected_value.0.is_empty() {
            // Return 0.0 if the VecDeque is empty
            0.0
        } else {
            // Remove elements from the VecDeque until its length is less than or equal to 1500
            remove_elements_at_capacity(&mut self.expected_value.0, 1500);

            // Calculate the average value of the remaining elements
            self.expected_value.0.iter().sum::<f64>()
                / self.expected_value.0.len() as f64
        }
    }
    /// Generates a  number between -1 and 1.
    fn generate_skew(&mut self) {
        let imb = self.imbalance_ratio * IMB_WEIGHT; // -1 to 1
        let trade_imb = self.trade_imb * TRADE_IMB_WEIGHT; // 0 to 1
        let deep_imb = self.deep_imbalance_ratio * DEEP_IMB_WEIGHT; // -1 to 1
        let exp_ret = {
            if self.expected_return > 0.0 {
                1.0 * EXP_RET_WEIGHT
            } else if self.expected_return < 0.0 {
                -1.0 * EXP_RET_WEIGHT
            } else {
                0.0
            }
        };
        let voi = {
            if self.voi > 0.0 {
                1.0 * VOI_WEIGHT
            } else if self.voi < 0.0 {
                -1.0 * VOI_WEIGHT
            } else {
                0.0
            }
        };
        let _wmid = self.wmid * EXP_RET_WEIGHT;
        let mid_b = {
            if self.mid_price_basis > 0.0 {
                1.0 * MID_BASIS_WEIGHT
            } else {
                -1.0 * MID_BASIS_WEIGHT
            }
        };
        self.skew = imb + trade_imb + deep_imb + voi + mid_b + exp_ret;
    }
}

/// Removes elements from the front of `data` until the length is less than or equal to `capacity`.
///
/// # Arguments
///
/// * `data` - The `VecDeque` to remove elements from.
/// * `capacity` - The maximum number of elements to allow in `data`.
pub fn remove_elements_at_capacity<T>(data: &mut VecDeque<T>, capacity: usize) {
    // Keep removing elements from the front of the VecDeque until the length is less than or equal to the capacity.
    while data.len() > capacity {
        // Remove the first element of the VecDeque.
        data.pop_front();
    }
}
