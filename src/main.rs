use actix_cors::Cors;
use actix_web::{App, HttpServer, web};
use dotenvy::dotenv;

mod bootstrap;
mod cache_metrics;
mod domain;
mod events;
mod handlers;
mod models;
mod reliability;
mod state;
mod storage;

use crate::bootstrap::{
    build_ring, ensure_db_schema, init_db_pools, init_redis_clients, load_db_node_urls,
    load_redis_node_urls,
};
use crate::events::{initialize_event_bus, ws_handler};
use crate::handlers::{create_short_url, redirect_short_url};
use crate::state::AppState;

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    dotenv().ok();

    let db_node_urls = load_db_node_urls();
    println!("DB nodes configured: {}", db_node_urls.join(", "));

    let db_pools = init_db_pools(&db_node_urls).await;
    ensure_db_schema(&db_pools).await;

    let redis_node_urls = load_redis_node_urls();
    println!("Redis nodes configured: {}", redis_node_urls.join(", "));

    let redis_clients = init_redis_clients(&redis_node_urls);

    if db_pools.len() != redis_clients.len() {
        panic!("DB shard count (DB_NODES/2) and REDIS_NODES count must match for aligned routing");
    }

    let ring = build_ring(redis_clients.len());
    let app_state = web::Data::new(AppState::new(db_pools, redis_clients, ring));

    if let Some(redis_for_bus) = app_state.redis_nodes.first().cloned() {
        initialize_event_bus(redis_for_bus);
    }

    HttpServer::new(move || {
        App::new()
            .wrap(Cors::permissive())
            .app_data(app_state.clone())
            .route("/create", web::post().to(create_short_url))
            .route("/ws", web::get().to(ws_handler))
            .route("/{short_code}", web::get().to(redirect_short_url))
    })
    .bind(("127.0.0.1", 8080))?
    .run()
    .await
}
