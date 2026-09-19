# BinanceGrid Live Trading Readiness Review

Date: 2026-09-19  
Scope: one private user, multiple live/paper bots, backend runtimes that continue running when the browser is refreshed or closed.

## Bottom line

The project already has a good Rust foundation for market data, historical data, research/backtesting, SQLite storage, and a browser UI. It is **not live-account ready yet**, because the actual bot lifecycle and authenticated execution layer do not exist.

The important point is that the project does **not** need a rewrite. The current architecture can be extended into the intended system.

## What exists now

### Backend

- Rust + Tokio + Axum.
- Public Binance Spot and USD-M perpetual market-data support.
- REST bootstrap of the latest candles.
- Binance WebSocket feeds for candles, best bid/ask, partial depth, and trades.
- Automatic WebSocket reconnect loop.
- MarketService keeps feeds in backend memory and starts them with tokio::spawn.
- Axum provides REST and WebSocket endpoints to the browser.
- Static HTML/CSS/JavaScript UI is served by the Rust process.

This is already the correct general pattern for the user's requirement that work continue in the backend independently of the browser. The existing market feeds are an example of that pattern: once created, the Rust backend owns the feed task. The missing part is applying the same ownership model to trading bots.

### Storage

- SQLite is already implemented.
- WAL mode, foreign keys, and busy timeouts are configured.
- Database files are ignored by Git.
- A migration system already exists through schema_migrations.
- 001_initial.sql and 002_download_start_date.sql are automatically applied from Rust.
- The schema already contains substantial historical-data and live-capture infrastructure.

This means future bot/order tables can be added with normal numbered migrations rather than manually modifying the production database.

### Research and UI

- Historical Binance archive downloader exists.
- Historical OHLCV storage exists.
- Backtest/research UI exists.
- Market View exists.
- Console UI already visually anticipates multiple bots.

## What the Console actually does today

The current bot Console is only a browser-side preview.

web/app.js currently stores bots in:

~~~text
const bots = [];
~~~

Creating a bot only adds an object to that JavaScript array. Therefore:

- refreshing the page loses the bot;
- closing the browser loses the bot;
- no Rust bot runtime is created;
- no strategy is running;
- no exchange connection is created for the bot;
- no order can be submitted.

The UI itself explicitly labels this as a local preview, so there is no hidden live-trading implementation that needs to be salvaged.

## Required backend shape

For the intended product the ownership should be:

~~~text
Browser UI
    ↓ commands / snapshots
Rust API
    ↓
Bot Manager
    ├── Bot A runtime
    ├── Bot B runtime
    ├── Bot C runtime
    └── ...
          ↓
Shared market-data services
          ↓
Strategy / risk / execution
          ↓
Binance
~~~

The browser must never own a running bot. It should only create/start/stop/configure bots and display backend state.

Refreshing or closing the page must have no effect on a running bot.

## Major pieces still missing

### 1. Backend Bot Manager

There is currently no backend bot registry or runtime.

Needed:

- unique bot ID;
- bot name;
- market type;
- symbol;
- strategy type and parameters;
- paper/live mode;
- lifecycle state such as stopped, starting, running, stopping, error;
- independent asynchronous runtime per running bot;
- API endpoints to create, start, stop, inspect, and later delete bots;
- backend snapshots for the Console.

Multiple bots should be able to run simultaneously without depending on the browser.

### 2. Persistent bot definitions

Bot definitions and desired state need to live in SQLite.

At minimum the database must remember enough information to answer:

- which bots exist;
- their parameters;
- whether each bot was intended to be running;
- paper or live mode;
- symbol/market;
- strategy configuration.

The hot trading loop can remain in memory. SQLite does not need to sit in the decision hot path.

After a Render restart, the service should reload bot definitions and recover the intended runtimes rather than starting with an empty Console.

### 3. Authenticated Binance trading client

The current Binance client is public-market-data only.

There is currently no implementation for:

- API key/secret configuration;
- signed authenticated requests;
- account/balance/position retrieval;
- placing orders;
- cancelling orders;
- reading open orders;
- exchange order IDs / client order IDs;
- private order and fill updates;
- reconnect/recovery of the authenticated stream.

Secrets must remain in environment variables and never enter Git or the browser.

### 4. Execution layer

The architecture document describes live and paper executors, but they are not implemented yet.

We need one strategy path with two execution implementations:

~~~text
strategy decision
      ↓
execution interface
   ↙       ↘
paper     live Binance
~~~

This prevents the paper bot and live bot from slowly becoming two different strategies.

### 5. Grid strategy runtime

There is currently no actual grid-bot engine.

The engine will need to own its grid state, inventory/position state, intended orders, fills, and next decisions independently for every bot.

