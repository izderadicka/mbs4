use std::path::PathBuf;

use anyhow::{anyhow, bail, Context as _, Result};
use clap::{Args, Parser};
use futures::stream::StreamExt as _;
use mbs4_calibre::EbookMetadata;
use mbs4_dal::author::{self, AuthorShort};
use reqwest::{multipart, Url};
use reqwest_eventsource::Event;
use serde_json::{Map, Value};
use tokio::fs;
use tracing::{debug, error, warn};

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

        let upload_info: Map<String, Value> = res.json().await?;

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
        let authors = upload.prepare_authors().await?;

        Ok(())
    }
}

struct UploadHelper {
    server: ServerConfig,
    file: PathBuf,
    book: EbookInfo,
    meta: EbookMetadata,
    upload_info: Map<String, Value>,
    client: reqwest::Client,
}

impl UploadHelper {
    async fn prepare_authors(&self) -> Result<Vec<AuthorShort>> {
        use mbs4_calibre::meta::Author;
        let mut authors = Vec::new();
        if self.book.author.is_empty() {
            authors.extend_from_slice(&self.meta.authors);
        } else {
            for author in self.book.author.iter() {
                authors.push(Author::from_comma_form(&author)?)
            }
        }
        let mut verified_authors = Vec::with_capacity(authors.len());
        for author in authors {
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

            let found_authors: Vec<Map<String, Value>> = res.json().await?;
            let mut matching_authors = filter_found_authors(found_authors, &author);

            match matching_authors.len() {
                0 => {}
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
}

fn filter_found_authors(
    found: Vec<Map<String, Value>>,
    author: &mbs4_calibre::meta::Author,
) -> Vec<AuthorShort> {
    fn extract_author(json: &Map<String, Value>) -> Result<AuthorShort> {
        let doc = json
            .get("doc")
            .and_then(|d| d.get("Author"))
            .and_then(Value::as_object)
            .context("Missing Author object in json")?;
        let id = doc
            .get("id")
            .and_then(Value::as_i64)
            .context("Missing id field")?;
        let last_name = doc
            .get("last_name")
            .and_then(Value::as_str)
            .context("Missing last_name field")?
            .to_string();
        let first_name = doc
            .get("first_name")
            .and_then(Value::as_str)
            .map(|s| s.to_string());
        Ok(AuthorShort {
            id,
            first_name,
            last_name,
        })
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
        .filter_map(|o| match extract_author(&o) {
            Ok(found_author) => {
                if author_matches(&found_author, &author) {
                    Some(found_author)
                } else {
                    None
                }
            }
            Err(e) => {
                error!("Invalid response from search");
                None
            }
        })
        .collect()
}

impl UploadCmd {
    fn to_executor(
        self,
        meta: EbookMetadata,
        upload_info: Map<String, Value>,
        client: reqwest::Client,
    ) -> UploadHelper {
        UploadHelper {
            server: self.server,
            file: self.file,
            book: self.book,
            meta,
            upload_info,
            client,
        }
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
