# Research Log

## Scope
- Repo purpose: log-only research environment for the Polymarket BTC 15m / final-5m same-end-time bracket strategy.
- Implementation language: Rust only.
- Current state: scanner/backfill exists and runs locally; no execution path is implemented.

## Confirmed Endpoint Learnings
- Polymarket market metadata:
  - `https://gamma-api.polymarket.com/markets/slug/{slug}`
- Polymarket CLOB orderbook:
  - `https://clob.polymarket.com/book?token_id={token_id}`
- Polymarket recurring BTC slug formats:
  - 15m: `btc-updown-15m-{window_start_unix}`
  - 5m: `btc-updown-5m-{window_start_unix}`
- Matching rule:
  - For a 15m window starting at `t`, the relevant final 5m window starts at `t + 10m`.
- Chainlink stream metadata page:
  - `https://data.chain.link/streams/btc-usd`
- Fastest confirmed public Chainlink live source for `B`:
  - `https://data.chain.link/api/live-data-engine-stream-data?feedId=...&abiIndex=0&queryWindow=1m`
- Chainlink 1-minute historical bars:
  - `https://data.chain.link/api/historical-data-engine-stream-data?feedId=...&abiIndex=0&timeRange=1D`
- Chainlink 15-minute history:
  - `https://data.chain.link/api/historical-timescale-stream-data?feedId=...&timeRange=1D`

## Strategy-Specific Learnings
- `A` does not need low-latency capture.
  - It is acceptable to use Polymarket's 15-minute `priceToBeat` because the trade decision does not need to happen until the final 5-minute window opens.
- `B` is latency-sensitive.
  - The implementation now sources `B` from Chainlink's direct benchmark tick stream as soon as the 5-minute window opens.
- This should be faster and more robust than waiting for Polymarket's 5-minute page state to become unambiguous.

## Polymarket HTML Caveat
- The Polymarket event page contains many recurring-window objects for neighboring windows.
- A naive regex over the whole HTML blob can silently bind a slug from one object to `priceToBeat` from another object.
- This is especially dangerous for 5-minute markets because many nearby archived windows are embedded in the same page hydration payload.
- Conclusion:
  - Do not trust a global HTML regex as proof of exact live 5-minute `B`.
  - Treat public HTML as acceptable for current `A` usage but not for exact live `B` verification.

## Verification Results
- Historical backfill over the last 24 hours ran successfully and produced `96` pair rows.
- Using Chainlink 1-minute bars as a proxy for 5-minute opens:
  - usable rows: `95`
  - median absolute delta: about `0.07868`
  - p90 absolute delta: about `1.17219`
  - max absolute delta: about `8.81956`
- Interpretation:
  - minute bars are useful for rough sanity checks
  - minute bars are not exact enough to prove opening-anchor equality

## Current Output Artifacts
- `data/chainlink/context.json`
- `data/chainlink/live_reports.ndjson`
- `data/chainlink/historical_1d_bars.json`
- `data/live/pair_scans.ndjson`
- `data/historical/pair_backfill.ndjson`
- `data/runs/latest_run.json`

## Current Operational Policy
- Live scanner:
  - use Polymarket 15m `priceToBeat` for `A`
  - use first Chainlink benchmark tick at/after final 5m open for `B`
  - log pair state and orderbooks during the final 5m window
- Historical scanner:
  - use Polymarket archived anchors where available
  - compare against Chainlink minute bars as approximation only

## Known Gaps
- No slug-stable public Polymarket endpoint has been confirmed yet for exact live 5-minute `priceToBeat`.
- Archived 5-minute anchor extraction from public HTML still needs a safer parser.
- No websocket orderbook logger yet.
- No trade execution path yet.

## Recommended Next Steps
- Build a structured parser for the Polymarket hydration payload instead of regex over raw HTML.
- Search for the hydrated frontend endpoint that directly serves the event payload with `eventMetadata.priceToBeat`.
- Add websocket logging for Polymarket books.
- Add post-close reconciliation that compares:
  - live-captured Chainlink `B`
  - archived Polymarket `B`
  - final resolved outcome
