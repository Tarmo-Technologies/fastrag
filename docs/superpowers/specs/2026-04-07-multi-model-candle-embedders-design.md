# Multi-Model Candle Embedders + Eval Matrix (#31b)

**Status:** Design — awaiting review
**Date:** 2026-04-07
**Issue:** crook3dfingers/fastrag#31
**Predecessor:** #31a (HTTP embedders — OpenAI + Ollama, merged as PR #40)

## Goal

Make the candle-backed local embedder model-agnostic within the BERT family, add two alternative presets (E5-small, BGE-base) alongside the current BGE-small baseline, and produce a measured eval matrix so the default model is chosen by data instead of vibes. Resolve issue #31 on its own terms.

## Non-Goals

- ONNX runtime / `ort` backend — deferred; tracked as #34 perf work.
- Arbitrary HuggingFace repo id auto-download (`hf:owner/name`) — defer until demand.
- GPU / CUDA device selection — candle CPU only.
- Quantized (Q4/Q8) weights — defer until eval shows RAM pressure.
- Hot-swap embedder on a live corpus — always requires re-index; existing `EmbedderMismatch` covers this.

## Architecture

### New type: `CandleHfEmbedder`

Lives in `crates/fastrag-embed/src/candle_hf.rs` and replaces `bge.rs`. Holds a loaded candle BERT model, tokenizer, and a `ModelPreset` describing the model's metadata.

```rust
pub struct CandleHfEmbedder {
    preset: ModelPreset,
    model: BertModel,       // candle_transformers
    tokenizer: Tokenizer,
    device: Device,         // Device::Cpu
}
```

Constructor:

```rust
impl CandleHfEmbedder {
    pub fn from_preset(preset: ModelPreset, source: LoadSource) -> Result<Self, EmbedError>;
}

pub enum LoadSource {
    HfHub,              // download via hf-hub crate; honors FASTRAG_OFFLINE
    LocalPath(PathBuf), // offline/custom directory containing safetensors + tokenizer.json + config.json
}
```

### New enum: `ModelPreset`

```rust
pub enum ModelPreset {
    BgeSmall,   // BAAI/bge-small-en-v1.5 — 384d, no prefixes
    E5Small,    // intfloat/e5-small-v2    — 384d, "query: " / "passage: "
    BgeBase,    // BAAI/bge-base-en-v1.5  — 768d, no prefixes
}

impl ModelPreset {
    pub fn repo_id(&self) -> &'static str;
    pub fn dim(&self) -> usize;
    pub fn query_prefix(&self) -> Option<&'static str>;
    pub fn passage_prefix(&self) -> Option<&'static str>;
    pub fn model_id(&self) -> String; // "candle:<repo_id>"
    pub fn from_cli_name(name: &str) -> Result<Self, EmbedError>;
}
```

### Trait change: `Embedder`

Add two default methods that route through `embed()`:

```rust
pub trait Embedder: Send + Sync {
    fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, EmbedError>;
    fn dim(&self) -> usize;
    fn model_id(&self) -> String;

    fn embed_query(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, EmbedError> {
        self.embed(texts)
    }
    fn embed_passage(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, EmbedError> {
        self.embed(texts)
    }
}
```

`CandleHfEmbedder` overrides both: when the preset declares a prefix, the override prepends it to each input before tokenization and calls the shared forward-pass helper. `BgeSmallEmbedder` (now removed), `MockEmbedder`, `OpenAiEmbedder`, `OllamaEmbedder` inherit the defaults unchanged.

**Call sites:** corpus index path calls `embedder.embed_passage(...)`; query path and serve-http call `embedder.embed_query(...)`.

## CLI

New and changed flags on `index`, `query`, `serve-http`:

```
--embedder candle | openai | ollama    # "candle" replaces previous implicit "bge"
--model    bge-small | e5-small | bge-base
--model-path <dir>                      # mutually exclusive with --model
```

**Backwards compatibility:** `--embedder bge` is kept as a hidden alias for `--embedder candle --model bge-small` so existing corpora and scripts continue to work without change.

`EmbedderOptions` in `fastrag-cli/src/embed_loader.rs` gains:

```rust
pub enum EmbedderOptions {
    Candle { preset: ModelPreset, source: LoadSource },
    OpenAi { /* unchanged */ },
    Ollama { /* unchanged */ },
}
```

`load_for_write` constructs the embedder. `load_for_read` first reads the manifest's `model_id`, reconstructs the expected preset, and errors via the existing `CorpusError::EmbedderMismatch { existing, requested }` if user flags disagree with the manifest before any heavy loading work happens.

## Weight Distribution

Weights are fetched via the `hf-hub` crate (new direct dep on `fastrag-embed`). First use of a preset downloads `model.safetensors`, `tokenizer.json`, and `config.json` into the standard `~/.cache/huggingface/` layout; subsequent runs hit the cache.

Offline override: if `FASTRAG_OFFLINE=1` is set (or the environment variable form the user chooses at implementation time) and the weights are not already present in the cache, `from_preset` returns `EmbedError::WeightsNotCached { model_id }` with a message naming the expected cache path and the `hf` CLI command to pre-fetch. `--model-path <dir>` always bypasses hf-hub entirely and is the recommended path for air-gapped engagements.

