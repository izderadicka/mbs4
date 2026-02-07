#[cfg_attr(not(feature = "openapi"), allow(unused_imports))]
use mbs4_dal::ebook::{Ebook, EbookRepository, EbookShort};
use mbs4_types::claim::Role;

use crate::{auth::token::RequiredRolesLayer, crud_api, publish_api_docs, state::AppState};
#[allow(unused_imports)]
use axum::routing::{delete, get, post, put};

#[derive(serde::Deserialize, serde::Serialize, Debug, garde::Validate)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct EbookFileInfo {
    #[garde(length(min = 1, max = 255))]
    pub uploaded_file: String,
    #[garde(range(min = 1, max = i64::MAX as u64))]
    pub size: u64,
    #[garde(length(min = 1, max = 255))]
    pub hash: String,
    #[garde(range(min = 0.0, max = 100.0))]
    pub quality: Option<f32>,
}

#[derive(serde::Deserialize, serde::Serialize, Debug, garde::Validate)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct EbookCoverInfo {
    #[garde(length(min = 1, max = 255))]
    pub cover_file: Option<String>,
    #[garde(range(min = 0))]
    pub ebook_id: i64,
    #[garde(range(min = 1))]
    pub ebook_version: i64,
}

#[derive(serde::Deserialize, Debug, garde::Validate)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct EbookMergeRequest {
    #[garde(range(min = 1))]
    ebook_id: i64,
}

publish_api_docs!(
    crud_api_extra::create,
    crud_api_extra::update,
    crud_api_extra::delete,
    crud_api_extra::ebook_sources,
    crud_api_extra::create_source_for_upload,
    crud_api_extra::update_ebook_cover,
    crud_api_extra::ebook_conversions,
    crud_api_extra::merge
);
crud_api!(Ebook, RO);

mod crud_api_extra {
    use axum::{
        extract::{Path, State},
        response::IntoResponse,
        Json,
    };
    use axum_valid::Garde;
    use http::StatusCode;
    #[cfg_attr(not(feature = "openapi"), allow(unused_imports))]
    use mbs4_dal::ebook::{CreateEbook, Ebook, EbookRepository, UpdateEbook};
    use mbs4_dal::{
        conversion::{self, ConversionRepository, EbookConversion},
        format,
        source::{self, CreateSource, SourceRepository},
    };
    use mbs4_store::{Store as _, StorePrefix, ValidPath};
    use mbs4_types::{claim::ApiClaim, utils::file_ext};
    use tracing::debug;

    use crate::{
        error::{ApiError, ApiResult},
        rest_api::ebook::{EbookCoverInfo, EbookFileInfo, EbookMergeRequest},
        state::AppState,
    };

