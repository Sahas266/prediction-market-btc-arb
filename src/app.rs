use crate::cli::{Cli, Command};
use crate::io::{append_jsonl, ensure_data_dirs, read_jsonl, resolve_input_path, write_json};
use crate::models::{
    ChainlinkContext, ChainlinkLiveReport, GammaMarket, HistoricalPairRecord, LegOrderbookSnapshot,
    LivePairScan, PriceBeat, RunManifest, SelectedLegOwned,
};
use crate::sources::{
    ManagedOrderBook, OrderbookStreamHandle, build_client, fetch_chainlink_1d_bars,
    fetch_chainlink_context, fetch_chainlink_live_reports, fetch_gamma_market,
    fetch_price_to_beat, log_http_orderbook_snapshot, start_orderbook_stream,
    write_chainlink_context,
};
use crate::strategy::{
    bars_by_minute, build_live_pair_detail, build_live_paper_trade, build_paper_trades,
    floor_to_window, parse_date_filter, reconcile_trade, resolved_outcome, select_legs,
    summarize_windows,
};
use anyhow::Result;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use chrono_tz::America::New_York;
use futures::{StreamExt, stream};
use std::collections::{BTreeMap, HashMap};
use std::time::{Duration, Instant};

struct ActiveWindow {
    final_5m_start_et: chrono::DateTime<chrono_tz::Tz>,
    fifteen_market: GammaMarket,
    five_market: GammaMarket,
    fifteen_anchor: PriceBeat,
    five_anchor: PriceBeat,
    selected_legs: Option<[SelectedLegOwned; 2]>,
    selected_pair: String,
    stream_handle: Option<OrderbookStreamHandle>,
    last_http_sanity: Instant,
    sanity_interval: Duration,
    first_pair_snapshot_at: Option<DateTime<Utc>>,
    live_trade_emitted: bool,
    previous_selected_pair: Option<String>,
}

impl ActiveWindow {
    async fn stop(self) -> Result<()> {
        if let Some(handle) = self.stream_handle {
            handle.stop().await?;
        }
        Ok(())
    }
}

pub async fn run(cli: Cli) -> Result<()> {
    ensure_data_dirs()?;

    match cli.command {
        Command::RunAll {
            backfill_hours,
            live_duration_seconds,
            poll_interval_ms,
        } => {
            let client = build_client()?;
            let chainlink = fetch_chainlink_context(&client).await?;
            prepare_run_manifest("run-all", &chainlink)?;
            write_chainlink_context("data/chainlink/context.json", &chainlink).await?;
            let bars = fetch_chainlink_1d_bars(&client, &chainlink).await?;
            write_json("data/chainlink/historical_1d_bars.json", &bars)?;
            run_backfill(&client, &bars, backfill_hours).await?;
            run_live(&client, &chainlink, &bars, live_duration_seconds, poll_interval_ms).await?;
        }
        Command::Backfill { hours } => {
            let client = build_client()?;
            let chainlink = fetch_chainlink_context(&client).await?;
            prepare_run_manifest("backfill", &chainlink)?;
            write_chainlink_context("data/chainlink/context.json", &chainlink).await?;
            let bars = fetch_chainlink_1d_bars(&client, &chainlink).await?;
            write_json("data/chainlink/historical_1d_bars.json", &bars)?;
            run_backfill(&client, &bars, hours).await?;
        }
        Command::Live {
            duration_seconds,
            poll_interval_ms,
        } => {
            let client = build_client()?;
            let chainlink = fetch_chainlink_context(&client).await?;
            prepare_run_manifest("live", &chainlink)?;
            write_chainlink_context("data/chainlink/context.json", &chainlink).await?;
            let bars = fetch_chainlink_1d_bars(&client, &chainlink).await?;
            write_json("data/chainlink/historical_1d_bars.json", &bars)?;
            run_live(&client, &chainlink, &bars, duration_seconds, poll_interval_ms).await?;
        }
        Command::PaperTrade {
            input,
            min_edge_cents,
            target_shares,
        } => {
            let client = build_client()?;
            let scans = load_scans(&input, &None, &None)?;
            let mut trades = build_paper_trades(&scans, min_edge_cents, target_shares);
            reconcile_paper_trades(&client, &mut trades).await?;
            write_json("data/paper/latest_paper_trades.json", &trades)?;
        }
        Command::Replay { input } => {
            let scans = load_scans(&input, &None, &None)?;
            let trades = build_paper_trades(&scans, 2.0, 5.0);
            let trade_map = trades
                .iter()
                .cloned()
                .map(|trade| (trade.window_start_et.clone(), trade))
                .collect();
            let summaries = summarize_windows(&scans, &trade_map);
            write_json(
                "data/replay/latest_replay.json",
                &serde_json::json!({
                    "replayed_at": Utc::now(),
                    "input": resolve_input_path(&input),
                    "paper_trades": trades,
                    "window_summaries": summaries,
                }),
            )?;
        }
        Command::Summarize { input, from, to } => {
            let scans = load_scans(&input, &from, &to)?;
            let trade_path = "data/paper/latest_paper_trades.json";
            let trade_map = if std::path::Path::new(trade_path).exists() {
                let trades: Vec<crate::models::PaperTradeRecord> =
                    serde_json::from_slice(&std::fs::read(trade_path)?)?;
                trades
                    .into_iter()
                    .map(|trade| (trade.window_start_et.clone(), trade))
                    .collect()
            } else {
                HashMap::new()
            };
            let summaries = summarize_windows(&scans, &trade_map);
            write_json("data/analysis/latest_window_summaries.json", &summaries)?;
        }
    }

    Ok(())
}

