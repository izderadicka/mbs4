use crate::{auth::token::RequiredRolesLayer, crud_api};
use mbs4_dal::source::{CreateSource, Source, SourceRepository, SourceShort, UpdateSource};
use mbs4_types::claim::Role;

use crate::state::AppState;
#[allow(unused_imports)]
use axum::routing::{delete, get, post, put};

crud_api!(Source);

pub fn router() -> axum::Router<AppState> {
    axum::Router::new()
        .route("/{id}", delete(crud_api::delete))
        .layer(RequiredRolesLayer::new([Role::Admin]))
        .route("/", post(crud_api::create))
        .route("/{id}", put(crud_api::update))
        .layer(RequiredRolesLayer::new([Role::Trusted, Role::Admin]))
        .route("/", get(crud_api::list))
        .route("/count", get(crud_api::count))
        .route("/{id}", get(crud_api::get))
}
