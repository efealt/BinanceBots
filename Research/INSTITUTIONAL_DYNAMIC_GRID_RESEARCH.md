# Institutional-Style Dynamic Grid Research

**Status:** Living research knowledge base  
**Last updated:** 2026-09-17  
**Scope:** Adaptive spot or derivatives grid systems, with emphasis on market-making and limit-order-book methods.

> **Research warning:** This document is research, not an approved trading strategy. Public sources do not reveal the proprietary production rules of institutional market makers. Nothing here authorizes live deployment. Every rule must be specified, calibrated, simulated, and approved before it can affect a live account.

## Executive conclusion

There is no single institutional “dynamic grid” standard. The closest institutional analogue is **inventory-aware electronic market making**: continuously quote a ladder of buy and sell prices, move the ladder’s center toward an estimated reservation value, adapt quote distances to volatility and fill probability, skew or remove one side when inventory or order-flow risk becomes unbalanced, and enforce hard limits.

The important distinction is:

> A retail grid reacts to price crossing a fixed ladder. An institutional-style ladder reacts to the state of the market, the state of the account, and the expected quality of the next fill.

The grid itself is not the source of an automatic edge. Net results depend on spread capture, fees, adverse selection, inventory exposure, latency, queue position, and the quality of the estimates used to reprice orders.

## 1. Terminology and model classes

| Model | Center | Spacing | Inventory response | Main risk |
| --- | --- | --- | --- | --- |
| Fixed retail grid | Launch price or fixed midpoint | Fixed arithmetic or geometric levels | Usually none | Price leaves the range and inventory accumulates |
| Boundary-reset dynamic grid | Current price after a boundary break | Often fixed or geometric within the new range | Reset-dependent | Reset can crystallize losses or retain directional exposure |
| Inventory-aware market-making ladder | Midprice or reservation price | State-dependent, often asymmetric | Explicit skew, size limits, and one-sided withdrawal | Model error and adverse selection |
| Delta-neutral grid | Hedge-adjusted fair value | State-dependent across spot and derivative legs | Hedge keeps net delta near target | Basis, funding, margin, liquidation, and execution risk |

