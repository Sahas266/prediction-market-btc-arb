use clap::{Parser, Subcommand};

#[derive(Parser, Debug, Clone)]
#[command(
    author,
    version,
    about = "Polymarket BTC pair scanner, replay, and paper-trading toolkit"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug, Clone)]
pub enum Command {
    RunAll {
        #[arg(long, default_value_t = 24)]
        backfill_hours: u64,
        #[arg(long, default_value_t = 90)]
        live_duration_seconds: u64,
        #[arg(long, default_value_t = 1_000)]
        poll_interval_ms: u64,
    },
    Backfill {
        #[arg(long, default_value_t = 24)]
        hours: u64,
    },
    Live {
        #[arg(long, default_value_t = 300)]
        duration_seconds: u64,
        #[arg(long, default_value_t = 1_000)]
        poll_interval_ms: u64,
    },
    PaperTrade {
        #[arg(long, default_value = "data/live/pair_scans.ndjson")]
        input: String,
        #[arg(long, env = "PAPER_MIN_EDGE_CENTS", default_value_t = 2.0)]
        min_edge_cents: f64,
        #[arg(long, env = "PAPER_TARGET_SHARES", default_value_t = 5.0)]
        target_shares: f64,
    },
    Replay {
        #[arg(long, default_value = "data/live/pair_scans.ndjson")]
        input: String,
    },
    Summarize {
        #[arg(long, default_value = "data/live/pair_scans.ndjson")]
        input: String,
        #[arg(long)]
        from: Option<String>,
        #[arg(long)]
        to: Option<String>,
    },
}
