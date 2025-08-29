use futures::StreamExt;
use mbs4_app::{ebook_format::OperationTicket, store::rest_api::UploadInfo};
use mbs4_e2e_tests::{TestUser, launch_env, prepare_env, rest::create_format};
use reqwest::{Url, multipart};
use serde_json::{Map, Value};
use tracing::{debug, info};
use tracing_test::traced_test;

fn parse_event(event: &str) -> Result<Map<String, Value>, anyhow::Error> {
    for line in event.lines() {
        let mut iter = line.splitn(2, ':');
        let tag = iter.next().unwrap().trim();
        let text = iter.next().unwrap().trim();
        if tag == "data" {
            let value = serde_json::from_str(text).unwrap();
            return Ok(value);
        }
    }

    anyhow::bail!("Failed to parse event")
}

fn catch_event(
    client: reqwest::Client,
    sse_url: Url,
    operation_id: String,
) -> Result<tokio::sync::oneshot::Receiver<Value>, anyhow::Error> {
    let (mut sender, receiver) = tokio::sync::oneshot::channel();

    tokio::spawn(async move {
        let res = client
            .get(sse_url)
            .send()
            .await
            .expect("Failed to connect to events")
            .error_for_status()
            .expect("HTTP error on events");

        let mut stream = res.bytes_stream();

        let mut buffer = String::new();
        let mut event: Option<Value> = None;

        'main_loop: while let Some(chunk) = stream.next().await {
            let chunk = chunk.unwrap();
            let text = String::from_utf8_lossy(&chunk);

            buffer.push_str(&text);

            // SSE messages are separated by double newline
            while let Some(pos) = buffer.find("\n\n") {
                let event_text = buffer[..pos].to_string();
                buffer.drain(..pos + 2); // remove processed part

                info!("Received raw SSE:\n{}", event_text);

                let event_json = parse_event(&event_text).unwrap();

                if event_json["data"].as_object().unwrap()["operation_id"] == operation_id {
                    event = Some(event_json["data"].clone());
                    break 'main_loop;
                }
            }
        }
        debug!("### Sending event: {event:#?}");
        sender.send(event.unwrap()).unwrap();
    });

    Ok(receiver)
}

#[tokio::test]
#[traced_test]
async fn test_upload() {
    let (args, _config_guard) = prepare_env("test_upload").await.unwrap();
    let base_url = args.base_url.clone();
    let (client, _state) = launch_env(args, TestUser::Admin).await.unwrap();

    // create format

    let _format = create_format(&client, &base_url, "epub", "application/epub+zip", "epub")
        .await
        .unwrap();

    // 1. upload file

    let ebook_file = "../../test-data/samples/Holmes.epub";
    info!("Current dir {:?}", std::env::current_dir().unwrap());
    assert!((tokio::fs::try_exists(ebook_file)).await.unwrap());

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
    info!("Response: {:?}", res);
    assert!(res.status().is_success());

    let upload_info: UploadInfo = res.json().await.unwrap();

    // 2. extract ebook metadata

    let meta_url = base_url.join("api/convert/extract_meta").unwrap();

    let res = client
        .post(meta_url)
        .json(&upload_info)
        .send()
        .await
        .unwrap();
    info!("Response: {:?}", res);
    assert!(res.status().is_success());

    let meta_ticket: serde_json::Map<String, Value> = res.json().await.unwrap();
    info!("Meta ticket: {:#?}", meta_ticket);
    let ticket_id = meta_ticket.get("id").unwrap().as_str().unwrap();

    // 3. wait for metadata event

    let sse_url = base_url.join("events").unwrap();
    let receiver = catch_event(client.clone(), sse_url, ticket_id.to_string()).unwrap();

    match tokio::time::timeout(std::time::Duration::from_secs(10), receiver).await {
        Ok(res) => {
            let res = res.unwrap();
            info!("Event response: {:#?}", res);
        }
        Err(_) => {
            panic!("Event timeout");
        }
    }

    // ### transaction ?
    // 4. create ebook - use metadata
    // 4.1. create author
    // 4.2. create genre
    // 4.3. create language
    // 4.4. create series?

    // 4.5. create ebook itself

    // 5. Create source
    // Create format
    // Move upload  file to destination
    // Move cover to source
    // Create source
    // Update ebook cover

    // 6.GET ebook

    // Download cover

    // Download ebook from source

    // Check files have same size and hash
}
