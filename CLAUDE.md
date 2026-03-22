# Prediction Market BTC Arb

## Project overview
Polymarket BTC 5m/15m same-end-time bracket arbitrage. Exploit deterministic payoff relationships between a BTC 15-minute Up/Down market and the final BTC 5-minute Up/Down market sharing the same end time T. The correct diagonal pair (determined by comparing opening anchors A vs B) guarantees a $1 floor payout, with $2 in the overlap zone.

## Strategy review findings (2026-03-22)

### Confirmed correct
- Payoff math is sound across all three cases (A<B, A>B, A=B)
- The anchor-ordering rule is **essential** — the wrong diagonal has a $0 payoff zone. You CANNOT just buy whichever diagonal is cheaper; the cheaper one is systematically the dangerous one.
- Pairing must be restricted to the **final** 5m subwindow only (shares terminal value F)
- Must wait until B is fixed (5m market opens) before entering
- Resolution source: Chainlink BTC/USD data stream

### Key gaps identified
1. **"Price to Beat" API availability** — plan assumes this is queryable but never specifies how. Needs verification: is it in Gamma API response, in market description text, or frontend-only?
2. **Token ID mapping** — Yes/No token IDs must be mapped to Up/Down per market. Not specified in plan.
3. **Fee formula** — Polymarket CLOB fees are roughly `min(price, 1-price) * 0.02`. At typical leg prices ($0.40-0.60), total fees ~$0.02/pair, which can eat the entire edge.
4. **Case A=B should be skipped** — no overlap bonus, edge almost certainly negative after fees.
5. **Speed sensitivity** — edge window likely 0-60 seconds after 5m market opens. Need <5s total latency budget.
6. **Settlement mechanics** — plan doesn't cover auto-redemption vs manual claim, settlement timing, or capital lockup.
7. **Pseudocode size loop is backwards** — iterates smallest-first and breaks; should iterate largest-first to maximize profitable size.
8. **No scanner-first phase** — should log-only for 48h+ before risking capital.

### Recommended implementation order
1. API discovery script — verify "Price to Beat" is programmatically accessible
2. Token ID mapper — confirm Yes=Up, No=Down per market
3. Fee formula integration — hard-code actual Polymarket fee curve
4. Phase 0 scanner/logger — no execution, data collection for 48h
5. Analyze Phase 0 data — confirm edge exists in practice
6. Build executor — only if Phase 0 confirms viable edges

## Polymarket API reference

### API docs
- Introduction: https://docs.polymarket.com/api-reference/introduction
- Auth: https://docs.polymarket.com/api-reference/authentication
- Clients/SDKs: https://docs.polymarket.com/api-reference/clients-sdks

### Three API services
- **Gamma API** — markets, events, tags, series, comments, search
- **Data API** — user positions, trades, activity, holder data
- **CLOB API** — orderbook data, pricing, midpoints, spreads, fee rates

### Gamma API market discovery
- Endpoint pattern: `https://gamma-api.polymarket.com/markets?slug_contains=btc-5-minute`
- Full schema not yet verified — need to dump response to find "Price to Beat" field

### Resolution source (BTC markets)
"This market will resolve to 'Up' if the Bitcoin price at the end of the time range specified in the title is greater than or equal to the price at the beginning of that range. Otherwise, it will resolve to 'Down'. Resolution source: Chainlink BTC/USD data stream at https://data.chain.link/streams/btc-usd."

### Auth credentials
- `.env` has: POLYMARKET_API_KEY, POLYMARKET_API_SECRET, POLYMARKET_API_PASSPHRASE, POLYGON_PRIVATE_KEY, POLYGON_PUBLIC_KEY, OPEN_ROUTER_API_KEY

### Unknown/unverified
- Whether "Price to Beat" is a field in Gamma API market response or only in frontend
- Fee rate endpoint exists but formula not confirmed from docs
- Token ID structure (which token = Yes/Up vs No/Down)
