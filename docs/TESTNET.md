# Testnet Runbook

Never deploy RS_SMM with real funds before completing these steps.

## 1. Testnet accounts

- **Bybit testnet**: https://testnet.bybit.com - create an account, generate
  API keys under the testnet API management page.
- **Binance testnet**: https://testnet.binancefuture.com - generate API keys
  there; the bot's Binance client must be pointed at the testnet base URL.

## 2. Recommended first config

    exchange = "bybit"
    symbols = ["BTCUSDT"]
    balances = [["BTCUSDT", 50.0]]
    leverage = 2.0
    orders_per_side = 3
    final_order_distance = 5
    depths = [3, 8, 34]
    rate_limit = 10
    tick_window = 8000

    [bps]
    BTCUSDT = 15

    [strategy]
    # Disable aggressive behavior during validation.
    rebalance_threshold = 0          # no market orders yet
    max_drawdown_frac = 0.05         # kill switch at 5% drawdown
    max_portfolio_delta = 0.6        # halt if aggregate inventory passes 0.6

## 3. Dry-run procedure

1. Run with record = "testnet.rec" and let it quote for a few hours with
   rebalance_threshold = 0.
2. Stop cleanly (Ctrl+C) and verify: orders cancelled, state.json written
   (if configured), and the stats lines per symbol (fills, adverse_ratio).
3. Replay and A/B test:
       cargo run --release --bin backtest -- testnet.rec
       cargo run --release --bin backtest -- testnet.rec --sweep vol_spread_scaling 0 800 100
4. Watch the **adverse_ratio** in the shutdown stats: sustained values well
   below 1.0 mean you are being picked off and the spread should be widened.
5. Enable rebalance_threshold = 0.45 only after grid behavior is verified.

## 4. Checklist before any mainnet deployment

- [ ] Testnet run with zero unexpected errors in logs
- [ ] Shutdown leaves no open orders on the exchange (check manually)
- [ ] state.json roundtrip: restart restores position and cancels/refreshes
      the grid correctly
- [ ] Risk limits tested: set max_drawdown_frac = 0.001 and verify the bot
      halts and cancels everything
- [ ] Config hot-reload works: edit config.toml while running and confirm
      the "Config hot-reloaded" log
- [ ] API keys stored via env: indirection, never in the repo
- [ ] Small position budget (balances), low leverage

## 5. Emergency procedures

- Stop: Ctrl+C cancels all orders and saves state.
- If the process is killed without cleanup: log into the exchange and cancel
  all open orders manually; the state file plus sync_position will
  reconcile the position on the next start.
- The risk kill switch (drawdown / portfolio limits) cancels everything and
  stops quoting, but the process keeps running for inspection.
