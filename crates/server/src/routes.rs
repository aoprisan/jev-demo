//! The HTTP surface.
//!
//! A run is started with one POST, polled while it judges, and read afterwards
//! from endpoints that project it: the summary, one decision in full, one page
//! of the audit log. Nothing is computed twice — the run is judged once and
//! every endpoint reads the same outcome.

use crate::dto::*;
use crate::error::{ApiError, ApiResult};
use crate::project;
use crate::run;
use crate::store::RunHandle;
use crate::AppState;
use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get};
use axum::{Json, Router};
use serde::Deserialize;
use std::sync::Arc;

/// How many calls one page of the audit log holds by default.
const DEFAULT_PAGE: usize = 100;
/// The most it can hold.
const MAX_PAGE: usize = 1000;
/// Days a run generates when the request does not say. The CLI's default.
const DEFAULT_DAYS: u32 = 90;

/// Everything under `/api`.
pub fn api() -> Router<AppState> {
    Router::new()
        .route("/health", get(health))
        .route("/info", get(info))
        .route("/prompts", get(prompts))
        .route("/schemas", get(schemas))
        .route("/runs", get(list_runs).post(start_run))
        .route("/runs/{id}", get(get_run))
        .route("/runs/{id}", delete(delete_run))
        .route("/runs/{id}/report", get(report))
        .route("/runs/{id}/decisions.jsonl", get(decisions_jsonl))
        .route("/runs/{id}/calls", get(calls))
        .route("/runs/{id}/calls/{index}", get(call))
        .route("/runs/{id}/calls/{index}/request", get(call_request))
        .route("/runs/{id}/fx/decisions/{index}", get(fx_decision))
        .route("/runs/{id}/battery/days/{day}", get(battery_day))
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "status": "ok" }))
}

async fn info(State(state): State<AppState>) -> Json<ServerInfo> {
    Json(ServerInfo {
        version: env!("CARGO_PKG_VERSION").to_owned(),
        default_mock: state.config.default_mock,
        live_available: live_key_present(),
        max_days: state.config.max_days,
        run_capacity: state.config.run_capacity,
    })
}

/// Whether a live run is possible at all.
fn live_key_present() -> bool {
    std::env::var_os("TYPESAFE_API_KEY").is_some()
}

async fn prompts() -> Json<Vec<PromptView>> {
    Json(
        jev_core::prompts::all()
            .into_iter()
            .map(|(primitive, text)| PromptView {
                primitive: primitive.as_str().to_owned(),
                text: text.to_owned(),
            })
            .collect(),
    )
}

async fn schemas() -> Json<serde_json::Value> {
    let mut all = serde_json::Map::new();
    for (name, schema) in jev_core::schema::all() {
        all.insert(name.to_owned(), schema);
    }
    Json(serde_json::Value::Object(all))
}

// ---------------------------------------------------------------------------
// Runs
// ---------------------------------------------------------------------------

async fn list_runs(State(state): State<AppState>) -> Json<Vec<RunSummary>> {
    Json(state.runs.list().iter().map(|r| r.summary()).collect())
}

/// Start a run. Answers immediately with `202` and a `running` summary; the
/// judging happens on a task, and the client polls `GET /api/runs/{id}`.
async fn start_run(
    State(state): State<AppState>,
    Json(request): Json<RunRequest>,
) -> ApiResult<Response> {
    let spec = resolve(&request, &state)?;
    if !spec.mock && !live_key_present() {
        return Err(ApiError::BadRequest(
            "no TYPESAFE_API_KEY in the server's environment; set one to run against the \
             System One API, or ask for mock: true to use the offline rule-based backend"
                .to_owned(),
        ));
    }

    let handle = state.runs.create(spec);
    let jev = jev_core::connect(spec.mock, Arc::clone(&handle.audit))?;

    let task = Arc::clone(&handle);
    tokio::spawn(async move {
        match run::execute(spec, &jev).await {
            Ok(outcome) => task.complete(outcome),
            Err(e) => task.fail(e.to_string()),
        }
    });

    let body = RunView { summary: handle.summary(), result: None };
    Ok((StatusCode::ACCEPTED, Json(body)).into_response())
}

/// Fill in the defaults and refuse anything out of bounds.
///
/// The bounds are the point: `days` decides how much work one request can ask
/// the process to do, and a live run's call count follows directly from it.
fn resolve(request: &RunRequest, state: &AppState) -> ApiResult<RunSpec> {
    let days = request.days.unwrap_or(DEFAULT_DAYS);
    if days == 0 || days > state.config.max_days {
        return Err(ApiError::BadRequest(format!(
            "days must be between 1 and {}",
            state.config.max_days
        )));
    }
    if let Some(limit) = request.limit {
        if limit == 0 {
            return Err(ApiError::BadRequest("limit must be at least 1".to_owned()));
        }
    }
    Ok(RunSpec {
        domain: request.domain,
        seed: request.seed.unwrap_or(synth::DEFAULT_SEED),
        days,
        limit: request.limit,
        mock: request.mock.unwrap_or(state.config.default_mock),
    })
}

fn find(state: &AppState, id: &str) -> ApiResult<Arc<RunHandle>> {
    state.runs.get(id).ok_or_else(|| ApiError::NotFound(format!("no run {id}")))
}

/// What the run's calls cost, at the rates the server's environment names.
///
/// The rates are read per response rather than cached, so a desk can set its
/// own prices without restarting the server.
fn ledger(handle: &RunHandle) -> jev_core::CostLedger {
    handle.audit.ledger(jev_core::Rates::from_env())
}

