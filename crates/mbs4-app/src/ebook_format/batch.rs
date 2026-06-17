use axum::{extract::State, response::IntoResponse, Json};
use axum_valid::Garde;
use http::StatusCode;
use mbs4_dal::{
    author::AuthorRepository,
    bookshelf::BookshelfRepository,
    conversion_batch::{ConversionBatchEntity, ConversionBatchRepository, CreateConversionBatch},
    ebook::EbookRepository,
    format::FormatRepository,
    series::SeriesRepository,
    ListingParams,
};

use crate::{ebook_format::convertor::BatchJobRequest, error::ApiError, state::AppState};

// `ConversionBatchRepository` extractor is registered in
// `crate::rest_api::conversion_batch`.

/// Maximum number of ebooks accepted into a single batch. Anything above
/// this is dropped and reported via `dropped` on the response.
pub const BATCH_MAX_EBOOKS: usize = 100;

#[derive(Clone, serde::Serialize, serde::Deserialize, Debug, garde::Validate)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[garde(allow_unvalidated)]
pub struct BatchConversionRequest {
    pub for_entity: ConversionBatchEntity,
    #[garde(range(min = 1))]
    pub entity_id: i64,
    #[garde(length(min = 1, max = 16))]
    pub to_format_extension: String,
}

#[derive(Debug, serde::Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct BatchOperationTicket {
    pub operation_id: String,
    pub batch_id: i64,
    /// Ebooks accepted into the batch (after the `BATCH_MAX_EBOOKS` cap).
    pub total: usize,
    /// Ebooks above the cap that were dropped.
    pub dropped: usize,
    pub created: time::OffsetDateTime,
}

/// SSE payload for `batch_progress` events emitted once per processed ebook.
#[derive(Debug, serde::Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct BatchProgress {
    pub operation_id: String,
    pub batch_id: i64,
    pub done: usize,
    pub total: usize,
    pub ebook_id: i64,
    pub label: String,
    pub outcome: BatchItemOutcomeKind,
    /// Set when `outcome` is `Failed`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// SSE payload for `batch_complete` emitted once after the per-ebook loop
/// (and the ZIP-and-store step).
#[derive(Debug, serde::Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct BatchComplete {
    pub operation_id: String,
    pub batch_id: i64,
    pub total: usize,
    pub ok: usize,
    pub reused: usize,
    pub failed: usize,
    pub dropped: usize,
    /// `Conversions`-prefix-relative path of the result ZIP. `None` only when
    /// ZIP creation itself failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zip_location: Option<String>,
    /// Set when ZIP creation failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zip_error: Option<String>,
}

#[derive(Debug, Clone, Copy, serde::Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum BatchItemOutcomeKind {
    /// Source was actually converted via `ebook-convert`.
    Converted,
    /// A source of the ebook already had the target format; no conversion run.
    ReusedSource,
    /// A prior non-synthetic conversion at the target format was reused.
    ReusedConversion,
    Failed,
}

#[cfg_attr(
    feature = "openapi",
    utoipa::path(
        post,
        path = "/batch",
        tag = "Convert",
        operation_id = "convert_batch",
        request_body = BatchConversionRequest,
        responses(
            (status = StatusCode::OK, description = "OK", body = BatchOperationTicket),
        )
    )
)]
pub async fn convert_batch(
    State(state): State<AppState>,
    format_repo: FormatRepository,
    bookshelf_repo: BookshelfRepository,
    ebook_repo: EbookRepository,
    batch_repo: ConversionBatchRepository,
    author_repo: AuthorRepository,
    series_repo: SeriesRepository,
    api_user: mbs4_types::claim::ApiClaim,
    Garde(Json(payload)): Garde<Json<BatchConversionRequest>>,
) -> Result<impl IntoResponse, ApiError> {
    let format = format_repo
        .get_by_extension(&payload.to_format_extension)
        .await?;

    let all_ebook_ids = match payload.for_entity {
        ConversionBatchEntity::Bookshelf => {
            bookshelf_repo.list_ebook_ids(payload.entity_id).await?
        }
        ConversionBatchEntity::Series => {
            ebook_repo
                .list_ids_by_series(ListingParams::new_unpaged(), payload.entity_id)
                .await?
                .rows
        }
        ConversionBatchEntity::Author => {
            ebook_repo
                .list_ids_by_author(ListingParams::new_unpaged(), payload.entity_id)
                .await?
                .rows
        }
    };

    let mut ebook_ids = all_ebook_ids;
    let dropped_ebook_ids: Vec<i64> = if ebook_ids.len() > BATCH_MAX_EBOOKS {
        ebook_ids.split_off(BATCH_MAX_EBOOKS)
    } else {
        Vec::new()
    };
    let dropped = dropped_ebook_ids.len();

    let entity_label = match payload.for_entity {
        ConversionBatchEntity::Author => {
            let author = author_repo.get(payload.entity_id).await?;
            match author.first_name {
                Some(first) => format!("Author {} {}", first, author.last_name),
                None => format!("Author {}", author.last_name),
            }
        }
        ConversionBatchEntity::Series => {
            let series = series_repo.get(payload.entity_id).await?;
            format!("Series {}", series.title)
        }
        ConversionBatchEntity::Bookshelf => {
            let shelf = bookshelf_repo.get(payload.entity_id).await?;
            format!("Bookshelf {}", shelf.name)
        }
    };
    let name = format!(
        "Books for {} [{}]",
        entity_label, payload.to_format_extension
    );
    let batch = batch_repo
        .create(CreateConversionBatch {
            name,
            for_entity: Some(payload.for_entity),
            entity_id: Some(payload.entity_id),
            format_id: format.id,
            zip_location: None,
            created_by: Some(api_user.sub.clone()),
        })
        .await?;

    let operation_id = uuid::Uuid::new_v4().to_string();
    let total = ebook_ids.len();
    state
        .convertor()
        .convert_batch(BatchJobRequest {
            operation_id: operation_id.clone(),
            batch_id: batch.id,
            target_format_id: format.id,
            target_format_extension: format.extension,
            ebook_ids,
            dropped_ebook_ids,
            user: api_user.sub,
        })
        .await;

    Ok((
        StatusCode::OK,
        Json(BatchOperationTicket {
            operation_id,
            batch_id: batch.id,
            total,
            dropped,
            created: time::OffsetDateTime::now_utc(),
        }),
    ))
}
