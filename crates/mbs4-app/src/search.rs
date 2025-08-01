use crate::{
    error::{ApiResult, Result},
    state::AppState,
};
use axum::{
    extract::{Query, State},
    response::IntoResponse,
    Json,
};
use axum_valid::Garde;
use garde::Validate;
use http::StatusCode;
use mbs4_search::SearchResult;
use serde::Deserialize;
use std::sync::Arc;
use tracing::info;

#[derive(Clone)]
pub struct Search {
    inner: Arc<SearchInner>,
}

impl Search {
    pub async fn new(
        index_db_path: impl AsRef<std::path::Path>,
        pool: mbs4_dal::Pool,
    ) -> Result<Search> {
        let index_db_path = index_db_path.as_ref();
        let need_refill = !tokio::fs::try_exists(index_db_path).await?;
        let (indexer, searcher) = mbs4_search::sql::init(index_db_path).await?;
        if need_refill {
            info!("Fulltext index does not exist at {index_db_path:?}. Creating it now and filling from database.");
            mbs4_search::sql::initial_index_fill(indexer.clone(), pool).await?;
            info!("Fulltext index filled.");
        }
        let inner = SearchInner {
            indexer: Box::new(indexer),
            searcher: Box::new(searcher),
        };
        Ok(Search {
            inner: Arc::new(inner),
        })
    }

    pub fn search(&self, query: &str, num_results: usize) -> SearchResult {
        self.inner.searcher.search(query, num_results)
    }

    pub fn index_book(&self, book: mbs4_dal::ebook::Ebook, update: bool) -> Result<()> {
        let _res = self.inner.indexer.index(vec![book], update)?;
        Ok(())
    }

    pub fn delete_book(&self, id: i64) -> Result<()> {
        let _res = self.inner.indexer.delete(vec![id])?;
        Ok(())
    }
}

struct SearchInner {
    searcher: Box<dyn mbs4_search::Searcher + Send + Sync>,
    indexer: Box<dyn mbs4_search::Indexer + Send + Sync>,
}

#[derive(Debug, Clone, Validate, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::IntoParams))]
#[into_params(parameter_in = Query)]
pub struct SearchQuery {
    #[garde(length(max = 255))]
    query: String,
    #[garde(range(min = 1, max = 1000))]
    num_results: Option<usize>,
}

#[cfg_attr(feature = "openapi", utoipa::path(get, path = "", tag = "Search", params(SearchQuery), 
responses((status = StatusCode::OK, description = "Search", body = Vec<mbs4_search::SearchItem>))))]
pub async fn search(
    Garde(Query(query)): Garde<Query<SearchQuery>>,
    State(state): State<AppState>,
) -> ApiResult<impl IntoResponse> {
    let num_results = query.num_results.unwrap_or(10);
    let res = state.search().search(&query.query, num_results).await?;
    Ok((StatusCode::OK, Json(res)))
}

pub fn router() -> axum::Router<AppState> {
    axum::Router::new().route("/", axum::routing::get(search))
}

pub fn api_docs() -> utoipa::openapi::OpenApi {
    use utoipa::OpenApi as _;
    #[derive(utoipa::OpenApi)]
    #[openapi(paths(search))]
    struct ApiDocs;
    ApiDocs::openapi()
}
