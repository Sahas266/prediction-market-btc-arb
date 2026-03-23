use anyhow::Result;
use clap::Parser;
use prediction_market_btc_arb::cli::Cli;

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    prediction_market_btc_arb::run(Cli::parse()).await
}
