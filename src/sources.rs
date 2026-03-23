use crate::io::{append_jsonl, write_json};
use crate::models::{
    ChainlinkContext, ChainlinkLiveReport, ChainlinkMinuteBar, GammaMarket, HttpOrderbookSnapshot,
    OrderBook, OrderLevel, PriceBeat, RawOrderbookEvent,
};
use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Utc};
use futures::{SinkExt, StreamExt};
use regex::Regex;
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, USER_AGENT};
use serde::{Deserialize, Deserializer, de::Error as DeError};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, watch};
use tokio::task::JoinHandle;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

const CHAINLINK_STREAM_PAGE: &str = "https://data.chain.link/streams/btc-usd";
const CHAINLINK_1D_BARS_URL: &str =
    "https://data.chain.link/api/historical-data-engine-stream-data";
const CHAINLINK_LIVE_STREAM_URL: &str = "https://data.chain.link/api/live-data-engine-stream-data";
const POLYMARKET_GAMMA_BY_SLUG: &str = "https://gamma-api.polymarket.com/markets/slug";
const POLYMARKET_EVENT_PAGE: &str = "https://polymarket.com/event";
const POLYMARKET_ORDERBOOK: &str = "https://clob.polymarket.com/book";
const POLYMARKET_MARKET_WS_URL: &str = "wss://ws-subscriptions-clob.polymarket.com/ws/market";

#[derive(Debug, Default, Clone)]
pub struct ManagedOrderBook {
    pub book: Option<OrderBook>,
    pub source_timestamp: Option<DateTime<Utc>>,
    pub source_latency_ms: Option<i64>,
    pub source: String,
}

#[derive(Debug, Default)]
pub struct OrderbookStreamState {
    pub connected: bool,
    pub last_error: Option<String>,
    pub books: HashMap<String, ManagedOrderBook>,
}

pub struct OrderbookStreamHandle {
    pub state: Arc<Mutex<OrderbookStreamState>>,
    stop_tx: watch::Sender<bool>,
    join_handle: JoinHandle<()>,
}

impl OrderbookStreamHandle {
    pub async fn stop(self) -> Result<()> {
        let _ = self.stop_tx.send(true);
        let _ = self.join_handle.await;
        Ok(())
    }
}

pub fn build_client() -> Result<reqwest::Client> {
    let mut headers = HeaderMap::new();
    headers.insert(
        USER_AGENT,
        HeaderValue::from_static("prediction-market-btc-arb/0.2 (+rust scanner/paper-trader)"),
    );
    headers.insert(ACCEPT, HeaderValue::from_static("*/*"));

    Ok(reqwest::Client::builder()
        .default_headers(headers)
        .build()?)
}

