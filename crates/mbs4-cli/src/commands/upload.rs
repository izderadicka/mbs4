use std::{collections::HashSet, path::PathBuf};

use anyhow::{anyhow, Context as _, Result};
use clap::{Args, Parser};
use futures::stream::StreamExt as _;
use mbs4_app::{
    rest_api::ebook::{EbookCoverInfo, EbookFileInfo},
    store::rest_api::UploadInfo,
};
use mbs4_calibre::EbookMetadata;
use mbs4_dal::{
    author::{AuthorShort, CreateAuthor},
    ebook::{CreateEbook, Ebook},
    genre::GenreShort,
    language::LanguageShort,
    series::{CreateSeries, SeriesShort},
    source::Source,
};
use mbs4_search::{EbookDoc, FoundDoc, SearchItem};
use reqwest::{multipart, Url};
use reqwest_eventsource::Event;
use serde_json::{Map, Value};
use tokio::fs;
use tracing::{debug, error, info, warn};

use crate::{commands::Executor, config::ServerConfig};

#[derive(Parser, Debug)]
pub struct UploadCmd {
    #[arg(
        short,
        long,
        help = "Path to ebook file, required. Must have known extension"
    )]
    file: PathBuf,
    #[command(flatten)]
    book: EbookInfo,
    #[command(flatten)]
    server: ServerConfig,
}

fn catch_event(
    client: reqwest::Client,
    sse_url: Url,
    operation_id: String,
) -> Result<tokio::sync::oneshot::Receiver<Value>, anyhow::Error> {
    let (sender, receiver) = tokio::sync::oneshot::channel();

    let mut sse = reqwest_eventsource::EventSource::new(client.get(sse_url))?;

    tokio::spawn(async move {
        while let Some(event) = sse.next().await {
            match event {
                Ok(Event::Open) => debug!("Event source opened"),
                Ok(Event::Message(msg)) => {
                    if msg.event == "message" {
                        match serde_json::from_str::<Map<String, Value>>(&msg.data) {
                            Ok(value) => {
                                let response_id = value
                                    .get("data")
                                    .and_then(|v| v.get("operation_id"))
                                    .and_then(|id| id.as_str());

                                if response_id == Some(&operation_id) {
                                    sender
                                        .send(value.get("data").unwrap().clone())
                                        .inspect_err(|e| error!("Failed to send event: {}", e))
                                        .ok();
                                    break;
                                }
                            }
                            Err(e) => {
                                error!("Failed to parse event: {}", e);
                                break;
                            }
                        }
                    }
                }

                Err(e) => {
                    error!("Event source error: {}", e);
                    sse.close();
                }
            }
        }
    });

    Ok(receiver)
}

macro_rules! check_response {
    ($res: ident, $what: expr) => {
        if !$res.status().is_success() {
            let error_message = format!("{} failed with status: {}", $what, $res.status());
            let text = $res.text().await.unwrap();

            error!("{}", error_message);
            debug!("Response text: {}", text);
            anyhow::bail!("{}", error_message);
        }
    };
}

