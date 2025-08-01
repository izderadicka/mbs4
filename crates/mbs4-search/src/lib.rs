pub mod sql;

use std::task::Poll;

pub use anyhow::Result;
use mbs4_dal::ebook::Ebook;
use pin_project_lite::pin_project;
use serde::Serialize;

#[derive(Debug, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SearchItem {
    pub score: f32,
    pub doc: BookResult,
}

pin_project!(
    pub struct SearchResult {
        #[pin]
        receiver: tokio::sync::oneshot::Receiver<Result<Vec<SearchItem>>>,
    }
);

impl SearchResult {
    pub fn new() -> (
        SearchResult,
        tokio::sync::oneshot::Sender<Result<Vec<SearchItem>>>,
    ) {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        (SearchResult { receiver }, sender)
    }

    pub async fn get(self) -> Result<Vec<SearchItem>> {
        self.receiver.await?
    }
}

impl Future for SearchResult {
    type Output = Result<Vec<SearchItem>>;

    fn poll(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        match this.receiver.poll(cx) {
            Poll::Ready(r) => match r {
                Ok(v) => Poll::Ready(v),
                Err(e) => Poll::Ready(Err(e.into())),
            },
            Poll::Pending => Poll::Pending,
        }
    }
}

pub type IndexerResult = Result<tokio::sync::oneshot::Receiver<Result<()>>>;

pub trait Indexer {
    fn index(&self, items: Vec<Ebook>, update: bool) -> IndexerResult;
    fn delete(&self, id: Vec<i64>) -> IndexerResult;
    fn reset(&self) -> IndexerResult;
}

pub trait Searcher {
    fn search(&self, query: &str, num_results: usize) -> SearchResult;
}

#[derive(Debug, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AuthorSummary {
    pub id: u64,
    pub name: String,
}

#[derive(Debug, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct BookResult {
    title: String,
    series: String,
    series_id: Option<i64>,
    authors: Vec<AuthorSummary>,
    id: i64,
}

enum IndexingJob {
    Stop,
    Add {
        items: Vec<Ebook>,
        update: bool,
        sender: tokio::sync::oneshot::Sender<Result<()>>,
    },
    Delete {
        ids: Vec<i64>,
        sender: tokio::sync::oneshot::Sender<Result<()>>,
    },
    Reset {
        sender: tokio::sync::oneshot::Sender<Result<()>>,
    },
}
