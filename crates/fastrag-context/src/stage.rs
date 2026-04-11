//! Pipeline stage helper consumed by `fastrag::corpus::index_path_with_metadata`.
//!
//! Filled in during Phase 4 of the implementation plan.

use crate::ContextError;
use crate::contextualizer::Contextualizer;

/// Placeholder entry point. Real implementation lands in Phase 4.
#[allow(dead_code)]
pub fn run_contextualize_stage<C: Contextualizer>(
    _ctx: &C,
    _doc_title: &str,
    _raw_chunk: &str,
) -> Result<Option<String>, ContextError> {
    unimplemented!("run_contextualize_stage lands in Phase 4")
}
