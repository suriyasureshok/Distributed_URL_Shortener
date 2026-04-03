use serde::{Deserialize, Serialize};
use sqlx::PgPool;

#[derive(Deserialize)]
pub struct CreateRequest {
    pub long_url: String,
}

#[derive(Serialize)]
pub struct CreateResponse {
    pub short_url: String,
}

pub type DbShard = (PgPool, PgPool);
