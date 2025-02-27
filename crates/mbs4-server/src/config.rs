use crate::error::Result;
use clap::Parser as _;
use url::Url;

#[derive(Debug, clap::Parser)]
pub struct ServerConfig {
    #[arg(
        short,
        long,
        default_value_t = 3000,
        env = "MBS4_LISTEN_PORT",
        help = "Port to listen on"
    )]
    pub port: u16,
    #[arg(
        short,
        long,
        default_value = "127.0.0.1",
        env = "MBS4_LISTEN_ADDRESS",
        help = "Address to listen on"
    )]
    pub listen_address: String,

    #[arg(
        long,
        env = "MBS4_OIDC_CONFIG",
        help = "Path to OIDC configuration file"
    )]
    pub oidc_config: String,

    #[arg(
        long,
        env = "MBS4_BASE_URL",
        default_value = "http://localhost:3000",
        help = "Base URL of server, as visible to users"
    )]
    pub base_url: Url,
}

impl ServerConfig {
    pub fn load() -> Result<Self> {
        ServerConfig::try_parse().map_err(|e| e.into())
    }
}
