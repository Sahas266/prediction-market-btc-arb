pub mod app;
pub mod cli;
pub mod io;
pub mod models;
pub mod sources;
pub mod strategy;

use anyhow::Result;

pub async fn run(cli: cli::Cli) -> Result<()> {
    app::run(cli).await
}