pub async fn fetch_chainlink_context(client: &reqwest::Client) -> Result<ChainlinkContext> {
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

pub async fn fetch_chainlink_live_reports(
    client: &reqwest::Client,
    chainlink: &ChainlinkContext,
    query_window: &str,
) -> Result<Vec<ChainlinkLiveReport>> {
    let fetched_at = Utc::now();
    let response: Value = client
        .get(CHAINLINK_LIVE_STREAM_URL)
        .query(&[
            ("feedId", chainlink.feed_id.as_str()),
            ("abiIndex", "0"),
            ("queryWindow", query_window),
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
            received_at: Some(fetched_at),
            source_latency_ms: Some((fetched_at - ts).num_milliseconds()),
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

pub async fn fetch_chainlink_1d_bars(
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

pub async fn fetch_gamma_market(client: &reqwest::Client, slug: &str) -> Result<GammaMarket> {
    client
        .get(format!("{POLYMARKET_GAMMA_BY_SLUG}/{slug}"))
        .send()
        .await?
        .error_for_status()?
        .json::<GammaMarket>()
        .await
        .with_context(|| format!("failed to deserialize market {slug}"))
}

pub async fn fetch_price_to_beat(client: &reqwest::Client, market: &GammaMarket) -> Result<PriceBeat> {
    if let Some(price) = gamma_price_to_beat(market) {
        return Ok(PriceBeat {
            slug: market.slug.clone(),
            price_to_beat: price,
            source: "polymarket_gamma_event_metadata".to_string(),
            extracted_at: Utc::now(),
        });
    }

    let html = client
        .get(format!("{POLYMARKET_EVENT_PAGE}/{}", market.slug))
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;

    let price = match extract_next_data_payload(&html)
        .and_then(|payload| extract_open_price_from_next_data(&payload, market))
    {
        Ok(price) => price,
        Err(_) => {
            let scoped_pattern = format!(
                r#"(?s)"slug":"{}".*?"priceToBeat":([0-9.]+)"#,
                regex::escape(&market.slug)
            );
            let regex = Regex::new(&scoped_pattern)?;
            let captures = regex
                .captures(&html)
                .ok_or_else(|| anyhow!("priceToBeat not found for {}", market.slug))?;
            captures
                .get(1)
                .ok_or_else(|| anyhow!("missing price capture for {}", market.slug))?
                .as_str()
                .parse::<f64>()
                .with_context(|| format!("invalid priceToBeat for {}", market.slug))?
        }
    };

    Ok(PriceBeat {
        slug: market.slug.clone(),
        price_to_beat: price,
        source: "polymarket_next_data_open_price".to_string(),
        extracted_at: Utc::now(),
    })
}

pub async fn fetch_orderbook(client: &reqwest::Client, token_id: &str) -> Result<OrderBook> {
    let mut book = client
        .get(POLYMARKET_ORDERBOOK)
        .query(&[("token_id", token_id)])
        .send()
        .await?
        .error_for_status()?
        .json::<OrderBook>()
        .await
        .with_context(|| format!("failed to deserialize orderbook for token {token_id}"))?;
    normalize_orderbook(&mut book);
    Ok(book)
}

pub async fn log_http_orderbook_snapshot(
    client: &reqwest::Client,
    token_id: &str,
    reason: &str,
) -> Result<OrderBook> {
    let book = fetch_orderbook(client, token_id).await?;
    append_jsonl(
        "data/live/http_orderbook_snapshots.ndjson",
        &HttpOrderbookSnapshot {
            captured_at: Utc::now(),
            token_id: token_id.to_string(),
            reason: reason.to_string(),
            best_bid: book.bids.first().map(|level| level.price),
            best_ask: book.asks.first().map(|level| level.price),
            top_bids: book.bids.iter().take(8).cloned().collect(),
            top_asks: book.asks.iter().take(8).cloned().collect(),
        },
    )?;
    Ok(book)
}

pub async fn write_chainlink_context(path: &str, value: &ChainlinkContext) -> Result<()> {
    write_json(path, value)
}

pub fn normalize_orderbook(book: &mut OrderBook) {
    book.bids.sort_by(|a, b| b.price.total_cmp(&a.price));
    book.asks.sort_by(|a, b| a.price.total_cmp(&b.price));
}

pub async fn start_orderbook_stream(token_ids: Vec<String>) -> Result<OrderbookStreamHandle> {
    let state = Arc::new(Mutex::new(OrderbookStreamState::default()));
    let (stop_tx, mut stop_rx) = watch::channel(false);
    let state_clone = Arc::clone(&state);
    let reconnect_delay = Duration::from_millis(read_env_u64("WS_RECONNECT_MS", 1_000));

    let join_handle = tokio::spawn(async move {
        loop {
            if *stop_rx.borrow() {
                break;
            }

            let connect_result = connect_async(POLYMARKET_MARKET_WS_URL).await;
            let (mut socket, _) = match connect_result {
                Ok(parts) => parts,
                Err(error) => {
                    let mut guard = state_clone.lock().await;
                    guard.connected = false;
                    guard.last_error = Some(error.to_string());
                    drop(guard);
                    tokio::time::sleep(reconnect_delay).await;
                    continue;
                }
            };

            {
                let mut guard = state_clone.lock().await;
                guard.connected = true;
                guard.last_error = None;
            }

            let subscribe = serde_json::json!({
                "auth": serde_json::Value::Null,
                "assets_ids": token_ids,
                "type": "market"
            });

            if socket.send(Message::Text(subscribe.to_string())).await.is_err() {
                let mut guard = state_clone.lock().await;
                guard.connected = false;
                guard.last_error = Some("failed to subscribe to market websocket".to_string());
                drop(guard);
                tokio::time::sleep(reconnect_delay).await;
                continue;
            }

            let mut heartbeat = tokio::time::interval(Duration::from_secs(10));

            loop {
                tokio::select! {
                    _ = heartbeat.tick() => {
                        if socket.send(Message::Text("PING".to_string())).await.is_err() {
                            break;
                        }
                    }
                    changed = stop_rx.changed() => {
                        if changed.is_ok() && *stop_rx.borrow() {
                            let _ = socket.close(None).await;
                            return;
                        }
                    }
                    message = socket.next() => {
                        match message {
                            Some(Ok(msg)) => {
                                if handle_ws_message(msg, &state_clone).await.is_err() {
                                    break;
                                }
                            }
                            Some(Err(error)) => {
                                let mut guard = state_clone.lock().await;
                                guard.connected = false;
                                guard.last_error = Some(error.to_string());
                                break;
                            }
                            None => {
                                let mut guard = state_clone.lock().await;
                                guard.connected = false;
                                guard.last_error = Some("websocket stream closed".to_string());
                                break;
                            }
                        }
                    }
                }
            }

            tokio::time::sleep(reconnect_delay).await;
        }
    });

    Ok(OrderbookStreamHandle {
        state,
        stop_tx,
        join_handle,
    })
}

async fn handle_ws_message(
    message: Message,
    state: &Arc<Mutex<OrderbookStreamState>>,
) -> Result<()> {
    let received_at = Utc::now();
    let text = match message {
        Message::Text(text) => text,
        Message::Binary(bytes) => String::from_utf8(bytes.to_vec())?,
        Message::Ping(_) | Message::Pong(_) => return Ok(()),
        Message::Close(_) => return Err(anyhow!("websocket closed")),
        Message::Frame(_) => return Ok(()),
    };

    if text == "PONG" {
        return Ok(());
    }

    let parsed: Value = serde_json::from_str(&text)?;
    if let Some(items) = parsed.as_array() {
        for item in items {
            handle_ws_value(item.clone(), received_at, state).await?;
        }
    } else {
        handle_ws_value(parsed, received_at, state).await?;
    }

    Ok(())
}

async fn handle_ws_value(
    value: Value,
    received_at: DateTime<Utc>,
    state: &Arc<Mutex<OrderbookStreamState>>,
) -> Result<()> {
    let event_type = value["event_type"]
        .as_str()
        .or_else(|| value["type"].as_str())
        .unwrap_or("unknown")
        .to_string();
    let source_timestamp = value["timestamp"]
        .as_i64()
        .and_then(|ts| DateTime::<Utc>::from_timestamp_millis(ts));
    let latency = source_timestamp.map(|ts| (received_at - ts).num_milliseconds());
    let asset_id = value["asset_id"].as_str().map(ToOwned::to_owned);
    let market = value["market"].as_str().map(ToOwned::to_owned);

    append_jsonl(
        "data/live/orderbook_events.ndjson",
        &RawOrderbookEvent {
            received_at,
            event_type: event_type.clone(),
            asset_id: asset_id.clone(),
            market: market.clone(),
            source_timestamp,
            source_latency_ms: latency,
            payload: value.clone(),
        },
    )?;

    match event_type.as_str() {
        "book" => {
            let mut book: OrderBook = serde_json::from_value(value)?;
            normalize_orderbook(&mut book);
            let asset_id = book.asset_id.clone();
            let mut guard = state.lock().await;
            guard.books.insert(
                asset_id,
                ManagedOrderBook {
                    book: Some(book),
                    source_timestamp,
                    source_latency_ms: latency,
                    source: "polymarket_market_ws_book".to_string(),
                },
            );
        }
        "price_change" => {
            let timestamp = source_timestamp;
            let mut guard = state.lock().await;
            let Some(changes) = value["price_changes"].as_array() else {
                return Ok(());
            };
            for change in changes {
                let Some(asset_id) = change["asset_id"].as_str() else {
                    continue;
                };
                let Some(side) = change["side"].as_str() else {
                    continue;
                };
                let Some(price) = value_to_f64_opt(&change["price"]) else {
                    continue;
                };
                let Some(size) = value_to_f64_opt(&change["size"]) else {
                    continue;
                };

                let entry = guard.books.entry(asset_id.to_string()).or_insert_with(|| ManagedOrderBook {
                    book: Some(OrderBook {
                        market: value["market"].as_str().unwrap_or_default().to_string(),
                        asset_id: asset_id.to_string(),
                        timestamp: value["timestamp"].to_string(),
                        bids: Vec::new(),
                        asks: Vec::new(),
                    }),
                    source_timestamp: timestamp,
                    source_latency_ms: latency,
                    source: "polymarket_market_ws_price_change".to_string(),
                });
                if let Some(book) = entry.book.as_mut() {
                    apply_price_level(book, side, price, size);
                    book.timestamp = value["timestamp"].to_string();
                    normalize_orderbook(book);
                }
                entry.source_timestamp = timestamp;
                entry.source_latency_ms = latency;
                entry.source = "polymarket_market_ws_price_change".to_string();
            }
        }
        _ => {}
    }

    Ok(())
}

fn apply_price_level(book: &mut OrderBook, side: &str, price: f64, size: f64) {
    let levels = if side.eq_ignore_ascii_case("BUY") {
        &mut book.bids
    } else {
        &mut book.asks
    };

    if let Some(existing) = levels.iter_mut().find(|level| level.price == price) {
        if size <= 0.0 {
            levels.retain(|level| level.price != price);
        } else {
            existing.size = size;
        }
        return;
    }

    if size > 0.0 {
        levels.push(OrderLevel { price, size });
    }
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

fn gamma_price_to_beat(market: &GammaMarket) -> Option<f64> {
    market
        .events
        .iter()
        .find_map(|event| event.event_metadata.as_ref()?.price_to_beat)
}

fn extract_next_data_payload(html: &str) -> Result<Value> {
    let next_data_regex = Regex::new(r#"<script id="__NEXT_DATA__"[^>]*>(.*?)</script>"#)?;
    let captures = next_data_regex
        .captures(html)
        .ok_or_else(|| anyhow!("could not locate Polymarket __NEXT_DATA__ payload"))?;
    let next_json = captures
        .get(1)
        .ok_or_else(|| anyhow!("missing Polymarket JSON capture"))?
        .as_str();
    Ok(serde_json::from_str(next_json)?)
}

fn extract_open_price_from_next_data(payload: &Value, market: &GammaMarket) -> Result<f64> {
    let queries = payload["props"]["pageProps"]["dehydratedState"]["queries"]
        .as_array()
        .ok_or_else(|| anyhow!("missing Polymarket dehydratedState queries"))?;

    let expected_interval = crate::strategy::market_interval_label(&market.slug)?;
    let expected_start = crate::strategy::market_start_time_utc(&market.slug)?;
    let expected_end = DateTime::parse_from_rfc3339(&market.end_date)
        .with_context(|| format!("invalid endDate for {}", market.slug))?
        .with_timezone(&Utc);

    for query in queries {
        let Some(query_key) = query["queryKey"].as_array() else {
            continue;
        };
        if query_key.len() < 6
            || query_key[0].as_str() != Some("crypto-prices")
            || query_key[1].as_str() != Some("price")
            || query_key[2].as_str() != Some("BTC")
            || query_key[4].as_str() != Some(expected_interval)
        {
            continue;
        }

        let key_start = parse_value_datetime_utc(&query_key[3]);
        let key_end = parse_value_datetime_utc(&query_key[5]);
        if key_start != Some(expected_start) || key_end != Some(expected_end) {
            continue;
        }

        if let Some(price) = value_to_f64_opt(&query["state"]["data"]["openPrice"]) {
            return Ok(price);
        }
    }

    Err(anyhow!(
        "openPrice query not found in Polymarket page payload for {}",
        market.slug
    ))
}

fn parse_value_datetime_utc(value: &Value) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value.as_str()?)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

pub fn value_to_f64_opt(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_i64().map(|v| v as f64))
        .or_else(|| value.as_u64().map(|v| v as f64))
        .or_else(|| value.as_str()?.parse::<f64>().ok())
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

pub fn de_json_string_vec<'de, D>(deserializer: D) -> std::result::Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let raw = String::deserialize(deserializer)?;
    serde_json::from_str(&raw).map_err(D::Error::custom)
}

pub fn de_json_string_vec_f64<'de, D>(
    deserializer: D,
) -> std::result::Result<Vec<f64>, D::Error>
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

pub fn de_opt_f64_from_any<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<f64>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<Value>::deserialize(deserializer)?;
    value
        .map(value_to_f64)
        .transpose()
        .map_err(D::Error::custom)
}

pub fn de_f64_from_any<'de, D>(deserializer: D) -> std::result::Result<f64, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Value::deserialize(deserializer)?;
    value_to_f64(value).map_err(D::Error::custom)
}

fn read_env_u64(key: &str, default: u64) -> u64 {
    std::env::var(key)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn chainlink_context_parsing_finds_feed_metadata() {
        let html = include_str!("../tests/fixtures/chainlink_stream_page.html");
        let next_data_regex =
            Regex::new(r#"<script id="__NEXT_DATA__" type="application/json">(.*?)</script>"#)
                .expect("regex");
        let captures = next_data_regex.captures(html).expect("next data");
        let payload: Value = serde_json::from_str(captures.get(1).unwrap().as_str()).unwrap();
        let stream_data = &payload["props"]["pageProps"]["streamData"];
        assert_eq!(
            stream_data["streamMetadata"]["feedId"].as_str(),
            Some("fixture-feed-id")
        );
        assert_eq!(
            stream_data["extraConfig"]["slug"].as_str(),
            Some("btc-usd-fixture")
        );
    }

    #[test]
    fn polymarket_next_data_extracts_open_price() {
        let html = include_str!("../tests/fixtures/polymarket_event_page.html");
        let payload = extract_next_data_payload(html).expect("payload");
        let market = GammaMarket {
            id: "1".to_string(),
            question: "Fixture".to_string(),
            slug: "btc-updown-15m-1774214100".to_string(),
            condition_id: "condition".to_string(),
            resolution_source: None,
            end_date: "2026-03-22T21:30:00Z".to_string(),
            description: None,
            outcomes: vec!["Up".to_string(), "Down".to_string()],
            outcome_prices: vec![0.5, 0.5],
            active: true,
            closed: false,
            accepting_orders: Some(true),
            clob_token_ids: vec!["1".to_string(), "2".to_string()],
            best_bid: None,
            best_ask: None,
            fees_enabled: Some(true),
            fee_type: None,
            fee_schedule: None,
            last_trade_price: None,
            events: Vec::new(),
        };
        let price = extract_open_price_from_next_data(&payload, &market).expect("open price");
        assert!((price - 67765.25689130378).abs() < 1e-9);
    }

    #[test]
    fn normalize_orderbook_sorts_levels() {
        let mut book = OrderBook {
            market: "fixture".to_string(),
            asset_id: "1".to_string(),
            timestamp: "0".to_string(),
            bids: vec![
                OrderLevel { price: 0.4, size: 1.0 },
                OrderLevel { price: 0.6, size: 1.0 },
            ],
            asks: vec![
                OrderLevel { price: 0.7, size: 1.0 },
                OrderLevel { price: 0.5, size: 1.0 },
            ],
        };
        normalize_orderbook(&mut book);
        assert_eq!(book.bids[0].price, 0.6);
        assert_eq!(book.asks[0].price, 0.5);
    }
}
