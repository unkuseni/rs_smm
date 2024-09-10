use std::collections::VecDeque;

use bybit::model::WsTrade;
use ndarray::{Array1, Array2};
use skeleton::util::localorderbook::LocalBook;

use super::{
    imbalance::{calculate_ofi, imbalance_ratio, trade_imbalance, voi},
    impact::{avg_trade_price, expected_return, mid_price_basis, price_flu, price_impact},
    linear_reg::mid_price_regression,
};

/// Weight for the imbalance ratio in the skew calculation.
const IMB_WEIGHT: f64 = 0.25; // 25

/// Weight for the deep imbalance ratio in the skew calculation.
const DEEP_IMB_WEIGHT: f64 = 0.10; // 35

/// Weight for the volume of interest (VOI) in the skew calculation.
const VOI_WEIGHT: f64 = 0.10; // 45

/// Weight for the order flow imbalance (OFI) in the skew calculation.
const OFI_WEIGHT: f64 = 0.20; // 65

/// Weight for the deep order flow imbalance in the skew calculation.
const DEEP_OFI_WEIGHT: f64 = 0.10; // 75

/// Weight for the predicted price in the skew calculation.
const PREDICT_WEIGHT: f64 = 0.25; // 100

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
    pub tick_window: usize,
}

impl Engine {
    /// Create a new instance of `Engine`.
    ///
    /// # Returns
    ///
    /// A new instance of `Engine`.
    pub fn new(tick_window: usize) -> Self {
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
            tick_window,
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
        self.deep_imbalance_ratio = depth[0..]
            .iter()
            .map(|v| imbalance_ratio(curr_book, Some(*v)))
            .collect();

        // Update volume of interest
        self.voi = voi(curr_book, prev_book, Some(depth[0]));

        // Update deep volume of interest
        self.deep_voi = depth[0..]
            .iter()
            .map(|v| voi(curr_book, prev_book, Some(*v)))
            .collect();

        // Update order flow imbalance
        self.ofi = calculate_ofi(curr_book, prev_book, Some(depth[0]));

        // Update deep order flow imbalance
        self.deep_ofi = depth[0..]
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
        self.expected_return = expected_return(
            prev_book.get_microprice(Some(depth[0])),
            curr_book.get_microprice(Some(depth[0])),
        );

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
        if self.mid_prices.len() > (self.tick_window + 11) {
            for _ in 0..10 {
                self.mid_prices.remove(0);
            }
        }

        // Push the current book's microprice at the specified depth to the mid_prices vector
        // This adds the latest microprice to the historical data used for price prediction
        self.mid_prices
            .push(curr_book.get_microprice(Some(depth[0])));

        // Update feature values
        if self.features.len() > (self.tick_window + 11) {
            for _ in 0..10 {
                self.features.remove(0);
            }
        }

        self.features
            .push([self.imbalance_ratio, self.voi, self.ofi]);

        if self.features.len() >= self.tick_window {
            self.predicted_price = {
                match self.predict_price(curr_book.get_spread_in_bps() as f64) {
                    Ok(v) => v,
                    Err(_) => curr_book.get_microprice(Some(depth[0])),
                }
            };
        }
        // Generate skew
        self.generate_skew(curr_book, depth[0]);
    }

    /// Predicts the future price based on historical data and current market conditions.
    ///
    /// This method uses linear regression to predict the future price. It takes into account
    /// the historical mid prices and features (imbalance ratio, volume of interest, and order flow imbalance)
    /// to make the prediction.
    ///
    /// # Arguments
    ///
    /// * `curr_spread` - The current spread in basis points.
    ///
    /// # Returns
    ///
    /// * `Result<f64, String>` - The predicted price if successful, or an error message if the prediction fails.
    ///
    /// # Errors
    ///
    /// This function will return an error if:
    /// * There's not enough historical data to make a prediction.
    /// * The linear regression model fails to fit or predict.
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