impl Executor for UploadCmd {
    async fn run(self) -> Result<()> {
        let upload = self.upload().await?;
        let title = upload.title();

        if let Some(title) = title {
            let mut authors = upload.authors()?;
            const MAX_AUTHORS: usize = 20;
            if authors.len() > MAX_AUTHORS {
                warn!("Found {} authors, will only use first 20 ", authors.len());
                authors.truncate(MAX_AUTHORS);
            }

            let series = upload.series()?;

            let existing_ebook = upload
                .search_ebook(title, &authors, series.as_ref())
                .await?;

            let ebook: Ebook = if let Some(ebook) = existing_ebook {
                debug!("Found existing ebook: {}:{}", ebook.id, ebook.title);
                upload.get_ebook(ebook.id).await?
            } else {
                let lang_code = upload
                    .book
                    .language
                    .as_ref()
                    .or_else(|| upload.meta.language.as_ref());

                let language = if let Some(ref lang) = lang_code {
                    upload.prepare_language(lang).await?
                } else {
                    anyhow::bail!("Missing language input");
                };
                let mut genres: Vec<_> = upload.book.genre.iter().map(|s| s.as_str()).collect();
                if genres.is_empty() {
                    genres = upload.meta.genres.iter().map(|s| s.as_str()).collect();
                }
                let genres = upload.prepare_genres(&genres).await?;
                let genres_ids = if genres.is_empty() {
                    None
                } else {
                    Some(genres.iter().map(|g| g.id).collect::<Vec<_>>())
                };
                let authors = upload.prepare_authors(authors).await?;
                let authors_ids = if authors.is_empty() {
                    None
                } else {
                    Some(authors.iter().map(|a| a.id).collect::<Vec<_>>())
                };

                let (series_id, series_index) = if let Some(series) = series {
                    match upload.prepare_series(series).await? {
                        Some((s, index)) => (Some(s.id), Some(index)),
                        None => (None, None),
                    }
                } else {
                    (None, None)
                };

                let new_ebook = CreateEbook {
                    title: title.to_string(),
                    description: upload.description(),
                    series_id,
                    series_index,
                    language_id: language.id,
                    authors: authors_ids,
                    genres: genres_ids,
                    created_by: Some(upload.server.email.clone()),
                };
                let ebook = upload.create_ebook(&new_ebook).await?;
                info!("Created new ebook: {}:{}", ebook.id, ebook.title);
                ebook
            };

            let source = upload.add_source_to_ebook(ebook.id).await?;
            if let Some(ref cover_file) = upload.meta.cover_file {
                if ebook.cover.is_none() {
                    upload.add_cover_to_ebook(&ebook, cover_file).await?;
                } else {
                    upload.delete_cover(cover_file).await?;
                }
            }
            debug!("Source {} added", source.id);
        } else {
            anyhow::bail!("Missing title");
        }

        Ok(())
    }
}

struct UploadHelper {
    server: ServerConfig,
    book: EbookInfo,
    meta: EbookMetadata,
    upload_info: UploadInfo,
    client: reqwest::Client,
}

impl UploadHelper {
    async fn add_cover_to_ebook(&self, ebook: &Ebook, cover_file: &str) -> Result<()> {
        let cover_url = self
            .server
            .url
            .join(&format!("api/ebook/{}/cover", ebook.id))?;
        let cover_info = EbookCoverInfo {
            cover_file: Some(cover_file.to_string()),
            ebook_id: ebook.id,
            ebook_version: ebook.version,
        };

        let res = self
            .client
            .put(cover_url.clone())
            .json(&cover_info)
            .send()
            .await?;
        check_response!(res, "Add Cover");
        let _updated_ebook: Ebook = res.json().await.unwrap();
        Ok(())
    }

    async fn delete_cover(&self, cover_file: &str) -> Result<()> {
        let cover_url = self
            .server
            .url
            .join(&format!("/files/uploaded/{}", cover_file))?;
        let res = self.client.delete(cover_url).send().await?;
        check_response!(res, "Delete Cover");
        Ok(())
    }
    async fn create_ebook(&self, new_ebook: &CreateEbook) -> Result<Ebook> {
        let ebook_url = self.server.url.join("api/ebook")?;
        let res = self.client.post(ebook_url).json(new_ebook).send().await?;
        check_response!(res, "Create ebook");
        let ebook = res.json().await?;
        Ok(ebook)
    }

    async fn get_ebook(&self, ebook_id: i64) -> Result<Ebook> {
        let ebook_url = self.server.url.join(&format!("api/ebook/{}", ebook_id))?;
        let res = self.client.get(ebook_url).send().await?;
        check_response!(res, "Get ebook");
        let ebook = res.json().await?;
        Ok(ebook)
    }

    async fn prepare_language(&self, lang_code: &str) -> Result<LanguageShort> {
        let lang_url = self.server.url.join("api/language/all")?;
        let res = self.client.get(lang_url).send().await?;
        check_response!(res, "Get languages");
        let languages: Vec<LanguageShort> = res.json().await?;

        languages
            .into_iter()
            .find(|l| l.code == lang_code)
            .ok_or_else(|| anyhow!("Language {} not found", lang_code))
    }

