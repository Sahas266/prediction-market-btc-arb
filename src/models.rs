use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

pub const SIZE_BUCKETS: [f64; 4] = [1.0, 5.0, 10.0, 25.0];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainlinkContext {
    pub stream_page_url: String,
    pub resolved_page_slug: String,
    pub feed_id: String,
    pub multiply: f64,
    pub stream_name: String,
    pub source_chain: Option<u64>,
    pub fetched_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunManifest {
    pub started_at: DateTime<Utc>,
    pub command: String,
    pub chainlink_feed_id: String,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainlinkLiveReport {
    pub valid_from: DateTime<Utc>,
    pub price: f64,
    pub bid: Option<f64>,
    pub ask: Option<f64>,
    #[serde(default)]
    pub received_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub source_latency_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainlinkMinuteBar {
    pub minute_start: DateTime<Utc>,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceBeat {
    pub slug: String,
    pub price_to_beat: f64,
    pub source: String,
    pub extracted_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeeSchedule {
    pub exponent: u32,
    pub rate: f64,
    #[serde(rename = "takerOnly")]
    pub taker_only: Option<bool>,
    #[serde(rename = "rebateRate")]
    pub rebate_rate: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GammaEventMetadata {
    #[serde(rename = "priceToBeat", default, deserialize_with = "crate::sources::de_opt_f64_from_any")]
    pub price_to_beat: Option<f64>,
    #[serde(rename = "finalPrice", default, deserialize_with = "crate::sources::de_opt_f64_from_any")]
    pub final_price: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GammaEvent {
    pub slug: Option<String>,
    #[serde(rename = "eventMetadata")]
    pub event_metadata: Option<GammaEventMetadata>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GammaMarket {
    pub id: String,
    pub question: String,
    pub slug: String,
    #[serde(rename = "conditionId")]
    pub condition_id: String,
    #[serde(rename = "resolutionSource")]
    pub resolution_source: Option<String>,
    #[serde(rename = "endDate")]
    pub end_date: String,
    pub description: Option<String>,
    #[serde(deserialize_with = "crate::sources::de_json_string_vec")]
    pub outcomes: Vec<String>,
    #[serde(rename = "outcomePrices", deserialize_with = "crate::sources::de_json_string_vec_f64")]
    pub outcome_prices: Vec<f64>,
    pub active: bool,
    pub closed: bool,
    #[serde(rename = "acceptingOrders")]
    pub accepting_orders: Option<bool>,
    #[serde(rename = "clobTokenIds", deserialize_with = "crate::sources::de_json_string_vec")]
    pub clob_token_ids: Vec<String>,
    #[serde(rename = "bestBid", default, deserialize_with = "crate::sources::de_opt_f64_from_any")]
    pub best_bid: Option<f64>,
    #[serde(rename = "bestAsk", default, deserialize_with = "crate::sources::de_opt_f64_from_any")]
    pub best_ask: Option<f64>,
    #[serde(rename = "feesEnabled")]
    pub fees_enabled: Option<bool>,
    #[serde(rename = "feeType")]
    pub fee_type: Option<String>,
    #[serde(rename = "feeSchedule")]
    pub fee_schedule: Option<FeeSchedule>,
    #[serde(rename = "lastTradePrice", default, deserialize_with = "crate::sources::de_opt_f64_from_any")]
    pub last_trade_price: Option<f64>,
    #[serde(default)]
    pub events: Vec<GammaEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderLevel {
    #[serde(deserialize_with = "crate::sources::de_f64_from_any")]
    pub price: f64,
    #[serde(deserialize_with = "crate::sources::de_f64_from_any")]
    pub size: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderBook {
    pub market: String,
    #[serde(rename = "asset_id")]
    pub asset_id: String,
    pub timestamp: String,
    pub bids: Vec<OrderLevel>,
    pub asks: Vec<OrderLevel>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawOrderbookEvent {
    pub received_at: DateTime<Utc>,
    pub event_type: String,
    pub asset_id: Option<String>,
    pub market: Option<String>,
    pub source_timestamp: Option<DateTime<Utc>>,
    pub source_latency_ms: Option<i64>,
    pub payload: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpOrderbookSnapshot {
    pub captured_at: DateTime<Utc>,
    pub token_id: String,
    pub reason: String,
    pub best_bid: Option<f64>,
    pub best_ask: Option<f64>,
    pub top_bids: Vec<OrderLevel>,
    pub top_asks: Vec<OrderLevel>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LegOrderbookSnapshot {
    pub outcome: String,
    pub token_id: String,
    pub best_bid: Option<f64>,
    pub best_ask: Option<f64>,
    pub top_bids: Vec<OrderLevel>,
    pub top_asks: Vec<OrderLevel>,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub source_timestamp: Option<DateTime<Utc>>,
    #[serde(default)]
    pub source_latency_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketSnapshot {
    pub slug: String,
    pub question: String,
    pub condition_id: String,
    pub start_anchor: f64,
    pub start_anchor_source: String,
    pub start_anchor_captured_at: DateTime<Utc>,
    pub outcomes: Vec<String>,
    pub outcome_prices: Vec<f64>,
    pub fees_enabled: bool,
    pub fee_schedule: Option<FeeSchedule>,
    pub best_bid: Option<f64>,
    pub best_ask: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PackageQuote {
    pub requested_shares: f64,
    pub fillable_shares: f64,
    pub executable: bool,
    pub gross_cost: Option<f64>,
    pub net_cost: Option<f64>,
    pub gross_edge: Option<f64>,
    pub net_edge: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LivePairScan {
    pub scanned_at: DateTime<Utc>,
    pub status: String,
    pub current_et: String,
    pub current_15m_start_et: String,
    pub current_final_5m_start_et: String,
    pub next_eligible_et: String,
    pub source_latest: Option<ChainlinkLiveReport>,
    pub pair: Option<LivePairDetail>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LivePairDetail {
    pub fifteen_minute: MarketSnapshot,
    pub five_minute: MarketSnapshot,
    pub selected_pair: String,
    pub floor_payout: f64,
    pub estimated_all_in_best_ask_cost: Option<f64>,
    pub estimated_edge_best_ask: Option<f64>,
    #[serde(default)]
    pub executable_costs: BTreeMap<String, f64>,
    #[serde(default)]
    pub package_quotes: BTreeMap<String, PackageQuote>,
    #[serde(default)]
    pub minimum_observed_fillable_shares: f64,
    #[serde(default)]
    pub signal_qualified: bool,
    #[serde(default)]
    pub signal_reason: String,
    #[serde(default)]
    pub selected_pair_changed: bool,
    pub leg_books: Vec<LegOrderbookSnapshot>,
    pub source_comparison: SourceComparison,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoricalPairRecord {
    pub scanned_at: DateTime<Utc>,
    pub fifteen_minute_slug: String,
    pub five_minute_slug: String,
    pub window_start_et: String,
    pub window_end_et: String,
    pub fifteen_anchor: f64,
    pub five_anchor: f64,
    pub selected_pair: String,
    pub fifteen_market: MarketSnapshot,
    pub five_market: MarketSnapshot,
    pub source_comparison: SourceComparison,
    pub resolved_outcome_fifteen: Option<String>,
    pub resolved_outcome_five: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SourceComparison {
    pub chainlink_open_15m_minute: Option<f64>,
    pub chainlink_open_5m_minute: Option<f64>,
    pub chainlink_end_minute: Option<f64>,
    pub delta_15m_anchor: Option<f64>,
    pub delta_5m_anchor: Option<f64>,
    pub polymarket_five_anchor: Option<f64>,
    pub chainlink_live_five_anchor: Option<f64>,
    pub delta_live_b_vs_polymarket_b: Option<f64>,
    pub live_b_matches_polymarket_b: Option<bool>,
}

#[derive(Debug, Clone)]
pub struct SelectedLegOwned {
    pub outcome: String,
    pub token_id: String,
    pub fee_schedule: Option<FeeSchedule>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaperTradeRecord {
    pub window_start_et: String,
    pub window_end_et: String,
    pub fifteen_minute_slug: String,
    pub five_minute_slug: String,
    pub selected_pair: String,
    pub target_shares: f64,
    pub fill_shares: f64,
    pub min_edge_cents: f64,
    pub status: String,
    pub reason: String,
    pub first_snapshot_at: Option<DateTime<Utc>>,
    pub entry_snapshot_at: Option<DateTime<Utc>>,
    pub entry_net_cost: Option<f64>,
    pub entry_gross_cost: Option<f64>,
    pub expected_floor_payout: f64,
    pub realized_payout: Option<f64>,
    pub realized_profit: Option<f64>,
    pub fee_paid: Option<f64>,
    pub qualified_edge: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowSummary {
    pub window_start_et: String,
    pub window_end_et: String,
    pub snapshot_count: usize,
    pub executable_snapshots: usize,
    pub qualifying_snapshots: usize,
    pub first_qualifying_at: Option<DateTime<Utc>>,
    pub last_qualifying_at: Option<DateTime<Utc>>,
    pub positive_duration_seconds: Option<i64>,
    pub best_edge: Option<f64>,
    pub best_cost: Option<f64>,
    pub selected_pair_changes: usize,
    pub max_fillable_shares: f64,
    pub fifteen_minute_slug: Option<String>,
    pub five_minute_slug: Option<String>,
    pub paper_trade_status: Option<String>,
}
