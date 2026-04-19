#[cfg(feature = "observability")]
mod tests {
    use mbs4_e2e_tests::{
        TestUser, launch_env, prepare_env, spawn_server,
        rest::{create_author, create_ebook, create_language},
    };
    use serde_json::json;
    use tracing::info;
    use tracing_test::traced_test;

    fn metric_value(metrics: &str, metric_name: &str, label: &str) -> Option<f64> {
        metrics
            .lines()
            .find(|l| !l.starts_with('#') && l.contains(metric_name) && l.contains(label))
            .and_then(|l| l.rsplit(' ').next())
            .and_then(|v| v.parse().ok())
    }

    #[tokio::test]
    #[traced_test]
    async fn test_metrics_endpoint() {
        let (mut args, mut config_guard) = prepare_env("test_metrics_endpoint").await.unwrap();
        args.metrics_token = Some("test-metrics-token".to_string());
        let base_url = args.base_url.clone();

        spawn_server(args, &mut config_guard).await.unwrap();

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

    #[tokio::test]
    #[traced_test]
    async fn test_indexing_metrics() {
        let (mut args, mut config_guard) = prepare_env("test_indexing_metrics").await.unwrap();
        args.metrics_token = Some("test-metrics-token".to_string());
        let base_url = args.base_url.clone();

        let (client, _state) = launch_env(args, TestUser::Admin, &mut config_guard)
            .await
            .unwrap();

        let author = create_author(&client, &base_url, "Tolkien", Some("J.R.R."))
            .await
            .unwrap();
        let lang = create_language(&client, &base_url, "English", "en")
            .await
            .unwrap();

        create_ebook(
            &client,
            &base_url,
            &json!({ "title": "The Hobbit", "authors": [author.id], "language_id": lang.id }),
        )
        .await
        .unwrap();

        let metrics_url = base_url.join("metrics").unwrap();
        let mut metrics_body = String::new();
        // Poll until the ebook counter is non-zero (counters are pre-initialized to 0,
        // so a positive value confirms the ebook was actually indexed)
        for _ in 0..20 {
            let response = client
                .get(metrics_url.clone())
                .header("Authorization", "Bearer test-metrics-token")
                .send()
                .await
                .unwrap();
            metrics_body = response.text().await.unwrap();
            if metric_value(&metrics_body, "fts_index_items_total", "entity=\"ebook\"")
                .is_some_and(|v| v > 0.0)
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        info!("Metrics body: {}", metrics_body);

        let ebook_count =
            metric_value(&metrics_body, "fts_index_items_total", "entity=\"ebook\"")
                .expect("fts_index_items_total{entity=\"ebook\"} not found");
        assert!(ebook_count > 0.0, "ebook index count should be > 0, got {ebook_count}");

        let duration_sum =
            metric_value(&metrics_body, "fts_index_duration_seconds_sum", "success=\"true\"")
                .expect("fts_index_duration_seconds_sum{success=\"true\"} not found");
        assert!(duration_sum > 0.0, "indexing duration sum should be > 0, got {duration_sum}");
    }
}
