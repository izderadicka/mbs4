use mbs4_dal::user;
use mbs4_e2e_tests::{prepare_env, spawn_server};
use serde_json::json;
use tracing::info;
use tracing_test::traced_test;

#[tokio::test]
#[traced_test]
async fn test_auth() {
    let (args, _config_guard) = prepare_env("test_auth").await.unwrap();
    let pool = mbs4_dal::new_pool(&args.database_url).await.unwrap();
    let user_registry = user::UserRepository::new(pool);
    let user_email = "admin@localhost";
    let user_password = "password";

    let new_user = user::CreateUser {
        name: Some("admin".to_string()),
        email: user_email.parse().unwrap(),
        password: Some(user_password.to_string()),
        roles: Some(vec!["admin".to_string()]),
    };
    let _user = user_registry.create(new_user).await.unwrap();
    let base_url = args.base_url.clone();

    spawn_server(args).await.unwrap();
    let client = reqwest::Client::builder()
        .cookie_store(true)
        .build()
        .unwrap();

    let url = base_url.join("auth/login").unwrap();
    info! {"Login URL: {:#?}", url};
    let response = client
        .post(url)
        .json(&json!({"email": user_email, "password": user_password}))
        .send()
        .await
        .unwrap();
    info! {"Login Response: {:#?}", response};
    assert!(response.status().is_success());

    let url = base_url.join("auth/token").unwrap();
    let response = client.get(url).send().await.unwrap();
    info! {"Token Response: {:#?}", response};
    assert!(response.status().is_success());

    let token = response.text().await.unwrap();

    let url = base_url.join("users").unwrap();
    let response = client
        .get(url)
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .await
        .unwrap();
    info! {"Response: {:#?}", response};
    assert!(response.status().is_success());

    let users: Vec<user::User> = response.json().await.unwrap();
    assert_eq!(users.len(), 1);
}