The published dynamic-grid paper by Chen, Chen, and Jang uses a geometric spot grid and resets the grid when price breaks its upper or lower boundary. Its data setup is minute candles and an assumption of no more than one transaction per minute, so it is useful as a boundary-reset reference but not as evidence of professional limit-order-book execution. [Dynamic Grid Trading Strategy](https://arxiv.org/abs/2506.11921)

The market-making literature is the stronger foundation. Avellaneda and Stoikov model a dealer that chooses bid and ask quotes while managing inventory risk and uncertain order arrivals. Guéant, Lehalle, and Fernandez-Tapia extend the problem to inventory constraints and closed-form approximations. [Avellaneda–Stoikov](https://doi.org/10.1080/14697680701381228) · [Guéant–Lehalle–Fernandez-Tapia](https://arxiv.org/abs/1105.3115)

## 2. What should happen after each bar or market event?

### The institutional answer is not “only after each bar”

Professional quoting systems are generally event-driven or continuously refreshed. A candle is a useful aggregation for slower volatility and regime features, but a fill, book change, spread change, inventory change, or feed-health event can matter before the next candle closes.

Use three clocks conceptually:

1. **Market clock:** Trades, best bid/ask changes, depth updates, and exchange events.
2. **Decision clock:** The cadence at which the strategy recomputes target quotes. This can be every event, a bounded micro-batch, or a slower bar cadence.
3. **Execution clock:** The stricter rule that decides whether a target change is large enough to cancel and replace an order.

The execution clock needs hysteresis. Repricing on every tiny change destroys queue priority, consumes exchange limits, and can leave the strategy constantly canceling without receiving fills. Avellaneda and Stoikov explicitly discuss the trade-off: the update step must be small enough to model order arrivals but not so small that quotes are updated before orders can reach them. [Paper discussion of quote updates](https://sumubai.cc/pdf/HighFrequencyTradingInALimitOrderbook.pdf)

### Event loop in plain language

~~~text
Market event arrives
    ↓
Update local market state and short-horizon statistics
    ↓
Update inventory and working-order state from account events
    ↓
Estimate fair/reservation price, volatility, fill quality, and risk
    ↓
Construct the desired ladder
    ↓
Compare desired orders with working orders
    ↓
Keep valid orders; cancel/replace only material differences
~~~

A fill is not merely a signal to place the opposite order one level away. It changes inventory, cash, fill-side asymmetry, and the risk of the remaining ladder. The strategy should recalculate the desired state, then decide whether the next order belongs at the old level, a shifted level, or nowhere at all.

## 3. The institutional-style quote model

### 3.1 Midprice and reservation price

Let \(a_t\) be the best ask and \(b_t\) the best bid. The midprice is

\[
m_t = \frac{a_t + b_t}{2}.
\]

Let \(q_t\) be inventory, positive when long; let \(\gamma\) be inventory risk aversion; let \(\sigma_t\) be the relevant price-volatility estimate; and let \(\tau_t\) be the remaining risk horizon. A canonical Avellaneda–Stoikov approximation is

\[
r_t = m_t - q_t\,\gamma\,\sigma_t^2\,\tau_t.
\]

This is an indifference or reservation price, not a price forecast. If the strategy is long, \(q_t>0\), the reservation price moves below the midprice. That makes selling more attractive and buying less attractive, helping the strategy work inventory back toward target. If the strategy is short, the adjustment reverses. [Reservation-price derivation](https://sumubai.cc/pdf/HighFrequencyTradingInALimitOrderbook.pdf)

The formula depends on strong assumptions about price dynamics, utility, horizon, and execution. It should be treated as a structural template and calibrated or simplified for the actual instrument, not copied as a universal constant.

### 3.2 Quote ladder around the reservation price

For level \(i\), define bid and ask distances \(\delta_{t,i}^{b}\) and \(\delta_{t,i}^{a}\):

\[
p_{t,i}^{b} = r_t - \delta_{t,i}^{b},
\qquad
p_{t,i}^{a} = r_t + \delta_{t,i}^{a}.
\]

The distances do not need to be symmetric. A long inventory can use a closer ask ladder and a farther or thinner bid ladder. A short inventory can do the opposite. A directional signal can shift or skew the reservation price, but it should not silently override inventory and risk limits.

A geometric ladder is naturally represented in log-price space:

\[
p_{t,i}^{a} = r_t e^{d_{t,i}^{a}},
\qquad
p_{t,i}^{b} = r_t e^{-d_{t,i}^{b}}.
\]

This keeps spacing proportional to price. Arithmetic spacing is easier to reason about in tick units. The choice is an empirical question, not a matter of branding.

### 3.3 Volatility-adaptive spacing

For log returns \(u_j=\ln(m_j/m_{j-1})\), a simple rolling volatility estimate is

\[
\widehat{\sigma}_t =
\sqrt{\frac{1}{N-1}\sum_{j=1}^{N}
(u_{t-j}-\bar{u}_t)^2}.
\]

The practical design principle is to widen levels when expected movement and adverse-selection risk rise, and tighten them only when expected net economics remain positive. A useful research parameterization is

\[
\delta_{t,i}^{\pm} =
\max\left(
\delta_{\text{tick}},
\delta_{\text{fee}},
c_i S_t\widehat{\sigma}_t\sqrt{h_i},
\delta_{\text{adverse},t}^{\pm}
\right),
\]

where \(S_t\) is the reference price, \(h_i\) is level \(i\)’s intended holding horizon, and \(c_i\) and the adverse-selection terms must be estimated. This is a research heuristic, not a published universal formula.

The important failure mode is using a very narrow fixed grid during a volatility expansion. It increases apparent activity precisely when fills are more likely to be informed or followed by a price move.

### 3.4 Fill probability and quote distance

The market-making model treats execution intensity as a decreasing function of quote distance. A commonly used approximation is

\[
\lambda(\delta)=A e^{-k\delta},
\]

where \(A\) is baseline activity and \(k\) controls how quickly fills decay with distance. The original paper derives this type of relationship from order-size and price-impact assumptions, and emphasizes that the current book and quote distance affect execution priority. [Execution-intensity discussion](https://sumubai.cc/pdf/HighFrequencyTradingInALimitOrderbook.pdf)

For research, every additional level is a trade-off:

~~~text
Closer quote  → higher fill probability, lower spread, higher adverse selection
Farther quote → lower fill probability, higher spread, lower queue participation
~~~

The model should eventually be estimated from observed order arrivals and fills, not selected because a chart looks attractive.

## 4. Order-book information

### 4.1 Best-queue imbalance

Let \(Q_t^b\) and \(Q_t^a\) be the total displayed quantity at the best bid and best ask. A normalized queue imbalance is

\[
I_t = \frac{Q_t^b-Q_t^a}{Q_t^b+Q_t^a}.
\]

Values near \(+1\) indicate a much larger best-bid queue; values near \(-1\) indicate a much larger best-ask queue. Gould and Bonart found a statistically significant relationship between best-queue imbalance and the direction of the next midprice move in ten Nasdaq stocks, with stronger out-of-sample improvement for large-tick stocks. That result does not prove the same effect size in crypto; it establishes why imbalance is a candidate state variable worth testing. [Queue Imbalance paper](https://arxiv.org/abs/1512.03492)

### 4.2 Microprice

A common depth-weighted reference is

\[
\mu_t = \frac{a_t Q_t^b+b_t Q_t^a}{Q_t^b+Q_t^a}.
\]

If bid depth dominates, the microprice moves above the midpoint; if ask depth dominates, it moves below. It can be used as a short-horizon fair-value input or to make one side less aggressive. It should not be used as a standalone directional signal: displayed depth can cancel, refresh, or be spoof-like, and a single book snapshot is not a tradeable forecast.

### 4.3 Multi-level order-flow imbalance

Best-level imbalance ignores cancellations, additions, and deeper book movement. A multi-level order-flow representation can be written as

\[
\mathbf{O}_t =
\left(OFI_{t,1}, OFI_{t,2}, \ldots, OFI_{t,M}\right),
\]

where each component measures net bid/ask queue change at a price level over a chosen event window. Xu, Gould, and Howison found in a six-stock Nasdaq study that deeper levels improved in-sample fit and that ridge regression reduced out-of-sample error relative to ordinary least squares in the presence of correlated level features. [Multi-Level Order-Flow Imbalance](https://arxiv.org/abs/1907.06230)

For a crypto implementation, this implies two requirements: preserve event order when building the local book, and evaluate the feature out of sample with realistic latency and costs.

## 5. How the grid should move

An institutional-style reprice decision has several independent components:

### Center movement

Move the center from \(m_t\) toward \(r_t\), a model-derived fair value, or a hedge-adjusted value. The center should not jump because of one noisy trade.

### Width movement

Change the distance of each level using volatility, spread, expected holding time, and estimated fill/adverse-selection economics.

### Side skew

Change bid and ask distances separately. If inventory is too long, make asks more executable and bids less executable. If inventory is too short, reverse the skew.

### Level count and size

Reduce the number of live levels or their quantities when liquidity, margin, or risk capacity deteriorates. More orders are not automatically better; they increase order-management and cancellation pressure.

### Repricing threshold

Do not replace an order solely because the theoretical price moved by a fraction of a tick. A target update should compete against the cost of losing queue priority, sending another request, and creating a stale-order gap.

### Boundary reset

Resetting the complete grid after a range break is one possible regime response. It is simpler than continuous inventory-aware quoting, but it can hide a directional position and crystallize the strategy’s loss profile. It must be evaluated against a benchmark that marks all inventory continuously, not only after resets.

### Practical fill reaction

After a buy fill:

- Increase inventory and update cash.
- Recompute reservation price and inventory risk.
- Decide whether the corresponding sell should be placed, moved, reduced, or suppressed.
- Reconcile all existing orders before sending replacements.

After a sell fill, apply the symmetric logic. This is the key difference between “one order up, one order down” grid logic and a state-aware market-making engine.

## 6. Economics and costs

For a buy at \(p_b\) and a sell at \(p_a\), with quote-denominated fee rates \(f_b\) and \(f_a\), a simplified round-trip result per unit is

\[
\Pi_{\text{round trip}}
=p_a(1-f_a)-p_b(1+f_b).
\]

The cycle is not attractive merely because \(p_a>p_b\). It must also cover expected adverse selection, price impact, latency, hedging, and any funding or borrow cost:

\[
\mathbb{E}\left[\Pi_{\text{net}}\right]
>
\text{fees}
+\text{adverse-selection cost}
+\text{execution cost}
+\text{hedge cost}.
\]

The exact fee accounting depends on whether the exchange charges in base or quote asset and whether the account receives maker rebates. The bot must use the actual account fee schedule rather than a hard-coded assumption.

The published dynamic-grid study reports that smaller grid sizes become more sensitive to fees and that its results are favorable to a period with substantial crypto appreciation. Those results should be read as a warning against evaluating grid density without a fee and exposure decomposition. [Dynamic-grid study and fee discussion](https://arxiv.org/abs/2506.11921)

## 7. Spot, perpetuals, and delta neutrality

### Spot

A spot grid can buy lower and sell higher, but a sustained decline consumes quote currency and accumulates the base asset. It cannot naturally short the asset. The risk is therefore real inventory exposure, not just a temporary accounting detail.

### Perpetual futures

A futures leg can short or hedge spot inventory, but it introduces funding, basis, margin, liquidation, and contract-specific risks. A “delta-neutral” label does not make the strategy risk-free; it changes the risk set.

### Cross-leg hedge

A hedge should be sized from measured beta and execution quality, not assumed one-for-one. The hedge itself can be delayed, partially filled, or more expensive exactly when the spot grid is under pressure.

The correct comparison is not only spot-grid P&L versus hedged P&L. Compare:

\[
\text{net return},\quad
\text{inventory exposure},\quad
\text{margin usage},\quad
\text{tail loss},\quad
\text{funding/basis cost},\quad
\text{execution shortfall}.
\]

## 8. Exchange and execution mechanics

The strategy must treat the exchange as a stateful matching engine, not a passive price feed.

### Market data

Binance provides kline streams in UTC, partial book-depth streams, and diff-depth streams used to maintain a local order book. The documented diff-depth stream can update at 100 ms or 1,000 ms, depending on the selected stream. [Binance Spot WebSocket Streams](https://developers.binance.com/docs/binance-spot-api-docs/web-socket-streams)

### Account state

Binance user-data streams push account events in real time. Order state arrives through execution reports, including new, canceled, replaced, rejected, traded, expired, and trade-prevention states. A live strategy must use these events as the authoritative feed for its own order and fill state. [Binance Spot User Data Streams](https://developers.binance.com/docs/binance-spot-api-docs/user-data-stream)

### Symbol filters

Before placing or repricing an order, the strategy must apply the current symbol rules. Binance documents filters for:

- **PRICE_FILTER:** Valid price range and tick size.
- **LOT_SIZE:** Valid quantity range and step size.
- **MIN_NOTIONAL or NOTIONAL:** Minimum or bounded order value.
- Open-order, order-amendment, and exchange-wide order limits.

These are not UI validation details; violating them creates rejected orders and can break the intended ladder. [Binance Spot Filters](https://developers.binance.com/docs/binance-spot-api-docs/filters)

### Rate limits and uncertain responses

Binance documents HTTP 429 for rate-limit violations and 418 for an IP ban after continued violations. It also warns that a 5XX response can leave execution status unknown. A cancel/replace operation can partially succeed. Therefore the order manager needs backoff, idempotent client IDs, reconciliation, and explicit handling for unknown state. [Binance Spot REST API information](https://developers.binance.com/docs/binance-spot-api-docs/rest-api/general-api-information)

## 9. Data required for a credible backtest

Candles alone are adequate only for a deliberately slow bar-based strategy with conservative assumptions. They are not enough to estimate queue position or realistic limit-order fills.

### Minimum bar-based dataset

- Open, high, low, close, and volume.
- Exchange timestamps in UTC.
- Fees and whether the assumed order is maker or taker.
- A fill rule that does not use future candle information.
- Spread and slippage assumptions.

### Required for order-book-aware research

- Trade or aggregate-trade events.
- Best bid/ask updates.
- Diff-depth events and an initial depth snapshot.
- Sequence IDs and gap detection.
- Local receive time and exchange event time.
- The strategy’s order submission, acknowledgment, cancel, replacement, and fill events.
- Partial-fill quantities and queue/latency assumptions.
- Exact symbol filters and account fee schedule for the test period.

### Backtest integrity tests

The simulator should explicitly test:

1. No look-ahead from candle high/low or future book state.
2. Partial fills and order-priority assumptions.
3. Latency between market event, decision, request, acknowledgment, and fill.
4. Cancel/replace failure and duplicate-event handling.
5. Feed gaps and local-book sequence breaks.
6. Fee, spread, and slippage sensitivity.
7. Sustained trend, volatility shock, liquidity collapse, and exchange outage scenarios.

## 10. Metrics that reveal what is actually working

Do not judge a dynamic grid by total P&L alone. Separate the return sources:

- Gross spread captured per completed round trip.
- Net spread after fees and rebates.
- Adverse selection after each fill.
- Fill probability by level and side.
- Average quote lifetime and cancellation rate.
- Queue-time and latency distribution.
- Inventory mean, volatility, maximum, and time outside target bands.
- Mark-to-market drawdown, not only realized grid profit.
- Turnover, capital utilization, and order-to-trade ratio.
- Funding, hedge, basis, or borrow costs where relevant.
- P&L by volatility and trend regime.

The central diagnostic is whether the strategy earns from repeated favorable execution or simply carries a long asset during a rising market.

## 11. Research directions for this project

These are research directions, not approved live rules:

### Direction A: Adaptive spot ladder

Start with candles and best bid/ask. Use volatility for spacing, an inventory target for skew, fee-aware minimum distance, and a hard maximum inventory. This is the simplest meaningful upgrade from a static grid.

### Direction B: Order-book-aware ladder

Add local depth, queue imbalance, microprice, and order-flow features. Use them to adjust quote aggressiveness and to withdraw from the side most exposed to adverse selection. Validate each feature out of sample.

### Direction C: Event-driven simulator

Before trusting L2 signals, model partial fills, queue position, message latency, cancel/replace behavior, and book gaps. Without this, a sophisticated signal can produce an unrealistic backtest.

### Direction D: Optional hedge overlay

Only after spot inventory economics are understood, evaluate a derivative hedge. Treat funding, basis, margin, and hedge execution as first-class P&L and risk terms.

## 12. Findings and non-findings

### Supported by the research

- Inventory should change the center and/or asymmetry of quotes.
- Quote distance affects fill probability.
- Volatility and order-flow conditions should affect quote width and aggressiveness.
- Order-book imbalance is a plausible short-horizon state variable, but its strength is instrument- and regime-dependent.
- Fees, queue position, latency, and partial fills can dominate a narrow-grid result.
- Live order state must be event-driven and reconciled with the exchange.

### Not established by the research

- That a fixed number of grid levels is optimal.
- That Bollinger Bands, moving averages, or any single chart indicator is sufficient for quote placement.
- That a dynamic reset automatically improves risk-adjusted returns.
- That a profitable candle backtest survives realistic order-book execution.
- That “delta neutral” means low total risk.
- That a formula from a market-making paper can be deployed without calibration.

## Sources

1. Avellaneda, M. and Stoikov, S., [High-frequency trading in a limit order book](https://doi.org/10.1080/14697680701381228), *Quantitative Finance*, 2008.
2. Guéant, O., Lehalle, C.-A., and Fernandez-Tapia, J., [Dealing with the Inventory Risk](https://arxiv.org/abs/1105.3115), 2011/2012.
3. Gould, M. D. and Bonart, J., [Queue Imbalance as a One-Tick-Ahead Price Predictor in a Limit Order Book](https://arxiv.org/abs/1512.03492), 2015.
4. Xu, K., Gould, M. D., and Howison, M. D., [Multi-Level Order-Flow Imbalance in a Limit Order Book](https://arxiv.org/abs/1907.06230), 2019.
5. Chen, K.-Y., Chen, K.-H., and Jang, J.-S. R., [Dynamic Grid Trading Strategy: From Zero Expectation to Market Outperformance](https://arxiv.org/abs/2506.11921), 2025.
6. Binance, [Spot WebSocket Market Streams](https://developers.binance.com/docs/binance-spot-api-docs/web-socket-streams).
7. Binance, [Spot User Data Streams](https://developers.binance.com/docs/binance-spot-api-docs/user-data-stream).
8. Binance, [Spot Symbol and Exchange Filters](https://developers.binance.com/docs/binance-spot-api-docs/filters).
9. Binance, [Spot REST API General Information](https://developers.binance.com/docs/binance-spot-api-docs/rest-api/general-api-information).
