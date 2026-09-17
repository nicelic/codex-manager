pub mod application;
pub mod logs;
pub mod settings;

use axum::Router;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .merge(application::router())
        .merge(logs::router())
        .merge(settings::router())
}
