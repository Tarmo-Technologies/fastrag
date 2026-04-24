use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EmbedBackend {
    Openai,
    Ollama,
    LlamaCpp,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrefixConfig {
    #[serde(default)]
    pub query: String,
    #[serde(default)]
    pub passage: String,
}

impl Default for PrefixConfig {
    fn default() -> Self {
        Self {
            query: String::new(),
            passage: String::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedEmbedderProfile {
    pub name: String,
    pub backend: EmbedBackend,
    pub model: String,
    pub base_url: Option<String>,
    pub prefix: PrefixConfig,
    pub dim_override: Option<usize>,
}
