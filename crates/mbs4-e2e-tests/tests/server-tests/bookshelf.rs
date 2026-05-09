use mbs4_e2e_tests::{TestUser, launch_env, prepare_env};
use reqwest::StatusCode;
use serde_json::Value;
use tracing_test::traced_test;

async fn create_bookshelf(
    client: &reqwest::Client,
    base_url: &reqwest::Url,
    name: &str,
    public: bool,
) -> i64 {
    let res = client
        .post(base_url.join("api/bookshelf").unwrap())
        .json(&serde_json::json!({"name": name, "public": public}))
        .send()
        .await
        .unwrap();
    assert!(
        res.status().is_success(),
        "create bookshelf failed: {}",
        res.status()
    );
    let body: Value = res.json().await.unwrap();
    body["id"].as_i64().expect("id field missing")
}

/// Owner can always read their own private bookshelf.
#[tokio::test]
#[traced_test]
async fn test_owner_can_read_private_bookshelf() {
    let (args, mut guard) = prepare_env("test_bshelf_owner_read").await.unwrap();
    let base_url = args.base_url.clone();
    let (client, _state) = launch_env(args, TestUser::Admin, &mut guard).await.unwrap();

    let id = create_bookshelf(&client, &base_url, "My private shelf", false).await;

    let res = client
        .get(base_url.join(&format!("api/bookshelf/{id}")).unwrap())
        .send()
        .await
        .unwrap();
    assert!(
        res.status().is_success(),
        "owner should be able to read own shelf: {}",
        res.status()
    );
}

/// A different user (User role, different sub) cannot read a private bookshelf.
#[tokio::test]
#[traced_test]
async fn test_non_owner_cannot_read_private_bookshelf() {
    let (args, mut guard) = prepare_env("test_bshelf_cross_user_read").await.unwrap();
    let base_url = args.base_url.clone();
    let (admin_client, state) = launch_env(args, TestUser::Admin, &mut guard).await.unwrap();

    let id = create_bookshelf(&admin_client, &base_url, "Admin private shelf", false).await;

    // Build a second client authenticated as a different user (User role, sub = "user@localhost").
    let user_headers = TestUser::User.auth_header(&state).unwrap();
    let user_client = reqwest::Client::builder()
        .default_headers(user_headers)
        .build()
        .unwrap();

    let res = user_client
        .get(base_url.join(&format!("api/bookshelf/{id}")).unwrap())
        .send()
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        StatusCode::FORBIDDEN,
        "non-owner should not access private shelf"
    );
}

/// Any authenticated user can read a public bookshelf.
#[tokio::test]
#[traced_test]
async fn test_non_owner_can_read_public_bookshelf() {
    let (args, mut guard) = prepare_env("test_bshelf_public_read").await.unwrap();
    let base_url = args.base_url.clone();
    let (admin_client, state) = launch_env(args, TestUser::Admin, &mut guard).await.unwrap();

    let id = create_bookshelf(&admin_client, &base_url, "Admin public shelf", true).await;

    let user_headers = TestUser::User.auth_header(&state).unwrap();
    let user_client = reqwest::Client::builder()
        .default_headers(user_headers)
        .build()
        .unwrap();

    let res = user_client
        .get(base_url.join(&format!("api/bookshelf/{id}")).unwrap())
        .send()
        .await
        .unwrap();
    assert!(
        res.status().is_success(),
        "anyone should read a public shelf: {}",
        res.status()
    );
}

/// Non-owner cannot delete another user's bookshelf.
#[tokio::test]
#[traced_test]
async fn test_non_owner_cannot_delete_bookshelf() {
    let (args, mut guard) = prepare_env("test_bshelf_cross_user_delete").await.unwrap();
    let base_url = args.base_url.clone();
    let (admin_client, state) = launch_env(args, TestUser::Admin, &mut guard).await.unwrap();

    let id = create_bookshelf(&admin_client, &base_url, "Admin shelf", false).await;

    let user_headers = TestUser::User.auth_header(&state).unwrap();
    let user_client = reqwest::Client::builder()
        .default_headers(user_headers)
        .build()
        .unwrap();

    let res = user_client
        .delete(base_url.join(&format!("api/bookshelf/{id}")).unwrap())
        .send()
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        StatusCode::FORBIDDEN,
        "non-owner should not delete others' shelf"
    );
}

/// Owner can delete their own bookshelf.
#[tokio::test]
#[traced_test]
async fn test_owner_can_delete_own_bookshelf() {
    let (args, mut guard) = prepare_env("test_bshelf_owner_delete").await.unwrap();
    let base_url = args.base_url.clone();
    let (client, _state) = launch_env(args, TestUser::Admin, &mut guard).await.unwrap();

    let id = create_bookshelf(&client, &base_url, "My shelf", false).await;

    let res = client
        .delete(base_url.join(&format!("api/bookshelf/{id}")).unwrap())
        .send()
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        StatusCode::NO_CONTENT,
        "owner should delete own shelf"
    );
}
