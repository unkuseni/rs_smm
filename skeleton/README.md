## SKELETON

   This is a crate for handling multiple exchange data

### SHARED STATE

   The shared state contains the market data, clients and private data.

#### Initializing the shared state
  
  ss::SharedState::new(exchange)

  exchange is &'static str which can be bybit or binance or both


#### Adding clients to the shared state

  The add_clients takes a key, secret, symbol and exchange(Optional: only to be used when exchange type is both) and updates shared state.

  ss::SharedState::add_clients(key, secret, symbol, exchange)


#### Adding symbols to the shared state

  The add_symbols takes a vector of &'_ static str and updates shared state.

  ss::SharedState::add_symbols(symbols)

#### Loading data from the shared state

  The load_data associated function takes in the shared state, an
  unbounded sender, and an optional recording path (websocket deltas are
  written there for offline replay).

  ss::load_data(state, sender, record)

#### CLIENTS AND MARKET DATA

   Market_data returns a struct containing time, books, klines, trades, tickers and liquidations.

   Private_data returns a struct containing time, wallet, orders, positions and executions.

   