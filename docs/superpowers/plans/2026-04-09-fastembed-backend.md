# fastembed-rs Backend + Flagship Presets Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace candle/BGE-small as the default embedding backend with fastembed-rs (ort + tokenizers) and ship two flagship presets — `arctic-embed-m-v1.5` (new default) and `nomic-embed-v1.5` (long-context, asymmetric) — while preserving the static `Embedder` trait invariants shipped in Step 1.

**Architecture:** New `fastembed` module under `crates/fastrag-embed/` gated by a default-on `fastembed` feature. Two concrete preset structs each own a `TextEmbedding` and hand-implement the static `Embedder` trait with their own const `DIM / MODEL_ID / PREFIX_SCHEME`. A private `fastembed_batch` helper centralizes the call/error-map path. Candle BGE moves behind a new `legacy-candle` feature (off by default) so existing manifest-v3 BGE corpora still load.

**Tech Stack:** Rust, `fastembed` crate (ort + tokenizers under the hood), existing `fastrag-embed` trait surface, `dirs` crate for XDG cache resolution.

**Spec:** `docs/superpowers/specs/2026-04-09-fastembed-backend-design.md`

---

## File Structure

**Modified:**
- `crates/fastrag-embed/Cargo.toml` — add `fastembed` optional dep, new `fastembed` + `legacy-candle` features; make candle deps optional
- `crates/fastrag-embed/src/lib.rs` — gate `mod bge` + `BgeSmallEmbedder` re-export behind `legacy-candle`; add `mod fastembed`
- `crates/fastrag-embed/src/error.rs` — add `EmbedError::ModelDownload(String)` if not already present
- `crates/fastrag/Cargo.toml` — propagate new features
- `fastrag-cli/Cargo.toml` — default features switch to `fastembed`
- `fastrag-cli/src/embed_loader.rs` (or wherever model selection lives) — wire new preset names
- `fastrag-cli/src/args.rs` — add `--embedder` values `arctic-embed-m-v1.5`, `nomic-embed-v1.5`
- `README.md` — document new default, legacy-candle migration path, new cache dir
- `.github/workflows/ci.yml` — ensure default build exercises `fastembed`; add a `legacy-candle` build-only job
- existing tests that construct `BgeSmallEmbedder` directly — gate behind `#[cfg(feature = "legacy-candle")]`

**Created:**
- `crates/fastrag-embed/src/fastembed/mod.rs` — module root, re-exports
- `crates/fastrag-embed/src/fastembed/cache.rs` — XDG cache dir resolution
- `crates/fastrag-embed/src/fastembed/shared.rs` — `fastembed_batch` helper + `init_text_embedding`
- `crates/fastrag-embed/src/fastembed/arctic.rs` — `ArcticEmbedMV15` preset
- `crates/fastrag-embed/src/fastembed/nomic.rs` — `NomicEmbedV15` preset
- `crates/fastrag-embed/tests/fastembed_presets.rs` — integration tests (round-trip, mismatch)
- `crates/fastrag/tests/fastembed_corpus_roundtrip.rs` — corpus build/load with each preset

---

## Important Context for Implementer

1. **The static `Embedder` trait already takes slices** (`fn embed_query(&self, &[QueryText])`). You do NOT need to add a new batch method. Just implement the existing trait.
2. **Prefix scheme constant is `PrefixScheme::NONE`**, not `SYMMETRIC`. Use `PrefixScheme::NONE` for arctic.
3. **`EmbedError` variants** — check `crates/fastrag-embed/src/error.rs` before adding `ModelDownload`; reuse existing variants where semantically valid.
4. **Candle is currently an unconditional dependency.** Making it optional is an atomic change — the `bge` module, its `pub use`, and all existing internal consumers must be gated in the same commit or the build breaks.
5. **Existing tests that reach for `BgeSmallEmbedder` directly** will break when candle goes behind `legacy-candle`. Run `grep -rn BgeSmallEmbedder crates/ fastrag-cli/ tests/` before starting; every hit needs a `#[cfg(feature = "legacy-candle")]` gate or conversion to a preset-agnostic test double.
6. **fastembed-rs model download on first use.** Tests that construct a real preset must either (a) be marked `#[ignore]` with a comment telling the developer how to run them, or (b) run in a CI job that pre-populates `$FASTRAG_MODEL_CACHE`. This plan uses (a).
7. **Commit frequently.** Each task ends in a commit. Run `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings` before every commit.

---

## Task 1: Add fastembed dependency and feature flags

**Files:**
- Modify: `crates/fastrag-embed/Cargo.toml`

- [ ] **Step 1: Make candle deps optional and add fastembed dep**

Edit `crates/fastrag-embed/Cargo.toml`:

```toml
[dependencies]
thiserror.workspace = true
serde.workspace = true

# ML / model loading — legacy candle path (behind legacy-candle feature)
candle-core = { version = "0.10.2", optional = true }
candle-nn = { version = "0.10.2", optional = true }
candle-transformers = { version = "0.10.2", optional = true }
tokenizers = { version = "0.22.2", optional = true }
hf-hub = { version = "0.5.0", default-features = false, features = ["ureq"], optional = true }

# fastembed-rs path (default)
fastembed = { version = "4", optional = true }

dirs = "6.0.0"
serde_json = "1"
reqwest = { version = "0.12", default-features = false, features = ["blocking", "json", "rustls-tls"], optional = true }

[features]
default = ["fastembed"]
fastembed = ["dep:fastembed"]
legacy-candle = ["dep:candle-core", "dep:candle-nn", "dep:candle-transformers", "dep:tokenizers", "dep:hf-hub"]
test-utils = []
http-embedders = ["dep:reqwest"]
```

