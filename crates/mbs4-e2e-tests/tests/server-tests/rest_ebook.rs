use anyhow::Result;
use mbs4_dal::ebook::Ebook;
use mbs4_e2e_tests::{
    TestUser, extend_url, launch_env, prepare_env,
    rest::{create_author, create_ebook, create_genre, create_language, create_series},
};
use reqwest::{StatusCode, Url};
use serde_json::{Value, json};
use tracing::info;
use tracing_test::traced_test;

async fn search(
    client: &reqwest::Client,
    base_url: &Url,
    query: &str,
    expected: usize,
) -> Result<Vec<serde_json::Value>> {
    let mut retries = 20;
    let mut found = Vec::new();
    while retries > 0 {
        found = _search(&client, &base_url, query).await.unwrap();
        if found.len() == expected {
            break;
        }
        retries -= 1;
        if retries == 0 {
            panic!("Not found in search");
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }

    assert_eq!(found.len(), expected, "Searching: {}", query);
    Ok(found)
}

async fn _search(
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
    let (args, mut _config_guard) = prepare_env("test_ebook").await.unwrap();
    let base_url = args.base_url.clone();
    let (client, state) = launch_env(args, TestUser::Admin, &mut _config_guard)
        .await
        .unwrap();

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

    let new_ebook = create_ebook(&client, &base_url, &payload).await.unwrap();
    assert_eq!(new_ebook.title, "Dune");
    assert_eq!(new_ebook.authors.unwrap().len(), 2);
    assert_eq!(new_ebook.genres.unwrap().len(), 3);
    assert_eq!(new_ebook.series.unwrap().id, series.id);
    assert_eq!(new_ebook.series_index, Some(1));
    assert_eq!(new_ebook.language.id, lang.id);

    let found = search(&client, &base_url, "Dune", 1).await.unwrap();

    let found_ebook = found.get(0).unwrap()["doc"].as_object().unwrap()["Ebook"]
        .as_object()
        .unwrap();
    assert_eq!(found_ebook["title"].as_str().unwrap(), "Dune");
    assert_eq!(found_ebook["authors"].as_array().unwrap().len(), 2);

    // Get by Author, Series and Genres

    async fn get_ebooks(client: &reqwest::Client, url: Url, expected: usize) {
        let response = client.get(url.clone()).send().await.unwrap();
        info!("Response: {:#?}", response);
        assert!(response.status().is_success());
        assert!(response.status().as_u16() == 200);
        let body = response.text().await.unwrap();
        info!("Response body: {:#?}", body);
        let ebooks: Value = serde_json::from_str(&body).unwrap();
        let total = ebooks["total"].as_u64().unwrap();
        let rows = ebooks["rows"].as_array().unwrap();
        assert_eq!(rows.len(), expected, "Searching: {}", url);
        assert_eq!(total, expected as u64, "Searching: {}", url);
    }

    let url = base_url
        .join(&format!("api/author/{}/ebooks", author1.id))
        .unwrap();
    get_ebooks(&client, url, 1).await;

    let url = base_url
        .join(&format!("api/series/{}/ebooks", series.id))
        .unwrap();
    get_ebooks(&client, url, 1).await;

    let genres_filter = vec![genre1.id, genre2.id]
        .iter()
        .map(|g| g.to_string())
        .collect::<Vec<String>>()
        .join(",");
    let url = base_url
        .join(&format!(
            "api/ebook?filter=genres={genres_filter}&sort=e.title",
        ))
        .unwrap();
    get_ebooks(&client, url, 1).await;

    let url = base_url
        .join(&format!(
            "api/ebook?filter=genres=99999999,888888888&sort=e.title",
        ))
        .unwrap();
    get_ebooks(&client, url, 0).await;

    // Update

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

    let api_url = base_url.join("api/ebook").unwrap();
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

    search(&client, &base_url, "Dune", 0).await.unwrap();
    search(&client, &base_url, "Holmes", 1).await.unwrap();

    // Delete

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

#[tokio::test]
#[traced_test]
async fn test_ebook_rating() {
    let (args, mut guard) = prepare_env("test_ebook_rating").await.unwrap();
    let base_url = args.base_url.clone();
    // Admin acts as the first rating user (sub = "admin@localhost").
    let (admin_client, state) = launch_env(args, TestUser::Admin, &mut guard).await.unwrap();

    // A second authenticated user (User role, sub = "user@localhost") rates the same ebook.
    let user_headers = TestUser::User.auth_header(&state).unwrap();
    let user_client = reqwest::Client::builder()
        .default_headers(user_headers)
        .build()
        .unwrap();

    let lang = create_language(&admin_client, &base_url, "English", "en")
        .await
        .unwrap();
    let ebook = create_ebook(
        &admin_client,
        &base_url,
        &json!({"title": "Rated Book", "language_id": lang.id}),
    )
    .await
    .unwrap();
    assert_eq!(ebook.rating, None);
    assert_eq!(ebook.rating_count, None);

    let rate_url = base_url
        .join(&format!("api/ebook/{}/rate", ebook.id))
        .unwrap();
    let my_rating_url = base_url
        .join(&format!("api/ebook/{}/my-rating", ebook.id))
        .unwrap();

    async fn rate(client: &reqwest::Client, url: &Url, rating: f32) -> Ebook {
        let response = client
            .post(url.clone())
            .json(&json!({ "rating": rating }))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "rate failed");
        response.json().await.unwrap()
    }

    // First rating by admin: average == 80, count == 1.
    let updated = rate(&admin_client, &rate_url, 80.0).await;
    assert_eq!(updated.rating, Some(80.0));
    assert_eq!(updated.rating_count, Some(1));

    // Admin re-rates: value replaced, count stays 1.
    let updated = rate(&admin_client, &rate_url, 40.0).await;
    assert_eq!(updated.rating, Some(40.0));
    assert_eq!(updated.rating_count, Some(1));

    // Second user rates 100: average == (40 + 100) / 2 == 70, count == 2.
    let updated = rate(&user_client, &rate_url, 100.0).await;
    assert_eq!(updated.rating, Some(70.0));
    assert_eq!(updated.rating_count, Some(2));

    // Admin can fetch their own current rating (40).
    let response = admin_client
        .get(my_rating_url.clone())
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let my: Value = response.json().await.unwrap();
    assert_eq!(my["rating"].as_f64().unwrap(), 40.0);

    // Admin deletes their rating: only the user's 100 remains.
    let response = admin_client.delete(rate_url.clone()).send().await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let updated: Ebook = response.json().await.unwrap();
    assert_eq!(updated.rating, Some(100.0));
    assert_eq!(updated.rating_count, Some(1));

    // Admin no longer has a rating.
    let response = admin_client
        .get(my_rating_url.clone())
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    // Deleting a non-existent rating (admin already removed) is a 404.
    let response = admin_client.delete(rate_url.clone()).send().await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    // User deletes the last rating: rating and count reset to null/zero.
    let response = user_client.delete(rate_url.clone()).send().await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let updated: Ebook = response.json().await.unwrap();
    assert_eq!(updated.rating, None);
    assert_eq!(updated.rating_count, Some(0));

    // Anonymous users cannot rate.
    let anon = reqwest::Client::new();
    let response = anon
        .post(rate_url.clone())
        .json(&json!({ "rating": 50 }))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    // Sorting by rating is accepted; an invalid sort field is rejected.
    let response = admin_client
        .get(base_url.join("api/ebook?sort=-e.rating").unwrap())
        .send()
        .await
        .unwrap();
    assert!(response.status().is_success());
    let response = admin_client
        .get(base_url.join("api/ebook?sort=-e.rating_count").unwrap())
        .send()
        .await
        .unwrap();
    assert!(response.status().is_success());
    let response = admin_client
        .get(base_url.join("api/ebook?sort=e.bogus").unwrap())
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
#[traced_test]
async fn test_ebook_sort_language_and_author() {
    let (args, mut guard) = prepare_env("test_ebook_sort_lang_auth").await.unwrap();
    let base_url = args.base_url.clone();
    let (client, _state) = launch_env(args, TestUser::Admin, &mut guard).await.unwrap();

    let lang_cs = create_language(&client, &base_url, "Czech", "cs")
        .await
        .unwrap();
    let lang_en = create_language(&client, &base_url, "English", "en")
        .await
        .unwrap();

    let author_a = create_author(&client, &base_url, "Anderson", Some("Jane"))
        .await
        .unwrap();
    let author_z = create_author(&client, &base_url, "Zola", Some("Émile"))
        .await
        .unwrap();

    let e1 = create_ebook(
        &client,
        &base_url,
        &json!({"title": "Czech Book", "language_id": lang_cs.id, "authors": [author_z.id]}),
    )
    .await
    .unwrap();
    let e2 = create_ebook(
        &client,
        &base_url,
        &json!({"title": "English Book", "language_id": lang_en.id, "authors": [author_a.id]}),
    )
    .await
    .unwrap();

    async fn row_ids(client: &reqwest::Client, url: reqwest::Url) -> Vec<i64> {
        let body: Value = client.get(url).send().await.unwrap().json().await.unwrap();
        body["rows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["id"].as_i64().unwrap())
            .collect()
    }

    // Sort by language name ascending: "Czech" < "English"
    let ids = row_ids(&client, base_url.join("api/ebook?sort=l.name").unwrap()).await;
    assert_eq!(ids, vec![e1.id, e2.id], "language asc order wrong");

    // Sort by language name descending: "English" > "Czech"
    let ids = row_ids(&client, base_url.join("api/ebook?sort=-l.name").unwrap()).await;
    assert_eq!(ids, vec![e2.id, e1.id], "language desc order wrong");

    // Sort by author last name ascending: "Anderson" < "Zola"
    let ids = row_ids(
        &client,
        base_url.join("api/ebook?sort=pa.sort_author_last").unwrap(),
    )
    .await;
    assert_eq!(ids, vec![e2.id, e1.id], "author last asc order wrong");

    // Sort by author last name descending: "Zola" > "Anderson"
    let ids = row_ids(
        &client,
        base_url
            .join("api/ebook?sort=-pa.sort_author_last")
            .unwrap(),
    )
    .await;
    assert_eq!(ids, vec![e1.id, e2.id], "author last desc order wrong");

    // Invalid sort field is still rejected
    let response = client
        .get(base_url.join("api/ebook?sort=e.bogus").unwrap())
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}
