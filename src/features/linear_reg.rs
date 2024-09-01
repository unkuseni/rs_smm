use linfa::{
    dataset, traits::{Fit, Predict}, Dataset
};
use linfa_linear::LinearRegression;
use ndarray::{Array1, Array2};
/// Performs linear regression on the given mid price data using the provided features.
///
/// # Arguments
///
/// * `mid_price_array` - The array of mid prices to be used for regression.
/// * `features` - The array of features used for regression.
/// * `curr_spread` - The current spread used to normalize the features.
///
/// # Returns
///
/// The mean of the prediction or 0.0 if the prediction is empty.
pub fn mid_price_regression(
    mid_price_array: Array1<f64>,
    features: Array2<f64>,
    curr_spread: f64,
) -> Result<f64, String> {
    // Normalize features if needed
    let normalized_features = features.map(|&x| x / curr_spread);

    // Create the dataset
    let dataset = Dataset::new(normalized_features, mid_price_array);

    // Create and fit the model
    let model = LinearRegression::default()
        .fit(&dataset)
        .map_err(|e| format!("Failed to fit the model: {}", e))?;

    // Make predictions
    let predictions = model.predict(&dataset);

    // Return the mean of the predictions
    Ok(predictions.mean().unwrap_or(0.0))
}

pub fn default_regression_single_feature(
    mid_price_array: &[f64],
    feature: &[f64],
) -> Result<f64, String> {
    use ndarray::{Array1, Array2};
    use linfa::{Dataset, traits::Fit};
    use linfa_linear::LinearRegression;

    // Convert slices to Array1
    let mid_prices = Array1::from_vec(mid_price_array.to_vec());
    let features = Array1::from_vec(feature.to_vec());

    // Reshape features to a 2D array with one column
    let features_2d = features.clone().into_shape((features.len(), 1)).map_err(|e| format!("Failed to reshape features: {}", e))?;

    let dataset = Dataset::new(features_2d, mid_prices);
    let model = LinearRegression::default().fit(&dataset).map_err(|e| format!("Failed to fit the model: {}", e))?;

    let predictions = model.predict(&dataset);

    Ok(predictions.mean().unwrap_or(0.0))
}

// use ndarray::{Array1, Array2};
// use linfa::{Dataset, traits::Fit};
// use linfa_linear::LinearRegression;
// use std::collections::VecDeque;

// pub struct RollingLinearRegression {
//     model: LinearRegression,
//     window_size: usize,
//     features: VecDeque<Array1<f64>>,
//     mid_prices: VecDeque<f64>,
// }

// impl RollingLinearRegression {
//     pub fn new(window_size: usize) -> Self {
//         RollingLinearRegression {
//             model: LinearRegression::default(),
//             window_size,
//             features: VecDeque::with_capacity(window_size),
//             mid_prices: VecDeque::with_capacity(window_size),
//         }
//     }

//     pub fn update(&mut self, new_features: Array1<f64>, new_mid_price: f64, curr_spread: f64) -> Result<f64, String> {
//         // Normalize and add new data
//         let normalized_features = new_features.map(|&x| x / curr_spread);
//         self.features.push_back(normalized_features);
//         self.mid_prices.push_back(new_mid_price);

//         // Remove oldest data if window is full
//         if self.features.len() > self.window_size {
//             self.features.pop_front();
//             self.mid_prices.pop_front();
//         }

//         // Only predict if we have enough data
//         if self.features.len() == self.window_size {
//             self.predict()
//         } else {
//             Ok(new_mid_price) // Return current mid price if not enough data
//         }
//     }

//     fn predict(&mut self) -> Result<f64, String> {
//         let features = Array2::from_shape_vec(
//             (self.features.len(), self.features[0].len()),
//             self.features.iter().flat_map(|a| a.to_vec()).collect(),
//         ).map_err(|e| format!("Failed to create features array: {}", e))?;

//         let mid_prices = Array1::from(self.mid_prices.clone().into_iter().collect::<Vec<f64>>());

//         let dataset = Dataset::new(features, mid_prices);

//         self.model = self.model.fit(&dataset)
//             .map_err(|e| format!("Failed to fit the model: {}", e))?;

//         let predictions = self.model.predict(&dataset);
//         Ok(predictions.mean().unwrap_or(self.mid_prices.back().copied().unwrap_or(0.0)))
//     }
// }