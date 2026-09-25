#!/usr/bin/env python3
"""One-off builder for the frozen full XAGUSDT 1m regression SQLite fixture.

This script intentionally downloads Binance public archives only when invoked manually
or by the dedicated one-off fixture workflow. Ordinary CI never calls it.
"""

from __future__ import annotations

import csv
import hashlib
import io
import json
import sqlite3
import sys
import urllib.request
import zipfile
from datetime import datetime, timedelta, timezone
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
OUT_DIR = ROOT / "test-data"
DB_PATH = OUT_DIR / "xagusdt_1m_2026.sqlite3"
MANIFEST_PATH = OUT_DIR / "xagusdt_1m_2026.manifest.json"

SYMBOL = "XAGUSDT"
MARKET_TYPE = "usd_m_perpetual"
INTERVAL = "1m"
START = datetime(2026, 1, 7, 10, 0, tzinfo=timezone.utc)
END = datetime(2026, 9, 20, 23, 59, tzinfo=timezone.utc)
START_MS = int(START.timestamp() * 1000)
END_OPEN_MS = int(END.timestamp() * 1000)
END_CLOSE_MS = END_OPEN_MS + 59_999
EXPECTED_ROWS = 369_480
FIXED_METADATA_TIME_MS = END_CLOSE_MS

MIGRATIONS = [
    (1, ROOT / "migrations" / "001_initial.sql"),
    (2, ROOT / "migrations" / "002_download_start_date.sql"),
    (3, ROOT / "migrations" / "003_auth_audit.sql"),
    (4, ROOT / "migrations" / "004_trading_runs.sql"),
]


def normalize_ms(value: str) -> int:
    raw = int(value)
    return raw // 1000 if raw >= 100_000_000_000_000 else raw


def archive_urls() -> list[str]:
    urls: list[str] = []
    for month in range(1, 9):
        ym = f"2026-{month:02d}"
        urls.append(
            f"https://data.binance.vision/data/futures/um/monthly/klines/{SYMBOL}/{INTERVAL}/"
            f"{SYMBOL}-{INTERVAL}-{ym}.zip"
        )
    day = datetime(2026, 9, 1, tzinfo=timezone.utc)
    stop = datetime(2026, 9, 20, tzinfo=timezone.utc)
    while day <= stop:
        ymd = day.strftime("%Y-%m-%d")
        urls.append(
            f"https://data.binance.vision/data/futures/um/daily/klines/{SYMBOL}/{INTERVAL}/"
            f"{SYMBOL}-{INTERVAL}-{ymd}.zip"
        )
        day += timedelta(days=1)
    return urls


def fetch_archive(url: str) -> tuple[bytes, str]:
    print(f"download {url}", flush=True)
    request = urllib.request.Request(url, headers={"User-Agent": "BinanceBots regression fixture builder"})
    with urllib.request.urlopen(request, timeout=90) as response:
        payload = response.read()
    return payload, hashlib.sha256(payload).hexdigest()


def parse_archive(payload: bytes) -> list[tuple]:
    with zipfile.ZipFile(io.BytesIO(payload)) as archive:
        names = [name for name in archive.namelist() if name.lower().endswith(".csv")]
        if len(names) != 1:
            raise RuntimeError(f"expected one CSV in archive, found {names}")
        with archive.open(names[0]) as raw:
            text = io.TextIOWrapper(raw, encoding="utf-8")
            reader = csv.reader(text)
            rows: list[tuple] = []
            for row in reader:
                if not row:
                    continue
                try:
                    open_time_ms = normalize_ms(row[0])
                except ValueError:
                    continue
                close_time_ms = normalize_ms(row[6])
                open_price = float(row[1])
                high_price = float(row[2])
                low_price = float(row[3])
                close_price = float(row[4])
                base_volume = float(row[5])
                quote_volume = float(row[7])
                trade_count = int(row[8])
                taker_buy_base_volume = float(row[9])
                taker_buy_quote_volume = float(row[10])
                if open_time_ms < START_MS or open_time_ms > END_OPEN_MS:
                    continue
                rows.append(
                    (
                        1,
                        open_time_ms,
                        close_time_ms,
                        open_price,
                        high_price,
                        low_price,
                        close_price,
                        base_volume,
                        quote_volume,
                        trade_count,
                        taker_buy_base_volume,
                        taker_buy_quote_volume,
                    )
                )
            return rows


def apply_schema(connection: sqlite3.Connection) -> None:
    connection.execute(
        "CREATE TABLE IF NOT EXISTS schema_migrations ("
        "version INTEGER PRIMARY KEY, applied_at_ms INTEGER NOT NULL)"
    )
    for version, path in MIGRATIONS:
        connection.executescript(path.read_text(encoding="utf-8"))
        connection.execute(
            "INSERT OR REPLACE INTO schema_migrations(version, applied_at_ms) VALUES (?, ?)",
            (version, FIXED_METADATA_TIME_MS),
        )
    connection.commit()


