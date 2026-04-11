//! SQLite-backed cache for contextualization results. See
//! `crates/fastrag-context/src/lib.rs` for the crate-level design notes.
//!
//! Filled in during Phase 2 of the implementation plan.

use crate::ContextError;

/// Placeholder cache type. Real implementation lands in Phase 2.
pub struct ContextCache {
    _private: (),
}

impl ContextCache {
    /// Placeholder so the module compiles before Phase 2 lands.
    #[allow(dead_code)]
    pub(crate) fn __unimplemented() -> Result<Self, ContextError> {
        unimplemented!("ContextCache lands in Phase 2")
    }
}
