//! The run store: what a client can come back to.
//!
//! A run is started, polled while it judges, and read afterwards. Everything
//! lives in memory — a run is a function of its seed, so the way to reproduce
//! one is to ask for the same seed again rather than to persist the answer.

use crate::dto::{RunSpec, RunStatus, RunSummary};
use crate::run::Outcome;
use jev_core::Audit;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// Milliseconds since the Unix epoch.
pub fn now_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

/// Where a run has got to, and what it produced.
#[derive(Debug)]
enum State {
    Running,
    Done { outcome: Arc<Outcome>, finished_at_ms: u128 },
    Failed { error: String, finished_at_ms: u128 },
}

/// One run: its identity, its audit log and whatever it has produced so far.
#[derive(Debug)]
pub struct RunHandle {
    /// Server-assigned identifier.
    pub id: String,
    /// What was asked for, with defaults resolved.
    pub spec: RunSpec,
    /// When it started.
    pub started_at_ms: u128,
    /// Every typed call the run has made. Grows while the run is judging, which
    /// is what the progress figure reads.
    pub audit: Arc<Audit>,
    state: Mutex<State>,
}

impl RunHandle {
    /// The summary half, safe to poll.
    pub fn summary(&self) -> RunSummary {
        let state = self.state.lock().expect("run state mutex poisoned");
        let (status, finished_at_ms, error) = match &*state {
            State::Running => (RunStatus::Running, None, None),
            State::Done { finished_at_ms, .. } => (RunStatus::Done, Some(*finished_at_ms), None),
            State::Failed { error, finished_at_ms } => {
                (RunStatus::Failed, Some(*finished_at_ms), Some(error.clone()))
            }
        };
        RunSummary {
            id: self.id.clone(),
            spec: self.spec,
            status,
            calls: self.audit.len(),
            started_at_ms: self.started_at_ms,
            finished_at_ms,
            error,
        }
    }

    /// What the run produced, once it is done.
    pub fn outcome(&self) -> Option<Arc<Outcome>> {
        match &*self.state.lock().expect("run state mutex poisoned") {
            State::Done { outcome, .. } => Some(Arc::clone(outcome)),
            _ => None,
        }
    }

    /// Whether the run has stopped, either way.
    pub fn finished(&self) -> bool {
        !matches!(*self.state.lock().expect("run state mutex poisoned"), State::Running)
    }

    /// Record a successful finish.
    pub fn complete(&self, outcome: Outcome) {
        *self.state.lock().expect("run state mutex poisoned") =
            State::Done { outcome: Arc::new(outcome), finished_at_ms: now_ms() };
    }

    /// Record a failure.
    pub fn fail(&self, error: impl Into<String>) {
        *self.state.lock().expect("run state mutex poisoned") =
            State::Failed { error: error.into(), finished_at_ms: now_ms() };
    }
}

/// The runs this process is holding, newest last.
#[derive(Debug)]
pub struct RunStore {
    runs: Mutex<VecDeque<Arc<RunHandle>>>,
    capacity: usize,
    counter: AtomicU64,
}

impl RunStore {
    /// A store keeping at most `capacity` runs.
    pub fn new(capacity: usize) -> Self {
        Self {
            runs: Mutex::new(VecDeque::new()),
            capacity: capacity.max(1),
            counter: AtomicU64::new(1),
        }
    }

    /// Register a new run, evicting the oldest finished one if the store is full.
    pub fn create(&self, spec: RunSpec) -> Arc<RunHandle> {
        let n = self.counter.fetch_add(1, Ordering::Relaxed);
        let handle = Arc::new(RunHandle {
            id: format!("{}-{n}", spec.domain.as_str()),
            spec,
            started_at_ms: now_ms(),
            audit: Arc::new(Audit::new()),
            state: Mutex::new(State::Running),
        });

        let mut runs = self.runs.lock().expect("run store mutex poisoned");
        runs.push_back(Arc::clone(&handle));
        // A running job still holds its handle, so evicting it would only drop
        // the client's way of reading the answer. Older finished runs go first.
        while runs.len() > self.capacity {
            let Some(index) = runs.iter().position(|r| r.finished()) else { break };
            runs.remove(index);
        }
        handle
    }

    /// One run, by id.
    pub fn get(&self, id: &str) -> Option<Arc<RunHandle>> {
        self.runs
            .lock()
            .expect("run store mutex poisoned")
            .iter()
            .find(|r| r.id == id)
            .map(Arc::clone)
    }

    /// Every run, newest first.
    pub fn list(&self) -> Vec<Arc<RunHandle>> {
        let runs = self.runs.lock().expect("run store mutex poisoned");
        runs.iter().rev().map(Arc::clone).collect()
    }

    /// Forget one run. Returns whether it was there.
    pub fn remove(&self, id: &str) -> bool {
        let mut runs = self.runs.lock().expect("run store mutex poisoned");
        match runs.iter().position(|r| r.id == id) {
            Some(index) => {
                runs.remove(index);
                true
            }
            None => false,
        }
    }
}
