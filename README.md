# prediction-market-btc-arb

Rust research repo for a Polymarket BTC 15-minute / final-5-minute bracket strategy.

## What This Repo Does

- Scans recurring BTC up/down Polymarket markets.
- Logs Polymarket market metadata, orderbook snapshots, and Chainlink BTC/USD source data.
- Backfills recent paired windows for historical comparison.
- Records evidence about when the diagonal package is overpriced or potentially profitable.
- Stays non-custodial today: no live order placement, no signing, and no wallet-side effects.

## Current Status

- The scanner exists and runs locally in Rust.
- Live `A` is sourced from Polymarket structured page data.
- Live `B` is sourced from the first Chainlink benchmark tick at or after the final 5-minute open.
- Historical comparisons use recent captured data and Chainlink minute bars for sanity checking.
- The repo has already observed both unprofitable windows and at least one live window with profitable moments after the data-quality fixes.

## Main Commands

```bash
cargo run -- live --duration-seconds 300 --poll-interval-ms 1000
cargo run -- backfill --hours 24
cargo run -- run-all --backfill-hours 24 --live-duration-seconds 90 --poll-interval-ms 1000
```

## Important Output Files

- `data/live/pair_scans.ndjson`: derived live pair snapshots
- `data/chainlink/live_reports.ndjson`: captured Chainlink live reports
- `data/historical/pair_backfill.ndjson`: historical paired-window comparisons
- `data/chainlink/historical_1d_bars.json`: Chainlink minute-bar history
- `data/runs/latest_run.json`: most recent run manifest
- `RESEARCH_LOG.md`: operating learnings and caveats
- `FINDINGS_2026-03-22.md`: current writeup of empirical results

## Known Limitations

- No websocket orderbook logger yet.
- No replay command yet.
- No paper-trading engine yet.
- No live execution path yet.
- Historical public data is not sufficient for a true synthetic orderbook backtest, so replay over captured live data is the correct next step.

## Next-Step Roadmap

The implementation roadmap is tracked in `NEXT_STEPS_PLAN.md`.

Highlights:

- refactor the scanner into reusable modules
- add websocket orderbook capture
- add replay and summary commands
- add a paper-trading command
- add settlement reconciliation

## Planned Operator Helper

Add a user-runnable live helper script in a later step, ideally `scripts/run_live.ps1`, that starts the live scanner and writes timestamped logs to `data/runs/`. That will make it easy to run the strategy logger manually without remembering the full CLI command.
