use mbs4_dal::language::{Language, LanguageShort};
use mbs4_e2e_tests::{TestUser, launch_env, prepare_env};
use reqwest::Url;
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
async fn test_paging() {
    let (args, _config_guard) = prepare_env("test_languages").await.unwrap();

    let base_url = args.base_url.clone();

    let mut count = 0;
    let conn = mbs4_dal::new_pool(&args.database_url).await.unwrap();
    let mut transaction = conn.begin().await.unwrap();

    for c1 in 'a'..='z' {
        for c2 in 'a'..='z' {
            let name = format!("Lang-{}{}", c1, c2);
            let code = format!("{}{}", c1, c2);
            sqlx::query("INSERT INTO language (name, code, version) VALUES (?, ?, 1)")
                .bind(&name)
                .bind(&code)
                .execute(&mut *transaction)
                .await
                .unwrap();

            count += 1;
        }
    }
    transaction.commit().await.unwrap();
    info!("Created {} languages", count);

    let (client, _) = launch_env(args, TestUser::User).await.unwrap();
    let api_url = base_url.join("api/language").unwrap();

    let count_url = extend_url(&api_url, "count");
    let response = client.get(count_url).send().await.unwrap();
    info! {"Response: {:#?}", response};
    assert!(response.status().is_success());
    let count: u64 = response.json().await.unwrap();
    assert_eq!(count, count as u64);

    let response = client.get(api_url.clone()).send().await.unwrap();
    info! {"Response: {:#?}", response};
    assert!(response.status().is_success());
    let page: Vec<LanguageShort> = response.json().await.unwrap();
    assert_eq!(100, page.len());

    let get_page = async |page: u64| {
        let mut page_url = api_url.clone();
        let query = format!("page={page}&page_size=50&sort=code");
        page_url.set_query(Some(&query));
        let response = client.get(page_url).send().await.unwrap();
        info! {"Response: {:#?}", response};
        assert!(response.status().is_success());
        let page: Vec<LanguageShort> = response.json().await.unwrap();
        page
    };

    let page: Vec<LanguageShort> = get_page(2).await;
    assert_eq!(50, page.len());
    assert_eq!("by", page[0].code);

    let page = get_page(1).await;
    assert_eq!(50, page.len());
    assert_eq!("aa", page[0].code);
    assert_eq!("bx", page[49].code);
}

fn extend_url(api_url: &Url, segment: impl ToString) -> Url {
    let mut record_url = api_url.clone();
    record_url
        .path_segments_mut()
        .unwrap()
        .push(&segment.to_string());
    record_url
}

#[tokio::test]
#[traced_test]
async fn test_languages() {
    let (args, _config_guard) = prepare_env("test_languages").await.unwrap();

    let base_url = args.base_url.clone();

    let (client, _) = launch_env(args, TestUser::Admin).await.unwrap();

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

    let record_url = extend_url(&api_url, id);

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
