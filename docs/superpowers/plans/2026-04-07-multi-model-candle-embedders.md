# Multi-Model Candle Embedders (#31b) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Generalize fastrag's candle-backed embedder into a `CandleHfEmbedder` that supports bge-small, e5-small, and bge-base presets (with automatic E5 prefix handling, offline mode, and `--model-path` overrides), then run and commit an eval matrix that picks the default by measurement. Closes #31.

**Architecture:** Replace `BgeSmallEmbedder` with a single `CandleHfEmbedder` keyed by a `ModelPreset` enum. Add default `embed_query` / `embed_passage` methods to the `Embedder` trait so E5 can apply its `"query: "` / `"passage: "` prefixes transparently while BGE / HTTP / mock embedders inherit no-op passthroughs. Reuse the existing `hf-hub` download path under `~/.cache/fastrag/models/<model>/`. Honor `FASTRAG_OFFLINE=1` at the hub entry point. Record model ids in the corpus manifest using the existing `fastrag/<model>` convention so read-path auto-detect in `embed_loader.rs` keeps working unchanged.

**Tech Stack:** Rust, candle-core 0.10, candle-transformers (BertModel), tokenizers, hf-hub (already a dep), clap, thiserror, wiremock (tests only).

---

## File Structure

**Created:**
- `crates/fastrag-embed/src/candle_hf.rs` — `CandleHfEmbedder`, `ModelPreset`, `LoadSource`, shared BERT forward-pass helpers.
- `fastrag-cli/tests/model_selection_e2e.rs` — CLI e2e for `--model`, backcompat alias, manifest mismatch (MockEmbedder-backed where possible).
- `fastrag-cli/tests/candle_real_e2e.rs` — gated on `FASTRAG_E2E_MODELS=1`, `#[ignore]`, exercises all three presets against a 5-doc fixture.
- `docs/evals/31b-bge-small-security.json`, `31b-e5-small-security.json`, `31b-bge-base-security.json`, and the three `-nfcorpus` counterparts — eval run outputs (produced in Task 15, committed).
- `docs/embedder-eval.md` — summary table + default-selection decision.

**Modified:**
- `crates/fastrag-embed/src/bge.rs` → deleted at the end of Task 7. Before deletion (Task 1) it gets a one-line fix for the pre-existing `model_id` mismatch.
- `crates/fastrag-embed/src/lib.rs` — trait additions, module wiring, re-exports.
- `crates/fastrag-embed/src/error.rs` — new `EmbedError` variants.
- `crates/fastrag/src/lib.rs` — re-export `CandleHfEmbedder`, `ModelPreset`, drop `BgeSmallEmbedder`.
- `crates/fastrag/src/corpus/mod.rs` — index path calls `embed_passage`, query path calls `embed_query` (internal callers only).
- `crates/fastrag-mcp/src/lib.rs` — query-side switch to `embed_query`.
- `fastrag-cli/src/embed_loader.rs` — `EmbedderOptions.model` field, preset-aware `build`, updated `detect_from_manifest`.
- `fastrag-cli/src/args.rs` — `--model` flag on `Index`, `Query`, `ServeHttp`; `EmbedderKindArg::Candle` replacing `Bge` with a hidden `Bge` alias.
- `fastrag-cli/src/main.rs` — pipe new args into `EmbedderOptions`, use `embed_query` on query/serve paths.
- `fastrag-cli/src/eval.rs` — extended to accept `--model` for the eval matrix.
- `README.md` — document `--model`, `--model-path`, `FASTRAG_OFFLINE`, link to `docs/embedder-eval.md`.
- `crates/fastrag/CLAUDE.md` / top-level `CLAUDE.md` — no changes needed; the existing conventions already apply.

---

## Task 1: Fix pre-existing BGE `model_id` mismatch

**Files:**
- Modify: `crates/fastrag-embed/src/bge.rs:10`
- Test: `crates/fastrag-embed/src/bge.rs` (cfg(test) module, inline)

`BgeSmallEmbedder::model_id()` currently returns `"BAAI/bge-small-en-v1.5"`, but `detect_from_manifest` in `embed_loader.rs` expects manifest ids to start with `"fastrag/bge"`. This means `load_for_read` on any real BGE corpus errors with `unrecognized embedding_model_id`. Fix before touching anything else so the regression test for Task 7 has a stable baseline.

- [ ] **Step 1: Write the failing test**

Append to the `mod tests` block at the bottom of `crates/fastrag-embed/src/bge.rs`:

```rust
#[test]
fn model_id_uses_fastrag_prefix_for_manifest_compat() {
    // embed_loader.rs::detect_from_manifest matches manifest ids by
    // `starts_with("fastrag/bge")`. Any change here will break read-path
    // auto-detect for existing BGE corpora.
    //
    // We assert on the constant rather than constructing a real embedder
    // (which would require downloading weights).
    assert!(
        MODEL_REPO_ID.starts_with("fastrag/bge"),
        "MODEL_REPO_ID must match detect_from_manifest() prefix"
    );
}
```

- [ ] **Step 2: Run and confirm it fails**

```
cargo test -p fastrag-embed bge::tests::model_id_uses_fastrag_prefix_for_manifest_compat
```
Expected: FAIL — `assertion failed: MODEL_REPO_ID.starts_with("fastrag/bge")`.

- [ ] **Step 3: Fix the constant**

Change line 10 of `crates/fastrag-embed/src/bge.rs` from:
```rust
const MODEL_REPO_ID: &str = "BAAI/bge-small-en-v1.5";
```
to:
```rust
// Manifest id (matches `fastrag/bge-small-en-v1.5` convention that
// embed_loader.rs::detect_from_manifest uses). The HF repo id for
// downloading weights is a separate constant below.
const MODEL_REPO_ID: &str = "fastrag/bge-small-en-v1.5";
const HF_REPO_ID: &str = "BAAI/bge-small-en-v1.5";
```
Then update line 93 (`from_hf_hub`): replace `MODEL_REPO_ID.to_string()` with `HF_REPO_ID.to_string()` in the `api.model(...)` call.

- [ ] **Step 4: Run the new test and the whole embed crate's tests**

```
cargo test -p fastrag-embed
```
Expected: PASS, no regressions.

- [ ] **Step 5: Commit**

```
git add crates/fastrag-embed/src/bge.rs
git commit -m "fix(embed): use fastrag/ prefix for BGE manifest id

Align BgeSmallEmbedder::model_id() with embed_loader.rs::detect_from_manifest,
which expects manifest ids to start with \"fastrag/bge\". Previously the read
path errored with 'unrecognized embedding_model_id' on real BGE corpora.

Pre-work for #31b."
```

---

## Task 2: Add `ModelPreset` enum

**Files:**
- Create: `crates/fastrag-embed/src/candle_hf.rs` (new, preset only for now — the embedder struct lands in Task 4)
- Modify: `crates/fastrag-embed/src/lib.rs` to declare `pub mod candle_hf;`

- [ ] **Step 1: Create the new module with failing tests first**

Create `crates/fastrag-embed/src/candle_hf.rs` containing only:

