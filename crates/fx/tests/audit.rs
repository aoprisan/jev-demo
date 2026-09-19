//! Every call a session makes is tagged with the trade it judges, so the audit
//! log can be totalled per decision.

use jev_core::{Audit, Jev, MockJev, Rates};
use std::sync::Arc;
use synth::{FxWorld, DEFAULT_SEED};

async fn session_audit(days: u32) -> (Arc<Audit>, fx::FxSession) {
    let audit = Arc::new(Audit::new());
    let jev = Jev::with_audit(Arc::new(MockJev::new()), audit.clone());
    let world = FxWorld::generate(DEFAULT_SEED, days);
    let params = fx::StrategyParams::default();
    let session = fx::run_session(&jev, &world, &params, None).await.unwrap();
    (audit, session)
}

#[tokio::test]
async fn each_trade_is_linked_to_the_two_calls_that_judged_it() {
    let (audit, session) = session_audit(20).await;
    assert!(!session.decisions.is_empty(), "the world produced candidates");

    let records = audit.records();
    assert!(records.iter().all(|r| r.decision.is_some()), "a session leaves no call untagged");
    assert_eq!(records.len(), 2 * session.decisions.len(), "two calls a trade");

    for index in 0..session.decisions.len() {
        let id = fx::decision_id(index);
        let calls = audit.records_for(&id);
        assert_eq!(calls.len(), 2, "{id} was judged in two calls");
        let stages: Vec<Option<&str>> = calls.iter().map(|c| c.stage.as_deref()).collect();
        assert_eq!(stages, vec![Some("assess"), Some("gate")]);
        let primitives: Vec<&str> = calls.iter().map(|c| c.primitive.as_str()).collect();
        assert_eq!(primitives, vec!["batch", "gate"]);
    }
}

#[tokio::test]
async fn the_ledger_prices_every_decision_and_the_run_totals_them() {
    let (audit, session) = session_audit(20).await;
    let rates = Rates::ASSUMED;
    let ledger = audit.ledger(rates);

    assert_eq!(ledger.decisions(), session.decisions.len());
    assert_eq!(ledger.untagged().calls, 0, "a session makes no run-level calls");

    let summed: f64 = ledger.by_decision().map(|(_, cost)| cost.usd).sum();
    assert!((summed - ledger.total().usd).abs() < 1e-9, "the parts are the whole");
    assert!(ledger.total().usd > 0.0, "the mock still reports tokens");
    assert!(
        (ledger.usd_per_decision() - ledger.total().usd / session.decisions.len() as f64).abs()
            < 1e-9
    );

    // The tag is the only thing linking a call to a trade: pricing one decision
    // must not depend on where in the log its calls fell.
    let first = ledger.of_decision(&fx::decision_id(0));
    assert_eq!(first.calls, 2);
    assert!((first.usd - rates.usd(first.input_tokens, first.output_tokens)).abs() < 1e-12);
}