def validate_rows(rows: list[tuple]) -> None:
    rows.sort(key=lambda row: row[1])
    if len(rows) != EXPECTED_ROWS:
        raise RuntimeError(f"expected {EXPECTED_ROWS} candles, got {len(rows)}")
    opens = [row[1] for row in rows]
    if len(set(opens)) != EXPECTED_ROWS:
        raise RuntimeError("duplicate candle open timestamps detected")
    if opens[0] != START_MS:
        raise RuntimeError(f"unexpected first open: {opens[0]} != {START_MS}")
    if opens[-1] != END_OPEN_MS:
        raise RuntimeError(f"unexpected last open: {opens[-1]} != {END_OPEN_MS}")
    for previous, current in zip(opens, opens[1:]):
        if current - previous != 60_000:
            raise RuntimeError(f"non-contiguous 1m candles: {previous} -> {current}")
    for row in rows:
        _, open_ms, close_ms, op, high, low, close, base_vol, quote_vol, trades, taker_base, taker_quote = row
        if close_ms < open_ms:
            raise RuntimeError(f"close before open at {open_ms}")
        if high < max(op, low, close) or low > min(op, high, close):
            raise RuntimeError(f"OHLC invariant failed at {open_ms}")
        if min(base_vol, quote_vol, taker_base, taker_quote) < 0 or trades < 0:
            raise RuntimeError(f"negative volume/count at {open_ms}")


def main() -> None:
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    if DB_PATH.exists():
        DB_PATH.unlink()

    all_rows: list[tuple] = []
    sources: list[dict] = []
    for url in archive_urls():
        payload, archive_sha = fetch_archive(url)
        rows = parse_archive(payload)
        all_rows.extend(rows)
        sources.append({"url": url, "sha256": archive_sha, "rows_in_target_range": len(rows)})

    validate_rows(all_rows)

    connection = sqlite3.connect(DB_PATH)
    try:
        connection.execute("PRAGMA foreign_keys = ON")
        connection.execute("PRAGMA journal_mode = DELETE")
        apply_schema(connection)
        connection.execute(
            "INSERT INTO market_instruments("
            "instrument_id, venue, market_type, symbol, base_asset, quote_asset, margin_asset, "
            "contract_type, created_at_ms) VALUES "
            "(1, 'binance', 'usd_m_perpetual', 'XAGUSDT', 'XAG', 'USDT', 'USDT', 'perpetual', ?)",
            (FIXED_METADATA_TIME_MS,),
        )
        connection.execute(
            "INSERT INTO historical_datasets("
            "dataset_id, instrument_id, dataset_kind, interval, source, start_time_ms, end_time_ms, "
            "downloaded_at_ms, status) VALUES "
            "(1, 1, 'traded_kline', '1m', 'binance_public_data', ?, ?, ?, 'complete')",
            (START_MS, END_CLOSE_MS, FIXED_METADATA_TIME_MS),
        )
        connection.executemany(
            "INSERT INTO historical_ohlcv("
            "dataset_id, open_time_ms, close_time_ms, open_price, high_price, low_price, close_price, "
            "base_volume, quote_volume, trade_count, taker_buy_base_volume, taker_buy_quote_volume) "
            "VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            all_rows,
        )
        connection.commit()

        checks = connection.execute(
            "SELECT COUNT(*), MIN(open_time_ms), MAX(open_time_ms), "
            "COUNT(DISTINCT open_time_ms) FROM historical_ohlcv WHERE dataset_id = 1"
        ).fetchone()
        if checks != (EXPECTED_ROWS, START_MS, END_OPEN_MS, EXPECTED_ROWS):
            raise RuntimeError(f"database verification failed: {checks}")

        auth_rows = connection.execute("SELECT COUNT(*) FROM auth_audit_events").fetchone()[0]
        run_rows = connection.execute("SELECT COUNT(*) FROM trading_runs").fetchone()[0]
        if auth_rows != 0 or run_rows != 0:
            raise RuntimeError("fixture must not contain auth or trading-run history")

        connection.execute("VACUUM")
    finally:
        connection.close()

    db_sha = hashlib.sha256(DB_PATH.read_bytes()).hexdigest()
    manifest = {
        "fixture": DB_PATH.name,
        "purpose": "Frozen full real-market regression fixture for BinanceBots Phase 3",
        "source": "Binance public data archives (data.binance.vision)",
        "venue": "binance",
        "market_type": MARKET_TYPE,
        "symbol": SYMBOL,
        "interval": INTERVAL,
        "coverage": {
            "first_open_time_utc": "2026-01-07T10:00:00Z",
            "last_open_time_utc": "2026-09-20T23:59:00Z",
            "last_close_time_utc": "2026-09-20T23:59:59.999Z",
            "first_open_time_ms": START_MS,
            "last_open_time_ms": END_OPEN_MS,
            "last_close_time_ms": END_CLOSE_MS,
            "row_count": EXPECTED_ROWS,
        },
        "schema_version": 4,
        "dataset_id": 1,
        "instrument_id": 1,
        "database_bytes": DB_PATH.stat().st_size,
        "database_sha256": db_sha,
        "archive_sources": sources,
        "contains_auth_rows": False,
        "contains_trading_run_history": False,
        "ordinary_ci_downloads_market_data": False,
    }
    MANIFEST_PATH.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps(manifest, indent=2), flush=True)


if __name__ == "__main__":
    try:
        main()
    except Exception as exc:
        print(f"fixture build failed: {exc}", file=sys.stderr, flush=True)
        raise