```rust
use crate::EmbedError;

/// One of the three BERT-family presets fastrag supports via candle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelPreset {
    BgeSmall,
    E5Small,
    BgeBase,
}

impl ModelPreset {
    /// HuggingFace repo id used to download weights.
    pub fn hf_repo_id(&self) -> &'static str {
        match self {
            ModelPreset::BgeSmall => "BAAI/bge-small-en-v1.5",
            ModelPreset::E5Small  => "intfloat/e5-small-v2",
            ModelPreset::BgeBase  => "BAAI/bge-base-en-v1.5",
        }
    }

    /// Manifest id recorded in the corpus. Uses the existing `fastrag/…`
    /// convention so `embed_loader.rs::detect_from_manifest` keeps working.
    pub fn manifest_id(&self) -> &'static str {
        match self {
            ModelPreset::BgeSmall => "fastrag/bge-small-en-v1.5",
            ModelPreset::E5Small  => "fastrag/e5-small-v2",
            ModelPreset::BgeBase  => "fastrag/bge-base-en-v1.5",
        }
    }

    /// Expected hidden dim.
    pub fn dim(&self) -> usize {
        match self {
            ModelPreset::BgeSmall | ModelPreset::E5Small => 384,
            ModelPreset::BgeBase => 768,
        }
    }

    /// Prefix to prepend to query strings before tokenization, if the model
    /// was trained with one. BGE does not use prefixes; E5 does.
    pub fn query_prefix(&self) -> Option<&'static str> {
        match self {
            ModelPreset::E5Small => Some("query: "),
            _ => None,
        }
    }

    /// Prefix to prepend to document/passage strings before tokenization.
    pub fn passage_prefix(&self) -> Option<&'static str> {
        match self {
            ModelPreset::E5Small => Some("passage: "),
            _ => None,
        }
    }

    /// Cache subdirectory under `~/.cache/fastrag/models/`. Stable across
    /// versions so existing downloads survive upgrades.
    pub fn cache_subdir(&self) -> &'static str {
        match self {
            ModelPreset::BgeSmall => "bge-small-en-v1.5",
            ModelPreset::E5Small  => "e5-small-v2",
            ModelPreset::BgeBase  => "bge-base-en-v1.5",
        }
    }

    /// Parse from the CLI `--model` value.
    pub fn from_cli_name(name: &str) -> Result<Self, EmbedError> {
        match name {
            "bge-small" => Ok(ModelPreset::BgeSmall),
            "e5-small"  => Ok(ModelPreset::E5Small),
            "bge-base"  => Ok(ModelPreset::BgeBase),
            other => Err(EmbedError::PresetUnknown { name: other.to_string() }),
        }
    }

    /// Reverse of `manifest_id` — used by `embed_loader.rs` to rebuild the
    /// preset from an existing corpus manifest.
    pub fn from_manifest_id(id: &str) -> Option<Self> {
        match id {
            "fastrag/bge-small-en-v1.5" => Some(ModelPreset::BgeSmall),
            "fastrag/e5-small-v2"       => Some(ModelPreset::E5Small),
            "fastrag/bge-base-en-v1.5"  => Some(ModelPreset::BgeBase),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_have_expected_dims() {
        assert_eq!(ModelPreset::BgeSmall.dim(), 384);
        assert_eq!(ModelPreset::E5Small.dim(), 384);
        assert_eq!(ModelPreset::BgeBase.dim(), 768);
    }

    #[test]
    fn only_e5_has_prefixes() {
        assert_eq!(ModelPreset::BgeSmall.query_prefix(), None);
        assert_eq!(ModelPreset::BgeSmall.passage_prefix(), None);
        assert_eq!(ModelPreset::E5Small.query_prefix(), Some("query: "));
        assert_eq!(ModelPreset::E5Small.passage_prefix(), Some("passage: "));
        assert_eq!(ModelPreset::BgeBase.query_prefix(), None);
        assert_eq!(ModelPreset::BgeBase.passage_prefix(), None);
    }

    #[test]
    fn from_cli_name_parses_all_presets() {
        assert_eq!(ModelPreset::from_cli_name("bge-small").unwrap(), ModelPreset::BgeSmall);
        assert_eq!(ModelPreset::from_cli_name("e5-small").unwrap(), ModelPreset::E5Small);
        assert_eq!(ModelPreset::from_cli_name("bge-base").unwrap(), ModelPreset::BgeBase);
    }

    #[test]
    fn from_cli_name_rejects_unknown() {
        let err = ModelPreset::from_cli_name("gibberish").unwrap_err();
        match err {
            EmbedError::PresetUnknown { name } => assert_eq!(name, "gibberish"),
            other => panic!("expected PresetUnknown, got {other:?}"),
        }
    }

    #[test]
    fn manifest_id_roundtrips_through_from_manifest_id() {
        for preset in [ModelPreset::BgeSmall, ModelPreset::E5Small, ModelPreset::BgeBase] {
            assert_eq!(ModelPreset::from_manifest_id(preset.manifest_id()), Some(preset));
        }
    }

    #[test]
    fn manifest_id_uses_fastrag_prefix() {
        for preset in [ModelPreset::BgeSmall, ModelPreset::E5Small, ModelPreset::BgeBase] {
            assert!(preset.manifest_id().starts_with("fastrag/"));
        }
    }
}
```

Also add to `crates/fastrag-embed/src/lib.rs` (near the top where `mod bge;` is):
```rust
pub mod candle_hf;
```

Add to the re-exports block:
```rust
pub use crate::candle_hf::ModelPreset;
```

- [ ] **Step 2: Add the `PresetUnknown` variant to `EmbedError`**

