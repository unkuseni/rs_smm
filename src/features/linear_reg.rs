use linfa::{
     traits::{Fit, Predict}, Dataset
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
    features: Array2<f64>, // imbalance_ratio, voi, ofi
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