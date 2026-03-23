# Next Steps Plan: Execution-Readiness With Paper Trading

## Status Update

- The main capture and paper-trading foundation is already in place in Rust.
- Completed since this plan was written:
  - modular refactor out of `src/main.rs`
  - Polymarket websocket orderbook capture
  - raw Chainlink tick logging
  - derived executable package pricing by size bucket
  - `replay`, `summarize`, and `paper-trade` commands
  - live paper-trade emission during active runs
  - user-runnable helper script at `scripts/run_live.ps1`
- Current remaining focus:
  - raw websocket-event-first replay hardening
  - settlement reconciliation
  - more live evidence collection
  - dynamic sizing only after the first confirmed successful real-time live paper-trade entry

## Summary

- Advance the project from a log-only scanner to a capture, replay, and paper-trading system in Rust.
- Keep the next milestone strictly non-custodial: no live order submission, no signing, and no wallet-side effects.
- Define success as: full final-5m window capture with lower-latency market data, deterministic replay over captured runs, fee-adjusted size-aware opportunity scoring, and paper-trade results reconciled against settled outcomes.

## Remaining Implementation Changes

### 1. Harden replay around raw events

- Rebuild replay primarily from raw websocket orderbook events and raw Chainlink ticks instead of relying mainly on derived pair snapshots.
- Keep replay output deterministic and comparable against the original saved live run.

### 2. Finish post-close reconciliation

- Compare captured `B`, archived Polymarket metadata when available, and the final resolved market outcomes.
- Extend paper-trade logs with reconciled settlement status and realized payout.

### 3. Expand live evidence collection

- Capture more full final-5m windows with the current continuous monitoring loop.
- Track how often profitability appears, how late it appears, and how much size is actually executable at positive edge.
- Confirm the first successful real-time live paper-trade entry in the always-on loop.

### 4. Defer dynamic sizing until after live proof

- Keep the baseline one-entry fixed-size paper-trade policy for now.
- Only add "size as deep as profitable" logic after the first confirmed successful real-time live paper-trade entry.

## Interfaces And Outputs

- Keep NDJSON and JSON storage under `data/`.
- Continue maintaining raw event logs for orderbook events and raw Chainlink ticks, plus derived window summaries and paper-trade ledgers.
- Active CLI subcommands:
  - `paper-trade --input <run_or_file> --min-edge-cents 2 --target-shares 5`
  - `replay --input <run_or_file>`
  - `summarize --input <run_or_file>`
- Continue preserving these live snapshot fields:
  - latency fields
  - executable depth by size bucket
  - gross and net edge by size bucket
  - signal qualification state
  - diagonal-flip markers
- Keep `.env.example` limited to non-secret runtime knobs.
- Do not read or write signing keys in this milestone, even if present in local `.env`.

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
  - settlement reconciliation matching logged paper trades to final outcomes once reconciliation is implemented
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

- `scripts/run_live.ps1` now exists and should remain the standard user-facing entry point for manual live monitoring.
- Keep extending it so each live run writes a clear dated manifest and summary under `data/runs/`.