    #[cfg_attr(feature = "openapi",  utoipa::path(post, path = "", tag = "Ebook", operation_id = "createEbook",
    responses((status = StatusCode::CREATED, description = "Created Ebook", body = Ebook))))]
    pub async fn create(
        repository: EbookRepository,
        State(state): State<AppState>,
        api_user: ApiClaim,
        Garde(Json(mut payload)): Garde<Json<CreateEbook>>,
    ) -> ApiResult<impl IntoResponse> {
        payload.created_by = Some(api_user.sub);
        let record = repository.create(payload).await?;
        if let Err(e) = state.search().index_book(record.clone(), false) {
            tracing::error!("Failed to index book: {}", e);
        }

        Ok((StatusCode::CREATED, Json(record)))
    }

    #[cfg_attr(feature = "openapi",  utoipa::path(put, path = "/{id}", tag = "Ebook", operation_id = "updateEbook",
    responses((status = StatusCode::OK, description = "Updated Ebook", body = Ebook))))]
    pub async fn update(
        Path(id): Path<i64>,
        repository: EbookRepository,
        State(state): State<AppState>,
        Garde(Json(payload)): Garde<Json<UpdateEbook>>,
    ) -> ApiResult<impl IntoResponse> {
        let record = repository.update(id, payload).await?;
        if let Err(e) = state.search().index_book(record.clone(), true) {
            tracing::error!("Failed to index book: {}", e);
        }

        Ok((StatusCode::OK, Json(record)))
    }

    #[cfg_attr(
        feature = "openapi",
        utoipa::path(delete, path = "/{id}", tag = "Ebook", operation_id = "deleteEbook",)
    )]
    pub async fn delete(
        Path(id): Path<i64>,
        repository: EbookRepository,
        sources_repository: SourceRepository,
        conversions_repository: ConversionRepository,
        State(state): State<AppState>,
    ) -> ApiResult<impl IntoResponse> {
        let cover_file = repository.get(id).await?.cover;
        let resources_files = sources_repository
            .list_for_ebook(id)
            .await?
            .into_iter()
            .map(|s| s.location);

        let conversions_files = conversions_repository
            .list_for_ebook(id)
            .await?
            .into_iter()
            .map(|c| c.location);

        repository.delete(id).await?;

        //delete cover
        if let Some(cover_file) = cover_file {
            let res = async {
                let path = ValidPath::new(cover_file)?.with_prefix(mbs4_store::StorePrefix::Books);
                state.store().delete(&path).await?;
                Ok::<_, anyhow::Error>(())
            }
            .await;
            if let Err(e) = res {
                tracing::error!("Failed to delete cover file: {}", e);
            }
        }

        // delete converted files
        for conversion in conversions_files {
            let res = async {
                let path =
                    ValidPath::new(conversion)?.with_prefix(mbs4_store::StorePrefix::Conversions);
                state.store().delete(&path).await?;
                Ok::<_, anyhow::Error>(())
            }
            .await;
            if let Err(e) = res {
                tracing::error!("Failed to delete conversion file: {}", e);
            }
        }
        // delete sources files
        for src in resources_files {
            let res: anyhow::Result<()> = async {
                let path = ValidPath::new(src)?.with_prefix(mbs4_store::StorePrefix::Books);
                state.store().delete(&path).await?;
                Ok(())
            }
            .await;
            if let Err(e) = res {
                tracing::error!("Failed to delete source file: {}", e);
            }
        }
        debug!("Sources deleted");
        if let Err(e) = state.search().delete_book(id) {
            tracing::error!("Failed to delete book: {}", e);
        }

        Ok((StatusCode::NO_CONTENT, ()))
    }

    #[cfg_attr(feature = "openapi",  utoipa::path(post, path = "/{id}/source", tag = "Ebook", operation_id = "createEbookSource",
    responses((status = StatusCode::CREATED, description = "Created Ebook Source", body = mbs4_dal::source::Source))))]
    pub async fn create_source_for_upload(
        Path(id): Path<i64>,
        ebook_repo: EbookRepository,
        source_repo: source::SourceRepository,
        format_repo: format::FormatRepository,
        State(state): State<AppState>,
        api_user: ApiClaim,
        Garde(Json(file_info)): Garde<Json<EbookFileInfo>>,
    ) -> ApiResult<impl IntoResponse> {
        let ebook = ebook_repo.get(id).await?;
        let naming_meta = ebook.naming_meta();
        let ext = file_ext(&file_info.uploaded_file).ok_or_else(|| {
            ApiError::UnprocessableRequest("Upload path without extension".into())
        })?;

        let format = format_repo.get_by_extension(&ext).await.map_err(|e| {
            ApiError::UnprocessableRequest(format!("Unsupported extension {ext}, error {e}"))
        })?;

        let to_path = naming_meta.norm_file_name(&ext);
        let from_path = ValidPath::new(file_info.uploaded_file)?.with_prefix(StorePrefix::Upload);
        let to_path = ValidPath::new(to_path)?.with_prefix(StorePrefix::Books);
        let new_path = state.store().rename(&from_path, &to_path).await?;

        let new_source = CreateSource {
            location: new_path.without_prefix(StorePrefix::Books).unwrap().into(), // safe as we used this prefix above
            ebook_id: id,
            format_id: format.id,
            size: file_info.size.try_into().unwrap(), // safe as we check max size of input
            hash: file_info.hash,
            quality: file_info.quality,
            created_by: Some(api_user.sub),
        };

        let source = source_repo.create(new_source).await?;
        Ok((StatusCode::CREATED, Json(source)))
    }

    #[cfg_attr(feature = "openapi", utoipa::path(get, path = "/{id}/source", tag = "Ebook", operation_id = "listEbookSources",
responses((status = StatusCode::OK, description = "List Ebook Sources", body = Vec<mbs4_dal::source::EbookSource>))))]
    pub async fn ebook_sources(
        Path(id): Path<i64>,
        source_repo: source::SourceRepository,
    ) -> ApiResult<impl IntoResponse> {
        let sources = source_repo.list_for_ebook(id).await?;
        Ok((StatusCode::OK, Json(sources)))
    }

    #[cfg_attr(feature = "openapi", utoipa::path(get, path = "/{id}/conversion", tag = "Ebook", operation_id = "listEbookConversions",