    /// Generates a skew value based on various market indicators.
    ///
    /// This function calculates a skew metric by combining multiple market indicators:
    /// - Imbalance ratio (normal and deep)
    /// - Volume of Interest (VOI)
    /// - Order Flow Imbalance (OFI, normal and deep)
    /// - Predicted price movement
    ///
    /// The skew value is used to adjust the market making strategy, influencing
    /// order placement and pricing decisions.
    ///
    /// # Arguments
    ///
    /// * `book` - A reference to the current order book state.
    /// * `depth` - The depth level to consider for certain calculations.
    ///
    /// # Effects
    ///
    /// Updates the `skew` field of the `Engine` struct with the calculated value.
    ///
    /// # Notes
    ///
    /// The skew calculation uses predefined weights for each component, which can
    /// be adjusted to fine-tune the strategy's behavior.
    /// Generates a skew value based on various market indicators.
    ///
    /// This function calculates a composite skew metric by combining multiple market indicators,
    /// each weighted according to its perceived importance. The resulting skew value can be used
    /// to adjust trading strategies, particularly in market making scenarios.
    ///
    /// # Arguments
    ///
    /// * `book` - A reference to the current `LocalBook`, representing the order book state.
    /// * `depth` - The depth level to consider for certain calculations, particularly for microprice.
    ///
    /// # Effects
    ///
    /// Updates the `skew` field of the `Engine` struct with the calculated value.
    ///
    /// # Algorithm
    ///
    /// 1. Calculate weighted imbalance ratios (normal and deep)
    /// 2. Calculate weighted volume of interest (VOI)
    /// 3. Calculate weighted order flow imbalances (OFI, normal and deep)
    /// 4. Determine a predicted value based on expected returns and price distances
    /// 5. Sum all components to produce the final skew value
    fn generate_skew(&mut self, book: &LocalBook, depth: usize) {
        // Calculate imbalance ratio and apply weight
        // The imbalance ratio is a value between -1 and 1, indicating buy/sell pressure
        let imb = self.imbalance_ratio * IMB_WEIGHT;

        // Calculate deep imbalance ratio and apply weight
        // This considers imbalance at multiple depth levels for a more comprehensive view
        let deep_imb = (self.deep_imbalance_ratio.iter().sum::<f64>()
            / self.deep_imbalance_ratio.len() as f64)
            * DEEP_IMB_WEIGHT;

        // Calculate volume of interest (VOI) and apply weight
        // VOI indicates the net volume added or removed from the order book
        let voi = self.voi * VOI_WEIGHT;

        // Calculate order flow imbalance (OFI) and apply weight
        // OFI measures the buying/selling pressure based on order flow
        let ofi = match self.ofi {
            v if v > 0.0 => 1.0 * OFI_WEIGHT,  // Positive OFI indicates buying pressure
            v if v < 0.0 => -1.0 * OFI_WEIGHT, // Negative OFI indicates selling pressure
            _ => 0.0,                          // Zero OFI indicates balance
        };

        // Calculate deep order flow imbalance and apply weight
        // This considers OFI at multiple depth levels for a more nuanced view
        let deep_ofi = {
            let value = self.deep_ofi.iter().sum::<f64>() / self.deep_ofi.len() as f64;
            match value {
                v if v > 0.0 => 1.0 * DEEP_OFI_WEIGHT,  // Positive deep OFI
                v if v < 0.0 => -1.0 * DEEP_OFI_WEIGHT, // Negative deep OFI
                _ => 0.0,                               // Balanced deep OFI
            }
        };

        // Calculate the distance from the microprice to the best ask and bid prices
        // These distances can indicate potential price movement directions
        let distance_to_ask = (book.get_microprice(Some(depth)) - book.get_best_ask().price).abs();
        let distance_to_bid = (book.get_microprice(Some(depth)) - book.get_best_bid().price).abs();

        // Determine the predicted value based on expected returns and price distances
        let predicted_value = match self.predicted_price {
            // If expected return is significantly positive or microprice is closer to ask
            v if expected_return(book.get_mid_price(), v) >= 0.0005
                || distance_to_ask < distance_to_bid =>
            {
                1.0 * PREDICT_WEIGHT // Predict upward movement
            }
            // If expected return is significantly negative or microprice is closer to bid
            v if expected_return(book.get_mid_price(), v) >= -0.0005
                || distance_to_bid < distance_to_ask =>
            {
                -1.0 * PREDICT_WEIGHT // Predict downward movement
            }
            _ => 0.0, // No clear prediction
        };

        // Calculate the final skew by summing all weighted components
        self.skew = imb + deep_imb + voi + ofi + deep_ofi + predicted_value;

        // Note: The resulting skew value will be between -1 and 1, where:
        // - Positive values indicate a bullish skew (tendency for price to increase)
        // - Negative values indicate a bearish skew (tendency for price to decrease)
        // - Values close to 0 indicate a neutral market
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
