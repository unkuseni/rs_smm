use std::time::{SystemTime, UNIX_EPOCH};

pub fn round_step(num: f64, step: f64) -> f64 {
    let p = (1.0 / step) as i32;
    let p_as_f64 = p as f64;
    (num * p_as_f64).floor() / p_as_f64
}

pub fn generate_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards")
        .as_millis() as u64
}


/*
This function generates a linearly spaced vector of f64 numbers.

Parameters:
- start: f64 - The starting value of the sequence.
- end: f64 - The ending value of the sequence.
- n: usize - The number of elements in the sequence.

Return:
- Vec<f64> - A vector containing n numbers, equally spaced between start and end.
*/
pub fn linspace(start: f64, end: f64, n: usize) -> Vec<f64> {
    // Calculate the step size between consecutive numbers in the sequence.
    // The step size is calculated as (end - start) divided by the number of elements minus 1.
    let step = (end - start) / (n - 1) as f64;

    // Generate the sequence of numbers using the start value, the step size and the number of elements.
    // The map function applies the formula (start + i as f64 * step) to each element in the range 0..n,
    // creating a new vector with the resulting numbers.
    (0..n).map(|i| start + i as f64 * step).collect()
}

trait Round {
    fn round_to(&self, digit: u8) -> f64;
    fn clip(&self, min: f64, max: f64) -> f64;
}
impl Round for f64 {
    fn round_to(&self, digit: u8) -> f64 {
        let pow = 10.0_f64.powi(digit as i32);
        (self * pow).round() / pow
    }
    fn clip(&self, min: f64, max: f64) -> f64 {
        self.max(min).min(max)
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn test_round() {
        assert_eq!(round_step(0.1, 0.1), 0.1);
        assert_eq!(round_step(5.67, 0.2), 5.6);
    }

    #[test]
    fn test_time() {
        assert_ne!(generate_timestamp(), 0);
        println!("{:#?}", generate_timestamp());
        let num: f64 = -5.437945;
        println!("{:#?}", num.abs().round_to(3));
    }

}
