//! End-to-end smoke for `POST /api/convert/batch`.
//!
//! Builds a bookshelf with two ebooks — one already in the target format
//! (epub) and one in a different format (mobi) — kicks off a batch
//! conversion targeting `epub`, drains SSE for the matching
//! `batch_complete` event, downloads the resulting ZIP, and verifies the
//! archive's contents and manifest entries.

use std::io::Read;

use futures::StreamExt;
use mbs4_app::{rest_api::ebook::EbookFileInfo, store::rest_api::UploadInfo};
use mbs4_dal::{author::Author, ebook::Ebook, source::Source};
use mbs4_e2e_tests::{
    TestUser, launch_env, prepare_env,
    rest::{create_author, create_ebook, create_format, create_language},
};
use reqwest::{Url, multipart};
use serde_json::{Map, Value, json};
use tracing::info;
use tracing_test::traced_test;

/// Drains the SSE stream and resolves once an event of `type` matching
/// `event_type` AND `data.operation_id` matching the supplied id is seen.
fn await_event(
    client: reqwest::Client,
    sse_url: Url,
    operation_id: String,
    event_type: &'static str,
) -> tokio::sync::oneshot::Receiver<Value> {
    let (sender, receiver) = tokio::sync::oneshot::channel();

    tokio::spawn(async move {
        let res = client
            .get(sse_url)
            .send()
            .await
            .expect("connect to SSE")
            .error_for_status()
            .expect("SSE HTTP status");

        let mut stream = res.bytes_stream();
        let mut buffer = String::new();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk.unwrap();
            buffer.push_str(&String::from_utf8_lossy(&chunk));

            while let Some(pos) = buffer.find("\n\n") {
                let raw = buffer[..pos].to_string();
                buffer.drain(..pos + 2);

                // SSE frames have lines like `id: ...`, `data: {...}`. We
                // only care about the JSON body, parsed as
                // {"type": "...", "data": {...}}.
                let json_line = raw
                    .lines()
                    .find_map(|l| l.strip_prefix("data:").map(|s| s.trim()));
                let Some(payload) = json_line else {
                    continue;
                };
                let value: Value = match serde_json::from_str(payload) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                if value["type"] != event_type {
                    continue;
                }
                if value["data"]["operation_id"].as_str() == Some(operation_id.as_str()) {
                    sender.send(value["data"].clone()).ok();
                    return;
                }
            }
        }
    });

    receiver
}

/// Uploads a file, registers an ebook with the supplied title and authors,
/// attaches the upload as a source, and returns the new ebook + source.
async fn upload_ebook_with_source(
    client: &reqwest::Client,
    base_url: &Url,
    file_path: &str,
    file_name: &str,
    title: &str,
    authors: &[i64],
    language_id: i64,
) -> (Ebook, Source) {
    let bytes = tokio::fs::read(file_path).await.expect("read sample file");
    let form = multipart::Form::new().part(
        "file",
        multipart::Part::bytes(bytes).file_name(file_name.to_string()),
    );
    let upload_url = base_url.join("files/upload/form").unwrap();
    let res = client
        .post(upload_url)
        .multipart(form)
        .send()
        .await
        .unwrap();
    assert!(res.status().is_success(), "upload failed: {}", res.status());
    let upload_info: UploadInfo = res.json().await.unwrap();

    let payload = json!({
        "title": title,
        "description": null,
        "series_id": null,
        "series_index": null,
        "language_id": language_id,
        "authors": authors,
        "genres": [],
        "created_by": "test",
    });
    let ebook: Ebook = create_ebook(client, base_url, &payload).await.unwrap();

    let source_url = base_url
        .join(&format!("api/ebook/{}/source", ebook.id))
        .unwrap();
    let file_info = EbookFileInfo {
        uploaded_file: upload_info.final_path,
        size: upload_info.size,
        hash: upload_info.hash.clone(),
        quality: None,
    };
    let res = client
        .post(source_url)
        .json(&file_info)
        .send()
        .await
        .unwrap();
    assert!(
        res.status().is_success(),
        "add source failed: {}",
        res.status()
    );
    let source: Source = res.json().await.unwrap();
    (ebook, source)
}

