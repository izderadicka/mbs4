use axum::routing::get;
use mbs4_dal::conversion_batch::ConversionBatchRepository;
use mbs4_types::claim::Role;

use crate::{auth::token::RequiredRolesLayer, state::AppState};

crate::repository_from_request!(ConversionBatchRepository);

#[cfg(feature = "openapi")]
pub fn api_docs() -> utoipa::openapi::OpenApi {
    use utoipa::OpenApi as _;
    crud_api_extra::ApiDocs::openapi()
}

mod crud_api_extra {
    use axum::{
        extract::{Path, Query, State},
        response::IntoResponse,
        Json,
    };
    use axum_valid::Garde;
    use http::StatusCode;
    use mbs4_dal::conversion::ConversionRepository;
    #[cfg_attr(not(feature = "openapi"), allow(unused_imports))]
    use mbs4_dal::conversion::EbookConversion;
    use mbs4_dal::conversion_batch::{ConversionBatch, ConversionBatchRepository};
    use mbs4_types::claim::{ApiClaim, Authorization as _, Role};

    use crate::{
        error::{ApiError, ApiResult},
        rest_api::{Page, Paging},
        state::AppState,
    };

    /// 404 unless the user is admin or the original creator. We don't expose
    /// "exists but forbidden" — same observable behavior as a missing batch.
    async fn get_owned_or_admin(
        id: i64,
        api_user: &ApiClaim,
        repo: &ConversionBatchRepository,
    ) -> ApiResult<ConversionBatch> {
        let batch = repo.get(id).await?;
        let is_owner = batch
            .created_by
            .as_deref()
            .map(|c| c == api_user.sub)
            .unwrap_or(false);
        if !api_user.has_role(Role::Admin) && !is_owner {
            return Err(ApiError::DeniedAccess(
                "You don't have access to this conversion batch".into(),
            ));
        }
        Ok(batch)
    }

    #[cfg(feature = "openapi")]
    #[derive(utoipa::OpenApi)]
    #[openapi(paths(list, get, list_items))]
    pub(super) struct ApiDocs;

    #[cfg_attr(
        feature = "openapi",
        utoipa::path(
            get, path = "", tag = "ConversionBatch",
            operation_id = "listConversionBatches",
            params(Paging),
            responses((status = StatusCode::OK, description = "List paginated",
                       body = crate::rest_api::Page<ConversionBatch>))
        )
    )]
    pub async fn list(
        repo: ConversionBatchRepository,
        api_user: ApiClaim,
        State(state): State<AppState>,
        Garde(Query(paging)): Garde<Query<Paging>>,
    ) -> ApiResult<impl IntoResponse> {
        let default_page_size: u32 = state.config().default_page_size;
        let page_size = paging.page_size(default_page_size);
        let listing_params = paging.into_listing_params(default_page_size)?;
        let scope: Option<&str> = if api_user.has_role(Role::Admin) {
            None
        } else {
            Some(api_user.sub.as_str())
        };
        let batch = repo.list_for_user(scope, listing_params).await?;
        Ok((StatusCode::OK, Json(Page::from_batch(batch, page_size))))
    }

    #[cfg_attr(
        feature = "openapi",
        utoipa::path(
            get, path = "/{id}", tag = "ConversionBatch",
            operation_id = "getConversionBatch",
            responses((status = StatusCode::OK, description = "Conversion batch",
                       body = ConversionBatch))
        )
    )]
    pub async fn get(
        Path(id): Path<i64>,
        repo: ConversionBatchRepository,
        api_user: ApiClaim,
    ) -> ApiResult<impl IntoResponse> {
        let batch = get_owned_or_admin(id, &api_user, &repo).await?;
        Ok((StatusCode::OK, Json(batch)))
    }

    #[cfg_attr(
        feature = "openapi",
        utoipa::path(
            get, path = "/{id}/items", tag = "ConversionBatch",
            operation_id = "listConversionBatchItems",
            responses((status = StatusCode::OK, description = "Conversion rows in batch",
                       body = Vec<EbookConversion>))
        )
    )]
    pub async fn list_items(
        Path(id): Path<i64>,
        repo: ConversionBatchRepository,
        conversion_repo: ConversionRepository,
        api_user: ApiClaim,
    ) -> ApiResult<impl IntoResponse> {
        let _ = get_owned_or_admin(id, &api_user, &repo).await?;
        let items = conversion_repo.list_for_batch(id).await?;
        Ok((StatusCode::OK, Json(items)))
    }
}

pub fn router() -> axum::Router<AppState> {
    axum::Router::new()
        .route("/", get(crud_api_extra::list))
        .route("/{id}", get(crud_api_extra::get))
        .route("/{id}/items", get(crud_api_extra::list_items))
        .layer(RequiredRolesLayer::new([Role::Trusted, Role::Admin]))
}
