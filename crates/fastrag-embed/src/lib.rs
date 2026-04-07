mod bge;
mod error;

#[cfg(feature = "test-utils")]
pub mod test_utils;

pub use crate::bge::BgeSmallEmbedder;
pub use crate::error::EmbedError;

/// An embedder produces fixed-size vectors for input texts.
pub trait Embedder: Send + Sync {
    /// An identifier for the embedding model implementation used.
    ///
    /// This is written into corpus manifests to enforce compatibility at load time.
    fn model_id(&self) -> &'static str {
        "unknown"
    }

    fn dim(&self) -> usize;
    fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbedError>;
}
