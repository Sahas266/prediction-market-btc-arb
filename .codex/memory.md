# Implementation Notes

## Strategy Pairing
- 15m market slug format: `btc-updown-15m-{window_start_unix}`
- 5m market slug format: `btc-updown-5m-{window_start_unix}`
- For a 15m window starting at `t`, the matching final 5m window starts at `t + 10m`.

## Live Logging Rules
- Only a valid same-end-time pair exists during the final 5 minutes of each 15-minute window.
- Outside that window the scanner still logs Chainlink live data and logs the next eligible pair timing.
- During live scanning:
  - `A` uses Polymarket's 15-minute `priceToBeat`
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
- Public Polymarket HTML is good enough for 15-minute `A`, but not yet trustworthy for exact live 5-minute `B` verification.
- Historical/archived 5-minute `priceToBeat` extraction from Polymarket pages still needs a more object-safe parser or a better endpoint.

## What To Improve Next
- Add websocket-based orderbook logging for lower latency.
- Add a proper JSON/DOM parser for archived Polymarket event hydration data instead of regex over the whole HTML blob.
- Replace HTML anchor scraping with a smaller endpoint if a stable one is discovered.
- Add historical trade replay if Polymarket exposes usable per-market trade history for these markets.
