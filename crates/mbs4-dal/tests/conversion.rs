use futures::TryStreamExt as _;
use mbs4_dal::{
    ListingParams,
    conversion::{ConversionRepositoryImpl, CreateConversion},
    conversion_batch::{
        ConversionBatchEntity, ConversionBatchRepositoryImpl, CreateConversionBatch,
    },
};
use sqlx::Executor;

// Seeds: two formats (epub, mobi), one ebook, two sources (pdf + epub), and
// later we add conversions & batches per-test so the assertions can target
// specific rows.
const TEST_DATA: &str = r#"
INSERT INTO language (id, version, code, name) VALUES (1, 1, 'en', 'English');

INSERT INTO format (id, version, mime_type, name, extension)
VALUES (1, 1, 'application/epub+zip', 'EPUB', 'epub');
INSERT INTO format (id, version, mime_type, name, extension)
VALUES (2, 1, 'application/x-mobipocket-ebook', 'MOBI', 'mobi');
INSERT INTO format (id, version, mime_type, name, extension)
VALUES (3, 1, 'application/pdf', 'PDF', 'pdf');

INSERT INTO ebook (id, version, created, modified, title, language_id, base_dir, created_by)
VALUES (1, 1, datetime(), datetime(), 'Test Book', 1, 'tb', 'ivan');
INSERT INTO ebook (id, version, created, modified, title, language_id, base_dir, created_by)
VALUES (2, 1, datetime(), datetime(), 'Other Book', 1, 'ob', 'pepa');

INSERT INTO source (id, version, created, modified, ebook_id, location, format_id, size, hash, created_by)
VALUES (1, 1, datetime(), datetime(), 1, 'tb/test.pdf', 3, 100, 'h-pdf', 'ivan');
INSERT INTO source (id, version, created, modified, ebook_id, location, format_id, size, hash, quality, created_by)
VALUES (2, 1, datetime(), datetime(), 1, 'tb/test.epub', 1, 100, 'h-epub', 80.0, 'ivan');
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

#[tokio::test]
async fn test_list_for_ebook_filters_synthetic() {
    let conn = init_db().await;
    let repo = ConversionRepositoryImpl::new(conn.clone());

    // Real conversion: source 1 (pdf) -> mobi.
    let real = repo
        .create(CreateConversion {
            location: "tb/test.mobi".into(),
            source_id: 1,
            format_id: 2,
            batch_id: None,
            synthetic: false,
            created_by: Some("ivan".into()),
        })
        .await
        .unwrap();

    // Synthetic conversion: source 2 (epub) -> mobi, attached to a batch.
    // Should be excluded from `list_for_ebook`.
    let batch_repo = ConversionBatchRepositoryImpl::new(conn.clone());
    let batch = batch_repo
        .create(CreateConversionBatch {
            name: "b".into(),
            for_entity: Some(ConversionBatchEntity::Author),
            entity_id: Some(99),
            format_id: 2,
            zip_location: None,
            created_by: Some("ivan".into()),
        })
        .await
        .unwrap();
    let _synth = repo
        .create(CreateConversion {
            location: "tb/test.mobi".into(),
            source_id: 2,
            format_id: 2,
            batch_id: Some(batch.id),
            synthetic: true,
            created_by: Some("ivan".into()),
        })
        .await
        .unwrap();

    let listed = repo.list_for_ebook(1).await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, real.id);
    assert!(!listed[0].synthetic);
}

#[tokio::test]
async fn test_find_existing_for_ebook_ignores_synthetic_and_picks_latest() {
    let conn = init_db().await;
    let repo = ConversionRepositoryImpl::new(conn.clone());

    // Synthetic-only at format 1: must return None even though a row exists.
    let batch_repo = ConversionBatchRepositoryImpl::new(conn.clone());
    let batch = batch_repo
        .create(CreateConversionBatch {
            name: "b".into(),
            for_entity: Some(ConversionBatchEntity::Author),
            entity_id: Some(1),
            format_id: 1,
            zip_location: None,
            created_by: Some("ivan".into()),
        })
        .await
        .unwrap();
    repo.create(CreateConversion {
        location: "tb/x.epub".into(),
        source_id: 1,
        format_id: 1,
        batch_id: Some(batch.id),
        synthetic: true,
        created_by: Some("ivan".into()),
    })
    .await
    .unwrap();
    assert!(repo.find_existing_for_ebook(1, 1).await.unwrap().is_none());

    // Two real conversions at format 2; the latest wins.
    repo.create(CreateConversion {
        location: "tb/a.mobi".into(),
        source_id: 1,
        format_id: 2,
        batch_id: None,
        synthetic: false,
        created_by: Some("ivan".into()),
    })
    .await
    .unwrap();
    // Ensure the next row has a strictly later `created` value — sqlite's
    // datetime() resolution is seconds, so sleep briefly.
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    let newer = repo
        .create(CreateConversion {
            location: "tb/b.mobi".into(),
            source_id: 2,
            format_id: 2,
            batch_id: None,
            synthetic: false,
            created_by: Some("ivan".into()),
        })
        .await
        .unwrap();
    let found = repo.find_existing_for_ebook(1, 2).await.unwrap().unwrap();
    assert_eq!(found.id, newer.id);

    // Other ebook: nothing.
    assert!(repo.find_existing_for_ebook(2, 2).await.unwrap().is_none());
}

