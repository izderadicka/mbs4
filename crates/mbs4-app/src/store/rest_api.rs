use axum::{
    extract::{DefaultBodyLimit, Multipart, Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::post,
    Router,
};

use crate::{error::ApiError, state::AppState};

use super::Store as _;

pub async fn upload(
    State(state): State<AppState>,
    Path(path): Path<String>,
    mut multipart: Multipart,
) -> Result<impl IntoResponse, ApiError> {
    if let Some(field) = multipart.next_field().await? {
        let file_name = field
            .file_name()
            .ok_or_else(|| ApiError::InvalidRequest("Missing file name".into()))?;
        let dest_path = path + "/" + file_name;

        state.store().store_stream(&dest_path, field).await?;

        Ok(StatusCode::CREATED)
    } else {
        Err(ApiError::InvalidRequest("Missing file field".into()))
    }
}

pub fn store_router(limit_mb: usize) -> Router<AppState> {
    let app = Router::new()
        .route("/upload/form/{*path}", post(upload))
        .layer(DefaultBodyLimit::max(1024 * 1024 * limit_mb));
    app
}
