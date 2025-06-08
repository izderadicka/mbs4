use anyhow::Result;
use axum::body;
use mbs4_dal::{author::Author, ebook::Ebook, genre::Genre, language::Language, series::Series};
use mbs4_e2e_tests::{TestUser, admin_token, extend_url, launch_env, now, prepare_env};
use rand::rand_core::le;
use reqwest::Url;
use serde_json::json;
use tracing::info;
use tracing_test::traced_test;

async fn create_author(
    client: &reqwest::Client,
    base_url: &Url,
    last_name: &str,
    first_name: Option<&str>,
) -> Result<Author> {
    let payload = json!({"first_name": first_name, "last_name": last_name});
    let api_url = base_url.join("api/author").unwrap();

    let response = client.post(api_url.clone()).json(&payload).send().await?;
    assert!(response.status().is_success());

    let new_author: Author = response.json().await?;

    Ok(new_author)
}

async fn create_genre(client: &reqwest::Client, base_url: &Url, name: &str) -> Result<Genre> {
    let payload = json!({"name": name});
    let api_url = base_url.join("api/genre").unwrap();

    let response = client.post(api_url.clone()).json(&payload).send().await?;
    assert!(response.status().is_success());

    let new_genre: Genre = response.json().await?;
    Ok(new_genre)
}

async fn create_series(client: &reqwest::Client, base_url: &Url, title: &str) -> Result<Series> {
    let payload = json!({"title": title});
    let api_url = base_url.join("api/series").unwrap();

    let response = client.post(api_url.clone()).json(&payload).send().await?;
    info!("Series Response: {:#?}", response);
    assert!(response.status().is_success());

    let new_series: Series = response.json().await?;
    Ok(new_series)
}

async fn create_language(
    client: &reqwest::Client,
    base_url: &Url,
    name: &str,
    code: &str,
) -> Result<Language> {
    let payload = json!({"name": name, "code": code});
    let api_url = base_url.join("api/language").unwrap();

    let response = client.post(api_url.clone()).json(&payload).send().await?;
    assert!(response.status().is_success());

    let new_language: Language = response.json().await?;
    Ok(new_language)
}

async fn search(
    client: &reqwest::Client,
    base_url: &Url,
    query: &str,
) -> Result<Vec<serde_json::Value>> {
    let search_api_url = base_url.join("search")?;
    let response = client
        .get(search_api_url)
        .query(&[("query", query)])
        .send()
        .await?;
    info!("Response: {:#?}", response);
    assert!(response.status().is_success());
    let body = response.text().await?;
    info!("Response body: {:#?}", body);
    let search_result: serde_json::Value = serde_json::from_str(&body)?;
    let found = search_result
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("Not array"))?;
    Ok(found.to_vec())
}

