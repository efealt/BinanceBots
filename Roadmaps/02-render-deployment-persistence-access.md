# Render Deployment, Persistence, and Private Access Roadmap

## Objective

Deploy BinanceGrid to Render, prove that the application and SQLite data persist correctly, then make the hosted application private behind single-user authentication. This roadmap stops before live Binance order execution.

## Rules

- Start only after the Pre-Render Readiness Roadmap is complete and pushed to `main`.
- Complete phases strictly in order.
- Keep runtime and persistence verification separate from authentication work.
- The production SQLite database must live on a Render persistent disk.
- The hosted application is private by default: pages, static application assets, APIs, WebSockets, diagnostics, historical data, downloader actions, bot state, and future trading controls require authentication.
- The minimal Render health-check endpoint may remain unauthenticated and must expose only basic process health.
- If the selected login/session design requires unauthenticated authentication bootstrap routes, expose only the minimum required to establish a session and no application data.
- Keep credentials, session secrets, Binance API keys, and private account data in server-side environment/secret storage only.
- Exclude live Binance trading implementation from this roadmap.

## Sequential phases

### Phase 1 — Create the Render service

- [x] Connect the GitHub repository `efealt/BinanceGrid` to a Render Web Service.
- [x] Configure the Rust build and start commands appropriate for the repository.
- [x] Configure required environment values, including the Render-provided `PORT` behavior.
- [x] Deploy from `main`.
- [x] Verify the health endpoint and main web pages load successfully from the Render URL.
- [x] Verify Binance public market-data requests/WebSockets operate from the hosted service.

**Exit:** The current application is reachable and functioning from its Render URL without relying on the user's Mac.

### Phase 2 — Attach and use persistent SQLite storage

- [x] Attach a Render persistent disk to the Web Service.
- [x] Choose a stable mount path and configure the application's database-path environment variable to point to the disk.
- [x] Redeploy/restart and verify SQLite initializes and migrations apply on the persistent disk.
- [x] Confirm the application can read and write the production database.

**Verification (2026-09-21):** Render disk mounted at `/var/data` (1 GB); `BINANCE_GRID_DATABASE_PATH=/var/data/binance_grid.sqlite3`; deployment reached `live`; startup completed database initialization/migrations before the server bound; live data endpoints returned HTTP 200 from the production database.

**Exit:** The live Render service is using SQLite on the persistent disk rather than ephemeral service storage.

### Phase 3 — Prove persistence with the downloader

- [x] Use the live Data Downloader to create/download a small, clearly identifiable dataset.
- [x] Verify the downloaded catalog/data is visible through the application.
- [x] Restart or redeploy the Render service without deleting the persistent disk.
- [x] Verify the same downloaded data remains available after the restart/redeploy.
- [x] Confirm local Mac SQLite data and Render SQLite data remain separate databases with the same migration-defined schema.

**Verification (2026-09-21):** Created `Phase 3 Persistence Test` for Spot `BTCUSDT` 1m starting 2026-09-19. After correcting Binance Spot archive microsecond timestamps, the live downloader imported 2,880 one-minute candles from two daily ZIP archives covering 2026-09-19 through 2026-09-20. A controlled Render redeploy completed without deleting the persistent disk. After redeploy, the catalog entry and coverage remained present, a repeat run reported `0 candles from 0 ZIP archives; 2 already present`, and Backtest loaded the persisted data and aggregated it to 48 one-hour candles. Local/default SQLite resolves to `data/binance_grid.sqlite3`; Render overrides the same application with `BINANCE_GRID_DATABASE_PATH=/var/data/binance_grid.sqlite3`. Both use the same migration code while remaining physically separate databases.

**Exit:** A real application write survives a Render redeploy, proving production persistence.

### Phase 4 — Define the full private application boundary

- [x] Replace the earlier public-observer/private-control design with a full private application model.
- [x] Classify all application pages, static application assets, data APIs, market APIs, WebSockets, diagnostics, historical-data views, downloader actions, bot state, and future trading controls as authenticated content.
- [x] Keep only the minimal Render health-check endpoint intentionally public at all times.
- [x] Allow only the minimum unauthenticated authentication bootstrap route(s) if required by the chosen login/session implementation; they must expose no application data.
- [x] Require server-side enforcement rather than browser-only hiding.
- [x] Record the boundary in `AGENTS.md` and `Documents/ARCHITECTURE.md`.

**Exit:** The target access boundary is explicit: BinanceGrid application content is private; unauthenticated access is limited to process health and, only if technically required, authentication bootstrap.

### Phase 5 — Implement single-user full-site authentication

- [ ] Choose and implement the single-user authentication/session mechanism in Rust/Axum.
- [ ] Add the login/session entrypoint required by that mechanism.
- [ ] Protect HTML pages and application static assets server-side.
- [ ] Protect every `/api/data/...` route server-side.
- [ ] Protect every `/api/market/...` route server-side.
- [ ] Protect the Market View WebSocket handshake/server route.
- [ ] Ensure future bot/control/trading endpoints inherit the authenticated boundary by default.
- [ ] Keep credentials and session/signing secrets in Render environment variables or equivalent server-side secret storage, never Git or client JavaScript.
- [ ] Preserve `/api/health` as a minimal unauthenticated Render health check.
- [ ] Verify unauthenticated direct requests to protected pages, APIs, and WebSockets are rejected or redirected to authentication.
- [ ] Verify authenticated access can use Console, Market, Data Downloader, Backtest, APIs, and WebSockets normally.
- [ ] Verify logout/session invalidation removes access to protected content.

**Exit:** The hosted application is usable only after single-user authentication, while Render can still perform its minimal health check.

### Phase 6 — Final hosted-system audit

- [ ] Verify the site remains available after browser refresh/close and does not depend on a local Mac process.
- [ ] Verify the persistent database survives another controlled restart/redeploy.
- [ ] Verify unauthenticated access exposes no application pages, market feeds, historical data, diagnostics, bot state, credentials, private keys, or state-changing controls.
- [ ] Verify the health endpoint exposes only minimal process-health status.
- [ ] Verify protected HTTP endpoints and WebSocket routes remain inaccessible without authentication.
- [ ] Verify authenticated access still works after a fresh deploy and browser login.
- [ ] Record any Render-specific operational settings that future development must preserve.

**Exit:** BinanceGrid is a persistent, remotely accessible, fully private Render deployment ready for subsequent backend bot development.
