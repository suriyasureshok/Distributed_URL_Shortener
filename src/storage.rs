use crate::domain::{ConsistentHash, fallback_indices};
use crate::reliability::{IO_TIMEOUT_MS, RETRY_ATTEMPTS, backoff_delay};
use redis::AsyncCommands;
use redis::Client as RedisClient;
use sqlx::PgPool;
use std::time::Duration;
use tokio::time::{sleep, timeout};

pub async fn check_rate_limit(
    key: &str,
    ring: &ConsistentHash,
    redis_nodes: &[RedisClient],
) -> Result<bool, ()> {
    for index in fallback_indices(key, ring, redis_nodes.len()) {
        let redis = &redis_nodes[index];

        for attempt in 0..RETRY_ATTEMPTS {
            match timeout(
                Duration::from_millis(IO_TIMEOUT_MS),
                redis.get_async_connection(),
            )
            .await
            {
                Ok(Ok(mut conn)) => {
                    let incr_result = timeout(
                        Duration::from_millis(IO_TIMEOUT_MS),
                        conn.incr::<_, _, i32>(key, 1),
                    )
                    .await;

                    if let Ok(Ok(count)) = incr_result {
                        let _ = timeout(
                            Duration::from_millis(IO_TIMEOUT_MS),
                            conn.expire::<_, ()>(key, 60),
                        )
                        .await;
                        return Ok(count > 10);
                    }
                }
                _ => {}
            }

            if attempt + 1 < RETRY_ATTEMPTS {
                sleep(backoff_delay(attempt)).await;
            }
        }
    }

    Err(())
}

pub async fn redis_get_with_fallback(
    key: &str,
    ring: &ConsistentHash,
    redis_nodes: &[RedisClient],
) -> Result<Option<String>, ()> {
    for index in fallback_indices(key, ring, redis_nodes.len()) {
        let redis = &redis_nodes[index];

        for attempt in 0..RETRY_ATTEMPTS {
            match timeout(
                Duration::from_millis(IO_TIMEOUT_MS),
                redis.get_async_connection(),
            )
            .await
            {
                Ok(Ok(mut conn)) => {
                    let get_result = timeout(
                        Duration::from_millis(IO_TIMEOUT_MS),
                        conn.get::<_, Option<String>>(key),
                    )
                    .await;

                    if let Ok(Ok(value)) = get_result {
                        return Ok(value);
                    }
                }
                _ => {}
            }

            if attempt + 1 < RETRY_ATTEMPTS {
                sleep(backoff_delay(attempt)).await;
            }
        }
    }

    Err(())
}

pub async fn redis_set_with_fallback(
    key: &str,
    value: &str,
    ring: &ConsistentHash,
    redis_nodes: &[RedisClient],
) -> bool {
    for index in fallback_indices(key, ring, redis_nodes.len()) {
        let redis = &redis_nodes[index];

        for attempt in 0..RETRY_ATTEMPTS {
            match timeout(
                Duration::from_millis(IO_TIMEOUT_MS),
                redis.get_async_connection(),
            )
            .await
            {
                Ok(Ok(mut conn)) => {
                    let set_result = timeout(
                        Duration::from_millis(IO_TIMEOUT_MS),
                        conn.set_ex::<_, _, ()>(key, value, 3600),
                    )
                    .await;

                    if let Ok(Ok(())) = set_result {
                        return true;
                    }
                }
                _ => {}
            }

            if attempt + 1 < RETRY_ATTEMPTS {
                sleep(backoff_delay(attempt)).await;
            }
        }
    }

    false
}

pub async fn db_insert_with_retry(
    pool: &PgPool,
    id: i64,
    short_code: &str,
    long_url: &str,
) -> bool {
    for attempt in 0..RETRY_ATTEMPTS {
        let result = timeout(
            Duration::from_millis(IO_TIMEOUT_MS),
            sqlx::query!(
                "INSERT INTO urls (id, short_code, long_url) VALUES ($1, $2, $3)",
                id,
                short_code,
                long_url
            )
            .execute(pool),
        )
        .await;

        if let Ok(Ok(_)) = result {
            return true;
        }

        if attempt + 1 < RETRY_ATTEMPTS {
            sleep(backoff_delay(attempt)).await;
        }
    }

    false
}

pub async fn db_fetch_with_retry(pool: &PgPool, short_code: &str) -> Result<Option<String>, ()> {
    for attempt in 0..RETRY_ATTEMPTS {
        let result = timeout(
            Duration::from_millis(IO_TIMEOUT_MS),
            sqlx::query!(
                "SELECT long_url FROM urls WHERE short_code = $1",
                short_code
            )
            .fetch_optional(pool),
        )
        .await;

        match result {
            Ok(Ok(Some(row))) => return Ok(Some(row.long_url)),
            Ok(Ok(None)) => return Ok(None),
            _ => {
                if attempt + 1 < RETRY_ATTEMPTS {
                    sleep(backoff_delay(attempt)).await;
                }
            }
        }
    }

    Err(())
}
