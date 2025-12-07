use mbs4_dal::user;
use mbs4_e2e_tests::{prepare_env, spawn_server};
use reqwest::{StatusCode, Url};
use serde_json::json;
use tracing::info;
use tracing_test::traced_test;

#[tokio::test]
#[traced_test]
async fn test_auth() {
    let (args, _config_guard) = prepare_env("test_auth").await.unwrap();
    let pool = mbs4_dal::new_pool(&args.database_url()).await.unwrap();
    let user_registry = user::UserRepository::new(pool);
    let user_email = "admin@localhost";
    let user_password = "password";

    let new_user = user::CreateUser {
        name: "admin".to_string(),
        email: user_email.parse().unwrap(),
        password: Some(user_password.to_string()),
        roles: Some(vec!["admin".to_string()]),
    };
    let _user = user_registry.create(new_user).await.unwrap();
    let base_url = args.base_url.clone();

    spawn_server(args).await.unwrap();
    let client = reqwest::Client::builder()
        .cookie_store(true)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();

    let url = base_url.join("auth/login?redirect=/").unwrap();
    info! {"Login URL: {:#?}", url};
    let response = client
        .post(url)
        .json(&json!({"email": user_email, "password": user_password}))
        .send()
        .await
        .unwrap();
    info! {"Login Response: {:#?}", response};
    assert_eq!(StatusCode::SEE_OTHER, response.status());

    let location = response
        .headers()
        .get("Location")
        .unwrap()
        .to_str()
        .unwrap();
    let location = Url::parse(location).unwrap();
    let tr_token = location
        .query_pairs()
        .find_map(|(k, v)| if k == "trt" { Some(v) } else { None })
        .expect("tr token not found in location query");

    let mut url = base_url.join("auth/token").unwrap();
    url.query_pairs_mut().append_pair("trt", &tr_token);
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