Append to `crates/fastrag-embed/src/error.rs` inside the `EmbedError` enum:
```rust
    #[error("unknown model preset `{name}` (expected one of: bge-small, e5-small, bge-base)")]
    PresetUnknown { name: String },
```
(Also add `Debug` to any `#[derive(...)]` on `EmbedError` if it's not already present — the test in Step 1 uses `{other:?}`.)

- [ ] **Step 3: Run tests**

```
cargo test -p fastrag-embed candle_hf::
```
Expected: all 6 new tests PASS.

- [ ] **Step 4: Commit**

```
git add crates/fastrag-embed/src/candle_hf.rs crates/fastrag-embed/src/lib.rs crates/fastrag-embed/src/error.rs
git commit -m "feat(embed): add ModelPreset enum for multi-model candle (#31b)"
```

---

## Task 3: Add `embed_query` / `embed_passage` default methods to `Embedder` trait

**Files:**
- Modify: `crates/fastrag-embed/src/lib.rs:14-55` (trait definition + inline tests)

- [ ] **Step 1: Write the failing test**

Add to the existing `mod trait_tests` in `crates/fastrag-embed/src/lib.rs`:
```rust
#[test]
fn default_embed_query_passes_through_to_embed() {
    let e = CountingEmbedder { calls: Default::default() };
    let out = e.embed_query(&["hello"]).unwrap();
    assert_eq!(out.len(), 1);
    assert_eq!(*e.calls.lock().unwrap(), vec![1]);
}

#[test]
fn default_embed_passage_passes_through_to_embed() {
    let e = CountingEmbedder { calls: Default::default() };
    let out = e.embed_passage(&["a", "b", "c"]).unwrap();
    assert_eq!(out.len(), 3);
    assert_eq!(*e.calls.lock().unwrap(), vec![3]);
}
```

- [ ] **Step 2: Run the test and confirm it fails to compile**

```
cargo test -p fastrag-embed trait_tests::
```
Expected: FAIL — `no method named embed_query found for ...`.

- [ ] **Step 3: Add default methods to the trait**

In `crates/fastrag-embed/src/lib.rs`, inside the `pub trait Embedder` block, directly below `embed_batched`:
```rust
    /// Embed `texts` as queries. Default impl passes through to `embed`; override
    /// for models (e.g. E5) that require a query-side prefix.
    fn embed_query(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbedError> {
        self.embed(texts)
    }

    /// Embed `texts` as passages/documents. Default impl passes through to `embed`;
    /// override for models (e.g. E5) that require a passage-side prefix.
    fn embed_passage(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbedError> {
        self.embed(texts)
    }
```

- [ ] **Step 4: Run tests**

```
cargo test -p fastrag-embed
```
Expected: PASS.

- [ ] **Step 5: Commit**

```
git add crates/fastrag-embed/src/lib.rs
git commit -m "feat(embed): add embed_query/embed_passage default trait methods (#31b)"
```

---

## Task 4: `CandleHfEmbedder` core — loading + shared forward pass

**Files:**
- Modify: `crates/fastrag-embed/src/candle_hf.rs` (extend the file from Task 2)
- Modify: `crates/fastrag-embed/src/error.rs` (add `WeightsNotCached`, `HfHubDownload`, `ModelLoad` if not already wrappable through existing `Candle`/`Io` variants)

The loading logic mirrors `BgeSmallEmbedder::from_local` / `from_hf_hub` but takes a `ModelPreset` and resolves paths through `preset.cache_subdir()` and `preset.hf_repo_id()`. Reuse the existing helpers (`mean_pool`, `l2_normalize_rows`, `ensure_exists`, `download_into`) — copy them into `candle_hf.rs` verbatim; they disappear with `bge.rs` in Task 7.

- [ ] **Step 1: Add `WeightsNotCached` error variant**

In `crates/fastrag-embed/src/error.rs`, add inside `EmbedError`:
```rust
    #[error("weights for `{model_id}` are not cached and FASTRAG_OFFLINE=1 is set; expected at `{expected_path}`. \
Pre-fetch with `cargo run -- index … --model <preset>` with FASTRAG_OFFLINE unset, or pass --model-path.")]
    WeightsNotCached { model_id: String, expected_path: std::path::PathBuf },
```
No new `HfHubDownload` or `ModelLoad` variants are needed — the existing `EmbedError::HfHub(#[from] hf_hub::api::sync::ApiError)`, `EmbedError::Candle(...)`, and `EmbedError::Io(#[from] std::io::Error)` already cover those cases (verify by grepping `EmbedError::` in `error.rs`; add only the ones missing).

- [ ] **Step 2: Write the failing unit test for `from_local` round-trip via a minimal fixture path**

Real weights can't live in CI. Instead, assert the preset wiring end-to-end with a logic test that exercises `ModelPreset` dispatch without touching candle:

Add to the `mod tests` in `crates/fastrag-embed/src/candle_hf.rs`:
```rust
#[test]
fn from_local_returns_missing_file_error_when_dir_is_empty() {
    let tmp = tempfile::tempdir().unwrap();
    let err = CandleHfEmbedder::from_local(ModelPreset::BgeSmall, tmp.path()).unwrap_err();
    match err {
        EmbedError::MissingModelFile { path } => {
            assert!(path.ends_with("tokenizer.json"));
        }
        other => panic!("expected MissingModelFile, got {other:?}"),
    }
}
```

Ensure `tempfile` is listed under `[dev-dependencies]` in `crates/fastrag-embed/Cargo.toml` (it likely is — grep to confirm).

- [ ] **Step 3: Run and confirm failure**

```
cargo test -p fastrag-embed candle_hf::tests::from_local_returns_missing_file_error_when_dir_is_empty
```
Expected: FAIL — `CandleHfEmbedder` type does not exist yet.

- [ ] **Step 4: Add `CandleHfEmbedder` + `from_local` + shared helpers**

Add to `crates/fastrag-embed/src/candle_hf.rs` (above the `#[cfg(test)]` block):

```rust
use std::fs;
use std::path::{Path, PathBuf};

use candle_core::{DType, Device, Tensor};
use candle_nn::VarBuilder;
use tokenizers::{PaddingParams, PaddingStrategy, Tokenizer, TruncationParams};

use crate::Embedder;

/// Where to load weights from.
pub enum LoadSource {
    /// Use the hf-hub cache under `~/.cache/fastrag/models/<preset>/`, downloading
    /// if missing. Honors `FASTRAG_OFFLINE=1` — an empty cache becomes `WeightsNotCached`.
    HfHub,
    /// Load directly from a local directory containing `tokenizer.json`, `config.json`,
    /// and `model.safetensors`. Never touches the network.
    LocalPath(PathBuf),
}

pub struct CandleHfEmbedder {
    preset: ModelPreset,
    device: Device,
    tokenizer: Tokenizer,
    model: candle_transformers::models::bert::BertModel,
    dim: usize,
}

impl CandleHfEmbedder {
    pub fn from_preset(preset: ModelPreset, source: LoadSource) -> Result<Self, EmbedError> {
        match source {
            LoadSource::LocalPath(dir) => Self::from_local(preset, &dir),
            LoadSource::HfHub => Self::from_hf_hub(preset),
        }
    }

    pub fn from_local(preset: ModelPreset, model_dir: &Path) -> Result<Self, EmbedError> {
        let tokenizer_path = model_dir.join("tokenizer.json");
        let config_path    = model_dir.join("config.json");
        let weights_path   = model_dir.join("model.safetensors");

        ensure_exists(&tokenizer_path)?;
        ensure_exists(&config_path)?;
        ensure_exists(&weights_path)?;

        let device = Device::Cpu;

        let mut tokenizer = Tokenizer::from_file(&tokenizer_path)?;
        tokenizer.with_padding(Some(PaddingParams {
            strategy: PaddingStrategy::BatchLongest,
            ..Default::default()
        }));
        tokenizer.with_truncation(Some(TruncationParams {
            max_length: 512,
            ..Default::default()
        }))?;

        let config_json = fs::read_to_string(&config_path)?;
        let config: candle_transformers::models::bert::Config =
            serde_json::from_str(&config_json).map_err(|e| EmbedError::Candle(e.to_string()))?;

        if config.hidden_size != preset.dim() {
            return Err(EmbedError::UnexpectedDim {
                expected: preset.dim(),
                got: config.hidden_size,
            });
        }

        let vb = unsafe {
            VarBuilder::from_mmaped_safetensors(&[weights_path], DType::F32, &device)?
        };
        let model = candle_transformers::models::bert::BertModel::load(vb, &config)?;

        Ok(Self { preset, device, tokenizer, model, dim: config.hidden_size })
    }

    pub fn from_hf_hub(preset: ModelPreset) -> Result<Self, EmbedError> {
        let base = dirs::cache_dir().ok_or(EmbedError::NoCacheDir)?;
        let model_dir = base.join("fastrag/models").join(preset.cache_subdir());
        let offline = std::env::var("FASTRAG_OFFLINE").ok().as_deref() == Some("1");

        if offline && !model_dir.join("model.safetensors").exists() {
            return Err(EmbedError::WeightsNotCached {
                model_id: preset.manifest_id().to_string(),
                expected_path: model_dir,
            });
        }

        fs::create_dir_all(&model_dir)?;
        let hf_cache_dir = base.join("fastrag/hf-hub");
        fs::create_dir_all(&hf_cache_dir)?;

        let api = hf_hub::api::sync::ApiBuilder::new()
            .with_cache_dir(hf_cache_dir)
            .build()?;
        let repo = api.model(preset.hf_repo_id().to_string());

        download_into(&repo, "tokenizer.json", &model_dir)?;
        download_into(&repo, "config.json", &model_dir)?;
        download_into(&repo, "model.safetensors", &model_dir)?;

        Self::from_local(preset, &model_dir)
    }

    /// Shared forward pass used by both `embed_query` and `embed_passage` after
    /// prefix application.
    fn forward(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbedError> {
        if texts.is_empty() {
            return Err(EmbedError::EmptyInput);
        }

        let encodings = self.tokenizer.encode_batch(texts.to_vec(), true)?;
        let batch = encodings.len();
        let seq_len = encodings.first().map(|e| e.get_ids().len()).unwrap_or(0);

        let mut input_ids: Vec<u32> = Vec::with_capacity(batch * seq_len);
        let mut attention_mask: Vec<u32> = Vec::with_capacity(batch * seq_len);
        for enc in &encodings {
            input_ids.extend_from_slice(enc.get_ids());
            attention_mask.extend_from_slice(enc.get_attention_mask());
        }

        let input_ids = Tensor::from_vec(input_ids, (batch, seq_len), &self.device)?
            .to_dtype(DType::I64)?;
        let attention_mask = Tensor::from_vec(attention_mask, (batch, seq_len), &self.device)?
            .to_dtype(DType::F32)?;
        let token_type_ids = Tensor::zeros((batch, seq_len), DType::I64, &self.device)?;

        let hidden = self.model
            .forward(&input_ids, &token_type_ids, Some(&attention_mask))?
            .to_dtype(DType::F32)?;

        let pooled = mean_pool(&hidden, &attention_mask)?;
        let normalized = l2_normalize_rows(&pooled)?;

        let vecs = normalized.to_vec2::<f32>()?;
        for v in &vecs {
            if v.len() != self.dim {
                return Err(EmbedError::UnexpectedDim { expected: self.dim, got: v.len() });
            }
        }
        Ok(vecs)
    }
}

impl Embedder for CandleHfEmbedder {
    fn model_id(&self) -> String {
        self.preset.manifest_id().to_string()
    }

    fn dim(&self) -> usize {
        self.dim
    }

    fn default_batch_size(&self) -> usize {
        // bge-base (hidden=768) gets a smaller batch to keep peak RSS comparable.
        match self.preset {
            ModelPreset::BgeBase => 16,
            _ => 32,
        }
    }

    fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbedError> {
        // Neutral form — used by callers that don't know whether they're embedding
        // queries or passages (e.g. the eval harness or a user who doesn't care).
        // E5 note: this never applies a prefix, so E5 callers MUST use
        // embed_query / embed_passage to get correct behavior.
        self.forward(texts)
    }
}

fn ensure_exists(path: &Path) -> Result<(), EmbedError> {
    if path.exists() {
        Ok(())
    } else {
        Err(EmbedError::MissingModelFile { path: path.to_path_buf() })
    }
}

fn download_into(
    repo: &hf_hub::api::sync::ApiRepo,
    filename: &str,
    model_dir: &Path,
) -> Result<(), EmbedError> {
    let dst = model_dir.join(filename);
    if dst.exists() {
        return Ok(());
    }
    let src = repo.get(filename)?;
    fs::copy(src, &dst)?;
    Ok(())
}

fn mean_pool(hidden: &Tensor, attention_mask: &Tensor) -> Result<Tensor, EmbedError> {
    let mask = attention_mask.unsqueeze(2)?;
    let masked = hidden.broadcast_mul(&mask)?;
    let summed = masked.sum(1)?;
    let denom = mask.sum(1)?.clamp(1e-9f32, f32::MAX)?;
    Ok(summed.broadcast_div(&denom)?)
}

fn l2_normalize_rows(x: &Tensor) -> Result<Tensor, EmbedError> {
    let sq = x.sqr()?;
    let sum = sq.sum(1)?;
    let norm = sum.sqrt()?.unsqueeze(1)?;
    let norm = norm.clamp(1e-12f32, f32::MAX)?;
    Ok(x.broadcast_div(&norm)?)
}
```

- [ ] **Step 5: Run tests**

```
cargo test -p fastrag-embed candle_hf::
```
Expected: all candle_hf tests PASS (including the Task 2 preset tests and the new `from_local_returns_missing_file_error_when_dir_is_empty`).

- [ ] **Step 6: Commit**

```
git add crates/fastrag-embed/src/candle_hf.rs crates/fastrag-embed/src/error.rs
git commit -m "feat(embed): CandleHfEmbedder core with from_local/from_hf_hub + FASTRAG_OFFLINE (#31b)"
```

---

## Task 5: E5 prefix override on `CandleHfEmbedder`

**Files:**
- Modify: `crates/fastrag-embed/src/candle_hf.rs`

- [ ] **Step 1: Write the failing tests**

Append to the `mod tests` in `candle_hf.rs`:

```rust
/// Helper: construct a CandleHfEmbedder without loading real weights.
/// We smuggle in a stub tokenizer and a dummy BertModel substitute by... we can't,
/// candle types are concrete. Instead we test the prefix logic at the preset level
/// by inspecting what `apply_prefix` produces.
#[test]
fn apply_query_prefix_adds_e5_prefix() {
    let texts = ["what is a CVE", "how does TLS work"];
    let prefixed = apply_prefix(ModelPreset::E5Small.query_prefix(), &texts);
    assert_eq!(
        prefixed,
        vec![
            "query: what is a CVE".to_string(),
            "query: how does TLS work".to_string(),
        ]
    );
}

#[test]
fn apply_passage_prefix_adds_e5_prefix() {
    let texts = ["some doc text"];
    let prefixed = apply_prefix(ModelPreset::E5Small.passage_prefix(), &texts);
    assert_eq!(prefixed, vec!["passage: some doc text".to_string()]);
}

#[test]
fn apply_prefix_is_noop_for_bge() {
    let texts = ["doc a", "doc b"];
    let prefixed = apply_prefix(ModelPreset::BgeSmall.passage_prefix(), &texts);
    assert_eq!(prefixed, vec!["doc a".to_string(), "doc b".to_string()]);
}
```

- [ ] **Step 2: Run and confirm failure**

```
cargo test -p fastrag-embed candle_hf::tests::apply_
```
Expected: FAIL — `apply_prefix` not defined.

- [ ] **Step 3: Implement `apply_prefix` and wire `embed_query` / `embed_passage`**

Add to `candle_hf.rs` (above the `impl Embedder for CandleHfEmbedder` block):

```rust
/// Owning variant of prefix application. We must materialize new `String`s because
/// `&str` borrowed from the preset const + the input can't cheaply produce a single
/// `&str` without allocation, and tokenizers want a slice of string-like values.
fn apply_prefix(prefix: Option<&'static str>, texts: &[&str]) -> Vec<String> {
    match prefix {
        Some(p) => texts.iter().map(|t| format!("{p}{t}")).collect(),
        None    => texts.iter().map(|t| t.to_string()).collect(),
    }
}
```

Extend the `impl Embedder for CandleHfEmbedder` block with the two overrides:

```rust
    fn embed_query(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbedError> {
        if self.preset.query_prefix().is_none() {
            return self.forward(texts);
        }
        let owned = apply_prefix(self.preset.query_prefix(), texts);
        let refs: Vec<&str> = owned.iter().map(|s| s.as_str()).collect();
        self.forward(&refs)
    }

    fn embed_passage(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbedError> {
        if self.preset.passage_prefix().is_none() {
            return self.forward(texts);
        }
        let owned = apply_prefix(self.preset.passage_prefix(), texts);
        let refs: Vec<&str> = owned.iter().map(|s| s.as_str()).collect();
        self.forward(&refs)
    }
```

- [ ] **Step 4: Run the tests**

```
cargo test -p fastrag-embed candle_hf::
```
Expected: PASS.

- [ ] **Step 5: Commit**

```
git add crates/fastrag-embed/src/candle_hf.rs
git commit -m "feat(embed): E5 prefix handling via embed_query/embed_passage overrides (#31b)"
```

---

## Task 6: Delete `BgeSmallEmbedder`, update re-exports

**Files:**
- Delete: `crates/fastrag-embed/src/bge.rs`
- Modify: `crates/fastrag-embed/src/lib.rs`
- Modify: `crates/fastrag/src/lib.rs` (re-exports)

- [ ] **Step 1: Find every call site**

```
cargo check -p fastrag-embed 2>&1 | head -20   # should still be clean
grep -rn "BgeSmallEmbedder\b" crates/ fastrag-cli/ --include='*.rs'
```
Expected: a handful of hits in `lib.rs` files, `embed_loader.rs`, and possibly tests.

- [ ] **Step 2: Delete `bge.rs` and drop its module declaration**

```
git rm crates/fastrag-embed/src/bge.rs
```
Edit `crates/fastrag-embed/src/lib.rs`:
- Delete `mod bge;`
- Delete `pub use crate::bge::BgeSmallEmbedder;`
- Add `pub use crate::candle_hf::{CandleHfEmbedder, LoadSource};` (`ModelPreset` re-export already added in Task 2)

Edit `crates/fastrag/src/lib.rs`:
- Replace any `pub use fastrag_embed::BgeSmallEmbedder;` with `pub use fastrag_embed::{CandleHfEmbedder, LoadSource, ModelPreset};`.

- [ ] **Step 3: Expect a cascade of compile errors in `embed_loader.rs` — resolved in Task 7**

```
cargo check -p fastrag-cli 2>&1 | head -30
```
Expected: errors naming `BgeSmallEmbedder`. Leave them; Task 7 fixes them in one pass.

- [ ] **Step 4: Commit the deletion alone so the diff stays reviewable**

```
git add -A crates/fastrag-embed/ crates/fastrag/src/lib.rs
git commit -m "refactor(embed): remove BgeSmallEmbedder (#31b)

CandleHfEmbedder + ModelPreset supersede it. CLI wiring in a follow-up commit."
```
(The repo is in a compile-broken state between this commit and Task 7. Acceptable because the two commits land back-to-back and neither is pushed in isolation — still, run `cargo check` after Task 7 to confirm the workspace is green again before any push.)

---

## Task 7: Rewire `embed_loader.rs` to preset-aware loading

**Files:**
- Modify: `fastrag-cli/src/embed_loader.rs`

- [ ] **Step 1: Write the failing test**

Add a new test module at the bottom of `fastrag-cli/src/embed_loader.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_from_manifest_recognizes_all_candle_presets() {
        for id in [
            "fastrag/bge-small-en-v1.5",
            "fastrag/e5-small-v2",
            "fastrag/bge-base-en-v1.5",
        ] {
            let (kind, preset, override_model) = detect_from_manifest(id).unwrap();
            assert_eq!(kind, EmbedderKindArg::Candle);
            assert!(preset.is_some(), "preset should be set for {id}");
            assert!(override_model.is_none());
        }
    }

    #[test]
    fn detect_from_manifest_passes_openai_through() {
        let (kind, preset, override_model) =
            detect_from_manifest("openai:text-embedding-3-small").unwrap();
        assert_eq!(kind, EmbedderKindArg::Openai);
        assert!(preset.is_none());
        assert_eq!(override_model.as_deref(), Some("text-embedding-3-small"));
    }

    #[test]
    fn detect_from_manifest_rejects_unknown() {
        assert!(detect_from_manifest("something-weird").is_err());
    }
}
```

- [ ] **Step 2: Run the test (will fail to compile)**

```
cargo test -p fastrag-cli embed_loader::tests:: --no-run
```
Expected: FAIL — `EmbedderKindArg::Candle` does not exist, signature of `detect_from_manifest` does not match.

- [ ] **Step 3: Replace the contents of `embed_loader.rs`**

Full new contents:

```rust
use std::path::{Path, PathBuf};
use std::sync::Arc;

use fastrag::{CandleHfEmbedder, Embedder, LoadSource, ModelPreset};
use thiserror::Error;

use crate::args::EmbedderKindArg;

#[derive(Debug, Error)]
pub enum EmbedLoaderError {
    #[error("embedding model error: {0}")]
    Embed(#[from] fastrag::EmbedderError),
    #[error("unsupported model path: {0}")]
    UnsupportedModelPath(PathBuf),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("failed to parse corpus manifest: {0}")]
    Manifest(String),
    #[error(
        "embedder mismatch: corpus built with `{existing}`, --embedder/--model specifies `{requested}`"
    )]
    Mismatch { existing: String, requested: String },
}

#[derive(Clone)]
pub struct EmbedderOptions {
    pub kind: Option<EmbedderKindArg>,
    /// Only meaningful when `kind == Candle`. `None` → bge-small default.
    pub candle_model: Option<ModelPreset>,
    pub model_path: Option<PathBuf>,
    pub openai_model: String,
    pub openai_base_url: String,
    pub ollama_model: String,
    pub ollama_url: String,
}

pub fn load_for_write(opts: &EmbedderOptions) -> Result<Arc<dyn Embedder>, EmbedLoaderError> {
    let kind = opts.kind.unwrap_or(EmbedderKindArg::Candle);
    build(kind, opts)
}

pub fn load_for_read(
    corpus_dir: &Path,
    opts: &EmbedderOptions,
) -> Result<Arc<dyn Embedder>, EmbedLoaderError> {
    let manifest_path = corpus_dir.join("manifest.json");
    let bytes = std::fs::read(&manifest_path)?;
    let value: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|e| EmbedLoaderError::Manifest(e.to_string()))?;
    let existing = value
        .get("embedding_model_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| EmbedLoaderError::Manifest("missing embedding_model_id".into()))?
        .to_string();

    let (detected_kind, detected_preset, model_override) = detect_from_manifest(&existing)?;
    let kind = opts.kind.unwrap_or(detected_kind);

    if kind != detected_kind {
        return Err(EmbedLoaderError::Mismatch {
            existing,
            requested: kind_name(kind).to_string(),
        });
    }

    let mut effective = opts.clone();
    match kind {
        EmbedderKindArg::Candle => {
            // If the user didn't pass --model, adopt the manifest's preset.
            if effective.candle_model.is_none() {
                effective.candle_model = detected_preset;
            }
        }
        EmbedderKindArg::Openai => {
            if let Some(m) = model_override {
                effective.openai_model = m;
            }
        }
        EmbedderKindArg::Ollama => {
            if let Some(m) = model_override {
                effective.ollama_model = m;
            }
        }
    }

    let emb = build(kind, &effective)?;
    let requested = emb.model_id();
    if requested != existing {
        return Err(EmbedLoaderError::Mismatch { existing, requested });
    }
    Ok(emb)
}

/// Returns `(kind, preset if candle, model override if present)`.
fn detect_from_manifest(
    existing: &str,
) -> Result<(EmbedderKindArg, Option<ModelPreset>, Option<String>), EmbedLoaderError> {
    if let Some(rest) = existing.strip_prefix("openai:") {
        Ok((EmbedderKindArg::Openai, None, Some(rest.to_string())))
    } else if let Some(rest) = existing.strip_prefix("ollama:") {
        Ok((EmbedderKindArg::Ollama, None, Some(rest.to_string())))
    } else if let Some(preset) = ModelPreset::from_manifest_id(existing) {
        Ok((EmbedderKindArg::Candle, Some(preset), None))
    } else {
        Err(EmbedLoaderError::Manifest(format!(
            "unrecognized embedding_model_id `{existing}`; pass --embedder/--model explicitly"
        )))
    }
}

fn kind_name(kind: EmbedderKindArg) -> &'static str {
    match kind {
        EmbedderKindArg::Candle => "candle",
        EmbedderKindArg::Openai => "openai",
        EmbedderKindArg::Ollama => "ollama",
    }
}

fn build(
    kind: EmbedderKindArg,
    opts: &EmbedderOptions,
) -> Result<Arc<dyn Embedder>, EmbedLoaderError> {
    match kind {
        EmbedderKindArg::Candle => {
            let preset = opts.candle_model.unwrap_or(ModelPreset::BgeSmall);
            let source = match &opts.model_path {
                Some(path) => LoadSource::LocalPath(path.clone()),
                None => LoadSource::HfHub,
            };
            let e = CandleHfEmbedder::from_preset(preset, source)?;
            Ok(Arc::new(e))
        }
        EmbedderKindArg::Openai => {
            use fastrag_embed::http::openai::OpenAIEmbedder;
            let e = OpenAIEmbedder::new(opts.openai_model.clone())?
                .with_base_url(opts.openai_base_url.clone());
            Ok(Arc::new(e))
        }
        EmbedderKindArg::Ollama => {
            use fastrag_embed::http::ollama::OllamaEmbedder;
            unsafe { std::env::set_var("OLLAMA_HOST", &opts.ollama_url) };
            let e = OllamaEmbedder::new(opts.ollama_model.clone())?;
            Ok(Arc::new(e))
        }
    }
}
```

- [ ] **Step 4: Update `EmbedderKindArg` in `fastrag-cli/src/args.rs`**

Change the enum in `fastrag-cli/src/args.rs:28-32` to:
```rust
#[cfg(feature = "retrieval")]
#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
pub enum EmbedderKindArg {
    /// Local candle-backed BERT family models (bge-small, e5-small, bge-base).
    Candle,
    /// Hidden alias for `Candle` — preserves pre-#31b `--embedder bge` usage.
    #[value(hide = true)]
    Bge,
    Openai,
    Ollama,
}
```

Anywhere in `main.rs` / `embed_loader.rs` / tests that matches on `EmbedderKindArg::Bge`, normalize early:

In `embed_loader.rs::build`, before the `match kind`, insert:
```rust
    let kind = if matches!(kind, EmbedderKindArg::Bge) {
        EmbedderKindArg::Candle
    } else {
        kind
    };
```
And the same normalization at the top of `load_for_write` and `load_for_read` (right after reading `opts.kind.unwrap_or(...)`).

- [ ] **Step 5: Run everything**

```
cargo test -p fastrag-cli embed_loader::
cargo check --workspace --features retrieval
```
Expected: loader tests PASS, workspace compiles cleanly.

- [ ] **Step 6: Commit**

```
git add fastrag-cli/src/embed_loader.rs fastrag-cli/src/args.rs
git commit -m "feat(cli): preset-aware embed_loader with Candle kind (#31b)"
```

---

## Task 8: Add `--model` CLI flag on Index / Query / ServeHttp

**Files:**
- Modify: `fastrag-cli/src/args.rs:141-...`, `:190-...`, `:292-...`
- Modify: `fastrag-cli/src/main.rs`

- [ ] **Step 1: Add `--model <bge-small|e5-small|bge-base>` flag to all three subcommands**

For each of the three Index / Query / ServeHttp struct variants in `args.rs` (the ones with `model_path: Option<PathBuf>` around lines 141/190/292), add next to `model_path`:

```rust
        /// Candle model preset when --embedder=candle (default). One of: bge-small, e5-small, bge-base.
        #[cfg(feature = "retrieval")]
        #[arg(long, value_parser = parse_model_preset, conflicts_with = "model_path")]
        model: Option<fastrag::ModelPreset>,
```

Add the parser function near the top of `args.rs`:
```rust
#[cfg(feature = "retrieval")]
fn parse_model_preset(s: &str) -> Result<fastrag::ModelPreset, String> {
    fastrag::ModelPreset::from_cli_name(s).map_err(|e| e.to_string())
}
```

(`fastrag::ModelPreset` is re-exported from the facade crate per Task 6.)

- [ ] **Step 2: Wire the flag through `EmbedderOptions` in `main.rs`**

Find every place `EmbedderOptions { … }` is constructed in `fastrag-cli/src/main.rs` (three spots: Index handler, Query handler, ServeHttp handler). Add:
```rust
        candle_model: model,
```
next to the existing `model_path: model_path.clone(),` line. (The binding name `model` comes from the destructured `model` field above.)

- [ ] **Step 3: Write and run the smoke test**

Add to `fastrag-cli/tests/model_selection_e2e.rs` (new file):

```rust
use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn model_flag_rejects_unknown_preset() {
    Command::cargo_bin("fastrag")
        .unwrap()
        .args(["index", "/tmp/does-not-matter", "--corpus", "/tmp/also-no", "--model", "gibberish"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("unknown model preset"));
}

#[test]
fn model_and_model_path_are_mutually_exclusive() {
    Command::cargo_bin("fastrag")
        .unwrap()
        .args([
            "index", "/tmp/whatever",
            "--corpus", "/tmp/also-whatever",
            "--model", "bge-small",
            "--model-path", "/tmp/models",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("cannot be used with"));
}
```

Run:
```
cargo test -p fastrag-cli --features retrieval --test model_selection_e2e
```
Expected: both tests PASS.

- [ ] **Step 4: Commit**

```
git add fastrag-cli/src/args.rs fastrag-cli/src/main.rs fastrag-cli/tests/model_selection_e2e.rs
git commit -m "feat(cli): add --model flag for candle preset selection (#31b)"
```

---

## Task 9: Index/query call sites use `embed_passage` / `embed_query`

**Files:**
- Modify: `crates/fastrag/src/corpus/mod.rs`
- Modify: `crates/fastrag-mcp/src/lib.rs` (query-side)
- Modify: `fastrag-cli/src/main.rs` (query + serve-http handlers)
- Modify: `fastrag-cli/src/eval.rs`

- [ ] **Step 1: Locate every `embedder.embed(` call and classify it**

```
grep -rn "\.embed(" crates/fastrag/src crates/fastrag-mcp fastrag-cli/src --include='*.rs'
```
Expected output: a small number of hits. For each:
- Indexing (building the index from document chunks) → `embed_passage`
- Query path (single-string or multi-string user query → vector search) → `embed_query`
- Eval harness (building queries and passages) → each side gets its matching call
- Ambiguous / caller doesn't care (e.g. health-check smoke) → leave as `embed`

- [ ] **Step 2: Write a regression test for the dispatch**

Add to `crates/fastrag-embed/src/candle_hf.rs` `mod tests` (inline, no real model needed — uses the stub pattern from `trait_tests`):

```rust
#[test]
fn e5_embed_query_and_passage_prepend_correct_prefixes_in_order() {
    // We can't exercise CandleHfEmbedder without weights, but we can exercise
    // the preset-level helper which is the full behavior under test.
    let q = apply_prefix(ModelPreset::E5Small.query_prefix(), &["find CVEs"]);
    let p = apply_prefix(ModelPreset::E5Small.passage_prefix(), &["find CVEs"]);
    assert_eq!(q, vec!["query: find CVEs".to_string()]);
    assert_eq!(p, vec!["passage: find CVEs".to_string()]);
    assert_ne!(q, p);
}
```

- [ ] **Step 3: Replace call sites**

For each call site identified in Step 1, make the minimal edit:
- `crates/fastrag/src/corpus/mod.rs` — in the indexing loop, `embedder.embed(&chunk_refs)` → `embedder.embed_passage(&chunk_refs)`.
- `crates/fastrag/src/corpus/mod.rs` — in any query helper, `embedder.embed(&[query])` → `embedder.embed_query(&[query])`.
- `crates/fastrag-mcp/src/lib.rs` — the `search_corpus` tool's query embedding call → `embed_query`.
- `fastrag-cli/src/main.rs` — Query and ServeHttp handlers' query embedding call → `embed_query`.
- `fastrag-cli/src/eval.rs` — passages (documents) → `embed_passage`, queries → `embed_query`.

No changes are required in:
- `fastrag-cli/tests/` (already use mock/http paths via CLI)
- Any openai/ollama HTTP test (they inherit the default passthrough)

- [ ] **Step 4: Run the workspace test suite**

```
cargo test --workspace --features retrieval
```
Expected: PASS.

```
cargo clippy --workspace --all-targets --features retrieval -- -D warnings
```
Expected: clean.

- [ ] **Step 5: Commit**

```
git add -A
git commit -m "feat(corpus): route index/query through embed_passage/embed_query (#31b)"
```

---

## Task 10: Real-model gated integration test

**Files:**
- Create: `crates/fastrag-embed/tests/candle_real_presets.rs`

- [ ] **Step 1: Write the gated test**

```rust
//! Real-model integration tests for CandleHfEmbedder. These download weights
//! from HuggingFace on first run and cache them under ~/.cache/fastrag/models/.
//!
//! Gated behind `FASTRAG_E2E_MODELS=1` and `#[ignore]` so CI stays fast. Run
//! locally with:
//!
//!     FASTRAG_E2E_MODELS=1 cargo test -p fastrag-embed --test candle_real_presets -- --ignored --test-threads=1
//!
//! The --test-threads=1 is because all three presets share the hf-hub cache
//! directory and concurrent downloads occasionally race.

use fastrag_embed::{CandleHfEmbedder, Embedder, LoadSource, ModelPreset};

fn enabled() -> bool {
    std::env::var("FASTRAG_E2E_MODELS").ok().as_deref() == Some("1")
}

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let na: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let nb: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    dot / (na * nb)
}

