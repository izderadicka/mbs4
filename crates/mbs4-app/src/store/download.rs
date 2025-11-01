use axum::{
    body::Body,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
};
use mbs4_dal::{ebook::EbookRepository, format::FormatRepository};
use mbs4_store::{error::StoreResult, Store, StorePrefix, ValidPath};
use mbs4_types::utils::file_ext;
use tokio::fs;

use crate::{error::ApiError, state::AppState};

fn icon_path(icon_id: i64) -> StoreResult<ValidPath> {
    Ok(ValidPath::new(format!("{icon_id}.png"))?.with_prefix(StorePrefix::Icons))
}

async fn create_and_save_icon(
    state: &AppState,
    cover: String,
    icon_id: i64,
) -> Result<Vec<u8>, ApiError> {
    let store = state.store();
    let cover_path = store.local_path(&ValidPath::new(cover)?.with_prefix(StorePrefix::Books));

    if let Some(path) = cover_path {
        if let Ok(true) = fs::try_exists(&path).await {
            let icon_data = mbs4_image::scale_icon(path)?;
            store
                .store_data_overwrite(&icon_path(icon_id)?, &icon_data)
                .await?;
            return Ok(icon_data);
        }
    }

    Err(ApiError::InternalError("Error creating icon".into()))
}

pub async fn get_icon(
    state: AppState,
    icon_id: i64,
    repository: EbookRepository,
) -> Result<impl IntoResponse, ApiError> {
    let store = state.store();
    let path = icon_path(icon_id)?;

    let exiting_stream = store.load_data(&path).await;

    let body = match exiting_stream {
        Ok(stream) => {
            let body = Body::from_stream(stream);
            body
        }
        Err(e) if matches!(e, mbs4_store::error::StoreError::NotFound(_)) => {
            match repository.get(icon_id).await {
                Ok(ebook) => match ebook.cover {
                    Some(cover) => {
                        let icon_data = create_and_save_icon(&state, cover, icon_id).await?;
                        Body::from(icon_data)
                    }
                    None => return Err(ApiError::ResourceNotFound("Cover".to_string())),
                },
                Err(e) if matches!(e, mbs4_dal::error::Error::RecordNotFound(_)) => {
                    return Err(ApiError::ResourceNotFound("Cover".to_string()));
                }

                Err(e) => return Err(e.into()),
            }
        }
        Err(e) => return Err(e.into()),
    };

    let mut headers = HeaderMap::new();
    let mime = "image/png";
    headers.insert(http::header::CONTENT_TYPE, mime.parse().unwrap());

    Ok((StatusCode::OK, headers, body))
}

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
    let mut headers = HeaderMap::new();

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
