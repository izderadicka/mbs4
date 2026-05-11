use std::path::Path;

use mbs4_app::{
    rest_api::ebook::EbookFileInfo,
    store::rest_api::{RenameResult, UploadInfo},
};
use mbs4_e2e_tests::{
    TestUser, launch_env, prepare_env, random_text_file,
    rest::{create_ebook, create_format as rest_create_format, create_language},
};
use reqwest::{
    Body,
    header::CONTENT_TYPE,
    multipart::{Form, Part},
};
use tokio::fs::{self, File};
use tokio_util::io::ReaderStream;
use tracing_test::traced_test;

fn create_format(name: &str, extension: &str, mime_type: &str) -> serde_json::Value {
    serde_json::json!({"name": name, "extension": extension, "mime_type": mime_type })
}

#[tokio::test]
#[traced_test]
async fn test_cover_upload() {
    let (args, mut _config_guard) = prepare_env("test_cover_upload").await.unwrap();
    let base_url = args.base_url.clone();

    let test_file_path = Path::new("../../test-data/samples/cover.jpg");
    assert!(fs::try_exists(test_file_path).await.unwrap());
    let file_size = fs::metadata(test_file_path).await.unwrap().len();

    let (client, _) = launch_env(args, TestUser::Admin, &mut _config_guard)
        .await
        .unwrap();

    let url = base_url.join("files/upload/form").unwrap();
    let file = File::open(&test_file_path).await.unwrap();
    let stream = ReaderStream::new(file);
    let kind_part = Part::text("Cover");
    let file_part = Part::stream(Body::wrap_stream(stream))
        .file_name(test_file_path.file_name().unwrap().to_str().unwrap());
    let form = Form::new().part("kind", kind_part).part("file", file_part);

    let response = client.post(url).multipart(form).send().await.unwrap();
    assert_eq!(response.status().as_u16(), 201);
    let info: UploadInfo = response.json().await.unwrap();
    assert_eq!(info.size, file_size);
    assert!(info.final_path.ends_with(".jpg"));
    assert!(!info.final_path.contains('/'));
}

#[tokio::test]
#[traced_test]
async fn test_store() {
    let (args, mut config_guard) = prepare_env("test_store").await.unwrap();
    let base_url = args.base_url.clone();
    let tmp_dir = config_guard.path();

    let test_file_path = tmp_dir.join("my_test.txt");
    const FILE_SIZE: u64 = 50 * 1024;
    random_text_file(&test_file_path, FILE_SIZE).await.unwrap();

    let (client, _) = launch_env(args, TestUser::Admin, &mut config_guard)
        .await
        .unwrap();

    let url = base_url.join("api/format").unwrap();
    let response = client
        .post(url)
        .json(&create_format("text", "txt", "text/plain"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status().as_u16(), 201);

    let url = base_url.join("files/download/tmp/test.txt").unwrap();
    let response = client.get(url).send().await.unwrap();
    assert_eq!(response.status().as_u16(), 404);

    let url = base_url.join("files/upload/form").unwrap();
    let file = File::open(&test_file_path).await.unwrap();
    let stream = ReaderStream::new(file);
    let kind_part = Part::text("Ebook");
    let file_part = Part::stream(Body::wrap_stream(stream)).file_name("my_test.txt");
    let form = Form::new().part("kind", kind_part).part("file", file_part);

    let response = client.post(url).multipart(form).send().await.unwrap();
    assert_eq!(response.status().as_u16(), 201);
    let info: UploadInfo = response.json().await.unwrap();
    assert_eq!(info.size, FILE_SIZE);
    assert!(info.final_path.ends_with(".txt"));
    assert!(!info.final_path.contains('/'));

    let url = base_url.join("files/upload/direct").unwrap();
    let file = File::open(&test_file_path).await.unwrap();
    let response = client
        .post(url)
        .body(file)
        .header(CONTENT_TYPE, "text/plain")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status().as_u16(), 201);
    let info2: UploadInfo = response.json().await.unwrap();
    assert_eq!(info2.size, FILE_SIZE);
    assert!(info2.final_path.ends_with("txt"));
    let move_upload = async |path| {
        let url = base_url.join("files/move/upload").unwrap();
        let body = serde_json::json!({
        "from_path": path,
        "to_path": "tmp/my_test.txt"});
        let response = client.post(url).json(&body).send().await.unwrap();
        assert_eq!(response.status().as_u16(), 200);
        response.json::<RenameResult>().await.unwrap()
    };

    let res = move_upload(info.final_path).await;
    assert_eq!("tmp/my_test.txt", res.final_path);
    let res = move_upload(info2.final_path).await;
    assert_eq!("tmp/my_test(1).txt", res.final_path);

    let original = tokio::fs::read(test_file_path).await.unwrap();
    for sufix in ["", "(1)"] {
        let path = format!("files/download/tmp/my_test{sufix}.txt");
        let url = base_url.join(&path).unwrap();
        let response = client.get(url).send().await.unwrap();
        assert_eq!(response.status().as_u16(), 200);
        let size = response.content_length().unwrap();
        assert_eq!(size, FILE_SIZE);
        let body = response.bytes().await.unwrap();
        assert_eq!(body, original);
    }
}

