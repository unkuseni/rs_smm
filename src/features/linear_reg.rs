use linfa::{
    traits::{Fit, Predict},
    Dataset,
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

