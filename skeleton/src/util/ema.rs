use std::collections::VecDeque;

pub struct EMA {
    window: usize,
    alpha: f64,
    arr: VecDeque<f64>,
    value: f64,
}

impl EMA {
    pub fn new(window: usize, alpha: Option<f64>) -> Self {
        let alpha = alpha.unwrap_or_else(|| 2.0 / (window + 1) as f64);
        Self {
            window,
            alpha,
            arr: VecDeque::with_capacity(window),
            value: 0.0,
        }
    }

    pub fn initialize(&mut self, arr_in: &[f64]) {
        self.arr.clear();
        self.value = arr_in[0]; // Initialize with the first value
        for val in arr_in.iter().skip(1) {
            self.update(*val);
        }
    }

    pub fn update(&mut self, new_val: f64) {
        if self.arr.len() == self.window {
            self.arr.pop_front();
        }
        self.value = self.alpha * new_val + (1.0 - self.alpha) * self.value;
        self.arr.push_back(self.value);
    }

    // Access the current EMA value
    pub fn value(&self) -> f64 {
        self.value
    }

    // Access the internal EMA values as a Vec
    pub fn arr(&self) -> Vec<f64> {
        self.arr.iter().cloned().collect()
    }
}
