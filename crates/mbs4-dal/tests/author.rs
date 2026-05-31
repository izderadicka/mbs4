use futures::TryStreamExt as _;
use sqlx::Executor;

const TEST_DATA: &str = r#"
INSERT INTO author (id, version, created, modified, last_name, created_by)
VALUES (1,1,datetime(),datetime(),'Source','test');
INSERT INTO author (id, version, created, modified, last_name, created_by)
VALUES (2,1,datetime(),datetime(),'Target','test');
INSERT INTO author (id, version, created, modified, last_name, created_by)
VALUES (3,1,datetime(),datetime(),'NoBooks','test');

INSERT INTO language (id, version, code, name)
VALUES (1,1,'en','English');

INSERT INTO ebook (id, version, created, modified, title, description, language_id, base_dir, created_by)
VALUES (1,1,datetime(),datetime(),'Shared Book',NULL,1,'dir1','test');
INSERT INTO ebook (id, version, created, modified, title, description, language_id, base_dir, created_by)
VALUES (2,1,datetime(),datetime(),'Exclusive Book',NULL,1,'dir2','test');

-- ebook 1 is attributed to both author 1 and author 2 (shared)
INSERT INTO ebook_authors (ebook_id, author_id) VALUES (1, 1);
INSERT INTO ebook_authors (ebook_id, author_id) VALUES (1, 2);
-- ebook 2 is attributed to author 1 only
INSERT INTO ebook_authors (ebook_id, author_id) VALUES (2, 1);
"#;

async fn init_db() -> sqlx::Pool<sqlx::Sqlite> {
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
    conn
}

// Normal case: exclusive ebooks of from_id are reassigned to to_id.
#[tokio::test]
async fn test_merge_reassigns_ebooks() {
    let conn = init_db().await;
    let repo = mbs4_dal::author::AuthorRepositoryImpl::new(conn.clone());

    repo.merge(1, 2).await.unwrap();

    // Author 1 must be deleted.
    let from_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM author WHERE id = 1")
        .fetch_one(&conn)
        .await
        .unwrap();
    assert_eq!(from_count, 0, "source author should be deleted");

    // Ebook 2 (was exclusive to author 1) must now belong to author 2.
    let ebook2_target: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM ebook_authors WHERE ebook_id = 2 AND author_id = 2",
    )
    .fetch_one(&conn)
    .await
    .unwrap();
    assert_eq!(
        ebook2_target, 1,
        "exclusive ebook should be reassigned to target author"
    );
}

// Bug case: both authors already share an ebook — must not produce a 409 / PK conflict.
#[tokio::test]
async fn test_merge_shared_ebook() {
    let conn = init_db().await;
    let repo = mbs4_dal::author::AuthorRepositoryImpl::new(conn.clone());

    repo.merge(1, 2).await.unwrap();

    // Author 1 must be deleted.
    let from_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM author WHERE id = 1")
        .fetch_one(&conn)
        .await
        .unwrap();
    assert_eq!(from_count, 0, "source author should be deleted");

    // Ebook 1 must appear exactly once for author 2 (no duplicate row).
    let shared_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM ebook_authors WHERE ebook_id = 1 AND author_id = 2",
    )
    .fetch_one(&conn)
    .await
    .unwrap();
    assert_eq!(
        shared_count, 1,
        "shared ebook should appear exactly once for target author"
    );

    // Author 2 should now own both ebooks.
    let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM ebook_authors WHERE author_id = 2")
        .fetch_one(&conn)
        .await
        .unwrap();
    assert_eq!(total, 2, "target author should own both ebooks after merge");
}

// Edge case: from_id has no ebooks — merge must still succeed.
#[tokio::test]
async fn test_merge_no_ebooks() {
    let conn = init_db().await;
    let repo = mbs4_dal::author::AuthorRepositoryImpl::new(conn.clone());

    repo.merge(3, 2).await.unwrap();

    // Author 3 must be deleted.
    let from_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM author WHERE id = 3")
        .fetch_one(&conn)
        .await
        .unwrap();
    assert_eq!(
        from_count, 0,
        "source author with no ebooks should be deleted"
    );

    // Author 2's ebook associations must be unchanged (still owns ebook 1 only from seed).
    let target_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM ebook_authors WHERE author_id = 2")
            .fetch_one(&conn)
            .await
            .unwrap();
    assert_eq!(
        target_count, 1,
        "target author ebook associations should be unchanged"
    );
}