#[tokio::test]
#[traced_test]
async fn test_ebook() {
    let (args, _config_guard) = prepare_env("test_ebook").await.unwrap();
    let base_url = args.base_url.clone();
    let (client, state) = launch_env(args, TestUser::Admin).await.unwrap();

    let author1 = create_author(&client, &base_url, "Usak", Some("Kulisak"))
        .await
        .unwrap();
    let author2 = create_author(&client, &base_url, "Makac", Some("Jan"))
        .await
        .unwrap();

    let (genre1, genre2, genre3) = tokio::try_join!(
        create_genre(&client, &base_url, "Horror"),
        create_genre(&client, &base_url, "Thriller"),
        create_genre(&client, &base_url, "Fantasy")
    )
    .unwrap();

    let series = create_series(&client, &base_url, "Dune").await.unwrap();
    let lang = create_language(&client, &base_url, "English", "en")
        .await
        .unwrap();

    let payload = json!({
            "title": "Dune",
            "authors": [author1.id, author2.id],
            "genres": [genre1.id, genre2.id, genre3.id],
            "series_id": series.id,
            "series_index": 1,
            "language_id": lang.id,
    });

    let api_url = base_url.join("api/ebook").unwrap();

    let response = client
        .post(api_url.clone())
        .json(&payload)
        .send()
        .await
        .unwrap();
    info!("Response: {:#?}", response);
    assert!(response.status().is_success());
    assert!(response.status().as_u16() == 201);
    let body = response.text().await.unwrap();
    info!("Response body: {:#?}", body);
    let new_ebook: Ebook = serde_json::from_str(&body).unwrap();
    assert_eq!(new_ebook.title, "Dune");
    assert_eq!(new_ebook.authors.unwrap().len(), 2);
    assert_eq!(new_ebook.genres.unwrap().len(), 3);
    assert_eq!(new_ebook.series.unwrap().id, series.id);
    assert_eq!(new_ebook.series_index, Some(1));
    assert_eq!(new_ebook.language.id, lang.id);

    //should be searchable now, to be sure wait a bit for indexing
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;

    let found = search(&client, &base_url, "Dune").await.unwrap();
    assert_eq!(found.len(), 1);
    let found_ebook = found.get(0).unwrap()["doc"].as_object().unwrap();
    assert_eq!(found_ebook["title"].as_str().unwrap(), "Dune");
    assert_eq!(found_ebook["authors"].as_array().unwrap().len(), 2);

    let series2 = create_series(&client, &base_url, "Adventures")
        .await
        .unwrap();

    let update_payload = json!({
            "title": "Holmes",
            "authors": [author1.id],
            "genres": [genre2.id, genre3.id],
            "series_id": series2.id,
            "series_index": 1,
            "language_id": lang.id,
            "id": new_ebook.id,
            "version": new_ebook.version
    });

    let ebook_url = extend_url(&api_url, new_ebook.id);

    let response = client
        .put(ebook_url.clone())
        .json(&update_payload)
        .send()
        .await
        .unwrap();
    info!("Response: {:#?}", response);
    assert!(response.status().is_success());
    assert!(response.status().as_u16() == 200);
    let body = response.text().await.unwrap();
    info!("Response body: {:#?}", body);
    let updated_ebook: Ebook = serde_json::from_str(&body).unwrap();
    assert_eq!(updated_ebook.title, "Holmes");
    assert_eq!(updated_ebook.authors.unwrap().len(), 1);
    assert_eq!(updated_ebook.genres.unwrap().len(), 2);
    assert_eq!(updated_ebook.series.unwrap().id, series2.id);
    assert_eq!(updated_ebook.series_index, Some(1));
    assert_eq!(updated_ebook.language.id, lang.id);
    assert!(updated_ebook.version > new_ebook.version);
    assert!(updated_ebook.modified > new_ebook.created);

    tokio::time::sleep(std::time::Duration::from_millis(10)).await;

    let found = search(&client, &base_url, "Dune").await.unwrap();
    assert_eq!(found.len(), 0);
    let found = search(&client, &base_url, "Holmes").await.unwrap();
    assert_eq!(found.len(), 1);

    let response = client.delete(ebook_url.clone()).send().await.unwrap();
    info!("Response: {:#?}", response);
    assert!(response.status().is_success());
    assert!(response.status().as_u16() == 204);

    let pool = state.pool();

    let num_links: u64 = sqlx::query_scalar("select count(*) from ebook_authors")
        .fetch_one(pool)
        .await
        .unwrap();

    assert_eq!(num_links, 0);

    let num_links: u64 = sqlx::query_scalar("select count(*) from ebook_genres")
        .fetch_one(pool)
        .await
        .unwrap();

    assert_eq!(num_links, 0);

    let response = client.get(ebook_url.clone()).send().await.unwrap();
    info!("Response: {:#?}", response);
    assert!(response.status().as_u16() == 404);

    let response = client.get(api_url.clone()).send().await.unwrap();
    info!("Response: {:#?}", response);
    assert!(response.status().is_success());
    let ebooks: serde_json::Value = response.json().await.unwrap();
    let rows = ebooks["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 0);
}
