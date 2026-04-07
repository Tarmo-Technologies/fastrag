use std::path::PathBuf;
use std::sync::Arc;

use fastrag::{BgeSmallEmbedder, Embedder};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum EmbedLoaderError {
    #[error("embedding model error: {0}")]
    Embed(#[from] fastrag::EmbedderError),
    #[error("unsupported model path: {0}")]
    UnsupportedModelPath(PathBuf),
}

pub fn load_embedder(model_path: Option<PathBuf>) -> Result<Arc<dyn Embedder>, EmbedLoaderError> {
    let embedder = match model_path {
        Some(path) => BgeSmallEmbedder::from_local(&path)?,
        None => BgeSmallEmbedder::from_hf_hub()?,
    };
    Ok(Arc::new(embedder))
}
