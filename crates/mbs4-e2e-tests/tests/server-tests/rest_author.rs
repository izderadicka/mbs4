use mbs4_dal::author::{Author, AuthorShort};
use mbs4_e2e_tests::{
    TestUser, admin_token, extend_url, launch_env, now, prepare_env, rest::create_author,
};
use tracing::info;
use tracing_test::traced_test;

#[tokio::test]
#[traced_test]
async fn test_authors() {
    let (args, _config_guard) = prepare_env("test_authors").await.unwrap();

    let base_url = args.base_url.clone();

    let (client, state) = launch_env(args, TestUser::TrustedUser).await.unwrap();

    let new_author: Author = create_author(&client, &base_url, "Usak", Some("Kulisak"))
        .await
        .unwrap();

    assert_eq!(1, new_author.version);
    let time_diff = now() - new_author.created;
    assert!(time::Duration::seconds(1) > time_diff);

    let id = new_author.id;
    info!("ID: {}", id);

    let api_url = base_url.join("api/author").unwrap();

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
    assert_eq!(rec.version, 2);

    //Can directly deserialize also short version
    let response = client.get(record_url.clone()).send().await.unwrap();
    info!("Response: {:#?}", response);
    assert!(response.status().is_success());

    let rec: AuthorShort = response.json().await.unwrap();
    assert_eq!(rec.last_name, "Kulisak");
    assert_eq!(rec.first_name, Some("Usak".into()));

    let response = client.get(api_url.clone()).send().await.unwrap();
    info!("Response: {:#?}", response);
    assert!(response.status().is_success());
    let res: serde_json::Value = response.json().await.unwrap();
    let recs = res.get("rows").unwrap().as_array().unwrap();
    assert_eq!(recs.len(), 1);

    let admin_creds = admin_token(&state).unwrap();
    let response = client
        .delete(record_url.clone())
        .header("Authorization", format!("Bearer {}", admin_creds))
        .send()
        .await
        .unwrap();
    info!("Response: {:#?}", response);
    assert!(response.status().is_success());

    let response = client.get(record_url.clone()).send().await.unwrap();
    assert!(!response.status().is_success());
    assert_eq!(response.status().as_u16(), 404);
}
