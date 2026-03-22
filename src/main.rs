use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Duration as ChronoDuration, Timelike, Utc};
use chrono_tz::America::New_York;
use clap::{Parser, Subcommand};
use futures::{stream, StreamExt};
use regex::Regex;
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, USER_AGENT};
use serde::{de::Error as DeError, Deserialize, Deserializer, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::time::{Duration, Instant};

const CHAINLINK_STREAM_PAGE: &str = "https://data.chain.link/streams/btc-usd";
const CHAINLINK_1D_BARS_URL: &str =
    "https://data.chain.link/api/historical-data-engine-stream-data";
const CHAINLINK_LIVE_STREAM_URL: &str = "https://data.chain.link/api/live-data-engine-stream-data";
const POLYMARKET_GAMMA_BY_SLUG: &str = "https://gamma-api.polymarket.com/markets/slug";
const POLYMARKET_EVENT_PAGE: &str = "https://polymarket.com/event";
const POLYMARKET_ORDERBOOK: &str = "https://clob.polymarket.com/book";
const ANCHOR_MATCH_TOLERANCE: f64 = 0.000_001;

#[derive(Parser, Debug)]
#[command(
    author,
    version,
    about = "Log-only Polymarket BTC pair scanner and Chainlink logger"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ChainlinkContext {
    stream_page_url: String,
    resolved_page_slug: String,
    feed_id: String,
    multiply: f64,
    stream_name: String,
    source_chain: Option<u64>,
    fetched_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
struct RunManifest {
    started_at: DateTime<Utc>,
    command: String,
    chainlink_feed_id: String,
    notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ChainlinkLiveReport {
    valid_from: DateTime<Utc>,
    price: f64,
    bid: Option<f64>,
    ask: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ChainlinkMinuteBar {
    minute_start: DateTime<Utc>,
    open: f64,
    high: f64,
    low: f64,
    close: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PriceBeat {
    slug: String,
    price_to_beat: f64,
    source: String,
    extracted_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FeeSchedule {
    exponent: u32,
    rate: f64,
    #[serde(rename = "takerOnly")]
    taker_only: Option<bool>,
    #[serde(rename = "rebateRate")]
    rebate_rate: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GammaMarket {
    id: String,
    question: String,
    slug: String,
    #[serde(rename = "conditionId")]
    condition_id: String,
    #[serde(rename = "resolutionSource")]
    resolution_source: Option<String>,
    #[serde(rename = "endDate")]
    end_date: String,
    description: Option<String>,
    #[serde(deserialize_with = "de_json_string_vec")]
    outcomes: Vec<String>,
    #[serde(rename = "outcomePrices", deserialize_with = "de_json_string_vec_f64")]
    outcome_prices: Vec<f64>,
    active: bool,
    closed: bool,
    #[serde(rename = "acceptingOrders")]
    accepting_orders: Option<bool>,
    #[serde(rename = "clobTokenIds", deserialize_with = "de_json_string_vec")]
    clob_token_ids: Vec<String>,
    #[serde(rename = "bestBid", default, deserialize_with = "de_opt_f64_from_any")]
    best_bid: Option<f64>,
    #[serde(rename = "bestAsk", default, deserialize_with = "de_opt_f64_from_any")]
    best_ask: Option<f64>,
    #[serde(rename = "feesEnabled")]
    fees_enabled: Option<bool>,
    #[serde(rename = "feeType")]
    fee_type: Option<String>,
    #[serde(rename = "feeSchedule")]
    fee_schedule: Option<FeeSchedule>,
    #[serde(
        rename = "lastTradePrice",
        default,
        deserialize_with = "de_opt_f64_from_any"
    )]
    last_trade_price: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OrderLevel {
    #[serde(deserialize_with = "de_f64_from_any")]
    price: f64,
    #[serde(deserialize_with = "de_f64_from_any")]
    size: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OrderBook {
    market: String,
    #[serde(rename = "asset_id")]
    asset_id: String,
    timestamp: String,
    bids: Vec<OrderLevel>,
    asks: Vec<OrderLevel>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LegOrderbookSnapshot {
    outcome: String,
    token_id: String,
    best_bid: Option<f64>,
    best_ask: Option<f64>,
    top_bids: Vec<OrderLevel>,
    top_asks: Vec<OrderLevel>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MarketSnapshot {
    slug: String,
    question: String,
    condition_id: String,
    start_anchor: f64,
    start_anchor_source: String,
    start_anchor_captured_at: DateTime<Utc>,
    outcomes: Vec<String>,
    outcome_prices: Vec<f64>,
    fees_enabled: bool,
    fee_schedule: Option<FeeSchedule>,
    best_bid: Option<f64>,
    best_ask: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LivePairScan {
    scanned_at: DateTime<Utc>,
    status: String,
    current_et: String,
    current_15m_start_et: String,
    current_final_5m_start_et: String,
    next_eligible_et: String,
    source_latest: Option<ChainlinkLiveReport>,
    pair: Option<LivePairDetail>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LivePairDetail {
    fifteen_minute: MarketSnapshot,
    five_minute: MarketSnapshot,
    selected_pair: String,
    floor_payout: f64,
    estimated_all_in_best_ask_cost: Option<f64>,
    estimated_edge_best_ask: Option<f64>,
    executable_costs: BTreeMap<String, f64>,
    leg_books: Vec<LegOrderbookSnapshot>,
    source_comparison: SourceComparison,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct HistoricalPairRecord {
    scanned_at: DateTime<Utc>,
    fifteen_minute_slug: String,
    five_minute_slug: String,
    window_start_et: String,
    window_end_et: String,
    fifteen_anchor: f64,
    five_anchor: f64,
    selected_pair: String,
    fifteen_market: MarketSnapshot,
    five_market: MarketSnapshot,
    source_comparison: SourceComparison,
    resolved_outcome_fifteen: Option<String>,
    resolved_outcome_five: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SourceComparison {
    chainlink_open_15m_minute: Option<f64>,
    chainlink_open_5m_minute: Option<f64>,
    chainlink_end_minute: Option<f64>,
    delta_15m_anchor: Option<f64>,
    delta_5m_anchor: Option<f64>,
    polymarket_five_anchor: Option<f64>,
    chainlink_live_five_anchor: Option<f64>,
    delta_live_b_vs_polymarket_b: Option<f64>,
    live_b_matches_polymarket_b: Option<bool>,
}

#[derive(Clone)]
struct SelectedLeg<'a> {
    outcome: String,
    token_id: String,
    fee_schedule: Option<&'a FeeSchedule>,
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    fs::create_dir_all("data/chainlink")?;
    fs::create_dir_all("data/live")?;
    fs::create_dir_all("data/historical")?;
    fs::create_dir_all("data/runs")?;

    let cli = Cli::parse();
    let client = build_client()?;
    let chainlink = fetch_chainlink_context(&client).await?;
    write_json("data/chainlink/context.json", &chainlink)?;

    let command_name = match &cli.command {
        Command::RunAll { .. } => "run-all",
        Command::Backfill { .. } => "backfill",
        Command::Live { .. } => "live",
    };
    write_json(
        "data/runs/latest_run.json",
        &RunManifest {
            started_at: Utc::now(),
            command: command_name.to_string(),
            chainlink_feed_id: chainlink.feed_id.clone(),
            notes: vec![
                "Rust-only implementation".to_string(),
                "Log-only; no execution path".to_string(),
                "Historical compare uses Chainlink 1-minute bars".to_string(),
            ],
        },
    )?;

    match cli.command {
        Command::RunAll {
            backfill_hours,
            live_duration_seconds,
            poll_interval_ms,
        } => {
            let bars = fetch_chainlink_1d_bars(&client, &chainlink).await?;
            write_json("data/chainlink/historical_1d_bars.json", &bars)?;
            run_backfill(&client, &bars, backfill_hours).await?;
            run_live(
                &client,
                &chainlink,
                &bars,
                live_duration_seconds,
                poll_interval_ms,
            )
            .await?;
        }
        Command::Backfill { hours } => {
            let bars = fetch_chainlink_1d_bars(&client, &chainlink).await?;
            write_json("data/chainlink/historical_1d_bars.json", &bars)?;
            run_backfill(&client, &bars, hours).await?;
        }
        Command::Live {
            duration_seconds,
            poll_interval_ms,
        } => {
            let bars = fetch_chainlink_1d_bars(&client, &chainlink).await?;
            write_json("data/chainlink/historical_1d_bars.json", &bars)?;
            run_live(
                &client,
                &chainlink,
                &bars,
                duration_seconds,
                poll_interval_ms,
            )
            .await?;
        }
    }

    Ok(())
}

async fn run_live(
    client: &reqwest::Client,
    chainlink: &ChainlinkContext,
    bars: &[ChainlinkMinuteBar],
    duration_seconds: u64,
    poll_interval_ms: u64,
) -> Result<()> {
    let start = Instant::now();
    let mut seen_reports = HashSet::new();
    let bar_map = bars_by_minute(bars);

    while start.elapsed() < Duration::from_secs(duration_seconds) {
        let reports = fetch_chainlink_live_reports(client, chainlink).await?;
        for report in &reports {
            let key = report.valid_from.timestamp_millis();
            if seen_reports.insert(key) {
                append_jsonl("data/chainlink/live_reports.ndjson", report)?;
            }
        }

        let snapshot = scan_live_once(
            client,
            &bar_map,
            reports.as_slice(),
            reports.last().cloned(),
        )
        .await?;
        append_jsonl("data/live/pair_scans.ndjson", &snapshot)?;

        tokio::time::sleep(Duration::from_millis(poll_interval_ms)).await;
    }

    Ok(())
}

async fn run_backfill(
    client: &reqwest::Client,
    bars: &[ChainlinkMinuteBar],
    hours: u64,
) -> Result<()> {
    let bar_map = bars_by_minute(bars);
    let now_et = Utc::now().with_timezone(&New_York);
    let current_15m_start = floor_to_window(now_et, 15);
    let last_completed_15m_start = current_15m_start - ChronoDuration::minutes(15);
    let window_count = ((hours * 60) / 15).max(1) as i64;
    let first_start = last_completed_15m_start - ChronoDuration::minutes((window_count - 1) * 15);

    let starts: Vec<_> = (0..window_count)
        .map(|idx| first_start + ChronoDuration::minutes(idx * 15))
        .collect();

    let results = stream::iter(starts.into_iter().map(|start_et| {
        let client = client.clone();
        let bar_map = bar_map.clone();
        async move { backfill_pair(&client, &bar_map, start_et).await }
    }))
    .buffer_unordered(8)
    .collect::<Vec<_>>()
    .await;

    for record in results {
        append_jsonl("data/historical/pair_backfill.ndjson", &record?)?;
    }

    Ok(())
}

async fn scan_live_once(
    client: &reqwest::Client,
    bars: &HashMap<i64, ChainlinkMinuteBar>,
    reports: &[ChainlinkLiveReport],
    latest_report: Option<ChainlinkLiveReport>,
) -> Result<LivePairScan> {
    let now_et = Utc::now().with_timezone(&New_York);
    let current_15m_start = floor_to_window(now_et, 15);
    let final_5m_start = current_15m_start + ChronoDuration::minutes(10);

    if now_et < final_5m_start {
        return Ok(LivePairScan {
            scanned_at: Utc::now(),
            status: "waiting_for_final_5m".to_string(),
            current_et: now_et.to_rfc3339(),
            current_15m_start_et: current_15m_start.to_rfc3339(),
            current_final_5m_start_et: final_5m_start.to_rfc3339(),
            next_eligible_et: final_5m_start.to_rfc3339(),
            source_latest: latest_report,
            pair: None,
        });
    }

    let five_start_utc = final_5m_start.with_timezone(&Utc);
    let live_five_anchor = reports
        .iter()
        .find(|report| report.valid_from >= five_start_utc)
        .map(|report| PriceBeat {
            slug: format!("btc-updown-5m-{}", five_start_utc.timestamp()),
            price_to_beat: report.price,
            source: "chainlink_live_stream_benchmark".to_string(),
            extracted_at: report.valid_from,
        });

    let Some(five_anchor) = live_five_anchor else {
        return Ok(LivePairScan {
            scanned_at: Utc::now(),
            status: "waiting_for_chainlink_open_tick".to_string(),
            current_et: now_et.to_rfc3339(),
            current_15m_start_et: current_15m_start.to_rfc3339(),
            current_final_5m_start_et: final_5m_start.to_rfc3339(),
            next_eligible_et: final_5m_start.to_rfc3339(),
            source_latest: latest_report,
            pair: None,
        });
    };

    let detail = load_pair_detail(client, current_15m_start, bars, true, Some(five_anchor)).await?;
    Ok(LivePairScan {
        scanned_at: Utc::now(),
        status: "pair_live".to_string(),
        current_et: now_et.to_rfc3339(),
        current_15m_start_et: current_15m_start.to_rfc3339(),
        current_final_5m_start_et: final_5m_start.to_rfc3339(),
        next_eligible_et: final_5m_start.to_rfc3339(),
        source_latest: latest_report,
        pair: Some(detail),
    })
}

async fn backfill_pair(
    client: &reqwest::Client,
    bars: &HashMap<i64, ChainlinkMinuteBar>,
    start_et: chrono::DateTime<chrono_tz::Tz>,
) -> Result<HistoricalPairRecord> {
    let detail = load_pair_detail(client, start_et, bars, false, None).await?;
    let window_end_et = start_et + ChronoDuration::minutes(15);

    Ok(HistoricalPairRecord {
        scanned_at: Utc::now(),
        fifteen_minute_slug: detail.fifteen_minute.slug.clone(),
        five_minute_slug: detail.five_minute.slug.clone(),
        window_start_et: start_et.to_rfc3339(),
        window_end_et: window_end_et.to_rfc3339(),
        fifteen_anchor: detail.fifteen_minute.start_anchor,
        five_anchor: detail.five_minute.start_anchor,
        selected_pair: detail.selected_pair.clone(),
        resolved_outcome_fifteen: resolved_outcome(
            &detail.fifteen_minute.outcomes,
            &detail.fifteen_minute.outcome_prices,
        ),
        resolved_outcome_five: resolved_outcome(
            &detail.five_minute.outcomes,
            &detail.five_minute.outcome_prices,
        ),
        fifteen_market: detail.fifteen_minute,
        five_market: detail.five_minute,
        source_comparison: detail.source_comparison,
    })
}

async fn load_pair_detail(
    client: &reqwest::Client,
    fifteen_start_et: chrono::DateTime<chrono_tz::Tz>,
    bars: &HashMap<i64, ChainlinkMinuteBar>,
    include_orderbooks: bool,
    live_five_anchor_override: Option<PriceBeat>,
) -> Result<LivePairDetail> {
    let five_start_et = fifteen_start_et + ChronoDuration::minutes(10);
    let fifteen_slug = format!(
        "btc-updown-15m-{}",
        fifteen_start_et.with_timezone(&Utc).timestamp()
    );
    let five_slug = format!(
        "btc-updown-5m-{}",
        five_start_et.with_timezone(&Utc).timestamp()
    );

    let (fifteen_market, five_market, fifteen_anchor) = tokio::try_join!(
        fetch_gamma_market(client, &fifteen_slug),
        fetch_gamma_market(client, &five_slug),
        fetch_price_to_beat(client, &fifteen_slug),
    )?;
    let live_mode = live_five_anchor_override.is_some();
    let polymarket_five_anchor: Option<PriceBeat> = None;
    let five_anchor = match live_five_anchor_override {
        Some(anchor) => anchor,
        None => fetch_price_to_beat(client, &five_slug).await?,
    };

    let fifteen_snapshot = MarketSnapshot {
        slug: fifteen_market.slug.clone(),
        question: fifteen_market.question.clone(),
        condition_id: fifteen_market.condition_id.clone(),
        start_anchor: fifteen_anchor.price_to_beat,
        start_anchor_source: fifteen_anchor.source.clone(),
        start_anchor_captured_at: fifteen_anchor.extracted_at,
        outcomes: fifteen_market.outcomes.clone(),
        outcome_prices: fifteen_market.outcome_prices.clone(),
        fees_enabled: fifteen_market.fees_enabled.unwrap_or(false),
        fee_schedule: fifteen_market.fee_schedule.clone(),
        best_bid: fifteen_market.best_bid,
        best_ask: fifteen_market.best_ask,
    };

    let five_snapshot = MarketSnapshot {
        slug: five_market.slug.clone(),
        question: five_market.question.clone(),
        condition_id: five_market.condition_id.clone(),
        start_anchor: five_anchor.price_to_beat,
        start_anchor_source: five_anchor.source.clone(),
        start_anchor_captured_at: five_anchor.extracted_at,
        outcomes: five_market.outcomes.clone(),
        outcome_prices: five_market.outcome_prices.clone(),
        fees_enabled: five_market.fees_enabled.unwrap_or(false),
        fee_schedule: five_market.fee_schedule.clone(),
        best_bid: five_market.best_bid,
        best_ask: five_market.best_ask,
    };

    let selected_pair = if fifteen_anchor.price_to_beat < five_anchor.price_to_beat {
        "U15+D5".to_string()
    } else if fifteen_anchor.price_to_beat > five_anchor.price_to_beat {
        "D15+U5".to_string()
    } else {
        "equal_skip".to_string()
    };

    let source_comparison = source_comparison(
        fifteen_start_et.with_timezone(&Utc),
        five_start_et.with_timezone(&Utc),
        (fifteen_start_et + ChronoDuration::minutes(15)).with_timezone(&Utc),
        fifteen_anchor.price_to_beat,
        five_anchor.price_to_beat,
        polymarket_five_anchor
            .as_ref()
            .map(|anchor| anchor.price_to_beat),
        live_mode.then_some(five_anchor.price_to_beat),
        bars,
    );

    let mut leg_books = Vec::new();
    let mut executable_costs = BTreeMap::new();
    let mut all_in_best_ask = None;
    let mut edge = None;

    if selected_pair != "equal_skip" && include_orderbooks {
        let selected = select_legs(&selected_pair, &fifteen_market, &five_market)?;
        let (book_one, book_two) = tokio::try_join!(
            fetch_orderbook(client, &selected[0].token_id),
            fetch_orderbook(client, &selected[1].token_id),
        )?;

        let leg_one = LegOrderbookSnapshot {
            outcome: selected[0].outcome.clone(),
            token_id: selected[0].token_id.clone(),
            best_bid: book_one.bids.first().map(|x| x.price),
            best_ask: book_one.asks.first().map(|x| x.price),
            top_bids: book_one.bids.iter().take(5).cloned().collect(),
            top_asks: book_one.asks.iter().take(5).cloned().collect(),
        };
        let leg_two = LegOrderbookSnapshot {
            outcome: selected[1].outcome.clone(),
            token_id: selected[1].token_id.clone(),
            best_bid: book_two.bids.first().map(|x| x.price),
            best_ask: book_two.asks.first().map(|x| x.price),
            top_bids: book_two.bids.iter().take(5).cloned().collect(),
            top_asks: book_two.asks.iter().take(5).cloned().collect(),
        };

        leg_books.push(leg_one.clone());
        leg_books.push(leg_two.clone());

        for qty in [10.0, 25.0, 50.0, 100.0] {
            if let (Some(cost_one), Some(cost_two)) = (
                executable_cost(qty, &book_one.asks, selected[0].fee_schedule),
                executable_cost(qty, &book_two.asks, selected[1].fee_schedule),
            ) {
                executable_costs.insert(format!("{qty:.0}"), cost_one + cost_two);
            }
        }

        all_in_best_ask =
            if let (Some(ask_one), Some(ask_two)) = (leg_one.best_ask, leg_two.best_ask) {
                Some(
                    ask_one
                        + ask_two
                        + taker_fee(1.0, ask_one, selected[0].fee_schedule)
                        + taker_fee(1.0, ask_two, selected[1].fee_schedule),
                )
            } else {
                None
            };
        edge = all_in_best_ask.map(|cost| 1.0 - cost);
    }

    Ok(LivePairDetail {
        fifteen_minute: fifteen_snapshot,
        five_minute: five_snapshot,
        selected_pair,
        floor_payout: 1.0,
        estimated_all_in_best_ask_cost: all_in_best_ask,
        estimated_edge_best_ask: edge,
        executable_costs,
        leg_books,
        source_comparison,
    })
}

fn resolved_outcome(outcomes: &[String], prices: &[f64]) -> Option<String> {
    outcomes
        .iter()
        .zip(prices.iter())
        .find_map(|(outcome, price)| {
            if (*price - 1.0).abs() < 1e-9 {
                Some(outcome.clone())
            } else {
                None
            }
        })
}

fn select_legs<'a>(
    selected_pair: &str,
    fifteen_market: &'a GammaMarket,
    five_market: &'a GammaMarket,
) -> Result<[SelectedLeg<'a>; 2]> {
    let fifteen_map = outcome_token_map(fifteen_market)?;
    let five_map = outcome_token_map(five_market)?;

    match selected_pair {
        "U15+D5" => Ok([
            SelectedLeg {
                outcome: "Up".to_string(),
                token_id: fifteen_map
                    .get("Up")
                    .cloned()
                    .ok_or_else(|| anyhow!("missing Up token for {}", fifteen_market.slug))?,
                fee_schedule: fifteen_market.fee_schedule.as_ref(),
            },
            SelectedLeg {
                outcome: "Down".to_string(),
                token_id: five_map
                    .get("Down")
                    .cloned()
                    .ok_or_else(|| anyhow!("missing Down token for {}", five_market.slug))?,
                fee_schedule: five_market.fee_schedule.as_ref(),
            },
        ]),
        "D15+U5" => Ok([
            SelectedLeg {
                outcome: "Down".to_string(),
                token_id: fifteen_map
                    .get("Down")
                    .cloned()
                    .ok_or_else(|| anyhow!("missing Down token for {}", fifteen_market.slug))?,
                fee_schedule: fifteen_market.fee_schedule.as_ref(),
            },
            SelectedLeg {
                outcome: "Up".to_string(),
                token_id: five_map
                    .get("Up")
                    .cloned()
                    .ok_or_else(|| anyhow!("missing Up token for {}", five_market.slug))?,
                fee_schedule: five_market.fee_schedule.as_ref(),
            },
        ]),
        _ => Err(anyhow!("unsupported pair selection {selected_pair}")),
    }
}

fn outcome_token_map(market: &GammaMarket) -> Result<HashMap<String, String>> {
    if market.outcomes.len() != market.clob_token_ids.len() {
        return Err(anyhow!(
            "outcome/token mismatch for {}: {} outcomes vs {} tokens",
            market.slug,
            market.outcomes.len(),
            market.clob_token_ids.len()
        ));
    }

    Ok(market
        .outcomes
        .iter()
        .cloned()
        .zip(market.clob_token_ids.iter().cloned())
        .collect())
}

fn source_comparison(
    start_15m: DateTime<Utc>,
    start_5m: DateTime<Utc>,
    end_15m: DateTime<Utc>,
    fifteen_anchor: f64,
    five_anchor: f64,
    polymarket_five_anchor: Option<f64>,
    chainlink_live_five_anchor: Option<f64>,
    bars: &HashMap<i64, ChainlinkMinuteBar>,
) -> SourceComparison {
    let open_15m = bars.get(&start_15m.timestamp()).map(|bar| bar.open);
    let open_5m = bars.get(&start_5m.timestamp()).map(|bar| bar.open);
    let end_price = bars.get(&end_15m.timestamp()).map(|bar| bar.open);
    let live_vs_poly_delta = chainlink_live_five_anchor
        .zip(polymarket_five_anchor)
        .map(|(live_b, poly_b)| live_b - poly_b);

    SourceComparison {
        chainlink_open_15m_minute: open_15m,
        chainlink_open_5m_minute: open_5m,
        chainlink_end_minute: end_price,
        delta_15m_anchor: open_15m.map(|price| fifteen_anchor - price),
        delta_5m_anchor: open_5m.map(|price| five_anchor - price),
        polymarket_five_anchor,
        chainlink_live_five_anchor,
        delta_live_b_vs_polymarket_b: live_vs_poly_delta,
        live_b_matches_polymarket_b: live_vs_poly_delta
            .map(|delta| delta.abs() <= ANCHOR_MATCH_TOLERANCE),
    }
}

fn executable_cost(
    qty: f64,
    asks: &[OrderLevel],
    fee_schedule: Option<&FeeSchedule>,
) -> Option<f64> {
    let mut remaining = qty;
    let mut gross = 0.0;

    for level in asks {
        if remaining <= 0.0 {
            break;
        }
        let fill = remaining.min(level.size);
        gross += fill * level.price;
        remaining -= fill;
    }

    if remaining > 1e-9 {
        return None;
    }

    let avg_price = gross / qty;
    Some(gross + taker_fee(qty, avg_price, fee_schedule))
}

fn taker_fee(qty: f64, price: f64, fee_schedule: Option<&FeeSchedule>) -> f64 {
    let Some(schedule) = fee_schedule else {
        return 0.0;
    };
    qty * price * schedule.rate * (price * (1.0 - price)).powi(schedule.exponent as i32)
}

fn bars_by_minute(bars: &[ChainlinkMinuteBar]) -> HashMap<i64, ChainlinkMinuteBar> {
    bars.iter()
        .cloned()
        .map(|bar| (bar.minute_start.timestamp(), bar))
        .collect()
}

async fn fetch_chainlink_context(client: &reqwest::Client) -> Result<ChainlinkContext> {
    let html = client
        .get(CHAINLINK_STREAM_PAGE)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;

    let next_data_regex =
        Regex::new(r#"<script id="__NEXT_DATA__" type="application/json">(.*?)</script>"#)?;
    let captures = next_data_regex
        .captures(&html)
        .ok_or_else(|| anyhow!("could not locate Chainlink __NEXT_DATA__ payload"))?;
    let next_json = captures
        .get(1)
        .ok_or_else(|| anyhow!("missing Chainlink JSON capture"))?
        .as_str();
    let payload: Value = serde_json::from_str(next_json)?;

    let stream_data = &payload["props"]["pageProps"]["streamData"];
    let feed_id = stream_data["streamMetadata"]["feedId"]
        .as_str()
        .ok_or_else(|| anyhow!("missing Chainlink feedId"))?
        .to_string();
    let multiply = stream_data["streamMetadata"]["multiply"]
        .as_str()
        .ok_or_else(|| anyhow!("missing Chainlink multiply"))?
        .parse::<f64>()
        .context("invalid Chainlink multiply")?;
    let slug = stream_data["extraConfig"]["slug"]
        .as_str()
        .unwrap_or("btc-usd")
        .to_string();
    let stream_name = stream_data["streamMetadata"]["name"]
        .as_str()
        .unwrap_or("BTC/USD")
        .to_string();
    let source_chain = stream_data["streamMetadata"]["sourceChain"].as_u64();

    Ok(ChainlinkContext {
        stream_page_url: CHAINLINK_STREAM_PAGE.to_string(),
        resolved_page_slug: slug,
        feed_id,
        multiply,
        stream_name,
        source_chain,
        fetched_at: Utc::now(),
    })
}

async fn fetch_chainlink_live_reports(
    client: &reqwest::Client,
    chainlink: &ChainlinkContext,
) -> Result<Vec<ChainlinkLiveReport>> {
    let response: Value = client
        .get(CHAINLINK_LIVE_STREAM_URL)
        .query(&[
            ("feedId", chainlink.feed_id.as_str()),
            ("abiIndex", "0"),
            ("queryWindow", "1m"),
        ])
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let nodes = response["data"]["allStreamValuesGenerics"]["nodes"]
        .as_array()
        .ok_or_else(|| anyhow!("missing allStreamValuesGenerics nodes"))?;

    let mut grouped: BTreeMap<DateTime<Utc>, ChainlinkLiveReport> = BTreeMap::new();
    for node in nodes {
        let valid_from = node["validAfterTs"]
            .as_str()
            .ok_or_else(|| anyhow!("missing validAfterTs"))?;
        let attribute = node["attributeName"]
            .as_str()
            .ok_or_else(|| anyhow!("missing attributeName"))?;
        let value = node["valueNumeric"]
            .as_str()
            .ok_or_else(|| anyhow!("missing valueNumeric"))?
            .parse::<f64>()?;
        let ts = DateTime::parse_from_rfc3339(valid_from)?.with_timezone(&Utc);
        let entry = grouped.entry(ts).or_insert(ChainlinkLiveReport {
            valid_from: ts,
            price: f64::NAN,
            bid: None,
            ask: None,
        });
        match attribute {
            "benchmark" => entry.price = value,
            "bid" => entry.bid = Some(value),
            "ask" => entry.ask = Some(value),
            _ => {}
        }
    }

    let mut out: Vec<_> = grouped
        .into_values()
        .filter(|report| report.price.is_finite())
        .collect();
    out.sort_by_key(|report| report.valid_from);
    Ok(out)
}

async fn fetch_chainlink_1d_bars(
    client: &reqwest::Client,
    chainlink: &ChainlinkContext,
) -> Result<Vec<ChainlinkMinuteBar>> {
    let response: Value = client
        .get(CHAINLINK_1D_BARS_URL)
        .query(&[
            ("feedId", chainlink.feed_id.as_str()),
            ("abiIndex", "0"),
            ("timeRange", "1D"),
        ])
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let nodes = response["data"]["allStreamValuesGeneric1Minutes"]["nodes"]
        .as_array()
        .ok_or_else(|| anyhow!("missing Chainlink 1D nodes"))?;
    let candle_regex = Regex::new(r#"(open|high|low|close):\(ts:"([^"]+)",val:([\d.\-]+)\)"#)?;

    let mut bars = Vec::new();
    for node in nodes {
        if node["attributeName"].as_str() != Some("benchmark") {
            continue;
        }
        let bucket = node["bucket"]
            .as_str()
            .ok_or_else(|| anyhow!("missing bucket"))?;
        let candlestick = node["candlestick"]
            .as_str()
            .ok_or_else(|| anyhow!("missing candlestick"))?;
        let parsed = parse_candlestick(candlestick, &candle_regex)
            .with_context(|| format!("failed to parse candlestick for bucket {bucket}"))?;
        bars.push(ChainlinkMinuteBar {
            minute_start: DateTime::parse_from_rfc3339(bucket)?.with_timezone(&Utc),
            open: parsed["open"],
            high: parsed["high"],
            low: parsed["low"],
            close: parsed["close"],
        });
    }

    bars.sort_by_key(|bar| bar.minute_start);
    Ok(bars)
}

fn parse_candlestick(candlestick: &str, regex: &Regex) -> Result<HashMap<String, f64>> {
    let mut parts = HashMap::new();
    for capture in regex.captures_iter(candlestick) {
        let label = capture
            .get(1)
            .ok_or_else(|| anyhow!("missing candle label"))?
            .as_str()
            .to_string();
        let value = capture
            .get(3)
            .ok_or_else(|| anyhow!("missing candle value"))?
            .as_str()
            .parse::<f64>()?;
        parts.insert(label, value);
    }

    if ["open", "high", "low", "close"]
        .iter()
        .all(|key| parts.contains_key(*key))
    {
        Ok(parts)
    } else {
        Err(anyhow!("incomplete candlestick payload"))
    }
}

async fn fetch_gamma_market(client: &reqwest::Client, slug: &str) -> Result<GammaMarket> {
    client
        .get(format!("{POLYMARKET_GAMMA_BY_SLUG}/{slug}"))
        .send()
        .await?
        .error_for_status()?
        .json::<GammaMarket>()
        .await
        .with_context(|| format!("failed to deserialize market {slug}"))
}

async fn fetch_price_to_beat(client: &reqwest::Client, slug: &str) -> Result<PriceBeat> {
    let html = client
        .get(format!("{POLYMARKET_EVENT_PAGE}/{slug}"))
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;

    let scoped_pattern = format!(
        r#"(?s)"slug":"{}".*?"priceToBeat":([0-9.]+)"#,
        regex::escape(slug)
    );
    let regex = Regex::new(&scoped_pattern)?;
    let captures = regex
        .captures(&html)
        .ok_or_else(|| anyhow!("priceToBeat not found for {slug}"))?;
    let price = captures
        .get(1)
        .ok_or_else(|| anyhow!("missing price capture for {slug}"))?
        .as_str()
        .parse::<f64>()
        .with_context(|| format!("invalid priceToBeat for {slug}"))?;

    Ok(PriceBeat {
        slug: slug.to_string(),
        price_to_beat: price,
        source: "polymarket_event_html".to_string(),
        extracted_at: Utc::now(),
    })
}

async fn fetch_orderbook(client: &reqwest::Client, token_id: &str) -> Result<OrderBook> {
    client
        .get(POLYMARKET_ORDERBOOK)
        .query(&[("token_id", token_id)])
        .send()
        .await?
        .error_for_status()?
        .json::<OrderBook>()
        .await
        .with_context(|| format!("failed to deserialize orderbook for token {token_id}"))
}

fn floor_to_window(
    dt: chrono::DateTime<chrono_tz::Tz>,
    window_minutes: u32,
) -> chrono::DateTime<chrono_tz::Tz> {
    let floored_minute = (dt.minute() / window_minutes) * window_minutes;
    dt.with_minute(floored_minute)
        .and_then(|v| v.with_second(0))
        .and_then(|v| v.with_nanosecond(0))
        .expect("valid floored datetime")
}

fn build_client() -> Result<reqwest::Client> {
    let mut headers = HeaderMap::new();
    headers.insert(
        USER_AGENT,
        HeaderValue::from_static("prediction-market-btc-arb/0.1 (+rust log-only scanner)"),
    );
    headers.insert(ACCEPT, HeaderValue::from_static("*/*"));

    Ok(reqwest::Client::builder()
        .default_headers(headers)
        .build()?)
}

fn append_jsonl<P: AsRef<Path>, T: Serialize>(path: P, value: &T) -> Result<()> {
    if let Some(parent) = path.as_ref().parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(file, "{}", serde_json::to_string(value)?)?;
    Ok(())
}

fn write_json<P: AsRef<Path>, T: Serialize>(path: P, value: &T) -> Result<()> {
    if let Some(parent) = path.as_ref().parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_vec_pretty(value)?)?;
    Ok(())
}

fn de_json_string_vec<'de, D>(deserializer: D) -> std::result::Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let raw = String::deserialize(deserializer)?;
    serde_json::from_str(&raw).map_err(D::Error::custom)
}

fn de_json_string_vec_f64<'de, D>(deserializer: D) -> std::result::Result<Vec<f64>, D::Error>
where
    D: Deserializer<'de>,
{
    let raw = String::deserialize(deserializer)?;
    let strings: Vec<String> = serde_json::from_str(&raw).map_err(D::Error::custom)?;
    strings
        .into_iter()
        .map(|item| item.parse::<f64>().map_err(D::Error::custom))
        .collect()
}

fn de_opt_f64_from_any<'de, D>(deserializer: D) -> std::result::Result<Option<f64>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<Value>::deserialize(deserializer)?;
    value
        .map(value_to_f64)
        .transpose()
        .map_err(D::Error::custom)
}

fn de_f64_from_any<'de, D>(deserializer: D) -> std::result::Result<f64, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Value::deserialize(deserializer)?;
    value_to_f64(value).map_err(D::Error::custom)
}

fn value_to_f64(value: Value) -> Result<f64> {
    match value {
        Value::Number(number) => number
            .as_f64()
            .ok_or_else(|| anyhow!("number is not representable as f64")),
        Value::String(text) => text.parse::<f64>().map_err(Into::into),
        other => Err(anyhow!("expected string or number, got {other:?}")),
    }
}
