use actix::{Actor, ActorContext, Addr, AsyncContext, Handler, Message, StreamHandler};
use actix_cors::Cors;
use actix_web::web::Path;
use actix_web::{App, HttpResponse, HttpServer, Responder, web};
use actix_web_actors::ws;
use dotenvy::dotenv;
use once_cell::sync::Lazy;
use redis::AsyncCommands;
use redis::Client as redisClient;
use serde::{Deserialize, Serialize};
use sqlx::PgPool as pgPool;
use std::env;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::time::{sleep, timeout};

use std::collections::BTreeMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

#[derive(Deserialize)]
struct CreateRequest {
    long_url: String,
}

#[derive(Serialize)]
struct CreateResponse {
    short_url: String,
}

fn hash<T: Hash>(item: &T) -> u64 {
    let mut hasher = DefaultHasher::new();
    item.hash(&mut hasher);
    hasher.finish()
}

struct ConsistentHash {
    ring: BTreeMap<u64, String>,
    replicas: usize,
}

struct Snowflake {
    machine_id: u16,
    sequence: u16,
    last_timestamp: u64,
}

impl Snowflake {
    fn new(machine_id: u16) -> Self {
        Self {
            machine_id,
            sequence: 0,
            last_timestamp: 0,
        }
    }

    fn now() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
    }

    fn generate(&mut self) -> u64 {
        let mut ts = Self::now();

        if ts == self.last_timestamp {
            self.sequence += 1;

            if self.sequence > 4095 {
                while ts <= self.last_timestamp {
                    ts = Self::now();
                }
                self.sequence = 0;
            }
        } else {
            self.sequence = 0;
        }

        self.last_timestamp = ts;

        (ts << 22) | ((self.machine_id as u64) << 12) | self.sequence as u64
    }
}

impl ConsistentHash {
    fn new(replicas: usize) -> Self {
        Self {
            ring: BTreeMap::new(),
            replicas,
        }
    }

    fn add_node(&mut self, node: &str) {
        for i in 0..self.replicas {
            let virtual_node = format!("{}#{}", node, i);
            let h = hash(&virtual_node);
            self.ring.insert(h, node.to_string());
        }
    }

    fn remove_node(&mut self, node: &str) {
        for i in 0..self.replicas {
            let virtual_node = format!("{}#{}", node, i);
            let h = hash(&virtual_node);
            self.ring.remove(&h);
        }
    }

    fn get_node(&self, key: &str) -> Option<&String> {
        let h = hash(&key);

        self.ring
            .range(h..)
            .next()
            .or_else(|| self.ring.iter().next())
            .map(|(_, v)| v)
    }
}

const BASE62: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";
type DbShard = (pgPool, pgPool);
const IO_TIMEOUT_MS: u64 = 800;
const RETRY_ATTEMPTS: usize = 3;
const BACKOFF_BASE_MS: u64 = 50;
static WS_CLIENTS: Lazy<Mutex<Vec<Addr<WsSession>>>> = Lazy::new(|| Mutex::new(Vec::new()));

#[derive(Message)]
#[rtype(result = "()")]
struct BroadcastText(String);

fn encode_base62(mut num: u64) -> String {
    if num == 0 {
        return "0".to_string();
    }

    let mut result = String::new();

    while num > 0 {
        let rem = (num % 62) as usize;
        result.push(BASE62[rem] as char);
        num /= 62;
    }

    result.chars().rev().collect()
}

fn unix_timestamp_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn fallback_indices(key: &str, ring: &ConsistentHash, total_nodes: usize) -> Vec<usize> {
    if total_nodes == 0 {
        return Vec::new();
    }

    let start = select_node_index(key, ring).unwrap_or(0) % total_nodes;
    (0..total_nodes)
        .map(|offset| (start + offset) % total_nodes)
        .collect()
}

fn backoff_delay(attempt: usize) -> Duration {
    Duration::from_millis(BACKOFF_BASE_MS * (1u64 << attempt.min(6)))
}

fn broadcast(message: String) {
    let mut clients = WS_CLIENTS.lock().unwrap();
    clients.retain(|client| client.connected());

    for client in clients.iter() {
        client.do_send(BroadcastText(message.clone()));
    }
}

fn request_base_url(req: &actix_web::HttpRequest) -> String {
    if let Ok(public_base) = env::var("PUBLIC_BASE_URL") {
        return public_base;
    }

    let conn_info = req.connection_info();
    format!("{}://{}", conn_info.scheme(), conn_info.host())
}

