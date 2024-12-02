use axum::{
    body::Body,
    extract::Path,
    http::Request,
    http::StatusCode,
    response::Html,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use lettre::{transport::smtp::authentication::Credentials, Message, SmtpTransport, Transport};
use rusqlite::Connection;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use tokio::sync::RwLock;
mod utils;
use crate::utils::{generate_random_string, id_to_key, split_short_key};
mod validators;
use crate::validators::{
    validate_email, validate_fallback_url, validate_url, validate_webhook_url,
};
mod schemas;
use crate::schemas::{CreateUrlRequest, CreateUrlResponse};
mod models;
use crate::models::migrate;
use dotenv::dotenv;
use std::env;

struct CacheEntry {
    data: String,
    expiry: Instant,
}

struct AppState {
    db: Connection,
    cache: RwLock<HashMap<String, CacheEntry>>,
}

#[tokio::main]
async fn main() -> Result<(), rusqlite::Error> {
    dotenv().ok();
    // DB 초기화
    let db = Connection::open("sqlite3.db")?;
    migrate(&db)?;

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

async fn send_email(email: String, code: String) -> Result<(), lettre::transport::smtp::Error> {
    let host = env::var("SERVER_HOST").unwrap_or("127.0.0.1".to_string());
    let port = env::var("SERVER_PORT").unwrap_or("3000".to_string());
    let email_body = format!(
        "http://{}:{}/v1/verify/{}\n\n이 코드는 5분 동안 유효합니다.",
        host, port, code
    );

    let from_email = env::var("EMAIL_ADDRESS").unwrap_or("lee@lou2.kr".to_string());
    let user_name = env::var("EMAIL_USER_NAME").unwrap_or("lee@lou2.kr".to_string());
    let password = env::var("EMAIL_PASSWORD").unwrap_or("PKPKKdJLXWJ5".to_string());
    let email_host = env::var("EMAIL_HOST").unwrap_or("smtppro.zoho.com".to_string());
    let email_port = env::var("EMAIL_PORT").unwrap_or("465".to_string());
    let creds = Credentials::new(user_name, password);

    let mailer = SmtpTransport::relay(&email_host)
        .unwrap()
        .credentials(creds)
        .port(email_port.parse().unwrap())
        .build();

    let email = Message::builder()
        .from(from_email.parse().unwrap())
        .to(email.parse().unwrap())
        .subject("[F-IT] 숏링크 생성을 위한 인증")
        .header(lettre::message::header::ContentType::TEXT_PLAIN)
        .body(email_body.as_bytes().to_vec())
        .unwrap();

    match mailer.send(&email) {
        Ok(_) => Ok(()),
        Err(e) => Err(e),
    }
}

async fn health_check() -> &'static str {
    "OK"
}

async fn index_handler() -> Html<&'static str> {
    Html(include_str!("templates/index.html"))
}

