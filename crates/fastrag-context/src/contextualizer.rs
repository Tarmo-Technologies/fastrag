//! The `Contextualizer` trait and the no-op `NoContextualizer` default.
//!
//! A contextualizer takes a document title and a raw chunk of text and
//! returns a short (50–100 token) natural-language prefix that situates the
//! chunk in the surrounding document. The prefix is prepended to the chunk
//! text before embedding and BM25 indexing to improve retrieval quality
//! (Anthropic Contextual Retrieval, Sept 2024).

use crate::{CTX_VERSION, ContextError};

/// Identity metadata for a contextualizer. Factored out so cache lookups can
/// key on the (model, prompt) tuple without depending on the contextualizer
/// object itself.
pub trait ContextualizerMeta {
    /// Stable identifier for the underlying model (e.g. the GGUF filename or
    /// a preset name). Used as part of the cache key so two runs with
    /// different models produce independent cached results.
    fn model_id(&self) -> &str;

    /// Version of the prompt template used by this contextualizer. Bumped
    /// when the prompt text itself changes, so prompt edits invalidate the
    /// cache cleanly.
    fn prompt_version(&self) -> u32;

    /// Schema version of the cache layout. Callers should not override this;
    /// it defaults to the crate-level [`CTX_VERSION`] constant.
    fn ctx_version(&self) -> u32 {
        CTX_VERSION
    }
}

/// Synchronous contextualizer. Sync matches the rest of fastrag-embed which
/// uses `reqwest::blocking`; contextualization happens inside the existing
/// `block_in_place` CLI path.
pub trait Contextualizer: ContextualizerMeta + Send + Sync {
    /// Produce a context prefix for `raw_chunk` given the surrounding
    /// `doc_title`. `doc_title` may be empty; the prompt template handles
    /// that case.
    fn contextualize(&self, doc_title: &str, raw_chunk: &str) -> Result<String, ContextError>;
}

/// No-op contextualizer. The default when the feature is not enabled at the
/// CLI level. Returns the input unchanged.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoContextualizer;

impl ContextualizerMeta for NoContextualizer {
    fn model_id(&self) -> &str {
        "none"
    }
    fn prompt_version(&self) -> u32 {
        0
    }
}

impl Contextualizer for NoContextualizer {
    fn contextualize(&self, _doc_title: &str, raw_chunk: &str) -> Result<String, ContextError> {
        Ok(raw_chunk.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_contextualizer_returns_input_unchanged() {
        let c = NoContextualizer;
        let out = c
            .contextualize("Doc title", "chunk body text here")
            .expect("ok");
        assert_eq!(out, "chunk body text here");
    }

    #[test]
    fn no_contextualizer_accepts_empty_title() {
        let c = NoContextualizer;
        let out = c.contextualize("", "body").expect("ok");
        assert_eq!(out, "body");
    }

    #[test]
    fn no_contextualizer_meta_is_stable() {
        let c = NoContextualizer;
        assert_eq!(c.model_id(), "none");
        assert_eq!(c.prompt_version(), 0);
        assert_eq!(c.ctx_version(), CTX_VERSION);
    }
}
