#![allow(dead_code)]

use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

pub fn write_config(dir: &Path, contents: &str) -> PathBuf {
    let path = dir.join("fastrag.toml");
    fs::write(&path, contents).expect("write fastrag.toml");
    path
}

pub fn write_openai_config(
    dir: &Path,
    default_profile: &str,
    profiles: &[(&str, &str)],
) -> PathBuf {
    let mut contents = format!(
        "[embedder]\ndefault_profile = {}\n\n",
        toml_string(default_profile)
    );
    for (name, model) in profiles {
        contents.push_str(&format!(
            "[embedder.profiles.{name}]\nbackend = \"openai\"\nmodel = {}\n\n",
            toml_string(model)
        ));
    }
    write_config(dir, &contents)
}

pub fn write_llama_cpp_config(dir: &Path, default_profile: &str, model_path: &Path) -> PathBuf {
    let contents = format!(
        "[embedder]\ndefault_profile = {}\n\n[embedder.profiles.{default_profile}]\nbackend = \"llama-cpp\"\nmodel = {}\ndim_override = 1024\n",
        toml_string(default_profile),
        toml_string(model_path.to_str().expect("model path must be valid utf-8")),
    );
    write_config(dir, &contents)
}

pub fn start_openai_embedding_server() -> (String, MockServer) {
    runtime().block_on(async {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/embeddings"))
            .respond_with(|request: &Request| openai_embedding_response(request))
            .mount(&server)
            .await;
        (server.uri(), server)
    })
}

pub fn llama_cpp_embed_model_path() -> Option<PathBuf> {
    std::env::var_os("FASTRAG_LLAMA_EMBED_MODEL_PATH")
        .map(PathBuf::from)
        .filter(|path| path.exists())
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime")
}

fn openai_embedding_response(request: &Request) -> ResponseTemplate {
    let payload: OpenAiEmbeddingRequest = request.body_json().expect("OpenAI request JSON");
    let dim = match payload.model.as_str() {
        "text-embedding-3-small" => 1536,
        "text-embedding-3-large" => 3072,
        other => panic!("unexpected OpenAI model in test request: {other}"),
    };
    let data: Vec<_> = payload
        .input
        .iter()
        .map(|input| json!({ "embedding": lexical_embedding(input, dim) }))
        .collect();
    ResponseTemplate::new(200).set_body_json(json!({ "data": data }))
}

fn lexical_embedding(input: &str, dim: usize) -> Vec<f32> {
    const ACTIVE_DIMS: usize = 64;
    let active_dims = ACTIVE_DIMS.min(dim);
    let mut out = vec![0.0_f32; dim];
    let tokens = tokenize(input);
    if tokens.is_empty() {
        out[0] = 1.0;
        return out;
    }

    for token in tokens {
        let mut hasher = DefaultHasher::new();
        token.hash(&mut hasher);
        let idx = (hasher.finish() as usize) % active_dims;
        out[idx] += 1.0;
    }

    let norm = out
        .iter()
        .take(active_dims)
        .map(|value| value * value)
        .sum::<f32>()
        .sqrt();
    if norm > 0.0 {
        for value in out.iter_mut().take(active_dims) {
            *value /= norm;
        }
    }
    out
}

fn tokenize(input: &str) -> Vec<String> {
    input
        .to_ascii_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn toml_string(value: &str) -> String {
    serde_json::to_string(value).expect("serialize TOML string")
}

#[derive(Debug, Deserialize)]
struct OpenAiEmbeddingRequest {
    model: String,
    input: Vec<String>,
}
