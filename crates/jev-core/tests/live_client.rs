//! The live backend's translation layer, against a stub System One server.
//!
//! What is under test is the mapping in both directions: our [`Ask`] into the
//! SDK's question types and onto the wire, and the API's answers back into our
//! [`Verdict`]s. The primitives above the seam are unchanged by which backend
//! answers, so covering the translation covers the live path.

mod common;

use common::TestInput;
use jev_core::{
    Action, Check, CheckSpec, Classify, ClassifySpec, Gate, GateSpec, Jev, JevError, Label,
    LiveJev, Score, ScoreSpec,
};
use serde_json::{json, Value};
use std::sync::Arc;
use typesafe::Client;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
enum Regime {
    Calm,
    Rough,
}

impl Label for Regime {
    fn labels() -> &'static [Self] {
        &[Regime::Calm, Regime::Rough]
    }
    fn name(&self) -> &'static str {
        match self {
            Regime::Calm => "calm",
            Regime::Rough => "rough",
        }
    }
    fn describe(&self) -> &'static str {
        match self {
            Regime::Calm => "Quiet conditions.",
            Regime::Rough => "Disturbed conditions.",
        }
    }
}

/// A Jev handle pointed at `server`, answering every call with `body`.
async fn stub(server: &MockServer, body: Value) -> Jev {
    Mock::given(method("POST"))
        .and(path("/v1/systemone"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(server)
        .await;
    let client = Client::builder()
        .api_key("test-key")
        .base_url(server.uri())
        .build()
        .expect("the stub client builds");
    Jev::new(Arc::new(LiveJev::new(client)))
}

/// The one request the server received, as JSON.
async fn sent(server: &MockServer) -> Value {
    let requests = server.received_requests().await.expect("requests are recorded");
    assert_eq!(requests.len(), 1, "exactly one call should have gone out");
    body_of(&requests[0])
}

fn body_of(request: &Request) -> Value {
    serde_json::from_slice(&request.body).expect("the body is JSON")
}

fn input() -> TestInput {
    TestInput::new(json!({ "hours_to_event": 2.0 }))
}

#[tokio::test]
async fn a_choice_and_a_score_reach_the_wire_in_the_shape_the_api_expects() {
    let server = MockServer::start().await;
    let jev = stub(
        &server,
        json!({
            "model": "jev-latest",
            "usage": { "input_tokens": 410, "output_tokens": 22 },
            "answers": {
                "action": {
                    "type": "choice",
                    "choice": "reduce",
                    "probabilities": { "execute": 0.2, "reduce": 0.6, "hold": 0.15, "escalate": 0.05 },
                    "confidence": 0.55
                },
                "size_factor": {
                    "type": "score",
                    "score": 2.0,
                    "legend": { "0": "none", "1": "a quarter", "2": "half", "3": "most", "4": "all" },
                    "probabilities": { "0": 0.0, "1": 0.1, "2": 0.8, "3": 0.1, "4": 0.0 },
                    "confidence": 0.8
                }
            }
        }),
    )
    .await;

    let out = Gate::gate(&jev, &input(), &GateSpec::new("EURUSD long", "Go?")).await.unwrap();
    assert_eq!(out.action, Action::Reduce);
    // Level 2 of a five-level rubric is half.
    assert!((out.size_factor - 0.5).abs() < 1e-6, "{}", out.size_factor);

    let body = sent(&server).await;

    // The state carries the primitive's instructions, the domain context block
    // and the decision state, so a live model sees everything the mock does.
    let state = &body["state"];
    assert!(state["instructions"].as_str().unwrap().contains("Gate"));
    assert_eq!(state["context"], json!("test domain"));
    assert_eq!(state["state"]["input"]["features"]["hours_to_event"], json!(2.0));
    assert!(state["state"]["prior_judgments"].is_array());

    // The action is a choice offering exactly the four actions, described.
    let action = &body["questions"]["action"];
    assert_eq!(action["type"], json!("choice"));
    let options = action["criteria"].as_object().unwrap();
    assert_eq!(options.len(), 4);
    for name in ["execute", "reduce", "hold", "escalate"] {
        assert!(options[name].as_str().is_some_and(|d| !d.is_empty()), "{name} undescribed");
    }

    // The size rubric is a score with its levels in order.
    let size = &body["questions"]["size_factor"];
    assert_eq!(size["type"], json!("score"));
    assert_eq!(size["criteria"].as_array().unwrap().len(), 5);
}

#[tokio::test]
async fn nouls_carry_their_yes_and_no_criteria() {
    let server = MockServer::start().await;
    let jev = stub(
        &server,
        json!({
            "model": "jev-latest",
            "answers": {
                "stop_sane": { "type": "noul", "noul": 0.91 },
                "signal_valid": { "type": "noul", "noul": 0.2 }
            }
        }),
    )
    .await;

    let spec = CheckSpec::new("the trade")
        .item("stop_sane", "The stop is sane.", "wide enough", "too tight")
        .item("signal_valid", "The signal is valid.", "valid here", "not here");
    let out = Check::check(&jev, &input(), &spec).await.unwrap();

    assert!(out.get("stop_sane").unwrap().ok);
    assert!(!out.get("signal_valid").unwrap().ok);
    assert!((out.get("stop_sane").unwrap().p - 0.91).abs() < 1e-6);

    let body = sent(&server).await;
    let question = &body["questions"]["stop_sane"];
    assert_eq!(question["type"], json!("noul"));
    assert_eq!(question["instructions"], json!("The stop is sane."));
    assert_eq!(question["criteria"]["true"], json!("wide enough"));
    assert_eq!(question["criteria"]["false"], json!("too tight"));
}

#[tokio::test]
async fn usage_and_the_model_are_carried_into_the_audit_record() {
    let server = MockServer::start().await;
    let jev = stub(
        &server,
        json!({
            "model": "jev-2",
            "usage": { "input_tokens": 512, "output_tokens": 31 },
            "answers": {
                "label": {
                    "type": "choice",
                    "choice": "rough",
                    "probabilities": { "calm": 0.3, "rough": 0.7 },
                    "confidence": 0.4
                }
            }
        }),
    )
    .await;

    let _: jev_core::ClassifyOut<Regime> =
        Classify::<_, Regime>::classify(&jev, &input(), &ClassifySpec::new("s", "Which?"))
            .await
            .unwrap();

    let records = jev.audit().records();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].backend, "jev");
    assert_eq!(records[0].model, "jev-2");
    assert_eq!(records[0].input_tokens, Some(512));
    assert_eq!(records[0].output_tokens, Some(31));
}

