//! The API's contract, exercised through the router itself.
//!
//! These tests pin the shape the TypeScript client mirrors. A field renamed in
//! `dto.rs` without the same rename in `ui/src/api/types.ts` is a silent break
//! at runtime, so the keys are asserted here rather than left to a reader.
//!
//! Offline by construction: every run asks for the mock backend.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use server::{AppState, Config};
use tower::ServiceExt;

/// A server with an empty run store, always offline.
fn state() -> AppState {
    AppState::new(Config { default_mock: true, ..Config::default() })
}

async fn get(state: &AppState, uri: &str) -> (StatusCode, Value) {
    let request = Request::builder().uri(uri).body(Body::empty()).expect("a valid request");
    send(state, request).await
}

async fn post(state: &AppState, uri: &str, body: Value) -> (StatusCode, Value) {
    let request = Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .expect("a valid request");
    send(state, request).await
}

async fn send(state: &AppState, request: Request<Body>) -> (StatusCode, Value) {
    let response =
        server::router(state.clone()).oneshot(request).await.expect("the router answers");
    let status = response.status();
    let bytes = response.into_body().collect().await.expect("a body").to_bytes();
    let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, value)
}

async fn text(state: &AppState, uri: &str) -> (StatusCode, String) {
    let request = Request::builder().uri(uri).body(Body::empty()).expect("a valid request");
    let response =
        server::router(state.clone()).oneshot(request).await.expect("the router answers");
    let status = response.status();
    let bytes = response.into_body().collect().await.expect("a body").to_bytes();
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

/// Start a run and wait for it, or say why it never finished.
async fn run_to_completion(state: &AppState, request: Value) -> Value {
    let (status, started) = post(state, "/api/runs", request).await;
    assert_eq!(status, StatusCode::ACCEPTED, "a run starts with 202: {started}");
    let id = started["id"].as_str().expect("a run id").to_owned();
    assert_eq!(started["status"], "running");

    for _ in 0..600 {
        let (status, view) = get(state, &format!("/api/runs/{id}")).await;
        assert_eq!(status, StatusCode::OK);
        match view["status"].as_str() {
            Some("done") => return view,
            Some("failed") => panic!("the run failed: {}", view["error"]),
            _ => tokio::time::sleep(std::time::Duration::from_millis(50)).await,
        }
    }
    panic!("the run never finished");
}

/// Every key the TypeScript client reads off a key-bearing object.
fn keys(value: &Value) -> Vec<&str> {
    value.as_object().expect("an object").keys().map(String::as_str).collect()
}

#[tokio::test]
async fn health_and_info_describe_the_server() {
    let state = state();
    let (status, body) = get(&state, "/api/health").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "ok");

    let (status, info) = get(&state, "/api/info").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        keys(&info),
        ["version", "default_mock", "live_available", "max_days", "run_capacity"]
    );
    assert_eq!(info["default_mock"], true);
}

#[tokio::test]
async fn the_catalogue_is_served() {
    let state = state();
    let (status, prompts) = get(&state, "/api/prompts").await;
    assert_eq!(status, StatusCode::OK);
    let prompts = prompts.as_array().expect("a list of prompts");
    assert_eq!(prompts.len(), 6, "one standing instruction per primitive");
    assert_eq!(keys(&prompts[0]), ["primitive", "text"]);

    let (status, schemas) = get(&state, "/api/schemas").await;
    assert_eq!(status, StatusCode::OK);
    let schemas = schemas.as_object().expect("a schema per primitive output");
    assert!(schemas.contains_key("GateOut"), "got {:?}", schemas.keys().collect::<Vec<_>>());
}

#[tokio::test]
async fn a_forex_run_answers_with_the_shape_the_ui_reads() {
    let state = state();
    let view = run_to_completion(&state, json!({ "domain": "fx", "days": 6, "seed": 7 })).await;

    assert_eq!(view["spec"]["domain"], "fx");
    assert_eq!(view["spec"]["seed"], 7);
    assert_eq!(view["spec"]["mock"], true);
    assert!(view["calls"].as_u64().expect("a call count") > 0);

    let result = &view["result"];
    assert_eq!(keys(result), ["fx", "battery", "compliance", "cost"]);
    assert!(result["battery"].is_null(), "a forex run has no battery desk");

    let fx = &result["fx"];
    assert_eq!(
        keys(fx),
        [
            "decisions_judged",
            "ungated",
            "gated",
            "gate_distribution",
            "regime_accuracy",
            "interventions",
            "escalations",
            "failed_checks",
            "scorecard",
            "equity",
            "decisions",
            "replay",
        ]
    );
    assert_eq!(
        keys(&fx["ungated"]),
        ["trades", "pnl", "wins", "losses", "hit_rate", "max_drawdown", "notional", "return_bps"]
    );

    // Every named check is scored against the outcomes it flagged.
    let scorecard = fx["scorecard"].as_array().expect("a scorecard");
    assert_eq!(scorecard.len(), 3, "three named checks in the forex pipeline");
    assert!(scorecard.iter().all(|c| c["name"].is_string() && c["edge_bps"].is_number()));
}