> **Note:** Verify the latest fastembed-rs major version with `cargo search fastembed` before pinning. Update the pin if a newer stable is available.

- [ ] **Step 2: Verify workspace still builds with default features**

Run: `cargo check -p fastrag-embed`
Expected: PASS (no code changes yet; candle deps still build because we haven't gated their use sites).

Actually this WILL fail because `mod bge` in `lib.rs` still pulls candle unconditionally. That's Task 2 — we fix it there. If this check fails with unresolved `candle_core`, that's expected; proceed to Task 2.

- [ ] **Step 3: Commit**

```bash
cargo fmt
git add crates/fastrag-embed/Cargo.toml
git commit -m "build(embed): add fastembed dep, make candle optional behind legacy-candle"
```

---

## Task 2: Gate candle BGE module behind legacy-candle

**Files:**
- Modify: `crates/fastrag-embed/src/lib.rs`

- [ ] **Step 1: Gate the bge module and re-export**

Edit `crates/fastrag-embed/src/lib.rs`:

```rust
#[cfg(feature = "legacy-candle")]
mod bge;
mod error;

#[cfg(feature = "http-embedders")]
pub mod http;

#[cfg(feature = "test-utils")]
pub mod test_utils;

#[cfg(feature = "fastembed")]
pub mod fastembed;

#[cfg(feature = "legacy-candle")]
pub use crate::bge::BgeSmallEmbedder;
pub use crate::error::EmbedError;
```

- [ ] **Step 2: Verify default build now succeeds**

Run: `cargo check -p fastrag-embed`
Expected: PASS. Candle is no longer pulled in; `fastembed` module doesn't exist yet but isn't referenced.

- [ ] **Step 3: Verify legacy-candle build still works**

Run: `cargo check -p fastrag-embed --no-default-features --features legacy-candle`
Expected: PASS.

- [ ] **Step 4: Verify test-only build still works**

Run: `cargo test -p fastrag-embed --lib --no-run`
Expected: compiles (core type tests only).

- [ ] **Step 5: Commit**

```bash
cargo fmt
git add crates/fastrag-embed/src/lib.rs
git commit -m "refactor(embed): gate candle BGE module behind legacy-candle feature"
```

---

## Task 3: Fix downstream breakage from gated BgeSmallEmbedder

**Files:**
- Search the whole workspace: `grep -rn BgeSmallEmbedder crates/ fastrag-cli/ tests/`
- Modify each hit

- [ ] **Step 1: Enumerate hits**

Run: `grep -rn BgeSmallEmbedder crates/ fastrag-cli/ tests/`

Expected: a list of files. Triage each:
- Test files that construct `BgeSmallEmbedder` for smoke tests → add `#[cfg(feature = "legacy-candle")]` at the module or test level.
- Library code (e.g., `fastrag-cli/src/embed_loader.rs`) that selects BGE by default → the default path will move to a fastembed preset in Task 9; for now, gate the candle branch behind `#[cfg(feature = "legacy-candle")]` and leave a `TODO(Task 9): wire fastembed default` comment. A todo is acceptable **only** within the plan's lifetime and only if a later task resolves it; this one is resolved in Task 9.
- Eval/integration code under `crates/fastrag-eval/` — gate the same way.

- [ ] **Step 2: Apply cfg gates**

For each hit, wrap the relevant `use`, function, or test in `#[cfg(feature = "legacy-candle")]`. Example for a test:

```rust
#[cfg(feature = "legacy-candle")]
#[test]
fn bge_loads_and_embeds() {
    let embedder = BgeSmallEmbedder::load_default().unwrap();
    // ...
}
```

- [ ] **Step 3: Verify default build of the full workspace**

Run: `cargo check --workspace`
Expected: PASS.

- [ ] **Step 4: Verify default test build of the full workspace**

Run: `cargo test --workspace --no-run`
Expected: PASS.

- [ ] **Step 5: Verify legacy-candle build of the full workspace**

Run: `cargo check --workspace --no-default-features --features legacy-candle`
Expected: PASS. If a crate doesn't expose the feature, add a passthrough (`legacy-candle = ["fastrag-embed/legacy-candle"]`) to its `Cargo.toml`.

- [ ] **Step 6: Commit**

```bash
cargo fmt
git add -A
git commit -m "refactor: gate BgeSmallEmbedder call sites behind legacy-candle"
```

---

## Task 4: Add ModelDownload error variant

**Files:**
- Modify: `crates/fastrag-embed/src/error.rs`

- [ ] **Step 1: Read the existing error enum**

Read `crates/fastrag-embed/src/error.rs`. If a variant already covers "model fetch failure" (e.g., a generic `Io(String)` or `Download(String)`), skip adding a new one and note the reuse. Otherwise proceed.

- [ ] **Step 2: Write failing test for the new variant**

Add to the `#[cfg(test)] mod tests` block in `error.rs` (create the mod if absent):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_download_error_formats_with_source() {
        let err = EmbedError::ModelDownload("connection refused".into());
        let msg = format!("{err}");
        assert!(msg.contains("model download"), "got: {msg}");
        assert!(msg.contains("connection refused"), "got: {msg}");
    }
}
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test -p fastrag-embed error::tests::model_download_error_formats_with_source`
Expected: FAIL — `EmbedError::ModelDownload` does not exist.

- [ ] **Step 4: Add the variant**

Edit `crates/fastrag-embed/src/error.rs` — add to the enum:

```rust
#[error("model download failed: {0}")]
ModelDownload(String),
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test -p fastrag-embed error::tests::model_download_error_formats_with_source`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
cargo fmt
git add crates/fastrag-embed/src/error.rs
git commit -m "feat(embed): add EmbedError::ModelDownload variant"
```