#[tokio::test]
async fn test_list_for_batch_includes_synthetic_and_real() {
    let conn = init_db().await;
    let conv_repo = ConversionRepositoryImpl::new(conn.clone());
    let batch_repo = ConversionBatchRepositoryImpl::new(conn.clone());

    let batch = batch_repo
        .create(CreateConversionBatch {
            name: "shelf-1-mobi".into(),
            for_entity: Some(ConversionBatchEntity::Bookshelf),
            entity_id: Some(1),
            format_id: 2,
            zip_location: None,
            created_by: Some("ivan".into()),
        })
        .await
        .unwrap();

    // Conversion outside any batch — should NOT appear in list_for_batch.
    conv_repo
        .create(CreateConversion {
            location: "tb/outside.mobi".into(),
            source_id: 1,
            format_id: 2,
            batch_id: None,
            synthetic: false,
            created_by: Some("ivan".into()),
        })
        .await
        .unwrap();

    // One real + one synthetic in the batch.
    let real = conv_repo
        .create(CreateConversion {
            location: "tb/in-real.mobi".into(),
            source_id: 1,
            format_id: 2,
            batch_id: Some(batch.id),
            synthetic: false,
            created_by: Some("ivan".into()),
        })
        .await
        .unwrap();
    let synth = conv_repo
        .create(CreateConversion {
            location: "tb/test.epub".into(),
            source_id: 2,
            format_id: 2,
            batch_id: Some(batch.id),
            synthetic: true,
            created_by: Some("ivan".into()),
        })
        .await
        .unwrap();

    let rows = conv_repo.list_for_batch(batch.id).await.unwrap();
    let ids: Vec<i64> = rows.iter().map(|r| r.id).collect();
    assert!(ids.contains(&real.id));
    assert!(ids.contains(&synth.id));
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().any(|r| r.synthetic));
    assert!(rows.iter().any(|r| !r.synthetic));
}

#[tokio::test]
async fn test_batch_list_for_user_scope_and_pagination() {
    let conn = init_db().await;
    let batch_repo = ConversionBatchRepositoryImpl::new(conn.clone());

    for (name, owner) in [
        ("ivan-1", "ivan"),
        ("pepa-1", "pepa"),
        ("ivan-2", "ivan"),
        ("ivan-3", "ivan"),
    ] {
        batch_repo
            .create(CreateConversionBatch {
                name: name.into(),
                for_entity: Some(ConversionBatchEntity::Author),
                entity_id: Some(1),
                format_id: 1,
                zip_location: None,
                created_by: Some(owner.into()),
            })
            .await
            .unwrap();
    }

    // Owner filter.
    let mine = batch_repo
        .list_for_user(Some("ivan"), ListingParams::default())
        .await
        .unwrap();
    assert_eq!(mine.total, 3);
    assert_eq!(mine.rows.len(), 3);
    assert!(
        mine.rows
            .iter()
            .all(|b| b.created_by.as_deref() == Some("ivan"))
    );

    let others = batch_repo
        .list_for_user(Some("nobody"), ListingParams::default())
        .await
        .unwrap();
    assert_eq!(others.total, 0);
    assert!(others.rows.is_empty());

    // Admin view: None == all users.
    let all = batch_repo
        .list_for_user(None, ListingParams::default())
        .await
        .unwrap();
    assert_eq!(all.total, 4);

    // Paginated owner view.
    let page = batch_repo
        .list_for_user(Some("ivan"), ListingParams::new(0, 2))
        .await
        .unwrap();
    assert_eq!(page.total, 3);
    assert_eq!(page.rows.len(), 2);
}

#[tokio::test]
async fn test_set_zip_location_persists() {
    let conn = init_db().await;
    let batch_repo = ConversionBatchRepositoryImpl::new(conn.clone());

    let created = batch_repo
        .create(CreateConversionBatch {
            name: "x".into(),
            for_entity: Some(ConversionBatchEntity::Series),
            entity_id: Some(7),
            format_id: 1,
            zip_location: None,
            created_by: Some("ivan".into()),
        })
        .await
        .unwrap();
    assert!(created.zip_location.is_none());

    batch_repo
        .set_zip_location(created.id, "batches/batch-1.zip")
        .await
        .unwrap();

    let reloaded = batch_repo.get(created.id).await.unwrap();
    assert_eq!(
        reloaded.zip_location.as_deref(),
        Some("batches/batch-1.zip")
    );
}
