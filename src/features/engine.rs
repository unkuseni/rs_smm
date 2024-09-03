use std::collections::VecDeque;

use bybit::model::WsTrade;
use ndarray::{Array1, Array2};
use skeleton::util::localorderbook::LocalBook;

use super::{
    imbalance::{calculate_ofi, imbalance_ratio, trade_imbalance, voi},
    impact::{avg_trade_price, expected_return, mid_price_basis, price_flu, price_impact},
    linear_reg::mid_price_regression,
};

const IMB_WEIGHT: f64 = 0.25; // 25
const DEEP_IMB_WEIGHT: f64 = 0.15; // 40
const VOI_WEIGHT: f64 = 0.10; // 50
const OFI_WEIGHT: f64 = 0.20; // 70
const DEEP_OFI_WEIGHT: f64 = 0.15; // 85
const PREDICT_WEIGHT: f64 = 0.15; // 100

#[derive(Clone, Debug)]
pub struct Engine {
    pub imbalance_ratio: f64,
    pub deep_imbalance_ratio: Vec<f64>,
    pub voi: f64,
    pub deep_voi: Vec<f64>,
    pub ofi: f64,
    pub deep_ofi: Vec<f64>,
    pub trade_imb: f64,
    pub price_impact: f64,
    pub expected_return: f64,
    pub price_flu: (VecDeque<f64>, f64), // in bps
    pub mid_price_basis: f64,
    pub avg_trade_price: f64,
    pub predicted_price: f64,
    pub skew: f64,
    mid_prices: Vec<f64>,
    features: Vec<[f64; 3]>,
    tick_window: usize,
}

impl Engine {
    /// Create a new instance of `Engine`.
    ///
    /// # Returns
    ///
    /// A new instance of `Engine`.
    pub fn new() -> Self {
        Self {
            // The imbalance ratio.
            imbalance_ratio: 0.0,
            // The deep imbalance ratios.
            deep_imbalance_ratio: Vec::new(),
            // The volume of interest.
            voi: 0.0,
            // The deep volume of interest.
            deep_voi: Vec::new(),
            // The order flow imbalance.
            ofi: 0.0,
            // The deep order flow imbalance.
            deep_ofi: Vec::new(),
            // The trade imbalance.
            trade_imb: 0.0,
            // The price impact.
            price_impact: 0.0,
            // The expected return.
            expected_return: 0.0,
            // The price flu in bps.
            price_flu: (VecDeque::new(), 0.0),
            // The mid price basis.
            mid_price_basis: 0.0,
            // The average trade price.
            avg_trade_price: 0.0,
            // The predicted price.
            predicted_price: 0.0,
            // The skew.
            skew: 0.0,
            // The mid prices.
            mid_prices: Vec::new(),
            // The features.
            features: Vec::new(),
            // The tick window.
            tick_window: 0,
        }
    }

