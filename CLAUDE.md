# CLAUDE.md

This file provides guidance to Claude Code when working with code in this repository.

## Build and run

```bash
cargo build
cargo test

# Live scanner (log-only, no execution)
cargo run -- live --duration-seconds 300 --poll-interval-ms 1000

# Historical backfill
cargo run -- backfill --hours 24

# Both backfill + live
cargo run -- run-all --backfill-hours 24 --live-duration-seconds 90 --poll-interval-ms 1000

# Replay captured data
cargo run -- replay --input data/live/pair_scans.ndjson

# Per-window summary
cargo run -- summarize --input data/live/pair_scans.ndjson

# Paper trade over captured data
cargo run -- paper-trade --input data/live/pair_scans.ndjson --min-edge-cents 2 --target-shares 5
```

Helper script:

```powershell
./scripts/run_live.ps1
./scripts/run_live.ps1 -DurationSeconds 600 -PollIntervalMs 500
```

Requires `.env` with: `OPENROUTER_API_KEY`, `POLYMARKET_API_KEY`, `POLYMARKET_API_ADDRESS`, `POLYGON_PRIVATE_KEY`, `POLYGON_ADDRESS`.

See `.env.example` for non-secret runtime knobs:

- `PAPER_MIN_EDGE_CENTS`
- `PAPER_TARGET_SHARES`
- `LIVE_ACTIVE_POLL_MS`
- `WS_SANITY_SNAPSHOT_SECONDS`
- `WS_RECONNECT_MS`

## Architecture

Rust library plus thin CLI binary. No live execution; log-only scanner with replay and paper-trading.

```text
src/main.rs     - CLI entry point, parses args, calls lib::run()
src/cli.rs      - clap subcommand definitions
src/lib.rs      - re-exports modules, top-level run()
src/app.rs      - orchestrates commands and the stateful live loop
src/sources.rs  - Chainlink, Gamma API, HTTP orderbook, Polymarket websocket integrations
src/strategy.rs - package pricing, diagonal selection, replay, summaries, paper-trade logic
src/models.rs   - persisted data schemas (serde structs)
src/io.rs       - file I/O helpers (NDJSON append, JSON write)
```

### Data flow

1. Chainlink context: scrape `data.chain.link/streams/btc-usd` for feed ID, fetch 1-day minute bars and live benchmark ticks.
2. Polymarket market discovery: fetch `GammaMarket` by slug pattern (`btc-updown-15m-{unix}`, `btc-updown-5m-{unix}`).
3. Anchor extraction: live windows use Polymarket page `__NEXT_DATA__` `crypto-prices` `openPrice`; closed windows use Gamma `events[].eventMetadata.priceToBeat`.
4. Live `B`: first Chainlink benchmark tick at or after the 5-minute open.
5. Orderbook: websocket stream during active final-5m window, HTTP fallback for periodic sanity snapshots. Books are normalized with bids descending and asks ascending.
6. Pair analysis: `select_legs()` picks the correct diagonal from anchor ordering. `executable_cost()` walks the ask ladder and includes taker fees.
7. Output: NDJSON and JSON artifacts under `data/`.

### Live loop behavior

- Outside the final 5-minute window: follow base Chainlink polling cadence.
- Inside the active final 5-minute window: re-evaluate at tighter cadence, default `10ms` via `LIVE_ACTIVE_POLL_MS`, using websocket book updates plus fallback HTTP snapshots.
- During active live runs: emit a paper-trade record immediately on the first qualifying snapshot, or a `skipped` record when the window closes without an entry.

## Strategy rules

The strategy pairs a BTC 15-minute Up/Down market with the final BTC 5-minute Up/Down market sharing the same end time.

- Anchor ordering is essential. The wrong diagonal has a $0 payoff zone.
- `A < B`: buy `U15 + D5`
- `A > B`: buy `D15 + U5`
- `A = B`: skip
- Correct diagonal guarantees $1 floor payout, $2 in the overlap zone.
- Fee formula used by the current implementation is derived from the market fee schedule.
- Resolution source is Chainlink BTC/USD stream data.

## Critical implementation caveats

- CLOB books are not sorted best-first. Always normalize bids descending and asks ascending before reading top-of-book or walking depth.
- Polymarket event HTML contains many neighboring recurring-window objects. Naive regex over the HTML blob can silently bind the wrong market's anchor.
- The `__NEXT_DATA__` script tag can include extra attributes such as `crossorigin`. Exact tag matching can miss it.
- Historical minute-bar comparisons are proxies, not tick-perfect truth.

## Key output files

- `data/live/pair_scans.ndjson` - derived live pair snapshots
- `data/live/chainlink_ticks.ndjson` - raw Chainlink ticks with receive timestamps
- `data/live/orderbook_events.ndjson` - raw Polymarket websocket events
- `data/live/http_orderbook_snapshots.ndjson` - periodic HTTP orderbook captures
- `data/historical/pair_backfill.ndjson` - historical paired-window comparisons
- `data/paper/latest_paper_trades.json` - latest paper-trade ledger
- `data/paper/live_paper_trades.ndjson` - live paper-trade decisions during runs
- `data/paper/latest_live_paper_trade.json` - most recent live paper-trade decision
- `data/analysis/latest_window_summaries.json` - per-window summary report
- `data/replay/latest_replay.json` - latest replay output

## Current status

Working:

- live scanner with websocket orderbook capture
- historical backfill
- replay
- per-window summaries
- paper trading over captured data
- live paper-trade emission during runs

Not implemented:

- live execution
- settlement automation
- dynamic sizing beyond the fixed 5-share baseline

Latest observed full-window negative result on March 22, 2026 `8:10PM-8:15PM ET`:

- `5584` total evaluations
- `1766` executable snapshots
- `0` qualifying snapshots
- best observed edge about `-0.02801`
- best observed package cost about `1.02801`
- live paper-trade result `skipped`

Detailed findings and roadmap live in `FINDINGS_2026-03-22.md`, `RESEARCH_LOG.md`, and `NEXT_STEPS_PLAN.md`.
