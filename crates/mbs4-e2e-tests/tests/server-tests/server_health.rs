use mbs4_e2e_tests::{prepare_env, spawn_server};
use tracing::info;
use tracing_test::traced_test;

#[tokio::test]
#[traced_test]
async fn test_health() {
    let (args, mut _config_guard) = prepare_env("test_health").await.unwrap();
    let base_url = args.base_url.clone();

    spawn_server(args, &mut _config_guard).await.unwrap();

    let client = reqwest::Client::new();

    let url = base_url.join("health").unwrap();
    let response = client.get(url).send().await.unwrap();
    info! {"Response: {:#?}", response};
    assert!(response.status().is_success());
}

#[tokio::test]
#[traced_test]
async fn test_metrics_endpoint() {
    let (mut args, mut _config_guard) = prepare_env("test_metrics_endpoint").await.unwrap();
    args.metrics_token = Some("test-metrics-token".to_string());
    let base_url = args.base_url.clone();

    spawn_server(args, &mut _config_guard).await.unwrap();

    let client = reqwest::Client::new();

    let health_url = base_url.join("health").unwrap();
    let health_response = client.get(health_url).send().await.unwrap();
    assert!(health_response.status().is_success());

    let metrics_url = base_url.join("metrics").unwrap();
    let unauthorized_response = client.get(metrics_url.clone()).send().await.unwrap();
    assert_eq!(
        unauthorized_response.status(),
        reqwest::StatusCode::UNAUTHORIZED
    );

    let metrics_response = client
        .get(metrics_url)
        .header("Authorization", "Bearer test-metrics-token")
        .send()
        .await
        .unwrap();
    assert!(metrics_response.status().is_success());

    let metrics_body = metrics_response.text().await.unwrap();
    assert!(metrics_body.contains("http_server_request_duration_seconds"));
    assert!(!metrics_body.contains("seconds_seconds"));
    assert!(metrics_body.contains("http_request_method=\"GET\""));
    assert!(metrics_body.contains("http_route=\"/health\""));
    assert!(metrics_body.contains("http_response_status_code=\"200\""));
}
