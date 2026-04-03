use actix_web::web::{self, Path};
use actix_web::{HttpRequest, HttpResponse, Responder};

use crate::cache_metrics::{CacheMetricsSnapshot, CacheMetricsTracker};
use crate::domain::{encode_base62, select_db_node, select_node_index, unix_timestamp_secs};
use crate::events::{broadcast, next_event_id, node_identity, request_base_url};
use crate::models::{CreateRequest, CreateResponse};
use crate::state::AppState;
use crate::storage::{
    check_rate_limit, db_fetch_with_retry, db_insert_with_retry, redis_get_with_fallback,
    redis_set_with_fallback,
};
use std::time::Instant;

fn with_cache_metrics(mut event: serde_json::Value, snapshot: CacheMetricsSnapshot) -> String {
    if let Some(payload) = event.as_object_mut() {
        payload.insert("cache_hit_count".to_string(), serde_json::json!(snapshot.hit_count));
        payload.insert("cache_miss_count".to_string(), serde_json::json!(snapshot.miss_count));
        payload.insert("cache_total".to_string(), serde_json::json!(snapshot.total));
        payload.insert("cache_hit_rate".to_string(), serde_json::json!(snapshot.hit_rate));
        payload.insert("cache_miss_rate".to_string(), serde_json::json!(snapshot.miss_rate));
        payload.insert(
            "cache_metrics".to_string(),
            serde_json::json!({
                "hit_count": snapshot.hit_count,
                "miss_count": snapshot.miss_count,
                "total": snapshot.total,
                "hit_rate": snapshot.hit_rate,
                "miss_rate": snapshot.miss_rate
            }),
        );
    }

    event.to_string()
}

pub async fn create_short_url(
    app_state: web::Data<AppState>,
    http_req: HttpRequest,
    req: web::Json<CreateRequest>,
) -> impl Responder {
    let started_at = Instant::now();
    let long_url = req.long_url.clone();
    let rate_key = format!("rate_limit:{}", long_url);

    if let Ok(is_limited) = check_rate_limit(&rate_key, &app_state.ring, &app_state.redis_nodes).await {
        if is_limited {
            return HttpResponse::TooManyRequests().body("Rate limit exceeded");
        }
    }

    let mut generator = app_state.generator.lock().unwrap();
    let id = generator.generate();
    drop(generator);

    let short_code = encode_base62(id);

    let node = match app_state.ring.get_node(&short_code) {
        Some(node) => node,
        None => return HttpResponse::InternalServerError().finish(),
    };
    let route_index = select_node_index(&short_code, &app_state.ring).unwrap_or(0);
    let redis_name = format!("redis{}", route_index);
    let db_name = format!("shard{}", route_index);

    println!(
        "[ROUTING] key={} node={} timestamp={}",
        short_code,
        node,
        unix_timestamp_secs()
    );

    let db_shard = match select_db_node(&short_code, &app_state.ring, &app_state.db_nodes) {
        Some(shard) => shard,
        None => return HttpResponse::InternalServerError().finish(),
    };
    let primary = &db_shard.0;
    let base_url = request_base_url(&http_req);

    if db_insert_with_retry(primary, id as i64, &short_code, &long_url).await {
        let _ =
            redis_set_with_fallback(&short_code, &long_url, &app_state.ring, &app_state.redis_nodes)
                .await;

        broadcast(
            serde_json::json!({
                "event_id": next_event_id(),
                "type": "write",
                "status": "OK",
                "node": node_identity(),
                "redis": redis_name,
                "db": db_name,
                "short_code": short_code,
                "latency_ms": started_at.elapsed().as_millis() as u64
            })
            .to_string(),
        );

        HttpResponse::Ok().json(CreateResponse {
            short_url: format!("{}/{}", base_url, short_code),
        })
    } else {
        broadcast(
            serde_json::json!({
                "event_id": next_event_id(),
                "type": "write",
                "status": "ERROR",
                "node": node_identity(),
                "redis": redis_name,
                "db": db_name,
                "short_code": short_code,
                "latency_ms": started_at.elapsed().as_millis() as u64
            })
            .to_string(),
        );

        HttpResponse::InternalServerError().finish()
    }
}

