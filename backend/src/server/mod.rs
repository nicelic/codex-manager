pub mod bridge;
pub mod events;
pub mod llmtrim_transport;
pub mod proxy;
pub mod upstream_transport;

use axum::Router;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .merge(events::router())
        .merge(proxy::router())
}
