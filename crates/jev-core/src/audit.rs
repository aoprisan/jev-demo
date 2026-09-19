//! The `decisions.jsonl` audit log: every primitive call, its questions, its
//! answers and the typed output it produced, one JSON object per line.

use crate::client::CallRecord;
use crate::error::{JevError, Result};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Collects [`CallRecord`]s in memory and, optionally, appends them to a file.
#[derive(Debug, Default)]
pub struct Audit {
    inner: Mutex<AuditInner>,
}

#[derive(Debug, Default)]
struct AuditInner {
    records: Vec<CallRecord>,
    sink: Option<PathBuf>,
}

impl Audit {
    /// An in-memory-only audit log.
    pub fn new() -> Self {
        Self::default()
    }

    /// An audit log that also appends to `path`, truncating whatever was there.
    pub fn to_file(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir).map_err(|e| JevError::Audit(e.to_string()))?;
        }
        std::fs::File::create(&path).map_err(|e| JevError::Audit(e.to_string()))?;
        Ok(Self { inner: Mutex::new(AuditInner { records: Vec::new(), sink: Some(path) }) })
    }

    /// Record a call.
    pub fn push(&self, record: CallRecord) -> Result<()> {
        let mut inner = self.inner.lock().expect("audit mutex poisoned");
        if let Some(path) = inner.sink.clone() {
            let line =
                serde_json::to_string(&record).map_err(|e| JevError::Audit(e.to_string()))?;
            let mut f = std::fs::OpenOptions::new()
                .append(true)
                .open(&path)
                .map_err(|e| JevError::Audit(e.to_string()))?;
            writeln!(f, "{line}").map_err(|e| JevError::Audit(e.to_string()))?;
        }
        inner.records.push(record);
        Ok(())
    }

    /// Every record so far, in call order.
    pub fn records(&self) -> Vec<CallRecord> {
        self.inner.lock().expect("audit mutex poisoned").records.clone()
    }

    /// How many calls were made.
    pub fn len(&self) -> usize {
        self.inner.lock().expect("audit mutex poisoned").records.len()
    }

    /// Whether no calls were made.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Total tokens and total latency across every call.
    pub fn totals(&self) -> (u64, u64) {
        let inner = self.inner.lock().expect("audit mutex poisoned");
        inner.records.iter().fold((0, 0), |(t, l), r| (t + r.tokens(), l + r.latency_ms))
    }
}
