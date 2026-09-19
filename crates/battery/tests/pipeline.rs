//! The battery pipeline end to end, against the mock backend.

use battery::{run_session, judge_day, with_grid_notice, without_grid_notices, ScheduleKind};
use jev_core::{Action, Audit, Jev, MockJev};
use std::sync::Arc;
use synth::{BatteryWorld, DEFAULT_SEED};

fn mock_jev() -> Jev {
    Jev::new(Arc::new(MockJev::new()))
}

fn world(days: u32) -> BatteryWorld {
    BatteryWorld::generate(DEFAULT_SEED, days)
}

#[tokio::test]
async fn the_stages_run_in_the_order_the_brief_names() {
    let audit = Arc::new(Audit::new());
    let jev = Jev::with_audit(Arc::new(MockJev::new()), audit.clone());
    judge_day(&jev, &world(20), 14).await.unwrap();

    let stages: Vec<String> =
        audit.records().iter().map(|r| r.stage.clone().unwrap()).collect();
    assert_eq!(stages, vec!["regime", "rank", "sanity", "risk", "gate"]);

    let primitives: Vec<&str> =
        audit.records().iter().map(|r| r.primitive.as_str()).collect();
    assert_eq!(primitives, vec!["classify", "rank", "check", "score", "gate"]);
}

#[tokio::test]
async fn the_checks_judge_the_schedule_the_ranking_chose() {
    // Before ranking, the schedule-level features describe the solver's
    // default; afterwards they describe the winner.
    let audit = Arc::new(Audit::new());
    let jev = Jev::with_audit(Arc::new(MockJev::new()), audit.clone());
    let w = world(20);
    let day = (0..20).find(|d| w.afrr_on(*d).is_some()).expect("a reserve day");
    let record = judge_day(&jev, &w, day).await.unwrap();

    let records = audit.records();
    let under = |i: usize| -> String {
        records[i].state["input"]["features"]["under_consideration"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    assert_eq!(under(0), "balanced", "the ranking is asked about the default day");
    assert_eq!(under(1), "balanced");
    assert_eq!(under(2), record.judgment.chosen.as_str(), "checks follow the winner");
    assert_eq!(under(4), record.judgment.chosen.as_str(), "so does the gate");
}

#[tokio::test]
async fn a_reserve_day_is_ranked_toward_the_reserve_heavy_schedule() {
    let w = world(30);
    let day = (0..30).find(|d| w.afrr_on(*d).is_some()).unwrap();
    let record = judge_day(&mock_jev(), &w, day).await.unwrap();

    assert_eq!(record.judgment.chosen, ScheduleKind::ReserveHeavy);
    assert_eq!(record.judgment.ranking.ordered.len(), 3, "every candidate is ranked");
    assert_eq!(record.judgment.ranking.top().unwrap().id, ScheduleKind::ReserveHeavy);
}

#[tokio::test]
async fn adding_a_grid_notice_over_the_discharge_block_flips_the_ranking() {
    // The demo's counterfactual: identical prices, one note added.
    let w = world(40);
    let day = (0..40)
        .find(|d| {
            w.afrr_on(*d).is_none() && w.grid_notes_on(*d).is_empty()
        })
        .expect("a quiet day");

    let quiet = judge_day(&mock_jev(), &without_grid_notices(&w, day), day).await.unwrap();
    let block = quiet.chosen().discharge_block().expect("the plan discharges");
    let noticed = judge_day(&mock_jev(), &with_grid_notice(&w, day, block.0, block.1), day)
        .await
        .unwrap();

    assert_eq!(quiet.judgment.chosen, ScheduleKind::Balanced);
    assert_eq!(
        noticed.judgment.chosen,
        ScheduleKind::ReserveHeavy,
        "a note over the discharge block should move the ranking"
    );
    // And the prices really were identical.
    assert_eq!(quiet.schedules, noticed.schedules);
}

#[tokio::test]
async fn an_implausible_margin_escalates_rather_than_resizing() {
    let jev = mock_jev();
    let session = run_session(&jev, &world(60), None).await.unwrap();
    let escalated: Vec<_> = session
        .days
        .iter()
        .filter(|d| d.judgment.gate.action == Action::Escalate)
        .collect();

    assert!(!escalated.is_empty(), "60 days should contain at least one escalation");
    for day in escalated {
        assert!(
            !day.judgment.checks.get("margin_plausible").unwrap().ok,
            "day {} escalated without an implausible margin",
            day.day
        );
        assert_eq!(day.gated.size_factor, 0.0, "an escalation runs nothing");
        assert!((day.gated.realised_margin - 0.0).abs() < 1e-9);
    }
}

#[tokio::test]
async fn the_gated_desk_holds_the_reserve_where_the_solver_only_desk_does_not() {
    // The headline comparison: the solver-only desk always runs the balanced
    // schedule, which breaches; the judgment layer picks one that does not.
    let session = run_session(&mock_jev(), &world(60), None).await.unwrap();

    assert!(
        session.solver_only.reserve_breaches > 0,
        "the solver-only desk should breach: otherwise there is nothing to show"
    );
    assert_eq!(
        session.gated.reserve_breaches, 0,
        "the gated desk breached the reserve {} times",
        session.gated.reserve_breaches
    );
    assert!(
        session.gated.reserve_payment > session.solver_only.reserve_payment,
        "holding the reserve should collect more of the capacity payment"
    );
    assert!(
        session.gated.cycles < session.solver_only.cycles,
        "the gated desk should wear the asset less"
    );
}

#[tokio::test]
async fn a_session_is_deterministic() {
    let a = run_session(&mock_jev(), &world(15), None).await.unwrap();
    let b = run_session(&mock_jev(), &world(15), None).await.unwrap();
    assert_eq!(
        serde_json::to_string(&a.days).unwrap(),
        serde_json::to_string(&b.days).unwrap()
    );
}

#[tokio::test]
async fn every_day_produces_a_complete_audit_trail() {
    let audit = Arc::new(Audit::new());
    let jev = Jev::with_audit(Arc::new(MockJev::new()), audit.clone());
    let session = run_session(&jev, &world(10), None).await.unwrap();

    assert_eq!(audit.len(), session.days.len() * 5, "five primitive calls per day");
    for record in audit.records() {
        assert!(!record.asks.is_empty());
        assert_eq!(record.verdicts.len(), record.asks.len());
        assert!(!record.output.is_null());
        assert_eq!(record.backend, "mock");
    }
}

#[tokio::test]
async fn the_day_limit_bounds_the_run() {
    let session = run_session(&mock_jev(), &world(30), Some(7)).await.unwrap();
    assert_eq!(session.days.len(), 7);

    // And a limit past the end of the world is clamped rather than panicking.
    let session = run_session(&mock_jev(), &world(5), Some(50)).await.unwrap();
    assert_eq!(session.days.len(), 5);
}
