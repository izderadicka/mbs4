use anyhow::Result;
use mbs4_dal::{author::Author, genre::Genre, language::Language, series::Series};
use reqwest::Url;
use serde_json::json;
use tracing::info;

pub async fn create_author(
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

pub async fn create_genre(client: &reqwest::Client, base_url: &Url, name: &str) -> Result<Genre> {
    let payload = json!({"name": name});
    let api_url = base_url.join("api/genre").unwrap();

    let response = client.post(api_url.clone()).json(&payload).send().await?;
    assert!(response.status().is_success());

    let new_genre: Genre = response.json().await?;
    Ok(new_genre)
}

pub async fn create_series(
    client: &reqwest::Client,
    base_url: &Url,
    title: &str,
) -> Result<Series> {
    let payload = json!({"title": title});
    let api_url = base_url.join("api/series").unwrap();

    let response = client.post(api_url.clone()).json(&payload).send().await?;
    info!("Series Response: {:#?}", response);
    assert!(response.status().is_success());

    let new_series: Series = response.json().await?;
    Ok(new_series)
}

pub async fn create_language(
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
