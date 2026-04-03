use crate::domain::ConsistentHash;
use crate::models::DbShard;
use redis::Client as RedisClient;
use sqlx::PgPool;
use std::env;

pub fn load_db_node_urls() -> Vec<String> {
    let db_urls = env::var("DATABASE_NODES")
        .or_else(|_| env::var("DB_NODES"))
        .unwrap_or_else(|_| {
            "postgres://postgres:pass@localhost:5433/postgres,postgres://postgres:pass@localhost:5434/postgres,postgres://postgres:pass@localhost:5435/postgres".to_string()
        });

    let db_node_urls: Vec<String> = db_urls
        .split(',')
        .map(str::trim)
        .filter(|url| !url.is_empty())
        .map(str::to_string)
        .collect();

    if db_node_urls.is_empty() {
        panic!("No DB nodes configured. Set DB_NODES in .env");
    }

    if db_node_urls.len() % 2 != 0 {
        panic!("DB_NODES must contain an even number of URLs (primary,replica per shard)");
    }

    db_node_urls
}

pub fn load_redis_node_urls() -> Vec<String> {
    let redis_urls = env::var("REDIS_NODES").unwrap_or_else(|_| {
        "redis://127.0.0.1:6379/,redis://127.0.0.1:6380/,redis://127.0.0.1:6381/".to_string()
    });

    let redis_node_urls: Vec<String> = redis_urls
        .split(',')
        .map(str::trim)
        .filter(|url| !url.is_empty())
        .map(str::to_string)
        .collect();

    if redis_node_urls.is_empty() {
        panic!("No Redis nodes configured. Set REDIS_NODES in .env");
    }

    redis_node_urls
}

pub async fn init_db_pools(db_node_urls: &[String]) -> Vec<DbShard> {
    let mut db_pools: Vec<DbShard> = Vec::new();

    for pair in db_node_urls.chunks(2) {
        let primary_url = &pair[0];
        let replica_url = &pair[1];

        let primary = PgPool::connect(primary_url.as_str())
            .await
            .unwrap_or_else(|_| panic!("DB primary connection failed for {}", primary_url));

        let replica = PgPool::connect(replica_url.as_str())
            .await
            .unwrap_or_else(|_| panic!("DB replica connection failed for {}", replica_url));

        db_pools.push((primary, replica));
    }

    db_pools
}

pub async fn ensure_db_schema(db_pools: &[DbShard]) {
    for (primary, replica) in db_pools {
        for db in [primary, replica] {
            sqlx::query(
                "CREATE TABLE IF NOT EXISTS urls (
                    id BIGINT PRIMARY KEY,
                    short_code TEXT UNIQUE,
                    long_url TEXT UNIQUE NOT NULL
                )",
            )
            .execute(db)
            .await
            .unwrap();
        }
    }
}

pub fn init_redis_clients(redis_node_urls: &[String]) -> Vec<RedisClient> {
    redis_node_urls
        .iter()
        .map(|url| {
            RedisClient::open(url.as_str())
                .unwrap_or_else(|_| panic!("Redis connection failed for {}", url))
        })
        .collect()
}

pub fn build_ring(redis_count: usize) -> ConsistentHash {
    let mut ring = ConsistentHash::new(100);
    for index in 0..redis_count {
        ring.add_node(&format!("redis{}", index));
    }
    ring
}
