# Codex Memory

## What This Repo Does
- Rust-only scanner/logger for the Polymarket BTC 5m/15m same-end-time bracket strategy.
- Live mode is log-only. It does not place trades.
- Historical mode backfills recent paired BTC windows and compares Polymarket anchors against Chainlink BTC/USD source data.

## Confirmed Data Paths
- Polymarket market metadata:
  - `https://gamma-api.polymarket.com/markets/slug/{slug}`
- Polymarket orderbooks:
  - `https://clob.polymarket.com/book?token_id={token_id}`
- Polymarket anchor extraction:
  - `https://polymarket.com/event/{slug}`
  - The page HTML embeds `eventMetadata.priceToBeat`
- Chainlink stream page:
  - `https://data.chain.link/streams/btc-usd`
- Chainlink recent live reports:
  - `https://data.chain.link/api/live-data-engine-stream-data?feedId=...&abiIndex=0&queryWindow=1m`
- Chainlink recent 1-minute candles:
  - `https://data.chain.link/api/historical-data-engine-stream-data?feedId=...&abiIndex=0&timeRange=1D`

## Output Files
- `data/chainlink/context.json`
- `data/chainlink/live_reports.ndjson`
- `data/chainlink/historical_1d_bars.json`
- `data/live/pair_scans.ndjson`
- `data/historical/pair_backfill.ndjson`
- `data/runs/latest_run.json`
- `RESEARCH_LOG.md`

## CLI
- `cargo run -- run-all --backfill-hours 24 --live-duration-seconds 90`
- `cargo run -- live --duration-seconds 300`
- `cargo run -- backfill --hours 24`

## Current Assumptions
- Backfill defaults to 24 hours because Chainlink exact 1-minute candle data is straightforward to compare over that window.
- Anchor extraction currently uses HTML parsing because a lightweight public Polymarket anchor endpoint was not confirmed.
- Live mode uses split anchor sourcing:
  - `A` from Polymarket 15-minute `priceToBeat`
  - `B` from the first Chainlink benchmark tick at or after the 5-minute open
- Exact live Polymarket-side `B` verification is still unresolved from public HTML alone.
  - Current event pages do not expose a clean current-window `priceToBeat` in static markup.
  - Do not trust any regex match that drifts into neighboring archived windows.
- Historical resolved orderbooks are not available through the public CLOB endpoint, so historical logging is metadata/anchor/source-focused rather than full-depth replay.

## Important Learnings
- The direct Chainlink endpoint to prioritize for speed is:
  - `https://data.chain.link/api/live-data-engine-stream-data?feedId=...&abiIndex=0&queryWindow=1m`
- The public Polymarket event page contains many neighboring recurring-window objects.
  - Naive `priceToBeat` regex extraction can silently return the wrong market's anchor.
- For live trading logic, it is safer to trust Chainlink directly for `B` than to wait for Polymarket's 5-minute anchor to become unambiguous through public HTML.
- Historical minute-bar comparisons are useful for sanity checks, but they are not tick-perfect reconstructions of the opening anchor.
