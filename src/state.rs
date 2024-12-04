use rusqlite::Connection;
use std::collections::HashMap;
use std::time::Instant;
use tokio::sync::RwLock;

pub struct CacheEntry {
    pub data: String,
    pub expiry: Instant,
}

pub struct AppState {
    pub db: Connection,
    pub cache: RwLock<HashMap<String, CacheEntry>>,
}
