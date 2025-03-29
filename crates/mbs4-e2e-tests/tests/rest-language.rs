use mbs4_dal::language::{Language, LanguageShort};
use mbs4_e2e_tests::{prepare_env, spawn_server};
use tracing::info;
use tracing_test::traced_test;

fn create_language(name: &str, code: &str, version: Option<i64>) -> serde_json::Value {
    match version {
        Some(v) => serde_json::json!({"name":name,"code":code,"version":v}),
        None => serde_json::json!({"name":name,"code":code}),
    }
}

#[tokio::test]
#[traced_test]
async fn test_languages() {
    let (args, _config_guard) = prepare_env("test_languages").await.unwrap();

    let base_url = args.base_url.clone();

    spawn_server(args).await.unwrap();
    let client = reqwest::Client::builder()
        .cookie_store(true)
        .build()
        .unwrap();

    let api_url = base_url.join("api/language").unwrap();
    let langs = [
        ("Czech", "cs"),
        ("English", "en"),
        ("Slovak", "sk"),
        ("Russian", "ru"),
    ];
    for (name, code) in langs.iter() {
        let l = create_language(name, code, None);
        let response = client.post(api_url.clone()).json(&l).send().await.unwrap();
        info!("Response: {:#?}", response);
        assert!(response.status().is_success());
        assert!(response.status().as_u16() == 201);
    }

    let response = client.get(api_url.clone()).send().await.unwrap();
    info! {"Response: {:#?}", response};
    assert!(response.status().is_success());
    let stored_langs: Vec<LanguageShort> = response.json().await.unwrap();
    assert_eq!(langs.len(), stored_langs.len());

    assert_eq!(stored_langs[3].name, "Russian");
    let id = stored_langs[3].id;
    info!("ID: {}", id);

    let mut record_url = api_url.clone();
    record_url
        .path_segments_mut()
        .unwrap()
        .push(&id.to_string());

    let response = client.get(record_url.clone()).send().await.unwrap();
    info!("Response: {:#?}", response);
    assert!(response.status().is_success());

    let rec: Language = response.json().await.unwrap();
    assert_eq!(rec.name, "Russian");

    let update_rec = create_language("Porussky", &rec.code, Some(rec.version));
    let response = client
        .put(record_url.clone())
        .json(&update_rec)
        .send()
        .await
        .unwrap();
    info!("Response: {:#?}", response);
    assert!(response.status().is_success());
    let new_rec: Language = response.json().await.unwrap();
    assert_eq!(new_rec.name, "Porussky");
    assert_eq!(new_rec.version, rec.version + 1);

    let update_rec = create_language("Porusskij", &rec.code, Some(rec.version));
    let response = client
        .put(record_url.clone())
        .json(&update_rec)
        .send()
        .await
        .unwrap();
    info!("Response: {:#?}", response);
    assert!(!response.status().is_success());
    assert_eq!(response.status().as_u16(), 409);

    let response = client.delete(record_url.clone()).send().await.unwrap();
    assert!(response.status().is_success());

    let response = client.get(record_url.clone()).send().await.unwrap();
    assert!(!response.status().is_success());
    assert_eq!(response.status().as_u16(), 404);

    let response = client.get(api_url.clone()).send().await.unwrap();
    info! {"Response: {:#?}", response};
    assert!(response.status().is_success());
    let stored_langs: Vec<LanguageShort> = response.json().await.unwrap();
    assert_eq!(langs.len() - 1, stored_langs.len());
}
