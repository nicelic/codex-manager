use axum::{
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Router,
};
use tower_http::cors::CorsLayer;
use crate::{api, server, state::AppState, tools, web};

pub async fn health() -> impl IntoResponse {
    (
        StatusCode::OK,
        [("content-type", "application/json")],
        r#"{"status":"ok"}"#,
    )
}

pub fn create_router(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(health))
        .merge(server::router())
        .merge(api::router())
        .merge(tools::router())
        .fallback(web::static_handler)
        .layer(CorsLayer::permissive())
        .with_state(state)
}
