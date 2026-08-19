use std::{
    fs,
    io::Read,
    path::Path,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use num_traits::{Float, Signed};

use serde::Deserialize;
use tokio::sync::mpsc;

/// Rounds a number to the nearest step.
///
/// # Arguments
///
/// * `num`: The number to round.
/// * `step`: The step size to round to.
///
/// # Returns
///
/// The rounded number.
pub fn round_step<T: Float>(num: T, step: T) -> T {
    (num * (T::one() / step)).round() * step
}

/// Generates a vector of geometric weights given a ratio and a length.
///
/// # Arguments
///
/// * `ratio`: The ratio to use for generating the weights.
/// * `n`: The length of the vector to generate.
/// * `reverse`: Whether to reverse the weights or not.
///
/// # Returns
///
/// A vector of geometric weights.
pub fn geometric_weights(ratio: f64, n: usize, reverse: bool) -> Vec<f64> {
    assert!(
        (0.0..=1.0).contains(&ratio),
        "Ratio must be between 0 and 1"
    );
    let mut weights = Vec::with_capacity(n);
    let mut sum = 0.0;
    let mut val = 1.0;
    for _ in 0..n {
        weights.push(val);
        sum += val;
        val *= ratio;
    }

    // Normalize the weights by dividing by the sum
    weights.iter_mut().for_each(|w| *w /= sum);

    // Reverse the weights if requested
    if reverse {
        weights.reverse();
    }

    weights
}

/// Generates the current timestamp in milliseconds.
///
/// # Returns
///
/// The current timestamp in milliseconds.
#[inline(always)]
pub fn generate_timestamp() -> u64 {
    // Get the current time and convert it to milliseconds.
    // The time is compared with the reference point of the Unix epoch.
    SystemTime::now()
        // Get the duration since the Unix epoch.
        .duration_since(UNIX_EPOCH)
        // This should never fail, but just in case it does, return an error.
        .expect("Time went backwards")
        // Convert the duration to milliseconds.
        .as_millis() as u64
}

/// Calculates the exponent of a given number.
///
/// # Parameters
///
/// * `n`: The number to calculate the exponent of.
///
/// # Returns
///
/// The exponent of the given number.
pub fn calculate_exponent(n: f64) -> f64 {
    // The exponent of a number is calculated as the result of raising e to the power of the number.
    // In this case, we use the exp method of the f64 type to calculate the exponent.
    // The result is the exponent of the given number.
    (-0.5 * n).exp()
}

/// Generates a linearly spaced vector of f64 numbers.
///
/// # Parameters
///
/// * `start`: The starting value of the sequence.
/// * `end`: The ending value of the sequence.
/// * `n`: The number of elements in the sequence.
///
/// # Returns
///
/// A vector containing n numbers, equally spaced between start and end.
pub fn linspace<T: Float + Signed + PartialOrd>(start: T, end: T, n: usize) -> Vec<T> {
    // Calculate the step size between consecutive numbers in the sequence.
    // The step size is calculated as (end - start) divided by the number of elements minus 1.
    let step = (end - start) / T::from(n as u32 - 1).unwrap();

    // Generate the sequence of numbers using the start value, the step size and the number of elements.
    // The iterator is created with the start value, step size and the number of elements.
    // The map function applies the formula (start + i as T * step) to each element in the iterator,
    // creating a new vector with the resulting numbers.
    (0..n as u32)
        .map(|i| start + T::from(i).unwrap() * step)
        .collect()
}

/// Generates a geometrically spaced vector of f64 numbers.
///
/// # Arguments
///
/// * `start` - The starting value of the sequence.
/// * `end` - The ending value of the sequence.
/// * `n` - The number of elements in the sequence.
///
/// # Returns
///
/// A vector containing n numbers, equally spaced between start and end.
///
/// # Panics
///
/// If start or end is zero.
pub fn geomspace<T: Float + PartialOrd + Signed>(start: T, end: T, n: usize) -> Vec<T> {
    assert!(
        start != T::zero() && end != T::zero(),
        "Start and end must be non-zero"
    );
    if n <= 1 {
        return vec![start];
    }

    let log_start = start.ln();
    let log_end = end.ln();
    let step = (log_end - log_start) / T::from(n - 1).unwrap();

    (0..n)
        .map(|i| {
            let t = T::from(i).unwrap() * step + log_start;
            t.exp()
        })
        .collect()
}

/// Returns the square root of a number, with the sign of the original number.
///
/// # Arguments
///
/// * `num`: The number to take the square root of.
///
/// # Returns
///
/// The signed square root of the number.
pub fn nbsqrt<T: PartialOrd + Float + Signed>(num: T) -> T {
    // First, calculate the absolute value of the number.
    let abs_num = num.abs();

    // Then, calculate the square root of the absolute value.
    let sqrt_num = abs_num.sqrt();

    // Finally, return the signed square root, by multiplying the square root by the sign of the original number.
    num.signum() * sqrt_num
}

/// Calculate the spread in basis points, given a spread and a price.
///
/// # Parameters
///
/// * `spread`: The spread as a decimal.
/// * `price`: The price as a decimal.
///
/// # Returns
///
/// The spread in basis points, rounded to the nearest integer.
pub fn spread_price_in_bps(spread: f64, price: f64) -> i32 {
    // Calculate the spread as a percentage of the price.
    let percent = spread / price;

    // Convert the percentage to basis points by multiplying by 10,000.
    (percent * 10000.0) as i32
}

pub trait Round<T> {
    /// Rounds the number to the given digit.
    ///
    /// # Parameters
    ///
    /// * `digit`: The number of decimal places to round to.
    ///
    /// # Returns
    ///
    /// The rounded number.
    fn round_to(&self, digit: u8) -> T;

    /// Clip the number to the given range.
    ///
    /// # Parameters
    ///
    /// * `min`: The minimum value.
    /// * `max`: The maximum value.
    ///
    /// # Returns
    ///
    /// The clipped number.
    fn clip(&self, min: T, max: T) -> T;

    /// Counts the number of decimal places in a number.
    ///
    /// # Returns
    ///
    /// The number of decimal places in the number.
    fn count_decimal_places(&self) -> usize;
}
impl Round<f64> for f64 {
    fn round_to(&self, digit: u8) -> f64 {
        let pow = 10_i64.pow(digit as u32);
        (self * pow as f64).trunc() / pow as f64
    }

    fn clip(&self, min: f64, max: f64) -> f64 {
        (*self).clamp(min, max)
    }

    fn count_decimal_places(&self) -> usize {
        self.to_string().split('.').nth(1).map_or(0, |s| s.len())
    }
}

/// This section is for a toml parser that will be used for reading config files
///
pub fn read_toml(path: impl AsRef<Path>) -> Config {
    let mut file = std::fs::File::open(path).expect("Unable to open file");
    let mut contents = String::new();
    file.read_to_string(&mut contents)
        .expect("Unable to read file");
    toml::from_str(&contents).expect("Unable to parse file")
}

pub async fn watch_config<T>(
    path: T,
    interval: Duration,
    sender: mpsc::UnboundedSender<Config>,
) -> Result<(), std::io::Error>
where
    T: AsRef<Path>,
{
    let mut last_modified = fs::metadata(path.as_ref())?.modified()?;
    loop {
        let metadata = fs::metadata(path.as_ref())?;
        let current_modified = metadata.modified()?;
        if current_modified > last_modified {
            last_modified = current_modified;
            let _ = sender.send(read_toml(path.as_ref()));
        }
        tokio::time::sleep(interval).await;
    }
}

/// Strategy constants that used to be hardcoded in the engine and quote
/// generator. Every field has a default (matching the historical constants),
/// so a config file that omits the [strategy] table keeps the old behavior.
#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct StrategyConfig {
    // Skew weights (should roughly sum to 1.0).
    pub imb_weight: f64,
    pub deep_imb_weight: f64,
    pub voi_weight: f64,
    pub trade_weight: f64,
    pub deep_ofi_weight: f64,
    pub predict_weight: f64,
    // Deadbands: signals inside the band contribute nothing.
    pub imb_deadband: f64,
    pub trade_deadband: f64,
    // Gates for the predicted-price and basis components of the skew.
    pub predict_gate: f64,
    pub basis_gate: f64,
    // Inventory adjustment strength (Avellaneda-Stoikov style).
    pub inventory_adjustment: f64,
    // Aggression clip for the skew-driven quote tilt.
    pub aggression_min: f64,
    pub aggression_max: f64,
    // Volatility-adaptive spread: the quote spread is widened by
    // (1 + vol_spread_scaling * vol) where vol is the standard deviation
    // of mid returns over the feature window.
    pub vol_spread_scaling: f64,
    /// Anchor the quote grid at the microprice instead of the mid price.
    pub use_microprice_anchor: bool,
    // Inventory rebalancing: when |inventory_delta| reaches the threshold,
    // a reducing market order is submitted (at most once per cooldown).
    pub rebalance_threshold: f64,
    pub rebalance_cooldown_ms: u64,
    // Grid lifecycle: staleness before a forced refresh and the cadence of
    // exchange position re-syncs.
    pub grid_stale_ms: u64,
    pub position_sync_ms: u64,
    // Avellaneda-Stoikov reservation pricing. When as_gamma > 0 the grid is
    // centered at the reservation price r = s - q*gamma*sigma^2*(T-t) and the
    // quote spread is floored at gamma*sigma^2*(T-t) + (2/gamma)*ln(1+gamma/kappa).
    // gamma is the risk-aversion coefficient (calibrate it: for perps it is
    // typically large, on the order of 1e4..1e6); kappa approximates the
    // order arrival intensity. 0 keeps the heuristic inventory adjustment.
    pub as_gamma: f64,
    pub as_horizon_secs: f64,
    pub as_kappa: f64,
    // Cont, Kukanov & Stoikov (2011) linear impact fallback predictor:
    // predicted mid = mid * (1 + ofi_impact_k * ofi_scaled), used when the
    // regression cannot produce a prediction. 0 disables.
    pub ofi_impact_k: f64,
    // Cartea et al. (2018) regime-smoothed imbalance: the EMA-smoothed
    // imbalance is quantized into 5 regimes (-2..=2) and regime_weight *
    // regime is added to the skew. 0 disables.
    pub regime_weight: f64,
    // Funding cost of held inventory in basis points per hour, charged in
    // the backtest (long pays when positive). 0 disables.
    pub funding_bps_per_hour: f64,
    // Portfolio risk limits (0 disables): halt quoting and cancel all orders
    // when the mark-to-market drawdown exceeds max_drawdown_frac of initial
    // equity, or when the sum of |inventory_delta| across symbols exceeds
    // max_portfolio_delta.
    pub max_drawdown_frac: f64,
    pub max_portfolio_delta: f64,
}

