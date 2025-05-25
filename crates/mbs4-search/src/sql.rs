use crate::{Author, BookResult, IndexingJob};
use crate::{Indexer, IndexerResult, Result, Searcher};
use anyhow::Context;
use futures::TryStreamExt as _;
use serde::Serialize;
use sqlx::Row as _;
use sqlx::migrate::MigrateDatabase;
use tracing::error;

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
    let db_existed = db_path.exists();
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

    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let indexer_runner = SqlIndexerRunner {
        pool: pool.clone(),
        queue: receiver,
    };
    tokio::spawn(indexer_runner.run());

    Ok((SqlIndexer { queue: sender }, SqlSearcher { pool }))
}

struct SqlIndexerRunner {
    pool: sqlx::Pool<sqlx::Sqlite>,
    queue: tokio::sync::mpsc::UnboundedReceiver<IndexingJob>,
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
                IndexingJob::Stop => break,
            }
        }
    }
}

#[derive(Clone)]
pub struct SqlIndexer {
    queue: tokio::sync::mpsc::UnboundedSender<IndexingJob>,
}

impl Indexer for SqlIndexer {
    fn index(&mut self, items: Vec<mbs4_dal::ebook::Ebook>, update: bool) -> IndexerResult {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        self.queue.send(IndexingJob::Add {
            items,
            update,
            sender,
        })?;
        // This is workaround - for future where indexer will run in separate thread

        Ok(receiver)
    }

    fn delete(&mut self, ids: Vec<i64>) -> IndexerResult {
        todo!()
    }

    fn reset(&mut self) -> IndexerResult {
        todo!()
    }
}

pub struct SqlSearcher {
    pool: sqlx::Pool<sqlx::Sqlite>,
}

impl Searcher for SqlSearcher {
    async fn search<S: Into<String>>(
        &self,
        query: S,
        num_results: usize,
    ) -> Result<Vec<crate::SearchResult>> {
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

            results.push(crate::SearchResult {
                score: -rank,
                doc: serde_json::to_string(&res).unwrap(),
            });
        }

        Ok(results)
    }
}
