use crate::utils::converter::split_short_key;
use crate::AppState;
use axum::{extract::Path, http::StatusCode, response::Html, response::IntoResponse};
use std::env;
use std::sync::Arc;
use tokio::sync::Mutex;

pub async fn verify_email_handler(
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
                Html(include_str!("../templates/verify/failed.html")),
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
                include_str!("../templates/verify/success.html").replace("{short_url}", &short_url);
            (StatusCode::OK, Html(success_html)).into_response()
        }
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Html(include_str!("../templates/verify/error.html")),
        )
            .into_response(),
    }
}
