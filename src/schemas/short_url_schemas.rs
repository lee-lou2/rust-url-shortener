use serde::{Deserialize, Serialize};

// URL 요청 구조체
#[derive(Deserialize)]
pub struct CreateUrlRequest {
    pub email: String,
    #[serde(rename = "iosDeepLink")]
    pub ios_deep_link: String,
    #[serde(rename = "iosFallbackUrl")]
    pub ios_fallback_url: String,
    #[serde(rename = "androidDeepLink")]
    pub android_deep_link: String,
    #[serde(rename = "androidFallbackUrl")]
    pub android_fallback_url: String,
    #[serde(rename = "defaultFallbackUrl")]
    pub default_fallback_url: String,
    #[serde(rename = "webhookUrl")]
    pub webhook_url: String,
    #[serde(rename = "headHtml")]
    pub head_html: String,
}

// URL 응답 구조체
#[derive(Serialize)]
pub struct CreateUrlResponse {
    pub is_created: bool,
}
