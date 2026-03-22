# Next Steps Plan: Execution-Readiness With Paper Trading

## Summary

- Advance the project from a log-only scanner to a capture, replay, and paper-trading system in Rust.
- Keep the next milestone strictly non-custodial: no live order submission, no signing, and no wallet-side effects.
- Define success as: full final-5m window capture with lower-latency market data, deterministic replay over captured runs, fee-adjusted size-aware opportunity scoring, and paper-trade results reconciled against settled outcomes.

## Implementation Changes

### 1. Refactor the scanner into clear subsystems

- Keep `src/main.rs` as CLI wiring only.
- Move source adapters, strategy logic, replay logic, and paper-trade state into separate Rust modules.
- Preserve current `live`, `backfill`, and `run-all` commands, but route them through shared library code rather than embedding all logic inline.

### 2. Upgrade live capture from polling-only to capture-grade logging

- Add a Polymarket websocket orderbook stream for the two selected legs during the final 5-minute window.
- Keep the current HTTP orderbook fetch as fallback and periodic sanity snapshot, not as the primary live source.
- Continue sourcing `A` from Polymarket structured page data and `B` from the first Chainlink benchmark tick at or after the 5-minute open.
- Record capture timestamps, source timestamps, and local receive latency for every Chainlink tick and every book event.
- Persist raw live inputs and derived snapshots separately so replay can operate from raw events rather than only summarized rows.

### 3. Replace top-of-book-only scoring with executable package pricing

- Compute package cost by walking asks across both selected legs for a configurable share ladder: `1`, `5`, `10`, `25`.
- Include taker fees in the package cost using the fee schedule already exposed in Gamma metadata.
- Emit both gross edge and fee-adjusted edge per size bucket.
- Treat a signal as executable only when both legs are orderable, both asks are present, and minimum fillable size is available at the configured threshold.
- Keep diagonal selection deterministic: use the current `A`/`B` relationship and skip `equal` cases.

### 4. Add a baseline paper-trading engine

- Add a `paper-trade` CLI command that consumes captured live or replayed events.
- Baseline policy:
  - one entry attempt per eligible final-5m window
  - entry on the first event where fee-adjusted edge is at least `+$0.02/share`
  - target size `5` shares, clipped down to available executable depth
  - no re-entry after the first fill attempt in the same window
  - hold to market resolution with no intra-window exit logic in this milestone
- Log paper trades, entry reason, modeled fill price, size, fees, expected floor payout, and realized settled payout.
- Mark trades as `skipped`, `entered`, `settled_win_1`, `settled_win_2`, or `settled_loss_unexpected_data_issue` so reconciliation is auditable.

### 5. Add replay and post-close analytics

- Add a `replay` command that rebuilds strategy state from raw captured events and reproduces paper-trade decisions deterministically.
- Add a `summarize` command that outputs per-window metrics: positive-edge duration, first and last qualifying timestamp, best edge, fillable size, selected diagonal changes, and paper-trade outcome.
- Historical analysis for this milestone must be replay-based over captured live windows. Do not present synthetic backtests from public historical CLOB data as if they were orderbook-accurate.
- Add post-close reconciliation that compares captured `B`, archived Polymarket metadata when available, and the final resolved market outcomes.

## Interfaces And Outputs

- Keep NDJSON and JSON storage under `data/`.
- Add raw event logs for orderbook events and raw Chainlink ticks, plus derived window summaries and paper-trade ledgers.
- Add CLI subcommands:
  - `paper-trade --input <run_or_file> --min-edge-cents 2 --target-shares 5`
  - `replay --input <run_or_file>`
  - `summarize --from <date> --to <date>`
- Extend the existing live snapshot schema with:
  - latency fields
  - executable depth by size bucket
  - gross and net edge by size bucket
  - signal qualification state
  - diagonal-flip markers
- Add `.env.example` entries only for non-secret runtime knobs such as minimum edge threshold, target shares, and optional websocket reconnect tuning.
- Do not read or write any signing keys in this milestone, even if present in local `.env`.

## Test Plan

- Unit tests for:
  - Polymarket and Chainlink payload parsing from saved fixtures
  - orderbook normalization
  - depth-walk executable pricing
  - fee math
  - diagonal selection
  - paper-trade state transitions
- Integration tests for:
  - replay of the March 22, 2026 `5:10PM-5:15PM ET` window producing no paper entry under baseline settings
  - replay of the March 22, 2026 `5:25PM-5:30PM ET` window producing at least one qualifying entry under baseline settings
  - settlement reconciliation matching logged paper trades to final outcomes
- Runtime acceptance checks:
  - one-hour live capture across multiple windows without panic or schema corruption
  - replay output matches original derived signals for the same run
  - no secrets are emitted into logs or manifests

## Assumptions And Defaults

- Rust remains the only implementation language.
- The next milestone is execution-readiness, not real-money execution.
- Paper trading is the only trading mode in scope.
- Chainlink's first benchmark tick at or after the 5-minute open remains the source of truth for live `B` until a cleaner Polymarket live anchor endpoint is confirmed.
- Historical backtesting in this phase means deterministic replay over captured live data. Fully synthetic orderbook backtests from public archives are out of scope.
- Default trade gate is `+$0.02/share` fee-adjusted edge and `5` target shares.

## Operator Note

- Add a user-runnable live helper script in a later step, preferably `scripts/run_live.ps1`, that launches the live scanner, timestamps the run, and writes logs to a dated file under `data/runs/` so the strategy can be monitored manually without reconstructing the CLI command each time.