## Manifest

The `model_id` field already exists on the corpus manifest (added in #31a). Values for candle become:

- `"candle:BAAI/bge-small-en-v1.5"`
- `"candle:intfloat/e5-small-v2"`
- `"candle:BAAI/bge-base-en-v1.5"`

The `candle:` prefix disambiguates from `openai:...` / `ollama:...`. The mismatch check is string equality on `model_id` and does not need any structural change — #31a's logic already covers this.

## Errors

New variants on `EmbedError` (thiserror):

- `PresetUnknown { name: String }` — bad `--model` value; mapped to a clap error at parse time where clap allows, runtime error otherwise.
- `WeightsNotCached { model_id: String }` — offline mode with a cache miss. Message includes the expected cache path.
- `HfHubDownload { source: hf_hub::api::ApiError }` — network, auth, or disk failure during fetch.
- `ModelLoad { source: candle_core::Error }` — safetensors parse failure, missing tokenizer, shape mismatch.

Reused: `CorpusError::EmbedderMismatch` from #31a.

No silent fallbacks. Offline mode never transparently reaches the network; network mode never uses a cache entry with a shape that disagrees with the preset (candle surfaces shape mismatch on load).

## Data Flow

**Index:** CLI → `EmbedderOptions::Candle { preset, source }` → `load_for_write` builds `CandleHfEmbedder` (resolving weights, honoring offline) → corpus calls `embed_passage(&chunks)` (prefix applied for E5) → manifest written with `model_id = "candle:<repo_id>"`.

**Query:** CLI → `load_for_read` reads manifest, reconstructs preset, checks against CLI flags (`EmbedderMismatch` on disagreement) → builds embedder → `embed_query(&[q])` (prefix applied for E5) → vector search → results.

**Serve-http:** same as query path; embedder built once at startup and reused per request.

## Testing

### Unit (in `candle_hf.rs`)

- `ModelPreset::from_cli_name` happy path + unknown-name error.
- Preset metadata assertions: bge-small dim=384 no prefixes, e5-small dim=384 + `"query: "` / `"passage: "`, bge-base dim=768 no prefixes.
- Prefix dispatch: use a stub `Embedder` impl that records the strings it receives; assert `embed_query` and `embed_passage` pass through the expected prefixed strings for each preset.
- `EmbedError` display strings for `PresetUnknown`, `WeightsNotCached`, `HfHubDownload`, `ModelLoad`.

### CLI e2e (`fastrag-cli/tests/`)

- `--model` and `--model-path` mutual exclusion (clap-level test).
- Backcompat: `--embedder bge` resolves to the bge-small preset path.
- Manifest mismatch: index with `--model bge-small`, query with `--model e5-small`, assert `EmbedderMismatch` error and non-zero exit.
- Use `MockEmbedder`-backed paths wherever the real candle model is not under test.

### Real-model integration (gated)

A single file under `fastrag-cli/tests/` or `fastrag-embed/tests/` exercises all three presets end-to-end on a 5-document fixture. Gated on `FASTRAG_E2E_MODELS=1` and marked `#[ignore]` so CI stays fast. Mirrors the pattern #31a established for `FASTRAG_E2E_OPENAI`.

### Eval matrix (not in CI; committed outputs)

A local-run eval command (extends the existing eval tooling from #25 rather than inventing new infra where possible) runs each preset against the security corpus and NFCorpus and writes:

- `docs/evals/31b-<model>-<dataset>.json` — per-run metrics: nDCG@10, recall@10, MRR, index wall time, query p50/p95, peak RSS, index bytes on disk.
- `docs/embedder-eval.md` — summary table (3 models × 2 datasets × the metrics above) and the default-selection decision. Default stays bge-small unless a model delivers ≥ 5% nDCG@10 improvement on the security corpus; the margin and decision are recorded in the doc.

## Out-of-Scope Reminders

ONNX, arbitrary-repo download, GPU, quantized weights, and hot-swap are all explicitly deferred. Attempting any of them during implementation is a scope violation — open a follow-up issue instead.

## Acceptance Criteria

- `CandleHfEmbedder` replaces `BgeSmallEmbedder`; all call sites and tests compile and pass under `cargo test --workspace --features retrieval`.
- Each of bge-small, e5-small, bge-base can be selected via `--model`, loaded from hf-hub cache or `--model-path`, and used to index and query a corpus.
- E5 prefix convention is applied automatically on index and query sides; a regression test asserts the prefixed strings reach the model.
- `--embedder bge` backcompat alias works without user-visible breakage.
- Manifest `model_id` round-trips and `EmbedderMismatch` triggers on disagreement across all three presets.
- `FASTRAG_OFFLINE=1` with an empty cache returns `WeightsNotCached` and does not touch the network.
- Eval matrix committed under `docs/evals/` and `docs/embedder-eval.md` with the default-selection decision recorded.
- `cargo clippy --workspace --all-targets --features retrieval -- -D warnings` is clean.
- README updated to mention `--model` / `--model-path` and point at `docs/embedder-eval.md`.
