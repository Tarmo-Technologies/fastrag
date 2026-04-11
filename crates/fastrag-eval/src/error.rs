use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum EvalError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("HTTP error: {0}")]
    Http(String),
    #[error("download checksum mismatch for {path}: expected {expected}, got {got}")]
    ChecksumMismatch {
        path: PathBuf,
        expected: String,
        got: String,
    },
    #[error("archive error: {0}")]
    Archive(String),
    #[error("XML parse error: {0}")]
    Xml(String),
    #[error("gzip decode error: {0}")]
    Gzip(String),
    #[error("cache directory is unavailable")]
    NoCacheDir,
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("embedding error: {0}")]
    Embed(#[from] fastrag_embed::EmbedError),
    #[error("index error: {0}")]
    Index(#[from] fastrag_index::IndexError),
    #[error("unsupported schema version: expected {expected}, got {got}")]
    UnsupportedSchemaVersion { expected: u32, got: u32 },
    #[error("unsupported chunking strategy: {0}")]
    UnsupportedChunkingStrategy(String),
    #[error("malformed dataset: {0}")]
    MalformedDataset(String),
    #[error("missing report parent directory for {path}")]
    MissingReportParent { path: PathBuf },
    #[error("histogram error: {0}")]
    Histogram(String),
    #[error("gold set parse error at {path}: {source}")]
    GoldSetParse {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("gold set validation failed: {0}")]
    GoldSetInvalid(String),
}

pub type EvalResult<T> = Result<T, EvalError>;