---

## Task 5: Cache directory resolution

**Files:**
- Create: `crates/fastrag-embed/src/fastembed/mod.rs`
- Create: `crates/fastrag-embed/src/fastembed/cache.rs`

- [ ] **Step 1: Create the module root**

Create `crates/fastrag-embed/src/fastembed/mod.rs`:

```rust
//! fastembed-rs backend: ort + tokenizers via the `fastembed` crate.
//!
//! Presets implement the static `Embedder` trait with their own
//! `DIM / MODEL_ID / PREFIX_SCHEME` constants so the manifest identity
//! invariant from Step 1 is enforced at compile time wherever the concrete
//! preset is known.

mod cache;
mod shared;

pub(crate) use cache::fastembed_cache_dir;
```

- [ ] **Step 2: Write failing test for cache dir**

Create `crates/fastrag-embed/src/fastembed/cache.rs`:

```rust
use std::path::PathBuf;

/// Resolve the fastembed model cache directory.
///
/// Honours `XDG_CACHE_HOME` via the `dirs` crate, falling back to
/// `~/.cache` on Linux / `~/Library/Caches` on macOS / `%LOCALAPPDATA%` on
/// Windows. Result: `<cache>/fastrag/models/fastembed/`.
pub(crate) fn fastembed_cache_dir() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("fastrag")
        .join("models")
        .join("fastembed")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_dir_ends_with_fastrag_models_fastembed() {
        let p = fastembed_cache_dir();
        let tail: Vec<_> = p
            .components()
            .rev()
            .take(3)
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect();
        assert_eq!(tail, vec!["fastembed", "models", "fastrag"]);
    }

    #[test]
    fn cache_dir_is_absolute_or_dot_relative() {
        let p = fastembed_cache_dir();
        assert!(
            p.is_absolute() || p.starts_with("."),
            "cache dir not plausible: {p:?}"
        );
    }
}
```

- [ ] **Step 3: Run test to verify it passes**

Run: `cargo test -p fastrag-embed fastembed::cache`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
cargo fmt
git add crates/fastrag-embed/src/fastembed/
git commit -m "feat(embed): add fastembed module skeleton with XDG cache resolution"
```

---

## Task 6: Shared fastembed_batch helper

**Files:**
- Create: `crates/fastrag-embed/src/fastembed/shared.rs`

- [ ] **Step 1: Write the shared module**

Create `crates/fastrag-embed/src/fastembed/shared.rs`:

```rust
use crate::EmbedError;
use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};

use super::cache::fastembed_cache_dir;

/// Initialize a `TextEmbedding` for the given fastembed model, using the
/// shared fastrag cache directory.
pub(crate) fn init_text_embedding(
    model: EmbeddingModel,
) -> Result<TextEmbedding, EmbedError> {
    let opts = InitOptions::new(model).with_cache_dir(fastembed_cache_dir());
    TextEmbedding::try_new(opts)
        .map_err(|e| EmbedError::ModelDownload(e.to_string()))
}

/// Run a batch embed through a `TextEmbedding`, mapping errors to
/// `EmbedError`. `batch_size` of `None` lets fastembed pick its default.
pub(crate) fn fastembed_batch(
    inner: &TextEmbedding,
    texts: Vec<String>,
    batch_size: Option<usize>,
) -> Result<Vec<Vec<f32>>, EmbedError> {
    inner
        .embed(texts, batch_size)
        .map_err(|e| EmbedError::Inference(e.to_string()))
}
```

> **Note:** If `EmbedError::Inference` doesn't exist in the current error enum, pick the closest existing variant or add one in the same commit (repeating the Task 4 pattern: red test → variant → green).

- [ ] **Step 2: Register the module**

Edit `crates/fastrag-embed/src/fastembed/mod.rs`:

```rust
mod cache;
mod shared;
pub mod arctic;   // added in Task 7
pub mod nomic;    // added in Task 8

pub use arctic::ArcticEmbedMV15;
pub use nomic::NomicEmbedV15;

pub(crate) use cache::fastembed_cache_dir;
pub(crate) use shared::{fastembed_batch, init_text_embedding};
```

> Leave the `arctic` / `nomic` lines commented out for now; uncomment as each task lands. Or add a `#[allow(dead_code)]` placeholder now — either is fine, just don't ship unresolved imports.

- [ ] **Step 3: Verify it compiles**

Run: `cargo check -p fastrag-embed`
Expected: PASS (helpers are `pub(crate)` and unused, which is allowed; add `#[allow(dead_code)]` on the functions if clippy complains).

- [ ] **Step 4: Commit**

```bash
cargo fmt
cargo clippy -p fastrag-embed --all-targets -- -D warnings
git add crates/fastrag-embed/src/fastembed/
git commit -m "feat(embed): add fastembed shared helpers (init + batch)"
```

---

## Task 7: ArcticEmbedMV15 preset

