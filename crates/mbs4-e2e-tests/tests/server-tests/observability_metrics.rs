#[cfg(feature = "observability")]
mod tests {
    use futures::StreamExt as _;
    use mbs4_app::store::rest_api::UploadInfo;
    use mbs4_e2e_tests::{
        TestUser, launch_env, prepare_env,
        rest::{create_author, create_ebook, create_format, create_language},
        spawn_server,
    };
    use reqwest::{Url, multipart};
    use serde_json::{Value, json};
    use tracing::info;
    use tracing_test::traced_test;

    /// Subscribe to SSE BEFORE triggering an operation, return a receiver that fires when
    /// an event with the given `operation_id` arrives. Must be called before triggering the
    /// operation to avoid missing a fast event.
    fn subscribe_sse(
        client: reqwest::Client,
        sse_url: Url,
        operation_id: String,
    ) -> tokio::sync::oneshot::Receiver<()> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            let res = client
                .get(sse_url)
                .send()
                .await
                .expect("SSE connect failed")
                .error_for_status()
                .expect("SSE HTTP error");

            let mut stream = res.bytes_stream();
            let mut buffer = String::new();

            while let Some(chunk) = stream.next().await {
                let text = String::from_utf8_lossy(&chunk.unwrap()).into_owned();
                buffer.push_str(&text);

                while let Some(pos) = buffer.find("\n\n") {
                    let event_text = buffer[..pos].to_string();
                    buffer.drain(..pos + 2);

                    for line in event_text.lines() {
                        if let Some(data) = line.strip_prefix("data:") {
                            if let Ok(v) = serde_json::from_str::<Value>(data.trim()) {
                                if v.get("data")
                                    .and_then(|d| d.get("operation_id"))
                                    .and_then(Value::as_str)
                                    == Some(operation_id.as_str())
                                {
                                    let _ = tx.send(());
                                    return;
                                }
                            }
                        }
                    }
                }
            }
        });
        rx
    }

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

        let ebook_count = metric_value(&metrics_body, "fts_index_items_total", "entity=\"ebook\"")
            .expect("fts_index_items_total{entity=\"ebook\"} not found");
        assert!(
            ebook_count > 0.0,
            "ebook index count should be > 0, got {ebook_count}"
        );

        let duration_sum = metric_value(
            &metrics_body,
            "fts_index_duration_seconds_sum",
            "success=\"true\"",
        )
        .expect("fts_index_duration_seconds_sum{success=\"true\"} not found");
        assert!(
            duration_sum > 0.0,
            "indexing duration sum should be > 0, got {duration_sum}"
        );
    }

    #[tokio::test]
    #[traced_test]
    async fn test_conversion_metrics() {
        let (mut args, mut config_guard) = prepare_env("test_conversion_metrics").await.unwrap();
        args.metrics_token = Some("test-metrics-token".to_string());
        let base_url = args.base_url.clone();

        let (client, _state) = launch_env(args, TestUser::Admin, &mut config_guard)
            .await
            .unwrap();

        // Register epub format (required for file extension validation)
        create_format(&client, &base_url, "epub", "application/epub+zip", "epub")
            .await
            .unwrap();

        // Upload a sample epub file
        let ebook_file = "../../test-data/samples/Holmes.epub";
        let file = tokio::fs::read(ebook_file).await.unwrap();
        let form = multipart::Form::new().part(
            "file",
            multipart::Part::bytes(file).file_name("Holmes.epub"),
        );
        let upload_url = base_url.join("files/upload/form").unwrap();
        let res = client
            .post(upload_url)
            .multipart(form)
            .send()
            .await
            .unwrap();
        assert!(res.status().is_success());
        let upload_info: UploadInfo = res.json().await.unwrap();

        // Subscribe to SSE before triggering extraction to avoid missing a fast event
        let sse_url = base_url.join("events").unwrap();
        // We don't know the operation_id yet; trigger extraction to get it, but SSE
        // subscription must come first. Use a placeholder and re-subscribe after we have the id.
        // Pattern: subscribe with a known id by triggering extraction, then subscribe immediately.
        let meta_url = base_url.join("api/convert/extract_meta").unwrap();
        let res = client
            .post(meta_url)
            .json(&upload_info)
            .send()
            .await
            .unwrap();
        assert!(res.status().is_success());
        let ticket: serde_json::Map<String, Value> = res.json().await.unwrap();
        let operation_id = ticket["id"].as_str().unwrap().to_string();

        // Subscribe to SSE (spawned task - may still catch the event if not yet fired)
        let sse_receiver = subscribe_sse(client.clone(), sse_url, operation_id);
        tokio::time::timeout(std::time::Duration::from_secs(30), sse_receiver)
            .await
            .expect("Timed out waiting for meta extraction SSE event")
            .expect("SSE receiver dropped");

        // Fetch metrics once extraction is confirmed done
        let metrics_url = base_url.join("metrics").unwrap();
        let metrics_body = client
            .get(metrics_url)
            .header("Authorization", "Bearer test-metrics-token")
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        info!("Metrics body: {}", metrics_body);

        let count = metric_value(
            &metrics_body,
            "conv_items_total",
            "operation=\"meta_extract\",success=\"true\"",
        )
        .expect("conv_items_total{operation=\"meta_extract\",success=\"true\"} not found");
        assert!(count > 0.0, "meta_extract count should be > 0, got {count}");

        let duration_sum = metric_value(
            &metrics_body,
            "conv_duration_seconds_sum",
            "operation=\"meta_extract\",success=\"true\"",
        )
        .expect("conv_duration_seconds_sum{operation=\"meta_extract\",success=\"true\"} not found");
        assert!(
            duration_sum > 0.0,
            "meta_extract duration sum should be > 0, got {duration_sum}"
        );
    }
}
