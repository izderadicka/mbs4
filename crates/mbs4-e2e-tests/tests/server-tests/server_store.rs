use mbs4_app::store::rest_api::UploadInfo;
use mbs4_e2e_tests::{TestUser, launch_env, prepare_env, random_text_file};
use reqwest::{
    Body,
    header::CONTENT_TYPE,
    multipart::{Form, Part},
};
use tokio::fs::File;
use tokio_util::io::ReaderStream;
use tracing_test::traced_test;

#[tokio::test]
#[traced_test]
async fn test_store() {
    let (args, config_guard) = prepare_env("test_health").await.unwrap();
    let base_url = args.base_url.clone();
    let tmp_dir = config_guard.path();

    let test_file_path = tmp_dir.join("my_test.txt");
    const FILE_SIZE: u64 = 50 * 1024;
    random_text_file(&test_file_path, FILE_SIZE).await.unwrap();

    let (client, _) = launch_env(args, TestUser::TrustedUser).await.unwrap();

    let url = base_url.join("store/download/tmp/test.txt").unwrap();
    let response = client.get(url).send().await.unwrap();
    assert_eq!(response.status().as_u16(), 404);

    let url = base_url.join("store/upload/form/tmp/").unwrap();
    let file = File::open(&test_file_path).await.unwrap();
    let stream = ReaderStream::new(file);
    let part = Part::stream(Body::wrap_stream(stream)).file_name("my_test.txt");
    let form = Form::new().part("file", part);

    let response = client.post(url).multipart(form).send().await.unwrap();
    assert_eq!(response.status().as_u16(), 201);
    let info: UploadInfo = response.json().await.unwrap();
    assert_eq!(info.size, FILE_SIZE);
    assert_eq!(info.final_path, "tmp/my_test.txt");

    let url = base_url
        .join("store/upload/direct/tmp/my_test.txt")
        .unwrap();
    let file = File::open(&test_file_path).await.unwrap();
    let response = client
        .post(url)
        .body(file)
        .header(CONTENT_TYPE, "text/plain")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status().as_u16(), 201);
    let info: UploadInfo = response.json().await.unwrap();
    assert_eq!(info.size, FILE_SIZE);
    assert_eq!(info.final_path, "tmp/my_test(1).txt");

    let original = tokio::fs::read(test_file_path).await.unwrap();

    for sufix in ["", "(1)"] {
        let path = format!("store/download/tmp/my_test{sufix}.txt");
        let url = base_url.join(&path).unwrap();
        let response = client.get(url).send().await.unwrap();
        assert_eq!(response.status().as_u16(), 200);
        let size = response.content_length().unwrap();
        assert_eq!(size, FILE_SIZE);
        let body = response.bytes().await.unwrap();
        assert_eq!(body, original);
    }
}
