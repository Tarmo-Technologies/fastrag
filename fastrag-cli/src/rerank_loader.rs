//! Reranker loading for the CLI.
//!
//! Resolves ONNX reranker artifacts and builds a `Box<dyn Reranker>` from the
//! CLI `--rerank` flag. The llama-cpp reranker backend (formerly keyed on
//! `BgeRerankerV2M3Llama`) was removed in the no-Chinese-origin purge; the
//! only supported backend is ONNX via `ort`, backed by the Tarmo-rehosted
//! ModernBERT-gooaq-bce weights.

use fastrag_rerank::Reranker;
use thiserror::Error;

use crate::args::RerankerKindArg;

#[derive(Debug, Error)]
pub enum RerankLoaderError {
    #[error("reranker model error: {0}")]
    Model(String),
    #[error("I/O: {0}")]
    Io(#[from] std::io::Error),
}

impl From<fastrag_rerank::RerankError> for RerankLoaderError {
    fn from(e: fastrag_rerank::RerankError) -> Self {
        RerankLoaderError::Model(e.to_string())
    }
}

pub fn load_reranker(kind: RerankerKindArg) -> Result<Box<dyn Reranker>, RerankLoaderError> {
    match kind {
        RerankerKindArg::Onnx => load_onnx(),
    }
}

fn load_onnx() -> Result<Box<dyn Reranker>, RerankLoaderError> {
    #[cfg(not(feature = "rerank"))]
    {
        Err(RerankLoaderError::Model(
            "ONNX reranker not available: fastrag-cli built without `rerank` feature".into(),
        ))
    }
    #[cfg(feature = "rerank")]
    {
        use fastrag_rerank::onnx::ModernBertGooaqReranker;
        let reranker = ModernBertGooaqReranker::load_default()?;
        Ok(Box::new(reranker))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "calls hf-hub sync API which can hang indefinitely on hosts \
                without the tarmotech ONNX reranker cached. Run explicitly with \
                `cargo test --ignored -- load_reranker_dispatches_to_onnx` \
                on a host that has the model in ~/.cache/huggingface."]
    fn load_reranker_dispatches_to_onnx() {
        // Verify ONNX path is reached. If model files are present the load
        // succeeds; if absent the error must come from the ONNX model loader.
        let result = load_reranker(RerankerKindArg::Onnx);
        match result {
            Ok(reranker) => {
                assert_eq!(
                    reranker.model_id(),
                    "tarmotech/reranker-modernbert-gooaq-bce-onnx-private"
                );
            }
            Err(err) => {
                let msg = err.to_string();
                assert!(
                    msg.contains("model") || msg.contains("ORT") || msg.contains("onnx"),
                    "expected ONNX-related error, got: {msg}"
                );
            }
        }
    }
}
