//! The cost line every report ends with.
//!
//! Calls, tokens and latency are measured. The dollar figure is an estimate at
//! [`Rates::from_env`] — the demo's mock backend bills nothing, and System One's
//! price list is not in this repository — so it is always printed as "est." and
//! always says which rates produced it.

use crate::fmt::thousands;
use jev_core::{CostLedger, Jev, Rates};

/// A dollar figure, at a precision that suits its size.
pub fn usd(value: f64) -> String {
    if value >= 100.0 {
        format!("${}", thousands(value))
    } else if value >= 1.0 {
        format!("${value:.2}")
    } else {
        format!("${value:.4}")
    }
}

/// What the run has cost so far, at the rates the environment names.
pub fn ledger(jev: &Jev) -> CostLedger {
    jev.audit().ledger(Rates::from_env())
}

/// Where the rates came from, for the line that quotes them.
fn provenance(rates: Rates) -> String {
    let at = format!(
        "${:.2}/${:.2} per Mtok in/out",
        rates.input_usd_per_mtok, rates.output_usd_per_mtok
    );
    if rates.assumed() {
        format!("{at}, assumed")
    } else {
        at
    }
}

/// The terminal footer: what was spent, and what one decision cost.
pub fn terminal_line(jev: &Jev) -> String {
    let ledger = ledger(jev);
    let total = ledger.total();
    format!(
        "{} calls, {} tokens, {} ms, est. {} ({} per decision over {} decisions; {})\n",
        total.calls,
        thousands(total.tokens() as f64),
        total.latency_ms,
        usd(total.usd),
        usd(ledger.usd_per_decision()),
        ledger.decisions(),
        provenance(ledger.rates()),
    )
}

/// The Markdown footer: the same figures, plus where to find the calls.
pub fn markdown_line(jev: &Jev) -> String {
    let ledger = ledger(jev);
    let total = ledger.total();
    format!(
        "\n---\n\n{} typed calls, {} tokens, backend `{}`. Estimated cost {} — {} per \
         decision across {} decisions, at {}. Every call and its output is in \
         `decisions.jsonl`, each one tagged with the decision it judged.\n",
        total.calls,
        thousands(total.tokens() as f64),
        jev.backend(),
        usd(total.usd),
        usd(ledger.usd_per_decision()),
        ledger.decisions(),
        provenance(ledger.rates()),
    )
}
