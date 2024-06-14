use std::collections::VecDeque;

use bybit::model::WsTrade;
use ndarray::{Array1, Array2};

type TradeImb = (String, VecDeque<WsTrade>);
pub struct Engine {
    pub imbalance_ratio: f64,
    pub voi: f64,
    pub trade_imb: TradeImb,
    pub impact: f64,
    pub mid_price_basis: f64,
    pub mid_price_change: f64,
    pub avg_trade_price: f64,
    pub target_dataset: Array1<f64>,
    pub record_dataset: Array2<f64>,
    pub pred: f64,
}

impl Engine {
    pub fn new() -> Self {
        Self {
            imbalance_ratio: 0.0,
            voi: 0.0,
            trade_imb: ("".to_string(), VecDeque::new()),
            impact: 0.0,
            mid_price_basis: 0.0,
            mid_price_change: 0.0,
            avg_trade_price: 0.0,
            target_dataset: Array1::zeros(0),
            record_dataset: Array2::zeros((37, 3)),
            pred: 0.0,
        }
    }
}
