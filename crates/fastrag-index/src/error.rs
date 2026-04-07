use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum IndexError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("bincode error: {0}")]
    Bincode(#[from] Box<bincode::ErrorKind>),

    #[error("dimension mismatch: expected {expected}, got {got}")]
    DimensionMismatch { expected: usize, got: usize },

    #[error("corpus is empty")]
    EmptyCorpus,

    #[error("corpus file missing: {path}")]
    MissingCorpusFile { path: PathBuf },

    #[error("corpus is corrupt: {message}")]
    CorruptCorpus { message: String },
}

pub type IndexResult<T> = Result<T, IndexError>;
