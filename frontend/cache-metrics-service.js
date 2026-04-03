(function (root) {
  function toBackendCount(value) {
    const parsed = Number(value);
    if (!Number.isFinite(parsed)) {
      return null;
    }

    return Math.max(0, Math.round(parsed));
  }

  function toBackendRate(value) {
    const parsed = Number(value);
    if (!Number.isFinite(parsed)) {
      return null;
    }

    return Math.max(0, Math.min(100, parsed));
  }

  function extractSnapshot(payload) {
    if (!payload || typeof payload !== "object") {
      return null;
    }

    const nested =
      payload.cache_metrics && typeof payload.cache_metrics === "object" ? payload.cache_metrics : null;

    const hitCount = toBackendCount(nested?.hit_count ?? payload.cache_hit_count);
    const missCount = toBackendCount(nested?.miss_count ?? payload.cache_miss_count);
    const hitRate = toBackendRate(nested?.hit_rate ?? payload.cache_hit_rate);
    const missRate = toBackendRate(nested?.miss_rate ?? payload.cache_miss_rate);

    if (hitCount === null || missCount === null || hitRate === null || missRate === null) {
      return null;
    }

    return {
      hitCount,
      missCount,
      total: hitCount + missCount,
      hasData: hitCount + missCount > 0,
      hitRate,
      missRate
    };
  }

  // Owns monotonic cache metric state derived from backend websocket payloads.
  class BackendCacheMetricsStore {
    constructor() {
      this.snapshot = null;
    }

    mergeFromPayload(payload) {
      const next = extractSnapshot(payload);
      if (!next) {
        return this.snapshot;
      }

      // Protects UI from out-of-order events when reconnecting.
      const currentTotal = this.snapshot ? this.snapshot.total : -1;
      if (!this.snapshot || next.total >= currentTotal) {
        this.snapshot = next;
      }

      return this.snapshot;
    }

    getSnapshot() {
      return this.snapshot;
    }
  }

  root.BackendCacheMetricsStore = BackendCacheMetricsStore;
})(typeof globalThis !== "undefined" ? globalThis : this);
