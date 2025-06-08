use mbs4_e2e_tests::{prepare_env, spawn_server};
use tracing::info;
use tracing_test::traced_test;

#[tokio::test]
#[traced_test]
async fn test_health() {
    let (args, _config_guard) = prepare_env("test_health").await.unwrap();
    let base_url = args.base_url.clone();

    spawn_server(args).await.unwrap();

    let client = reqwest::Client::new();

    let url = base_url.join("health").unwrap();
    let response = client.get(url).send().await.unwrap();
    info! {"Response: {:#?}", response};
    assert!(response.status().is_success());
}
