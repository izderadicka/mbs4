use std::fs;
use std::path::Path;

use anyhow::Result;
use clap::Parser as _;
use mbs4_dal;
use tantivy::collector::TopDocs;
use tantivy::query::QueryParser;
use tantivy::schema::*;
use tantivy::tokenizer::{AsciiFoldingFilter, LowerCaser, SimpleTokenizer, TextAnalyzer};
use tantivy::{Index, IndexWriter, ReloadPolicy};

#[derive(clap::Parser)]
struct Args {
    #[arg(long, default_value_t = String::from("test-index"), help = "Index directory")]
    index_dir: String,

    #[command(subcommand)]
    command: Command,
}

#[derive(clap::Subcommand)]
enum Command {
    FillIndex {
        #[arg(short, long, default_value_t = String::from("../../test-data/mbs4.db"))]
        database_path: String,
    },
    Search {
        #[arg(short, long)]
        query: String,
    },
}

fn create_index(index_dir: &str) -> Result<Index> {
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

fn open_index(index_dir: &str) -> Result<Index> {
    let index = Index::open_in_dir(index_dir)?;
    Ok(index)
}

async fn fill_index(index: &Index, db_path: &str) -> Result<()> {
    let pool = mbs4_dal::new_pool(db_path).await?;
    let schema = index.schema();
    let mut writer: IndexWriter<TantivyDocument> = index.writer(50_000_000)?;

    let id = schema.get_field("id").unwrap();
    let title = schema.get_field("title").unwrap();
    let author = schema.get_field("author").unwrap();
    let series = schema.get_field("series").unwrap();

    let repository = mbs4_dal::ebook::EbookRepository::new(pool);
    const PAGE_SIZE: i64 = 1000;
    let mut page_no = 0;
    let params = mbs4_dal::ListingParams {
        limit: PAGE_SIZE,
        offset: 0,
        order: Some(vec![mbs4_dal::Order::Asc("e.id".to_string())]),
    };

    let mut indexed = 0;
    loop {
        let mut page_params = params.clone();
        page_params.offset = page_no * PAGE_SIZE;
        let page = repository.list(page_params).await?;
        for ebook in &page.rows {
            let ebook = repository.get(ebook.id).await?;
            let mut doc = TantivyDocument::new();
            doc.add_i64(id, ebook.id);
            doc.add_text(title, &ebook.title);
            if let Some(authors) = ebook.authors {
                for author_record in authors {
                    let author_name = match author_record.first_name {
                        Some(first_name) => format!("{} {}", first_name, author_record.last_name),
                        None => author_record.last_name,
                    };

                    doc.add_text(author, &author_name);
                }
            }
            if let Some(series_record) = ebook.series {
                doc.add_text(series, &series_record.title);
            }
            writer.add_document(doc)?;
        }

        writer.commit()?;
        indexed += page.rows.len();
        page_no += 1;

        if indexed >= page.total as usize {
            break;
        }
    }

    Ok(())
}

#[derive(Debug)]
pub struct SearchResult {
    pub score: f32,
    pub doc: String,
}

fn search(index: &Index, query: &str) -> Result<Vec<SearchResult>> {
    let reader = index
        .reader_builder()
        .reload_policy(ReloadPolicy::OnCommitWithDelay)
        .try_into()?;
    let searcher = reader.searcher();

    let schema = index.schema();
    let title = schema.get_field("title").unwrap();
    let author = schema.get_field("author").unwrap();
    let series = schema.get_field("series").unwrap();

    let query_parser = QueryParser::for_index(&index, vec![title, author, series]);
    let query = query_parser.parse_query(query)?;

    let top_docs = searcher.search(&query, &TopDocs::with_limit(10))?;

    let mut results = Vec::new();
    for (score, doc_address) in top_docs {
        let retrieved_doc: TantivyDocument = searcher.doc(doc_address)?;
        results.push(SearchResult {
            score,
            doc: retrieved_doc.to_json(&schema),
        })
    }

    Ok(results)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let index = if !Path::new(&args.index_dir).exists() {
        fs::create_dir(&args.index_dir)?;
        create_index(&args.index_dir)?
    } else {
        open_index(&args.index_dir)?
    };

    register_tokenizer(&index);

    match args.command {
        Command::FillIndex { database_path } => fill_index(&index, &database_path).await?,
        Command::Search { query } => {
            let res = search(&index, &query)?;
            println!("Results: {:#?}", res);
        }
    };

    Ok(())
}
