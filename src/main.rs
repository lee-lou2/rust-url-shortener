mod handlers;
mod models;
mod schemas;
mod state;
mod utils;
mod validators;
use crate::handlers::{
    page_handlers::*, redirect_handlers::*, short_url_handlers::*, verify_handlers::*,
};
use crate::models::migrate::db_init;
use crate::state::AppState;
use axum::{
    routing::{get, post},
    Router,
};
use dotenv::dotenv;
use rusqlite::Connection;
use std::collections::HashMap;
use std::env;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::sync::RwLock;

#[tokio::main]
async fn main() -> Result<(), rusqlite::Error> {
    dotenv().ok();
    // DB 초기화
    let db = Connection::open("sqlite3.db")?;
    db_init(&db)?;

    let state = Arc::new(Mutex::new(AppState {
        db,
        cache: RwLock::new(HashMap::new()),
    }));

    // 라우터 설정
    let app = Router::new()
        .route("/", get(index_handler))
        .route("/health", get(health_check))
        .route("/v1/urls", post(create_short_url_handler))
        .route("/v1/verify/:code", get(verify_email_handler))
        .route("/:short_key", get(redirect_to_original_handler))
        .with_state(state);

    // 서버 시작
    let host = env::var("SERVER_HOST").unwrap_or("127.0.0.1".to_string());
    let port = env::var("SERVER_PORT").unwrap_or("3000".to_string());
    let listener = tokio::net::TcpListener::bind(format!("{}:{}", host, port))
        .await
        .unwrap();
    println!("Server running on http://{}:{}", host, port);
    axum::serve(listener, app).await.unwrap();
    Ok(())
}
