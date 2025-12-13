use std::path::PathBuf;

use clap::{Args, Parser};

use crate::commands::Executor;

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
}

impl Executor for UploadCmd {
    async fn run(self) -> anyhow::Result<()> {
        todo!()
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
