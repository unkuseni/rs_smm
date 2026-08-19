//! Tests for the portfolio risk evaluation (drawdown and aggregate inventory).

use rs_smm::strategy::market_maker::{evaluate_risk, RiskDecision};
use skeleton::util::helpers::StrategyConfig;

#[test]
fn drawdown_limit_halts() {
    let strategy = StrategyConfig {
        max_drawdown_frac: 0.2,
        ..Default::default()
    };
    assert_eq!(
        evaluate_risk(1000.0, 900.0, 0.0, 0.0, &strategy),
        RiskDecision::Ok
    );
    assert_eq!(
        evaluate_risk(1000.0, 790.0, 0.0, 0.0, &strategy),
        RiskDecision::Halt
    );
}

#[test]
fn portfolio_inventory_limit_halts() {
    let strategy = StrategyConfig {
        max_portfolio_delta: 0.8,
        ..Default::default()
    };
    assert_eq!(
        evaluate_risk(1000.0, 1000.0, 0.5, 0.0, &strategy),
        RiskDecision::Ok
    );
    assert_eq!(
        evaluate_risk(1000.0, 1000.0, 0.9, 0.0, &strategy),
        RiskDecision::Halt
    );
}

#[test]
fn zero_limits_never_halt() {
    let strategy = StrategyConfig::default();
    assert_eq!(
        evaluate_risk(1000.0, 0.0, 1000.0, 0.0, &strategy),
        RiskDecision::Ok
    );
    assert_eq!(
        evaluate_risk(1000.0, 0.0, 1000.0, 1000.0, &strategy),
        RiskDecision::Ok
    );
}

#[test]
fn volatility_limit_halts() {
    let strategy = StrategyConfig {
        max_vol_bps: 50.0,
        ..Default::default()
    };
    assert_eq!(
        evaluate_risk(1000.0, 1000.0, 0.0, 30.0, &strategy),
        RiskDecision::Ok
    );
    assert_eq!(
        evaluate_risk(1000.0, 1000.0, 0.0, 80.0, &strategy),
        RiskDecision::Halt
    );
}

#[test]
fn prediction_hit_rate_evaluation() {
    use skeleton::util::helpers::StrategyConfig;

    // A steep uptrend: the 30 bps up barrier is reached quickly, so a
    // +1 prediction should always be right.
    let entries: Vec<(u64, f64, f64)> = (0..20)
        .map(|i| {
            let mid = 100.0 + i as f64 * 0.1;
            (i * 10, mid, mid * 1.003)
        })
        .collect();
    let strategy = StrategyConfig {
        predict_horizon_bps: 30.0,
        ..Default::default()
    };
    let (evals, hits) = rs_smm::backtest::evaluate_predictions(&entries, &strategy);
    assert!(evals > 0, "should evaluate some predictions");
    assert_eq!(evals, hits, "all up-trend predictions should hit");

    // Without a configured horizon nothing is evaluated.
    let (evals0, hits0) =
        rs_smm::backtest::evaluate_predictions(&entries, &StrategyConfig::default());
    assert_eq!((evals0, hits0), (0, 0));
}

#[test]
fn funding_pnl_is_linear() {
    // Long 10 units at 100, 10 bps/hour for 1 hour: pays 0.001 * 1000.
    let pnl = rs_smm::backtest::funding_pnl(10.0, 100.0, 10.0, 1.0);
    assert!((pnl + 1.0).abs() < 1e-9, "funding pnl was {}", pnl);
    // Short position earns the funding.
    let pnl_short = rs_smm::backtest::funding_pnl(-10.0, 100.0, 10.0, 1.0);
    assert!((pnl_short - 1.0).abs() < 1e-9);
}

#[test]
fn sweep_value_sets_known_keys() {
    let mut strategy = StrategyConfig::default();
    rs_smm::backtest::sweep_value("trade_weight", 0.42, &mut strategy).expect("known key");
    assert_eq!(strategy.trade_weight, 0.42);
    rs_smm::backtest::sweep_value("as_gamma", 123.0, &mut strategy).expect("known key");
    assert_eq!(strategy.as_gamma, 123.0);
    assert!(rs_smm::backtest::sweep_value("nope", 1.0, &mut strategy).is_err());
}
