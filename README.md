# Rust Simple Market Maker (RS_SMM)

## Overview

RS_SMM is a sophisticated market making bot implemented in Rust. It's designed to provide liquidity and profit from the bid-ask spread in cryptocurrency markets. The system supports multiple exchanges, employs advanced order book analysis, and uses dynamic quote generation based on market conditions.

## Table of Contents

1. [Prerequisites](#prerequisites)
2. [Installation](#installation)
3. [Configuration](#configuration)
4. [Running the Bot](#running-the-bot)
5. [Project Structure](#project-structure)
6. [Making Changes](#making-changes)
7. [Key Components](#key-components)
8. [Contributing](#contributing)
9. [Disclaimer](#disclaimer)

## Prerequisites

- Rust (latest stable version)
- Cargo
- Git
- OpenSSL development files (required by the `rs_bybit` crate)
- An account with supported exchanges (currently Bybit and Binance)
- API keys for the exchanges you plan to use

> OpenSSL setup: on Linux install `libssl-dev` (Debian/Ubuntu) or
> `openssl-devel` (Fedora). On Windows install the Shining Light
> [Win64 OpenSSL](https://slproweb.com/products/Win32OpenSSL.html) package
> (which provides `include/` and `lib/`), and point `openssl-sys` at it by
> setting the `OPENSSL_DIR` environment variable, e.g.:
> `$env:OPENSSL_DIR = "C:\Program Files\OpenSSL-Win64"` before running cargo.

## Installation

1. Clone the repository:
   ```
   git clone https://github.com/your-repo/rs_smm.git
   cd rs_smm
   ```

2. Build the project:
   ```
   cargo build --release
   ```

## Configuration

1. Create a `config.toml` file in the project root directory (or point the
   `RS_SMM_CONFIG` environment variable at a different path).
2. Add your configuration settings. Here's a template that matches what the
   code expects:

   ```toml
   exchange = "bybit"  # or "binance" ("both" is experimental and not yet supported by the strategy loop)
   symbols = ["BTCUSDT", "ETHUSDT"]
   leverage = 10
   orders_per_side = 5
   final_order_distance = 0.01
   depths = [5, 50]
   rate_limit = 100
   tick_window = 6000  # 1 tick = 10ms, so 6000 ticks = 1 min

   # API keys as (key, secret, symbol) tuples.
   # Prefix a value with "env:" to load it from an environment variable,
   # e.g. ["env:RS_SMM_API_KEY", "env:RS_SMM_API_SECRET", "BTCUSDT"].
   api_keys = [["your_api_key", "your_api_secret", "BTCUSDT"]]

   balances = [["BTCUSDT", 1000.0]]

   # Profit spread in basis points (1 bp = 0.01%), keyed by symbol.
   [bps]
   BTCUSDT = 1.0
   ETHUSDT = 2.0

   # Optional: record websocket deltas for offline replay.
   # record = "session.rec"

   # Strategy constants (all optional; defaults shown).
   [strategy]
   imb_weight = 0.25
   deep_imb_weight = 0.10
   trade_weight = 0.30
   voi_weight = 0.10
   deep_ofi_weight = 0.10
   predict_weight = 0.15
   imb_deadband = 0.20
   trade_deadband = 0.20
   predict_gate = 0.0005
   basis_gate = 0.0005
   inventory_adjustment = 0.63
   aggression_min = 0.10
   aggression_max = 0.63
   vol_spread_scaling = 200.0
   use_microprice_anchor = true
   rebalance_threshold = 0.45
   rebalance_cooldown_ms = 30000
   grid_stale_ms = 180000
   position_sync_ms = 60000
   # Avellaneda-Stoikov reservation pricing (0 = heuristic inventory term).
   as_gamma = 0
   as_horizon_secs = 10.0
   as_kappa = 1.0
   # Cont et al. OFI impact fallback predictor (0 disables).
   ofi_impact_k = 0.0
   # Cartea-style regime-smoothed imbalance offset (0 disables).
   regime_weight = 0.0
   # Funding cost in bps/hour for the backtest (0 disables).
   funding_bps_per_hour = 0.0
   # Portfolio risk kill switch (0 disables): halt + cancel-all on breach.
   max_drawdown_frac = 0.0
   max_portfolio_delta = 0.0
   # Optional: persist position/live orders across restarts.
   # state_file = "state.json"
   ```

   The config file is watched for changes: edit and save `config.toml` while
   the bot is running and the new spreads/strategy constants are applied
   within a few seconds (hot reload).

3. Adjust the values according to your trading strategy and risk tolerance.

## Backtesting / Offline Replay

1. Set `record = "session.rec"` in `config.toml` and run the bot; it writes
   the raw websocket deltas (book and trades) to that file.
2. Replay the recording offline with the same or modified strategy settings:
   ```
   cargo run --release --bin backtest -- session.rec
   ```
   The harness replays the deltas through the real feature engine and a
   simulated quote generator: resting orders fill with a FIFO queue-position
   model (a trade at your level consumes the visible queue ahead first),
   market-order rebalances fill at the best bid/ask, funding is accrued on
   held inventory, and maker/taker fees (2/5 bps) are charged. It reports
   PnL, Sharpe (mean/std of per-update PnL), fees, grid refreshes, and fills
   per grid level — use it to A/B test `[strategy]` settings on recorded
   data before deploying.
3. Parameter sensitivity sweep over one strategy key:
   ```
   cargo run --release --bin backtest -- session.rec --sweep trade_weight 0.0 0.6 0.05
   ```

   Before deploying with real funds, follow [docs/TESTNET.md](docs/TESTNET.md).

## Running the Bot

1. Ensure your `config.toml` is properly set up.
2. Run the bot:
   ```
   cargo run --release
   ```
3. The bot will start, connect to the specified exchange(s), and begin market making based on your configuration.
4. Press `Ctrl+C` to shut down gracefully: all open orders are cancelled before the process exits.

## Project Structure

- `src/`
  - `features/`: Contains market microstructure analysis tools
  - `parameters/`: Handles configuration and parameter management
  - `strategy/`: Implements the market making strategy
  - `trader/`: Manages order generation and execution
  - `backtest.rs`: Offline replay/backtest harness (recording -> simulated trading)
  - `bin/backtest.rs`: CLI entry point for replaying a recording
  - `main.rs`: Entry point of the application
- `skeleton/`
  - `exchanges/`: Exchange clients, websocket subscriptions, and shared-state loading
  - `util/`: Local order book, helpers, logger, candles, and the recorder/replay module

## Making Changes

1. **Modifying the Strategy**:
   - Edit `src/strategy/market_maker.rs` to adjust the core market making logic.
   - Modify `src/trader/quote_gen.rs` to change how orders are generated.
   - Skew weights, deadbands, spread scaling, the microprice anchor, and
     inventory rebalancing are configurable through the `[strategy]` table
     in `config.toml` (defaults in `skeleton/src/util/helpers.rs`,
     `StrategyConfig`) — no code edits needed to tune them.

2. **Adjusting Parameters**:
   - Edit `skeleton/src/util/helpers.rs` (the `Config` struct) to add or modify configurable parameters.
   - Update `config.toml` to reflect any new parameters.

3. **Adding New Features**:
   - Add new files in the relevant directories (e.g., `src/features/` for new market analysis tools).
   - Integrate new features in `src/strategy/market_maker.rs` or `src/trader/quote_gen.rs` as appropriate.

4. **Supporting New Exchanges**:
   - Extend the `OrderManagement` enum in `src/trader/quote_gen.rs`.
   - Implement necessary API calls for the new exchange.

5. **Improving Performance**:
   - Profile the application to identify bottlenecks.
   - Consider optimizing critical paths, especially in order generation and market data processing.

## Key Components

- **MarketMaker**: Main strategy implementation (`src/strategy/market_maker.rs`)
- **QuoteGenerator**: Responsible for order generation (`src/trader/quote_gen.rs`)
- **Engine**: Calculates market microstructure features (`src/features/engine.rs`)
- **Parameters**: Manages configuration and runtime parameters (`src/parameters/parameters.rs`)

## Tests

Pure unit tests run with `cargo test`. Integration tests that need live
exchange connectivity or real credentials are marked `#[ignore]`; run them
explicitly with `cargo test -- --ignored`.

## Contributing

Contributions are welcome! Please follow these steps:

1. Fork the repository
2. Create a new branch for your feature
3. Implement your changes
4. Write or update tests as necessary
5. Submit a pull request with a clear description of your changes

## Disclaimer

This software is for educational and research purposes only. Use it at your own risk. Cryptocurrency trading carries a high level of risk and may not be suitable for all investors. Always thoroughly test any trading bot in a safe, simulated environment (e.g., an exchange testnet) before deploying with real funds.
