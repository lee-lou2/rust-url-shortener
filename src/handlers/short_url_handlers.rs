use axum::{
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use lettre::{transport::smtp::authentication::Credentials, Message, SmtpTransport, Transport};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use tokio::sync::Mutex;
use crate::utils::generator::generate_random_string;
use crate::utils::converter::id_to_key;
use crate::validators::validate_url::{validate_email, validate_fallback_url, validate_url, validate_webhook_url};
use crate::schemas::short_url_schemas::{CreateUrlRequest, CreateUrlResponse};
use scraper::Html as ScraperHtml;
use std::env;
use crate::state::AppState;


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

fn extract_head_html(html: &str) -> String {
    let document = ScraperHtml::parse_document(html);
    let selector = scraper::Selector::parse("head").unwrap();
    if let Some(head) = document.select(&selector).next() {
        head.html()
    } else {
        String::new()
    }
}

// URL 단축 핸들러
pub async fn create_short_url_handler(
    state: axum::extract::State<Arc<Mutex<AppState>>>,
    Json(payload): Json<CreateUrlRequest>,
) -> impl IntoResponse {
    let state_clone = state.clone();
    let state = state.lock().await;
    
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
        "INSERT INTO urls (random_key, email, ios_deep_link, ios_fallback_url, android_deep_link, android_fallback_url, default_fallback_url, hashed_value, webhook_url, head_html) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10) RETURNING id",
        (&random_key, &payload.email, &payload.ios_deep_link, &payload.ios_fallback_url, &payload.android_deep_link, &payload.android_fallback_url, &payload.default_fallback_url, &hashed_value, &payload.webhook_url, &payload.head_html),
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
                // 클론된 state 사용
                let state = state_clone.lock().await;
                
                if let Err(e) = send_email(payload.email, code).await {
                    println!("이메일 전송 실패: {}", e);
                }

                if payload.head_html.is_empty() {
                    let client = reqwest::Client::new();
                    match client.get(&payload.default_fallback_url).send().await {
                        Ok(response) => {
                            if let Ok(html) = response.text().await {
                                let head_html = extract_head_html(&html);
                                let _ = state.db.execute(
                                    "UPDATE urls SET head_html = ?1 WHERE id = ?2",
                                    (&head_html, &id)
                                );
                            }
                        }
                        Err(e) => println!("헤드 HTML 가져오기 실패: {}", e),
                    }
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
