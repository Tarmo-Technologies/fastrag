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

    #[error("hf-hub error: {0}")]
    HfHub(String),

    #[error("unexpected embedding dimension: expected {expected}, got {got}")]
    UnexpectedDim { expected: usize, got: usize },

    #[error("missing required environment variable: {0}")]
    MissingEnv(&'static str),

    #[error("http transport error: {0}")]
    Http(String),

    #[error("api error: status {status}: {message}")]
    Api { status: u16, message: String },

    #[error("dimension probe failed: {0}")]
    DimensionProbeFailed(String),

    #[error("unknown model for backend {backend}: {model}")]
    UnknownModel {
        backend: &'static str,
        model: String,
    },

    #[error("empty input")]
    EmptyInput,

    #[cfg(feature = "llama-cpp")]
    #[error("failed to spawn llama-server: {0}")]
    LlamaServerSpawn(String),

    #[cfg(feature = "llama-cpp")]
    #[error("llama-server exited before becoming ready: status={status}")]
    LlamaServerExitedEarly { status: String },

    #[cfg(feature = "llama-cpp")]
    #[error("llama-server /health did not become ready on port {port} within {waited:?}")]
    LlamaServerHealthTimeout {
        port: u16,
        waited: std::time::Duration,
    },

    #[cfg(feature = "llama-cpp")]
    #[error(
        "llama-server version b{found} is below minimum b{minimum}; \
         upgrade from https://github.com/ggml-org/llama.cpp/releases"
    )]
    LlamaServerVersionTooOld { found: u32, minimum: u32 },

    #[cfg(feature = "llama-cpp")]
    #[error("failed to parse llama-server --version output: {0}")]
    LlamaServerVersionParse(String),
}

#[cfg(feature = "llama-cpp")]
impl From<hf_hub::api::sync::ApiError> for EmbedError {
    fn from(value: hf_hub::api::sync::ApiError) -> Self {
        Self::HfHub(value.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_variants_format_cleanly() {
        let e = EmbedError::MissingEnv("OPENAI_API_KEY");
        assert_eq!(
            e.to_string(),
            "missing required environment variable: OPENAI_API_KEY"
        );

        let e = EmbedError::Http("connection reset".into());
        assert_eq!(e.to_string(), "http transport error: connection reset");

        let e = EmbedError::Api {
            status: 401,
            message: "bad key".into(),
        };
        assert_eq!(e.to_string(), "api error: status 401: bad key");

        let e = EmbedError::DimensionProbeFailed("refused".into());
        assert_eq!(e.to_string(), "dimension probe failed: refused");

        let e = EmbedError::UnknownModel {
            backend: "openai",
            model: "text-embedding-9001".into(),
        };
        assert_eq!(
            e.to_string(),
            "unknown model for backend openai: text-embedding-9001"
        );
    }
}
