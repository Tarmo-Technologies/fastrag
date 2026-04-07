use serde::{Deserialize, Serialize};

/// Persisted corpus metadata stored in `manifest.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CorpusManifest {
    pub version: u32,
    pub embedding_model_id: String,
    pub dim: usize,
    pub created_at_unix_seconds: u64,
    pub chunk_count: usize,
    pub chunking_strategy: ManifestChunkingStrategy,
    #[serde(default)]
    pub roots: Vec<RootEntry>,
    #[serde(default)]
    pub files: Vec<FileEntry>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RootEntry {
    pub id: u32,
    pub path: std::path::PathBuf,
    pub last_indexed_unix_seconds: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileEntry {
    pub root_id: u32,
    pub rel_path: std::path::PathBuf,
    pub size: u64,
    pub mtime_ns: i128,
    pub content_hash: Option<String>,
    pub chunk_ids: Vec<u64>,
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
            roots: Vec::new(),
            files: Vec::new(),
        }
    }
}

#[cfg(test)]
mod v2_tests {
    use super::*;

    #[test]
    fn v2_roundtrip() {
        let m = CorpusManifest {
            version: 2,
            embedding_model_id: "mock".into(),
            dim: 3,
            created_at_unix_seconds: 1,
            chunk_count: 0,
            chunking_strategy: ManifestChunkingStrategy::Basic {
                max_characters: 100,
                overlap: 0,
            },
            roots: vec![RootEntry {
                id: 0,
                path: "/tmp/docs".into(),
                last_indexed_unix_seconds: 42,
            }],
            files: vec![FileEntry {
                root_id: 0,
                rel_path: "a.txt".into(),
                size: 10,
                mtime_ns: 1_700_000_000_000_000_000,
                content_hash: Some("blake3:abc".into()),
                chunk_ids: vec![1, 2],
            }],
        };
        let s = serde_json::to_string(&m).unwrap();
        let back: CorpusManifest = serde_json::from_str(&s).unwrap();
        assert_eq!(back, m);
    }

    #[test]
    fn v1_manifest_loads_with_empty_roots_files() {
        let v1 = r#"{
            "version": 1,
            "embedding_model_id": "mock",
            "dim": 3,
            "created_at_unix_seconds": 1,
            "chunk_count": 0,
            "chunking_strategy": {"kind":"basic","max_characters":100,"overlap":0}
        }"#;
        let m: CorpusManifest = serde_json::from_str(v1).unwrap();
        assert_eq!(m.version, 1);
        assert!(m.roots.is_empty());
        assert!(m.files.is_empty());
    }
}
