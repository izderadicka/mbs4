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

publish_api_docs!(
    crud_api_extra::create,
    crud_api_extra::update,
    crud_api_extra::delete,
    crud_api_extra::ebook_sources,
    crud_api_extra::create_source_for_upload
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
        format,
        source::{self, CreateSource, Source},
    };
    use mbs4_store::{Store as _, StorePrefix, ValidPath};
    use mbs4_types::{claim::ApiClaim, utils::file_ext};

    use crate::{
        error::{ApiError, ApiResult},
        rest_api::ebook::EbookFileInfo,
        state::AppState,
    };

    #[cfg_attr(feature = "openapi",  utoipa::path(post, path = "", tag = "Ebook", operation_id = "createEbook",
    responses((status = StatusCode::CREATED, description = "Created Ebook", body = Ebook))))]
    pub async fn create(
        repository: EbookRepository,
        State(state): State<AppState>,
        Garde(Json(payload)): Garde<Json<CreateEbook>>,
    ) -> ApiResult<impl IntoResponse> {
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
        State(state): State<AppState>,
    ) -> ApiResult<impl IntoResponse> {
        repository.delete(id).await?;

        if let Err(e) = state.search().delete_book(id) {
            tracing::error!("Failed to delete book: {}", e);
        }

        Ok((StatusCode::NO_CONTENT, ()))
    }

    #[cfg_attr(feature = "openapi",  utoipa::path(post, path = "/{id}/source", tag = "Ebook", operation_id = "createEbookSource",
    responses((status = StatusCode::CREATED, description = "Created Ebook Source", body = Source))))]
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
responses((status = StatusCode::OK, description = "List Ebook Sources", body = Vec<Source>))))]
    pub async fn ebook_sources(
        Path(id): Path<i64>,
        source_repo: source::SourceRepository,
    ) -> ApiResult<impl IntoResponse> {
        let sources = source_repo.list_for_ebook(id).await?;
        Ok((StatusCode::OK, Json(sources)))
    }
}

pub fn router() -> axum::Router<AppState> {
    axum::Router::new()
        .route("/{id}", delete(crud_api_extra::delete))
        .layer(RequiredRolesLayer::new([Role::Admin]))
        .route("/", post(crud_api_extra::create))
        .route("/{id}", put(crud_api_extra::update))
        .route(
            "/{id}/source",
            post(crud_api_extra::create_source_for_upload),
        )
        .layer(RequiredRolesLayer::new([Role::Trusted, Role::Admin]))
        .route("/", get(crud_api::list))
        .route("/count", get(crud_api::count))
        .route("/{id}", get(crud_api::get))
        .route("/{id}/source", get(crud_api_extra::ebook_sources))
}
