pub mod cleanup;
pub mod create_user;
pub mod upload;

#[allow(async_fn_in_trait)]
pub trait Executor {
    async fn run(self) -> anyhow::Result<()>;
}
