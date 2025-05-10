use mbs4_dal::ebook::{CreateEbook, EbookRepository, UpdateEbook};
use mbs4_types::claim::Role;

use crate::{auth::token::RequiredRolesLayer, crud_api, state::AppState};
#[allow(unused_imports)]
use axum::routing::{delete, get, post, put};
// crate::repository_from_request!(EbookRepository);

crud_api!(EbookRepository, CreateEbook, UpdateEbook);

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
