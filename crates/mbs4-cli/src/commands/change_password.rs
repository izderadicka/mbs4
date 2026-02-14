use clap::Args;
use mbs4_types::{config::BackendConfig, general::ValidEmail};

use crate::commands::{create_user_repository, Executor};

#[derive(Args, Debug)]
pub struct ChangePasswordCmd {
    #[command(flatten)]
    backend: BackendConfig,
    #[arg(short, long, help = "User email, used as username")]
    pub email: ValidEmail,
    #[arg(short, long, help = "New user password")]
    pub password: String,
}

impl Executor for ChangePasswordCmd {
    async fn run(self) -> anyhow::Result<()> {
        let repository = create_user_repository(&self.backend.database_url()).await?;
        repository
            .change_password(self.email.as_ref(), &self.password)
            .await?;
        Ok(())
    }
}
