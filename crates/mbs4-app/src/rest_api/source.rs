use crate::{auth::token::RequiredRolesLayer, crud_api, publish_api_docs};
#[cfg_attr(not(feature = "openapi"), allow(unused_imports))]
use mbs4_dal::source::{CreateSource, Source, SourceRepository, SourceShort, UpdateSource};
use mbs4_types::claim::Role;

use crate::state::AppState;
#[allow(unused_imports)]
use axum::routing::{delete, get, post, put};

publish_api_docs!(extra_crud_api::delete_with_file);
crud_api!(Source);

mod extra_crud_api {
    use axum::{
        extract::{Path, State},
        response::IntoResponse,
    };
    use http::StatusCode;
    use mbs4_dal::source::SourceRepository;
    use mbs4_store::{Store, ValidPath};

    use crate::{error::ApiResult, state::AppState};

    #[cfg_attr(
        feature = "openapi",
        utoipa::path(
            delete,
            path = "/{id}",
            tag = "Source",
            operation_id = "deleteSourceWithFile"
        )
    )]
    pub async fn delete_with_file(
        Path(id): Path<i64>,
        repository: SourceRepository,
        State(state): State<AppState>,
    ) -> ApiResult<impl IntoResponse> {
        let source = repository.get(id).await?;
        repository.delete(id).await?;
        let path = ValidPath::new(source.location)?.with_prefix(mbs4_store::StorePrefix::Books);
        state.store().delete(&path).await?;

        Ok((StatusCode::NO_CONTENT, ()))
    }
}

pub fn router() -> axum::Router<AppState> {
    axum::Router::new()
        .route("/{id}", delete(extra_crud_api::delete_with_file))
        .layer(RequiredRolesLayer::new([Role::Admin]))
        .route("/", post(crud_api::create))
        .route("/{id}", put(crud_api::update))
        .layer(RequiredRolesLayer::new([Role::Trusted, Role::Admin]))
        .route("/", get(crud_api::list))
        .route("/count", get(crud_api::count))
        .route("/{id}", get(crud_api::get))
}
