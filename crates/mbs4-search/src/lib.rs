pub mod sql;
pub mod tnv;

pub use anyhow::Result;
use mbs4_dal::ebook::Ebook;

#[derive(Debug)]
pub struct SearchResult {
    pub score: f32,
    pub doc: String,
}

pub trait Indexer {
    fn index(&mut self, items: Vec<Ebook>, update: bool) -> Result<()>;
}

pub trait Searcher {
    fn search(&self, query: &str, num_results: usize) -> Result<Vec<SearchResult>>;
}
