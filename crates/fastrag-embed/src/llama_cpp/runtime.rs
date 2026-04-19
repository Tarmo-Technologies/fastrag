//! Generic llama.cpp embedding backend driven by explicit runtime config.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use serde::Deserialize;
use serde_json::json;

use super::{
    client::LlamaCppClient,
    handle::{LlamaServerConfig, LlamaServerHandle},
};
use crate::error::EmbedError;
use crate::{DynEmbedderTrait, EmbedderIdentity, PassageText, PrefixScheme, QueryText};

fn intern_str(s: String) -> &'static str {
    static CACHE: OnceLock<Mutex<HashMap<String, &'static str>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut map = cache.lock().expect("prefix cache poisoned");
    if let Some(&cached) = map.get(&s) {
        return cached;
    }
    let leaked: &'static str = Box::leak(s.clone().into_boxed_str());
    map.insert(s, leaked);
    leaked
}

#[derive(Deserialize)]
struct EmbeddingsResponse {
    data: Vec<EmbeddingItem>,
}

#[derive(Deserialize)]
struct EmbeddingItem {
    embedding: Vec<f32>,
}

/// Runtime-configured llama.cpp embedder backed by a local `llama-server`
/// subprocess.
pub struct GenericLlamaCppEmbedder {
    // Keep client before handle so the HTTP client drops before the subprocess.
    client: LlamaCppClient,
    handle: LlamaServerHandle,
    dim: usize,
    model_id_static: &'static str,
    prefix_scheme: PrefixScheme,
}

impl GenericLlamaCppEmbedder {
    pub fn load(
        server: LlamaServerConfig,
        model: String,
        prefix_scheme: PrefixScheme,
    ) -> Result<Self, EmbedError> {
        let handle = LlamaServerHandle::spawn(server)?;
        let dim = probe_dim(&handle, &model, prefix_scheme.query)?;
        let client = LlamaCppClient::new(handle.base_url().to_string(), model.clone(), dim)?;
        let model_id_static = intern_str(format!("llama-cpp:{model}"));
        Ok(Self {
            client,
            handle,
            dim,
            model_id_static,
            prefix_scheme,
        })
    }
}

impl DynEmbedderTrait for GenericLlamaCppEmbedder {
    fn model_id(&self) -> &'static str {
        self.model_id_static
    }

    fn dim(&self) -> usize {
        self.dim
    }

    fn prefix_scheme(&self) -> PrefixScheme {
        self.prefix_scheme.clone()
    }

    fn prefix_scheme_hash(&self) -> u64 {
        self.prefix_scheme.hash()
    }

    fn identity(&self) -> EmbedderIdentity {
        EmbedderIdentity {
            model_id: self.model_id_static.to_string(),
            dim: self.dim,
            prefix_scheme_hash: self.prefix_scheme.hash(),
        }
    }

    fn default_batch_size(&self) -> usize {
        32
    }

    fn embed_query_dyn(&self, texts: &[QueryText]) -> Result<Vec<Vec<f32>>, EmbedError> {
        let prefixed: Vec<String> = texts
            .iter()
            .map(|text| format!("{}{}", self.prefix_scheme.query, text.as_str()))
            .collect();
        let refs: Vec<&str> = prefixed.iter().map(String::as_str).collect();
        self.client.embed(&refs)
    }

    fn embed_passage_dyn(&self, texts: &[PassageText]) -> Result<Vec<Vec<f32>>, EmbedError> {
        let prefixed: Vec<String> = texts
            .iter()
            .map(|text| format!("{}{}", self.prefix_scheme.passage, text.as_str()))
            .collect();
        let refs: Vec<&str> = prefixed.iter().map(String::as_str).collect();
        self.client.embed(&refs)
    }

    fn is_ready(&self) -> bool {
        self.handle
            .client()
            .get(format!("{}/health", self.handle.base_url()))
            .send()
            .map(|resp| resp.status().is_success())
            .unwrap_or(false)
    }
}

fn probe_dim(
    handle: &LlamaServerHandle,
    model: &str,
    query_prefix: &str,
) -> Result<usize, EmbedError> {
    let url = format!("{}/v1/embeddings", handle.base_url());
    let body = json!({
        "model": model,
        "input": [format!("{query_prefix}a")],
    });
    let resp = handle
        .client()
        .post(&url)
        .json(&body)
        .send()
        .map_err(|e| EmbedError::DimensionProbeFailed(e.to_string()))?;
    if !resp.status().is_success() {
        return Err(EmbedError::DimensionProbeFailed(format!(
            "status {}",
            resp.status().as_u16()
        )));
    }
    let parsed: EmbeddingsResponse = resp
        .json()
        .map_err(|e| EmbedError::DimensionProbeFailed(e.to_string()))?;
    let first = parsed
        .data
        .into_iter()
        .next()
        .ok_or_else(|| EmbedError::DimensionProbeFailed("empty embedding response".into()))?;
    if first.embedding.is_empty() {
        return Err(EmbedError::DimensionProbeFailed(
            "empty embedding vector".into(),
        ));
    }
    Ok(first.embedding.len())
}
