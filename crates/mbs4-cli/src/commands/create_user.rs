use clap::Parser;
use mbs4_types::{claim::Role, config::BackendConfig, general::ValidEmail};

use crate::commands::Executor;

#[derive(Parser, Debug)]
pub struct CreateUserCmd {
    #[command(flatten)]
    backend: BackendConfig,
    #[arg(short, long, help = "User name")]
    name: String,
    #[arg(short, long, help = "User email, used as username")]
    pub email: ValidEmail,
    #[arg(
        short,
        long,
        help = "User password, optional if delegated authetication (OIDC) is enabled"
    )]
    pub password: Option<String>,
    #[arg(short, long, num_args=0..,
        value_delimiter = ';',help = "Roles of the user, comma separated or used multiple times, currently admin,trusted roles are supported, not hiearchical - add all roles to the user")]
    pub roles: Vec<Role>,
}

impl Executor for CreateUserCmd {
    async fn run(self) -> anyhow::Result<()> {
        let db_url = self.backend.database_url();
        let pool = sqlx::sqlite::SqlitePool::connect(&db_url).await?;
        let repository = mbs4_dal::user::UserRepository::new(pool);
        let roles: Vec<String> = self.roles.iter().map(|r| r.to_string()).collect();
        let new_user = mbs4_dal::user::CreateUser {
            name: self.name,
            email: self.email,
            password: self.password,
            roles: if roles.is_empty() { None } else { Some(roles) },
        };
        repository.create(new_user).await?;

        Ok(())
    }
}
