use axum::{
    body::Body,
    extract::{DefaultBodyLimit, Multipart, Request, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use mbs4_dal::format::FormatRepository;
use mbs4_types::claim::Role;
use tracing::debug;

use crate::{auth::token::RequiredRolesLayer, error::ApiError, state::AppState, store::StorePrefix};

use super::{Store as _, ValidPath, UPLOAD_PATH_PREFIX};

#[cfg(feature = "openapi")]
#[derive(serde::Deserialize, utoipa::ToSchema)]
#[allow(unused)]
struct UploadForm {
    #[schema(format = Binary, content_media_type = "application/octet-stream")]
    file: String,
}

#[derive(serde::Serialize, serde::Deserialize, Debug)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct UploadInfo {
    pub final_path: String,
    pub size: u64,
    /// SHA256 hash
    pub hash: String,
    pub original_name: Option<String>,
}

impl UploadInfo {
    fn from_store_info(info: super::StoreInfo, original_name: Option<String>) -> Self {
        Self {
            final_path: info.final_path.into(),
            size: info.size,
            hash: info.hash,
            original_name,
        }
    }
}

fn upload_path(ext: &str) -> Result<ValidPath, ApiError> {
    let id = uuid::Uuid::new_v4().to_string();
    let dest_path = format!("{id}.{ext}");
    let dest_path = ValidPath::new(dest_path)?.with_prefix(StorePrefix::Upload);
    Ok(dest_path)
}

#[cfg_attr(
    feature = "openapi",
    utoipa::path(post, path = "/upload/form", tag = "File Store",
    request_body(content = UploadForm, content_type = "multipart/form-data"),
    responses(
        (status = StatusCode::CREATED, description = "Created", body = UploadInfo),
    )
    )
)]
pub async fn upload_form(
    State(state): State<AppState>,
    repository: FormatRepository,
    mut multipart: Multipart,
) -> Result<impl IntoResponse, ApiError> {
    if let Some(field) = multipart.next_field().await? {
        let file_name = field
            .file_name()
            .ok_or_else(|| ApiError::InvalidRequest("Missing file name".into()))?
            .to_string();
        let ext = std::path::Path::new(&file_name)
            .extension()
            .and_then(std::ffi::OsStr::to_str)
            .ok_or_else(|| ApiError::InvalidRequest("Missing file extension".into()))?
            .to_lowercase();

        let format = repository
            .get_by_extension(&ext)
            .await
            .map_err(|e| ApiError::InvalidRequest(format!("Invalid file extension, error: {e}")))?;
        // TODO: More check?
        let mime_type = format.mime_type;

        let dest_path = upload_path(&ext)?;
        debug!(
            "Uploading file {} to {:?}, mime {}",
            file_name, dest_path, mime_type
        );
        let info = state.store().store_stream(&dest_path, field).await?;

        let info = UploadInfo::from_store_info(info, Some(file_name));

        Ok((StatusCode::CREATED, Json(info)))
    } else {
        Err(ApiError::InvalidRequest("Missing file field".into()))
    }
}

#[cfg_attr(
    feature = "openapi",
    utoipa::path(post, path = "/upload/direct", tag = "File Store",
    
    request_body(
        description = "File data of supported mime types",
        content ((Vec<u8> = "*/*"),
        (String = "text/plain", example = "This is just test sample for swagger")
    )),
    responses(
        (status = StatusCode::CREATED, description = "Created", body = UploadInfo),
    )
    )
)]
pub async fn upload_direct(
    State(state): State<AppState>,
    repository: FormatRepository,
    request: Request,
) -> Result<impl IntoResponse, ApiError> {
    let (parts, body) = request.into_parts();
    let stream = body.into_data_stream();

    let mime = parts
        .headers
        .get("content-type")
        .ok_or_else(|| ApiError::InvalidRequest("Missing content-type header".into()))?
        .to_str()
        .map_err(|e| ApiError::InvalidRequest(e.to_string()))?;
    let format = repository
        .get_by_mime_type(mime)
        .await
        .map_err(|e| ApiError::InvalidRequest(format!("Invalid mime type, error: {e}")))?;

    let ext = format.extension;

    let path = upload_path(&ext)?;
    debug!("Uploading file to {:?}, mime {}", path, mime);
    let info = state.store().store_stream(&path, stream).await?;
    let info = UploadInfo::from_store_info(info, None);

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
    if let Some(file_name) = path.as_ref()
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

#[derive(serde::Deserialize, Debug, garde::Validate)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct RenameBody {
    #[garde(length(min = 1, max = 4096))]
    from_path: String,
    #[garde(length(min = 1, max = 4096))]
    to_path: String,
}

#[derive(serde::Serialize, Debug)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct RenameResult {
    new_path: String
}

#[cfg_attr(
    feature = "openapi",
    utoipa::path(post, path = "/rename", tag = "File Store",
    request_body = RenameBody,
    responses(
        (status = StatusCode::OK, description = "OK", body = RenameResult),
    )
    )
)]
pub async fn rename(
    State(state): State<AppState>,
    Json(body): Json<RenameBody>,
    
) -> Result<impl IntoResponse, ApiError> {
    if !body.from_path.starts_with(UPLOAD_PATH_PREFIX) {

    }
    let from_path = ValidPath::new(body.from_path)?;
    let to_path = ValidPath::new(body.to_path)?;
    let new_path = state.store().rename(&from_path, &to_path).await?;

    Ok((StatusCode::OK, Json(RenameResult { new_path: new_path.into() })))
}

pub fn store_router(limit_mb: usize) -> Router<AppState> {
    let app = Router::new()
        .route("/upload/form", post(upload_form))
        .route("/upload/direct", post(upload_direct))
        .layer(RequiredRolesLayer::new([Role::Admin, Role::Trusted]))
        .route("/download/{*path}", get(download))
        .layer(DefaultBodyLimit::max(1024 * 1024 * limit_mb));
    app
}

#[cfg(feature = "openapi")]
pub fn api_docs() -> utoipa::openapi::OpenApi {
    use utoipa::OpenApi as _;
    #[derive(utoipa::OpenApi)]
    #[openapi(paths(download, upload_direct, upload_form))]
    struct ApiDoc;
    ApiDoc::openapi()
}
