pub mod gortex;
pub mod llmtrim;
pub mod rtk;
pub mod snip;

use axum::Router;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .merge(rtk::router())
        .merge(snip::router())
        .merge(llmtrim::router())
        .merge(gortex::router())
}
