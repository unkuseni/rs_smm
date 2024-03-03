use std::time::{SystemTime, UNIX_EPOCH};

pub fn round_step(num: f64, step: f64) -> f64 {
    let p = (1.0 / step) as i32;
    (num * p as f64).floor() / p as f64
}

pub fn generate_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards")
        .as_millis() as u64
}

// Generate a linearly spaced array of `n` numbers between `start` and `end`
pub fn linspace(start: f64, end: f64, n: usize) -> Vec<f64> {
    let step = (end - start) / (n - 1) as f64;
    (0..n).map(|i| start + i as f64 * step).collect()
}

// Round a number `num` to `digit` decimal places
trait Round {
    fn round_to(&self, digit: u8) -> f64;
    fn clip(&self, min: f64, max: f64) -> f64;
}
impl Round for f64 {
    fn round_to(&self, digit: u8) -> f64 {
        (self * 10.0_f64.powi(digit as i32)).round() / 10.0_f64.powi(digit as i32)
    }
    fn clip(&self, min: f64, max: f64) -> f64 {
        if self < &min {
            min
        } else if self > &max {
            max
        } else {
            *self
        }
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
