use futures::TryStreamExt as _;
use mbs4_dal::ListingParams;
use sqlx::Executor;

const TEST_DATA: &str = r#"
INSERT INTO author (id, version, created, modified, last_name, first_name, description, created_by) 
VALUES (1,1,datetime(),datetime(),'Bacon','Lord',NULL,'ivan');
INSERT INTO author (id, version, created, modified, last_name, first_name, description, created_by) 
VALUES (2,1,datetime(),datetime(),'Cahoon','Janes',NULL,'ivan');
INSERT INTO author (id, version, created, modified, last_name, first_name, description, created_by) 
VALUES (3,1,datetime(),datetime(),'Usak','Pepa',NULL,'ivan');

INSERT INTO genre (id, version, name) 
VALUES (1,1, 'crime');
INSERT INTO genre (id, version, name) 
VALUES (2,1, 'sci-fi');
INSERT INTO genre (id, version, name) 
VALUES (3,1, 'fantasy');

INSERT INTO language (id, version, code, name) 
VALUES (1,1, 'cs', 'Czech');

INSERT INTO series (id, version, created, modified, title, description, created_by) 
VALUES (1,1, datetime(), datetime(), 'Serie', NULL, 'ivan');

INSERT INTO ebook (id, version, created, modified, title, description, language_id, series_id, series_index, cover, base_dir, created_by) 
VALUES (1,1,datetime(),datetime(),'Kniha knih',NULL,1,1,1,'xxx/kniha/cover.jpg','xxx/kniha','ivan');

INSERT INTO ebook_authors (ebook_id, author_id) VALUES (1,2);
INSERT INTO ebook_authors (ebook_id, author_id) VALUES (1,1);
INSERT INTO ebook_authors (ebook_id, author_id) VALUES (1,3);

INSERT INTO ebook_genres (ebook_id, genre_id) VALUES (1,1);
INSERT INTO ebook_genres (ebook_id, genre_id) VALUES (1,2);
INSERT INTO ebook_genres (ebook_id, genre_id) VALUES (1,3);

"#;

#[tokio::test]
pub async fn test_ebooks() {
    const DB_URL: &str = "sqlite::memory:";
    let conn = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .min_connections(1)
        .connect(DB_URL)
        .await
        .unwrap();
    conn.execute("PRAGMA foreign_keys = ON").await.unwrap();
    sqlx::migrate!("../../migrations").run(&conn).await.unwrap();

    conn.execute_many(TEST_DATA)
        .try_collect::<Vec<_>>()
        .await
        .unwrap();

    let count: u64 = sqlx::query_scalar("select count(*) from author")
        .fetch_one(&conn)
        .await
        .unwrap();

    assert_eq!(3, count);

    let repo = mbs4_dal::ebook::EbookRepositoryImpl::new(conn);
    let ebook = repo.get(1).await.unwrap();
    assert_eq!(ebook.title, "Kniha knih");
    assert_eq!(ebook.series.unwrap().title, "Serie");
    assert_eq!(ebook.authors.unwrap().len(), 3);
    assert_eq!(ebook.genres.unwrap().len(), 3);
    let params = ListingParams {
        order: Some(vec![mbs4_dal::Order::Asc("e.title".to_string())]),
        ..Default::default()
    };
    let all = repo.list(params).await.unwrap();
    assert_eq!(all.rows.len(), 1);
    assert_eq!(all.total, 1);
    assert_eq!(all.rows[0].title, "Kniha knih");
    assert_eq!(all.rows[0].series.as_ref().unwrap().title, "Serie");
    assert_eq!(all.rows[0].language.name, "Czech");
    assert_eq!(all.rows[0].authors.as_ref().unwrap().len(), 3);

    let author_ebooks = repo
        .list_by_author(ListingParams::default(), 2)
        .await
        .unwrap();
    assert_eq!(author_ebooks.rows.len(), 1);
    assert_eq!(author_ebooks.total, 1);

    let series_ebooks = repo
        .list_by_series(ListingParams::default(), 1)
        .await
        .unwrap();
    assert_eq!(series_ebooks.rows.len(), 1);
    assert_eq!(series_ebooks.total, 1);
}
