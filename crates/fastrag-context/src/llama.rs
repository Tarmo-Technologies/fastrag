//! llama.cpp HTTP contextualizer backend. Filled in during Phase 3.

use crate::ContextError;
use crate::contextualizer::{Contextualizer, ContextualizerMeta};

/// Placeholder type. Real implementation lands in Phase 3.
pub struct LlamaCppContextualizer {
    _private: (),
}

impl ContextualizerMeta for LlamaCppContextualizer {
    fn model_id(&self) -> &str {
        unimplemented!("Phase 3")
    }
    fn prompt_version(&self) -> u32 {
        unimplemented!("Phase 3")
    }
}

impl Contextualizer for LlamaCppContextualizer {
    fn contextualize(&self, _doc_title: &str, _raw_chunk: &str) -> Result<String, ContextError> {
        unimplemented!("Phase 3")
    }
}
