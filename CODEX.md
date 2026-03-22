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
  - Active windows: use the page `__NEXT_DATA__` payload and read the exact `crypto-prices` query `openPrice`
  - Closed windows: prefer Gamma `events[].eventMetadata.priceToBeat`
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
- `FINDINGS_2026-03-22.md`

## CLI
- `cargo run -- run-all --backfill-hours 24 --live-duration-seconds 90`
- `cargo run -- live --duration-seconds 300`
- `cargo run -- backfill --hours 24`

## Current Assumptions
- Backfill defaults to 24 hours because Chainlink exact 1-minute candle data is straightforward to compare over that window.
- Live mode uses split anchor sourcing:
  - `A` from Polymarket structured page data:
    - live windows from `__NEXT_DATA__` `crypto-prices` `openPrice`
    - closed windows from Gamma `eventMetadata.priceToBeat`
  - `B` from the first Chainlink benchmark tick at or after the 5-minute open
- Exact live Polymarket-side `B` verification is still unresolved from public HTML alone.
  - Do not trust raw regex matches across the event HTML blob.
- Historical resolved orderbooks are not available through the public CLOB endpoint, so historical logging is metadata/anchor/source-focused rather than full-depth replay.
- The CLOB `book` response is not returned best-first.
  - Normalize bids descending and asks ascending before taking top-of-book or walking executable depth.

## Important Learnings
- The direct Chainlink endpoint to prioritize for speed is:
  - `https://data.chain.link/api/live-data-engine-stream-data?feedId=...&abiIndex=0&queryWindow=1m`
- The public Polymarket event page contains many neighboring recurring-window objects.
  - Naive `priceToBeat` regex extraction can silently return the wrong market's anchor.
- The `__NEXT_DATA__` script tag on Polymarket includes extra attributes like `crossorigin`.
  - Any parser looking for an exact script tag string can silently miss the payload and fall back to bad logic.
- For live trading logic, it is safer to trust Chainlink directly for `B` than to wait for Polymarket's 5-minute anchor to become unambiguous through public HTML.
- Historical minute-bar comparisons are useful for sanity checks, but they are not tick-perfect reconstructions of the opening anchor.
- Verified on March 22, 2026:
  - live 5:10PM-5:15PM ET window used `A = 68168.33529096024` from Polymarket `__NEXT_DATA__`
  - that matched the page's exact `crypto-prices` query and was within about `0.49` of the Chainlink 5:00PM minute open proxy
  - after CLOB normalization, selected leg quotes became internally coherent and package cost dropped from the earlier fake `~1.98` range to about `1.11-1.21`
  - full patched live `5:25PM-5:30PM ET` window proved the strategy can become profitable:
    - executable snapshots: `118`
    - profitable snapshots: `29`
    - first profitable snapshot: `2026-03-22T17:28:46.531154800-04:00`
    - best observed edge: about `+0.30195`
    - best observed cost: about `0.69805`
    - profitable rows all occurred after the diagonal flipped to `U15+D5`
