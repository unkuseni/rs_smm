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

/// Regularization strength for the engine's horizon regression. Small but
/// enough to stabilize the collinear lagged feature pairs.
pub const RIDGE_LAMBDA: f64 = 0.01;

/// Fits a ridge-regularized linear model (with intercept) by solving the
/// regularized normal equations (X'X + lambda*I) w = X'y with a Cholesky
/// decomposition. Returns (target_scale, weights) where weights[0] is the
/// intercept. Deterministic and exact for these tiny matrices; the L2
/// penalty keeps the fit stable under the near-collinear lagged feature
/// pairs the engine uses (plain least squares can explode there).
pub fn ridge_fit(x: &Array2<f64>, y: &Array1<f64>, lambda: f64) -> Result<(f64, Array1<f64>), String> {
    let (n, d) = x.dim();
    if n < 2 || d == 0 {
        return Err("Not enough observations for ridge regression".to_string());
    }
    // Mean-scale targets so the conditioning is independent of the price
    // level.
    let y_scale = y.iter().fold(0.0f64, |acc, v| acc.max(v.abs())).max(1.0);
    let y_norm = y.mapv(|v| v / y_scale);

    // Augment with an intercept column of ones.
    let mut xa = Array2::ones((n, d + 1));
    xa.slice_mut(ndarray::s![.., 1..]).assign(x);

    let mut xtx = xa.t().dot(&xa);
    let xty = xa.t().dot(&y_norm);
    // L2 penalty on the feature weights only (not the intercept).
    for i in 1..=d {
        xtx[(i, i)] += lambda;
    }
    let w = solve_cholesky(&xtx, &xty).ok_or_else(|| "Ridge solve failed".to_string())?;
    Ok((y_scale, w))
}

/// Solves A x = b for a symmetric positive-definite A via Cholesky
/// decomposition (A = L L^T). Returns None when A is not positive definite.
fn solve_cholesky(a: &Array2<f64>, b: &Array1<f64>) -> Option<Array1<f64>> {
    let n = a.nrows();
    let mut l = Array2::zeros((n, n));
    for i in 0..n {
        for j in 0..=i {
            let mut sum = a[(i, j)];
            for k in 0..j {
                sum -= l[(i, k)] * l[(j, k)];
            }
            if i == j {
                if sum <= 0.0 {
                    return None;
                }
                l[(i, j)] = sum.sqrt();
            } else {
                l[(i, j)] = sum / l[(j, j)];
            }
        }
    }
    // Forward substitution: L y = b.
    let mut y = Array1::zeros(n);
    for i in 0..n {
        let mut sum = b[i];
        for k in 0..i {
            sum -= l[(i, k)] * y[k];
        }
        y[i] = sum / l[(i, i)];
    }
    // Back substitution: L^T w = y.
    let mut w = Array1::zeros(n);
    for i in (0..n).rev() {
        let mut sum = y[i];
        for k in i + 1..n {
            sum -= l[(k, i)] * w[k];
        }
        w[i] = sum / l[(i, i)];
    }
    Some(w)
}

/// Predicts one feature row (without the intercept column) using weights
/// produced by ridge_fit.
pub fn ridge_predict(weights: &(f64, Array1<f64>), row: &[f64]) -> f64 {
    let (scale, w) = weights;
    let mut out = w[0];
    for (i, value) in row.iter().enumerate() {
        out += w[i + 1] * value;
    }
    out * scale
}

#[cfg(test)]
mod tests {
    use super::*;
    use ndarray::array;

    #[test]
    fn ridge_recovers_linear_relation() {
        // y = 3 + 2*x1 - x2 + small noise.
        let x = array![[1.0, 2.0], [2.0, 1.0], [3.0, 0.5], [4.0, 0.0], [5.0, -1.0]];
        let y = array![3.1, 6.0, 8.4, 11.1, 13.9];
        let weights = ridge_fit(&x, &y, 0.01).expect("fit");
        let pred = |row: &[f64]| ridge_predict(&weights, row);
        for i in 0..x.nrows() {
            let row: Vec<f64> = x.row(i).to_vec();
            let est = pred(&row);
            let true_v = 3.0 + 2.0 * row[0] - row[1];
            assert!(
                (est - true_v).abs() < 0.4,
                "ridge estimate {} too far from {}",
                est,
                true_v
            );
        }
    }

    #[test]
    fn ridge_stays_finite_on_collinear_inputs() {
        // Perfectly collinear lagged pairs: coefficients must stay bounded.
        let n = 20;
        let mut rows = Vec::new();
        let mut ys = Vec::new();
        for i in 0..n {
            let t = i as f64 * 0.01;
            rows.extend_from_slice(&[t, t + 1e-12, 1.0 - t, t * 2.0, t, t, 1.0 - t, t * 2.0]);
            ys.push(100.0 + t);
        }
        let x = Array2::from_shape_vec((n, 8), rows).expect("shape");
        let y = Array1::from(ys);
        let weights = ridge_fit(&x, &y, 0.01).expect("fit");
        let pred = ridge_predict(&weights, &[0.1, 0.1, 0.9, 0.2, 0.1, 0.1, 0.9, 0.2]);
        assert!(pred.is_finite() && (pred - 100.1).abs() < 1.0, "pred {}", pred);
    }
}