**Files:**
- Create: `crates/fastrag-embed/src/fastembed/arctic.rs`

- [ ] **Step 1: Write failing compile-time invariant test**

Create `crates/fastrag-embed/src/fastembed/arctic.rs`:

```rust
use crate::{
    DynEmbedderTrait, EmbedError, Embedder, EmbedderIdentity, PassageText, PrefixScheme,
    QueryText,
};
use fastembed::{EmbeddingModel, TextEmbedding};

use super::shared::{fastembed_batch, init_text_embedding};

/// Snowflake Arctic Embed Medium v1.5. Symmetric (no query/passage prefixes),
/// 768-dimensional, ~110M params, top-tier small-model MTEB retrieval scores.
///
/// Default preset for fastrag corpora.
pub struct ArcticEmbedMV15 {
    inner: TextEmbedding,
}

impl ArcticEmbedMV15 {
    /// Load the model, downloading it into the fastrag cache on first use.
    pub fn load() -> Result<Self, EmbedError> {
        // NOTE: verify the exact fastembed variant enum name at implementation
        // time; pick the quantized variant if fastembed-rs exposes one in the
        // pinned version, otherwise fall back to the f32 variant. The chosen
        // variant must be reflected in MODEL_ID below.
        let inner = init_text_embedding(EmbeddingModel::SnowflakeArcticEmbedMV15)?;
        Ok(Self { inner })
    }
}

impl Embedder for ArcticEmbedMV15 {
    const DIM: usize = 768;
    const MODEL_ID: &'static str = "fastrag/arctic-embed-m-v1.5";
    const PREFIX_SCHEME: PrefixScheme = PrefixScheme::NONE;

    fn embed_query(&self, texts: &[QueryText]) -> Result<Vec<Vec<f32>>, EmbedError> {
        let owned: Vec<String> = texts.iter().map(|t| t.as_str().to_owned()).collect();
        fastembed_batch(&self.inner, owned, None)
    }

    fn embed_passage(&self, texts: &[PassageText]) -> Result<Vec<Vec<f32>>, EmbedError> {
        let owned: Vec<String> = texts.iter().map(|t| t.as_str().to_owned()).collect();
        fastembed_batch(&self.inner, owned, None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arctic_has_768_dim() {
        assert_eq!(<ArcticEmbedMV15 as Embedder>::DIM, 768);
    }

    #[test]
    fn arctic_has_stable_model_id() {
        assert_eq!(
            <ArcticEmbedMV15 as Embedder>::MODEL_ID,
            "fastrag/arctic-embed-m-v1.5"
        );
    }

    #[test]
    fn arctic_is_symmetric() {
        let scheme = <ArcticEmbedMV15 as Embedder>::PREFIX_SCHEME;
        assert_eq!(scheme.query, "");
        assert_eq!(scheme.passage, "");
        assert_eq!(scheme.hash(), PrefixScheme::NONE.hash());
    }

    #[test]
    fn arctic_identity_round_trips_through_json() {
        let id = EmbedderIdentity {
            model_id: <ArcticEmbedMV15 as Embedder>::MODEL_ID.to_string(),
            dim: <ArcticEmbedMV15 as Embedder>::DIM,
            prefix_scheme_hash: <ArcticEmbedMV15 as Embedder>::PREFIX_SCHEME.hash(),
        };
        let s = serde_json::to_string(&id).unwrap();
        let back: EmbedderIdentity = serde_json::from_str(&s).unwrap();
        assert_eq!(id, back);
    }

    /// Full load + embed test. Downloads the model on first run — marked
    /// `#[ignore]` so CI is deterministic.
    ///
    /// Run locally with: `cargo test -p fastrag-embed --features fastembed -- --ignored arctic_loads_and_embeds`
    #[test]
    #[ignore]
    fn arctic_loads_and_embeds() {
        let e = ArcticEmbedMV15::load().expect("load arctic");
        let qs = vec![QueryText::new("what is the capital of France?")];
        let ps = vec![PassageText::new("Paris is the capital of France.")];
        let qv = e.embed_query(&qs).unwrap();
        let pv = e.embed_passage(&ps).unwrap();
        assert_eq!(qv.len(), 1);
        assert_eq!(qv[0].len(), 768);
        assert_eq!(pv[0].len(), 768);
        // Symmetric: same text embedded as query vs passage must match exactly.
        let same_q = e.embed_query(&[QueryText::new("hello")]).unwrap();
        let same_p = e.embed_passage(&[PassageText::new("hello")]).unwrap();
        let cos = cosine(&same_q[0], &same_p[0]);
        assert!(cos > 0.9999, "symmetric preset produced different vectors for same text: cos={cos}");
    }

    fn cosine(a: &[f32], b: &[f32]) -> f32 {
        let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
        let na: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        let nb: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
        dot / (na * nb)
    }
}
```

- [ ] **Step 2: Register in module**

Uncomment `pub mod arctic;` and `pub use arctic::ArcticEmbedMV15;` in `crates/fastrag-embed/src/fastembed/mod.rs`.

- [ ] **Step 3: Run non-ignored tests**

Run: `cargo test -p fastrag-embed fastembed::arctic`
Expected: PASS (the 4 non-ignored tests; no model download).

- [ ] **Step 4: Verify clippy is clean**

Run: `cargo clippy -p fastrag-embed --all-targets -- -D warnings`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
cargo fmt
git add crates/fastrag-embed/src/fastembed/
git commit -m "feat(embed): add ArcticEmbedMV15 preset (768-d, symmetric)"
```

