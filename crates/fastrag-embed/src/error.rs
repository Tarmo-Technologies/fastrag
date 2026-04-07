use std::path::PathBuf;

/// Errors that can occur during embedding.
#[derive(Debug, thiserror::Error)]
pub enum EmbedError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("failed to locate a cache directory for model downloads")]
    NoCacheDir,

    #[error("missing required model file: {path}")]
    MissingModelFile { path: PathBuf },

    #[error("tokenizer error: {0}")]
    Tokenizer(String),

    #[error("candle error: {0}")]
    Candle(String),

    #[error("hf-hub error: {0}")]
    HfHub(String),

    #[error("unexpected embedding dimension: expected {expected}, got {got}")]
    UnexpectedDim { expected: usize, got: usize },

    #[error("empty input")]
    EmptyInput,
}

impl From<candle_core::Error> for EmbedError {
    fn from(value: candle_core::Error) -> Self {
        Self::Candle(value.to_string())
    }
}

impl From<tokenizers::Error> for EmbedError {
    fn from(value: tokenizers::Error) -> Self {
        Self::Tokenizer(value.to_string())
    }
}

impl From<hf_hub::api::sync::ApiError> for EmbedError {
    fn from(value: hf_hub::api::sync::ApiError) -> Self {
        Self::HfHub(value.to_string())
    }
}
