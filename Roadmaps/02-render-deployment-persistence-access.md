# Render Deployment, Persistence, and Access Roadmap

## Objective

Deploy BinanceGrid to Render, prove that the application and SQLite data persist correctly, then establish the read-only diagnostic versus authenticated control boundary. This roadmap stops before live Binance order execution.

## Rules

- Start only after the Pre-Render Readiness Roadmap is complete and pushed to `main`.
- Complete phases strictly in order.
- Keep the first deployment focused on proving runtime and persistence before adding access-control complexity.
- The production SQLite database must live on a Render persistent disk.
- Public access may expose only non-sensitive read-only diagnostics.
- Any action that mutates bot/application state or can eventually affect trading must require authentication.
- Never place Binance API secrets, credentials, or private account data in public observer output.
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

**Verification (2026-09-21):** Render disk mounted at `/var/data` (1 GB); `BINANCE_GRID_DATABASE_PATH=/var/data/binance_grid.sqlite3`; redeploy `dep-daoodnbbc2fs73889br0` reached `live`; startup completed database initialization/migrations before the server bound; live GETs to `/api/data/downloads` and `/api/data/datasets` returned HTTP 200 from the production database.\n\n**Exit:** The live Render service is using SQLite on the persistent disk rather than ephemeral service storage.

### Phase 3 — Prove persistence with the downloader

- [ ] Use the live Data Downloader to create/download a small, clearly identifiable dataset.
- [ ] Verify the downloaded catalog/data is visible through the application.
- [ ] Restart or redeploy the Render service without deleting the persistent disk.
- [ ] Verify the same downloaded data remains available after the restart/redeploy.
- [ ] Confirm local Mac SQLite data and Render SQLite data remain separate databases with the same migration-defined schema.

**Exit:** A real application write survives a Render restart/redeploy, proving production persistence.

### Phase 4 — Establish the observer versus control boundary

- [ ] Define the exact read-only diagnostic information that may be available without login.
- [ ] Keep health, runtime status, market-feed status, bot status, grid/strategy diagnostic state, and safe logs available only where they do not reveal secrets or permit mutations.
- [ ] Ensure all state-changing endpoints/actions are classified as control operations.
- [ ] Ensure no observer route can create, start, stop, modify, delete, place, cancel, or otherwise mutate application/trading state.
- [ ] Update `Documents/ARCHITECTURE.md` if the concrete route/access design adds details beyond the already agreed boundary.

**Exit:** The application has an explicit, testable separation between safe observation and privileged control.

### Phase 5 — Add authentication for control access

- [ ] Add single-user authentication suitable for the private control surface.
- [ ] Protect every control/mutation endpoint server-side; hiding buttons in the browser is not sufficient.
- [ ] Keep credentials/secrets in environment variables or another appropriate server-side secret mechanism, never in Git or client JavaScript.
- [ ] Verify unauthenticated users can access only the intended read-only observer surface.
- [ ] Verify authenticated access can reach the intended private control surface.
- [ ] Verify direct unauthenticated requests to protected mutation endpoints are rejected.

**Exit:** The hosted application can be safely inspected through its read-only surface while all present and future trading controls remain behind server-enforced authentication.

### Phase 6 — Final hosted-system audit

- [ ] Verify the site remains available after browser refresh/close and does not depend on a local Mac process.
- [ ] Verify the persistent database survives another controlled restart/redeploy.
- [ ] Verify observer pages expose no API secrets, credentials, private keys, or state-changing controls.
- [ ] Verify protected endpoints remain inaccessible without authentication.
- [ ] Record any Render-specific operational settings that future development must preserve.

**Exit:** BinanceGrid is a persistent, remotely inspectable, access-controlled Render deployment ready for subsequent backend bot development.
