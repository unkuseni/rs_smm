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

pub fn linspace(start: f64, end: f64, n: usize) -> Vec<f64> {
    let step = (end - start) / (n - 1) as f64;
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
