use std::fs;
use std::path::Path;
use std::sync::{Arc, mpsc};

use crate::{Indexer, IndexerResult, IndexingJob, Result, SearchResult, Searcher};
use mbs4_dal::ebook::Ebook;
use tantivy::collector::TopDocs;
use tantivy::query::{Query, QueryParser, RegexQuery};
use tantivy::tokenizer::{AsciiFoldingFilter, LowerCaser, SimpleTokenizer, TextAnalyzer};
use tantivy::{Index, IndexWriter, ReloadPolicy};
use tantivy::{IndexReader, schema::*};
use tracing::error;

#[derive(Clone)]
struct Fields {
    id: Field,
    title: Field,
    author: Field,
    series: Field,
}

impl Fields {
    fn new(schema: &Schema) -> Result<Self> {
        Ok(Self {
            id: schema.get_field("id")?,
            title: schema.get_field("title")?,
            author: schema.get_field("author")?,
            series: schema.get_field("series")?,
        })
    }
}

const WRITER_MEMORY_LIMIT: usize = 50_000_000;

struct IndexerRunner {
    writer: IndexWriter,
    fields: Fields,
    queue: mpsc::Receiver<IndexingJob>,
}

pub struct TantivyIndexer {
    sender: mpsc::Sender<IndexingJob>,
}

impl TantivyIndexer {
    pub fn new(index: &Index) -> Result<Self> {
        let writer = index.writer(WRITER_MEMORY_LIMIT)?;
        let fields = Fields::new(&index.schema())?;
        let (sender, receiver) = mpsc::channel();
        let runner = IndexerRunner {
            writer,
            fields,
            queue: receiver,
        };
        std::thread::spawn(move || {
            runner.run();
        });
        Ok(TantivyIndexer { sender })
    }

    pub fn stop(&self) {
        if let Err(e) = self.sender.send(IndexingJob::Stop) {
            error!("Failed to send stop command: {e}");
        }
    }
}

impl IndexerRunner {
    fn run(mut self) {
        loop {
            let job = match self.queue.recv() {
                Ok(job) => job,
                Err(e) => {
                    error!("Failed to receive job: {e}");
                    break;
                }
            };
            match job {
                IndexingJob::Stop => break,
                IndexingJob::Add {
                    items,
                    update,
                    sender,
                } => {
                    let res = self.index(items, update);
                    if let Err(ref e) = res {
                        error!("Indexing failed: {e}");
                    }
                    if let Err(_) = sender.send(res) {
                        error!("Failed to send indexing result");
                    }
                }
            }
        }
    }
    fn index(&mut self, items: Vec<Ebook>, update: bool) -> Result<()> {
        for ebook in items {
            if update {
                let term = Term::from_field_i64(self.fields.id, ebook.id);
                self.writer.delete_term(term);
            }
            let mut doc = TantivyDocument::new();
            doc.add_i64(self.fields.id, ebook.id);
            doc.add_text(self.fields.title, &ebook.title);
            if let Some(authors) = ebook.authors {
                for author_record in authors {
                    let author_name = match author_record.first_name {
                        Some(first_name) => format!("{} {}", first_name, author_record.last_name),
                        None => author_record.last_name,
                    };

                    doc.add_text(self.fields.author, &author_name);
                }
            }
            if let Some(series_record) = ebook.series {
                doc.add_text(self.fields.series, &series_record.title);
            }
            self.writer.add_document(doc)?;
        }

        self.writer.commit()?;
        Ok(())
    }
}

impl Indexer for TantivyIndexer {
    fn index(&mut self, items: Vec<Ebook>, update: bool) -> IndexerResult {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        self.sender.send(IndexingJob::Add {
            items,
            update,
            sender,
        })?;
        Ok(receiver)
    }

    fn delete(&mut self, id: Vec<i64>) -> IndexerResult {
        todo!()
    }

    fn reset(&mut self) -> IndexerResult {
        todo!()
    }
}

