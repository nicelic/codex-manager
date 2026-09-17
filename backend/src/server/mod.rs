pub mod events;
pub mod proxy;

use axum::Router;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .merge(events::router())
        .merge(proxy::router())
}