#[tokio::test]
#[traced_test]
async fn test_duplicate_upload_rejected() {
    let (args, mut config_guard) = prepare_env("test_duplicate_upload").await.unwrap();
    let base_url = args.base_url.clone();
    let tmp_dir = config_guard.path().to_path_buf();
    const FILE_SIZE: u64 = 8 * 1024;

    let test_file_path = tmp_dir.join("dedup_test.txt");
    random_text_file(&test_file_path, FILE_SIZE).await.unwrap();

    let (client, _) = launch_env(args, TestUser::Admin, &mut config_guard)
        .await
        .unwrap();

    // Register a text format and create an ebook so we can register a source.
    let _fmt = rest_create_format(&client, &base_url, "text", "text/plain", "txt")
        .await
        .unwrap();
    let lang = create_language(&client, &base_url, "English", "en")
        .await
        .unwrap();
    let ebook = create_ebook(
        &client,
        &base_url,
        &serde_json::json!({"title": "Dedup Test Book", "language_id": lang.id}),
    )
    .await
    .unwrap();

    // First upload: store the file in the upload area.
    let upload_info: UploadInfo = {
        let file = File::open(&test_file_path).await.unwrap();
        let stream = ReaderStream::new(file);
        let form = Form::new().part("kind", Part::text("Ebook")).part(
            "file",
            Part::stream(Body::wrap_stream(stream)).file_name("dedup_test.txt"),
        );
        let url = base_url.join("files/upload/form").unwrap();
        let res = client.post(url).multipart(form).send().await.unwrap();
        assert_eq!(res.status().as_u16(), 201, "first upload should succeed");
        res.json().await.unwrap()
    };

    // Register the upload as an ebook source — this records the hash in the DB
    // and moves the file from the upload area to the books area.
    let source_url = base_url
        .join(&format!("api/ebook/{}/source", ebook.id))
        .unwrap();
    let file_info = EbookFileInfo {
        uploaded_file: upload_info.final_path.clone(),
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
        "source creation should succeed: {}",
        res.status()
    );

    // Second upload of the same file content → must be rejected because the hash is now in the
    // source table.
    let file = File::open(&test_file_path).await.unwrap();
    let stream = ReaderStream::new(file);
    let form = Form::new().part("kind", Part::text("Ebook")).part(
        "file",
        Part::stream(Body::wrap_stream(stream)).file_name("dedup_test.txt"),
    );
    let url = base_url.join("files/upload/form").unwrap();
    let res = client.post(url).multipart(form).send().await.unwrap();
    assert_eq!(
        res.status().as_u16(),
        409,
        "duplicate upload should return 409 Conflict"
    );

    // Verify a different file still uploads fine (dedup is hash-based, not name-based).
    let other_path = tmp_dir.join("other.txt");
    random_text_file(&other_path, FILE_SIZE).await.unwrap();
    let file = File::open(&other_path).await.unwrap();
    let stream = ReaderStream::new(file);
    let form = Form::new().part("kind", Part::text("Ebook")).part(
        "file",
        Part::stream(Body::wrap_stream(stream)).file_name("dedup_test.txt"),
    );
    let url = base_url.join("files/upload/form").unwrap();
    let res = client.post(url).multipart(form).send().await.unwrap();
    assert_eq!(
        res.status().as_u16(),
        201,
        "different file should still upload: {}",
        res.status()
    );
}
