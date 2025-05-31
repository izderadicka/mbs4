use crate::{Author, BookResult, IndexingJob};
use crate::{Indexer, IndexerResult, Result, Searcher};
use anyhow::Context;
use futures::TryStreamExt as _;
use sqlx::Row as _;
use sqlx::migrate::MigrateDatabase;
use tracing::error;

const INDEXING_CHANNEL_CAPACITY: usize = 10_000;

const CREATE_INDEX_QUERY: &str = "
CREATE TABLE docs (id INTEGER PRIMARY KEY, title TEXT, series TEXT, series_id INTEGER, author TEXT, author_id TEXT);
CREATE VIRTUAL TABLE idx USING fts5(title, series, series_id UNINDEXED, author, author_id UNINDEXED,content=docs, content_rowid=id );
CREATE TRIGGER after_insert AFTER INSERT ON docs BEGIN
    INSERT INTO idx(rowid, title, series, series_id, author, author_id) VALUES (new.id, new.title, new.series, new.series_id, new.author, new.author_id);
    END;
CREATE TRIGGER after_delete AFTER DELETE ON docs BEGIN
    INSERT INTO idx(idx, rowid, title, series, series_id, author, author_id) VALUES('delete', old.id, old.title, old.series, old.series_id, old.author, old.author_id);
    END;
CREATE TRIGGER after_update AFTER UPDATE ON docs BEGIN
    INSERT INTO idx(idx, rowid, title, series, series_id, author, author_id) VALUES('delete', old.id, old.title, old.series, old.series_id, old.author, old.author_id);
    INSERT INTO idx(rowid, title, series, series_id, author, author_id) VALUES (new.id, new.title, new.series, new.series_id, new.author, new.author_id);
    END;
";

pub async fn init(index_db_path: impl AsRef<std::path::Path>) -> Result<(SqlIndexer, SqlSearcher)> {
    let db_path = index_db_path.as_ref();
    let db_existed = tokio::fs::try_exists(db_path).await?;
    let db_url = format!(
        "{}",
        db_path
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("Invalid path"))?
    );
    if !db_existed {
        sqlx::Sqlite::create_database(&db_url).await?;
    }
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .min_connections(2)
        .max_connections(50)
        .connect(&db_url)
        .await
        .context(format!("Cannot connect to database {db_url}"))?;

    if !db_existed {
        sqlx::query(CREATE_INDEX_QUERY).execute(&pool).await?;
    }

    let (sender, receiver) = tokio::sync::mpsc::channel(INDEXING_CHANNEL_CAPACITY);
    let indexer_runner = SqlIndexerRunner {
        pool: pool.clone(),
        queue: receiver,
    };
    tokio::spawn(indexer_runner.run());

    Ok((SqlIndexer { queue: sender }, SqlSearcher { pool }))
}

