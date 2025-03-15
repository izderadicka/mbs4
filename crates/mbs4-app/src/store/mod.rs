use axum::{
    extract::{Multipart, Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::post,
    Router,
};

use futures::{
    pin_mut,
    stream::{self, try_unfold},
    StreamExt,
};
use tracing::debug;

use crate::{error::ApiError, state::AppState};

pub mod error;
pub mod file_store;

pub async fn upload(
    State(state): State<AppState>,
    Path(path): Path<String>,
    mut multipart: Multipart,
) -> Result<impl IntoResponse, ApiError> {
    if let Some(mut field) = multipart.next_field().await? {
        let file_name = field
            .file_name()
            .ok_or_else(|| ApiError::InvalidRequest("Missing file name".into()))?;
        let dest_path = path + "/" + file_name;

        Ok(StatusCode::CREATED)
    } else {
        Err(ApiError::InvalidRequest("Missing file field".into()))
    }
}

pub fn store_router() -> Router<AppState> {
    let app = Router::new().route("/upload/form/{*path}", post(upload));
    app
}
