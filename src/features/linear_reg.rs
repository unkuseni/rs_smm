use linfa::traits::{Fit, Predict};
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
    mut features: Array2<f64>,
    curr_spread: f64,
) -> f64 {
    // Normalize the features by dividing each value in the feature columns by the current spread
    for i in 0..3 {
        let mut column = features.column_mut(i);
        column.mapv_inplace(|x| x / curr_spread);
    }

    // Create a linfa dataset with the features and mid price array
    let dataset = linfa::Dataset::new(features, mid_price_array);

    // Create a new linear regression model
    let lin_reg = LinearRegression::new();

    // Fit the model to the dataset and get the resulting model
    let model = lin_reg.fit(&dataset).unwrap();

    // Use the model to predict the mid price values
    let prediction = model.predict(&dataset);

    // Assuming you want to return some value related to the prediction here
    // Return the mean of the prediction or 0.0 if the prediction is empty
    prediction.mean().unwrap_or(0.0)
}


    //  // Independent variables
    // let mut x: Array2<f64> = array![
    //     [2.0, 6.0, 12.0],
    //     [4.0, 9.0, 16.0],
    //     [6.0, 12.0, 20.0]
    // ];

    // // Dependent variable
    // let y: Array1<f64> = array![6.0, 15.0, 24.0];
