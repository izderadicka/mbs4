use mbs4_e2e_tests::{TestUser, launch_env, prepare_env};
use reqwest::StatusCode;
use tracing_test::traced_test;

/// GET /users requires Admin role.
#[tokio::test]
#[traced_test]
async fn test_admin_endpoint_with_user_token_returns_403() {
    let (args, mut guard) = prepare_env("test_authz_user_forbidden").await.unwrap();
    let base_url = args.base_url.clone();
    let (client, _state) = launch_env(args, TestUser::User, &mut guard).await.unwrap();

    let res = client
        .get(base_url.join("users").unwrap())
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
#[traced_test]
async fn test_admin_endpoint_without_token_returns_401() {
    let (args, mut guard) = prepare_env("test_authz_no_token").await.unwrap();
    let base_url = args.base_url.clone();
    let (client, _state) = launch_env(args, TestUser::None, &mut guard).await.unwrap();

    let res = client
        .get(base_url.join("users").unwrap())
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
#[traced_test]
async fn test_admin_endpoint_with_admin_token_succeeds() {
    let (args, mut guard) = prepare_env("test_authz_admin_ok").await.unwrap();
    let base_url = args.base_url.clone();
    let (client, _state) = launch_env(args, TestUser::Admin, &mut guard).await.unwrap();

    let res = client
        .get(base_url.join("users").unwrap())
        .send()
        .await
        .unwrap();
    assert!(
        res.status().is_success(),
        "expected 2xx, got {}",
        res.status()
    );
}

/// POST /api/author requires Admin or Trusted role.
#[tokio::test]
#[traced_test]
async fn test_trusted_endpoint_with_plain_user_returns_403() {
    let (args, mut guard) = prepare_env("test_authz_trusted_forbidden").await.unwrap();
    let base_url = args.base_url.clone();
    let (client, _state) = launch_env(args, TestUser::User, &mut guard).await.unwrap();

    let res = client
        .post(base_url.join("api/author").unwrap())
        .json(&serde_json::json!({"last_name": "Test", "first_name": "User"}))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
#[traced_test]
async fn test_trusted_endpoint_with_trusted_user_succeeds() {
    let (args, mut guard) = prepare_env("test_authz_trusted_ok").await.unwrap();
    let base_url = args.base_url.clone();
    let (client, _state) = launch_env(args, TestUser::TrustedUser, &mut guard)
        .await
        .unwrap();

    let res = client
        .post(base_url.join("api/author").unwrap())
        .json(&serde_json::json!({"last_name": "Test", "first_name": "User"}))
        .send()
        .await
        .unwrap();
    // 201 Created or 200 — anything in 2xx range is acceptable
    assert!(
        res.status().is_success(),
        "expected 2xx, got {}",
        res.status()
    );
}
