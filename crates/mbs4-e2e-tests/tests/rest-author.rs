use mbs4_dal::author::{Author, UpdateAuthor};
use mbs4_e2e_tests::{TestUser, extend_url, launch_env, now, prepare_env};
use tracing::info;
use tracing_test::traced_test;

fn create_author(last_name: &str, first_name: Option<&str>) -> serde_json::Value {
    serde_json::json!({"first_name": first_name, "last_name": last_name})
}

#[tokio::test]
#[traced_test]
async fn test_authors() {
    let (args, _config_guard) = prepare_env("test_authors").await.unwrap();

    let base_url = args.base_url.clone();

    let (client, _) = launch_env(args, TestUser::TrustedUser).await.unwrap();

    let api_url = base_url.join("api/author").unwrap();

    let author = create_author("Usak", Some("Kulisak"));
    let response = client
        .post(api_url.clone())
        .json(&author)
        .send()
        .await
        .unwrap();
    info!("Response: {:#?}", response);
    assert!(response.status().is_success());
    assert!(response.status().as_u16() == 201);

    let new_author: Author = response.json().await.unwrap();

    assert_eq!(1, new_author.version);
    let time_diff = now() - new_author.created;
    assert!(time::Duration::seconds(1) > time_diff);

    let id = new_author.id;
    info!("ID: {}", id);

    let record_url = extend_url(&api_url, id);

    let response = client.get(record_url.clone()).send().await.unwrap();
    info!("Response: {:#?}", response);
    assert!(response.status().is_success());

    let rec: Author = response.json().await.unwrap();
    assert_eq!(rec.last_name, "Usak");

    let updated_author = serde_json::json!({
        "id": rec.id,
        "first_name": Some(rec.last_name.clone()),
        "last_name": rec.first_name.clone().unwrap(),
        "description": rec.description.clone(),
        "version": rec.version,
    });

    let response = client
        .put(record_url.clone())
        .json(&updated_author)
        .send()
        .await
        .unwrap();
    info!("Response: {:#?}", response);
    assert!(response.status().is_success());
    assert!(response.status().as_u16() == 200);

    let response = client.get(record_url.clone()).send().await.unwrap();
    info!("Response: {:#?}", response);
    assert!(response.status().is_success());

    let rec: Author = response.json().await.unwrap();
    assert_eq!(rec.last_name, "Kulisak");
    assert_eq!(rec.first_name, Some("Usak".into()));

    // let response = client.delete(record_url.clone()).send().await.unwrap();
    // assert!(response.status().is_success());

    // let response = client.get(record_url.clone()).send().await.unwrap();
    // assert!(!response.status().is_success());
    // assert_eq!(response.status().as_u16(), 404);
}
