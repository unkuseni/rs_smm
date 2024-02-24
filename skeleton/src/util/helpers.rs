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

#[cfg(test)]
mod tests {

    use super::*;
    use crate::ex_bybit;

    #[test]
    fn test_round() {
        assert_eq!(round_step(0.1, 0.1), 0.1);
        assert_eq!(round_step(5.67, 0.2), 5.6);
    }

    #[test]
    fn test_time() {
        assert_ne!(generate_timestamp(), 0);
        println!("{:#?}", generate_timestamp());
    }

    #[test]
    fn test_order() {
        ex_bybit::orderbook();
    }
}
