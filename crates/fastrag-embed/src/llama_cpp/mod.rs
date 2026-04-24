//! llama.cpp HTTP embedding backend.
//!
//! Talks to a `llama-server` process (llama.cpp ≥ b5000) running the
//! `/embedding` endpoint. The server process lifecycle (spawn, health-check,
//! shutdown) lives in [`LlamaServerHandle`]. Task 4 will add the embedding
//! HTTP client.

mod client;
mod completion_client;
mod handle;
mod model_source;
mod pool;
mod runtime;
mod snowflake_arctic;

pub use client::LlamaCppClient;
pub use completion_client::{CompletionError, LlamaCppChatClient};
pub use handle::{LlamaServerConfig, LlamaServerHandle, MIN_LLAMA_SERVER_BUILD};
pub use model_source::{
    HfHubDownloader, ModelDownloader, ModelSource, resolve_model_path, resolve_model_path_default,
};
pub use pool::LlamaServerPool;
pub use runtime::GenericLlamaCppEmbedder;
pub use snowflake_arctic::SnowflakeArcticEmbedL1024Q8;
