//! Each primitive's standing instructions, kept as Markdown under `prompts/`.
//!
//! Domain-specific framing is never a separate prompt: it is injected into the
//! call as a context block, so the primitive's own instructions are identical
//! across forex, battery and anything added later.

use crate::client::Primitive;

const GATE: &str = include_str!("../prompts/gate.md");
const CLASSIFY: &str = include_str!("../prompts/classify.md");
const SCORE: &str = include_str!("../prompts/score.md");
const CHECK: &str = include_str!("../prompts/check.md");
const RANK: &str = include_str!("../prompts/rank.md");
const EXPLAIN: &str = include_str!("../prompts/explain.md");

/// The standing instructions for `primitive`.
pub fn for_primitive(primitive: Primitive) -> &'static str {
    match primitive {
        Primitive::Gate => GATE,
        Primitive::Classify => CLASSIFY,
        Primitive::Score => SCORE,
        Primitive::Check => CHECK,
        Primitive::Rank => RANK,
        Primitive::Explain => EXPLAIN,
    }
}

/// Every prompt, for `jev-desk prompts`.
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
