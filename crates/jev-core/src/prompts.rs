//! Each primitive's standing guidance, kept as JSON under `prompts/`.
//!
//! System One takes a question's `instructions` as a string *or an object*, and
//! the object form is the one that reads well: named parts such as `what`,
//! `not_for` and `evidence` instead of a paragraph. Each file holds one object
//! per question part the primitive asks (`gate.json` has `action` and
//! `size_factor`; `score.json` has `level` and `driver`), and
//! [`instructions`] merges the caller's question into it. The guidance is
//! identical across domains; domain framing is a named field of the state, and
//! is never an instruction.

use crate::client::Primitive;
use serde_json::{Map, Value};
use std::sync::LazyLock;

const GATE: &str = include_str!("../prompts/gate.json");
const CLASSIFY: &str = include_str!("../prompts/classify.json");
const SCORE: &str = include_str!("../prompts/score.json");
const CHECK: &str = include_str!("../prompts/check.json");
const RANK: &str = include_str!("../prompts/rank.json");
const EXPLAIN: &str = include_str!("../prompts/explain.json");

static PARSED: LazyLock<[(Primitive, Value); 6]> = LazyLock::new(|| {
    let parse = |p: Primitive, text: &str| {
        let value: Value =
            serde_json::from_str(text).unwrap_or_else(|e| panic!("prompts/{p}.json: {e}"));
        assert!(value.is_object(), "prompts/{p}.json must be an object keyed by question part");
        (p, value)
    };
    [
        parse(Primitive::Classify, CLASSIFY),
        parse(Primitive::Check, CHECK),
        parse(Primitive::Score, SCORE),
        parse(Primitive::Rank, RANK),
        parse(Primitive::Gate, GATE),
        parse(Primitive::Explain, EXPLAIN),
    ]
});

/// The standing guidance for `primitive`, as the JSON text in `prompts/`.
pub fn for_primitive(primitive: Primitive) -> &'static str {
    match primitive {
        Primitive::Gate => GATE,
        Primitive::Classify => CLASSIFY,
        Primitive::Score => SCORE,
        Primitive::Check => CHECK,
        Primitive::Rank => RANK,
        Primitive::Explain => EXPLAIN,
        Primitive::Batch => "{}",
    }
}

/// Every primitive's guidance, for `jev-desk prompts`.
pub fn all() -> [(Primitive, &'static str); 6] {
    [
        (Primitive::Classify, CLASSIFY),
        (Primitive::Check, CHECK),
        (Primitive::Score, SCORE),
        (Primitive::Rank, RANK),
        (Primitive::Gate, GATE),
        (Primitive::Explain, EXPLAIN),
    ]
}

/// The guidance object for one question `part` of `primitive`, if the file has one.
pub fn guidance(primitive: Primitive, part: &str) -> Option<&'static Map<String, Value>> {
    PARSED.iter().find(|(p, _)| *p == primitive)?.1.get(part)?.as_object()
}

/// A question's `instructions`: `{"question": …}` merged with the part's guidance.
///
/// `extra` adds question-specific named fields (a candidate being rated, the
/// audience being written for); it is applied last, so it can override a
/// guidance field if it ever needs to.
pub fn instructions(
    primitive: Primitive,
    part: &str,
    question: impl Into<Value>,
    extra: impl IntoIterator<Item = (&'static str, Value)>,
) -> Value {
    let mut out = Map::new();
    out.insert("question".to_owned(), question.into());
    if let Some(g) = guidance(primitive, part) {
        for (k, v) in g {
            out.insert(k.clone(), v.clone());
        }
    }
    for (k, v) in extra {
        out.insert(k.to_owned(), v);
    }
    Value::Object(out)
}
