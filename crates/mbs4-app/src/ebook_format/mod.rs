use axum::{extract::State, response::IntoResponse, routing::post, Json};
use axum_valid::Garde;
use http::StatusCode;
use mbs4_types::claim::Role;

use crate::{
    auth::token::RequiredRolesLayer, error::ApiError, events::EventMessage, state::AppState,
    store::rest_api::UploadInfo,
};

pub mod convertor;

#[derive(Debug, serde::Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct OperationTicket {
    pub id: String,
    pub created: time::OffsetDateTime,
}

#[derive(Debug, serde::Serialize)]
pub struct MetaResult {
    pub operation_id: String,
    pub created: time::OffsetDateTime,
    pub success: bool,
    pub error: Option<String>,
    pub metadata: Option<mbs4_calibre::EbookMetadata>,
}

#[cfg_attr(
    feature = "openapi",
    utoipa::path(
        post,
        path = "/extract_meta",
        tag = "Convert",
        operation_id = "extract_meta",
        request_body = UploadInfo,
        responses(
            (status = StatusCode::OK, description = "OK", body = OperationTicket),
        )
    )
)]
pub async fn get_ebook_meta(
    State(state): State<AppState>,
    Garde(Json(payload)): Garde<Json<UploadInfo>>,
) -> Result<impl IntoResponse, ApiError> {
    let event = EventMessage::message("test", payload.clone());
    state.events().send(event);
    Ok((
        StatusCode::OK,
        Json(OperationTicket {
            id: "test".to_string(),
            created: time::OffsetDateTime::now_utc(),
        }),
    ))
}

pub fn router() -> axum::Router<AppState> {
    axum::Router::new()
        .route("/extract_meta", post(get_ebook_meta))
        .layer(RequiredRolesLayer::new([Role::Trusted, Role::Admin]))
}

pub fn api_docs() -> utoipa::openapi::OpenApi {
    use utoipa::OpenApi as _;
    #[derive(utoipa::OpenApi)]
    #[openapi(paths(get_ebook_meta))]
    struct ApiDocs;
    ApiDocs::openapi()
}
