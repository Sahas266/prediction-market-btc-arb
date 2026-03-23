# Implementation Notes

## Strategy Pairing

- 15m market slug format: `btc-updown-15m-{window_start_unix}`
- 5m market slug format: `btc-updown-5m-{window_start_unix}`
- For a 15m window starting at `t`, the matching final 5m window starts at `t + 10m`.

## Live Logging Rules

- Only a valid same-end-time pair exists during the final 5 minutes of each 15-minute window.
- Outside that window the scanner still logs Chainlink live data and logs the next eligible pair timing.
- During live scanning:
  - `A` uses structured Polymarket data, not raw HTML regex:
    - live windows: page `__NEXT_DATA__` `crypto-prices` `openPrice`
    - closed windows: Gamma `events[].eventMetadata.priceToBeat`
  - `B` uses Chainlink's direct benchmark stream immediately after the final 5-minute window opens
  - if no Chainlink tick has arrived yet for that 5-minute open, status is `waiting_for_chainlink_open_tick`
  - live Polymarket-side `B` verification should be treated as unavailable unless a slug-stable endpoint is discovered

## Fee Model

- The scanner uses the market `feeSchedule` when present.
- Current BTC crypto markets show:
  - `feesEnabled = true`
  - `feeType = crypto_fees`
  - `feeSchedule.rate = 0.25`
  - `feeSchedule.exponent = 2`

## Verification Status

- Live `B` capture is sourced from Chainlink benchmark ticks and is the fastest confirmed public path.
- Historical `B` comparisons against Chainlink 1-minute opens are only approximate.
- Public Polymarket raw HTML regex is not trustworthy for anchors.
- Structured Polymarket page hydration is now trusted for live `A`.
- Historical and archived 5-minute `priceToBeat` extraction from Polymarket pages still needs a more object-safe parser or a better endpoint.
- CLOB books must be normalized before use:
  - bids descending
  - asks ascending
  - otherwise the scanner will read fake `0.99` asks from the wrong end of the book

## March 22, 2026 Validation

- The `5:10PM-5:15PM ET` live check validated the fix:
  - `A = 68168.33529096024` from Polymarket `__NEXT_DATA__`
  - page query matched exactly: `["crypto-prices","price","BTC","2026-03-22T21:00:00Z","fifteen","2026-03-22T21:15:00Z"]`
  - `B` came from Chainlink live benchmark ticks
  - selected pair remained `D15+U5`
  - package cost was about `1.11-1.21`, so still not profitable, but no longer falsely near `1.98`
- The next full patched window `5:25PM-5:30PM ET` produced the first verified profitable segment:
  - executable snapshots: `118`
  - profitable snapshots: `29`
  - first profitable snapshot: `2026-03-22T17:28:46.531154800-04:00`
  - last profitable snapshot: `2026-03-22T17:29:55.234304100-04:00`
  - best observed edge: about `+0.30195`
  - best observed cost: about `0.69805`
  - profitable rows were all `U15+D5`
- The full continuous-monitoring `8:10PM-8:15PM ET` window stayed negative:
  - total evaluations: `5584`
  - executable snapshots: `1766`
  - qualifying snapshots: `0`
  - best observed edge: about `-0.02801`
  - best observed cost: about `1.02801`
  - live paper-trade result: `skipped`

## What To Improve Next

- Websocket-based orderbook logging is already implemented for live capture.
- Add a proper JSON or DOM parser for archived Polymarket event hydration data instead of regex over the whole HTML blob.
- Replace HTML anchor scraping with a smaller endpoint if a stable one is discovered.
- Harden replay from raw websocket orderbook events instead of relying mainly on derived pair snapshots.
- Confirm the first successful real-time live paper-trade entry before changing sizing policy.

## Current Commands

- `cargo run -- live --duration-seconds 300 --poll-interval-ms 1000`
- `cargo run -- backfill --hours 24`
- `cargo run -- replay --input data/live/pair_scans.ndjson`
- `cargo run -- summarize --input data/live/pair_scans.ndjson`
- `cargo run -- paper-trade --input data/live/pair_scans.ndjson`
- `powershell -ExecutionPolicy Bypass -File scripts/run_live.ps1 -DurationSeconds 300`

## Current Artifacts

- `data/live/chainlink_ticks.ndjson`
- `data/live/orderbook_events.ndjson`
- `data/live/http_orderbook_snapshots.ndjson`
- `data/paper/latest_paper_trades.json`
- `data/paper/live_paper_trades.ndjson`
- `data/paper/latest_live_paper_trade.json`
- `data/analysis/latest_window_summaries.json`
- `data/replay/latest_replay.json`

## Current Implementation Notes

- `src/main.rs` is now CLI wiring only.
- `src/app.rs` owns command orchestration and the stateful live loop.
- `src/sources.rs` owns Chainlink, Gamma, HTTP orderbook, and Polymarket websocket integration.
- `src/strategy.rs` owns executable package pricing, replay, summary generation, and paper-trade policy.
- The live loop keeps the first Chainlink tick for a 5-minute window fixed as `B`; do not recompute `B` from a rolling 1-minute query window.
- Replay and paper-trade commands include a compatibility fallback for legacy `pair_scans` rows that predate the richer `package_quotes` schema.
- Current sizing policy is fixed-size baseline validation:
  - default target is 5 shares
  - dynamic "size as deep as profitable" logic is a later task, gated on first achieving a successful live paper-trade run
- Active live monitoring default is `10ms` via `LIVE_ACTIVE_POLL_MS`, with a hard floor of `1ms`.
- Live paper-trade behavior:
  - emit a paper-trade record immediately on the first qualifying snapshot in a window
  - emit a `skipped` paper-trade record when the window closes without an entry
