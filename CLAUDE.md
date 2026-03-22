# CLAUDE.md

This file provides guidance to Claude Code and other agents working in this repository.

## Build And Run

```bash
# Build
cargo build

# Run all (backfill + live scanner)
cargo run -- run-all --backfill-hours 24 --live-duration-seconds 90 --poll-interval-ms 1000

# Backfill only
cargo run -- backfill --hours 24

# Live scanner only
cargo run -- live --duration-seconds 300 --poll-interval-ms 1000

# There are no tests yet
cargo test
```

Requires `.env` with:
- `OPENROUTER_API_KEY`
- `POLYMARKET_API_KEY`
- `POLYMARKET_API_ADDRESS`
- `POLYGON_PRIVATE_KEY`
- `POLYGON_ADDRESS`

## Architecture

Single-file Rust binary in [src/main.rs](c:/Users/sahas/Github%20Repos/prediction-market-btc-arb/src/main.rs). The project is still log-only and does not place trades.

CLI subcommands:
- `run-all`
- `backfill`
- `live`

### Data Flow

1. Chainlink context:
   - scrape `https://data.chain.link/streams/btc-usd` to get the feed ID
   - fetch 1-day minute bars
   - fetch live benchmark/bid/ask ticks
2. Polymarket market discovery:
   - fetch market metadata from Gamma by slug
3. Anchor extraction:
   - live windows: use Polymarket page `__NEXT_DATA__` and read the exact `crypto-prices` query `openPrice`
   - closed windows: use Gamma `events[].eventMetadata.priceToBeat`
4. Pair analysis:
   - match a 15m market with the final 5m market sharing the same end time
   - choose the correct diagonal from anchor ordering
   - normalize CLOB books before computing best quotes and executable costs
5. Output:
   - write NDJSON and JSON artifacts under `data/`

### Key Functions

- `scan_live_once()`:
  main per-tick timing logic
- `load_pair_detail()`:
  fetches both markets, anchors, and orderbooks and builds the pair snapshot
- `select_legs()`:
  implements the anchor-ordering rule
- `fetch_price_to_beat()`:
  now uses structured Polymarket sources instead of raw regex-only HTML scraping
- `fetch_orderbook()`:
  normalizes bids descending and asks ascending
- `executable_cost()`:
  walks the ask ladder and includes taker fees

## External APIs

- Chainlink stream page:
  `https://data.chain.link/streams/btc-usd`
- Chainlink live reports:
  `https://data.chain.link/api/live-data-engine-stream-data`
- Chainlink 1-minute bars:
  `https://data.chain.link/api/historical-data-engine-stream-data`
- Polymarket Gamma by slug:
  `https://gamma-api.polymarket.com/markets/slug/{slug}`
- Polymarket CLOB book:
  `https://clob.polymarket.com/book?token_id={id}`
- Polymarket event page:
  `https://polymarket.com/event/{slug}`

## Strategy Overview

The strategy pairs:
- one BTC 15-minute Up/Down market
- the final BTC 5-minute Up/Down market with the same end time

If `A < B`, the correct diagonal is `U15 + D5`.

If `A > B`, the correct diagonal is `D15 + U5`.

The correct diagonal has a deterministic `$1` floor payoff and a `$2` overlap zone.

## Current Status

Current state:
- working Rust scanner/logger
- live and historical logging both functional
- no execution bot yet

What is fixed:
- bad recurring-window anchor extraction from raw Polymarket HTML
- bad CLOB top-of-book interpretation from unsorted arrays

What is still missing:
- websocket-based low-latency ingestion
- live Polymarket-side `B` verification from a slug-stable public endpoint
- execution, inventory handling, and settlement automation

## March 22, 2026 Findings

### Fixed Measurement Bugs

- Earlier negative readings were partly false.
- Root causes:
  - raw HTML regex could bind the wrong recurring market anchor
  - CLOB books are not returned best-first, so naive `.asks.first()` was wrong

### Patched Live Results

- `5:10PM-5:15PM ET` window:
  - captured executable snapshots were all negative
  - best observed edge was about `-0.1093`

- `5:25PM-5:30PM ET` window:
  - executable snapshots: `118`
  - profitable snapshots: `29`
  - first profitable snapshot:
    `2026-03-22T17:28:46.531154800-04:00`
  - best observed edge:
    about `+0.30195`
  - best observed package cost:
    about `0.69805`

### Interpretation

- The strategy is not continuously available.
- Some windows are fully unprofitable.
- Some windows become profitable late in the final 5-minute period.
- The scanner is now good enough to prove that profitable live moments can appear.

## Recommended Next Steps

1. Add websocket logging for Polymarket books and, if possible, market updates.
2. Capture more live windows and characterize:
   - frequency of profitable windows
   - time-within-window that profit first appears
   - available size at positive edge
3. Only build execution after confirming the profitable windows survive realistic fill assumptions.
