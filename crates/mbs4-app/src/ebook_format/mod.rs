use axum::{extract::State, response::IntoResponse, routing::post, Json};
use axum_valid::Garde;
use http::StatusCode;
use mbs4_dal::{format, source};
use mbs4_store::ValidPath;
use mbs4_types::claim::{ApiClaim, Role};

use crate::{
    auth::token::RequiredRolesLayer, error::ApiError, state::AppState, store::rest_api::UploadInfo,
};

pub mod convertor;

#[derive(Debug, serde::Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct OperationTicket {
    pub id: String,
    pub created: time::OffsetDateTime,
}

#[derive(Debug, serde::Serialize)]
pub struct ErrorResult {
    pub operation_id: String,
    pub created: time::OffsetDateTime,
    pub error: String,
}

macro_rules! result_struct {
    ($name:ident, $field:ident, $field_type:ty) => {
        #[derive(Debug, serde::Serialize)]
        pub struct $name {
            pub operation_id: String,
            pub created: time::OffsetDateTime,
            pub success: bool,
            pub error: Option<String>,
            pub $field: $field_type,
        }
    };

    ($name:ident) => {};
}

result_struct!(MetaResult, metadata, mbs4_calibre::EbookMetadata);
result_struct!(
    ConversionResult,
    conversion,
    mbs4_dal::conversion::Conversion
);

#[derive(Clone, serde::Serialize, serde::Deserialize, Debug, garde::Validate)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[garde(allow_unvalidated)]
pub struct ConversionRequest {
    pub source_id: i64,
    pub to_format_id: i64,
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
    let to_path = ValidPath::new(payload.final_path)?.with_prefix(mbs4_store::StorePrefix::Upload);
    let operation_id = uuid::Uuid::new_v4().to_string();
    state
        .convertor()
        .extract_meta(crate::ebook_format::convertor::MetadataRequest {
            operation_id: operation_id.clone(),
            file_path: to_path,
            extract_cover: true,
        })
        .await;
    Ok((
        StatusCode::OK,
        Json(OperationTicket {
            id: operation_id,
            created: time::OffsetDateTime::now_utc(),
        }),
    ))
}

#[cfg_attr(
    feature = "openapi",
    utoipa::path(
        post,
        path = "/convert",
        tag = "Convert",
        operation_id = "convert_source",
        request_body = ConversionRequest,
        responses(
            (status = StatusCode::OK, description = "OK", body = OperationTicket),
        )
    )
)]
pub async fn convert_source(
    State(state): State<AppState>,
    source_repositry: source::SourceRepository,
    format_repositry: format::FormatRepository,
    api_user: ApiClaim,
    Garde(Json(payload)): Garde<Json<ConversionRequest>>,
) -> Result<impl IntoResponse, ApiError> {
    let source = source_repositry.get(payload.source_id).await?;
    let to_format = format_repositry.get(payload.to_format_id).await?;

    let file_path = ValidPath::new(source.location)?.with_prefix(mbs4_store::StorePrefix::Books);

    let operation_id = uuid::Uuid::new_v4().to_string();
    state
        .convertor()
        .convert(crate::ebook_format::convertor::ConversionRequest {
            operation_id: operation_id.clone(),
            file_path,
            to_ext: to_format.extension,
            source_id: source.id,
            user: api_user.sub,
        })
        .await;
    Ok((
        StatusCode::OK,
        Json(OperationTicket {
            id: operation_id,
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
