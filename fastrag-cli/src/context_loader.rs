//! Helpers for bringing up the contextualization stack at CLI startup.
//!
//! Compiled only when `--features contextual` is active.
//!
//! The `contextual-llama` sub-feature (which auto-spawned a `llama-server`
//! subprocess backed by a Qwen3-4B completion preset) was removed in the
//! no-Chinese-origin purge. `fastrag context-layer` now requires the caller
//! to supply a contextualizer out-of-band; `load_context_state` returns
//! `BackendNotCompiled` in every configuration. Re-enable once a non-Chinese
//! default contextualizer lands.

#![cfg(feature = "contextual")]

use std::path::Path;

use thiserror::Error;

use fastrag_context::{ContextCache, ContextError, Contextualizer};

/// Error surface for `context_loader::load_context_state`.
#[derive(Debug, Error)]
pub enum ContextLoaderError {
    #[error(
        "contextualization has no built-in backend after the no-Chinese-origin purge; \
         pass --context-model or wire a non-Chinese default contextualizer into \
         fastrag-cli before calling `context-layer`"
    )]
    BackendNotCompiled,
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("cache error: {0}")]
    Cache(#[from] ContextError),
}

/// Live contextualization state. Retained as a struct for callers that build
/// their own contextualizer outside this loader and still want to use the
/// same cache-plus-server lifecycle pattern.
pub struct ContextState {
    pub cache: ContextCache,
    pub contextualizer: Box<dyn Contextualizer>,
}

/// No-op loader for `fastrag context-layer` until a non-Chinese default
/// contextualizer is wired up. Callers should construct their own
/// `Contextualizer` directly.
pub fn load_context_state(_corpus: &Path) -> Result<ContextState, ContextLoaderError> {
    Err(ContextLoaderError::BackendNotCompiled)
}
