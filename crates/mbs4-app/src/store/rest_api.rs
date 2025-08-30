use axum::{
    body::Body,
    extract::{DefaultBodyLimit, Multipart, Request, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use futures::TryStreamExt as _;
use mbs4_dal::format::FormatRepository;
use mbs4_store::{error::StoreError, upload_path, Store, StoreInfo, StorePrefix};
use mbs4_types::{claim::Role, utils::file_ext};
use tracing::debug;

use crate::{auth::token::RequiredRolesLayer, error::ApiError, state::AppState};

use super::ValidPath;

#[cfg(feature = "openapi")]
#[derive(serde::Deserialize, utoipa::ToSchema)]
#[allow(unused)]
struct UploadForm {
    #[schema(value_type = String, format = Binary, content_media_type = "application/octet-stream")]
    file: String,
}

#[derive(Clone, serde::Serialize, serde::Deserialize, Debug, garde::Validate)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct UploadInfo {
    #[garde(length(min = 1, max = 255))]
    pub final_path: String,
    #[garde(range(min = 1))]
    pub size: u64,
    /// SHA256 hash
    #[garde(length(min = 64, max = 64))]
    pub hash: String,
    #[garde(length(min = 1, max = 255))]
    pub original_name: Option<String>,
}

impl UploadInfo {
    fn from_store_info(info: StoreInfo, original_name: Option<String>) -> Self {
        Self {
            // safe due to logic -  always used with this prefix
            final_path: info
                .final_path
                .without_prefix(StorePrefix::Upload)
                .unwrap()
                .into(),
            size: info.size,
            hash: info.hash,
            original_name,
        }
    }
}

#[cfg_attr(
    feature = "openapi",
    utoipa::path(post, path = "/upload/form", tag = "File Store", operation_id = "uploadForm",
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
        let ext = file_ext(&file_name)
            .ok_or_else(|| ApiError::UnprocessableRequest("Missing file extension".into()))?;

        let format = repository.get_by_extension(&ext).await.map_err(|e| {
            ApiError::UnprocessableRequest(format!("Invalid file extension, error: {e}"))
        })?;
        // TODO: More check?
        let mime_type = format.mime_type;

        let dest_path = upload_path(&ext)?;
        debug!(
            "Uploading file {} to {:?}, mime {}",
            file_name, dest_path, mime_type
        );
        let stream = field.map_err(|e| {
            StoreError::StreamError(format!("Error reading multipart field in request: {e}"))
        });
        let info = state.store().store_stream(&dest_path, stream).await?;

        let info = UploadInfo::from_store_info(info, Some(file_name));

        Ok((StatusCode::CREATED, Json(info)))
    } else {
        Err(ApiError::InvalidRequest("Missing file field".into()))
    }
}

#[cfg_attr(
    feature = "openapi",
    utoipa::path(post, path = "/upload/direct", tag = "File Store", operation_id = "uploadDirect",
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
        .map_err(|e| ApiError::UnprocessableRequest(format!("Invalid mime type, error: {e}")))?;

    let ext = format.extension;

    let path = upload_path(&ext)?;
    debug!("Uploading file to {:?}, mime {}", path, mime);
    let stream =
        stream.map_err(|e| StoreError::StreamError(format!("Error reading request body: {e}")));
    let info = state.store().store_stream(&path, stream).await?;
    let info = UploadInfo::from_store_info(info, None);

    Ok((StatusCode::CREATED, Json(info)))
}

#[cfg_attr(
    feature = "openapi",
    utoipa::path(get, path = "/download/{path}", tag = "File Store", operation_id = "download",
    params(("path"=String, Path, description = "Path to file"))),
)]
pub async fn download(
    State(state): State<AppState>,
    path: ValidPath,
    repository: FormatRepository,
) -> Result<impl IntoResponse, ApiError> {
    let path = path.with_prefix(StorePrefix::Books);
    let store = state.store();
    let data = store.load_data(&path).await?;
    let size = store.size(&path).await?;
    let body = Body::from_stream(data);
    let mut headers = axum::http::HeaderMap::new();

    let ext = file_ext(path.as_ref());

    let mut content_type = None;
    if let Some(ext) = ext.as_ref() {
        content_type = repository
            .get_by_extension(ext)
            .await
            .ok()
            .map(|f| f.mime_type);
    }

    // .and_then(|s| repository.get_by_extension(&s).await.ok()).map(|f| f.mime_type).unwrap_or_else(|| "application/octet-stream".to_string());
    let mime = content_type
        .or_else(|| {
            ext.as_ref()
                .and_then(|ext| new_mime_guess::from_ext(ext).first().map(|m| m.to_string()))
        })
        .unwrap_or_else(|| "application/octet-stream".to_string());

    headers.insert(
        http::header::CONTENT_TYPE,
        mime.parse().unwrap(), // safe as MIME is ASCII
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

#[derive(serde::Deserialize, Debug, garde::Validate)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct RenameBody {
    #[garde(length(min = 1, max = 4096))]
    from_path: String,
    #[garde(length(min = 1, max = 4096))]
    to_path: String,
}

#[derive(serde::Serialize, serde::Deserialize, Debug)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct RenameResult {
    pub final_path: String,
}

#[cfg_attr(
    feature = "openapi",
    utoipa::path(post, path = "/move/upload", tag = "File Store", operation_id = "moveUpload",
    request_body = RenameBody,
    responses(
        (status = StatusCode::OK, description = "OK", body = RenameResult),
    )
    )
)]
pub async fn move_upload(
    State(state): State<AppState>,
    Json(body): Json<RenameBody>,
) -> Result<impl IntoResponse, ApiError> {
    let from_path = ValidPath::new(body.from_path)?.with_prefix(StorePrefix::Upload);
    let to_path = ValidPath::new(body.to_path)?.with_prefix(StorePrefix::Books);
    let new_path = state.store().rename(&from_path, &to_path).await?;

    // safe - we set same prefix above
    Ok((
        StatusCode::OK,
        Json(RenameResult {
            final_path: new_path.without_prefix(StorePrefix::Books).unwrap().into(),
        }),
    ))
}

pub fn router(limit_mb: usize) -> Router<AppState> {
    let app = Router::new()
        .route("/upload/form", post(upload_form))
        .route("/upload/direct", post(upload_direct))
        .route("/move/upload", post(move_upload))
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
