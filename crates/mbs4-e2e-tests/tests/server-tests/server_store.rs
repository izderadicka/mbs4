use std::path::Path;

use mbs4_app::store::rest_api::{RenameResult, UploadInfo};
use mbs4_e2e_tests::{TestUser, launch_env, prepare_env, random_text_file};
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
    let (args, _config_guard) = prepare_env("test_cover_upload").await.unwrap();
    let base_url = args.base_url.clone();

    let test_file_path = Path::new("../../test-data/samples/cover.jpg");
    assert!(fs::try_exists(test_file_path).await.unwrap());
    let file_size = fs::metadata(test_file_path).await.unwrap().len();

    let (client, _) = launch_env(args, TestUser::Admin).await.unwrap();

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
    let (args, config_guard) = prepare_env("test_store").await.unwrap();
    let base_url = args.base_url.clone();
    let tmp_dir = config_guard.path();

    let test_file_path = tmp_dir.join("my_test.txt");
    const FILE_SIZE: u64 = 50 * 1024;
    random_text_file(&test_file_path, FILE_SIZE).await.unwrap();

    let (client, _) = launch_env(args, TestUser::Admin).await.unwrap();

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