// URL 단축 핸들러
async fn create_short_url_handler(
    state: axum::extract::State<Arc<Mutex<AppState>>>,
    Json(payload): Json<CreateUrlRequest>,
) -> impl IntoResponse {
    // 유효성 검사
    fn validate_data(payload: &CreateUrlRequest) -> Result<(), String> {
        validate_email(&payload.email)?;
        validate_url(&payload.default_fallback_url)?;
        validate_webhook_url(&payload.webhook_url)?;
        validate_fallback_url(&payload.default_fallback_url)?;
        Ok(())
    }

    if let Err(e) = validate_data(&payload) {
        return (StatusCode::BAD_REQUEST, e).into_response();
    }

    // 고유 ID 생성
    let random_key = generate_random_string(4);
    let state = state.lock().await;
    let mut hasher = Sha256::new();
    hasher.update(format!(
        "{}{}{}{}{}",
        payload.ios_deep_link,
        payload.ios_fallback_url,
        payload.android_deep_link,
        payload.android_fallback_url,
        payload.default_fallback_url
    ));
    let hashed_value = format!("{:x}", hasher.finalize());

    // hashed_value 로 이미 있으면 그걸 그대로 반환
    match state.db.query_row(
        "SELECT id, email, random_key, is_verified FROM urls WHERE hashed_value = ?1 and is_deleted = 0",
        [&hashed_value],
        |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, i64>(3)?)),
    ) {
        Ok((id, email_address, random_key, is_verified)) => {
            if is_verified == 1 {
                return (StatusCode::CONFLICT, "이미 인증된 이메일입니다.").into_response();
            }
            let unique_key = id_to_key(id);
            let short_key = (&random_key[..2]).to_string() + &unique_key + &random_key[2..];
            // 이메일 인증 테이블에 추가
            let code = generate_random_string(8);
            let expires_at = chrono::Utc::now() + chrono::Duration::minutes(5);
            if let Err(_) = state.db.execute(
                "INSERT INTO email_auth (short_key, code, expires_at) VALUES (?1, ?2, ?3)",
                (&short_key, &code, expires_at.naive_utc().to_string()),
            ) {
                return (StatusCode::INTERNAL_SERVER_ERROR, "저장 실패").into_response();
            }
            tokio::spawn(async move {
                if let Err(e) = send_email(email_address, code).await {
                    println!("이메일 전송 실패: {}", e);
                }
            });
            let response = CreateUrlResponse {is_created: false};
            return (StatusCode::CREATED, Json(response)).into_response();
        }
        Err(_) => (),
    }
    // 기존 URL이 없는 경우 새로 생성
    match state.db.query_row(
        "INSERT INTO urls (random_key, email, ios_deep_link, ios_fallback_url, android_deep_link, android_fallback_url, default_fallback_url, hashed_value, webhook_url) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9) RETURNING id",
        (&random_key, &payload.email, &payload.ios_deep_link, &payload.ios_fallback_url, &payload.android_deep_link, &payload.android_fallback_url, &payload.default_fallback_url, &hashed_value, &payload.webhook_url),
        |row| row.get::<_, i64>(0),
    ) {
        Ok(id) => {
            let unique_key = id_to_key(id);
            // 캐시에 추가
            let short_key = (&random_key[..2]).to_string() + &unique_key + &random_key[2..];
            let code = generate_random_string(8);
            let expires_at = chrono::Utc::now() + chrono::Duration::minutes(5);
            if let Err(_) = state.db.execute(
                "INSERT INTO email_auth (short_key, code, expires_at) VALUES (?1, ?2, ?3)",
                (&short_key, &code, expires_at.naive_utc().to_string()),
            ) {
                return (StatusCode::INTERNAL_SERVER_ERROR, "저장 실패").into_response();
            }
            tokio::spawn(async move {
                if let Err(e) = send_email(payload.email, code).await {
                    println!("이메일 전송 실패: {}", e);
                }
            });
            let response = CreateUrlResponse {
                is_created: true,
            };
            (StatusCode::CREATED, Json(response)).into_response()
        }
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "저장 실패").into_response(),
    }
}

async fn verify_email_handler(
    state: axum::extract::State<Arc<Mutex<AppState>>>,
    Path(code): Path<String>,
) -> impl IntoResponse {
    let state = state.lock().await;

    // 검증 코드로 short_key 찾기
    let short_key = match state.db.query_row(
        "SELECT short_key FROM email_auth WHERE code = ?1 AND expires_at > datetime('now')",
        [&code],
        |row| row.get::<_, String>(0),
    ) {
        Ok(key) => key,
        Err(_) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Html(include_str!("templates/verify/failed.html")),
            )
                .into_response()
        }
    };

    let (url_id, random_key) = split_short_key(&short_key);

    // URL 검증 상태 업데이트
    match state.db.execute(
        "UPDATE urls SET is_verified = true WHERE random_key = ?1 AND id = ?2",
        [random_key.as_str(), &url_id.to_string()],
    ) {
        Ok(_) => {
            // 검증 완료된 코드 삭제
            let host = env::var("SERVER_HOST").unwrap_or("127.0.0.1".to_string());
            let port = env::var("SERVER_PORT").unwrap_or("3000".to_string());
            let short_url = format!("http://{}:{}/{}", host, port, short_key);
            let _ = state
                .db
                .execute("DELETE FROM email_auth WHERE code = ?1", [&code]);
            let success_html =
                include_str!("templates/verify/success.html").replace("{short_url}", &short_url);
            (StatusCode::OK, Html(success_html)).into_response()
        }
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Html(include_str!("templates/verify/error.html")),
        )
            .into_response(),
    }
}

