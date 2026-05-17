pub mod change_password;
pub mod cleanup;
pub mod create_user;
pub mod upload;

#[allow(async_fn_in_trait)]
pub trait Executor {
    async fn run(self) -> anyhow::Result<()>;
}

pub(crate) async fn create_pool(database_url: &str) -> anyhow::Result<mbs4_dal::Pool> {
    Ok(sqlx::sqlite::SqlitePool::connect(database_url).await?)
}

pub(crate) async fn create_user_repository(
    database_url: &str,
) -> anyhow::Result<mbs4_dal::user::UserRepository> {
    Ok(mbs4_dal::user::UserRepository::new(
        create_pool(database_url).await?,
    ))
}

pub(crate) async fn create_source_repository(
    database_url: &str,
) -> anyhow::Result<mbs4_dal::source::SourceRepository> {
    Ok(mbs4_dal::source::SourceRepository::new(
        create_pool(database_url).await?,
    ))
}
