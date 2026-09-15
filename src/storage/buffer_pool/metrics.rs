use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Default)]
pub struct BufferPoolMetrics {
    fetches: AtomicU64,
    cache_hits: AtomicU64,
    cache_misses: AtomicU64,
    evictions: AtomicU64,
}
#[derive(Debug, Clone, Copy)]
pub struct MetricsSnapshot {
    pub fetches: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub evictions: u64,
    pub hit_rate: f64,
}

impl BufferPoolMetrics {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_fetch(&self) {
        self.fetches.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_hit(&self) {
        self.cache_hits.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_miss(&self) {
        self.cache_misses.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_eviction(&self) {
        self.evictions.fetch_add(1, Ordering::Relaxed);
    }

    pub fn fetches(&self) -> u64 {
        self.fetches.load(Ordering::Relaxed)
    }

    pub fn cache_hits(&self) -> u64 {
        self.cache_hits.load(Ordering::Relaxed)
    }

    pub fn cache_misses(&self) -> u64 {
        self.cache_misses.load(Ordering::Relaxed)
    }

    pub fn evictions(&self) -> u64 {
        self.evictions.load(Ordering::Relaxed)
    }

    pub fn hit_rate(&self) -> f64 {
        let hits = self.cache_hits();
        let misses = self.cache_misses();
        let total = hits + misses;

        if total == 0 {
            return 0.0;
        }

        hits as f64 / total as f64
    }

    pub fn reset(&self) {
        self.fetches.store(0, Ordering::Relaxed);
        self.cache_hits.store(0, Ordering::Relaxed);
        self.cache_misses.store(0, Ordering::Relaxed);
        self.evictions.store(0, Ordering::Relaxed);
    }
}

impl BufferPoolMetrics {
    pub fn snapshot(&self) -> MetricsSnapshot {
        let fetches = self.fetches();
        let cache_hits = self.cache_hits();

        MetricsSnapshot {
            fetches,
            cache_hits,
            cache_misses: self.cache_misses(),
            evictions: self.evictions(),
            hit_rate: if fetches == 0 {
                0.0
            } else {
                cache_hits as f64 / fetches as f64
            },
        }
    }
}
