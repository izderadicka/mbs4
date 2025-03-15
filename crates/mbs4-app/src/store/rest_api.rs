use axum::{
    body::Body,
    extract::{DefaultBodyLimit, Multipart, Path, Request, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};

use crate::{error::ApiError, state::AppState};

use super::{error::StoreError, Store as _};

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

        let info = state.store().store_stream(&dest_path, field).await?;

        Ok((StatusCode::CREATED, Json(info)))
    } else {
        Err(ApiError::InvalidRequest("Missing file field".into()))
    }
}

#[axum::debug_handler]
pub async fn upload_direct(
    State(state): State<AppState>,
    Path(path): Path<String>,
    response: Request,
) -> Result<impl IntoResponse, ApiError> {
    let (_parts, body) = response.into_parts();
    let stream = body.into_data_stream();
    let info = state.store().store_stream(&path, stream).await?;

    Ok((StatusCode::CREATED, Json(info)))
}

pub async fn download(
    State(state): State<AppState>,
    Path(path): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    let store = state.store();
    let data = match store.load_data(&path).await {
        Ok(data) => data,
        Err(StoreError::NotFound) => return Err(ApiError::ResourceNotFound(path)),
        Err(e) => return Err(ApiError::from(e)),
    };
    let size = store.size(&path).await?;
    let body = Body::from_stream(data);
    let mut headers = axum::http::HeaderMap::new();
    let content_type = new_mime_guess::from_path(&path).first_or_octet_stream();

    headers.insert(
        http::header::CONTENT_TYPE,
        content_type.as_ref().parse().unwrap(), // safe as MIME is ASCII
    );
    headers.insert(
        http::header::CONTENT_LENGTH,
        size.to_string().parse().unwrap(), // safe - number is ASCII
    );
    if let Some(file_name) = path
        .split('/')
        .last()
        .filter(|s| s.chars().all(|c| c.is_ascii() && !c.is_ascii_control()))
    {
        headers.insert(
            http::header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{file_name}\"")
                .parse()
                .unwrap(), // should be safe as we check ASCII
        );
    }

    Ok((StatusCode::OK, headers, body))
}

pub fn store_router(limit_mb: usize) -> Router<AppState> {
    let app = Router::new()
        .route("/upload/form/{*path}", post(upload))
        .route("/upload/direct/{*path}", post(upload_direct))
        .route("/download/{*path}", get(download))
        .layer(DefaultBodyLimit::max(1024 * 1024 * limit_mb));
    app
}