    async fn prepare_genres(&self, genres: &[&str]) -> Result<Vec<GenreShort>> {
        let genres: HashSet<String> = genres
            .into_iter()
            .map(|s| s.trim().to_lowercase())
            .collect();
        let genre_url = self.server.url.join("api/genre/all")?;
        let res = self.client.get(genre_url).send().await?;
        check_response!(res, "Get genres");
        let all_genres: Vec<GenreShort> = res.json().await?;

        Ok(all_genres
            .into_iter()
            .filter(|g| genres.contains(&g.name.to_lowercase()))
            .collect())
    }

    async fn add_source_to_ebook(&self, ebook_id: i64) -> Result<Source> {
        let source_url = self
            .server
            .url
            .join(&format!("api/ebook/{}/source", ebook_id))
            .unwrap();
        let ebook_file_info = EbookFileInfo {
            uploaded_file: self.upload_info.final_path.clone(),
            size: self.upload_info.size,
            hash: self.upload_info.hash.clone(),
            quality: None,
        };

        let res = self
            .client
            .post(source_url.clone())
            .json(&ebook_file_info)
            .send()
            .await
            .unwrap();
        check_response!(res, "Add Source");

        let source: Source = res.json().await.unwrap();

        Ok(source)
    }
    fn title(&self) -> Option<&str> {
        self.book
            .title
            .as_ref()
            .map(|t| t.as_str())
            .or_else(|| self.meta.title.as_ref().map(|t| t.as_str()))
    }

    fn description(&self) -> Option<String> {
        self.book
            .description
            .as_ref()
            .or_else(|| self.meta.comments.as_ref())
            .map(|s| s.to_string())
    }

    fn authors(&self) -> Result<Vec<mbs4_calibre::meta::Author>> {
        let mut authors = Vec::new();
        if self.book.author.is_empty() {
            authors.extend_from_slice(&self.meta.authors);
        } else {
            for author in self.book.author.iter() {
                authors.push(author.parse()?)
            }
        }
        Ok(authors)
    }

    async fn search_ebook(
        &self,
        title: &str,
        authors: &[mbs4_calibre::meta::Author],
        series: Option<&mbs4_calibre::meta::Series>,
    ) -> Result<Option<EbookDoc>> {
        let mut search_url = self.server.url.join("search")?;
        let mut query = title.to_string()
            + " "
            + authors
                .into_iter()
                .map(|a| a.to_string())
                .collect::<Vec<_>>()
                .join(" ")
                .as_str();
        if let Some(series) = series {
            query += " ";
            query += series.title.as_str();
        }
        search_url
            .query_pairs_mut()
            .append_pair("what", "ebook")
            .append_pair("num_results", "10")
            .append_pair("query", title);
        let res = self.client.get(search_url).send().await?;
        check_response!(res, "Search Ebook");
        let found_ebooks: Vec<SearchItem> = res.json().await?;
        let first_item = found_ebooks.into_iter().next();

        if let Some(first_item) = first_item {
            if let FoundDoc::Ebook(ebook) = first_item.doc {
                Ok(Some(ebook))
            } else {
                anyhow::bail!("Found item is not an ebook");
            }
        } else {
            Ok(None)
        }
    }

    async fn prepare_authors(
        &self,
        authors: Vec<mbs4_calibre::meta::Author>,
    ) -> Result<Vec<AuthorShort>> {
        let mut verified_authors = Vec::with_capacity(authors.len());
        for author in authors {
            let mut matching_authors = self.search_author(&author).await?;

            match matching_authors.len() {
                0 => {
                    let created_author = self.create_author(author).await?;
                    info!("Created author: {created_author:?}");
                    verified_authors.push(created_author);
                }
                1 => verified_authors.push(matching_authors.pop().unwrap()),
                n => {
                    warn!("Found {n} matching authors");
                    verified_authors.push(matching_authors.into_iter().next().unwrap())
                }
            }
        }
        debug!("Found authors {verified_authors:?}");

        Ok(verified_authors)
    }

    fn series(&self) -> Result<Option<mbs4_calibre::meta::Series>> {
        use mbs4_calibre::meta::Series;
        let provided_series: Series;

        if let Some(series_str) = self.book.series.as_ref() {
            provided_series = series_str.parse()?;
        } else if let Some(ref series) = self.meta.series {
            provided_series = series.clone();
        } else {
            return Ok(None);
        }

        Ok(Some(provided_series))
    }

