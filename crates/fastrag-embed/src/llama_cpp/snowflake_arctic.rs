//! Snowflake Arctic Embed L GGUF Q8_0 preset backed by a local llama-server
//! subprocess.
//!
//! Replaces the previous Qwen-based preset. Arctic Embed L is asymmetric:
//! query inputs require a fixed instruction prefix, passage inputs do not.
//! The prefix is applied in `embed_query` before the text reaches
//! llama-server, mirroring the pattern used by `OllamaEmbedder`.

use crate::error::EmbedError;
use crate::llama_cpp::client::LlamaCppClient;
use crate::llama_cpp::handle::{LlamaServerConfig, LlamaServerHandle};
use crate::llama_cpp::model_source::ModelSource;
use crate::{Embedder, PassageText, PrefixScheme, QueryText};

/// Snowflake Arctic Embed L at Q8_0 quantization. 1024-dim, 512-token
/// context. Uses the Snowflake asymmetric scheme: the query is prepended
/// with the instruction below; passages are embedded verbatim.
pub struct SnowflakeArcticEmbedL1024Q8 {
    // The handle must outlive the client, and Drop ordering in Rust is
    // field declaration order (top-down), so `client` is dropped first and
    // then `handle` tears down the subprocess. Keep this order.
    client: LlamaCppClient,
    handle: LlamaServerHandle,
}

impl SnowflakeArcticEmbedL1024Q8 {
    pub const HF_REPO: &'static str = "tarmotech/snowflake-arctic-embed-l-gguf-private";
    pub const GGUF_FILE: &'static str = "snowflake-arctic-embed-l-Q8_0.GGUF";

    /// Snowflake Arctic Embed L instruction prefix, applied to queries only.
    pub const QUERY_PREFIX: &'static str =
        "Represent this sentence for searching relevant passages: ";

    pub fn model_source() -> ModelSource {
        ModelSource::HfHub {
            repo: Self::HF_REPO,
            file: Self::GGUF_FILE,
        }
    }

    /// Spawn a llama-server and build a ready-to-use preset.
    ///
    /// The `LlamaServerConfig.extra_args` must include `--model <path>` so
    /// llama-server knows which GGUF file to load. The model path is also
    /// sent in each `/v1/embeddings` request as the `model` field (required
    /// by newer llama-server builds).
    pub fn load(server: LlamaServerConfig) -> Result<Self, EmbedError> {
        let model_name = server
            .extra_args
            .windows(2)
            .find(|w| w[0] == "--model")
            .map(|w| w[1].clone())
            .unwrap_or_default();
        let handle = LlamaServerHandle::spawn(server)?;
        let client = LlamaCppClient::new(handle.base_url().to_string(), model_name, Self::DIM)?;
        Ok(Self { client, handle })
    }

    pub fn base_url(&self) -> &str {
        self.handle.base_url()
    }
}

impl Embedder for SnowflakeArcticEmbedL1024Q8 {
    const DIM: usize = 1024;
    const MODEL_ID: &'static str =
        "tarmotech/snowflake-arctic-embed-l-gguf-private@Q8_0";
    const PREFIX_SCHEME: PrefixScheme =
        PrefixScheme::new(Self::QUERY_PREFIX, "");

    fn embed_query(&self, texts: &[QueryText]) -> Result<Vec<Vec<f32>>, EmbedError> {
        let prefixed: Vec<String> = texts
            .iter()
            .map(|t| format!("{}{}", Self::QUERY_PREFIX, t.as_str()))
            .collect();
        let refs: Vec<&str> = prefixed.iter().map(String::as_str).collect();
        self.client.embed(&refs)
    }

    fn embed_passage(&self, texts: &[PassageText]) -> Result<Vec<Vec<f32>>, EmbedError> {
        let refs: Vec<&str> = texts.iter().map(PassageText::as_str).collect();
        self.client.embed(&refs)
    }

    fn default_batch_size(&self) -> usize {
        32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn const_invariants() {
        assert_eq!(SnowflakeArcticEmbedL1024Q8::DIM, 1024);
        assert_eq!(
            SnowflakeArcticEmbedL1024Q8::MODEL_ID,
            "tarmotech/snowflake-arctic-embed-l-gguf-private@Q8_0"
        );
        assert_ne!(
            SnowflakeArcticEmbedL1024Q8::PREFIX_SCHEME.hash(),
            PrefixScheme::NONE.hash(),
            "Arctic Embed L must declare an asymmetric prefix scheme"
        );
        assert!(
            SnowflakeArcticEmbedL1024Q8::QUERY_PREFIX
                .starts_with("Represent this sentence")
        );
        assert_eq!(SnowflakeArcticEmbedL1024Q8::PREFIX_SCHEME.passage, "");
    }

    #[test]
    fn model_source_points_at_tarmotech_repo() {
        match SnowflakeArcticEmbedL1024Q8::model_source() {
            ModelSource::HfHub { repo, file } => {
                assert_eq!(repo, "tarmotech/snowflake-arctic-embed-l-gguf-private");
                assert_eq!(file, "snowflake-arctic-embed-l-Q8_0.GGUF");
            }
            other => panic!("expected HfHub, got {other:?}"),
        }
    }

    #[test]
    fn no_chinese_origin_strings() {
        // Compliance guard — if anyone reintroduces Qwen/BAAI/Alibaba into
        // this preset, this test fails at unit-test time.
        let haystack = format!(
            "{} {} {}",
            SnowflakeArcticEmbedL1024Q8::HF_REPO,
            SnowflakeArcticEmbedL1024Q8::GGUF_FILE,
            SnowflakeArcticEmbedL1024Q8::MODEL_ID,
        )
        .to_lowercase();
        for banned in ["qwen", "alibaba", "baai", "bge"] {
            assert!(
                !haystack.contains(banned),
                "banned token '{banned}' found in preset constants"
            );
        }
    }
}