#[tokio::test]
async fn a_decision_expands_without_leaking_the_generators_regime() {
    let state = state();
    let view = run_to_completion(&state, json!({ "domain": "fx", "days": 8 })).await;
    let id = view["id"].as_str().expect("a run id");
    assert!(view["result"]["fx"]["decisions_judged"].as_u64().expect("a count") > 0);

    let (status, detail) = get(&state, &format!("/api/runs/{id}/fx/decisions/0")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        keys(&detail),
        ["row", "features", "headlines", "regime", "checks", "risk", "gate", "ungated", "gated"]
    );

    // The row carries the generator's regime for scoring the classifier; the
    // features the judgment layer actually read must not.
    assert!(detail["row"]["true_regime"].is_string());
    let features = detail["features"].to_string();
    assert!(
        !features.contains("true_regime"),
        "the generator's regime must not reach the judged state: {features}"
    );

    let (status, _) = get(&state, &format!("/api/runs/{id}/fx/decisions/99999")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_battery_run_ranks_three_schedules_and_replays_a_notice() {
    let state = state();
    let view = run_to_completion(&state, json!({ "domain": "battery", "days": 6 })).await;
    let id = view["id"].as_str().expect("a run id").to_owned();
    let battery = &view["result"]["battery"];

    assert_eq!(battery["rank_distribution"].as_array().expect("a distribution").len(), 3);
    assert_eq!(battery["days_judged"], 6);

    let replay = &battery["replay"];
    assert!(replay["quiet"]["ranking"].as_array().expect("a ranking").len() == 3);
    assert!(replay["noticed"]["ranking"].as_array().expect("a ranking").len() == 3);
    assert!(replay["flipped"].is_boolean());

    let (status, detail) = get(&state, &format!("/api/runs/{id}/battery/days/0")).await;
    assert_eq!(status, StatusCode::OK);
    let schedules = detail["schedules"].as_array().expect("three schedules");
    assert_eq!(schedules.len(), 3);
    assert_eq!(schedules[0]["power_mw"].as_array().expect("hourly power").len(), 24);
    assert_eq!(schedules[0]["soc_mwh"].as_array().expect("hourly charge").len(), 25);
}

#[tokio::test]
async fn the_audit_log_is_paged_and_downloadable() {
    let state = state();
    let view = run_to_completion(&state, json!({ "domain": "fx", "days": 5 })).await;
    let id = view["id"].as_str().expect("a run id").to_owned();

    let (status, page) = get(&state, &format!("/api/runs/{id}/calls?limit=2")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(keys(&page), ["total", "offset", "calls"]);
    let calls = page["calls"].as_array().expect("a page of calls");
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0]["backend"], "mock");

    // One call in full: the state judged, the questions, the verdicts, the output.
    let (status, call) = get(&state, &format!("/api/runs/{id}/calls/0")).await;
    assert_eq!(status, StatusCode::OK);
    for field in ["state", "asks", "verdicts", "output", "primitive"] {
        assert!(call.get(field).is_some(), "the audit record carries {field}");
    }

    // The same log the CLI writes: one JSON object per line.
    let (status, jsonl) = text(&state, &format!("/api/runs/{id}/decisions.jsonl")).await;
    assert_eq!(status, StatusCode::OK);
    let lines: Vec<&str> = jsonl.lines().collect();
    assert_eq!(lines.len(), page["total"].as_u64().expect("a total") as usize);
    assert!(serde_json::from_str::<Value>(lines[0]).is_ok(), "every line parses on its own");
}

#[tokio::test]
async fn a_report_comes_back_as_markdown() {
    let state = state();
    let view = run_to_completion(&state, json!({ "domain": "battery", "days": 5 })).await;
    let id = view["id"].as_str().expect("a run id").to_owned();

    let (status, markdown) = text(&state, &format!("/api/runs/{id}/report")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        markdown.starts_with('#'),
        "a Markdown report: {}",
        &markdown[..40.min(markdown.len())]
    );

    let (status, _) = text(&state, &format!("/api/runs/{id}/report?domain=fx")).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "this run has no forex desk");
}

#[tokio::test]
async fn bad_requests_are_refused_rather_than_clamped() {
    let state = state();

    let (status, body) = post(&state, "/api/runs", json!({ "domain": "fx", "days": 0 })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "bad_request");

    let (status, _) = post(&state, "/api/runs", json!({ "domain": "fx", "days": 10_000 })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, _) = post(&state, "/api/runs", json!({ "domain": "cocoa" })).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "the domain is an enum, not a string");

    let (status, body) = get(&state, "/api/runs/nope").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"], "not_found");
}

#[tokio::test]
async fn runs_are_listed_and_can_be_forgotten() {
    let state = state();
    let view = run_to_completion(&state, json!({ "domain": "fx", "days": 4 })).await;
    let id = view["id"].as_str().expect("a run id").to_owned();

    let (status, runs) = get(&state, "/api/runs").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(runs.as_array().expect("a list").len(), 1);

    let request = Request::builder()
        .method("DELETE")
        .uri(format!("/api/runs/{id}"))
        .body(Body::empty())
        .expect("a valid request");
    let response =
        server::router(state.clone()).oneshot(request).await.expect("the router answers");
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let (status, _) = get(&state, &format!("/api/runs/{id}")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_run_is_a_function_of_its_seed() {
    let state = state();
    let first = run_to_completion(&state, json!({ "domain": "fx", "days": 6, "seed": 99 })).await;
    let second = run_to_completion(&state, json!({ "domain": "fx", "days": 6, "seed": 99 })).await;
    assert_eq!(first["result"]["fx"]["decisions"], second["result"]["fx"]["decisions"]);
    assert_ne!(first["id"], second["id"]);
}
