# prediction-market-btc-arb

Rust research repo for a Polymarket BTC 15-minute / final-5-minute bracket strategy.

## What This Repo Does

- Scans recurring BTC up/down Polymarket markets.
- Logs Polymarket market metadata, orderbook snapshots, websocket orderbook events, and Chainlink BTC/USD source data.
- Backfills recent paired windows for historical comparison.
- Records evidence about when the diagonal package is overpriced or potentially profitable.
- Replays captured runs, summarizes windows, and paper-trades the signal without touching a wallet.
- Stays non-custodial: no live order placement, no signing, and no wallet-side effects.

## Current Status

- The scanner exists and runs locally in Rust.
- `src/main.rs` is CLI wiring only; core logic lives in library modules.
- Live `A` is sourced from Polymarket structured page data.
- Live `B` is sourced from the first Chainlink benchmark tick at or after the final 5-minute open.
- Live mode records raw Chainlink ticks, raw Polymarket orderbook websocket events, HTTP fallback orderbook snapshots, and derived pair snapshots.
- During an active final-5-minute window, live mode continuously re-evaluates opportunities at default `10ms` cadence using websocket book updates plus fallback HTTP snapshots.
- Live mode emits paper-trade decisions immediately when a qualifying snapshot appears, and emits a `skipped` record when the window closes without an entry.
- Historical comparisons use captured data and Chainlink minute bars for sanity checking.

## Latest Observations

- March 22, 2026 `5:25PM-5:30PM ET`:
  - `118` executable snapshots
  - `29` profitable snapshots
  - best observed edge `+0.301950295775`
  - best observed package cost `0.698049704225`
- March 22, 2026 `8:10PM-8:15PM ET`:
  - `5584` total evaluations
  - `1766` executable snapshots
  - `0` qualifying snapshots
  - best observed edge `-0.028005997988036935`
  - best observed package cost `1.0280059979880369`
  - live paper-trade result `skipped`

## Main Commands

```bash
cargo run -- live --duration-seconds 300 --poll-interval-ms 1000
cargo run -- backfill --hours 24
cargo run -- run-all --backfill-hours 24 --live-duration-seconds 90 --poll-interval-ms 1000
cargo run -- replay --input data/live/pair_scans.ndjson
cargo run -- summarize --input data/live/pair_scans.ndjson
cargo run -- paper-trade --input data/live/pair_scans.ndjson
```

Helper script:

```powershell
./scripts/run_live.ps1
./scripts/run_live.ps1 -DurationSeconds 600 -PollIntervalMs 500
```

The helper script writes a summary log plus separate stdout and stderr files under `data/runs/logs/`.

## Important Output Files

- `data/live/pair_scans.ndjson`: derived live pair snapshots
- `data/live/chainlink_ticks.ndjson`: raw Chainlink live ticks with receive timestamps
- `data/live/orderbook_events.ndjson`: raw Polymarket websocket events
- `data/live/http_orderbook_snapshots.ndjson`: periodic and fallback HTTP orderbook captures
- `data/chainlink/live_reports.ndjson`: captured Chainlink live reports
- `data/historical/pair_backfill.ndjson`: historical paired-window comparisons
- `data/chainlink/historical_1d_bars.json`: Chainlink minute-bar history
- `data/runs/latest_run.json`: most recent run manifest
- `data/paper/latest_paper_trades.json`: latest paper-trade ledger
- `data/paper/live_paper_trades.ndjson`: live paper-trade decisions emitted during live runs
- `data/paper/latest_live_paper_trade.json`: most recent live paper-trade decision
- `data/analysis/latest_window_summaries.json`: latest per-window summary report
- `data/replay/latest_replay.json`: latest replay output
- `RESEARCH_LOG.md`: operating learnings and caveats
- `FINDINGS_2026-03-22.md`: current writeup of empirical results

## Known Limitations

- No live execution path yet.
- Replay is deterministic over captured snapshots, not a synthetic reconstruction of historical public CLOB depth.
- Historical public data is not sufficient for a true synthetic orderbook backtest, so replay over captured live data is the correct next step.
- Legacy live snapshots from before this refactor can still be replayed, but they have less detail than new captures.
- A successful real-time live paper-trade entry has not yet been confirmed in the current always-on live loop; the strongest positive evidence is still from captured profitable windows and replay/paper-trade over saved data.

## Next-Step Roadmap

The implementation roadmap is tracked in `NEXT_STEPS_PLAN.md`.

Completed highlights:

- refactor the scanner into reusable modules
- add websocket orderbook capture
- add replay and summary commands
- add a paper-trading command
- add a user-runnable live helper script

Remaining highlights:

- settlement reconciliation
- raw websocket-event-first replay hardening
- dynamic sizing after the first confirmed successful live paper-trade entry

## Runtime Knobs

- `.env.example` contains non-secret tuning knobs:
  - `PAPER_MIN_EDGE_CENTS`
  - `PAPER_TARGET_SHARES`
  - `LIVE_ACTIVE_POLL_MS`
  - `WS_SANITY_SNAPSHOT_SECONDS`
  - `WS_RECONNECT_MS`
