use actix::{Actor, ActorContext, Addr, AsyncContext, Handler, Message, StreamHandler};
use actix_web::{Error, HttpRequest, HttpResponse, web};
use actix_web_actors::ws;
use futures_util::StreamExt;
use once_cell::sync::Lazy;
use redis::AsyncCommands;
use redis::Client as RedisClient;
use std::env;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::time::{sleep, timeout};

use crate::reliability::IO_TIMEOUT_MS;

static WS_CLIENTS: Lazy<Mutex<Vec<Addr<WsSession>>>> = Lazy::new(|| Mutex::new(Vec::new()));
static EVENT_BUS_CLIENT: Lazy<Mutex<Option<RedisClient>>> = Lazy::new(|| Mutex::new(None));
static EVENT_SEQUENCE: AtomicU64 = AtomicU64::new(1);
static NODE_IDENTITY: Lazy<String> = Lazy::new(resolve_node_identity);
const EVENT_CHANNEL: &str = "shortener:events";

#[derive(Message)]
#[rtype(result = "()")]
struct BroadcastText(String);

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

pub async fn ws_handler(req: HttpRequest, stream: web::Payload) -> Result<HttpResponse, Error> {
    ws::start(WsSession, &req, stream)
}

pub fn request_base_url(req: &HttpRequest) -> String {
    if let Ok(public_base) = env::var("PUBLIC_BASE_URL") {
        return public_base;
    }

    let conn_info = req.connection_info();
    format!("{}://{}", conn_info.scheme(), conn_info.host())
}

fn resolve_node_identity() -> String {
    env::var("NODE_ID")
        .or_else(|_| env::var("API_NODE_ID"))
        .or_else(|_| env::var("HOSTNAME"))
        .or_else(|_| env::var("COMPUTERNAME"))
        .unwrap_or_else(|_| "api-local".to_string())
}

pub fn node_identity() -> &'static str {
    NODE_IDENTITY.as_str()
}

pub fn next_event_id() -> String {
    format!(
        "{}-{}",
        node_identity(),
        EVENT_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    )
}

fn broadcast_local(message: &str) {
    let mut clients = WS_CLIENTS.lock().unwrap();
    clients.retain(|client| client.connected());

    for client in clients.iter() {
        client.do_send(BroadcastText(message.to_string()));
    }
}

pub fn broadcast(message: String) {
    let redis_for_pub = EVENT_BUS_CLIENT.lock().unwrap().clone();

    if let Some(redis) = redis_for_pub {
        tokio::spawn(async move {
            let publish_result = timeout(
                Duration::from_millis(IO_TIMEOUT_MS),
                redis.get_async_connection(),
            )
            .await;

            match publish_result {
                Ok(Ok(mut conn)) => {
                    let publish_result = timeout(
                        Duration::from_millis(IO_TIMEOUT_MS),
                        conn.publish::<_, _, i32>(EVENT_CHANNEL, message.clone()),
                    )
                    .await;

                    match publish_result {
                        Ok(Ok(_)) => {}
                        _ => broadcast_local(&message),
                    }
                }
                _ => broadcast_local(&message),
            }
        });
    } else {
        broadcast_local(&message);
    }
}

pub fn initialize_event_bus(redis: RedisClient) {
    *EVENT_BUS_CLIENT.lock().unwrap() = Some(redis.clone());
    start_event_bus_listener(redis);
}

fn start_event_bus_listener(redis: RedisClient) {
    tokio::spawn(async move {
        loop {
            let connection = timeout(
                Duration::from_millis(IO_TIMEOUT_MS),
                redis.get_async_connection(),
            )
            .await;

            let mut pubsub = match connection {
                Ok(Ok(conn)) => conn.into_pubsub(),
                _ => {
                    sleep(Duration::from_secs(1)).await;
                    continue;
                }
            };

            let subscribe_result = timeout(
                Duration::from_millis(IO_TIMEOUT_MS),
                pubsub.subscribe(EVENT_CHANNEL),
            )
            .await;

            match subscribe_result {
                Ok(Ok(())) => {}
                _ => {
                    sleep(Duration::from_secs(1)).await;
                    continue;
                }
            }

            loop {
                let mut messages = pubsub.on_message();
                let message_result = timeout(Duration::from_secs(30), messages.next()).await;

                match message_result {
                    Ok(Some(message)) => {
                        if let Ok(payload) = message.get_payload::<String>() {
                            broadcast_local(&payload);
                        }
                    }
                    _ => break,
                }
            }
        }
    });
}
