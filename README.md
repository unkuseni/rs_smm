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
- An account with supported exchanges (currently Bybit and Binance)
- API keys for the exchanges you plan to use

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

1. Create a `config.toml` file in the project root directory.
2. Add your configuration settings. Here's a template:

   ```toml
   exchange = "bybit"  # or "binance"
   symbols = ["BTCUSDT", "ETHUSDT"]
   leverage = 10
   orders_per_side = 5
   final_order_distance = 0.01
   depths = [5, 50]
   rate_limit = 100
   tick_window = 6000 // 1 mins
   bps = [0.01, 0.02]  # Basis points for spread

   [[api_keys]]
   key = "your_api_key"
   secret = "your_api_secret"
   symbol = "BTCUSDT"

   [[balances]]
   symbol = "BTCUSDT"
   amount = 1000.0
   ```

3. Adjust the values according to your trading strategy and risk tolerance.

## Running the Bot

1. Ensure your `config.toml` is properly set up.
2. Run the bot:
   ```
   cargo run --release
   ```
3. The bot will start, connect to the specified exchange(s), and begin market making based on your configuration.

## Project Structure

- `src/`
  - `features/`: Contains market microstructure analysis tools
  - `parameters/`: Handles configuration and parameter management
  - `strategy/`: Implements the market making strategy
  - `trader/`: Manages order generation and execution
  - `main.rs`: Entry point of the application

## Making Changes

1. **Modifying the Strategy**:
   - Edit `src/strategy/market_maker.rs` to adjust the core market making logic.
   - Modify `src/trader/quote_gen.rs` to change how orders are generated.

2. **Adjusting Parameters**:
   - Edit `src/parameters/parameters.rs` to add or modify configurable parameters.
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

## Contributing

Contributions are welcome! Please follow these steps:

1. Fork the repository
2. Create a new branch for your feature
3. Implement your changes
4. Write or update tests as necessary
5. Submit a pull request with a clear description of your changes

## Disclaimer

This software is for educational and research purposes only. Use it at your own risk. Cryptocurrency trading carries a high level of risk and may not be suitable for all investors. Always thoroughly test any trading bot in a safe, simulated environment before deploying with real funds.