---

## Task 8: NomicEmbedV15 preset

**Files:**
- Create: `crates/fastrag-embed/src/fastembed/nomic.rs`

- [ ] **Step 1: Write the preset with asymmetric prefix handling**

Create `crates/fastrag-embed/src/fastembed/nomic.rs`:

```rust
use crate::{
    EmbedError, Embedder, EmbedderIdentity, PassageText, PrefixScheme, QueryText,
};
use fastembed::{EmbeddingModel, TextEmbedding};

use super::shared::{fastembed_batch, init_text_embedding};

/// nomic-embed-text v1.5 — 768-d, **asymmetric** with the
/// `search_query: ` / `search_document: ` prefix convention.
///
/// Long-context option (8192-token window) for corpora that need it.
/// Prefer `ArcticEmbedMV15` for general retrieval.
pub struct NomicEmbedV15 {
    inner: TextEmbedding,
}

impl NomicEmbedV15 {
    pub fn load() -> Result<Self, EmbedError> {
        // Use the quantized variant (`NomicEmbedTextV15Q`) if fastembed-rs
        // exposes it in the pinned version. If you flip variants here, you
        // MUST also update MODEL_ID below — the identity is what keeps
        // mismatched indexes from loading silently.
        let inner = init_text_embedding(EmbeddingModel::NomicEmbedTextV15Q)?;
        Ok(Self { inner })
    }
}

impl Embedder for NomicEmbedV15 {
    const DIM: usize = 768;
    const MODEL_ID: &'static str = "fastrag/nomic-embed-v1.5-q";
    const PREFIX_SCHEME: PrefixScheme = PrefixScheme::new(
        "search_query: ",
        "search_document: ",
    );

    fn embed_query(&self, texts: &[QueryText]) -> Result<Vec<Vec<f32>>, EmbedError> {
        let owned: Vec<String> = texts
            .iter()
            .map(|t| format!("{}{}", Self::PREFIX_SCHEME.query, t.as_str()))
            .collect();
        fastembed_batch(&self.inner, owned, None)
    }

    fn embed_passage(&self, texts: &[PassageText]) -> Result<Vec<Vec<f32>>, EmbedError> {
        let owned: Vec<String> = texts
            .iter()
            .map(|t| format!("{}{}", Self::PREFIX_SCHEME.passage, t.as_str()))
            .collect();
        fastembed_batch(&self.inner, owned, None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nomic_has_768_dim() {
        assert_eq!(<NomicEmbedV15 as Embedder>::DIM, 768);
    }

    #[test]
    fn nomic_has_stable_model_id() {
        assert_eq!(
            <NomicEmbedV15 as Embedder>::MODEL_ID,
            "fastrag/nomic-embed-v1.5-q"
        );
    }

    #[test]
    fn nomic_prefix_scheme_is_asymmetric() {
        let s = <NomicEmbedV15 as Embedder>::PREFIX_SCHEME;
        assert_eq!(s.query, "search_query: ");
        assert_eq!(s.passage, "search_document: ");
        assert_ne!(s.hash(), PrefixScheme::NONE.hash());
    }

    #[test]
    fn nomic_identity_differs_from_arctic() {
        use crate::fastembed::arctic::ArcticEmbedMV15;
        let n = EmbedderIdentity {
            model_id: <NomicEmbedV15 as Embedder>::MODEL_ID.to_string(),
            dim: <NomicEmbedV15 as Embedder>::DIM,
            prefix_scheme_hash: <NomicEmbedV15 as Embedder>::PREFIX_SCHEME.hash(),
        };
        let a = EmbedderIdentity {
            model_id: <ArcticEmbedMV15 as Embedder>::MODEL_ID.to_string(),
            dim: <ArcticEmbedMV15 as Embedder>::DIM,
            prefix_scheme_hash: <ArcticEmbedMV15 as Embedder>::PREFIX_SCHEME.hash(),
        };
        assert_ne!(n, a);
        // Dim equal, so the difference must come from id + scheme.
        assert_eq!(n.dim, a.dim);
    }

    /// Exercises the prefix plumbing against the real model. Ignored so CI
    /// stays deterministic; run locally with `-- --ignored nomic_`.
    #[test]
    #[ignore]
    fn nomic_query_and_passage_vectors_differ_for_same_text() {
        let e = NomicEmbedV15::load().expect("load nomic");
        let qv = e.embed_query(&[QueryText::new("hello")]).unwrap();
        let pv = e.embed_passage(&[PassageText::new("hello")]).unwrap();
        assert_eq!(qv[0].len(), 768);
        assert_eq!(pv[0].len(), 768);
        // Asymmetric: different prefixes => different vectors.
        let cos = cosine(&qv[0], &pv[0]);
        assert!(
            cos < 0.999,
            "nomic query/passage vectors identical for same input — prefix not applied? cos={cos}"
        );
    }

    fn cosine(a: &[f32], b: &[f32]) -> f32 {
        let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
        let na: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        let nb: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
        dot / (na * nb)
    }
}
```

- [ ] **Step 2: Register in module**

Uncomment `pub mod nomic;` and `pub use nomic::NomicEmbedV15;` in `crates/fastrag-embed/src/fastembed/mod.rs`.

- [ ] **Step 3: Run non-ignored tests**

Run: `cargo test -p fastrag-embed fastembed::nomic`
Expected: PASS (4 tests, no download).

