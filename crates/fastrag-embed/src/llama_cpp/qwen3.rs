//! Qwen3-Embedding-0.6B GGUF Q8_0 preset backed by a local llama-server
//! subprocess.

use crate::error::EmbedError;
use crate::llama_cpp::client::LlamaCppClient;
use crate::llama_cpp::handle::{LlamaServerConfig, LlamaServerHandle};
use crate::llama_cpp::model_source::ModelSource;
use crate::{Embedder, PassageText, PrefixScheme, QueryText};

/// Qwen3-Embedding-0.6B at Q8_0 quantization. 1024-dim, no query/passage
/// prefix (the Qwen3-Embedding family is instruction-tuned without the
/// BGE-style prefix convention).
pub struct Qwen3Embed600mQ8 {
    // The handle must outlive the client, and Drop ordering in Rust is
    // field declaration order (top-down), so `client` is dropped first and
    // then `handle` tears down the subprocess. Keep this order.
    client: LlamaCppClient,
    handle: LlamaServerHandle,
}

impl Qwen3Embed600mQ8 {
    pub const HF_REPO: &'static str = "Qwen/Qwen3-Embedding-0.6B-GGUF";
    pub const GGUF_FILE: &'static str = "Qwen3-Embedding-0.6B-Q8_0.gguf";

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
        // Extract the --model value from extra_args for the HTTP client.
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

impl Embedder for Qwen3Embed600mQ8 {
    const DIM: usize = 1024;
    const MODEL_ID: &'static str = "Qwen/Qwen3-Embedding-0.6B-GGUF@Q8_0";
    const PREFIX_SCHEME: PrefixScheme = PrefixScheme::NONE;

    fn embed_query(&self, texts: &[QueryText]) -> Result<Vec<Vec<f32>>, EmbedError> {
        let refs: Vec<&str> = texts.iter().map(QueryText::as_str).collect();
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
        assert_eq!(Qwen3Embed600mQ8::DIM, 1024);
        assert_eq!(
            Qwen3Embed600mQ8::MODEL_ID,
            "Qwen/Qwen3-Embedding-0.6B-GGUF@Q8_0"
        );
        assert_eq!(
            Qwen3Embed600mQ8::PREFIX_SCHEME.hash(),
            PrefixScheme::NONE.hash()
        );
    }
}
