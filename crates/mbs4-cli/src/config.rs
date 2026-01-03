use clap::{Args, Parser, Subcommand};
use reqwest::Url;
use serde_json::json;

use crate::commands::{cleanup::CleanupCmd, create_user::CreateUserCmd, upload::UploadCmd};

#[derive(Parser)]
#[command(
    version,
    about,
    long_about = "CLI for mbs4 - provides various commands to manage and interact with mbs4 server."
)]
pub struct CliConfig {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Args, Debug)]
pub struct ServerConfig {
    #[arg(short, long, env = "MBS4_URL")]
    pub url: Url,

    #[arg(long, alias = "user", env = "MBS4_USER")]
    pub email: String,

    #[arg(long, env = "MBS4_PASSWORD")]
    pub password: String,
}

impl ServerConfig {
    pub async fn authenticated_client(&self) -> anyhow::Result<reqwest::Client> {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        let response = client
            .post(self.url.join("auth/login?token=true")?)
            .json(&json!({"email": &self.email, "password": &self.password}))
            .send()
            .await?;
        if !response.status().is_success() {
            anyhow::bail!("Failed to login with status: {}", response.status());
        }
        let token = response.text().await?;
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("Authorization", format!("Bearer {token}").parse()?);
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .default_headers(headers)
            .build()?;
        Ok(client)
    }
}

#[derive(Subcommand)]
pub enum Command {
    Cleanup(CleanupCmd),
    Upload(UploadCmd),
    CreateUser(CreateUserCmd),
}

impl crate::commands::Executor for Command {
    async fn run(self) -> anyhow::Result<()> {
        match self {
            Command::Cleanup(cmd) => cmd.run().await,
            Command::Upload(cmd) => cmd.run().await,
            Command::CreateUser(cmd) => cmd.run().await,
        }
    }
}
