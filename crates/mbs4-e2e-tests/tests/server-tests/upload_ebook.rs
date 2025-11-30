use futures::StreamExt;
use mbs4_app::{
    rest_api::ebook::{EbookCoverInfo, EbookFileInfo},
    store::rest_api::UploadInfo,
};
use mbs4_calibre::meta::EbookMetadata;
use mbs4_dal::{
    ebook::{CreateEbook, Ebook},
    source::Source,
};
use mbs4_e2e_tests::{
    TestUser, launch_env, prepare_env,
    rest::{create_author, create_ebook, create_format, create_genre, create_language},
};
use reqwest::{Url, multipart};
use serde_json::{Map, Value};
use sha2::Digest;
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
    let (sender, receiver) = tokio::sync::oneshot::channel();

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

    let meta;
    match tokio::time::timeout(std::time::Duration::from_secs(10), receiver).await {
        Ok(res) => {
            let res = res.unwrap();
            meta = res["metadata"].clone();
        }
        Err(_) => {
            panic!("Event timeout");
        }
    }
    info!("Meta: {:#?}", meta);

    let meta: EbookMetadata = serde_json::from_value(meta).unwrap();

    // ### transaction ?
    // 4. create ebook - use metadata
    // 4.1. create author
    let mut authors = Vec::new();
    for author in meta.authors {
        let new_author = create_author(
            &client,
            &base_url,
            &author.last_name,
            author.first_name.as_deref(),
        )
        .await
        .unwrap();

        authors.push(new_author.id);
    }

    // 4.2. create genre
    let mut genres = Vec::new();
    for genre in meta.genres {
        let new_genre = create_genre(&client, &base_url, &genre).await.unwrap();
        genres.push(new_genre.id);
    }
    // 4.3. create language
    let eng_lang = create_language(&client, &base_url, "English", "en")
        .await
        .unwrap();
    let eng_lang_id = eng_lang.id;
    // 4.4. create series? Not in metadata

    // 4.5. create ebook itself

    let new_ebook = CreateEbook {
        title: meta.title.unwrap(),
        description: meta.comments,
        series_id: None,
        series_index: None,
        language_id: eng_lang_id,
        authors: Some(authors),
        genres: Some(genres),
        created_by: Some("test".to_string()),
    };

    let new_ebook = create_ebook(&client, &base_url, &new_ebook).await.unwrap();

    // 5. Create source
    let ebook_dir = new_ebook.base_dir;

    // Create source

    let source_url = base_url
        .join(&format!("api/ebook/{}/source", new_ebook.id))
        .unwrap();
    let ebook_file_info = EbookFileInfo {
        uploaded_file: upload_info.final_path,
        size: upload_info.size,
        hash: upload_info.hash.clone(),
        quality: None,
    };

    let res = client
        .post(source_url.clone())
        .json(&ebook_file_info)
        .send()
        .await
        .unwrap();
    info!("Response: {:?}", res);
    assert!(res.status().is_success());
    assert!(res.status().as_u16() == 201);

    let source: Source = res.json().await.unwrap();

    info!("Source: {:#?} and ebook_dir: {}", source, ebook_dir);

    assert!(source.location.starts_with(&ebook_dir));
    assert!(source.location.ends_with(".epub"));
    assert_eq!(new_ebook.id, source.ebook_id);
    // Update ebook cover

    let cover_url = base_url
        .join(&format!("api/ebook/{}/cover", new_ebook.id))
        .unwrap();
    let cover_info = EbookCoverInfo {
        cover_file: meta.cover_file,
        ebook_id: new_ebook.id,
        ebook_version: new_ebook.version,
    };

    let res = client
        .put(cover_url.clone())
        .json(&cover_info)
        .send()
        .await
        .unwrap();
    info!("Response: {:?}", res);
    assert!(res.status().is_success());
    assert!(res.status().as_u16() == 200);

    let updated_ebook: Ebook = res.json().await.unwrap();
    assert_eq!(updated_ebook.id, new_ebook.id);
    assert_eq!(updated_ebook.version, new_ebook.version + 1);
    assert!(
        updated_ebook
            .cover
            .as_ref()
            .unwrap()
            .starts_with(&ebook_dir)
    );
    assert!(updated_ebook.cover.as_ref().unwrap().ends_with(".jpg"));

    // 6. check files

    // Download cover

    let cover_download_url = base_url
        .join(&format!("files/download/{}", updated_ebook.cover.unwrap()))
        .unwrap();

    let res = client.get(cover_download_url).send().await.unwrap();
    assert!(res.status().is_success());
    let mut stream = res.bytes_stream();

    let mut size = 0;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.unwrap();
        size = size + chunk.len() as u64;
    }

    assert!(size > 1000);

    // Download ebook from source
    let res = client.get(source_url).send().await.unwrap();
    assert!(res.status().is_success());

    let sources: Vec<Map<String, Value>> = res.json().await.unwrap();
    assert_eq!(sources.len(), 1);
    let location = sources[0]["location"].as_str().unwrap();
    let format_ext = sources[0]["format_extension"].as_str().unwrap();
    assert_eq!("epub", format_ext);
    let download_url = base_url
        .join(&format!("files/download/{}", location))
        .unwrap();

    let res = client.get(download_url).send().await.unwrap();
    assert!(res.status().is_success());
    let mut stream = res.bytes_stream();

    let mut size = 0;
    let mut digester = sha2::Sha256::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.unwrap();
        size = size + chunk.len() as u64;
        digester.update(&chunk);
    }

    let hash = digester.finalize();
    let hash = base16ct::lower::encode_string(hash.as_slice());

    // Check files have same size and hash
    assert_eq!(upload_info.size, size);
    assert_eq!(upload_info.hash, hash);

    // Check conversion

    let conversion_url = base_url
        .join(&format!("api/ebook/{}/conversion", new_ebook.id))
        .unwrap();

    let res = client.get(conversion_url).send().await.unwrap();
    assert!(res.status().is_success());
    assert!(res.status().as_u16() == 200);

    let conversions: Vec<Map<String, Value>> = res.json().await.unwrap();
    assert_eq!(conversions.len(), 0);
}
