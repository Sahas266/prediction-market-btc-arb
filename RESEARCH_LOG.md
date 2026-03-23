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
- This is faster and more robust than waiting for Polymarket's 5-minute page state to become unambiguous.
- Live `A` is now sourced from the Polymarket page hydration payload, not raw regex over the HTML blob.
  - Exact source: the `__NEXT_DATA__` `crypto-prices` query `openPrice`
- Closed-window `A` and `B` are now sourced from Gamma `events[].eventMetadata.priceToBeat` when available.

## Polymarket HTML Caveat

- The Polymarket event page contains many recurring-window objects for neighboring windows.
- A naive regex over the whole HTML blob can silently bind a slug from one object to `priceToBeat` from another object.
- This is especially dangerous for 5-minute markets because many nearby archived windows are embedded in the same page hydration payload.
- Conclusion:
  - do not trust a global HTML regex as proof of exact anchors
  - parse the structured page payload instead
  - the `__NEXT_DATA__` script tag can include extra attributes like `crossorigin`, so exact-tag regexes can miss it

## CLOB Book Normalization

- The public CLOB `book` response is not returned best-first.
- Correct normalization:
  - bids descending
  - asks ascending
- Before this fix, the scanner was reading the wrong end of the book and producing fake near-`1.98` package costs.

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
- Post-fix validation on March 22, 2026:
  - live `5:10PM-5:15PM ET` window:
    - `A = 68168.33529096024` from Polymarket `__NEXT_DATA__`
    - exact page query match: `["crypto-prices","price","BTC","2026-03-22T21:00:00Z","fifteen","2026-03-22T21:15:00Z"]`
    - Chainlink `5:00PM` minute-open proxy: `68168.82957403638`
    - delta `A - proxy`: about `-0.49428`
    - selected pair: `D15+U5`
    - observed package cost after CLOB normalization: about `1.11-1.21`
    - observed edge after fix: about `-0.11` to `-0.21`
  - full patched live `5:25PM-5:30PM ET` window:
    - executable snapshots: `118`
    - profitable snapshots: `29`
    - first profitable snapshot: `2026-03-22T17:28:46.531154800-04:00`
    - last profitable snapshot: `2026-03-22T17:29:55.234304100-04:00`
    - best observed edge: about `+0.30195`
    - best observed cost: about `0.69805`
    - worst observed edge in that same window: about `-0.68781`
    - early in the window the selected pair was `D15+U5`
    - profitable rows appeared only after the pair flipped to `U15+D5`
    - interpretation:
      - profitable live windows do exist
      - profitability can emerge late rather than immediately at the 5-minute open
  - full continuous-monitoring live `8:10PM-8:15PM ET` window:
    - total evaluations: `5584`
    - executable snapshots: `1766`
    - qualifying snapshots: `0`
    - best observed edge: about `-0.02801`
    - best observed cost: about `1.02801`
    - live paper-trade result: `skipped`
    - skip reason: `live_window_closed_without_entry`
    - interpretation:
      - some full windows remain fully negative even under dense `10ms` monitoring
      - continuous monitoring is still necessary because the positive windows found so far have turned profitable late
- Historical sanity run after the fix:
  - recent backfill rows now show `polymarket_gamma_event_metadata` as the anchor source for closed windows
  - examples:
    - `4:30PM-4:45PM ET` `A = 68312.1083495388`
    - `4:45PM-5:00PM ET` `A = 68361.713848192`
    - `4:40PM-4:45PM ET` `B = 68232.09734852398`
    - `4:55PM-5:00PM ET` `B = 68195.981`

## Current Output Artifacts

- `data/chainlink/context.json`
- `data/chainlink/live_reports.ndjson`
- `data/chainlink/historical_1d_bars.json`
- `data/live/pair_scans.ndjson`
- `data/live/chainlink_ticks.ndjson`
- `data/live/orderbook_events.ndjson`
- `data/live/http_orderbook_snapshots.ndjson`
- `data/historical/pair_backfill.ndjson`
- `data/runs/latest_run.json`
- `data/paper/latest_paper_trades.json`
- `data/paper/live_paper_trades.ndjson`
- `data/paper/latest_live_paper_trade.json`
- `data/analysis/latest_window_summaries.json`
- `data/replay/latest_replay.json`

## Current Operational Policy

- Live scanner:
  - use Polymarket structured page hydration `openPrice` for live `A`
  - use first Chainlink benchmark tick at or after final 5m open for `B`
  - continuously re-evaluate pair state during the final 5m window at a tighter active cadence, default `10ms`
  - emit live paper-trade records immediately on the first qualifying snapshot for a window
  - emit a live `skipped` paper-trade record if the window closes without an entry
- Historical scanner:
  - use Gamma `eventMetadata.priceToBeat` where available
  - compare against Chainlink minute bars as approximation only

## Known Gaps

- No slug-stable public Polymarket endpoint has been confirmed yet for exact live 5-minute `priceToBeat`.
- No trade execution path yet.
- Live Polymarket-side `B` parity checking is still unresolved.
- Historical minute-bar comparisons are still only proxies, not tick-perfect reconstructions.
- Replay currently derives decisions from captured live snapshots. Raw websocket event replay is the next hardening step.

## Recommended Next Steps

- Keep the structured Polymarket hydration parser and remove the legacy raw-regex fallback once confidence is high enough.
- Search for the hydrated frontend endpoint that directly serves the same `crypto-prices` / anchor data without the full page HTML.
- Harden replay from raw websocket orderbook events instead of relying mainly on derived live snapshots.
- After a successful end-to-end live paper-trade run is confirmed, add dynamic sizing logic that walks depth and scales beyond the current fixed 5-share baseline only while marginal edge remains acceptable.
- Add post-close reconciliation that compares:
  - live-captured Chainlink `B`
  - archived Polymarket `B`
  - final resolved outcome
