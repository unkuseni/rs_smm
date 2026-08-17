use linfa::{
    traits::{Fit, Predict},
    Dataset,
};
use linfa_linear::LinearRegression;
use ndarray::{s, Array1, Array2};

/// Fits a linear regression of mid prices on the provided features and
/// returns a one-step-ahead prediction: the model is trained on all but the
/// last observation and predicts the most recent feature row.
///
/// # Arguments
///
/// * `mid_price_array` - The mid prices used as regression targets.
/// * `features` - The feature matrix (one row per observation).
/// * `curr_spread` - The current spread in basis points, used to normalize the features.
///
/// # Returns
///
/// The predicted mid price for the latest observation, or an error message.
pub fn mid_price_regression(
    mid_price_array: Array1<f64>,
    features: Array2<f64>,
    curr_spread: f64,
) -> Result<f64, String> {
    let n = features.nrows();
    if n < 2 {
        return Err("Not enough observations for regression".to_string());
    }
    if !curr_spread.is_finite() || curr_spread <= 0.0 {
        return Err("Invalid current spread".to_string());
    }

    // Normalize features by the current spread.
    let normalized_features = features.map(|&x| x / curr_spread);

    // Train on all but the last observation.
    let train_features = normalized_features.slice(s![0..n - 1, ..]).to_owned();
    let train_targets = mid_price_array.slice(s![0..n - 1]).to_owned();
    let dataset = Dataset::new(train_features, train_targets);

    let model = LinearRegression::default()
        .fit(&dataset)
        .map_err(|e| format!("Failed to fit the model: {}", e))?;

    // Predict the most recent feature row.
    let last_row = normalized_features.slice(s![n - 1..n, ..]).to_owned();
    let prediction = model.predict(&last_row);
    Ok(prediction[0])
}
