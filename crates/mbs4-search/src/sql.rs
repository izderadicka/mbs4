use std::path::PathBuf;

use crate::{Indexer, Result, Searcher};
use rusqlite::{Connection, params};
use serde::Serialize;

pub fn sample() -> Result<()> {
    let db_path = "test.db";
    let conn = Connection::open(db_path)?;

    let mut stmt =
        conn.prepare("SELECT title, rank FROM docs WHERE docs MATCH ?1 order by rank")?;
    let mut rows = stmt.query(["tes*"])?;

    while let Some(row) = rows.next()? {
        let title: String = row.get(0)?;
        let rank: f32 = row.get(1)?;
        println!("Found: {} {}", rank, title);
    }

    Ok(())
}

pub fn init(index_db_path: impl AsRef<std::path::Path>) -> Result<(SqlIndexer, SqlSearcher)> {
    let db_path = index_db_path.as_ref();
    let db_existed = db_path.exists();
    let conn = Connection::open(db_path)?;

    if !db_existed {
        conn.execute(
            "CREATE VIRTUAL TABLE docs USING fts5(title, series, author, id UNINDEXED );",
            [],
        )?;
    }

    Ok((
        SqlIndexer { conn: conn },
        SqlSearcher {
            db_path: db_path.to_path_buf(),
        },
    ))
}

pub struct SqlIndexer {
    conn: Connection,
}

impl Indexer for SqlIndexer {
    fn index(&mut self, items: Vec<mbs4_dal::ebook::Ebook>, update: bool) -> Result<()> {
        self.conn.execute_batch("BEGIN;")?;
        for ebook in items {
            let title = ebook.title;
            let series = ebook.series.map(|s| s.title).unwrap_or_default();
            let author = ebook
                .authors
                .map(|authors| {
                    authors
                        .into_iter()
                        .map(|a| match a.first_name {
                            Some(first_name) => format!("{} {}", first_name, a.last_name),
                            None => a.last_name,
                        })
                        .collect::<Vec<_>>()
                        .join("; ")
                })
                .unwrap_or_default();
            let id = ebook.id.to_string();
            self.conn
                .execute(
                    "INSERT INTO docs (title, series, author, id) VALUES (?1, ?2, ?3, ?4)",
                    [&title, &series, &author, &id],
                )
                .inspect_err(|_e| {
                    self.conn.execute_batch("ROLLBACK;").ok();
                })?;
        }
        self.conn.execute_batch("COMMIT;")?;
        Ok(())
    }
}

pub struct SqlSearcher {
    db_path: PathBuf,
}

#[derive(Debug, Serialize)]
struct BookResult {
    title: String,
    series: String,
    author: Vec<String>,
    id: i64,
}

impl Searcher for SqlSearcher {
    fn search(&self, query: &str, num_results: usize) -> Result<Vec<crate::SearchResult>> {
        use rusqlite::OpenFlags;
        let conn = Connection::open_with_flags(
            &self.db_path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;

        let mut stmt = conn
            .prepare("SELECT title, series, author, id, rank FROM docs WHERE docs MATCH ?1 order by rank LIMIT ?2")?;
        let mut rows = stmt.query(params![query, num_results])?;

        let mut results = Vec::new();
        while let Some(row) = rows.next()? {
            let title: String = row.get(0)?;
            let series: String = row.get(1)?;
            let author: Vec<String> = row
                .get::<_, String>(2)?
                .split("; ")
                .map(String::from)
                .collect();
            let id: i64 = row.get::<_, String>(3)?.parse()?;
            let rank: f32 = row.get(4)?;

            let res = BookResult {
                title,
                series,
                author,
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