pub async fn initial_index_fill(mut indexer: SqlIndexer, pool: mbs4_dal::Pool) -> Result<()> {
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
        let page = repository.list_ids(page_params).await?;
        let ebooks = repository.map_ids_to_ebooks(&page.rows).await?;

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

struct SqlIndexerRunner {
    pool: sqlx::Pool<sqlx::Sqlite>,
    queue: tokio::sync::mpsc::Receiver<IndexingJob>,
}

const LIST_SEP: &str = "; ";

impl SqlIndexerRunner {
    async fn index_batch(
        &mut self,
        items: Vec<mbs4_dal::ebook::Ebook>,
        update: bool,
    ) -> Result<()> {
        let mut transaction = self.pool.begin().await?;
        for ebook in items {
            let title = ebook.title;
            let series = ebook.series.clone().map(|s| s.title).unwrap_or_default();
            let series_id = ebook.series.map(|s| s.id);
            let author = ebook
                .authors
                .clone()
                .map(|authors| {
                    authors
                        .into_iter()
                        .map(|a| match a.first_name {
                            Some(first_name) => format!("{} {}", first_name, a.last_name),
                            None => a.last_name,
                        })
                        .collect::<Vec<_>>()
                        .join(LIST_SEP)
                })
                .unwrap_or_default();
            let author_id = ebook
                .authors
                .map(|authors| {
                    authors
                        .iter()
                        .map(|a| a.id.to_string())
                        .collect::<Vec<_>>()
                        .join(LIST_SEP)
                })
                .unwrap_or_default();
            let id = ebook.id.to_string();
            if update {
                sqlx::query("UPDATE docs SET title = ?1, series = ?2, series_id = ?3, author = ?4, author_id = ?5 WHERE id = ?6")
                    .bind(&title)
                    .bind(&series)
                    .bind(series_id)
                    .bind(&author)
                    .bind(&author_id)
                    .bind(&id)
                    .execute(&mut *transaction)
                    .await?;
            } else {
                sqlx::query("INSERT INTO docs (title, series, series_id, author, author_id, id) VALUES (?1, ?2, ?3, ?4, ?5, ?6)")
                .bind(&title)
                .bind(&series)
                .bind(series_id)
                .bind(&author)
                .bind(&author_id)
                .bind(&id)
                .execute(&mut *transaction)
                .await?;
            }
        }
        transaction.commit().await?;
        Ok(())
    }

    async fn delete_batch(&mut self, items: Vec<i64>) -> Result<()> {
        let mut transaction = self.pool.begin().await?;
        for id in items {
            sqlx::query("DELETE FROM docs WHERE id = ?")
                .bind(id)
                .execute(&mut *transaction)
                .await?;
        }
        transaction.commit().await?;
        Ok(())
    }

    async fn reset_index(&mut self) -> Result<()> {
        // TODO - if slow consider drop and recreate tables
        sqlx::query("DELETE FROM docs").execute(&self.pool).await?;
        Ok(())
    }

    async fn run(mut self) {
        while let Some(job) = self.queue.recv().await {
            match job {
                IndexingJob::Add {
                    items,
                    update,
                    sender,
                } => {
                    let res = self.index_batch(items, update).await;
                    if let Err(ref e) = res {
                        error!("Indexing failed: {e}");
                    }
                    if let Err(_) = sender.send(res) {
                        error!("Failed to send indexing result");
                    }
                }
                IndexingJob::Delete { ids, sender } => {
                    let res = self.delete_batch(ids).await;
                    if let Err(ref e) = res {
                        error!("Indexing failed: {e}");
                    }
                    if let Err(_) = sender.send(res) {
                        error!("Failed to send indexing result");
                    }
                }
                IndexingJob::Reset { sender } => {
                    let res = self.reset_index().await;
                    if let Err(ref e) = res {
                        error!("Indexing failed: {e}");
                    }
                    if let Err(_) = sender.send(res) {
                        error!("Failed to send indexing result");
                    }
                }
                IndexingJob::Stop => break,
            }
        }
    }
}

#[derive(Clone)]
pub struct SqlIndexer {
    queue: tokio::sync::mpsc::Sender<IndexingJob>,
}

impl SqlIndexer {
    pub fn stop(&mut self) -> Result<()> {
        self.queue.try_send(IndexingJob::Stop)?;
        Ok(())
    }
}

impl Indexer for SqlIndexer {
    fn index(&mut self, items: Vec<mbs4_dal::ebook::Ebook>, update: bool) -> IndexerResult {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        self.queue.try_send(IndexingJob::Add {
            items,
            update,
            sender,
        })?;
        // This is workaround - for future where indexer will run in separate thread

        Ok(receiver)
    }

    fn delete(&mut self, ids: Vec<i64>) -> IndexerResult {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        self.queue.try_send(IndexingJob::Delete { ids, sender })?;
        Ok(receiver)
    }

    fn reset(&mut self) -> IndexerResult {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        self.queue.try_send(IndexingJob::Reset { sender })?;
        Ok(receiver)
    }
}

#[derive(Clone)]
pub struct SqlSearcher {
    pool: sqlx::Pool<sqlx::Sqlite>,
}

impl SqlSearcher {
    async fn search_async(
        &self,
        query: &str,
        num_results: usize,
    ) -> Result<Vec<crate::SearchItem>> {
        let query: String = query.into();

        let mut rows = sqlx::query("SELECT title, series, series_id, author, author_id, rowid, rank FROM idx WHERE idx MATCH ? order by rank LIMIT ?")
        .bind(query)
        .bind(num_results as i64)
        .fetch(&self.pool);

        let mut results = Vec::new();
        while let Some(row) = rows.try_next().await? {
            let title: String = row.get(0);
            let series: String = row.get(1);
            let series_id: Option<i64> = row.get(2);
            let author: Vec<String> = row
                .get::<String, _>(3)
                .split(LIST_SEP)
                .map(String::from)
                .collect();
            let author_id: Vec<i64> = row
                .get::<String, _>(4)
                .split(LIST_SEP)
                .map(|s| s.parse::<i64>())
                .collect::<Result<_, _>>()?;
            let id: i64 = row.get(5);
            let rank: f32 = row.get(6);

            let authors = author
                .into_iter()
                .zip(author_id.into_iter())
                .map(|(name, id)| Author {
                    id: u64::try_from(id).unwrap(), // as we control id as and must be always positive we can unwrap
                    name,
                })
                .collect::<Vec<_>>();

            let res = BookResult {
                title,
                series,
                series_id,
                authors,
                id,
            };

            results.push(crate::SearchItem {
                score: -rank,
                doc: res,
            });
        }

        Ok(results)
    }
}

impl Searcher for SqlSearcher {
    fn search(&self, query: &str, num_results: usize) -> crate::SearchResult {
        let (res, sender) = crate::SearchResult::new();
        let searcher = self.clone();
        let query = query.to_string();
        tokio::spawn(async move {
            let res = searcher.search_async(&query, num_results).await;
            if let Err(ref e) = res {
                error!("Search failed: {e}");
            }
            if let Err(_) = sender.send(res) {
                error!("Failed to send search result");
            }
        });
        res
    }
}