fn assert_preset_round_trip(preset: ModelPreset) {
    let e = CandleHfEmbedder::from_preset(preset, LoadSource::HfHub).unwrap();
    assert_eq!(e.dim(), preset.dim());
    assert_eq!(e.model_id(), preset.manifest_id());

    // Passage vs query for the same text must both normalize to unit length.
    let q = e.embed_query(&["find known CVEs in openssl"]).unwrap();
    let p = e.embed_passage(&["OpenSSL CVE-2022-0778 infinite loop in BN_mod_sqrt"]).unwrap();
    assert_eq!(q[0].len(), preset.dim());
    assert_eq!(p[0].len(), preset.dim());
    let qn: f32 = q[0].iter().map(|x| x * x).sum::<f32>().sqrt();
    let pn: f32 = p[0].iter().map(|x| x * x).sum::<f32>().sqrt();
    assert!((qn - 1.0).abs() < 1e-4, "query vec not unit-norm: {qn}");
    assert!((pn - 1.0).abs() < 1e-4, "passage vec not unit-norm: {pn}");

    // Query-passage cosine for related texts must beat query-unrelated cosine.
    let unrelated = e.embed_passage(&["chocolate chip cookie recipe"]).unwrap();
    let related_sim = cosine(&q[0], &p[0]);
    let unrelated_sim = cosine(&q[0], &unrelated[0]);
    assert!(
        related_sim > unrelated_sim + 0.05,
        "preset {:?}: related {:.3} not meaningfully > unrelated {:.3}",
        preset, related_sim, unrelated_sim
    );
}

