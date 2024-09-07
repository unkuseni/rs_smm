- - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - 
## Notes about Features
- - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - 


### List of Features

- Price Impact of trades
- Time Decay
- Order flow Imbalance Volume
- Mid price changes
- Order Imbalance Ratio
- Order Imbalance Ratio using exponential weights for decay or geometric weights at depth
- Imbalance bins: use odd numbers for even bins
- Average Trade Price
- Mid Price Basis
- Local Volatility
- Expected Value due to Imbalance
- Weighted Midprice
- Stacked Imbalances
- EMA and Weighted EMA
- Book tilting
- Price fluctuation or Volatility
- Trade Classifier
- Mean Reversion of Midprice


For the regression, lagged and instantaneous order imbalances are fed into the dataset at a set tick interval. it is then used to predict mid price changes for the forecast window.