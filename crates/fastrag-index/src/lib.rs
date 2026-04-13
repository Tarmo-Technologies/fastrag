mod entry;
mod error;
pub mod fusion;
pub mod hash;
mod hnsw;
pub mod identifiers;
mod manifest;

pub use entry::{VectorEntry, VectorHit};
pub use error::{IndexError, IndexResult};
pub use hnsw::HnswIndex;
pub use manifest::{
    ContextualizerManifest, CorpusManifest, FileEntry, ManifestChunkingStrategy, RootEntry,
};

use std::path::Path;

/// A persistent vector index for approximate nearest-neighbor search.
pub trait VectorIndex {
    fn add(&mut self, entries: Vec<VectorEntry>) -> IndexResult<()>;
    fn query(&self, vector: &[f32], top_k: usize) -> IndexResult<Vec<VectorHit>>;
    fn save(&self, dir: &Path) -> IndexResult<()>;
    fn len(&self) -> usize;
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