pub async fn redirect_short_url(app_state: web::Data<AppState>, path: Path<String>) -> HttpResponse {
    let started_at = Instant::now();
    let short_code = path.into_inner();
    let node = match app_state.ring.get_node(&short_code) {
        Some(node) => node,
        None => return HttpResponse::InternalServerError().finish(),
    };
    let route_index = select_node_index(&short_code, &app_state.ring).unwrap_or(0);
    let redis_name = format!("redis{}", route_index);
    let db_name = format!("shard{}", route_index);

    println!(
        "[ROUTING] key={} node={} timestamp={}",
        short_code,
        node,
        unix_timestamp_secs()
    );

    let db_shard = match select_db_node(&short_code, &app_state.ring, &app_state.db_nodes) {
        Some(shard) => shard,
        None => return HttpResponse::InternalServerError().finish(),
    };
    let primary = &db_shard.0;
    let replica = &db_shard.1;

    if let Ok(Some(long_url)) =
        redis_get_with_fallback(&short_code, &app_state.ring, &app_state.redis_nodes).await
    {
        let cache_metrics = app_state.cache_metrics.record_hit();

        broadcast(
            with_cache_metrics(
                serde_json::json!({
                    "event_id": next_event_id(),
                    "type": "read",
                    "status": "OK",
                    "cache": "HIT",
                    "source": "redis",
                    "node": node_identity(),
                    "redis": redis_name,
                    "db": db_name,
                    "short_code": short_code,
                    "latency_ms": started_at.elapsed().as_millis() as u64
                }),
                cache_metrics,
            ),
        );

        return HttpResponse::Found()
            .append_header(("Location", long_url))
            .finish();
    }

    let (db_value, source) = match db_fetch_with_retry(replica, &short_code).await {
        Ok(Some(url)) => (Some(url), "replica"),
        Ok(None) => match db_fetch_with_retry(primary, &short_code).await {
            Ok(Some(url)) => (Some(url), "primary"),
            Ok(None) => (None, "none"),
            Err(_) => (None, "error"),
        },
        Err(_) => match db_fetch_with_retry(primary, &short_code).await {
            Ok(Some(url)) => (Some(url), "primary"),
            Ok(None) => (None, "none"),
            Err(_) => (None, "error"),
        },
    };

    match db_value {
        Some(long_url) => {
            let _ = redis_set_with_fallback(
                &short_code,
                &long_url,
                &app_state.ring,
                &app_state.redis_nodes,
            )
            .await;

            let cache_metrics = app_state.cache_metrics.record_miss();

            broadcast(
                with_cache_metrics(
                    serde_json::json!({
                        "event_id": next_event_id(),
                        "type": "read",
                        "status": "OK",
                        "cache": "MISS",
                        "source": source,
                        "node": node_identity(),
                        "redis": redis_name,
                        "db": db_name,
                        "short_code": short_code,
                        "latency_ms": started_at.elapsed().as_millis() as u64
                    }),
                    cache_metrics,
                ),
            );

            HttpResponse::Found()
                .append_header(("Location", long_url))
                .finish()
        }
        None => {
            let status = if source == "error" {
                "ERROR"
            } else {
                "NOT_FOUND"
            };

            let cache_metrics = app_state.cache_metrics.record_miss();

            broadcast(
                with_cache_metrics(
                    serde_json::json!({
                        "event_id": next_event_id(),
                        "type": "read",
                        "status": status,
                        "cache": "MISS",
                        "source": source,
                        "node": node_identity(),
                        "redis": redis_name,
                        "db": db_name,
                        "short_code": short_code,
                        "latency_ms": started_at.elapsed().as_millis() as u64
                    }),
                    cache_metrics,
                ),
            );

            if source == "error" {
                HttpResponse::InternalServerError().finish()
            } else {
                HttpResponse::NotFound().body("Not found")
            }
        }
    }
}
