pub mod sql;
pub mod tnv;

pub use anyhow::Result;
use mbs4_dal::ebook::Ebook;
use serde::Serialize;

#[derive(Debug)]
pub struct SearchResult {
    pub score: f32,
    pub doc: String,
}

pub type IndexerResult = Result<tokio::sync::oneshot::Receiver<Result<()>>>;

pub trait Indexer {
    fn index(&mut self, items: Vec<Ebook>, update: bool) -> IndexerResult;
    fn delete(&mut self, id: Vec<i64>) -> IndexerResult;
    fn reset(&mut self) -> IndexerResult;
}

pub trait Searcher {
    #[allow(async_fn_in_trait)]
    async fn search<S: Into<String>>(
        &self,
        query: S,
        num_results: usize,
    ) -> Result<Vec<SearchResult>>;
}

#[derive(Debug, Serialize)]
pub struct Author {
    pub id: u64,
    pub name: String,
}

#[derive(Debug, Serialize)]
pub struct BookResult {
    title: String,
    series: String,
    series_id: Option<i64>,
    authors: Vec<Author>,
    id: i64,
}

enum IndexingJob {
    Stop,
    Add {
        items: Vec<Ebook>,
        update: bool,
        sender: tokio::sync::oneshot::Sender<Result<()>>,
    },
}
