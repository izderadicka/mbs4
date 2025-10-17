use anyhow::Result;
use clap::Parser as _;
use mbs4_dal;
use mbs4_search::{SearchItem, SearchTarget, Searcher as _, sql};

#[derive(clap::Parser)]
struct Args {
    #[arg(long, default_value_t = String::from("./test-data/index-test.db"), help = "Index db file path")]
    index_db: String,

    #[command(subcommand)]
    command: Command,
}

#[derive(clap::Subcommand)]
enum Command {
    FillIndex {
        #[arg(short, long, default_value_t = String::from("./test-data/mbs4.db"))]
        database_path: String,
    },
    Search {
        #[arg(short, long)]
        query: String,
        #[arg(short, long, default_value_t = SearchTarget::Ebook)]
        what: SearchTarget,
    },
}

async fn fill_index(indexer: sql::SqlIndexer, db_path: &str) -> Result<()> {
    let pool = mbs4_dal::new_pool(db_path).await?;
    sql::initial_index_fill(indexer, pool).await
}

async fn search(
    searcher: sql::SqlSearcher,
    query: &str,
    what: SearchTarget,
) -> Result<Vec<SearchItem>> {
    searcher.search(query, what, 10).await
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let args = Args::parse();

    let (indexer, searcher) = sql::init(&args.index_db).await?;

    match args.command {
        Command::FillIndex { database_path } => fill_index(indexer, &database_path).await?,
        Command::Search { query, what } => {
            let start = std::time::Instant::now();
            let res = search(searcher, &query, what).await?;
            let enlapsed = start.elapsed();
            println!("Results: {:#?}", res);
            println!("Enlapsed: {} ms", enlapsed.as_millis());
        }
    };

    Ok(())
}
