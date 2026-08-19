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
        evaluate_risk(1000.0, 900.0, 0.0, &strategy),
        RiskDecision::Ok
    );
    assert_eq!(
        evaluate_risk(1000.0, 790.0, 0.0, &strategy),
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
        evaluate_risk(1000.0, 1000.0, 0.5, &strategy),
        RiskDecision::Ok
    );
    assert_eq!(
        evaluate_risk(1000.0, 1000.0, 0.9, &strategy),
        RiskDecision::Halt
    );
}

#[test]
fn zero_limits_never_halt() {
    let strategy = StrategyConfig::default();
    assert_eq!(
        evaluate_risk(1000.0, 0.0, 1000.0, &strategy),
        RiskDecision::Ok
    );
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