- [ ] **Step 4: Clippy clean**

Run: `cargo clippy -p fastrag-embed --all-targets -- -D warnings`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
cargo fmt
git add crates/fastrag-embed/src/fastembed/
git commit -m "feat(embed): add NomicEmbedV15 preset (768-d, asymmetric prefixes)"
```

---

## Task 9: Wire presets into CLI embed loader

**Files:**
- Modify: `fastrag-cli/src/embed_loader.rs` (or equivalent — grep for `BgeSmallEmbedder` in `fastrag-cli/`)
- Modify: `fastrag-cli/src/args.rs`
- Modify: `fastrag-cli/Cargo.toml`

- [ ] **Step 1: Locate the embed loader**

Run: `grep -rn "BgeSmallEmbedder\|EmbedderKind\|--embedder" fastrag-cli/src/`

Identify the function that maps a CLI flag / config to `DynEmbedder`. Read it in full before editing.

- [ ] **Step 2: Update CLI arg enum**

In `fastrag-cli/src/args.rs`, extend the embedder selector to include the two new presets. Example (adapt to the existing clap derive pattern):

```rust
#[derive(Debug, Clone, clap::ValueEnum, Default)]
pub enum EmbedderKind {
    #[default]
    ArcticEmbedMV15,
    NomicEmbedV15,
    #[cfg(feature = "legacy-candle")]
    BgeSmallEnV15,
    // existing OpenAi / Ollama variants preserved
    OpenAi,
    Ollama,
}
```

The `Default` now points at arctic. If the existing enum uses different casing (e.g. kebab-case via `value(name = "...")`), preserve it; just flip the `#[default]`.

- [ ] **Step 3: Update the loader**

Replace the BGE default branch with arctic and add nomic. Example shape:

```rust
use std::sync::Arc;
use fastrag_embed::{DynEmbedder, DynEmbedderTrait};

pub fn load_embedder(kind: EmbedderKind) -> Result<DynEmbedder, EmbedError> {
    match kind {
        EmbedderKind::ArcticEmbedMV15 => {
            let e = fastrag_embed::fastembed::ArcticEmbedMV15::load()?;
            Ok(Arc::new(e) as DynEmbedder)
        }
        EmbedderKind::NomicEmbedV15 => {
            let e = fastrag_embed::fastembed::NomicEmbedV15::load()?;
            Ok(Arc::new(e) as DynEmbedder)
        }
        #[cfg(feature = "legacy-candle")]
        EmbedderKind::BgeSmallEnV15 => {
            let e = fastrag_embed::BgeSmallEmbedder::load_default()?;
            Ok(Arc::new(e) as DynEmbedder)
        }
        EmbedderKind::OpenAi => { /* unchanged */ todo!("preserve existing") }
        EmbedderKind::Ollama => { /* unchanged */ todo!("preserve existing") }
    }
}
```

Remove the `TODO(Task 9)` comment you left in Task 3.

- [ ] **Step 4: Update the model-id resolution in `embed_loader`**

