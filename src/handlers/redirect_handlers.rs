use crate::state::CacheEntry;
use crate::utils::converter::split_short_key;
use crate::AppState;
use axum::{
    body::Body, extract::Path, http::Request, http::StatusCode, response::Html,
    response::IntoResponse,
};
use serde_json::json;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

// 리다이렉션 핸들러
pub async fn redirect_to_original_handler(
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
            let head_html = data["head_html"].as_str().unwrap_or("");

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
            let success_html = include_str!("../templates/redirect.html")
                .replace("{ios_deep_link}", &ios_deep_link)
                .replace("{ios_fallback_url}", &ios_fallback_url)
                .replace("{android_deep_link}", &android_deep_link)
                .replace("{android_fallback_url}", &android_fallback_url)
                .replace("{default_fallback_url}", &default_fallback_url)
                .replace("{head_html}", &head_html);
            return (StatusCode::OK, Html(success_html)).into_response();
        }
    }
    drop(cache);

    let (url_id, request_random_key) = split_short_key(&short_key);

    // 캐시에 없으면 DB에서 조회
    match state.db.query_row(
        "SELECT random_key, ios_deep_link, ios_fallback_url, android_deep_link, android_fallback_url, default_fallback_url, webhook_url, head_html FROM urls WHERE id = ?1 and is_deleted = 0 and is_verified = 1",
        [&url_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, String>(3)?, row.get::<_, String>(4)?, row.get::<_, String>(5)?, row.get::<_, String>(6)?, row.get::<_, String>(7)?)),
        ) {
        Ok((random_key, ios_deep_link, ios_fallback_url, android_deep_link, android_fallback_url, default_fallback_url, webhook_url, head_html)) => {
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
                "webhook_url": webhook_url,
                "head_html": head_html
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
            let success_html = include_str!("../templates/redirect.html")
                .replace("{ios_deep_link}", &ios_deep_link)
                .replace("{ios_fallback_url}", &ios_fallback_url)
                .replace("{android_deep_link}", &android_deep_link)
                .replace("{android_fallback_url}", &android_fallback_url)
                .replace("{default_fallback_url}", &default_fallback_url)
                .replace("{head_html}", &head_html);
            return (StatusCode::OK, Html(success_html)).into_response()
        },
        Err(_) => (StatusCode::NOT_FOUND, "URL을 찾을 수 없습니다").into_response(),
    }
}