#[test]
#[ignore]
fn bge_small_end_to_end() {
    if !enabled() { return; }
    assert_preset_round_trip(ModelPreset::BgeSmall);
}

#[test]
#[ignore]
fn e5_small_end_to_end() {
    if !enabled() { return; }
    assert_preset_round_trip(ModelPreset::E5Small);
}

#[test]
#[ignore]
fn bge_base_end_to_end() {
    if !enabled() { return; }
    assert_preset_round_trip(ModelPreset::BgeBase);
}
```

- [ ] **Step 2: Run locally (do not add to CI)**

```
FASTRAG_E2E_MODELS=1 cargo test -p fastrag-embed --test candle_real_presets -- --ignored --test-threads=1
```
Expected: three tests PASS after weight downloads. First run may take several minutes depending on bandwidth.

- [ ] **Step 3: Run the normal test suite and confirm these stay ignored**

```
cargo test -p fastrag-embed --test candle_real_presets
```
Expected: 3 ignored, 0 run, 0 failed.

- [ ] **Step 4: Commit**

```
git add crates/fastrag-embed/tests/candle_real_presets.rs
git commit -m "test(embed): real-model end-to-end for all three presets (#31b)

Gated on FASTRAG_E2E_MODELS=1, #[ignore]d in CI."
```

---

## Task 11: CLI e2e — manifest mismatch across presets

**Files:**
- Modify: `fastrag-cli/tests/model_selection_e2e.rs`

This test uses the real candle path for bge-small only (small enough to be acceptable in local runs) and is gated behind `FASTRAG_E2E_MODELS=1` for the one test that needs real weights. Pure-logic tests (mutual exclusion, unknown preset) stay ungated.

- [ ] **Step 1: Add the gated mismatch test**

Append to `fastrag-cli/tests/model_selection_e2e.rs`:

```rust
#[test]
#[ignore]
fn manifest_mismatch_between_presets_is_rejected() {
    if std::env::var("FASTRAG_E2E_MODELS").ok().as_deref() != Some("1") {
        return;
    }

    let docs = tempfile::tempdir().unwrap();
    std::fs::write(docs.path().join("a.txt"), "the quick brown fox").unwrap();

    let corpus = tempfile::tempdir().unwrap();

    // Index with bge-small.
    Command::cargo_bin("fastrag")
        .unwrap()
        .args([
            "index", docs.path().to_str().unwrap(),
            "--corpus", corpus.path().to_str().unwrap(),
            "--model", "bge-small",
        ])
        .assert()
        .success();

    // Query with e5-small → should fail with mismatch.
    Command::cargo_bin("fastrag")
        .unwrap()
        .args([
            "query", "quick fox",
            "--corpus", corpus.path().to_str().unwrap(),
            "--model", "e5-small",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("embedder mismatch"));
}
```

- [ ] **Step 2: Run locally**

```
FASTRAG_E2E_MODELS=1 cargo test -p fastrag-cli --features retrieval --test model_selection_e2e -- --ignored --test-threads=1
```
Expected: PASS. (CI will skip via `#[ignore]`.)

- [ ] **Step 3: Commit**

```
git add fastrag-cli/tests/model_selection_e2e.rs
git commit -m "test(cli): preset mismatch e2e (#31b)"
```

---

## Task 12: `FASTRAG_OFFLINE` integration test

**Files:**
- Modify: `crates/fastrag-embed/tests/candle_real_presets.rs` (add offline case) OR create a dedicated unit test in `candle_hf.rs` using a nonexistent cache dir.

The cleanest approach is a unit test that doesn't need real weights: point `dirs::cache_dir()` at a tempdir (not possible via a setter — `dirs::cache_dir` reads `$XDG_CACHE_HOME` on Linux), so set `XDG_CACHE_HOME` to a tempdir, set `FASTRAG_OFFLINE=1`, and assert `from_hf_hub` returns `WeightsNotCached` without hitting the network.

- [ ] **Step 1: Write the test**

Add to `mod tests` in `crates/fastrag-embed/src/candle_hf.rs`:

```rust
#[test]
fn from_hf_hub_errors_when_offline_and_not_cached() {
    // Redirect XDG_CACHE_HOME so we get a guaranteed-empty cache, then flip
    // FASTRAG_OFFLINE on. Must run single-threaded because we mutate env.
    let tmp = tempfile::tempdir().unwrap();
    // SAFETY: tests in this crate that touch env run with --test-threads=1
    // (per crate-level Cargo.toml config or via the caller). Same discipline
    // as the HTTP embedder tests from #31a.
    unsafe {
        std::env::set_var("XDG_CACHE_HOME", tmp.path());
        std::env::set_var("FASTRAG_OFFLINE", "1");
    }

    let result = CandleHfEmbedder::from_hf_hub(ModelPreset::BgeSmall);

    unsafe {
        std::env::remove_var("FASTRAG_OFFLINE");
        std::env::remove_var("XDG_CACHE_HOME");
    }

    match result {
        Err(EmbedError::WeightsNotCached { model_id, expected_path }) => {
            assert_eq!(model_id, "fastrag/bge-small-en-v1.5");
            assert!(expected_path.starts_with(tmp.path()));
        }
        other => panic!("expected WeightsNotCached, got {other:?}"),
    }
}
```

- [ ] **Step 2: Ensure the embed crate's tests run single-threaded for env-mutating tests**

Check `crates/fastrag-embed/Cargo.toml` or the #31a HTTP tests — the HTTP tests already establish the `--test-threads=1` requirement and likely document it. If there's a `[package.metadata]` note or a CI snippet, add this new test to the same group. Otherwise leave a comment in the test body and document in the commit message.

- [ ] **Step 3: Run**

```
cargo test -p fastrag-embed -- --test-threads=1
```
Expected: PASS.

- [ ] **Step 4: Commit**

```
git add crates/fastrag-embed/src/candle_hf.rs
git commit -m "test(embed): FASTRAG_OFFLINE unit test for WeightsNotCached (#31b)"
```

---

## Task 13: Eval harness `--model` support

**Files:**
- Modify: `fastrag-cli/src/eval.rs`
- Modify: `fastrag-cli/src/args.rs` (Eval subcommand, if it exists)

If the eval harness is a top-level subcommand (inspect with `grep -n 'Eval\|EvalRun' fastrag-cli/src/args.rs`), add `--model <preset>` and plumb through `EmbedderOptions` the same way Task 8 did for Index/Query. If the eval entry point is a standalone binary under `crates/`, mirror the change there.

- [ ] **Step 1: Locate the eval entry point**

```
grep -rn "fn.*eval\|Eval " fastrag-cli/src/ --include='*.rs'
```

- [ ] **Step 2: Add `--model` to its arg struct and route through `EmbedderOptions::candle_model`**

(Exact edit depends on the eval harness shape; apply the same pattern as Task 8. If there is no dedicated eval subcommand and the eval is an external script, skip this task and mark it N/A in the commit.)

- [ ] **Step 3: Smoke test with the MockEmbedder path if available, otherwise `cargo check`**

```
cargo check -p fastrag-cli --features retrieval
```
Expected: clean.

- [ ] **Step 4: Commit**

```
git add fastrag-cli/src/eval.rs fastrag-cli/src/args.rs
git commit -m "feat(cli): eval harness accepts --model preset (#31b)"
```

---

## Task 14: Run the eval matrix

**Files:**
- Create: `docs/evals/31b-bge-small-security.json`
- Create: `docs/evals/31b-e5-small-security.json`
- Create: `docs/evals/31b-bge-base-security.json`
- Create: `docs/evals/31b-bge-small-nfcorpus.json`
- Create: `docs/evals/31b-e5-small-nfcorpus.json`
- Create: `docs/evals/31b-bge-base-nfcorpus.json`

This is a local-only task — CI does not run it. The outputs are committed so the comparison is reproducible and auditable.

- [ ] **Step 1: Confirm the eval harness exists and knows how to load both datasets**

```
ls docs/evals/ 2>/dev/null   # should already contain the #25 baseline outputs
grep -rn 'security\|nfcorpus' fastrag-cli/src/eval.rs
```
If NFCorpus isn't wired, this is the first work: the #25 baselines established the harness for the security corpus; NFCorpus loading may already be there or may need a small loader. If adding NFCorpus is more than ~40 lines, stop and open a follow-up issue — this plan assumes the existing eval harness already knows both datasets (per #25).

- [ ] **Step 2: Run each of the six combinations**

```
export FASTRAG_E2E_MODELS=1
# Pattern: cargo run --release -p fastrag-cli -- eval --model <preset> --dataset <dataset> --out docs/evals/31b-<preset>-<dataset>.json
for preset in bge-small e5-small bge-base; do
  for dataset in security nfcorpus; do
    cargo run --release -p fastrag-cli -- eval \
      --model "$preset" \
      --dataset "$dataset" \
      --out "docs/evals/31b-${preset}-${dataset}.json"
  done
done
```
Expected: six JSON files under `docs/evals/` with `ndcg_at_10`, `recall_at_10`, `mrr`, `index_seconds`, `query_p50_ms`, `query_p95_ms`, `peak_rss_mb`, and `index_bytes_on_disk` fields.

- [ ] **Step 3: Commit the raw results**

```
git add docs/evals/31b-*.json
git commit -m "eval: #31b matrix — bge-small/e5-small/bge-base × security/nfcorpus"
```

---

## Task 15: `docs/embedder-eval.md` summary + default-selection decision

**Files:**
- Create: `docs/embedder-eval.md`
- Modify: `README.md`

- [ ] **Step 1: Compose the summary markdown**

Create `docs/embedder-eval.md` with the following structure (fill in numbers from the Task 14 JSON files):

```markdown
# Embedder Evaluation (#31)

Three candle-backed BERT-family presets, evaluated on two datasets with the
fastrag eval harness established in #25. Raw per-run JSON lives under
`docs/evals/31b-*.json`.

## Summary

| Model | Dataset  | nDCG@10 | Recall@10 | MRR | Index (s) | Query p50 (ms) | Query p95 (ms) | Peak RSS (MB) | Index size (MB) |
|-------|----------|---------|-----------|-----|-----------|----------------|----------------|---------------|-----------------|
| bge-small | security  | …       | …         | …   | …         | …              | …              | …             | …               |
| bge-small | nfcorpus  | …       | …         | …   | …         | …              | …              | …             | …               |
| e5-small  | security  | …       | …         | …   | …         | …              | …              | …             | …               |
| e5-small  | nfcorpus  | …       | …         | …   | …         | …              | …              | …             | …               |
| bge-base  | security  | …       | …         | …   | …         | …              | …              | …             | …               |
| bge-base  | nfcorpus  | …       | …         | …   | …         | …              | …              | …             | …               |

## Decision

Default: **bge-small** / **<winner>**.

Rule: switch the default only when a challenger beats bge-small on nDCG@10 by
≥ 5% *on the security corpus*. (The security set is closer to fastrag's primary
user population than NFCorpus.)

Observed margin on security corpus:
- e5-small vs bge-small: <+X.X%>
- bge-base vs bge-small:  <+X.X%>

[If either challenger meets the bar, summarize the trade-off: "+7% nDCG@10 at
2.3× the RSS and 1.9× the index size — worth it / not worth it because …".]

## E5 prefix convention

e5-small requires `"query: "` / `"passage: "` prefixes. CandleHfEmbedder
applies them automatically via `Embedder::embed_query` / `embed_passage`; no
user action required. The `manifest_id` field in the corpus manifest records
which preset the index was built with, and `load_for_read` refuses mismatched
queries with `EmbedLoaderError::Mismatch`.

## Reproducing

```
FASTRAG_E2E_MODELS=1 cargo run --release -p fastrag-cli -- eval \
  --model <bge-small|e5-small|bge-base> \
  --dataset <security|nfcorpus> \
  --out docs/evals/31b-<preset>-<dataset>.json
```
```

- [ ] **Step 2: Update README**

In `README.md`, find the retrieval / embedder section (it currently documents the `--embedder bge|openai|ollama` trio from #31a). Add:

```markdown
### Candle model presets

`--embedder candle` (default) supports three presets via `--model`:

| Preset      | HF repo                          | Dim | Notes |
|-------------|----------------------------------|-----|-------|
| `bge-small` | `BAAI/bge-small-en-v1.5`         | 384 | Default. No prefixes. |
| `e5-small`  | `intfloat/e5-small-v2`            | 384 | Applies `query: `/`passage: ` automatically. |
| `bge-base`  | `BAAI/bge-base-en-v1.5`           | 768 | Bigger + slower. See `docs/embedder-eval.md` for the cost/quality trade. |

Weights are downloaded on first use into `~/.cache/fastrag/models/<preset>/`.
For air-gapped engagements, set `FASTRAG_OFFLINE=1` (errors if weights are not
already cached) or pass `--model-path <dir>` to load from a local directory
containing `tokenizer.json`, `config.json`, `model.safetensors`.

`--embedder bge` is a hidden alias for `--embedder candle --model bge-small`
and remains supported for existing scripts.
```

- [ ] **Step 3: Pass the README edit through doc-editor skill before writing**

Per `CLAUDE.md`: "Before every Edit or Write to a .md file — mandatory". Invoke the `doc-editor` skill with the draft README chunk before committing it.

- [ ] **Step 4: Commit**

```
git add docs/embedder-eval.md README.md
git commit -m "docs: embedder eval matrix + README preset docs (#31b)

Closes #31"
```

---

## Task 16: Final verification

- [ ] **Step 1: Full workspace gate**

```
cargo fmt --check
cargo clippy --workspace --all-targets --features retrieval -- -D warnings
cargo test --workspace --features retrieval
```
Expected: all three PASS.

- [ ] **Step 2: Push**

```
git push -u origin <branch-name>
```

- [ ] **Step 3: Invoke ci-watcher skill (mandatory, background Haiku Agent) to watch all workflows**

Per `CLAUDE.md`, after every push: invoke the `ci-watcher.md` skill as a background Haiku Agent call. Do not use ad-hoc `gh run watch`.

- [ ] **Step 4: Open the PR once CI is green**

```
gh pr create --title "feat(embed): multi-model candle embedders (#31b)" --body "$(cat <<'EOF'
## Summary
- Replace BgeSmallEmbedder with CandleHfEmbedder keyed by ModelPreset (bge-small, e5-small, bge-base)
- Add embed_query/embed_passage default methods on Embedder trait; E5 overrides apply its "query: "/"passage: " prefixes automatically
- Rewire embed_loader.rs to preset-aware loading, preserving the --embedder bge alias for back-compat
- FASTRAG_OFFLINE=1 blocks hf-hub downloads and errors on empty caches with a clear message
- Run and commit eval matrix (3 models × 2 datasets) under docs/evals/; docs/embedder-eval.md records the default-selection decision

Closes #31

## Test plan
- [ ] cargo test --workspace --features retrieval
- [ ] cargo clippy --workspace --all-targets --features retrieval -- -D warnings
- [ ] FASTRAG_E2E_MODELS=1 cargo test -p fastrag-embed --test candle_real_presets -- --ignored --test-threads=1
- [ ] FASTRAG_E2E_MODELS=1 cargo test -p fastrag-cli --features retrieval --test model_selection_e2e -- --ignored --test-threads=1
- [ ] Manual: FASTRAG_OFFLINE=1 with empty cache returns WeightsNotCached
- [ ] Manual: index with --model bge-small, query with --model e5-small → mismatch error
EOF
)"
```
