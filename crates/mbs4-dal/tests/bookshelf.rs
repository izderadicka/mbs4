use futures::TryStreamExt as _;
use mbs4_dal::{
    ListingParams, Order,
    bookshelf::{
        BookshelfRepositoryImpl, CreateBookshelf, CreateBookshelfItem, UpdateBookshelf,
        UpdateBookshelfItem,
    },
};
use sqlx::Executor;

const TEST_DATA: &str = r#"
INSERT INTO author (id, version, created, modified, last_name, first_name, description, created_by)
VALUES (1,1,datetime(),datetime(),'Bacon','Lord',NULL,'ivan');
INSERT INTO author (id, version, created, modified, last_name, first_name, description, created_by)
VALUES (2,1,datetime(),datetime(),'Cahoon','Janes',NULL,'ivan');
INSERT INTO author (id, version, created, modified, last_name, first_name, description, created_by)
VALUES (3,1,datetime(),datetime(),'Usak','Pepa',NULL,'ivan');

INSERT INTO language (id, version, code, name)
VALUES (1,1, 'cs', 'Czech');

INSERT INTO series (id, version, created, modified, title, description, created_by)
VALUES (1,1, datetime(), datetime(), 'Serie', NULL, 'ivan');

INSERT INTO ebook (id, version, created, modified, title, description, language_id, series_id, series_index, cover, base_dir, created_by)
VALUES (1,1,datetime(),datetime(),'Kniha knih',NULL,1,1,1,'xxx/kniha/cover.jpg','xxx/kniha','ivan');

INSERT INTO ebook_authors (ebook_id, author_id) VALUES (1,2);
INSERT INTO ebook_authors (ebook_id, author_id) VALUES (1,1);
INSERT INTO ebook_authors (ebook_id, author_id) VALUES (1,3);

INSERT INTO bookshelf (id, version, created, modified, name, description, public, created_by)
VALUES (10,1,datetime(),datetime(),'Ivan private shelf',NULL,0,'ivan');
INSERT INTO bookshelf (id, version, created, modified, name, description, public, created_by)
VALUES (11,1,datetime(),datetime(),'Pepa public shelf',NULL,1,'pepa');

INSERT INTO bookshelf_item (id, version, created, modified, type, bookshelf_id, ebook_id, series_id, "order", note, created_by)
VALUES (20,1,datetime(),datetime(),'EBOOK',11,1,NULL,1,'seeded item','pepa');
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
async fn test_bookshelf_crud_and_items() {
    let conn = init_db().await;
    let repo = BookshelfRepositoryImpl::new(conn.clone());

    let created = repo
        .create(CreateBookshelf {
            name: "Ivan public shelf".to_string(),
            description: Some("Shelf for testing".to_string()),
            public: true,
            created_by: Some("ivan".to_string()),
        })
        .await
        .unwrap();

    assert_eq!(created.name, "Ivan public shelf");
    assert!(created.public);
    assert_eq!(created.created_by.as_deref(), Some("ivan"));

    let mine = repo
        .list_for_user("ivan", ListingParams::default())
        .await
        .unwrap();
    assert_eq!(mine.total, 2);
    assert_eq!(mine.rows.len(), 2);
    assert!(mine.rows.iter().any(|row| row.id == 10));
    assert!(mine.rows.iter().any(|row| row.id == created.id));

    let public = repo
        .list_public("ivan", ListingParams::default())
        .await
        .unwrap();
    assert_eq!(public.total, 1);
    assert_eq!(public.rows.len(), 1);
    assert_eq!(public.rows[0].id, 11);
    assert_eq!(public.rows[0].items_count, 1);

    assert_eq!(repo.get_owner(created.id).await.unwrap(), "ivan");

    let updated = repo
        .update(
            created.id,
            UpdateBookshelf {
                id: created.id,
                name: "Ivan public shelf updated".to_string(),
                description: Some("Updated description".to_string()),
                public: false,
                version: created.version,
            },
        )
        .await
        .unwrap();

    assert_eq!(updated.name, "Ivan public shelf updated");
    assert!(!updated.public);
    assert_eq!(updated.version, created.version + 1);

    let ebook_item_id = repo
        .add_item(
            created.id,
            CreateBookshelfItem {
                note: Some("first note".to_string()),
                item_type: "EBOOK".to_string(),
                ebook_id: Some(1),
                series_id: None,
                order: Some(2),
                created_by: Some("ivan".to_string()),
            },
        )
        .await
        .unwrap();

    let duplicate_ebook_item_id = repo
        .add_item(
            created.id,
            CreateBookshelfItem {
                note: Some("replaced note".to_string()),
                item_type: "EBOOK".to_string(),
                ebook_id: Some(1),
                series_id: None,
                order: Some(3),
                created_by: Some("ivan".to_string()),
            },
        )
        .await
        .unwrap();

    assert_eq!(duplicate_ebook_item_id, ebook_item_id);

    let series_item_id = repo
        .add_item(
            created.id,
            CreateBookshelfItem {
                note: Some("series note".to_string()),
                item_type: "SERIES".to_string(),
                ebook_id: None,
                series_id: Some(1),
                order: Some(1),
                created_by: Some("ivan".to_string()),
            },
        )
        .await
        .unwrap();

    let items = repo
        .list_items(
            created.id,
            ListingParams::new(0, 10).with_order(vec![Order::Asc("title".to_string())]),
        )
        .await
        .unwrap();

    assert_eq!(items.total, 2);
    assert_eq!(items.rows.len(), 2);
    assert_eq!(items.rows[0].id, ebook_item_id);
    assert_eq!(items.rows[0].title, "Kniha knih");
    assert!(items.rows[0].has_cover);
    assert_eq!(items.rows[0].note.as_deref(), Some("replaced note"));
    assert_eq!(items.rows[0].series_title.as_deref(), Some("Serie"));
    assert_eq!(items.rows[0].series_index, Some(1));
    assert_eq!(items.rows[0].authors.as_ref().map(Vec::len), Some(3));
    assert_eq!(items.rows[1].id, series_item_id);
    assert_eq!(items.rows[1].title, "Serie");
    assert!(!items.rows[1].has_cover);
    assert_eq!(items.rows[1].authors.as_ref().map(Vec::len), Some(3));

    let item_version: i64 = sqlx::query_scalar("SELECT version FROM bookshelf_item WHERE id = ?")
        .bind(series_item_id)
        .fetch_one(&conn)
        .await
        .unwrap();

    let updated_item_id = repo
        .update_item(
            created.id,
            UpdateBookshelfItem {
                id: series_item_id,
                note: Some("series updated".to_string()),
                order: Some(5),
                version: item_version,
            },
        )
        .await
        .unwrap();

    assert_eq!(updated_item_id, series_item_id);

    let updated_note: Option<String> =
        sqlx::query_scalar("SELECT note FROM bookshelf_item WHERE id = ?")
            .bind(series_item_id)
            .fetch_one(&conn)
            .await
            .unwrap();
    assert_eq!(updated_note.as_deref(), Some("series updated"));

    repo.remove_item(created.id, series_item_id).await.unwrap();

    let remaining_count: u64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM bookshelf_item WHERE bookshelf_id = ?")
            .bind(created.id)
            .fetch_one(&conn)
            .await
            .unwrap();
    assert_eq!(remaining_count, 1);

    repo.delete(created.id).await.unwrap();
    assert_eq!(repo.count().await.unwrap(), 2);
}
