use crate::{auth::token::RequiredRolesLayer, crud_api, publish_api_docs};
use axum::routing::delete;
#[cfg_attr(not(feature = "openapi"), allow(unused_imports))]
use mbs4_dal::conversion::{Conversion, ConversionRepository, ConversionShort};
use mbs4_types::claim::Role;

use crate::state::AppState;
#[allow(unused_imports)]
use axum::routing::get;

publish_api_docs!(extra_crud_api::delete);
crud_api!(Conversion, RO);

mod extra_crud_api {
    use axum::{
        extract::{Path, State},
        response::IntoResponse,
    };
    use http::StatusCode;
    use mbs4_dal::conversion::ConversionRepository;
    use mbs4_store::{Store, ValidPath};

    use crate::{error::ApiResult, state::AppState};

    #[cfg_attr(
        feature = "openapi",
        utoipa::path(
            delete,
            path = "/{id}",
            tag = "Conversion",
            operation_id = "deleteConversion"
        )
    )]
    pub async fn delete(
        Path(id): Path<i64>,
        repository: ConversionRepository,
        State(state): State<AppState>,
    ) -> ApiResult<impl IntoResponse> {
        let conversion = repository.get(id).await?;
        repository.delete(id).await?;
        let path =
            ValidPath::new(conversion.location)?.with_prefix(mbs4_store::StorePrefix::Conversions);
        state.store().delete(&path).await?;

        Ok((StatusCode::NO_CONTENT, ()))
    }
}

pub fn router() -> axum::Router<AppState> {
    axum::Router::new()
        .route("/", get(crud_api::list))
        .route("/count", get(crud_api::count))
        .route("/{id}", get(crud_api::get))
        .route("/{id}", delete(extra_crud_api::delete))
        .layer(RequiredRolesLayer::new([Role::Trusted, Role::Admin]))
}
