use anyhow::Result;
use clap::Parser as _;
use mbs4_dal;
use mbs4_search::{Indexer as _, SearchResult, Searcher as _, sql};

#[derive(clap::Parser)]
struct Args {
    #[arg(long, default_value_t = String::from("test.db"), help = "Index db file path")]
    index_db: String,

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

async fn fill_index(mut indexer: sql::SqlIndexer, db_path: &str) -> Result<()> {
    let pool = mbs4_dal::new_pool(db_path).await?;

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
        let mut ebooks = Vec::with_capacity(page.rows.len());
        for ebook in &page.rows {
            let ebook = repository.get(ebook.id).await?;
            ebooks.push(ebook);
        }

        let res = indexer.index(ebooks, false)?;
        res.await??;
        indexed += page.rows.len();
        page_no += 1;

        if indexed >= page.total as usize {
            break;
        }
    }

    Ok(())
}

async fn search(searcher: sql::SqlSearcher, query: &str) -> Result<Vec<SearchResult>> {
    searcher.search(query, 10).await
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let args = Args::parse();

    let (indexer, searcher) = sql::init(&args.index_db).await?;

    match args.command {
        Command::FillIndex { database_path } => fill_index(indexer, &database_path).await?,
        Command::Search { query } => {
            let start = std::time::Instant::now();
            let res = search(searcher, &query).await?;
            let enlapsed = start.elapsed();
            println!("Results: {:#?}", res);
            println!("Enlapsed: {} ms", enlapsed.as_millis());
        }
    };

    Ok(())
}
