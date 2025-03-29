use crate::crud_api;
use mbs4_dal::language::{CreateLanguage, LanguageRepository};

use crate::state::AppState;
#[allow(unused_imports)]
use axum::routing::{delete, get, post, put};

crud_api!(LanguageRepository, CreateLanguage);

pub fn router() -> axum::Router<AppState> {
    axum::Router::new()
        .route(
            "/",
            post(crud_api::create)
                .get(crud_api::list)
                .put(crud_api::update),
        )
        .route(
            "/{id}",
            get(crud_api::get)
                .delete(crud_api::delete)
                .put(crud_api::update),
        )
}