    async fn prepare_series(
        &self,
        provided_series: mbs4_calibre::meta::Series,
    ) -> Result<Option<(SeriesShort, u32)>> {
        let existing_series = self.search_series(&provided_series.title).await?;

        match existing_series {
            Some(series) => Ok(Some((series, provided_series.index))),
            None => {
                let created_series = self.create_series(provided_series.title).await?;
                Ok(Some((created_series, provided_series.index)))
            }
        }
    }

    async fn create_series(&self, series: String) -> Result<SeriesShort> {
        let create_url = self.server.url.join("api/series")?;
        let series_request = CreateSeries {
            title: series,
            description: None,
            created_by: Some(self.server.email.clone()),
        };
        let res = self
            .client
            .post(create_url)
            .json(&series_request)
            .send()
            .await?;
        check_response!(res, "Create Series");
        let created_series: mbs4_dal::series::SeriesShort = res.json().await?;
        Ok(created_series)
    }

    async fn create_author(&self, author: mbs4_calibre::meta::Author) -> Result<AuthorShort> {
        let create_url = self.server.url.join("api/author")?;
        let author_request = CreateAuthor {
            first_name: author.first_name,
            last_name: author.last_name,
            description: None,
            created_by: Some(self.server.email.clone()),
        };
        let res = self
            .client
            .post(create_url)
            .json(&author_request)
            .send()
            .await?;
        check_response!(res, "Create Author");
        let created_author: mbs4_dal::author::AuthorShort = res.json().await?;
        Ok(created_author)
    }

    async fn search_author(&self, author: &mbs4_calibre::meta::Author) -> Result<Vec<AuthorShort>> {
        let mut search_url = self.server.url.join("search")?;
        let query = if let Some(ref first_name) = author.first_name {
            format!("{} {}", first_name, author.last_name)
        } else {
            author.last_name.clone()
        };
        search_url
            .query_pairs_mut()
            .append_pair("what", "author")
            .append_pair("num_results", "10")
            .append_pair("query", &query);
        let res = self.client.get(search_url).send().await?;
        check_response!(res, "Search Author");
        let found_authors: Vec<SearchItem> = res.json().await?;
        let matching_authors = filter_found_authors(found_authors, &author);
        Ok(matching_authors)
    }

    async fn search_series(&self, series: &str) -> Result<Option<SeriesShort>> {
        let mut search_url = self.server.url.join("search")?;
        search_url
            .query_pairs_mut()
            .append_pair("what", "series")
            .append_pair("num_results", "10")
            .append_pair("query", series);
        let res = self.client.get(search_url).send().await?;
        let json = res.json().await?;
        Ok(filter_found_series(json, series))
    }
}

fn filter_found_series(found: Vec<SearchItem>, series: &str) -> Option<SeriesShort> {
    fn extract_series(i: SearchItem) -> Option<SeriesShort> {
        match i.doc {
            FoundDoc::Series(s) => Some(s),
            _ => None,
        }
    }

    found.into_iter().find_map(|item| {
        extract_series(item).and_then(|s| {
            if s.title.trim().to_lowercase() == series.trim().to_lowercase() {
                Some(s)
            } else {
                None
            }
        })
    })
}
fn filter_found_authors(
    found: Vec<SearchItem>,
    author: &mbs4_calibre::meta::Author,
) -> Vec<AuthorShort> {
    fn extract_author(i: SearchItem) -> Option<AuthorShort> {
        match i.doc {
            FoundDoc::Author(a) => Some(a),
            _ => None,
        }
    }

    fn author_matches(a: &AuthorShort, to: &mbs4_calibre::meta::Author) -> bool {
        if a.last_name != to.last_name {
            return false;
        }
        if a.first_name == to.first_name {
            return true;
        } else if let (Some(n1), Some(n2)) = (&a.first_name, &to.first_name) {
            let mut names1 = n1.split_whitespace();
            let mut names2 = n2.split_whitespace();

            return names1
                .by_ref()
                .zip(&mut names2)
                .enumerate()
                .all(|(i, (n1, n2))| {
                    if i == 0 {
                        n1 == n2
                    } else {
                        n1.chars().next() == n2.chars().next()
                    }
                })
                && names2.next().is_none()
                && names1.next().is_none();
        }
        false
    }

    found
        .into_iter()
        .filter_map(|item| {
            extract_author(item).filter(|found_author| author_matches(found_author, &author))
        })
        .collect()
}