The exact trading rules remain a finance/strategy decision and should not be invented by the software layer.

### 6. Live-account safety layer

Because this is a real-money system, execution needs explicit safeguards rather than relying only on strategy logic.

Required before live activation:

- validate Binance tick size, step size, minimum quantity and minimum notional;
- prevent duplicate order submission;
- idempotent client order identifiers;
- explicit maximum exposure / allowed capital per bot;
- controlled start and stop behavior;
- emergency stop / cancel behavior;
- clear handling of rejected, partially filled, cancelled, expired, and unknown orders;
- stale-market-data protection;
- API/WebSocket disconnect handling;
- no silent failure that leaves UI state different from exchange state.

### 7. Restart and reconciliation

This is one of the most important production requirements.

A Render restart, deploy, crash, or temporary Binance disconnect must not make the software assume that previous orders disappeared.

On startup/reconnect:

1. load persisted bot definitions;
2. connect to Binance;
3. query actual balances/positions/open orders;
4. reconcile exchange state with each bot;
5. rebuild the in-memory runtime;
6. only then allow normal trading to continue.

For live trading, Binance is the authority for actual orders, fills, balances, and positions.

### 8. Order/fill audit history

The current database is heavily developed for market/research data but does not contain the bot/order/fill journal needed for live trading.

We should persist the trading audit trail asynchronously:

- bot lifecycle events;
- submitted order intent;
- exchange order acknowledgement;
- order status changes;
- fills;
- cancellations/rejections;
- strategy decisions important enough to explain why an order was sent.

This should not block the trading hot path.

### 9. Tests

The repository currently has only a very small test footprint: the detected Rust tests are in the SQLite storage/schema area.

Before live trading, the critical behavior needs automated tests around:

- grid calculations;
- quantity/price rounding;
- risk limits;
- order state transitions;
- duplicate-event handling;
- partial fills;
- reconnect/reconciliation;
- restart recovery;
- paper execution;
- mocked Binance failures.

The live account should not be the test environment.

## Render readiness

Several small deployment changes are required before the current app itself is Render-ready.

Current code binds to:

~~~text
127.0.0.1:8080
~~~

For Render it should use the supplied port and bind externally, effectively:

~~~text
0.0.0.0:$PORT
~~~

The SQLite location is also currently hardcoded as:

~~~text
data/binance_grid.sqlite3
~~~

It should become configuration from an environment variable so the Render production database can live on the persistent-disk mount.

The production URL also needs access protection. Single-user does not mean authentication is unnecessary: a live-trading control panel exposed on the public internet must not be open to anyone who discovers the URL.

For the intended one-service deployment, SQLite on a Render persistent disk is appropriate. There is no current need to introduce PostgreSQL merely because the application is deployed.

## Migration readiness

The project already has the right basic migration concept and automatically runs migrations during storage initialization.

Before live trading, I would harden the migration runner so each schema migration is atomic: either the migration and its recorded version both succeed, or neither does. Then new live-trading tables can be introduced as normal numbered migrations.

The local Mac database and Render database will contain different data, but both can reach the same schema version by running the same migrations.

## Logging and observability

There is currently a basic health endpoint, but the production trading layer will need structured logging.

The architecture mentions tracing, but it is not currently present in Cargo.toml.

At minimum we need enough logs to answer:

- which bots are running;
- whether market/private feeds are connected;
- when an order was requested;
- Binance acknowledgement/rejection;
- fills/cancellations;
- reconciliation results;
- fatal runtime errors.

The dashboard can later expose the useful subset, while Render logs remain the operational record.

## Recommended implementation order

1. Make configuration deployment-safe: Render bind/port, database path, secrets, access protection.
2. Harden migrations and add persistent bot tables.
3. Build the backend Bot Manager and replace the frontend-only bot array with backend bot APIs/snapshots.
4. Add paper execution and the actual grid runtime.
5. Add authenticated Binance account/order infrastructure.
6. Add live execution with the safety controls.
7. Add restart/reconnect reconciliation and asynchronous audit history.
8. Exercise the complete system in paper mode and mocked failure tests.
9. Enable live mode only after the same lifecycle has been proven without real orders.

## Assessment

The codebase is **a useful foundation rather than a prototype that should be discarded**.

The strongest existing pieces are the Rust async foundation, Binance public market-data layer, reconnecting feeds, SQLite/migrations, historical data pipeline, and separation between backend data processing and browser presentation.

The main gap is very clear: **the project currently stops at market data/research/UI. The live bot-management and execution layer has not been built yet.**

That is a good point to be at before deployment because we can now build the live architecture deliberately instead of having to untangle an already-running trading implementation.
