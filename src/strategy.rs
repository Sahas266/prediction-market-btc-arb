use crate::models::{
    ChainlinkMinuteBar, FeeSchedule, GammaMarket, LegOrderbookSnapshot, LivePairDetail,
    MarketSnapshot, OrderLevel, PackageQuote, PaperTradeRecord, PriceBeat, SelectedLegOwned,
    SourceComparison, WindowSummary, SIZE_BUCKETS,
};
use anyhow::{anyhow, Result};
use chrono::{DateTime, Duration as ChronoDuration, NaiveDate, Timelike, Utc};
use chrono_tz::{America::New_York, Tz};
use std::collections::{BTreeMap, HashMap};

const ANCHOR_MATCH_TOLERANCE: f64 = 0.000_001;

pub fn bars_by_minute(bars: &[ChainlinkMinuteBar]) -> HashMap<i64, ChainlinkMinuteBar> {
    bars.iter()
        .cloned()
        .map(|bar| (bar.minute_start.timestamp(), bar))
        .collect()
}

pub fn floor_to_window(dt: chrono::DateTime<Tz>, window_minutes: u32) -> chrono::DateTime<Tz> {
    let floored_minute = (dt.minute() / window_minutes) * window_minutes;
    dt.with_minute(floored_minute)
        .and_then(|value| value.with_second(0))
        .and_then(|value| value.with_nanosecond(0))
        .expect("valid floored datetime")
}

pub fn market_interval_label(slug: &str) -> Result<&'static str> {
    if slug.contains("-15m-") {
        Ok("fifteen")
    } else if slug.contains("-5m-") {
        Ok("five")
    } else {
        Err(anyhow!("unsupported market interval for {slug}"))
    }
}

pub fn market_start_time_utc(slug: &str) -> Result<DateTime<Utc>> {
    let timestamp = slug
        .rsplit('-')
        .next()
        .ok_or_else(|| anyhow!("missing unix timestamp in {slug}"))?
        .parse::<i64>()?;
    DateTime::<Utc>::from_timestamp(timestamp, 0)
        .ok_or_else(|| anyhow!("invalid timestamp {timestamp} in {slug}"))
}

pub fn resolved_outcome(outcomes: &[String], prices: &[f64]) -> Option<String> {
    outcomes.iter().zip(prices.iter()).find_map(|(outcome, price)| {
        if (*price - 1.0).abs() < 1e-9 {
            Some(outcome.clone())
        } else {
            None
        }
    })
}

pub fn select_pair_label(fifteen_anchor: f64, five_anchor: f64) -> String {
    if fifteen_anchor < five_anchor {
        "U15+D5".to_string()
    } else if fifteen_anchor > five_anchor {
        "D15+U5".to_string()
    } else {
        "equal_skip".to_string()
    }
}

