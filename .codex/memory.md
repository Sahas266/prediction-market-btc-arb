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
  - If no Chainlink tick has arrived yet for that 5-minute open, status is `waiting_for_chainlink_open_tick`
  - Live Polymarket-side `B` verification should be treated as unavailable unless a slug-stable endpoint is discovered

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
- Historical/archived 5-minute `priceToBeat` extraction from Polymarket pages still needs a more object-safe parser or a better endpoint.
- CLOB books must be normalized before use:
  - bids descending
  - asks ascending
  - otherwise the scanner will read fake `0.99` asks from the wrong end of the book

## March 22, 2026 Fix Validation
- The 5:10PM-5:15PM ET live check validated the fix:
  - `A = 68168.33529096024` from Polymarket `__NEXT_DATA__`
  - page query matched exactly: `["crypto-prices","price","BTC","2026-03-22T21:00:00Z","fifteen","2026-03-22T21:15:00Z"]`
  - `B` came from Chainlink live benchmark ticks around `67765-67792`
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

## What To Improve Next
- Add websocket-based orderbook logging for lower latency.
- Add a proper JSON/DOM parser for archived Polymarket event hydration data instead of regex over the whole HTML blob.
- Replace HTML anchor scraping with a smaller endpoint if a stable one is discovered.
- Add historical trade replay if Polymarket exposes usable per-market trade history for these markets.
