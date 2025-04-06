use crate::{auth::token::RequiredRolesLayer, crud_api};
use mbs4_dal::language::{CreateLanguage, LanguageRepository};

use crate::state::AppState;
#[allow(unused_imports)]
use axum::routing::{delete, get, post, put};

crud_api!(LanguageRepository, CreateLanguage);

pub fn router() -> axum::Router<AppState> {
    axum::Router::new()
        .route("/", post(crud_api::create))
        .route("/{id}", delete(crud_api::delete).put(crud_api::update))
        .layer(RequiredRolesLayer::new(["admin"]))
        .route("/", get(crud_api::list))
        .route("/count", get(crud_api::count))
        .route("/all", get(crud_api::list_all))
        .route("/{id}", get(crud_api::get))
}
