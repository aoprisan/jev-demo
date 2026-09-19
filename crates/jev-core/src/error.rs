//! Errors. Every failure here is hard: a primitive that cannot produce a valid,
//! in-range, schema-conforming output returns `Err`. Nothing degrades to a
//! permissive default, and in particular nothing degrades to `Action::Execute`.

/// Result alias for primitive and transport calls.
pub type Result<T> = std::result::Result<T, JevError>;

/// Why a judgment call failed.
#[derive(Debug, thiserror::Error)]
pub enum JevError {
    /// The transport (live API or mock) failed outright.
    #[error("jev transport error: {0}")]
    Transport(String),

    /// The reply is missing an answer the primitive asked for.
    #[error("jev answered without `{name}` (primitive {primitive}); asked {asked} question(s)")]
    MissingAnswer {
        /// The question name that went unanswered.
        name: String,
        /// The primitive that asked.
        primitive: &'static str,
        /// How many questions were asked in the call.
        asked: usize,
    },

    /// An answer came back as the wrong kind (e.g. a Score where a Choice was asked).
    #[error("jev answered `{name}` as {got}, expected {want} (primitive {primitive})")]
    AnswerKind {
        /// The question name.
        name: String,
        /// Kind received.
        got: &'static str,
        /// Kind expected.
        want: &'static str,
        /// The primitive that asked.
        primitive: &'static str,
    },

    /// A value fell outside the range its schema allows.
    #[error("{primitive}.{field} = {value} is outside {min}..={max}")]
    OutOfRange {
        /// The primitive that produced it.
        primitive: &'static str,
        /// The offending field.
        field: &'static str,
        /// The value seen.
        value: f64,
        /// Inclusive lower bound.
        min: f64,
        /// Inclusive upper bound.
        max: f64,
    },

    /// A string or collection exceeded the length its schema allows.
    #[error("{primitive}.{field} has length {len}, schema allows at most {max}")]
    TooLong {
        /// The primitive that produced it.
        primitive: &'static str,
        /// The offending field.
        field: &'static str,
        /// The length seen.
        len: usize,
        /// The maximum allowed.
        max: usize,
    },

    /// A choice came back as a label outside the enum the primitive offered.
    #[error("{primitive} answered `{label}`, which is not one of [{allowed}]")]
    UnknownLabel {
        /// The primitive that asked.
        primitive: &'static str,
        /// The label received.
        label: String,
        /// The labels that were offered.
        allowed: String,
    },

    /// A ranking did not cover exactly the candidates it was given.
    #[error("{primitive} ranked {got} candidate(s), expected exactly {want} with no repeats")]
    BadRanking {
        /// The primitive that asked.
        primitive: &'static str,
        /// Distinct candidates returned.
        got: usize,
        /// Candidates offered.
        want: usize,
    },

    /// The call was malformed before it left the process.
    #[error("invalid jev call: {0}")]
    InvalidCall(String),

    /// Writing the audit log failed.
    #[error("audit log: {0}")]
    Audit(String),
}
