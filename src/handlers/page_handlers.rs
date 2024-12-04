use axum::response::Html;

pub async fn health_check() -> &'static str {
    "OK"
}

pub async fn index_handler() -> Html<&'static str> {
    Html(include_str!("../templates/index.html"))
}