impl UploadCmd {
    fn to_executor(
        self,
        meta: EbookMetadata,
        upload_info: UploadInfo,
        client: reqwest::Client,
    ) -> UploadHelper {
        UploadHelper {
            server: self.server,
            book: self.book,
            meta,
            upload_info,
            client,
        }
    }

    async fn upload(self) -> Result<UploadHelper, anyhow::Error> {
        let file_name = self
            .file
            .file_name()
            .ok_or_else(|| anyhow!("Missing file name"))?
            .to_string_lossy()
            .to_string();
        let file = fs::File::open(&self.file).await?;
        let form =
            multipart::Form::new().part("file", multipart::Part::stream(file).file_name(file_name));
        let client = self.server.authenticated_client().await?;
        debug!("Client created");
        let upload_url = self.server.url.join("files/upload/form").unwrap();
        let res = client
            .post(upload_url)
            .multipart(form)
            .send()
            .await
            .unwrap();
        debug!("Upload Response: {:?}", res);
        check_response!(res, "Upload");
        let upload_info: UploadInfo = res.json().await?;
        let meta_url = self.server.url.join("api/convert/extract_meta").unwrap();
        let res = client
            .post(meta_url)
            .json(&upload_info)
            .send()
            .await
            .unwrap();
        debug!("Extract meta Response: {:?}", res);
        check_response!(res, "Extract meta");
        let meta_ticket: Map<String, Value> = res.json().await.unwrap();
        debug!("Meta ticket: {:#?}", meta_ticket);
        let ticket_id = meta_ticket.get("id").unwrap().as_str().unwrap();
        let sse_url = self.server.url.join("events").unwrap();
        let receiver = catch_event(client.clone(), sse_url, ticket_id.to_string())?;
        let meta;
        match tokio::time::timeout(std::time::Duration::from_secs(10), receiver).await {
            Ok(res) => {
                let res = res.unwrap();
                meta = res["metadata"].clone();
                debug!("Meta: {:#?}", meta);
            }
            Err(_) => {
                anyhow::bail!("Meta event timeout");
            }
        }
        let meta: EbookMetadata = serde_json::from_value(meta).context("Failed to parse meta")?;
        let upload = self.to_executor(meta, upload_info, client);
        Ok(upload)
    }
}

#[derive(Args, Debug)]
pub struct EbookInfo {
    #[arg(
        long,
        help = "Title of the ebook, if not provided will be taken from metadata"
    )]
    title: Option<String>,

    #[arg(
        long,
        help = "Authors of the ebook, if not provided will be taken from metadata. MUST have form last_name, first_name, can be used multiple times or values separated by semicolon - ;",
        num_args=0..,
        value_delimiter = ';'
    )]
    author: Vec<String>,

    #[arg(
        long,
        help = "Language of the ebook, if not provided will be taken from metadata, MUST be 2 letter ISO code"
    )]
    language: Option<String>,

    #[arg(
        long,
        help = "Description of the ebook, if not provided will be taken from metadata"
    )]
    description: Option<String>,

    #[arg(
        long,
        help = "Cover image of the ebook, if not provided will be taken from metadata"
    )]
    cover: Option<PathBuf>,

    #[arg(
        long,
        help = "Series of the ebook, if not provided will be taken from metadata, should be in form series title #index"
    )]
    series: Option<String>,

    #[arg(
        long,
        help = "Quality of the ebook, meaning technical quality of the file. Should be in range 0-100, 0 being the worst quality possible"
    )]
    quality: Option<f32>,

    #[arg(
        long,
        help = "Genres of the ebook, if not provided will be taken from metadata. Only know genres will be used. Can be used multiple times or values separated by semicolon - ;",
        num_args=0..,
        value_delimiter = ';'
    )]
    genre: Vec<String>,
}
