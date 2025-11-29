use crate::{crud_api, publish_api_docs};
#[cfg_attr(not(feature = "openapi"), allow(unused_imports))]
use mbs4_dal::conversion::{Conversion, ConversionRepository, ConversionShort};

use crate::state::AppState;
#[allow(unused_imports)]
use axum::routing::get;

publish_api_docs!();
crud_api!(Conversion, RO);

pub fn router() -> axum::Router<AppState> {
    axum::Router::new()
        .route("/", get(crud_api::list))
        .route("/count", get(crud_api::count))
        .route("/{id}", get(crud_api::get))
}