    /// Update the features of the `Engine` with the latest data.
    ///
    /// # Arguments
    ///
    /// * `curr_book`: The current order book.
    /// * `prev_book`: The previous order book.
    /// * `curr_trades`: The current trades.
    /// * `prev_trades`: The previous trades.
    /// * `prev_avg`: The average trade price of the previous tick window.
    /// * `depth`: The list of depths to calculate the features at.
    /// * `use_wmid`: Whether to use the W-MID for calculating the imbalance ratio.
    pub fn update(
        &mut self,
        curr_book: &LocalBook,
        prev_book: &LocalBook,
        curr_trades: &VecDeque<WsTrade>,
        prev_trades: &VecDeque<WsTrade>,
        prev_avg: &f64,
        depth: Vec<usize>,
    ) {
        // Update imbalance ratio
        self.imbalance_ratio = imbalance_ratio(curr_book, Some(depth[0]));

        // Update deep imbalance ratio
        self.deep_imbalance_ratio = depth[1..]
            .iter()
            .map(|v| imbalance_ratio(curr_book, Some(*v)))
            .collect();

        // Update volume of interest
        self.voi = voi(curr_book, prev_book, Some(depth[0]));

        // Update deep volume of interest
        self.deep_voi = depth[1..]
            .iter()
            .map(|v| voi(curr_book, prev_book, Some(*v)))
            .collect();

        // Update order flow imbalance
        self.ofi = calculate_ofi(curr_book, prev_book, Some(depth[0]));

        // Update deep order flow imbalance
        self.deep_ofi = depth[1..]
            .iter()
            .map(|v| calculate_ofi(curr_book, prev_book, Some(*v)))
            .collect();

        // Update trade imbalance
        self.trade_imb = trade_imbalance(curr_trades);

        // Update price impact
        self.price_impact = price_impact(curr_book, prev_book, Some(depth[0]));

        // Update price flu
        self.price_flu
            .0
            .push_back(price_flu(prev_book.mid_price, curr_book.mid_price));

        self.price_flu.1 = self.avg_flu_value();

        // Update expected return
        self.expected_return = expected_return(prev_book.mid_price, curr_book.mid_price);

        // Update average trade price
        self.avg_trade_price = avg_trade_price(
            curr_book.get_mid_price(),
            Some(prev_trades),
            curr_trades,
            *prev_avg,
            self.tick_window,
        );

        // Update mid price basis
        self.mid_price_basis = mid_price_basis(
            prev_book.get_mid_price(),
            curr_book.get_mid_price(),
            self.avg_trade_price,
        );

        // Update mid price array for regression
        if self.mid_prices.len() > self.tick_window {
            for _ in 0..10 {
                self.mid_prices.pop();
            }
        }

        self.mid_prices.push(curr_book.get_mid_price());

        // Update feature values
        if self.features.len() > self.tick_window {
            for _ in 0..10 {
                self.features.pop();
            }
        }

        self.features
            .push([self.imbalance_ratio, self.voi, self.ofi]);

        self.predicted_price = {
            match self.predict_price(curr_book.get_spread_in_bps() as f64) {
                Ok(v) => v,
                Err(_) => curr_book.mid_price,
            }
        };

        // Generate skew
        self.generate_skew(curr_book);
    }

    fn predict_price(&mut self, curr_spread: f64) -> Result<f64, String> {
        let mids = self.mid_prices.clone();
        let y = Array1::from_vec(mids);
        let x = match Array2::from_shape_vec(
            (self.features.len(), 3),
            self.features
                .clone()
                .into_iter()
                .flat_map(|v| v.into_iter())
                .collect::<Vec<f64>>(),
        ) {
            Ok(x) => mid_price_regression(y, x, curr_spread),
            Err(e) => return Err(e.to_string()),
        };
        x
    }

    /// Calculates the average price fluctuation over the last [tick_window] periods.
    ///
    /// # Returns
    ///
    /// The average price fluctuation as a logarithmic value.
    fn avg_flu_value(&mut self) -> f64 {
        // If there is no price fluctuation data, return 0.0
        if self.price_flu.0.is_empty() {
            0.0
        } else {
            // Remove any price fluctuation data that is older than the tick window
            remove_elements_at_capacity(&mut self.price_flu.0, self.tick_window);

            // Calculate the average price fluctuation
            self.price_flu.0.iter().sum::<f64>() / self.price_flu.0.len() as f64
        }
    }

    /// Generates a  number between -1 and 1.
    fn generate_skew(&mut self, book: &LocalBook) {
        // generate a skew metric and update the regression model for predictions
        let imb = self.imbalance_ratio * IMB_WEIGHT; // Ratio is -1 to 1
        let deep_imb = (self.deep_imbalance_ratio.iter().sum::<f64>()
            / self.deep_imbalance_ratio.len() as f64)
            * DEEP_IMB_WEIGHT; // Ratio is -1 to 1

        let voi = self.voi * VOI_WEIGHT;
        let ofi = match self.ofi {
            v if v > 0.0 => 1.0 * OFI_WEIGHT,
            v if v < 0.0 => -1.0 * DEEP_OFI_WEIGHT,
            _ => 0.0,
        };
        let deep_ofi = {
            let value = self.deep_ofi.iter().sum::<f64>() / self.deep_ofi.len() as f64;
            match value {
                v if v > 0.0 => 1.0 * DEEP_OFI_WEIGHT,
                v if v < 0.0 => -1.0 * DEEP_OFI_WEIGHT,
                _ => 0.0,
            }
        };

        let predicted_value = match self.predicted_price {
            v if v > book.get_wmid(self.imbalance_ratio) => 1.0 * PREDICT_WEIGHT,
            v if v < book.get_wmid(self.imbalance_ratio) => -1.0 * PREDICT_WEIGHT,
            _ => 0.0,
        };

        self.skew = imb + deep_imb + voi + ofi + deep_ofi + predicted_value;
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
    while data.len() >= capacity {
        // Remove the first element of the VecDeque.
        data.pop_front();
    }
}