responses((status = StatusCode::OK, description = "List Ebook Conversions", body = Vec<EbookConversion>))))]
    pub async fn ebook_conversions(
        Path(id): Path<i64>,
        conversion_repo: conversion::ConversionRepository,
    ) -> ApiResult<impl IntoResponse> {
        let conversions: Vec<EbookConversion> = conversion_repo.list_for_ebook(id).await?;
        Ok((StatusCode::OK, Json(conversions)))
    }

    #[cfg_attr(feature = "openapi", utoipa::path(put, path = "/{id}/cover", tag = "Ebook", operation_id = "updateEbookCover",
    request_body = EbookCoverInfo,
    responses((status = StatusCode::OK, description = "Updated Ebook Cover", body = Ebook))))]
    pub async fn update_ebook_cover(
        Path(id): Path<i64>,
        repository: EbookRepository,
        State(state): State<AppState>,
        Garde(Json(payload)): Garde<Json<EbookCoverInfo>>,
    ) -> ApiResult<impl IntoResponse> {
        if id != payload.ebook_id {
            return Err(ApiError::UnprocessableRequest("Ebook id mismatch".into()));
        }

        let record = match &payload.cover_file {
            Some(cover) => {
                let ext = file_ext(cover).ok_or_else(|| {
                    ApiError::UnprocessableRequest("Upload path without extension".into())
                })?;

                let ebook = repository.get(id).await?;
                let from_path = ValidPath::new(cover)?.with_prefix(StorePrefix::Upload);
                let to_path = format!("{}/cover.{}", ebook.base_dir, ext);
                let to_path = ValidPath::new(to_path)?.with_prefix(StorePrefix::Books);
                let new_path = state.store().rename(&from_path, &to_path).await?;
                let new_path = new_path.without_prefix(StorePrefix::Books).unwrap();

                let record = repository
                    .update_cover(id, Some(new_path.into()), payload.ebook_version)
                    .await?;

                // delete icon if exists
                let icon_path = ValidPath::new(format!("{}.png", id))
                    .unwrap()
                    .with_prefix(StorePrefix::Icons);
                state
                    .store()
                    .delete(&icon_path)
                    .await
                    .inspect_err(|e| debug!("Failed to delete icon id {id}: {e}"))?;
                record
            }
            None => {
                repository
                    .update_cover(id, None, payload.ebook_version)
                    .await?
            }
        };

        Ok((StatusCode::OK, Json(record)))
    }

    #[cfg_attr(feature = "openapi", utoipa::path(put, path = "/{id}/merge", tag = "Ebook", operation_id = "mergeEbook",
    responses((status = StatusCode::OK, description = "Merge ebook to other ebook"))))]
    pub async fn merge(
        Path(id): Path<i64>,
        repository: EbookRepository,
        state: State<AppState>,
        Garde(Json(merge_request)): Garde<Json<EbookMergeRequest>>,
    ) -> ApiResult<impl IntoResponse> {
        let from_id = merge_request.ebook_id;
        repository.merge(from_id, id).await?;
        if let Err(e) = state.search().delete_book(id) {
            tracing::error!("Failed to delete book: {}", e);
        }
        Ok((StatusCode::NO_CONTENT, ()))
    }
}

pub fn router() -> axum::Router<AppState> {
    axum::Router::new()
        .route("/{id}", delete(crud_api_extra::delete))
        .layer(RequiredRolesLayer::new([Role::Admin]))
        .route("/{id}/merge", put(crud_api_extra::merge))
        .route("/", post(crud_api_extra::create))
        .route("/{id}", put(crud_api_extra::update))
        .route(
            "/{id}/source",
            post(crud_api_extra::create_source_for_upload),
        )
        .route("/{id}/cover", put(crud_api_extra::update_ebook_cover))
        .layer(RequiredRolesLayer::new([Role::Trusted, Role::Admin]))
        .route("/", get(crud_api::list))
        .route("/count", get(crud_api::count))
        .route("/{id}", get(crud_api::get))
        .route("/{id}/source", get(crud_api_extra::ebook_sources))
        .route("/{id}/conversion", get(crud_api_extra::ebook_conversions))
}
