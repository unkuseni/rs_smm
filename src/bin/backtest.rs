//! Offline replay of a recorded market-data session.
//!
//! Usage:
//!   cargo run --release --bin backtest -- <record_file>
//!   cargo run --release --bin backtest -- <record_file> --sweep <key> <start> <end> <step>
//!
//! Strategy parameters come from config.toml (or RS_SMM_CONFIG), so the same
//! recording can be replayed under different strategy settings to A/B test
//! weights, spread scaling, the microprice anchor, and rebalancing. The
//! --sweep mode varies one strategy key over a range and prints a table.

use rs_smm::parameters::use_toml;
use rs_smm::{backtest, backtest::BacktestReport};

#[tokio::main]
async fn main() {
    let mut args = std::env::args().skip(1);
    let Some(path) = args.next() else {
        eprintln!("usage: backtest <record_file> [--sweep <key> <start> <end> <step>]");
        std::process::exit(1);
    };
    let config = use_toml();

    if args.next().as_deref() == Some("--sweep") {
        let key = args
            .next()
            .unwrap_or_else(|| panic!("--sweep requires a parameter key"));
        let start: f64 = args
            .next()
            .and_then(|v| v.parse().ok())
            .unwrap_or_else(|| panic!("--sweep requires a numeric start"));
        let end: f64 = args
            .next()
            .and_then(|v| v.parse().ok())
            .unwrap_or_else(|| panic!("--sweep requires a numeric end"));
        let step: f64 = args
            .next()
            .and_then(|v| v.parse().ok())
            .unwrap_or_else(|| panic!("--sweep requires a numeric step"));
        if step <= 0.0 {
            panic!("--sweep step must be positive");
        }

        println!("key	value	symbol	total_pnl	sharpe	maker_fills	taker_fills	funding_paid	hit_rate");
        let mut value = start;
        while value <= end + 1e-12 {
            let mut cfg = config.clone();
            if let Err(e) = backtest::sweep_value(&key, value, &mut cfg.strategy) {
                eprintln!("{}", e);
                std::process::exit(1);
            }
            match backtest::run(&path, &cfg).await {
                Ok(report) => {
                    for s in &report.symbols {
                        let hit_rate = if s.pred_evals > 0 {
                            s.pred_hits as f64 / s.pred_evals as f64
                        } else {
                            0.0
                        };
                        println!(
                            "{}	{}	{}	{:.6}	{:.4}	{}	{}	{:.6}	{:.4}",
                            key,
                            value,
                            s.symbol,
                            s.total_pnl,
                            s.sharpe,
                            s.maker_fills,
                            s.taker_fills,
                            s.funding_paid,
                            hit_rate
                        );
                    }
                }
                Err(e) => {
                    eprintln!("Backtest failed at {}={}: {}", key, value, e);
                    std::process::exit(1);
                }
            }
            value += step;
        }
        return;
    }

    match backtest::run(&path, &config).await {
        Ok(report) => print_report(&report),
        Err(e) => {
            eprintln!("Backtest failed: {}", e);
            std::process::exit(1);
        }
    }
}

fn print_report(report: &BacktestReport) {
    println!("Recording: {} ms", report.duration_ms);
    for s in &report.symbols {
        println!("
=== {} ===", s.symbol);
        println!("  book updates:      {}", s.book_updates);
        println!("  trades:            {}", s.trades);
        println!("  grid refreshes:    {}", s.grid_refreshes);
        println!(
            "  maker fills:       {} (notional {:.2}, fees {:.4})",
            s.maker_fills, s.maker_notional, s.maker_fees
        );
        println!(
            "  taker fills:       {} (notional {:.2}, fees {:.4})",
            s.taker_fills, s.taker_notional, s.taker_fees
        );
        println!("  funding paid:      {:.6}", s.funding_paid);
        println!("  fills per level:   {:?} (best level first)", s.fill_by_level);
        println!(
            "  final position:    {:.6} @ mid {:.6}",
            s.final_position, s.final_mid
        );
        println!("  total pnl:         {:.6}", s.total_pnl);
        println!("  pnl mean/std:      {:.4}", s.sharpe);
        println!("  adverse ratio:     {:.3}", s.adverse_ratio);
        println!("  grid amends:       {}", s.grid_amends);
        println!("  rebalances:        {}", s.rebalance_count);
        if s.pred_evals > 0 {
            println!(
                "  pred hit-rate:     {:.4} ({}/{} evals)",
                s.pred_hits as f64 / s.pred_evals as f64,
                s.pred_hits,
                s.pred_evals
            );
        }
    }
}