fn hostname() -> String {
    env::var("HOSTNAME").unwrap_or_else(|_| "unknown".into())
}

fn select_node_index(key: &str, ring: &ConsistentHash) -> Option<usize> {
    let node = ring.get_node(key)?;
    node.strip_prefix("redis")
        .and_then(|id| id.parse::<usize>().ok())
}

fn select_db_node<'a>(
    key: &str,
    ring: &ConsistentHash,
    db_nodes: &'a [DbShard],
) -> Option<&'a DbShard> {
    let index = select_node_index(key, ring)?;

    db_nodes.get(index)
}

async fn check_rate_limit(
    key: &str,
    ring: &ConsistentHash,
    redis_nodes: &[redisClient],
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

async fn redis_get_with_fallback(
    key: &str,
    ring: &ConsistentHash,
    redis_nodes: &[redisClient],
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

async fn redis_set_with_fallback(
    key: &str,
    value: &str,
    ring: &ConsistentHash,
    redis_nodes: &[redisClient],
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

async fn db_insert_with_retry(pool: &pgPool, id: i64, short_code: &str, long_url: &str) -> bool {
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

async fn db_fetch_with_retry(pool: &pgPool, short_code: &str) -> Result<Option<String>, ()> {
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

async fn create_short_url(
    db_nodes: web::Data<Vec<DbShard>>,
    generator: web::Data<Mutex<Snowflake>>,
    redis_nodes: web::Data<Vec<redisClient>>,
    ring: web::Data<ConsistentHash>,
    http_req: actix_web::HttpRequest,
    req: web::Json<CreateRequest>,
) -> impl Responder {
    let started_at = Instant::now();
    let long_url = req.long_url.clone();
    let rate_key = format!("rate_limit:{}", long_url);

    if let Ok(is_limited) = check_rate_limit(&rate_key, ring.get_ref(), redis_nodes.get_ref()).await
    {
        if is_limited {
            return HttpResponse::TooManyRequests().body("Rate limit exceeded");
        }
    }

    let mut r#gen = generator.lock().unwrap();
    let id = r#gen.generate();
    drop(r#gen);

    let short_code = encode_base62(id);

    let node = match ring.get_node(&short_code) {
        Some(node) => node,
        None => return HttpResponse::InternalServerError().finish(),
    };
    let route_index = select_node_index(&short_code, ring.get_ref()).unwrap_or(0);
    let redis_name = format!("redis{}", route_index);
    let db_name = format!("shard{}", route_index);

    println!(
        "[ROUTING] key={} node={} timestamp={}",
        short_code,
        node,
        unix_timestamp_secs()
    );

    let db_shard = match select_db_node(&short_code, ring.get_ref(), db_nodes.get_ref()) {
        Some(shard) => shard,
        None => return HttpResponse::InternalServerError().finish(),
    };
    let primary = &db_shard.0;
    let base_url = request_base_url(&http_req);

    if db_insert_with_retry(primary, id as i64, &short_code, &long_url).await {
        let _ = redis_set_with_fallback(
            &short_code,
            &long_url,
            ring.get_ref(),
            redis_nodes.get_ref(),
        )
        .await;

        broadcast(
            serde_json::json!({
                "type": "write",
                "status": "OK",
                "node": hostname(),
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
                "type": "write",
                "status": "ERROR",
                "node": hostname(),
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

async fn redirect_short_url(
    redis_nodes: web::Data<Vec<redisClient>>,
    db_nodes: web::Data<Vec<DbShard>>,
    ring: web::Data<ConsistentHash>,
    path: Path<String>,
) -> HttpResponse {
    let started_at = Instant::now();
    let short_code = path.into_inner();
    let node = match ring.get_node(&short_code) {
        Some(node) => node,
        None => return HttpResponse::InternalServerError().finish(),
    };
    let route_index = select_node_index(&short_code, ring.get_ref()).unwrap_or(0);
    let redis_name = format!("redis{}", route_index);
    let db_name = format!("shard{}", route_index);

    println!(
        "[ROUTING] key={} node={} timestamp={}",
        short_code,
        node,
        unix_timestamp_secs()
    );

    let db_shard = match select_db_node(&short_code, ring.get_ref(), db_nodes.get_ref()) {
        Some(shard) => shard,
        None => return HttpResponse::InternalServerError().finish(),
    };
    let primary = &db_shard.0;
    let replica = &db_shard.1;

    if let Ok(Some(long_url)) =
        redis_get_with_fallback(&short_code, ring.get_ref(), redis_nodes.get_ref()).await
    {
        broadcast(
            serde_json::json!({
                "type": "read",
                "status": "OK",
                "cache": "HIT",
                "source": "redis",
                "node": hostname(),
                "redis": redis_name,
                "db": db_name,
                "short_code": short_code,
                "latency_ms": started_at.elapsed().as_millis() as u64
            })
            .to_string(),
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
                ring.get_ref(),
                redis_nodes.get_ref(),
            )
            .await;

            broadcast(
                serde_json::json!({
                    "type": "read",
                    "status": "OK",
                    "cache": "MISS",
                    "source": source,
                    "node": hostname(),
                    "redis": redis_name,
                    "db": db_name,
                    "short_code": short_code,
                    "latency_ms": started_at.elapsed().as_millis() as u64
                })
                .to_string(),
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

            broadcast(
                serde_json::json!({
                    "type": "read",
                    "status": status,
                    "cache": "MISS",
                    "source": source,
                    "node": hostname(),
                    "redis": redis_name,
                    "db": db_name,
                    "short_code": short_code,
                    "latency_ms": started_at.elapsed().as_millis() as u64
                })
                .to_string(),
            );

            if source == "error" {
                HttpResponse::InternalServerError().finish()
            } else {
                HttpResponse::NotFound().body("Not found")
            }
        }
    }
}

struct WsSession;

impl Actor for WsSession {
    type Context = ws::WebsocketContext<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        WS_CLIENTS.lock().unwrap().push(ctx.address());
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        WS_CLIENTS
            .lock()
            .unwrap()
            .retain(|client| client.connected());
    }
}

impl Handler<BroadcastText> for WsSession {
    type Result = ();

    fn handle(&mut self, msg: BroadcastText, ctx: &mut Self::Context) {
        ctx.text(msg.0);
    }
}

impl StreamHandler<Result<ws::Message, ws::ProtocolError>> for WsSession {
    fn handle(&mut self, msg: Result<ws::Message, ws::ProtocolError>, ctx: &mut Self::Context) {
        match msg {
            Ok(ws::Message::Ping(msg)) => ctx.pong(&msg),
            Ok(ws::Message::Text(_)) => {}
            Ok(ws::Message::Binary(_)) => {}
            Ok(ws::Message::Close(_)) => ctx.stop(),
            _ => {}
        }
    }
}

async fn ws_handler(
    req: actix_web::HttpRequest,
    stream: actix_web::web::Payload,
) -> Result<actix_web::HttpResponse, actix_web::Error> {
    ws::start(WsSession, &req, stream)
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    dotenv().ok();

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

    println!("DB nodes configured: {}", db_node_urls.join(", "));

    let mut db_pools: Vec<DbShard> = Vec::new();
    for pair in db_node_urls.chunks(2) {
        let primary_url = &pair[0];
        let replica_url = &pair[1];

        let primary = pgPool::connect(primary_url.as_str())
            .await
            .unwrap_or_else(|_| panic!("DB primary connection failed for {}", primary_url));

        let replica = pgPool::connect(replica_url.as_str())
            .await
            .unwrap_or_else(|_| panic!("DB replica connection failed for {}", replica_url));

        db_pools.push((primary, replica));
    }

    for (primary, replica) in &db_pools {
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

    let db_nodes = web::Data::new(db_pools);

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

    println!("Redis nodes configured: {}", redis_node_urls.join(", "));

    let redis_clients: Vec<redisClient> = redis_node_urls
        .iter()
        .map(|url| {
            redisClient::open(url.as_str())
                .unwrap_or_else(|_| panic!("Redis connection failed for {}", url))
        })
        .collect();

    let redis_nodes = web::Data::new(redis_clients);

    if db_nodes.len() != redis_nodes.len() {
        panic!("DB shard count (DB_NODES/2) and REDIS_NODES count must match for aligned routing");
    }

    let mut ring = ConsistentHash::new(100);
    for (index, _) in redis_nodes.iter().enumerate() {
        ring.add_node(&format!("redis{}", index));
    }
    let ring = web::Data::new(ring);

    let generator = web::Data::new(Mutex::new(Snowflake::new(1)));

    HttpServer::new(move || {
        App::new()
            .wrap(Cors::permissive())
            .app_data(generator.clone())
            .app_data(db_nodes.clone())
            .app_data(redis_nodes.clone())
            .app_data(ring.clone())
            .route("/create", web::post().to(create_short_url))
            .route("/ws", web::get().to(ws_handler))
            .route("/{short_code}", web::get().to(redirect_short_url))
    })
    .bind(("127.0.0.1", 8080))?
    .run()
    .await
}
