use mbs4_dal::user::{CreateUser, UpdateUser, UserRepositoryImpl};
use mbs4_types::general::ValidEmail;
use sqlx::Executor;

async fn init_db() -> sqlx::Pool<sqlx::Sqlite> {
    let conn = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .min_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    conn.execute("PRAGMA foreign_keys = ON").await.unwrap();
    sqlx::migrate!("../../migrations").run(&conn).await.unwrap();
    conn
}

fn create_user_payload(name: &str, email: &str, password: Option<&str>) -> CreateUser {
    CreateUser {
        email: email.parse::<ValidEmail>().unwrap(),
        name: name.to_string(),
        password: password.map(|s| s.to_string()),
        roles: Some(vec!["user".to_string()]),
    }
}

#[tokio::test]
async fn test_update_user() {
    let conn = init_db().await;
    let repo = UserRepositoryImpl::new(conn.clone());

    let created = repo
        .create(create_user_payload(
            "Alice",
            "alice@example.com",
            Some("password123"),
        ))
        .await
        .unwrap();

    assert_eq!(created.name, "Alice");
    assert_eq!(created.email, "alice@example.com");
    assert_eq!(
        created.roles.as_deref(),
        Some(["user".to_string()].as_slice())
    );

    // Update name and roles; leave password unchanged (None)
    let updated = repo
        .update(
            created.id,
            UpdateUser {
                name: "Alice Updated".to_string(),
                password: None,
                roles: Some(vec!["user".to_string(), "admin".to_string()]),
            },
        )
        .await
        .unwrap();

    assert_eq!(updated.name, "Alice Updated");
    assert_eq!(updated.email, "alice@example.com");
    let mut roles = updated.roles.unwrap();
    roles.sort();
    assert_eq!(roles, ["admin", "user"]);

    // Password unchanged: original credentials still work
    repo.check_password("alice@example.com", "password123")
        .await
        .unwrap();

    // Update with new password
    let updated = repo
        .update(
            created.id,
            UpdateUser {
                name: "Alice Updated".to_string(),
                password: Some("newpassword456".to_string()),
                roles: None,
            },
        )
        .await
        .unwrap();

    assert_eq!(updated.roles, None);
    repo.check_password("alice@example.com", "newpassword456")
        .await
        .unwrap();

    // Empty password removes it (sets NULL); check_password should then fail
    repo.update(
        created.id,
        UpdateUser {
            name: "Alice Updated".to_string(),
            password: Some("".to_string()),
            roles: None,
        },
    )
    .await
    .unwrap();

    let stored_password: Option<String> =
        sqlx::query_scalar("SELECT password FROM users WHERE id = ?")
            .bind(created.id)
            .fetch_one(&conn)
            .await
            .unwrap();
    assert!(
        stored_password.is_none(),
        "password should be NULL after empty-string update"
    );

    // Update on non-existent id returns an error
    let err = repo
        .update(
            99999,
            UpdateUser {
                name: "Ghost".to_string(),
                password: None,
                roles: None,
            },
        )
        .await;
    assert!(err.is_err());
}
