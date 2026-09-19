//! Terminal and Markdown reporting for the jev-desk demo.
//!
//! Every report shows the same four things: what the solver proposed, what the
//! judgment layer did with it, what each book ended up with, and which
//! individual decisions mattered most. The Explain primitive then writes the
//! same run up for two different readers.

#![warn(missing_docs)]

pub mod battery_report;
pub mod cost;
pub mod explain;
pub mod fmt;
pub mod fx_report;

use jev_core::{Audit, JevError, Result};
use std::path::{Path, PathBuf};

/// One domain's finished report.
#[derive(Debug, Clone)]
pub struct DomainReport {
    /// Short name, used for the output directory.
    pub name: String,
    /// What to print.
    pub terminal: String,
    /// What to write to `report.md`.
    pub markdown: String,
}

impl DomainReport {
    /// Write `report.md` under `dir`, creating it if needed.
    ///
    /// The audit log is written as the run proceeds, by [`Audit::to_file`]; this
    /// only adds the human-readable half.
    pub fn write(&self, dir: impl AsRef<Path>) -> Result<PathBuf> {
        let dir = dir.as_ref();
        std::fs::create_dir_all(dir).map_err(|e| JevError::Audit(e.to_string()))?;
        let path = dir.join("report.md");
        std::fs::write(&path, &self.markdown).map_err(|e| JevError::Audit(e.to_string()))?;
        Ok(path)
    }
}

/// Where a domain's output goes.
pub fn out_dir(root: impl AsRef<Path>, domain: &str) -> PathBuf {
    root.as_ref().join(domain)
}

/// Open an audit log writing to `<root>/<domain>/decisions.jsonl`.
pub fn audit_for(root: impl AsRef<Path>, domain: &str) -> Result<std::sync::Arc<Audit>> {
    let path = out_dir(root, domain).join("decisions.jsonl");
    Ok(std::sync::Arc::new(Audit::to_file(path)?))
}
