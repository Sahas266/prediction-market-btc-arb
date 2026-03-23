use prediction_market_btc_arb::io::read_jsonl;
use prediction_market_btc_arb::models::{GammaMarket, LivePairScan};
use prediction_market_btc_arb::strategy::{build_paper_trades, reconcile_trade};

#[test]
fn replay_fixture_for_510_window_produces_no_entry() {
    let scans: Vec<LivePairScan> =
        read_jsonl("tests/fixtures/replay_negative_510.ndjson").expect("fixture loads");
    let trades = build_paper_trades(&scans, 2.0, 5.0);
    assert_eq!(trades.len(), 1);
    assert_eq!(trades[0].status, "skipped");
}

#[test]
fn replay_fixture_for_525_window_produces_entry() {
    let scans: Vec<LivePairScan> =
        read_jsonl("tests/fixtures/replay_positive_525.ndjson").expect("fixture loads");
    let trades = build_paper_trades(&scans, 2.0, 5.0);
    assert_eq!(trades.len(), 1);
    assert_eq!(trades[0].status, "entered");
    assert!(trades[0].entry_snapshot_at.is_some());
}

#[test]
fn settlement_reconciliation_updates_trade_outcome() {
    let scans: Vec<LivePairScan> =
        read_jsonl("tests/fixtures/replay_positive_525.ndjson").expect("fixture loads");
    let mut trades = build_paper_trades(&scans, 2.0, 5.0);
    let mut trade = trades.remove(0);

    let fifteen_market = GammaMarket {
        id: "1".to_string(),
        question: "Fixture fifteen".to_string(),
        slug: "btc-updown-15m-1774214100".to_string(),
        condition_id: "cond1".to_string(),
        resolution_source: None,
        end_date: "2026-03-22T21:30:00Z".to_string(),
        description: None,
        outcomes: vec!["Up".to_string(), "Down".to_string()],
        outcome_prices: vec![1.0, 0.0],
        active: false,
        closed: true,
        accepting_orders: Some(false),
        clob_token_ids: vec!["1".to_string(), "2".to_string()],
        best_bid: None,
        best_ask: None,
        fees_enabled: Some(true),
        fee_type: None,
        fee_schedule: None,
        last_trade_price: None,
        events: Vec::new(),
    };
    let five_market = GammaMarket {
        id: "2".to_string(),
        question: "Fixture five".to_string(),
        slug: "btc-updown-5m-1774214700".to_string(),
        condition_id: "cond2".to_string(),
        resolution_source: None,
        end_date: "2026-03-22T21:30:00Z".to_string(),
        description: None,
        outcomes: vec!["Up".to_string(), "Down".to_string()],
        outcome_prices: vec![0.0, 1.0],
        active: false,
        closed: true,
        accepting_orders: Some(false),
        clob_token_ids: vec!["3".to_string(), "4".to_string()],
        best_bid: None,
        best_ask: None,
        fees_enabled: Some(true),
        fee_type: None,
        fee_schedule: None,
        last_trade_price: None,
        events: Vec::new(),
    };

    reconcile_trade(&mut trade, &fifteen_market, &five_market);
    assert_eq!(trade.status, "settled_win_2");
    assert!(trade.realized_profit.unwrap() > 0.0);
}
