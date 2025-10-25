use axum::{body::Body, http::StatusCode, response::IntoResponse};
use mbs4_dal::format::FormatRepository;
use mbs4_store::{Store, StorePrefix, ValidPath};
use mbs4_types::utils::file_ext;

use crate::{error::ApiError, state::AppState};

pub async fn download_file(
    state: AppState,
    path: ValidPath,
    repository: FormatRepository,
    path_prefix: StorePrefix,
    as_attachment: bool,
) -> Result<impl IntoResponse, ApiError> {
    let path = path.with_prefix(path_prefix);
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

    if as_attachment {
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
    }

    Ok((StatusCode::OK, headers, body))
}
