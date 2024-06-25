use std::{
    io::Read,
    time::{SystemTime, UNIX_EPOCH},
};

use num_traits::{Float, Signed};

use serde::Deserialize;

pub fn round_step<T: Float>(num: T, step: T) -> T {
    (num / step).round() * step
}

pub fn geometric_weights(ratio: f64, n: usize) -> Vec<f64> {
    assert!(ratio > 0.0 && ratio < 1.0, "Ratio must be between 0 and 1");

    // Generate the geometric series
    let weights: Vec<f64> = (0..n).map(|i| ratio.powi(i as i32)).collect();

    // Calculate the sum of the series to normalize
    let sum: f64 = weights.iter().sum();

    // Normalize the weights so that their sum equals 1
    weights.iter().map(|w| w / sum).collect()
}

pub fn generate_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards")
        .as_millis() as u64
}

pub fn calculate_exponent(n: f64) -> f64 {
    let exponent = -0.5 * n;
    f64::exp(exponent)
}

/*
This function generates a linearly spaced vector of f64 numbers.

Parameters:
- start: f64 - The starting value of the sequence.
- end: f64 - The ending value of the sequence.
- n: usize - The number of elements in the sequence.

Return:
- Vec<f64> - A vector containing n numbers, equally spaced between start and end.
*/
pub fn linspace<T: Float + Signed + PartialOrd>(start: T, end: T, n: usize) -> Vec<T> {
    // Calculate the step size between consecutive numbers in the sequence.
    // The step size is calculated as (end - start) divided by the number of elements minus 1.
    let step = (end - start) / T::from(n - 1).unwrap();

    // Generate the sequence of numbers using the start value, the step size and the number of elements.
    // The map function applies the formula (start + i as T * step) to each element in the range 0..n,
    // creating a new vector with the resulting numbers.
    (0..n).map(|i| start + T::from(i).unwrap() * step).collect()
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
    // Check if the number of elements is zero and return an empty vector if it is.
    if n == 0 {
        return Vec::new();
    }

    // Check if start or end is zero and panic if it is.
    if start == T::zero() || end == T::zero() {
        panic!("Start and end must be non-zero for a geometric space.");
    }

    // Calculate the logarithmic ratio between consecutive numbers in the sequence.
    let log_ratio = (end / start).log10() / T::from(n - 1).unwrap();

    // Create a vector with pre-allocated capacity for n elements.
    let mut res = Vec::with_capacity(n);

    // Add the starting value to the vector.
    res.push(start);

    // Generate the sequence of numbers using the logarithmic ratio and the number of elements.
    // The map function applies the formula res[i - 1] * 10.0_f64.powf(log_ratio) to each element in the range 1..n,
    // creating a new vector with the resulting numbers.
    for i in 1..n {
        res.push(res[i - 1] * T::from(10.0_f64).unwrap().powf(log_ratio));
    }

    // Return the generated vector.
    res
}

pub fn nbsqrt<T: PartialOrd + Float + Signed>(num: T) -> T {
    if num >= T::zero() {
        num.sqrt()
    } else {
        -(num.abs().sqrt())
    }
}

pub fn spread_price_in_bps(spread: f64, price: f64) -> f64 {
    let percent = spread / price;
    let bps = percent * 10000.0;
    bps.round()
}

pub trait Round<T> {
    fn round_to(&self, digit: u8) -> T;
    fn clip(&self, min: T, max: T) -> T;
    fn count_decimal_places(&self) -> usize;
}
impl Round<f64> for f64 {
    fn round_to(&self, digit: u8) -> f64 {
        let pow = 10.0_f64.powi(digit as i32);
        (self * pow).round() / pow
    }
    fn clip(&self, min: f64, max: f64) -> f64 {
        self.max(min).min(max)
    }
    fn count_decimal_places(&self) -> usize {
        let num_str = self.to_string();
        match num_str.split_once('.') {
            Some((_, decimals)) => decimals.trim_end_matches('0').len(),
            None => 0,
        }
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn test_round() {
        assert_eq!(round_step(15643.456, 1.0), 15643.0);
        assert_eq!(round_step(5.6567422344, 0.0005), 5.6565);
        println!("{:#?}", spread_price_in_bps(0.00055, 0.5678));
    }

    #[test]
    fn test_time() {
        assert_ne!(generate_timestamp(), 0);
        println!("{:#?}", generate_timestamp());
        let num: f64 = 0.0000016;
        println!("{:#?}", num.abs().round_to(6));
    }

    #[test]
    fn test_places() {
        let num: f64 = 0.000001;
        println!("{:#?}", num.abs().count_decimal_places());
    }

    #[test]
    fn lin() {
        let num = linspace(0.6243, 0.6532, 14);
        let num_geom = geomspace(1.0, 0.01, 14);
        println!("{:#?}", num);
        println!("{:#?}", num_geom);
    }

    #[test]
    fn params() {
        let result = read_toml("./src/util/test.toml");
        println!("{:#?}", result);
    }
}

/// This section is for a toml parser that will be used for reading config files
///
pub fn read_toml(path: &str) -> Config {
    let mut file = std::fs::File::open(path).expect("Unable to open file");
    let mut contents = String::new();
    file.read_to_string(&mut contents)
        .expect("Unable to read file");
    toml::from_str(&contents).expect("Unable to parse file")
}

#[derive(Deserialize, Debug)]
pub struct Config {
    pub exchange: String,
    pub symbols: Vec<String>,
    pub api_keys: Vec<(String, String, String)>,
    pub balances: Vec<(String, f64)>,
    pub leverage: f64,
    pub orders_per_side: usize,
    pub final_order_distance: f64,
    pub depths: Vec<usize>,
    pub rebalance_ratio: f64,
    pub rate_limit: u32,
    pub bps: Vec<f64>,
}