/// A finished run, or a `not_found` if it is still judging.
fn finished(handle: &RunHandle) -> ApiResult<Arc<run::Outcome>> {
    handle
        .outcome()
        .ok_or_else(|| ApiError::NotFound(format!("run {} has not finished judging", handle.id)))
}

async fn get_run(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<RunView>> {
    let handle = find(&state, &id)?;
    let summary = handle.summary();
    let result = handle.outcome().map(|outcome| project::run_result(&outcome, &ledger(&handle)));
    Ok(Json(RunView { summary, result }))
}

async fn delete_run(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    if state.runs.remove(&id) {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::NotFound(format!("no run {id}")))
    }
}

/// Which desk's report to serve.
#[derive(Debug, Deserialize)]
struct ReportQuery {
    domain: Option<Domain>,
}

/// One desk's `report.md`, byte for byte what the CLI writes to disk.
async fn report(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<ReportQuery>,
) -> ApiResult<Response> {
    let handle = find(&state, &id)?;
    let outcome = finished(&handle)?;
    let wanted = query.domain.unwrap_or(handle.spec.domain);
    let markdown = match wanted {
        Domain::Fx => outcome.fx.as_ref().map(|r| r.report.markdown.clone()),
        Domain::Battery => outcome.battery.as_ref().map(|r| r.report.markdown.clone()),
        // `all` ran both, so it has no single report; the client picks one.
        Domain::All => outcome
            .fx
            .as_ref()
            .map(|r| r.report.markdown.clone())
            .or_else(|| outcome.battery.as_ref().map(|r| r.report.markdown.clone())),
    };
    let markdown = markdown
        .ok_or_else(|| ApiError::NotFound(format!("run {id} has no {} report", wanted.as_str())))?;
    Ok(([(header::CONTENT_TYPE, "text/markdown; charset=utf-8")], markdown).into_response())
}

/// The audit log exactly as the CLI writes it: one JSON object per line.
async fn decisions_jsonl(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Response> {
    let handle = find(&state, &id)?;
    let mut body = String::new();
    for record in handle.audit.records() {
        let line = serde_json::to_string(&record)
            .map_err(|e| ApiError::Internal(format!("serialising the audit log: {e}")))?;
        body.push_str(&line);
        body.push('\n');
    }
    Ok((
        [
            (header::CONTENT_TYPE, "application/x-ndjson".to_owned()),
            (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{id}-decisions.jsonl\"")),
        ],
        body,
    )
        .into_response())
}

/// Where a page of the audit log starts and how long it is.
#[derive(Debug, Deserialize)]
struct PageQuery {
    offset: Option<usize>,
    limit: Option<usize>,
}

async fn calls(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(page): Query<PageQuery>,
) -> ApiResult<Json<CallPage>> {
    let handle = find(&state, &id)?;
    let records = handle.audit.records();
    let limit = page.limit.unwrap_or(DEFAULT_PAGE).clamp(1, MAX_PAGE);
    Ok(Json(project::call_page(&records, page.offset.unwrap_or(0), limit)))
}

/// One call in full: the state that was judged, the questions, the verdicts and
/// the typed output. Deliberately the raw record — it is the audit artefact.
async fn call(
    State(state): State<AppState>,
    Path((id, index)): Path<(String, usize)>,
) -> ApiResult<Json<jev_core::CallRecord>> {
    let handle = find(&state, &id)?;
    let records = handle.audit.records();
    records
        .into_iter()
        .nth(index)
        .map(Json)
        .ok_or_else(|| ApiError::NotFound(format!("run {id} has no call {index}")))
}

/// The `POST /v1/systemone` body for one call, byte for byte what the live
/// client sends: the state, the model and the questions in the SDK's wire
/// shape. For a mock call it is the body a live run would have sent.
async fn call_request(
    State(state): State<AppState>,
    Path((id, index)): Path<(String, usize)>,
) -> ApiResult<Json<CallRequestView>> {
    let handle = find(&state, &id)?;
    let record = handle
        .audit
        .records()
        .into_iter()
        .nth(index)
        .ok_or_else(|| ApiError::NotFound(format!("run {id} has no call {index}")))?;
    let live = record.backend == "jev";
    let model = if live { record.model.clone() } else { jev_core::live::DEFAULT_MODEL.to_owned() };
    let call =
        jev_core::JevCall { primitive: record.primitive, state: record.state, asks: record.asks };
    Ok(Json(CallRequestView {
        method: "POST".to_owned(),
        url: jev_core::live::ENDPOINT.to_owned(),
        sent: live,
        body: jev_core::live::wire_request(&call, &model),
    }))
}

async fn fx_decision(
    State(state): State<AppState>,
    Path((id, index)): Path<(String, usize)>,
) -> ApiResult<Json<FxDecisionDetail>> {
    let handle = find(&state, &id)?;
    let outcome = finished(&handle)?;
    let fx_run = outcome
        .fx
        .as_ref()
        .ok_or_else(|| ApiError::NotFound(format!("run {id} has no forex desk")))?;
    project::fx_detail(fx_run, index, &ledger(&handle))
        .map(Json)
        .ok_or_else(|| ApiError::NotFound(format!("run {id} has no decision {index}")))
}

async fn battery_day(
    State(state): State<AppState>,
    Path((id, day)): Path<(String, u32)>,
) -> ApiResult<Json<BatteryDayDetail>> {
    let handle = find(&state, &id)?;
    let outcome = finished(&handle)?;
    let battery_run = outcome
        .battery
        .as_ref()
        .ok_or_else(|| ApiError::NotFound(format!("run {id} has no battery desk")))?;
    project::battery_detail(battery_run, day, &ledger(&handle))
        .map(Json)
        .ok_or_else(|| ApiError::NotFound(format!("run {id} has no day {day}")))
}