pub fn select_legs(
    selected_pair: &str,
    fifteen_market: &GammaMarket,
    five_market: &GammaMarket,
) -> Result<[SelectedLegOwned; 2]> {
    let fifteen_map = outcome_token_map(fifteen_market)?;
    let five_map = outcome_token_map(five_market)?;

    match selected_pair {
        "U15+D5" => Ok([
            SelectedLegOwned {
                outcome: "Up".to_string(),
                token_id: fifteen_map
                    .get("Up")
                    .cloned()
                    .ok_or_else(|| anyhow!("missing Up token for {}", fifteen_market.slug))?,
                fee_schedule: fifteen_market.fee_schedule.clone(),
            },
            SelectedLegOwned {
                outcome: "Down".to_string(),
                token_id: five_map
                    .get("Down")
                    .cloned()
                    .ok_or_else(|| anyhow!("missing Down token for {}", five_market.slug))?,
                fee_schedule: five_market.fee_schedule.clone(),
            },
        ]),
        "D15+U5" => Ok([
            SelectedLegOwned {
                outcome: "Down".to_string(),
                token_id: fifteen_map
                    .get("Down")
                    .cloned()
                    .ok_or_else(|| anyhow!("missing Down token for {}", fifteen_market.slug))?,
                fee_schedule: fifteen_market.fee_schedule.clone(),
            },
            SelectedLegOwned {
                outcome: "Up".to_string(),
                token_id: five_map
                    .get("Up")
                    .cloned()
                    .ok_or_else(|| anyhow!("missing Up token for {}", five_market.slug))?,
                fee_schedule: five_market.fee_schedule.clone(),
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

pub fn source_comparison(
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

pub fn taker_fee(qty: f64, price: f64, fee_schedule: Option<&FeeSchedule>) -> f64 {
    let Some(schedule) = fee_schedule else {
        return 0.0;
    };
    qty * price * schedule.rate * (price * (1.0 - price)).powi(schedule.exponent as i32)
}

pub fn executable_cost(
    qty: f64,
    asks: &[OrderLevel],
    fee_schedule: Option<&FeeSchedule>,
) -> Option<(f64, f64)> {
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
    let fee = taker_fee(qty, avg_price, fee_schedule);
    Some((gross, gross + fee))
}

pub fn fillable_shares(asks: &[OrderLevel]) -> f64 {
    asks.iter().map(|level| level.size).sum()
}

pub fn compute_package_quotes(
    leg_one: &LegOrderbookSnapshot,
    leg_two: &LegOrderbookSnapshot,
    leg_one_fee: Option<&FeeSchedule>,
    leg_two_fee: Option<&FeeSchedule>,
) -> (BTreeMap<String, PackageQuote>, BTreeMap<String, f64>) {
    let mut quotes = BTreeMap::new();
    let mut executable_costs = BTreeMap::new();

    for shares in SIZE_BUCKETS {
        let quote = if let (Some((gross_one, net_one)), Some((gross_two, net_two))) = (
            executable_cost(shares, &leg_one.top_asks, leg_one_fee),
            executable_cost(shares, &leg_two.top_asks, leg_two_fee),
        ) {
            let gross_cost = gross_one + gross_two;
            let net_cost = net_one + net_two;
            executable_costs.insert(format!("{shares:.0}"), net_cost);
            PackageQuote {
                requested_shares: shares,
                fillable_shares: shares,
                executable: true,
                gross_cost: Some(gross_cost),
                net_cost: Some(net_cost),
                gross_edge: Some(shares - gross_cost),
                net_edge: Some(shares - net_cost),
            }
        } else {
            PackageQuote {
                requested_shares: shares,
                fillable_shares: fillable_shares(&leg_one.top_asks).min(fillable_shares(&leg_two.top_asks)),
                executable: false,
                gross_cost: None,
                net_cost: None,
                gross_edge: None,
                net_edge: None,
            }
        };
        quotes.insert(format!("{shares:.0}"), quote);
    }

    (quotes, executable_costs)
}

pub fn build_live_pair_detail(
    fifteen_market: &GammaMarket,
    fifteen_anchor: &PriceBeat,
    five_market: &GammaMarket,
    five_anchor: &PriceBeat,
    bars: &HashMap<i64, ChainlinkMinuteBar>,
    leg_books: Vec<LegOrderbookSnapshot>,
    previous_selected_pair: Option<&str>,
) -> Result<LivePairDetail> {
    let selected_pair = select_pair_label(fifteen_anchor.price_to_beat, five_anchor.price_to_beat);
    let mut package_quotes = BTreeMap::new();
    let mut executable_costs = BTreeMap::new();
    let mut all_in_best_ask = None;
    let mut edge = None;
    let mut minimum_observed_fillable = 0.0;
    let mut signal_qualified = false;
    let mut signal_reason = "equal_skip".to_string();

    if selected_pair != "equal_skip" && leg_books.len() == 2 {
        let selected = select_legs(&selected_pair, fifteen_market, five_market)?;
        let (quotes, costs) = compute_package_quotes(
            &leg_books[0],
            &leg_books[1],
            selected[0].fee_schedule.as_ref(),
            selected[1].fee_schedule.as_ref(),
        );
        minimum_observed_fillable = quotes
            .values()
            .map(|quote| quote.fillable_shares)
            .fold(f64::INFINITY, f64::min);
        if !minimum_observed_fillable.is_finite() {
            minimum_observed_fillable = 0.0;
        }
        if let Some(quote) = quotes.get("1").filter(|quote| quote.executable) {
            all_in_best_ask = quote.net_cost;
            edge = quote.net_edge;
        }
        if let Some(quote) = quotes.get("5") {
            signal_qualified = quote
                .net_edge
                .map(|net_edge| net_edge / 5.0 >= 0.02)
                .unwrap_or(false);
            signal_reason = if quote.executable {
                if signal_qualified {
                    "qualified_target_bucket".to_string()
                } else {
                    "edge_below_threshold".to_string()
                }
            } else {
                "insufficient_depth".to_string()
            };
        }
        package_quotes = quotes;
        executable_costs = costs;
    }

    let fifteen_start = market_start_time_utc(&fifteen_market.slug)?;
    let five_start = market_start_time_utc(&five_market.slug)?;
    let five_end = DateTime::parse_from_rfc3339(&fifteen_market.end_date)?.with_timezone(&Utc);

    let source_comparison = source_comparison(
        fifteen_start,
        five_start,
        five_end,
        fifteen_anchor.price_to_beat,
        five_anchor.price_to_beat,
        None,
        five_anchor
            .source
            .contains("chainlink")
            .then_some(five_anchor.price_to_beat),
        bars,
    );

    Ok(LivePairDetail {
        fifteen_minute: MarketSnapshot {
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
        },
        five_minute: MarketSnapshot {
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
        },
        selected_pair: selected_pair.clone(),
        floor_payout: 1.0,
        estimated_all_in_best_ask_cost: all_in_best_ask,
        estimated_edge_best_ask: edge,
        executable_costs,
        package_quotes,
        minimum_observed_fillable_shares: minimum_observed_fillable,
        signal_qualified,
        signal_reason,
        selected_pair_changed: previous_selected_pair
            .map(|previous| previous != selected_pair)
            .unwrap_or(false),
        leg_books,
        source_comparison,
    })
}

pub fn summarize_windows(
    scans: &[crate::models::LivePairScan],
    paper_trades: &HashMap<String, PaperTradeRecord>,
) -> Vec<WindowSummary> {
    let mut grouped: BTreeMap<String, Vec<&crate::models::LivePairScan>> = BTreeMap::new();
    for scan in scans {
        grouped
            .entry(scan.current_final_5m_start_et.clone())
            .or_default()
            .push(scan);
    }

    grouped
        .into_iter()
        .map(|(window_start, rows)| summarize_window(&window_start, &rows, paper_trades.get(&window_start)))
        .collect()
}

fn summarize_window(
    window_start: &str,
    rows: &[&crate::models::LivePairScan],
    paper_trade: Option<&PaperTradeRecord>,
) -> WindowSummary {
    let mut executable = 0usize;
    let mut qualifying = 0usize;
    let mut first_q = None;
    let mut last_q = None;
    let mut best_edge = None;
    let mut best_cost = None;
    let mut max_fillable: f64 = 0.0;
    let mut selected_pair_changes = 0usize;
    let mut previous_pair: Option<&str> = None;
    let mut fifteen_slug = None;
    let mut five_slug = None;

    for row in rows {
        if let Some(pair) = &row.pair {
            fifteen_slug = Some(pair.fifteen_minute.slug.clone());
            five_slug = Some(pair.five_minute.slug.clone());
            max_fillable = max_fillable.max(legacy_fillable_shares(pair));

            if pair.estimated_all_in_best_ask_cost.is_some() {
                executable += 1;
            }

            if pair.signal_qualified || legacy_signal(pair, 2.0, 5.0) {
                qualifying += 1;
                first_q.get_or_insert(row.scanned_at);
                last_q = Some(row.scanned_at);
            }

            if let Some(edge) = pair.estimated_edge_best_ask {
                best_edge = Some(best_edge.map_or(edge, |current: f64| current.max(edge)));
            }
            if let Some(cost) = pair.estimated_all_in_best_ask_cost {
                best_cost = Some(best_cost.map_or(cost, |current: f64| current.min(cost)));
            }

            if let Some(previous) = previous_pair {
                if previous != pair.selected_pair {
                    selected_pair_changes += 1;
                }
            }
            previous_pair = Some(&pair.selected_pair);
        }
    }

    let positive_duration_seconds = first_q
        .zip(last_q)
        .map(|(first, last)| (last - first).num_seconds());
    let window_start_dt = DateTime::parse_from_rfc3339(window_start)
        .ok()
        .map(|dt| dt.with_timezone(&New_York));
    let window_end = window_start_dt
        .map(|dt| (dt + ChronoDuration::minutes(5)).to_rfc3339())
        .unwrap_or_default();

    WindowSummary {
        window_start_et: window_start.to_string(),
        window_end_et: window_end,
        snapshot_count: rows.len(),
        executable_snapshots: executable,
        qualifying_snapshots: qualifying,
        first_qualifying_at: first_q,
        last_qualifying_at: last_q,
        positive_duration_seconds,
        best_edge,
        best_cost,
        selected_pair_changes,
        max_fillable_shares: max_fillable,
        fifteen_minute_slug: fifteen_slug,
        five_minute_slug: five_slug,
        paper_trade_status: paper_trade.map(|trade| trade.status.clone()),
    }
}

pub fn build_paper_trades(
    scans: &[crate::models::LivePairScan],
    min_edge_cents: f64,
    target_shares: f64,
) -> Vec<PaperTradeRecord> {
    let mut grouped: BTreeMap<String, Vec<&crate::models::LivePairScan>> = BTreeMap::new();
    for scan in scans {
        grouped
            .entry(scan.current_final_5m_start_et.clone())
            .or_default()
            .push(scan);
    }

    grouped
        .into_iter()
        .filter_map(|(window_start, mut rows)| {
            rows.sort_by_key(|row| row.scanned_at);
            build_paper_trade_for_window(&window_start, &rows, min_edge_cents, target_shares)
        })
        .collect()
}

pub fn build_live_paper_trade(
    scan: &crate::models::LivePairScan,
    min_edge_cents: f64,
    target_shares: f64,
    first_snapshot_at: Option<DateTime<Utc>>,
) -> Option<PaperTradeRecord> {
    let pair = scan.pair.as_ref()?;
    let target_key = format!("{target_shares:.0}");
    let (requested_shares, net_cost, gross_cost, executable) =
        qualify_snapshot_quote(pair, &target_key, target_shares)?;
    let net_edge = requested_shares - net_cost;
    let edge_per_share = net_edge / requested_shares.max(1.0);
    if !executable || edge_per_share * 100.0 < min_edge_cents {
        return None;
    }

    let window_start_dt = DateTime::parse_from_rfc3339(&scan.current_final_5m_start_et)
        .ok()
        .map(|dt| dt.with_timezone(&New_York));
    let window_end = window_start_dt
        .map(|dt| (dt + ChronoDuration::minutes(5)).to_rfc3339())
        .unwrap_or_default();
    let fee_paid = gross_cost.map(|gross| net_cost - gross);

    Some(PaperTradeRecord {
        window_start_et: scan.current_final_5m_start_et.clone(),
        window_end_et: window_end,
        fifteen_minute_slug: pair.fifteen_minute.slug.clone(),
        five_minute_slug: pair.five_minute.slug.clone(),
        selected_pair: pair.selected_pair.clone(),
        target_shares,
        fill_shares: target_shares.min(requested_shares),
        min_edge_cents,
        status: "entered".to_string(),
        reason: "live_first_qualifying_snapshot".to_string(),
        first_snapshot_at,
        entry_snapshot_at: Some(scan.scanned_at),
        entry_net_cost: Some(net_cost),
        entry_gross_cost: gross_cost,
        expected_floor_payout: target_shares,
        realized_payout: None,
        realized_profit: None,
        fee_paid,
        qualified_edge: Some(net_edge),
    })
}

fn build_paper_trade_for_window(
    window_start: &str,
    rows: &[&crate::models::LivePairScan],
    min_edge_cents: f64,
    target_shares: f64,
) -> Option<PaperTradeRecord> {
    let first_pair = rows.iter().find_map(|row| row.pair.as_ref())?;
    let window_start_dt = DateTime::parse_from_rfc3339(window_start)
        .ok()
        .map(|dt| dt.with_timezone(&New_York));
    let window_end = window_start_dt
        .map(|dt| (dt + ChronoDuration::minutes(5)).to_rfc3339())
        .unwrap_or_default();

    let target_key = format!("{target_shares:.0}");

    for row in rows {
        let Some(pair) = &row.pair else {
            continue;
        };
        let Some((requested_shares, net_cost, gross_cost, executable)) =
            qualify_snapshot_quote(pair, &target_key, target_shares)
        else {
            continue;
        };
        let net_edge = requested_shares - net_cost;
        let edge_per_share = net_edge / requested_shares.max(1.0);
        if executable && edge_per_share * 100.0 >= min_edge_cents {
            let fee_paid = gross_cost.map(|gross| net_cost - gross);
            return Some(PaperTradeRecord {
                window_start_et: window_start.to_string(),
                window_end_et: window_end,
                fifteen_minute_slug: pair.fifteen_minute.slug.clone(),
                five_minute_slug: pair.five_minute.slug.clone(),
                selected_pair: pair.selected_pair.clone(),
                target_shares,
                fill_shares: target_shares.min(requested_shares),
                min_edge_cents,
                status: "entered".to_string(),
                reason: "first_qualifying_snapshot".to_string(),
                first_snapshot_at: rows.first().map(|item| item.scanned_at),
                entry_snapshot_at: Some(row.scanned_at),
                entry_net_cost: Some(net_cost),
                entry_gross_cost: gross_cost,
                expected_floor_payout: target_shares,
                realized_payout: None,
                realized_profit: None,
                fee_paid,
                qualified_edge: Some(net_edge),
            });
        }
    }

    Some(PaperTradeRecord {
        window_start_et: window_start.to_string(),
        window_end_et: window_end,
        fifteen_minute_slug: first_pair.fifteen_minute.slug.clone(),
        five_minute_slug: first_pair.five_minute.slug.clone(),
        selected_pair: first_pair.selected_pair.clone(),
        target_shares,
        fill_shares: 0.0,
        min_edge_cents,
        status: "skipped".to_string(),
        reason: "no_qualifying_snapshot".to_string(),
        first_snapshot_at: rows.first().map(|item| item.scanned_at),
        entry_snapshot_at: None,
        entry_net_cost: None,
        entry_gross_cost: None,
        expected_floor_payout: target_shares,
        realized_payout: None,
        realized_profit: None,
        fee_paid: None,
        qualified_edge: None,
    })
}

fn qualify_snapshot_quote(
    pair: &LivePairDetail,
    target_key: &str,
    target_shares: f64,
) -> Option<(f64, f64, Option<f64>, bool)> {
    if let Some(quote) = pair.package_quotes.get(target_key) {
        return Some((
            quote.requested_shares,
            quote.net_cost?,
            quote.gross_cost,
            quote.executable,
        ));
    }

    let mut fallback: Vec<(f64, f64)> = pair
        .executable_costs
        .iter()
        .filter_map(|(shares, cost)| shares.parse::<f64>().ok().map(|shares| (shares, *cost)))
        .collect();
    fallback.sort_by(|a, b| a.0.total_cmp(&b.0));
    let (available_shares, available_net_cost) = fallback
        .into_iter()
        .find(|(shares, _)| *shares >= target_shares)
        .or_else(|| {
            pair.estimated_all_in_best_ask_cost
                .map(|cost| (1.0, cost))
        })?;
    let scale = if available_shares > 0.0 {
        target_shares.min(available_shares) / available_shares
    } else {
        1.0
    };
    let requested_shares = target_shares.min(available_shares.max(1.0));
    let net_cost = available_net_cost * scale;

    Some((requested_shares, net_cost, None, true))
}

fn legacy_signal(pair: &LivePairDetail, min_edge_cents: f64, target_shares: f64) -> bool {
    let target_key = format!("{target_shares:.0}");
    qualify_snapshot_quote(pair, &target_key, target_shares)
        .map(|(requested_shares, net_cost, _, executable)| {
            executable && ((requested_shares - net_cost) / requested_shares.max(1.0)) * 100.0 >= min_edge_cents
        })
        .unwrap_or(false)
}

fn legacy_fillable_shares(pair: &LivePairDetail) -> f64 {
    if pair.minimum_observed_fillable_shares > 0.0 {
        return pair.minimum_observed_fillable_shares;
    }
    pair.package_quotes
        .values()
        .map(|quote| quote.fillable_shares)
        .fold(0.0, f64::max)
        .max(
            pair.executable_costs
                .keys()
                .filter_map(|shares| shares.parse::<f64>().ok())
                .fold(0.0, f64::max),
        )
}

pub fn reconcile_trade(
    trade: &mut PaperTradeRecord,
    fifteen_market: &GammaMarket,
    five_market: &GammaMarket,
) {
    if trade.status == "skipped" {
        return;
    }

    let outcome_fifteen = resolved_outcome(&fifteen_market.outcomes, &fifteen_market.outcome_prices);
    let outcome_five = resolved_outcome(&five_market.outcomes, &five_market.outcome_prices);
    if outcome_fifteen.is_none() || outcome_five.is_none() {
        return;
    }

    let payout = match (trade.selected_pair.as_str(), outcome_fifteen.as_deref(), outcome_five.as_deref()) {
        ("U15+D5", Some("Up"), Some("Down")) => Some(trade.fill_shares * 2.0),
        ("U15+D5", Some("Up"), _) | ("U15+D5", _, Some("Down")) => Some(trade.fill_shares),
        ("D15+U5", Some("Down"), Some("Up")) => Some(trade.fill_shares * 2.0),
        ("D15+U5", Some("Down"), _) | ("D15+U5", _, Some("Up")) => Some(trade.fill_shares),
        ("equal_skip", _, _) => None,
        _ => Some(0.0),
    };

    trade.realized_payout = payout;
    trade.realized_profit = trade
        .entry_net_cost
        .zip(payout)
        .map(|(entry_cost, payout)| payout - entry_cost);
    trade.status = match payout {
        Some(value) if (value - trade.fill_shares * 2.0).abs() < 1e-9 => "settled_win_2".to_string(),
        Some(value) if (value - trade.fill_shares).abs() < 1e-9 => "settled_win_1".to_string(),
        Some(_) => "settled_loss_unexpected_data_issue".to_string(),
        None => "settled_loss_unexpected_data_issue".to_string(),
    };
}

pub fn parse_date_filter(input: &Option<String>, end_of_day: bool) -> Result<Option<DateTime<Utc>>> {
    let Some(text) = input else {
        return Ok(None);
    };
    if let Ok(date) = NaiveDate::parse_from_str(text, "%Y-%m-%d") {
        let naive = if end_of_day {
            date.and_hms_opt(23, 59, 59).ok_or_else(|| anyhow!("invalid end-of-day date"))?
        } else {
            date.and_hms_opt(0, 0, 0).ok_or_else(|| anyhow!("invalid start-of-day date"))?
        };
        return Ok(Some(DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc)));
    }

    Ok(Some(
        DateTime::parse_from_rfc3339(text)
            .map_err(|error| anyhow!("invalid datetime filter {text}: {error}"))?
            .with_timezone(&Utc),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::LegOrderbookSnapshot;

    #[test]
    fn taker_fee_positive_when_schedule_present() {
        let fee = taker_fee(
            1.0,
            0.5,
            Some(&FeeSchedule {
                exponent: 2,
                rate: 0.25,
                taker_only: Some(true),
                rebate_rate: Some(0.2),
            }),
        );
        assert!(fee > 0.0);
    }

    #[test]
    fn executable_cost_walks_depth() {
        let asks = vec![
            OrderLevel { price: 0.4, size: 2.0 },
            OrderLevel { price: 0.5, size: 3.0 },
        ];
        let (_, net) = executable_cost(5.0, &asks, None).expect("depth available");
        assert!((net - 2.3).abs() < 1e-9);
    }

    #[test]
    fn package_quotes_mark_unfilled_when_depth_missing() {
        let leg_one = LegOrderbookSnapshot {
            outcome: "Up".to_string(),
            token_id: "1".to_string(),
            best_bid: Some(0.4),
            best_ask: Some(0.5),
            top_bids: vec![],
            top_asks: vec![OrderLevel { price: 0.5, size: 1.0 }],
            source: String::new(),
            source_timestamp: None,
            source_latency_ms: None,
        };
        let leg_two = LegOrderbookSnapshot {
            outcome: "Down".to_string(),
            token_id: "2".to_string(),
            best_bid: Some(0.4),
            best_ask: Some(0.5),
            top_bids: vec![],
            top_asks: vec![OrderLevel { price: 0.5, size: 1.0 }],
            source: String::new(),
            source_timestamp: None,
            source_latency_ms: None,
        };
        let (quotes, _) = compute_package_quotes(&leg_one, &leg_two, None, None);
        assert!(!quotes["5"].executable);
    }
}
