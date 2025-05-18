use std::fs;
use std::path::Path;

use crate::{Indexer, Result, SearchResult, Searcher};
use tantivy::collector::TopDocs;
use tantivy::query::{Query, QueryParser, RegexQuery};
use tantivy::tokenizer::{AsciiFoldingFilter, LowerCaser, SimpleTokenizer, TextAnalyzer};
use tantivy::{Index, IndexWriter, ReloadPolicy};
use tantivy::{IndexReader, schema::*};

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

pub struct TantivyIndexer {
    writer: IndexWriter,
    fields: Fields,
}

impl TantivyIndexer {
    pub fn new(index: &Index) -> Result<Self> {
        let writer = index.writer(WRITER_MEMORY_LIMIT)?;
        let fields = Fields::new(&index.schema())?;
        Ok(TantivyIndexer { writer, fields })
    }
}

impl Indexer for TantivyIndexer {
    fn index(&mut self, items: Vec<mbs4_dal::ebook::Ebook>, update: bool) -> Result<()> {
        for ebook in items {
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

pub struct TantivySearcher {
    inner: TantivySearcherInner,
}

impl TantivySearcher {
    pub fn new(index: &Index) -> Result<Self> {
        Ok(TantivySearcher {
            inner: TantivySearcherInner::new(index)?,
        })
    }
}

impl Searcher for TantivySearcher {
    fn search(&self, query: &str, num_results: usize) -> Result<Vec<SearchResult>> {
        self.inner.search(query, num_results)
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