If the loader reads `identity.model_id` from the manifest to decide which preset to construct (Step 1's pattern), add the two new model IDs to that mapping:
- `"fastrag/arctic-embed-m-v1.5"` → `EmbedderKind::ArcticEmbedMV15`
- `"fastrag/nomic-embed-v1.5-q"` → `EmbedderKind::NomicEmbedV15`

- [ ] **Step 5: Update `fastrag-cli/Cargo.toml`**

Ensure the CLI's default features include `fastembed` (via `fastrag-embed/fastembed`). Add a `legacy-candle` passthrough feature for users who need to load old corpora.

```toml
[features]
default = ["retrieval", "fastembed"]
fastembed = ["fastrag-embed/fastembed"]
legacy-candle = ["fastrag-embed/legacy-candle"]
# existing features preserved
```

- [ ] **Step 6: Verify CLI builds with default features**

Run: `cargo build -p fastrag-cli`
Expected: PASS.

- [ ] **Step 7: Verify CLI builds with legacy-candle**

Run: `cargo build -p fastrag-cli --features legacy-candle`
Expected: PASS.

- [ ] **Step 8: Commit**

```bash
cargo fmt
cargo clippy -p fastrag-cli --all-targets -- -D warnings
git add -A
git commit -m "feat(cli): default to arctic-embed-m-v1.5, add nomic preset"
```

---

## Task 10: Integration test — corpus round-trip per preset

**Files:**
- Create: `crates/fastrag/tests/fastembed_corpus_roundtrip.rs`

- [ ] **Step 1: Write the round-trip test**

Create the file (mark `#[ignore]` because it downloads models):

```rust
//! End-to-end: build a corpus with a fastembed preset, persist, reload,
//! assert identity + canary invariants. Ignored by default because first
//! run downloads the model. Run with:
//!   cargo test -p fastrag --test fastembed_corpus_roundtrip -- --ignored

#![cfg(feature = "fastembed")]

use std::sync::Arc;
use tempfile::tempdir;

use fastrag_embed::fastembed::{ArcticEmbedMV15, NomicEmbedV15};
use fastrag_embed::{DynEmbedder, DynEmbedderTrait, Embedder, PassageText};

// NOTE: Replace these imports with the actual public corpus API once the
// implementer locates it. Expected entry points (from Step 1):
//   fastrag::corpus::Corpus::create(dir, embedder, docs)
//   fastrag::corpus::Corpus::open(dir, embedder)
use fastrag::corpus::Corpus;

fn docs() -> Vec<String> {
    vec![
        "Paris is the capital of France.".to_string(),
        "The Eiffel Tower is in Paris.".to_string(),
        "Rust is a systems programming language.".to_string(),
    ]
}

#[test]
#[ignore]
fn arctic_corpus_round_trips() {
    let dir = tempdir().unwrap();
    let e = Arc::new(ArcticEmbedMV15::load().unwrap()) as DynEmbedder;
    let id_before = e.identity();

    Corpus::create(dir.path(), e.clone(), docs()).expect("create corpus");

    let e2 = Arc::new(ArcticEmbedMV15::load().unwrap()) as DynEmbedder;
    let loaded = Corpus::open(dir.path(), e2).expect("reopen corpus");
    assert_eq!(loaded.identity(), id_before);
}

#[test]
#[ignore]
fn nomic_corpus_round_trips() {
    let dir = tempdir().unwrap();
    let e = Arc::new(NomicEmbedV15::load().unwrap()) as DynEmbedder;
    Corpus::create(dir.path(), e.clone(), docs()).expect("create corpus");

    let e2 = Arc::new(NomicEmbedV15::load().unwrap()) as DynEmbedder;
    Corpus::open(dir.path(), e2).expect("reopen corpus");
}

#[test]
#[ignore]
fn cross_preset_load_fails_with_identity_mismatch() {
    let dir = tempdir().unwrap();
    let arctic = Arc::new(ArcticEmbedMV15::load().unwrap()) as DynEmbedder;
    Corpus::create(dir.path(), arctic, docs()).expect("create with arctic");

    // Attempt to reopen with nomic — must fail with IdentityMismatch.
    let nomic = Arc::new(NomicEmbedV15::load().unwrap()) as DynEmbedder;
    let err = Corpus::open(dir.path(), nomic).expect_err("expected mismatch");
    let msg = format!("{err}");
    assert!(
        msg.to_lowercase().contains("identity") || msg.to_lowercase().contains("mismatch"),
        "unexpected error: {msg}"
    );
}
```

- [ ] **Step 2: Add `tempfile` to dev-deps if missing**

Run: `grep tempfile crates/fastrag/Cargo.toml`
If absent, add to `[dev-dependencies]`: `tempfile = "3"`.

- [ ] **Step 3: Verify the test file compiles**

Run: `cargo test -p fastrag --test fastembed_corpus_roundtrip --no-run`
Expected: PASS. If the `Corpus` API differs, update the imports/calls to match the real signatures from `crates/fastrag/src/corpus.rs` — do not invent new APIs.

- [ ] **Step 4: Run the tests locally (manual, not CI)**

Run: `cargo test -p fastrag --test fastembed_corpus_roundtrip -- --ignored`
Expected: All three PASS. First run will download both models to the fastrag cache dir. This may take several minutes.

> If `cross_preset_load_fails_with_identity_mismatch` does NOT fail the load, the identity check is broken — stop, investigate `HnswIndex::load` identity comparison from Step 1.

- [ ] **Step 5: Commit**

```bash
cargo fmt
git add crates/fastrag/tests/fastembed_corpus_roundtrip.rs crates/fastrag/Cargo.toml
git commit -m "test(fastrag): add fastembed corpus round-trip + mismatch tests"
```

---

## Task 11: Eval smoke check — arctic vs legacy BGE

**Files:**
- Run the existing eval harness; no new files

- [ ] **Step 1: Identify the eval entry point**

Run: `ls crates/fastrag-eval/` and read its README / main entry point. Note the command to run the harness against a fixture corpus.

- [ ] **Step 2: Build a fixture corpus with arctic**

Use the CLI:

```bash
cargo run --release -p fastrag-cli -- index tests/fixtures/eval-corpus \
    --corpus /tmp/fastrag-eval-arctic \
    --embedder arctic-embed-m-v1.5
```

(Adjust the fixture path to whatever the eval harness uses.)

- [ ] **Step 3: Build a fixture corpus with legacy BGE**

```bash
cargo run --release -p fastrag-cli --features legacy-candle -- index tests/fixtures/eval-corpus \
    --corpus /tmp/fastrag-eval-bge \
    --embedder bge-small-en-v1.5
```

- [ ] **Step 4: Run the eval harness against both**

Record hit@5 and MRR@10 for each. Save the numbers to a scratch file:

```
arctic-embed-m-v1.5  hit@5=<X>  MRR@10=<Y>
bge-small-en-v1.5    hit@5=<X>  MRR@10=<Y>
```

- [ ] **Step 5: Decision**

- If arctic ≥ BGE on both metrics → proceed, paste numbers into the PR description.
- If arctic regresses by < 2pt on a small fixture set → acceptable, note it as a fixture-size artifact and open a follow-up to re-run on a larger corpus.
- If arctic regresses by ≥ 2pt on either metric → STOP. Options:
  1. Switch default to arctic f32 (non-quantized) and re-run.
  2. Switch default to nomic and re-run.
  3. Escalate to the user — do NOT merge a worse default silently.

- [ ] **Step 6: Record numbers in the PR body**

No commit; these numbers go in the PR description in Task 13.

---

## Task 12: README + docs update

**Files:**
- Modify: `README.md`
- Modify: `crates/fastrag-embed/README.md` (if it exists)

- [ ] **Step 1: Use doc-editor skill before editing README**

Per project `CLAUDE.md`, every edit to `.md` goes through the `doc-editor` skill. Draft the changes, run them through doc-editor, then apply.

Draft to include:

1. **Default embedder changed** to `arctic-embed-m-v1.5` (768-d, symmetric, fastembed-rs backend). Previous default was BGE-small via candle.
2. **New optional preset**: `nomic-embed-v1.5` (768-d, asymmetric, 8192-token context) — choose for long-context corpora.
3. **Feature flags**:
   - `fastembed` (default on) — new ort-based backend.
   - `legacy-candle` — the old candle BGE-small path. Needed to build new corpora with BGE, and to load existing manifest-v3 BGE corpora.
4. **Model cache directory**: `$XDG_CACHE_HOME/fastrag/models/fastembed/` (falls back to `~/.cache/fastrag/models/fastembed/` on Linux).
5. **Migration note**: existing corpora indexed with BGE-small continue to load if the binary is built with `--features legacy-candle`. To migrate to arctic, re-run `fastrag index` with `--embedder arctic-embed-m-v1.5` into a new corpus directory. Indexes cannot be cross-loaded — identity check will reject it.
6. **GPU features**: `gpu-cuda` and `gpu-coreml` are opt-in and documented with their requirements.

- [ ] **Step 2: Apply edits**

After doc-editor returns cleaned prose, apply via `Edit`.

- [ ] **Step 3: Verify README still parses as markdown**

Run: `cargo doc --no-deps --workspace 2>&1 | grep -i "readme" || true` (smoke check for any doc-test errors; README isn't strictly parsed but rustdoc will flag obvious breakage).

- [ ] **Step 4: Commit**

```bash
git add README.md crates/fastrag-embed/README.md 2>/dev/null || git add README.md
git commit -m "docs: document fastembed default, nomic preset, and legacy-candle migration"
```

---

## Task 13: CI — build matrix and push

**Files:**
- Modify: `.github/workflows/ci.yml`

- [ ] **Step 1: Read the current CI file**

Read `.github/workflows/ci.yml`. Identify the main build/test job.

- [ ] **Step 2: Add a legacy-candle build-only job**

Add a new job (or a matrix entry) that runs:

```yaml
- name: Build with legacy-candle
  run: cargo build --workspace --no-default-features --features legacy-candle
```

This verifies the gated path still compiles without running its tests (which need model downloads).

- [ ] **Step 3: GPU feature build-only check (optional)**

If the project is willing to absorb the CI time, add:

```yaml
- name: Build with gpu-cuda feature (no runtime test)
  run: cargo build -p fastrag-embed --features gpu-cuda
  continue-on-error: true  # system CUDA may be unavailable on runners
```

Skip this if the feature flag isn't implemented yet (it's a fastembed-rs/ort passthrough — confirm the fastembed crate exposes it in the pinned version before adding).

- [ ] **Step 4: Run cargo fmt + clippy gate locally**

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy --workspace --all-targets --no-default-features --features legacy-candle -- -D warnings
cargo test --workspace
```

Expected: all green.

- [ ] **Step 5: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "ci: add legacy-candle build-only job"
```

- [ ] **Step 6: Push and watch CI**

```bash
git push
```

Then invoke the `ci-watcher` skill per project `CLAUDE.md` — **mandatory** after every push. Run as a background Haiku Agent. Do not use ad-hoc `gh run watch`.

- [ ] **Step 7: Open PR with eval numbers**

Once CI is green, open a PR. Body must include:
- Link to the spec: `docs/superpowers/specs/2026-04-09-fastembed-backend-design.md`
- Eval smoke check numbers from Task 11
- Release binary size delta (before/after `ls -lh target/release/fastrag`)
- Explicit note: "`Closes` follow-up issue: TBD create a Step 2 tracking issue if none exists."

---

## Self-Review Checklist (completed)

**Spec coverage:**
- Goal (replace default, ship two presets) → Tasks 1, 7, 8, 9
- Non-goals → respected; no reranker/hybrid/eval-refresh work in this plan
- Crate layout → Tasks 1, 2
- Preset implementations → Tasks 7, 8
- Shared helper → Task 6
- Prefix application → Task 8 (nomic), Task 7 (arctic symmetric)
- Batch API → **N/A, already slice-based in Step 1** (noted in "Important Context")
- Model variants / quantization → Tasks 7, 8 (note on variant selection)
- Model cache → Task 5
- Build surface / GPU features → Tasks 1, 13
- Error handling → Task 4
- Unit tests → Tasks 5, 7, 8
- Integration tests → Task 10
- Eval smoke check → Task 11
- Risk mitigations → addressed in Tasks 10 (download determinism via `#[ignore]`), 11 (accuracy), 13 (binary size measurement in PR body), 9 + 12 (legacy migration path)
- Decision log → reflected in task choices

**Placeholder scan:** one `TODO(Task 9)` noted in Task 3; resolved in Task 9. No unresolved TBDs. Two implementation-time notes where fastembed-rs variant enum names need verification against the pinned version — these are unavoidable without pinning the exact version in the plan.

**Type consistency:** `ArcticEmbedMV15` / `NomicEmbedV15` names used identically across Tasks 7, 8, 9, 10. Model IDs `fastrag/arctic-embed-m-v1.5` and `fastrag/nomic-embed-v1.5-q` used consistently. `PrefixScheme::NONE` (not `SYMMETRIC` as the spec draft had) corrected throughout.
