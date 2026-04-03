use crate::cache_metrics::AtomicCacheMetricsTracker;
use crate::domain::{ConsistentHash, Snowflake};
use crate::models::DbShard;
use redis::Client as RedisClient;
use std::sync::Mutex;

pub struct AppState {
    pub db_nodes: Vec<DbShard>,
    pub redis_nodes: Vec<RedisClient>,
    pub ring: ConsistentHash,
    pub generator: Mutex<Snowflake>,
    pub cache_metrics: AtomicCacheMetricsTracker,
}

impl AppState {
    pub fn new(db_nodes: Vec<DbShard>, redis_nodes: Vec<RedisClient>, ring: ConsistentHash) -> Self {
        Self {
            db_nodes,
            redis_nodes,
            ring,
            generator: Mutex::new(Snowflake::new(1)),
            cache_metrics: AtomicCacheMetricsTracker::new(),
        }
    }
}