#[tokio::test]
async fn a_score_maps_onto_the_rubric_we_sent_not_the_legend_that_came_back() {
    // The API echoes the legend it was given; the level count that matters for
    // mapping a score onto 0..=100 is the rubric this call actually sent.
    let server = MockServer::start().await;
    let jev = stub(
        &server,
        json!({
            "model": "m",
            "answers": {
                "level": {
                    "type": "score",
                    "score": 1.5,
                    "legend": { "0": "a", "1": "b" },
                    "probabilities": { "0": 0.5, "1": 0.5 },
                    "confidence": 0.5
                }
            }
        }),
    )
    .await;

    let spec = ScoreSpec::new("risk", "How risky?", ["none", "low", "mid", "high", "max"]);
    let out = Score::score(&jev, &input(), &spec).await.unwrap();
    // 1.5 of a five-level rubric is 37.5, not 150 of a two-level one.
    assert_eq!(out.score, 38, "score {} should be 1.5/4 rounded", out.score);
}

#[tokio::test]
async fn an_api_error_is_a_transport_error_not_a_permissive_default() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/systemone"))
        .respond_with(ResponseTemplate::new(500).set_body_string("upstream on fire"))
        .mount(&server)
        .await;
    let client = Client::builder()
        .api_key("k")
        .base_url(server.uri())
        .retry(typesafe::RetryPolicy::none())
        .build()
        .unwrap();
    let jev = Jev::new(Arc::new(LiveJev::new(client)));

    let err = Gate::gate(&jev, &input(), &GateSpec::new("x", "go?")).await.unwrap_err();
    assert!(matches!(err, JevError::Transport(_)), "{err:?}");
    assert!(jev.audit().is_empty(), "a failed call records no decision");
}

#[tokio::test]
async fn a_malformed_answer_is_an_error_rather_than_a_guess() {
    let server = MockServer::start().await;
    let jev = stub(
        &server,
        json!({
            "model": "m",
            "answers": {
                // A noul where the gate asked for a choice.
                "action": { "type": "noul", "noul": 0.9 },
                "size_factor": {
                    "type": "score", "score": 4.0,
                    "legend": {}, "probabilities": { "4": 1.0 }, "confidence": 1.0
                }
            }
        }),
    )
    .await;

    let err = Gate::gate(&jev, &input(), &GateSpec::new("x", "go?")).await.unwrap_err();
    assert!(matches!(err, JevError::AnswerKind { want: "choice", .. }), "{err:?}");
}

#[tokio::test]
async fn an_answer_the_primitive_did_not_ask_for_is_a_missing_answer() {
    let server = MockServer::start().await;
    let jev = stub(
        &server,
        json!({
            "model": "m",
            "answers": {
                "something_else": {
                    "type": "choice", "choice": "a",
                    "probabilities": { "a": 1.0 }, "confidence": 1.0
                }
            }
        }),
    )
    .await;

    let err = Gate::gate(&jev, &input(), &GateSpec::new("x", "go?")).await.unwrap_err();
    assert!(matches!(err, JevError::MissingAnswer { .. }), "{err:?}");
}
