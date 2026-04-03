use std::sync::atomic::{AtomicU64, Ordering};

// Immutable value object shared with websocket event builders.
#[derive(Clone, Copy)]
pub struct CacheMetricsSnapshot {
    pub hit_count: u64,
    pub miss_count: u64,
    pub total: u64,
    pub hit_rate: f64,
    pub miss_rate: f64,
}

pub trait CacheMetricsTracker {
    fn record_hit(&self) -> CacheMetricsSnapshot;
    fn record_miss(&self) -> CacheMetricsSnapshot;
}

// Thread-safe tracker used by all request handlers.
pub struct AtomicCacheMetricsTracker {
    hit_count: AtomicU64,
    miss_count: AtomicU64,
}

impl AtomicCacheMetricsTracker {
    pub const fn new() -> Self {
        Self {
            hit_count: AtomicU64::new(0),
            miss_count: AtomicU64::new(0),
        }
    }

    fn compute_snapshot(&self) -> CacheMetricsSnapshot {
        let hit_count = self.hit_count.load(Ordering::Relaxed);
        let miss_count = self.miss_count.load(Ordering::Relaxed);
        let total = hit_count + miss_count;

        if total == 0 {
            return CacheMetricsSnapshot {
                hit_count,
                miss_count,
                total,
                hit_rate: 0.0,
                miss_rate: 0.0,
            };
        }

        let hit_rate = ((hit_count as f64 / total as f64) * 1000.0).round() / 10.0;
        let miss_rate = ((100.0 - hit_rate) * 10.0).round() / 10.0;

        CacheMetricsSnapshot {
            hit_count,
            miss_count,
            total,
            hit_rate,
            miss_rate,
        }
    }
}

impl CacheMetricsTracker for AtomicCacheMetricsTracker {
    fn record_hit(&self) -> CacheMetricsSnapshot {
        self.hit_count.fetch_add(1, Ordering::Relaxed);
        self.compute_snapshot()
    }

    fn record_miss(&self) -> CacheMetricsSnapshot {
        self.miss_count.fetch_add(1, Ordering::Relaxed);
        self.compute_snapshot()
    }
}
