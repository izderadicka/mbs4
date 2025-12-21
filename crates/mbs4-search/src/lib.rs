pub mod sql;

use std::{fmt::Display, str::FromStr, task::Poll};

pub use anyhow::Result;
use mbs4_dal::{author::AuthorShort, ebook::Ebook, series::SeriesShort};
use pin_project_lite::pin_project;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
pub enum SearchTarget {
    Ebook,
    Series,
    Author,
}

impl FromStr for SearchTarget {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "ebook" => Ok(SearchTarget::Ebook),
            "series" => Ok(SearchTarget::Series),
            "author" => Ok(SearchTarget::Author),
            _ => Err(anyhow::anyhow!("Invalid search target: {}", s)),
        }
    }
}

impl Display for SearchTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SearchTarget::Ebook => write!(f, "ebook"),
            SearchTarget::Series => write!(f, "series"),
            SearchTarget::Author => write!(f, "author"),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct EbookDoc {
    pub title: String,
    pub series: String,
    pub series_index: String,
    pub series_id: Option<i64>,
    pub authors: Vec<AuthorSummary>,
    pub id: i64,
}

#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum FoundDoc {
    Ebook(EbookDoc),
    Series(SeriesShort),
    Author(AuthorShort),
}

#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SearchItem {
    pub score: f32,
    pub doc: FoundDoc,
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

pub enum ItemToIndex {
    Ebook(Ebook),
    Series(mbs4_dal::series::SeriesShort),
    Author(mbs4_dal::author::AuthorShort),
}

pub trait Indexer {
    fn index(&self, items: Vec<ItemToIndex>, update: bool) -> IndexerResult;
    fn delete(&self, id: Vec<i64>, what: SearchTarget) -> IndexerResult;
    fn reset(&self) -> IndexerResult;
}

pub trait Searcher {
    fn search(&self, query: &str, what: SearchTarget, num_results: usize) -> SearchResult;
}

#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AuthorSummary {
    pub id: u64,
    pub name: String,
}

enum IndexingJob {
    Stop,
    Add {
        items: Vec<ItemToIndex>,
        update: bool,
        sender: tokio::sync::oneshot::Sender<Result<()>>,
    },
    Delete {
        ids: Vec<i64>,
        what: SearchTarget,
        sender: tokio::sync::oneshot::Sender<Result<()>>,
    },
    Reset {
        sender: tokio::sync::oneshot::Sender<Result<()>>,
    },
}
