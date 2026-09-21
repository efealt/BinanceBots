# Pre-Render Readiness Roadmap

## Objective

Prepare BinanceGrid for its first Render deployment without changing trading strategy behavior or adding live Binance execution. The finished state is a clean GitHub `main` branch ready to hand to Render. The user's Mac remains available as an optional research/local-compute environment but is not a required deployment gate.

## Rules

- Complete phases strictly in order.
- Keep each phase small enough to implement and verify independently.
- Follow `AGENTS.md` and keep `Documents/ARCHITECTURE.md` synchronized with agreed architectural decisions.
- Preserve current local behavior unless an intentional deployment-related change is documented and verified.
- Do not add Binance private API trading, bot execution, ML, PostgreSQL, Redis, Docker, or unrelated infrastructure.
- Do not expose secrets in code, Git, logs, or the browser.

## Sequential phases

### Phase 1 — Record the agreed deployment architecture

- [x] Update `Documents/ARCHITECTURE.md` to record Render as the intended always-on deployment target.
- [x] Record that production SQLite lives on a Render persistent disk while local development keeps a separate local SQLite database.
- [x] Record that backend bot runtimes must remain independent of the browser.
- [x] Record the long-term access model: public/read-only diagnostic observer surface and authenticated control/trading surface.
- [x] State explicitly that observer access must never expose secrets or permit state-changing/trading actions.

**Exit:** The architecture document accurately reflects the deployment and access decisions agreed before Render.

### Phase 2 — Update repository operating rules

- [x] Update `AGENTS.md` only with rules future agents need to preserve the observer/control separation and production deployment constraints.
- [x] Keep implementation details in architecture/code rather than turning `AGENTS.md` into a design document.
- [x] Verify the updated rules do not conflict with the existing single-user, live-account, production-quality requirements.

**Exit:** Future coding work has concise instructions that preserve the agreed security and diagnostic boundaries.

### Phase 3 — Make the Rust application Render-ready

- [x] Replace the hardcoded `127.0.0.1:8080` server binding with deployment-safe configuration that uses Render's `PORT` and binds externally while preserving a sensible local default.
- [x] Make the SQLite database path configurable by environment variable while preserving `data/binance_grid.sqlite3` as the local default.
- [x] Ensure the configured database parent directory is created when needed.
- [x] Harden schema migrations so each migration and its version record succeed atomically or roll back together.
- [x] Verify the existing health endpoint and static UI still work with the configuration changes.
- [x] Keep database files and environment/secrets files ignored by Git.

**Exit:** The same codebase can run locally and on Render using environment configuration, and a failed migration cannot leave a partially recorded schema update.

### Phase 4 — Confirm the GitHub handoff to Render

- [x] Confirm GitHub `main` contains the completed Phase 1–3 architecture, operating rules, and Render-readiness code.
- [x] Confirm no deployment secret, database file, or machine-specific production path is committed.
- [x] Confirm `Documents/ARCHITECTURE.md` describes the actual operating model: ChatGPT Chat + GitHub as the primary coding path, GitHub `main` as source of truth, and Render as the primary runtime environment.
- [x] Confirm the repository is ready for Render to perform the first real hosted build and runtime verification.
- [x] Do not require the user's Mac or a local build as the exit condition for this phase; hosted build/runtime validation begins in the Render deployment roadmap.

**Exit:** GitHub `main` is the agreed Render-ready source of truth and Roadmap 2 can begin with the first hosted build.
