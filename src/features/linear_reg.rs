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

#[cfg(test)]
mod tests {
    use super::*;
    use ndarray::array;

    #[test]
    fn test_mid_price_regression() {
        let mid_price = array![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0];
        let features = array![
            [1.0, 2.0, 3.0],
            [1.1, 2.2, 2.9],
            [1.2, 2.1, 3.1],
            [1.3, 2.3, 2.8],
            [1.4, 2.4, 3.2],
            [1.5, 2.5, 3.3],
            [1.6, 2.6, 3.4],
            [1.7, 2.7, 3.5],
            [1.8, 2.8, 3.6],
            [1.9, 2.9, 3.7]
        ];
        let curr_spread = 2.0;
        let result = mid_price_regression(mid_price, features, curr_spread).unwrap();
        println!("Result: {}", result);
        assert!((result - 5.5).abs() < 1e-6);
    }

    #[test]
    fn test_mid_price_regression_extended() {
        let mid_price = array![
            1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0, 13.0, 14.0, 15.0, 16.0,
            17.0, 18.0, 19.0, 20.0
        ];

        let features = array![
            [1.0, 2.0, 3.0],
            [1.1, 2.2, 2.9],
            [1.2, 2.1, 3.1],
            [1.3, 2.3, 2.8],
            [1.4, 2.4, 3.2],
            [1.5, 2.5, 3.3],
            [1.6, 2.6, 3.4],
            [1.7, 2.7, 3.5],
            [1.8, 2.8, 3.6],
            [1.9, 2.9, 3.7],
            [2.0, 3.0, 3.8],
            [2.1, 3.1, 3.9],
            [2.2, 3.2, 4.0],
            [2.3, 3.3, 4.1],
            [2.4, 3.4, 4.2],
            [2.5, 3.5, 4.3],
            [2.6, 3.6, 4.4],
            [2.7, 3.7, 4.5],
            [2.8, 3.8, 4.6],
            [2.9, 3.9, 4.7]
        ];

        let curr_spread = 2.5;
        let result = mid_price_regression(mid_price, features, curr_spread).unwrap();
        println!("Result: {}", result);
        assert!((result - 10.5).abs() < 1e-6);
    }

    #[test]
    fn test_mid_price_regression_with_negatives() {
        let mid_price = array![-1.0, -0.5, 0.0, 0.5, 1.0, 1.5, 2.0, 2.5, 3.0, 3.5];

        let features = array![
            [-1.0, -0.5, 0.0],
            [-0.8, -0.3, 0.2],
            [-0.6, -0.1, 0.4],
            [-0.4, 0.1, 0.6],
            [-0.2, 0.3, 0.8],
            [0.0, 0.5, 1.0],
            [0.2, 0.7, 1.2],
            [0.4, 0.9, 1.4],
            [0.6, 1.1, 1.6],
            [0.8, 1.3, 1.8]
        ];

        let curr_spread = 1.0;
        let result = mid_price_regression(mid_price, features, curr_spread).unwrap();
        println!("Result: {}", result);
        // Adjust this assertion based on the expected result
        assert!((result - 1.25).abs() < 1e-6);
    }

    #[test]
    fn test_mid_price_regression_with_negatives_extended() {
        // Add more negative values
        let mid_price = array![
            -1.0, -0.5, 0.0, 0.5, 1.0, 1.5, 2.0, 2.5, 3.0, 3.5, 4.0, 0.0, 5.0, 5.5, 6.0, 0.0, 7.0,
            7.5, 8.0, 8.5
        ];
        // Add more features
        let features = array![
            [1.0, -0.8, 0.0],
            [0.8, -0.3, 0.2],
            [0.6, 0.6, 0.4],
            [0.4, -0.1, 0.6],
            [0.2, -0.3, 0.8],
            [0.0, -0.5, 1.0],
            [0.2, -0.7, 1.2],
            [0.4, 0.9, 1.4],
            [0.6, -1.1, 1.6],
            [0.8, 0.1, 1.8],
            [1.0, -1.5, 2.0],
            [1.2, -1.7, 2.2],
            [1.4, -1.9, 2.4],
            [1.6, 2.1, 2.6],
            [1.8, -2.3, 2.8],
            [2.0, 0.0, 3.0],
            [2.2, 2.7, 3.2],
            [2.4, -2.9, 3.4],
            [2.6, 0.1, 3.6],
            [2.8, -3.3, 3.8]
        ];

        let curr_spread = 1.0;
        let result = mid_price_regression(mid_price, features, curr_spread).unwrap();
        println!("Result: {}", result);
    }
}
