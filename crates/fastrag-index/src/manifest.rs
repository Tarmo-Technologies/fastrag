use serde::{Deserialize, Serialize};

/// Persisted corpus metadata stored in `manifest.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CorpusManifest {
    pub version: u32,
    pub embedding_model_id: String,
    pub dim: usize,
    pub created_at_unix_seconds: u64,
    pub chunk_count: usize,
    pub chunking_strategy: ManifestChunkingStrategy,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum ManifestChunkingStrategy {
    Basic {
        max_characters: usize,
        overlap: usize,
    },
    ByTitle {
        max_characters: usize,
        overlap: usize,
    },
    RecursiveCharacter {
        max_characters: usize,
        overlap: usize,
        separators: Vec<String>,
    },
    Semantic {
        max_characters: usize,
        similarity_threshold: Option<f32>,
        percentile_threshold: Option<f32>,
    },
}

impl CorpusManifest {
    pub fn new(
        embedding_model_id: impl Into<String>,
        dim: usize,
        created_at_unix_seconds: u64,
        chunking_strategy: ManifestChunkingStrategy,
    ) -> Self {
        Self {
            version: 1,
            embedding_model_id: embedding_model_id.into(),
            dim,
            created_at_unix_seconds,
            chunk_count: 0,
            chunking_strategy,
        }
    }
}
