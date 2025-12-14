use std::path::PathBuf;

use anyhow::anyhow;
use clap::{Args, Parser};
use futures::stream::StreamExt as _;
use reqwest::{multipart, Url};
use reqwest_eventsource::Event;
use serde_json::{Map, Value};
use tokio::fs;
use tracing::{debug, error};

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
    async fn run(self) -> anyhow::Result<()> {
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

        Ok(())
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
