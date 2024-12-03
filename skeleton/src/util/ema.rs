use std::collections::VecDeque;


// The Exponential Moving Average (EMA) struct stores the EMA values and
// other necessary data for the computation of the EMA.
#[derive(Debug, Clone)]
pub struct EMA {
    // The size of the window for the EMA calculation. This is the number of
    // previous data points to consider for the calculation.
    window: usize,
    // The alpha value is a constant that affects the weight given to the
    // most recent data point in the calculation of the EMA. A smaller value
    // gives more weight to recent data points.
    alpha: f64,
    // The arr VecDeque is a deque that holds the EMA values computed so far.
    // It is a ring buffer with a maximum capacity of 'window'.
    arr: VecDeque<f64>,
    // The value is the current EMA value computed from the previous
    // 'window' data points.
    value: f64,
}

impl EMA {
    // The new function creates a new EMA struct with the given window and
    // alpha values. If alpha is not provided, it is calculated based on
    // the window size.
    pub fn new(window: usize, alpha: Option<f64>) -> Self {
        let alpha = alpha.unwrap_or_else(|| 2.0 / (window + 1) as f64);
        Self {
            window,
            alpha,
            arr: VecDeque::with_capacity(window),
            value: 0.0,
        }
    }

    // The initialize function initializes the EMA with the given array of
    // data points. It clears the arr VecDeque and sets the value to the
    // first data point in the array.
    pub fn initialize(&mut self, arr_in: &[f64]) {
        self.arr.clear();
        self.value = arr_in[0]; // Initialize with the first value
        for val in arr_in.iter().skip(1) {
            self.update(*val);
        }
    }

    // The update function updates the EMA with the given new data point.
    // If the window size is reached, it pops the oldest data point from the
    // arr VecDeque. It calculates the new EMA value using the formula:
    // new EMA value = alpha * new data point + (1 - alpha) * old EMA value
    // and pushes the new EMA value to the arr VecDeque.
    pub fn update(&mut self, new_val: f64) {
        if self.arr.len() == self.window {
            self.arr.pop_front();
        }
        self.value = self.alpha * new_val + (1.0 - self.alpha) * self.value;
        self.arr.push_back(self.value);
    }

    // The value function returns the current EMA value.
    pub fn value(&self) -> f64 {
        self.value
    }

    // The arr function returns the internal EMA values as a Vec.
    pub fn arr(&self) -> Vec<f64> {
        self.arr.iter().cloned().collect()
    }
}

