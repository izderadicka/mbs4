use anyhow::Result;
use mbs4_dal::{
    author::Author, ebook::Ebook, format::Format, genre::Genre, language::Language, series::Series,
};
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
    assert!(response.status().as_u16() == 201);

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
    assert!(response.status().as_u16() == 201);

    let new_language: Language = response.json().await?;
    Ok(new_language)
}

pub async fn create_format(
    client: &reqwest::Client,
    base_url: &Url,
    name: &str,
    mime_type: &str,
    extension: &str,
) -> Result<Format> {
    let payload = json!({"name": name, "mime_type": mime_type, "extension": extension});
    let api_url = base_url.join("api/format").unwrap();

    let response = client.post(api_url.clone()).json(&payload).send().await?;
    assert!(response.status().is_success());
    assert!(response.status().as_u16() == 201);

    let new_format: Format = response.json().await?;
    Ok(new_format)
}

pub async fn create_ebook<T>(client: &reqwest::Client, base_url: &Url, payload: &T) -> Result<Ebook>
where
    T: serde::Serialize,
{
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

    Ok(new_ebook)
}
