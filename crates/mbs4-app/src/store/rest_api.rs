use axum::{
    body::Body,
    extract::{DefaultBodyLimit, Multipart, Path, Request, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use mbs4_types::claim::Role;

use crate::{auth::token::RequiredRolesLayer, error::ApiError, state::AppState};

use super::{Store as _, ValidPath};

#[cfg(feature = "openapi")]
#[derive(serde::Deserialize, utoipa::ToSchema)]
#[allow(unused)]
struct UploadForm {
    #[schema(format = Binary, content_media_type = "application/octet-stream")]
    file: String,
}

#[cfg_attr(
    feature = "openapi",
    utoipa::path(post, path = "/upload/form/{path}", tag = "File Store",
    request_body(content = UploadForm, content_type = "multipart/form-data"),
    params(("path"=String, Path, description = "Path to save file")))

)]
pub async fn upload(
    State(state): State<AppState>,
    Path(path): Path<String>,
    mut multipart: Multipart,
) -> Result<impl IntoResponse, ApiError> {
    if let Some(field) = multipart.next_field().await? {
        let file_name = field
            .file_name()
            .ok_or_else(|| ApiError::InvalidRequest("Missing file name".into()))?;
        let dest_path = if path.ends_with('/') {
            path + file_name
        } else {
            path + "/" + file_name
        };
        let dest_path = ValidPath::new(dest_path)?;
        let info = state.store().store_stream(&dest_path, field).await?;

        Ok((StatusCode::CREATED, Json(info)))
    } else {
        Err(ApiError::InvalidRequest("Missing file field".into()))
    }
}

#[axum::debug_handler]
pub async fn upload_direct(
    State(state): State<AppState>,
    path: ValidPath,
    response: Request,
) -> Result<impl IntoResponse, ApiError> {
    let (_parts, body) = response.into_parts();
    let stream = body.into_data_stream();
    let info = state.store().store_stream(&path, stream).await?;

    Ok((StatusCode::CREATED, Json(info)))
}

#[cfg_attr(
    feature = "openapi",
    utoipa::path(get, path = "/download/{path}", tag = "File Store",
    params(("path"=String, Path, description = "Path to file"))),
)]
pub async fn download(
    State(state): State<AppState>,
    path: ValidPath,
) -> Result<impl IntoResponse, ApiError> {
    let store = state.store();
    let data = store.load_data(&path).await?;
    let size = store.size(&path).await?;
    let body = Body::from_stream(data);
    let mut headers = axum::http::HeaderMap::new();
    let content_type = new_mime_guess::from_path(path.as_ref()).first_or_octet_stream();

    headers.insert(
        http::header::CONTENT_TYPE,
        content_type.as_ref().parse().unwrap(), // safe as MIME is ASCII
    );
    headers.insert(
        http::header::CONTENT_LENGTH,
        size.to_string().parse().unwrap(), // safe - number is ASCII
    );
    if let Some(file_name) = path
        .as_ref()
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
        .layer(RequiredRolesLayer::new([Role::Admin, Role::Trusted]))
        .route("/download/{*path}", get(download))
        .layer(DefaultBodyLimit::max(1024 * 1024 * limit_mb));
    app
}

#[cfg(feature = "openapi")]
pub fn api_docs() -> utoipa::openapi::OpenApi {
    use utoipa::OpenApi as _;
    #[derive(utoipa::OpenApi)]
    #[openapi(paths(download, upload))]
    struct ApiDoc;
    ApiDoc::openapi()
}