// 리다이렉션 핸들러
async fn redirect_to_original_handler(
    Path(short_key): Path<String>,
    state: axum::extract::State<Arc<Mutex<AppState>>>,
    req: Request<Body>,
) -> impl IntoResponse {
    let state = state.lock().await;

    // 캐시 확인
    let cache = state.cache.read().await;
    if let Some(entry) = cache.get(&short_key) {
        println!("{:?}", entry.expiry);
        if entry.expiry > Instant::now() {
            // entry.data 를 분리
            let data: serde_json::Value = serde_json::from_str(&entry.data).unwrap();

            let ios_deep_link = data["ios_deep_link"].as_str().unwrap_or("");
            let ios_fallback_url = data["ios_fallback_url"].as_str().unwrap_or("");
            let android_deep_link = data["android_deep_link"].as_str().unwrap_or("");
            let android_fallback_url = data["android_fallback_url"].as_str().unwrap_or("");
            let default_fallback_url = data["default_fallback_url"].as_str().unwrap_or("");

            let webhook_url = data["webhook_url"].as_str().unwrap_or("");
            if webhook_url != "" {
                // 웹훅 보내기
                let user_agent = req.headers().get("User-Agent").unwrap().to_str().unwrap();
                let client = reqwest::Client::new();
                if let Err(e) = client
                    .post(webhook_url)
                    .json(&json!({
                        "short_key": short_key,
                        "user_agent": user_agent,
                    }))
                    .send()
                    .await
                {
                    println!("웹훅 전송 실패: {}", e);
                }
            }
            let success_html = include_str!("templates/redirect.html")
                .replace("{ios_deep_link}", &ios_deep_link)
                .replace("{ios_fallback_url}", &ios_fallback_url)
                .replace("{android_deep_link}", &android_deep_link)
                .replace("{android_fallback_url}", &android_fallback_url)
                .replace("{default_fallback_url}", &default_fallback_url);
            return (StatusCode::OK, Html(success_html)).into_response();
        }
    }
    drop(cache);

    let (url_id, request_random_key) = split_short_key(&short_key);

    // 캐시에 없으면 DB에서 조회
    match state.db.query_row(
        "SELECT random_key, ios_deep_link, ios_fallback_url, android_deep_link, android_fallback_url, default_fallback_url, webhook_url FROM urls WHERE id = ?1 and is_deleted = 0 and is_verified = 1",
        [&url_id],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, String>(3)?, row.get::<_, String>(4)?, row.get::<_, String>(5)?, row.get::<_, String>(6)?)),
    ) {
        Ok((random_key, ios_deep_link, ios_fallback_url, android_deep_link, android_fallback_url, default_fallback_url, webhook_url)) => {
            // random_key 가 일치하는지 확인
            if random_key != request_random_key {
                return (StatusCode::NOT_FOUND, "URL을 찾을 수 없습니다").into_response();
            }
            let data = json!({
                "ios_deep_link": ios_deep_link,
                "ios_fallback_url": ios_fallback_url,
                "android_deep_link": android_deep_link,
                "android_fallback_url": android_fallback_url,
                "default_fallback_url": default_fallback_url,
                "webhook_url": webhook_url
            });
            let data_string = serde_json::to_string(&data).unwrap();
            state.cache.write().await.insert(short_key.clone(), CacheEntry {
                data: data_string,
                expiry: Instant::now() + Duration::from_secs(3600),
            });

            if webhook_url != "" {
                // 웹훅 보내기
                let user_agent = req.headers().get("User-Agent").unwrap().to_str().unwrap();
                let client = reqwest::Client::new();
                if let Err(e) = client.post(webhook_url).json(&json!({
                    "short_key": short_key,
                    "user_agent": user_agent,
                })).send().await {
                    println!("웹훅 전송 실패: {}", e);
                }
            }
            let success_html = include_str!("templates/redirect.html")
                .replace("{ios_deep_link}", &ios_deep_link)
                .replace("{ios_fallback_url}", &ios_fallback_url)
                .replace("{android_deep_link}", &android_deep_link)
                .replace("{android_fallback_url}", &android_fallback_url)
                .replace("{default_fallback_url}", &default_fallback_url);
            return (StatusCode::OK, Html(success_html)).into_response()
        },
        Err(_) => (StatusCode::NOT_FOUND, "URL을 찾을 수 없습니다").into_response(),
    }
}