impl Default for StrategyConfig {
    fn default() -> Self {
        Self {
            imb_weight: 0.25,
            deep_imb_weight: 0.10,
            voi_weight: 0.10,
            trade_weight: 0.30,
            deep_ofi_weight: 0.10,
            predict_weight: 0.15,
            imb_deadband: 0.20,
            trade_deadband: 0.20,
            predict_gate: 0.0005,
            basis_gate: 0.0005,
            inventory_adjustment: 0.63,
            aggression_min: 0.10,
            aggression_max: 0.63,
            vol_spread_scaling: 200.0,
            use_microprice_anchor: true,
            rebalance_threshold: 0.45,
            rebalance_cooldown_ms: 30_000,
            grid_stale_ms: 180_000,
            position_sync_ms: 60_000,
            as_gamma: 0.0,
            as_horizon_secs: 10.0,
            as_kappa: 1.0,
            ofi_impact_k: 0.0,
            regime_weight: 0.0,
            funding_bps_per_hour: 0.0,
            max_drawdown_frac: 0.0,
            max_portfolio_delta: 0.0,
        }
    }
}

#[derive(Deserialize, Debug, Clone)]
pub struct Config {
    pub exchange: String,
    pub symbols: Vec<String>,
    pub api_keys: Vec<(String, String, String)>,
    pub balances: Vec<(String, f64)>,
    pub leverage: f64,
    pub orders_per_side: usize,
    pub final_order_distance: f64,
    pub depths: Vec<usize>,
    pub rate_limit: u32,
    /// Profit spread in basis points, keyed by symbol.
    pub bps: std::collections::HashMap<String, f64>,
    pub tick_window: usize,
    /// Optional path to record websocket deltas to for offline replay.
    #[serde(default)]
    pub record: Option<String>,
    /// Optional path where position/live-order state is saved on shutdown and
    /// restored on startup, so a restart does not start the bot blind.
    #[serde(default)]
    pub state_file: Option<String>,
    /// Strategy constants; omitted keys fall back to StrategyConfig::default().
    #[serde(default)]
    pub strategy: StrategyConfig,
}

impl PartialEq for Config {
    fn eq(&self, other: &Self) -> bool {
        self.exchange == other.exchange
            && self.symbols == other.symbols
            && self.api_keys == other.api_keys
            && self.balances == other.balances
            && self.leverage == other.leverage
            && self.orders_per_side == other.orders_per_side
            && self.final_order_distance == other.final_order_distance
            && self.depths == other.depths
            && self.rate_limit == other.rate_limit
            && self.bps == other.bps
            && self.tick_window == other.tick_window
            && self.record == other.record
            && self.state_file == other.state_file
            && self.strategy == other.strategy
    }
}