#[tokio::test]
#[traced_test]
async fn test_batch_convert_bookshelf() {
    let (args, mut guard) = prepare_env("test_batch_convert").await.unwrap();
    let base_url = args.base_url.clone();
    let (client, _state) = launch_env(args, TestUser::Admin, &mut guard).await.unwrap();

    // Formats, language, author — minimal scaffolding for two ebooks.
    let _epub_format = create_format(&client, &base_url, "EPUB", "application/epub+zip", "epub")
        .await
        .unwrap();
    let _mobi_format = create_format(
        &client,
        &base_url,
        "MOBI",
        "application/x-mobipocket-ebook",
        "mobi",
    )
    .await
    .unwrap();
    let language = create_language(&client, &base_url, "English", "en")
        .await
        .unwrap();
    let author: Author = create_author(&client, &base_url, "Tester", Some("Sample"))
        .await
        .unwrap();

    // Ebook 1: already in the target format (epub). The batch runner should
    // mark this as REUSED-source and not invoke ebook-convert for it.
    let (ebook_epub, _src_epub) = upload_ebook_with_source(
        &client,
        &base_url,
        "../../test-data/samples/Dabel.epub",
        "Dabel.epub",
        "Dabel",
        &[author.id],
        language.id,
    )
    .await;

    // Ebook 2: a mobi source — actual conversion to epub via calibre.
    let (ebook_mobi, _src_mobi) = upload_ebook_with_source(
        &client,
        &base_url,
        "../../test-data/samples/Mura.mobi",
        "Mura.mobi",
        "Mura",
        &[author.id],
        language.id,
    )
    .await;

    // Bookshelf containing both ebooks.
    let res = client
        .post(base_url.join("api/bookshelf").unwrap())
        .json(&json!({ "name": "Batch shelf", "public": false }))
        .send()
        .await
        .unwrap();
    assert!(res.status().is_success(), "bookshelf: {}", res.status());
    let body: Value = res.json().await.unwrap();
    let bookshelf_id = body["id"].as_i64().unwrap();

    for ebook_id in [ebook_epub.id, ebook_mobi.id] {
        let res = client
            .post(
                base_url
                    .join(&format!("api/bookshelf/{bookshelf_id}/items"))
                    .unwrap(),
            )
            .json(&json!({
                "item_type": "EBOOK",
                "ebook_id": ebook_id,
            }))
            .send()
            .await
            .unwrap();
        assert!(res.status().is_success(), "add item: {}", res.status());
    }

    // Kick off the batch.
    let res = client
        .post(base_url.join("api/convert/batch").unwrap())
        .json(&json!({
            "for_entity": "BOOKSHELF",
            "entity_id": bookshelf_id,
            "to_format_extension": "epub",
        }))
        .send()
        .await
        .unwrap();
    assert!(res.status().is_success(), "batch start: {}", res.status());
    let ticket: Map<String, Value> = res.json().await.unwrap();
    let operation_id = ticket["operation_id"].as_str().unwrap().to_string();
    let batch_id = ticket["batch_id"].as_i64().unwrap();
    assert_eq!(ticket["total"].as_u64().unwrap(), 2);
    assert_eq!(ticket["dropped"].as_u64().unwrap(), 0);
    info!("Batch operation_id={operation_id} batch_id={batch_id}");

    // Listen for batch_complete on the SSE channel.
    let sse_url = base_url.join("events").unwrap();
    let receiver = await_event(
        client.clone(),
        sse_url,
        operation_id.clone(),
        "batch_complete",
    );
    let complete = tokio::time::timeout(std::time::Duration::from_secs(120), receiver)
        .await
        .expect("batch_complete timeout")
        .expect("event channel closed");
    info!("batch_complete: {complete:#?}");

    assert_eq!(complete["batch_id"].as_i64().unwrap(), batch_id);
    assert_eq!(complete["total"].as_u64().unwrap(), 2);
    // Exactly one must come from each path; assert the totals are coherent
    // rather than which-ebook-took-which-branch.
    let ok = complete["ok"].as_u64().unwrap();
    let reused = complete["reused"].as_u64().unwrap();
    let failed = complete["failed"].as_u64().unwrap();
    assert_eq!(ok + reused, 2, "all items should succeed");
    assert_eq!(failed, 0);
    assert!(complete["zip_location"].is_string(), "zip must be created");
    let zip_location = complete["zip_location"].as_str().unwrap().to_string();

    // Download the ZIP via the existing store download route.
    let download_url = base_url
        .join(&format!("files/download/conversion/{zip_location}"))
        .unwrap();
    let res = client.get(download_url).send().await.unwrap();
    assert!(res.status().is_success(), "download: {}", res.status());
    let zip_bytes = res.bytes().await.unwrap();
    assert!(zip_bytes.len() > 100, "zip too small to be valid");

    // Inspect the archive.
    let cursor = std::io::Cursor::new(zip_bytes.to_vec());
    let mut archive = zip::ZipArchive::new(cursor).expect("valid zip");
    let names: Vec<String> = (0..archive.len())
        .map(|i| archive.by_index(i).unwrap().name().to_string())
        .collect();
    info!("ZIP entries: {names:?}");

    assert!(
        names.iter().any(|n| n == "manifest.txt"),
        "manifest missing"
    );
    let ebook_entries: Vec<&String> = names.iter().filter(|n| *n != "manifest.txt").collect();
    assert_eq!(ebook_entries.len(), 2, "expected one entry per ebook");
    for entry in &ebook_entries {
        assert!(
            entry.ends_with(".epub"),
            "entry {entry:?} not in target format"
        );
    }

    // Manifest should record both items.
    let mut manifest_buf = String::new();
    archive
        .by_name("manifest.txt")
        .unwrap()
        .read_to_string(&mut manifest_buf)
        .unwrap();
    info!("manifest:\n{manifest_buf}");
    assert!(manifest_buf.contains("Dabel"));
    assert!(manifest_buf.contains("Mura"));
    // At least one of {OK,REUSED} per ebook → 2 status lines total.
    let status_lines = manifest_buf
        .lines()
        .filter(|l| l.starts_with("OK") || l.starts_with("REUSED"))
        .count();
    assert_eq!(status_lines, 2);

    // GET endpoints reflect the new batch.
    let res = client
        .get(base_url.join("api/conversion-batch").unwrap())
        .send()
        .await
        .unwrap();
    assert!(res.status().is_success(), "list batches: {}", res.status());
    let listing: Value = res.json().await.unwrap();
    let rows = listing["rows"].as_array().unwrap();
    assert!(rows.iter().any(|r| r["id"].as_i64() == Some(batch_id)));

    let res = client
        .get(
            base_url
                .join(&format!("api/conversion-batch/{batch_id}/items"))
                .unwrap(),
        )
        .send()
        .await
        .unwrap();
    assert!(res.status().is_success(), "list items: {}", res.status());
    let items: Vec<Value> = res.json().await.unwrap();
    assert_eq!(items.len(), 2);
}
