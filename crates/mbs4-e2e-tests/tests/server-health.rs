use mbs4_server::run::run;
use tracing_test::traced_test;

#[ignore]
#[tokio::test]
#[traced_test]
async fn test_health() {
    // get current directory
    let dir = std::env::current_dir().unwrap();
    println!("Current directory: {}", dir.display());
    let data_dir = dir.join("test-data");

    let (args, config_guard) = mbs4_e2e_tests::test_config("server-health", &data_dir).unwrap();
    let port = args.port;
    let base_url = args.base_url.clone();
    tokio::spawn(async move {
        run(args).await.unwrap();
    });

    tokio::time::sleep(std::time::Duration::from_secs(1)).await;

    let client = reqwest::Client::new();

    let url = format!("{}/", base_url);

    let response = client.get(&url).send().await.unwrap();
    assert!(response.status().is_success());
}
