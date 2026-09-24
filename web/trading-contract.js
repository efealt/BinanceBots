(function (root, factory) {
  const contract = factory();
  if (typeof module === "object" && module.exports) {
    module.exports = contract;
  }
  root.BinanceGridTradingContract = contract;
})(typeof globalThis !== "undefined" ? globalThis : this, function () {
  "use strict";

  function finiteNumber(value) {
    const parsed = Number(value);
    return Number.isFinite(parsed) ? parsed : null;
  }

  function positiveInteger(value, fallback) {
    const parsed = Number(value);
    return Number.isInteger(parsed) && parsed > 0 ? parsed : fallback;
  }

  function runPath(runId, suffix = "") {
    const id = positiveInteger(runId, null);
    if (id === null) throw new Error("run_id must be a positive integer");
    return "/api/trading/runs/" + id + suffix;
  }

  const paperAdapter = Object.freeze({
    mode: "paper",
    label: "Paper",
    selectorLabel: "PAPER",
    executionKind: "simulated",
    locked: false,
    lockReason: null,
    theme: "paper",
    urls: Object.freeze({
      runs(limit = 100) {
        return "/api/trading/runs?limit=" + positiveInteger(limit, 100);
      },
      snapshot(runId) {
        return runPath(runId);
      },
      chart(runId) {
        return runPath(runId, "/chart");
      },
      auditTail(runId, limit = 250) {
        return runPath(runId, "/audit") + "?limit=" + positiveInteger(limit, 250);
      },
      auditAfter(runId, afterSequence, limit = 500) {
        const sequence = Number(afterSequence);
        if (!Number.isInteger(sequence) || sequence < 0) {
          throw new Error("after_sequence must be zero or positive");
        }
        return runPath(runId, "/audit") + "?after_sequence=" + sequence + "&limit=" + positiveInteger(limit, 500);
      },
      stream(runId) {
        return runPath(runId, "/stream");
      },
      start() {
        return "/api/trading/runs";
      },
      stop(runId) {
        return runPath(runId, "/stop");
      },
    }),
    buildStartPayload(configuration) {
      return { mode: "paper", ...configuration };
    },
  });

  const liveAdapter = Object.freeze({
    mode: "live",
    label: "Live",
    selectorLabel: "LIVE",
    executionKind: "binance_private",
    locked: true,
    lockReason: "Live execution is locked server-side until Phase 8.",
    theme: "live",
    urls: Object.freeze({
      runs() { return null; },
      snapshot() { return null; },
      chart() { return null; },
      auditTail() { return null; },
      auditAfter() { return null; },
      stream() { return null; },
      start() { return null; },
      stop() { return null; },
    }),
    buildStartPayload() {
      throw new Error("Live execution is locked server-side until Phase 8.");
    },
  });

  const adapters = Object.freeze({
    paper: paperAdapter,
    live: liveAdapter,
  });

  function adapterFor(mode) {
    const key = String(mode || "").toLowerCase();
    const adapter = adapters[key];
    if (!adapter) throw new Error("Unsupported trading mode: " + mode);
    return adapter;
  }

  function normalizeOrder(order) {
    const originalQuantity = finiteNumber(order?.original_quantity ?? order?.originalQuantity ?? order?.quantity) ?? 0;
    const filledQuantity = finiteNumber(order?.filled_quantity ?? order?.filledQuantity) ?? 0;
    const remainingQuantity = finiteNumber(order?.remaining_quantity ?? order?.remainingQuantity)
      ?? Math.max(0, originalQuantity - filledQuantity);
    return {
      id: Number(order?.order_id ?? order?.id),
      side: String(order?.side ?? "").toLowerCase(),
      orderType: String(order?.order_type ?? order?.orderType ?? "").toLowerCase(),
      price: finiteNumber(order?.price),
      originalQuantity,
      filledQuantity,
      remainingQuantity,
      status: String(order?.status ?? "").toLowerCase(),
      submittedAtMs: finiteNumber(order?.submitted_at_ms ?? order?.submittedAtMs),
      raw: order,
    };
  }

  function normalizeFill(fill) {
    return {
      orderId: Number(fill?.order_id ?? fill?.orderId),
      side: String(fill?.side ?? "").toLowerCase(),
      orderType: String(fill?.order_type ?? fill?.orderType ?? "").toLowerCase(),
      price: finiteNumber(fill?.price),
      quantity: finiteNumber(fill?.quantity),
      fee: finiteNumber(fill?.fee),
      status: String(fill?.status ?? "").toLowerCase(),
      eventTimeMs: finiteNumber(fill?.event_time_ms ?? fill?.eventTimeMs),
      raw: fill,
    };
  }

  function normalizeSnapshot(snapshot) {
    if (!snapshot || typeof snapshot !== "object") {
      throw new Error("Trading snapshot must be an object");
    }

    const mode = String(snapshot.mode || "paper").toLowerCase();
    const adapter = adapterFor(mode);
    const portfolio = snapshot.portfolio || {};
    const positionQuantity = finiteNumber(portfolio.position_quantity ?? portfolio.positionQuantity) ?? 0;
    const equity = finiteNumber(portfolio.equity);
    const markPrice = finiteNumber(snapshot.mark_price ?? snapshot.markPrice)
      ?? finiteNumber(snapshot.mid_price ?? snapshot.midPrice)
      ?? finiteNumber(snapshot.latest_base_candle?.close ?? snapshot.latestBaseCandle?.close)
      ?? finiteNumber(snapshot.latest_replay_candle?.close ?? snapshot.latestReplayCandle?.close)
      ?? finiteNumber(portfolio.average_entry_price ?? portfolio.averageEntryPrice);
    const grossExposurePercent = equity !== null && equity > 0 && markPrice !== null
      ? (Math.abs(positionQuantity * markPrice) / equity) * 100
      : null;

    return {
      run: {
        id: Number(snapshot.run_id ?? snapshot.runId),
        mode,
        runtimeStatus: String(snapshot.runtime_status ?? snapshot.runtimeStatus ?? ""),
        canonicalStatus: String(snapshot.canonical_status ?? snapshot.canonicalStatus ?? ""),
        runtimeActive: Boolean(snapshot.runtime_active ?? snapshot.runtimeActive),
        createdAtMs: finiteNumber(snapshot.created_at_ms ?? snapshot.createdAtMs),
        startedAtMs: finiteNumber(snapshot.started_at_ms ?? snapshot.startedAtMs),
        endedAtMs: finiteNumber(snapshot.ended_at_ms ?? snapshot.endedAtMs),
        updatedAtMs: finiteNumber(snapshot.updated_at_ms ?? snapshot.updatedAtMs),
        streamRevision: finiteNumber(snapshot.stream_revision ?? snapshot.streamRevision),
      },
      market: {
        symbol: String(snapshot.symbol ?? ""),
        marketType: String(snapshot.market_type ?? snapshot.marketType ?? ""),
        replayInterval: String(snapshot.replay_interval ?? snapshot.replayInterval ?? ""),
        feedStatus: String(snapshot.feed_status ?? snapshot.feedStatus ?? "loading"),
        bestBid: finiteNumber(snapshot.best_bid ?? snapshot.bestBid),
        bestAsk: finiteNumber(snapshot.best_ask ?? snapshot.bestAsk),
        midPrice: finiteNumber(snapshot.mid_price ?? snapshot.midPrice),
        markPrice,
        latestBaseCandle: snapshot.latest_base_candle ?? snapshot.latestBaseCandle ?? null,
        latestReplayCandle: snapshot.latest_replay_candle ?? snapshot.latestReplayCandle ?? null,
      },
      strategy: {
        id: String(snapshot.strategy_id ?? snapshot.strategyId ?? ""),
        version: String(snapshot.strategy_version ?? snapshot.strategyVersion ?? ""),
        params: snapshot.strategy_params ?? snapshot.strategyParams ?? {},
      },
      portfolio: {
        cash: finiteNumber(portfolio.cash),
        positionQuantity,
        averageEntryPrice: finiteNumber(portfolio.average_entry_price ?? portfolio.averageEntryPrice),
        realizedPnl: finiteNumber(portfolio.realized_pnl ?? portfolio.realizedPnl),
        unrealizedPnl: finiteNumber(portfolio.unrealized_pnl ?? portfolio.unrealizedPnl),
        feesPaid: finiteNumber(portfolio.fees_paid ?? portfolio.feesPaid),
        equity,
        grossExposurePercent,
      },
      orders: (snapshot.open_orders ?? snapshot.openOrders ?? []).map(normalizeOrder),
      fills: (snapshot.recent_fills ?? snapshot.recentFills ?? []).map(normalizeFill),
      events: snapshot.recent_events ?? snapshot.recentEvents ?? [],
      execution: {
        kind: adapter.executionKind,
        assumptions: snapshot.execution_assumptions ?? snapshot.executionAssumptions ?? {},
      },
      configuration: {
        initialCapital: snapshot.initial_capital ?? snapshot.initialCapital ?? null,
        runConfig: snapshot.run_config ?? snapshot.runConfig ?? {},
        dataSource: snapshot.data_source ?? snapshot.dataSource ?? {},
      },
      adapter,
      raw: snapshot,
    };
  }

  function selectRun(runs) {
    const items = Array.isArray(runs) ? runs : [];
    return items.find((run) => ["arming", "running"].includes(String(run.runtime_status ?? run.runtimeStatus ?? "")))
      ?? null;
  }

  return Object.freeze({
    adapters,
    adapterFor,
    normalizeSnapshot,
    selectRun,
  });
});
