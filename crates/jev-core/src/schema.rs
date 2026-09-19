//! JSON Schemas for the primitive outputs.
//!
//! The schema is the contract between the judgment layer and everything
//! downstream. These are derived from the same structs the primitives return,
//! and each output type's `validate()` enforces the same bounds in code, so a
//! value that would not validate against the schema never reaches a caller.

use crate::primitives::{CheckOut, ExplainOut, GateOut, ScoreOut};
use schemars::schema_for;

/// Every non-generic primitive output, as `(name, schema JSON)`.
///
/// `Classify` and `Rank` are generic over the caller's own enum and candidate
/// id, so their schemas are generated per domain rather than here.
pub fn all() -> Vec<(&'static str, serde_json::Value)> {
    vec![
        ("GateOut", to_value(schema_for!(GateOut))),
        ("ScoreOut", to_value(schema_for!(ScoreOut))),
        ("CheckOut", to_value(schema_for!(CheckOut))),
        ("ExplainOut", to_value(schema_for!(ExplainOut))),
    ]
}

fn to_value(schema: schemars::schema::RootSchema) -> serde_json::Value {
    serde_json::to_value(schema).unwrap_or(serde_json::Value::Null)
}