fn prepare_run_manifest(command: &str, chainlink: &ChainlinkContext) -> Result<()> {
    write_json(
        "data/runs/latest_run.json",
        &RunManifest {
            started_at: Utc::now(),
            command: command.to_string(),
            chainlink_feed_id: chainlink.feed_id.clone(),
            notes: vec![
                "Rust-only implementation".to_string(),
                "Paper trading only; no live execution path".to_string(),
                "Live mode records raw Chainlink ticks and Polymarket orderbook events".to_string(),
            ],
        },
    )
}

async fn run_backfill(
    client: &reqwest::Client,
    bars: &[crate::models::ChainlinkMinuteBar],
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

async fn run_live(
    client: &reqwest::Client,
    chainlink: &ChainlinkContext,
    bars: &[crate::models::ChainlinkMinuteBar],
    duration_seconds: u64,
    poll_interval_ms: u64,
) -> Result<()> {
    let start = Instant::now();
    let mut seen_reports: BTreeMap<i64, ChainlinkLiveReport> = BTreeMap::new();
    let bar_map = bars_by_minute(bars);
    let mut active_window: Option<ActiveWindow> = None;
    let chainlink_refresh_interval = Duration::from_millis(poll_interval_ms);
    let active_monitor_interval =
        Duration::from_millis(read_env_u64("LIVE_ACTIVE_POLL_MS", 10).max(1));
    let mut last_chainlink_fetch = Instant::now() - chainlink_refresh_interval;
    let live_min_edge_cents = read_env_f64("PAPER_MIN_EDGE_CENTS", 2.0);
    let live_target_shares = read_env_f64("PAPER_TARGET_SHARES", 5.0);

    while start.elapsed() < Duration::from_secs(duration_seconds) {
        let now_et = Utc::now().with_timezone(&New_York);
        let current_15m_start = floor_to_window(now_et, 15);
        let current_final_5m_start = current_15m_start + ChronoDuration::minutes(10);

        if active_window
            .as_ref()
            .map(|window| window.final_5m_start_et != current_final_5m_start)
            .unwrap_or(false)
        {
            finalize_live_window(active_window.as_ref().unwrap())?;
            if let Some(window) = active_window.take() {
                window.stop().await?;
            }
        }

        if active_window.is_none() || last_chainlink_fetch.elapsed() >= chainlink_refresh_interval {
            let reports = fetch_chainlink_live_reports(client, chainlink, "1m").await?;
            ingest_reports(&mut seen_reports, reports)?;
            last_chainlink_fetch = Instant::now();
        }

        let snapshot =
            scan_live_once(client, chainlink, &bar_map, &seen_reports, &mut active_window).await?;
        append_jsonl("data/live/pair_scans.ndjson", &snapshot)?;
        process_live_paper_trade(
            &snapshot,
            &mut active_window,
            live_min_edge_cents,
            live_target_shares,
        )?;

        let sleep_for = if active_window.is_some() {
            active_monitor_interval
        } else {
            chainlink_refresh_interval
        };
        tokio::time::sleep(sleep_for).await;
    }

    if let Some(window) = active_window.take() {
        window.stop().await?;
    }

    Ok(())
}

fn ingest_reports(
    seen_reports: &mut BTreeMap<i64, ChainlinkLiveReport>,
    reports: Vec<ChainlinkLiveReport>,
) -> Result<()> {
    for report in reports {
        let key = report.valid_from.timestamp_millis();
        if seen_reports.insert(key, report.clone()).is_none() {
            append_jsonl("data/chainlink/live_reports.ndjson", &report)?;
            append_jsonl("data/live/chainlink_ticks.ndjson", &report)?;
        }
    }
    Ok(())
}

async fn scan_live_once(
    client: &reqwest::Client,
    chainlink: &ChainlinkContext,
    bars: &HashMap<i64, crate::models::ChainlinkMinuteBar>,
    seen_reports: &BTreeMap<i64, ChainlinkLiveReport>,
    active_window: &mut Option<ActiveWindow>,
) -> Result<LivePairScan> {
    let now_et = Utc::now().with_timezone(&New_York);
    let current_15m_start = floor_to_window(now_et, 15);
    let final_5m_start = current_15m_start + ChronoDuration::minutes(10);

    if let Some(window) = active_window.as_ref() {
        if window.final_5m_start_et != final_5m_start {
            if let Some(window) = active_window.take() {
                window.stop().await?;
            }
        }
    }

    let latest_report = seen_reports.values().last().cloned();

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
    let mut opening_report = first_report_at_or_after(seen_reports, five_start_utc);
    if opening_report.is_none() {
        let bootstrap = match fetch_chainlink_live_reports(client, chainlink, "5m").await {
            Ok(reports) => reports,
            Err(_) => fetch_chainlink_live_reports(client, chainlink, "1m").await?,
        };
        let mut boot_seen = seen_reports.clone();
        ingest_reports(&mut boot_seen, bootstrap)?;
        opening_report = first_report_at_or_after(&boot_seen, five_start_utc);
    }

    let Some(open_report) = opening_report else {
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

    if active_window.is_none() {
        *active_window = Some(initialize_active_window(client, current_15m_start, open_report).await?);
    }

    let window = active_window.as_mut().expect("window just initialized");
    let leg_books = collect_live_leg_books(client, window).await?;
    let mut detail = build_live_pair_detail(
        &window.fifteen_market,
        &window.fifteen_anchor,
        &window.five_market,
        &window.five_anchor,
        bars,
        leg_books,
        window.previous_selected_pair.as_deref(),
    )?;
    window.previous_selected_pair = Some(detail.selected_pair.clone());
    detail.selected_pair_changed = detail.selected_pair != window.selected_pair;
    window.first_pair_snapshot_at.get_or_insert(Utc::now());

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

fn first_report_at_or_after(
    reports: &BTreeMap<i64, ChainlinkLiveReport>,
    threshold: DateTime<Utc>,
) -> Option<ChainlinkLiveReport> {
    reports
        .values()
        .find(|report| report.valid_from >= threshold)
        .cloned()
}

async fn initialize_active_window(
    client: &reqwest::Client,
    fifteen_start_et: chrono::DateTime<chrono_tz::Tz>,
    opening_report: ChainlinkLiveReport,
) -> Result<ActiveWindow> {
    let five_start_et = fifteen_start_et + ChronoDuration::minutes(10);
    let fifteen_slug = format!(
        "btc-updown-15m-{}",
        fifteen_start_et.with_timezone(&Utc).timestamp()
    );
    let five_slug = format!(
        "btc-updown-5m-{}",
        five_start_et.with_timezone(&Utc).timestamp()
    );
    let (fifteen_market, five_market) = tokio::try_join!(
        fetch_gamma_market(client, &fifteen_slug),
        fetch_gamma_market(client, &five_slug),
    )?;
    let fifteen_anchor = fetch_price_to_beat(client, &fifteen_market).await?;
    let five_anchor = PriceBeat {
        slug: five_slug.clone(),
        price_to_beat: opening_report.price,
        source: "chainlink_live_stream_benchmark".to_string(),
        extracted_at: opening_report.valid_from,
    };
    let selected_pair = crate::strategy::select_pair_label(
        fifteen_anchor.price_to_beat,
        five_anchor.price_to_beat,
    );
    let selected_legs = if selected_pair == "equal_skip" {
        None
    } else {
        Some(select_legs(&selected_pair, &fifteen_market, &five_market)?)
    };
    let stream_handle = if let Some(legs) = &selected_legs {
        let token_ids = vec![legs[0].token_id.clone(), legs[1].token_id.clone()];
        Some(start_orderbook_stream(token_ids).await?)
    } else {
        None
    };

    Ok(ActiveWindow {
        final_5m_start_et: five_start_et,
        fifteen_market,
        five_market,
        fifteen_anchor,
        five_anchor,
        selected_legs,
        selected_pair,
        stream_handle,
        last_http_sanity: Instant::now() - Duration::from_secs(60),
        sanity_interval: Duration::from_secs(read_env_u64("WS_SANITY_SNAPSHOT_SECONDS", 30)),
        first_pair_snapshot_at: None,
        live_trade_emitted: false,
        previous_selected_pair: None,
    })
}

async fn collect_live_leg_books(
    client: &reqwest::Client,
    window: &mut ActiveWindow,
) -> Result<Vec<LegOrderbookSnapshot>> {
    let Some(selected_legs) = &window.selected_legs else {
        return Ok(Vec::new());
    };

    let use_sanity_fetch = window.last_http_sanity.elapsed() >= window.sanity_interval;
    let ws_books = if let Some(handle) = &window.stream_handle {
        handle.state.lock().await.books.clone()
    } else {
        HashMap::new()
    };

    let mut output = Vec::new();
    for leg in selected_legs {
        let ws_book = ws_books.get(&leg.token_id).cloned();
        if use_sanity_fetch {
            let _ = log_http_orderbook_snapshot(client, &leg.token_id, "periodic_sanity").await;
        }
        let (book, managed) = match ws_book.and_then(|managed| managed.book.clone().map(|book| (book, managed))) {
            Some((book, managed)) => (book, managed),
            None => {
                let book = log_http_orderbook_snapshot(client, &leg.token_id, "ws_missing_fallback").await?;
                let managed = ManagedOrderBook {
                    book: Some(book.clone()),
                    source_timestamp: None,
                    source_latency_ms: None,
                    source: "polymarket_http_fallback".to_string(),
                };
                (book, managed)
            }
        };

        output.push(LegOrderbookSnapshot {
            outcome: leg.outcome.clone(),
            token_id: leg.token_id.clone(),
            best_bid: book.bids.first().map(|level| level.price),
            best_ask: book.asks.first().map(|level| level.price),
            top_bids: book.bids.iter().take(16).cloned().collect(),
            top_asks: book.asks.iter().take(16).cloned().collect(),
            source: managed.source,
            source_timestamp: managed.source_timestamp,
            source_latency_ms: managed.source_latency_ms,
        });
    }

    if use_sanity_fetch {
        window.last_http_sanity = Instant::now();
    }

    Ok(output)
}

async fn backfill_pair(
    client: &reqwest::Client,
    bars: &HashMap<i64, crate::models::ChainlinkMinuteBar>,
    start_et: chrono::DateTime<chrono_tz::Tz>,
) -> Result<HistoricalPairRecord> {
    let five_start_et = start_et + ChronoDuration::minutes(10);
    let fifteen_slug = format!("btc-updown-15m-{}", start_et.with_timezone(&Utc).timestamp());
    let five_slug = format!("btc-updown-5m-{}", five_start_et.with_timezone(&Utc).timestamp());

    let (fifteen_market, five_market) = tokio::try_join!(
        fetch_gamma_market(client, &fifteen_slug),
        fetch_gamma_market(client, &five_slug),
    )?;
    let fifteen_anchor = fetch_price_to_beat(client, &fifteen_market).await?;
    let five_anchor = fetch_price_to_beat(client, &five_market).await?;
    let detail = build_live_pair_detail(
        &fifteen_market,
        &fifteen_anchor,
        &five_market,
        &five_anchor,
        bars,
        Vec::new(),
        None,
    )?;
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

fn load_scans(input: &str, from: &Option<String>, to: &Option<String>) -> Result<Vec<LivePairScan>> {
    let path = resolve_input_path(input);
    let scans: Vec<LivePairScan> = read_jsonl(path)?;
    let from_filter = parse_date_filter(from, false)?;
    let to_filter = parse_date_filter(to, true)?;

    Ok(scans
        .into_iter()
        .filter(|scan| {
            let window_dt = DateTime::parse_from_rfc3339(&scan.current_final_5m_start_et)
                .ok()
                .map(|dt| dt.with_timezone(&Utc));
            let after_from = from_filter
                .map(|from| window_dt.map(|dt| dt >= from).unwrap_or(true))
                .unwrap_or(true);
            let before_to = to_filter
                .map(|to| window_dt.map(|dt| dt <= to).unwrap_or(true))
                .unwrap_or(true);
            after_from && before_to
        })
        .collect())
}

async fn reconcile_paper_trades(
    client: &reqwest::Client,
    trades: &mut [crate::models::PaperTradeRecord],
) -> Result<()> {
    for trade in trades.iter_mut() {
        if trade.status != "entered" {
            continue;
        }
        let (fifteen_market, five_market) = tokio::try_join!(
            fetch_gamma_market(client, &trade.fifteen_minute_slug),
            fetch_gamma_market(client, &trade.five_minute_slug),
        )?;
        reconcile_trade(trade, &fifteen_market, &five_market);
    }
    Ok(())
}

fn read_env_u64(key: &str, default: u64) -> u64 {
    std::env::var(key)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(default)
}

fn read_env_f64(key: &str, default: f64) -> f64 {
    std::env::var(key)
        .ok()
        .and_then(|value| value.parse::<f64>().ok())
        .unwrap_or(default)
}

fn process_live_paper_trade(
    snapshot: &LivePairScan,
    active_window: &mut Option<ActiveWindow>,
    min_edge_cents: f64,
    target_shares: f64,
) -> Result<()> {
    let Some(window) = active_window.as_mut() else {
        return Ok(());
    };
    if window.live_trade_emitted {
        return Ok(());
    }
    let Some(trade) = build_live_paper_trade(
        snapshot,
        min_edge_cents,
        target_shares,
        window.first_pair_snapshot_at,
    ) else {
        return Ok(());
    };

    append_jsonl("data/paper/live_paper_trades.ndjson", &trade)?;
    write_json("data/paper/latest_live_paper_trade.json", &trade)?;
    window.live_trade_emitted = true;
    Ok(())
}

fn finalize_live_window(window: &ActiveWindow) -> Result<()> {
    if window.live_trade_emitted {
        return Ok(());
    }
    let window_end_et = (window.final_5m_start_et + ChronoDuration::minutes(5)).to_rfc3339();
    let skipped = crate::models::PaperTradeRecord {
        window_start_et: window.final_5m_start_et.to_rfc3339(),
        window_end_et,
        fifteen_minute_slug: window.fifteen_market.slug.clone(),
        five_minute_slug: window.five_market.slug.clone(),
        selected_pair: window.selected_pair.clone(),
        target_shares: read_env_f64("PAPER_TARGET_SHARES", 5.0),
        fill_shares: 0.0,
        min_edge_cents: read_env_f64("PAPER_MIN_EDGE_CENTS", 2.0),
        status: "skipped".to_string(),
        reason: "live_window_closed_without_entry".to_string(),
        first_snapshot_at: window.first_pair_snapshot_at,
        entry_snapshot_at: None,
        entry_net_cost: None,
        entry_gross_cost: None,
        expected_floor_payout: read_env_f64("PAPER_TARGET_SHARES", 5.0),
        realized_payout: None,
        realized_profit: None,
        fee_paid: None,
        qualified_edge: None,
    };
    append_jsonl("data/paper/live_paper_trades.ndjson", &skipped)?;
    write_json("data/paper/latest_live_paper_trade.json", &skipped)?;
    Ok(())
}
