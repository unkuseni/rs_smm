use linfa::traits::{Fit, Predict};
use linfa_linear::LinearRegression;
use ndarray::{Array1, Array2};

pub fn mid_price_regression(
    mid_price_array: Array1<f64>,
    mut features: Array2<f64>,
    curr_spread: f64,
) -> f64 {
    for i in 0..3 {
        let mut column = features.column_mut(i);
        column.mapv_inplace(|x| x / curr_spread);
    }

    let dataset = linfa::Dataset::new(features, mid_price_array);
    let lin_reg = LinearRegression::new();
    let model = lin_reg.fit(&dataset).unwrap();
    let prediction = model.predict(&dataset);

    // Assuming you want to return some value related to the prediction here
    // Placeholder return value (modify as needed)
    prediction.mean().unwrap_or(0.0)
}
