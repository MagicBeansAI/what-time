//! Per-token predictions shared by the tagger and evaluation suites.

#[derive(Clone, Debug)]
pub struct Predictions {
    pub labels: Vec<u8>,
    pub clause_starts: Vec<u8>,
    pub scores: Vec<f32>,
    /// Raw classifier logits per non-whitespace token (evaluation only).
    pub logits: Option<Vec<f32>>,
    /// Raw boundary logits per token (evaluation only).
    pub boundary_logits: Option<Vec<f32>>,
    /// The window's sentence-intent logit (positive: a request to
    /// schedule), when the weights carry an intent head.
    pub intent: Option<f32>,
}