pub struct TantivySearcher {
    inner: Arc<TantivySearcherInner>,
}

impl TantivySearcher {
    pub fn new(index: &Index) -> Result<Self> {
        Ok(TantivySearcher {
            inner: Arc::new(TantivySearcherInner::new(index)?),
        })
    }
}

impl Searcher for TantivySearcher {
    async fn search<S: Into<String>>(
        &self,
        query: S,
        num_results: usize,
    ) -> Result<Vec<SearchResult>> {
        let indexer = self.inner.clone();
        let query = query.into();
        let res =
            tokio::task::spawn_blocking(move || indexer.search(&query, num_results)).await??;
        Ok(res)
    }
}

struct TantivySearcherInner {
    reader: IndexReader,
    #[allow(dead_code)]
    fields: Fields,
    query_parser: QueryParser,
    schema: Schema,
}

impl TantivySearcherInner {
    fn new(index: &Index) -> Result<Self> {
        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::OnCommitWithDelay)
            .try_into()?;
        let schema = index.schema();
        let fields = Fields::new(&schema)?;
        let query_parser =
            QueryParser::for_index(&index, vec![fields.title, fields.author, fields.series]);

        Ok(Self {
            reader,
            fields,
            query_parser,
            schema,
        })
    }
}

impl TantivySearcherInner {
    fn search(&self, query: &str, num_results: usize) -> Result<Vec<SearchResult>> {
        let boxed_query: Box<dyn Query + 'static> = if query.starts_with("/") {
            let query = &query[1..];
            let query = RegexQuery::from_pattern(query, self.fields.title)?;
            Box::new(query)
        } else {
            self.query_parser.parse_query(query)?
        };

        let searcher = self.reader.searcher();

        let top_docs = searcher.search(&boxed_query, &TopDocs::with_limit(num_results))?;

        let mut results = Vec::new();
        for (score, doc_address) in top_docs {
            let retrieved_doc: TantivyDocument = searcher.doc(doc_address)?;
            results.push(SearchResult {
                score,
                doc: retrieved_doc.to_json(&self.schema),
            })
        }

        Ok(results)
    }
}

pub fn init(index_dir: impl AsRef<Path>) -> Result<(TantivyIndexer, TantivySearcher)> {
    let index_dir = index_dir.as_ref();
    let index = if !index_dir.exists() {
        fs::create_dir(index_dir)?;
        create_index(index_dir)?
    } else {
        open_index(index_dir)?
    };

    register_tokenizer(&index);
    let indexer = TantivyIndexer::new(&index)?;
    let searcher = TantivySearcher::new(&index)?;
    Ok((indexer, searcher))
}

fn create_index(index_dir: &Path) -> Result<Index> {
    let text_options = TextOptions::default().set_indexing_options(
        TextFieldIndexing::default()
            .set_tokenizer("custom_tokenizer")
            .set_index_option(IndexRecordOption::WithFreqsAndPositions),
    );

    let mut schema_builder = Schema::builder();
    schema_builder.add_i64_field("id", FAST | INDEXED | STORED);
    schema_builder.add_text_field("title", text_options.clone() | STORED);
    schema_builder.add_text_field("author", text_options.clone() | STORED);
    schema_builder.add_text_field("series", text_options.clone() | STORED);

    let schema = schema_builder.build();

    let index = Index::create_in_dir(index_dir, schema.clone())?;
    Ok(index)
}

fn register_tokenizer(index: &Index) {
    let custom_tokenizer = TextAnalyzer::builder(SimpleTokenizer::default())
        .filter(LowerCaser)
        .filter(AsciiFoldingFilter)
        .build();
    index
        .tokenizers()
        .register("custom_tokenizer", custom_tokenizer);
}

fn open_index(index_dir: &Path) -> Result<Index> {
    let index = Index::open_in_dir(index_dir)?;
    Ok(index)
}
