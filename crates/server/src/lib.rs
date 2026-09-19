//! `jev-desk` over HTTP.
//!
//! The same runs the CLI prints, served as a typed JSON API for a browser to
//! read. The split the workspace is built on holds here too: this crate owns no
//! numbers and makes no judgments. It starts a run, keeps what the run
//! produced, and projects it onto the wire types in [`dto`] — which are the
//! contract the TypeScript client mirrors, field for field.

#![warn(missing_docs)]

pub mod dto;
pub mod error;
pub mod project;
pub mod routes;
pub mod run;
pub mod store;

use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Router;
use std::path::PathBuf;
use std::sync::Arc;
use store::RunStore;
use tower_http::compression::CompressionLayer;
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::trace::TraceLayer;

/// How the server is configured.
#[derive(Debug, Clone)]
pub struct Config {
    /// Whether a request that does not say which backend it wants gets the
    /// offline one. The CLI's `--mock`, as a default rather than a flag.
    pub default_mock: bool,
    /// The largest `days` a request may ask for.
    pub max_days: u32,
    /// How many runs are kept before the oldest finished one is dropped.
    pub run_capacity: usize,
    /// The built TypeScript UI, when there is one to serve.
    pub ui_dir: Option<PathBuf>,
    /// Whether to answer cross-origin requests, for the Vite dev server.
    pub permissive_cors: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            default_mock: true,
            max_days: 365,
            run_capacity: 16,
            ui_dir: None,
            permissive_cors: false,
        }
    }
}

/// What every handler shares.
#[derive(Debug, Clone)]
pub struct AppState {
    /// The runs this process is holding.
    pub runs: Arc<RunStore>,
    /// How the server is configured.
    pub config: Arc<Config>,
}

impl AppState {
    /// A state with an empty run store.
    pub fn new(config: Config) -> Self {
        let runs = Arc::new(RunStore::new(config.run_capacity));
        Self { runs, config: Arc::new(config) }
    }
}

/// The whole application: the API, and the UI when one is built.
pub fn router(state: AppState) -> Router {
    // Every API answer is the state of a run *now*; a poll that comes back
    // from a cache is a lie. `no-store` keeps browsers, proxies and any
    // service worker another app left on this origin from replaying one.
    let api = routes::api().layer(SetResponseHeaderLayer::overriding(
        header::CACHE_CONTROL,
        HeaderValue::from_static("no-store"),
    ));
    let mut app = Router::new().nest("/api", api);

    if let Some(dir) = state.config.ui_dir.clone() {
        // A single-page app: anything the API did not claim falls back to
        // `index.html` so a deep link survives a reload.
        let index = dir.join("index.html");
        app = app.fallback_service(ServeDir::new(dir).fallback(ServeFile::new(index)));
    } else {
        app = app.fallback(no_ui);
    }

    if state.config.permissive_cors {
        app = app.layer(CorsLayer::permissive());
    }

    app.layer(CompressionLayer::new()).layer(TraceLayer::new_for_http()).with_state(state)
}

/// What a browser gets when the UI has not been built.
async fn no_ui() -> Response {
    (
        StatusCode::NOT_FOUND,
        [(axum::http::header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        "jev-desk: the API is at /api, but no built UI was found.\n\n\
         Build it:\n\n    just ui-build\n\n\
         or run the Vite dev server against this process:\n\n    just ui-dev\n",
    )
        .into_response()
}
