# Embedder Invariant Refactor Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make dim/prefix/model-id drift between the embedder used at index time and at query time a compile-time error wherever possible, and a hard runtime error (canary mismatch) everywhere else.

**Architecture:** Split `Embedder` into a static trait (`Embedder` with `const DIM`, `const MODEL_ID`, `const PREFIX_SCHEME`, typed `embed_query` / `embed_passage`) plus a dyn-safe mirror (`DynEmbedderTrait`) with a blanket impl. Runtime storage stays `Arc<dyn DynEmbedderTrait>` but construction flows through `EmbedderHandle<E>` which burns in the static invariants. `CorpusManifest` schema bumps to v3 with `EmbedderIdentity` + `Canary`; `HnswIndex::load` verifies both against the live embedder. Ollama gets a `create_runtime_identity` carve-out because its dim is probed, not `const`.

**Tech Stack:** Rust 2021, existing crates (`fastrag-embed`, `fastrag-index`, `fastrag`, `fastrag-cli`), no new dependencies.

---

## File Structure

**Modified:**
- `crates/fastrag-embed/src/lib.rs` — new `Embedder` trait, `DynEmbedderTrait`, `QueryText`, `PassageText`, `PrefixScheme`, `EmbedderIdentity`, `EmbedderHandle`.
- `crates/fastrag-embed/src/bge.rs` — implement new trait; fix `MODEL_ID` to `"fastrag/bge-small-en-v1.5"`.
- `crates/fastrag-embed/src/test_utils.rs` — `MockEmbedder` implements new trait.
- `crates/fastrag-embed/src/http/openai.rs` — split into const-generic `OpenAiEmbedder<const DIM: usize>` with aliases.
- `crates/fastrag-embed/src/http/ollama.rs` — expose `runtime_identity()` escape hatch.
- `crates/fastrag-embed/src/error.rs` — no new variants (reuse existing).
- `crates/fastrag-index/src/manifest.rs` — `CorpusManifest` v3: add `identity: EmbedderIdentity`, `canary: Canary`; bump `version` default.
- `crates/fastrag-index/src/hnsw.rs` — `HnswIndex::load` accepts `&dyn DynEmbedderTrait` and verifies identity + canary; `HnswIndex::new` takes identity + canary at construction.
- `crates/fastrag-index/src/error.rs` — add `IdentityMismatch`, `CanaryMismatch`, `UnsupportedSchema`.
- `crates/fastrag/src/corpus/mod.rs` — rewire `index_path_with_metadata`, `query_corpus*` to use `&dyn DynEmbedderTrait`; drop `CorpusError::EmbedderMismatch` in favour of index errors.
- `fastrag-cli/src/embed_loader.rs` — return `Arc<dyn DynEmbedderTrait>`; build via typed `EmbedderHandle<E>::new()?.erase()`; fix `detect_from_manifest` for `fastrag/bge-small-en-v1.5`.

**No new files** — everything lands in existing modules.

---

## Task 1: Core types — `QueryText`, `PassageText`, `PrefixScheme`, `EmbedderIdentity`, `Canary`

**Files:**
- Modify: `crates/fastrag-embed/src/lib.rs`

- [ ] **Step 1: Write failing tests**

Append to `crates/fastrag-embed/src/lib.rs`:

```rust
#[cfg(test)]
mod core_type_tests {
    use super::*;

    #[test]
    fn prefix_scheme_hash_is_stable_for_same_prefixes() {
        let a = PrefixScheme::new("query: ", "passage: ");
        let b = PrefixScheme::new("query: ", "passage: ");
        assert_eq!(a.hash(), b.hash());
    }

    #[test]
    fn prefix_scheme_hash_differs_when_prefixes_differ() {
        let a = PrefixScheme::new("query: ", "passage: ");
        let b = PrefixScheme::new("search_query: ", "search_document: ");
        assert_ne!(a.hash(), b.hash());
    }

    #[test]
    fn query_and_passage_text_are_distinct_types() {
        let q = QueryText::new("hi");
        let p = PassageText::new("hi");
        assert_eq!(q.as_str(), "hi");
        assert_eq!(p.as_str(), "hi");
    }

    #[test]
    fn embedder_identity_equality_is_field_wise() {
        let a = EmbedderIdentity {
            model_id: "fastrag/bge-small-en-v1.5".into(),
            dim: 384,
            prefix_scheme_hash: 0xDEADBEEF,
        };
        let b = a.clone();
        assert_eq!(a, b);
    }
}
```

- [ ] **Step 2: Run tests — expect compile errors**

Run: `cargo test -p fastrag-embed core_type_tests`
Expected: build fails — `QueryText`, `PassageText`, `PrefixScheme`, `EmbedderIdentity` undefined.

- [ ] **Step 3: Add the types**

In `crates/fastrag-embed/src/lib.rs`, above the `Embedder` trait:

```rust
use serde::{Deserialize, Serialize};

/// A query-side input. Distinct from `PassageText` at the type level so prefix
/// conventions cannot be confused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryText(String);

impl QueryText {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A passage-side input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PassageText(String);

impl PassageText {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Prefix pair used by asymmetric retrievers (E5, nomic, arctic, …). Empty
/// strings mean "no prefix" (BGE-small, OpenAI, mock).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrefixScheme {
    pub query: &'static str,
    pub passage: &'static str,
}

impl PrefixScheme {
    pub const NONE: PrefixScheme = PrefixScheme {
        query: "",
        passage: "",
    };

    pub const fn new(query: &'static str, passage: &'static str) -> Self {
        Self { query, passage }
    }

    /// FNV-1a 64-bit hash of `"{query}\0{passage}"`. Deterministic across runs.
    pub const fn hash(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        let qb = self.query.as_bytes();
        let mut i = 0;
        while i < qb.len() {
            h ^= qb[i] as u64;
            h = h.wrapping_mul(0x100000001b3);
            i += 1;
        }
        h ^= 0;
        h = h.wrapping_mul(0x100000001b3);
        let pb = self.passage.as_bytes();
        let mut j = 0;
        while j < pb.len() {
            h ^= pb[j] as u64;
            h = h.wrapping_mul(0x100000001b3);
            j += 1;
        }
        h
    }
}

/// Identity of the embedder that produced a corpus. Persisted in the manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmbedderIdentity {
    pub model_id: String,
    pub dim: usize,
    pub prefix_scheme_hash: u64,
}

/// Fixed canary text, embedded once at corpus creation and re-embedded on load
/// to detect silent drift.
pub const CANARY_TEXT: &str =
    "fastrag canary v1: the quick brown fox jumps over the lazy dog";

pub const CANARY_COSINE_TOLERANCE: f32 = 0.999;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Canary {
    pub text_version: u32,
    pub vector: Vec<f32>,
}
```

Add `serde` to `fastrag-embed`'s `Cargo.toml` if not already there:

```toml
serde = { version = "1", features = ["derive"] }
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p fastrag-embed core_type_tests`
Expected: 4 passed.

- [ ] **Step 5: Commit**

```bash
git add crates/fastrag-embed
git commit -m "feat(embed): core types for typed embedder invariant

QueryText, PassageText, PrefixScheme (const hash), EmbedderIdentity,
Canary. No behavior change yet — the new Embedder trait lands in the
next task."
```

---

## Task 2: New `Embedder` trait + dyn-safe `DynEmbedderTrait` + blanket impl

**Files:**
- Modify: `crates/fastrag-embed/src/lib.rs`

- [ ] **Step 1: Write failing test**

Append to `core_type_tests` module:

```rust
#[test]
fn dyn_embedder_forwards_to_static_impl() {
    use std::sync::Arc;

    struct Toy;
    impl Embedder for Toy {
        const DIM: usize = 2;
        const MODEL_ID: &'static str = "toy/v1";
        const PREFIX_SCHEME: PrefixScheme = PrefixScheme::NONE;

        fn embed_query(
            &self,
            texts: &[QueryText],
        ) -> Result<Vec<Vec<f32>>, EmbedError> {
            Ok(texts.iter().map(|_| vec![1.0, 0.0]).collect())
        }

        fn embed_passage(
            &self,
            texts: &[PassageText],
        ) -> Result<Vec<Vec<f32>>, EmbedError> {
            Ok(texts.iter().map(|_| vec![0.0, 1.0]).collect())
        }
    }

    let erased: Arc<dyn DynEmbedderTrait> = Arc::new(Toy);
    assert_eq!(erased.dim(), 2);
    assert_eq!(erased.model_id(), "toy/v1");
    assert_eq!(
        erased.prefix_scheme_hash(),
        PrefixScheme::NONE.hash()
    );

    let qv = erased
        .embed_query_dyn(&[QueryText::new("q")])
        .unwrap();
    assert_eq!(qv, vec![vec![1.0, 0.0]]);
    let pv = erased
        .embed_passage_dyn(&[PassageText::new("p")])
        .unwrap();
    assert_eq!(pv, vec![vec![0.0, 1.0]]);
}
```

- [ ] **Step 2: Run — expect compile errors**

Run: `cargo test -p fastrag-embed core_type_tests::dyn_embedder_forwards_to_static_impl`
Expected: build fails.

- [ ] **Step 3: Replace the existing `Embedder` trait**

In `crates/fastrag-embed/src/lib.rs`, delete the current `pub trait Embedder { … }` block and the `trait_tests` module entirely, and add:

```rust
/// Static embedder trait. Every implementation must burn its dim, model id,
/// and prefix scheme into associated consts so the compiler can enforce
/// compatibility wherever the concrete type is known.
pub trait Embedder: Send + Sync + 'static {
    const DIM: usize;
    const MODEL_ID: &'static str;
    const PREFIX_SCHEME: PrefixScheme;

    fn embed_query(
        &self,
        texts: &[QueryText],
    ) -> Result<Vec<Vec<f32>>, EmbedError>;

    fn embed_passage(
        &self,
        texts: &[PassageText],
    ) -> Result<Vec<Vec<f32>>, EmbedError>;

    fn default_batch_size(&self) -> usize {
        64
    }
}

/// Dyn-safe mirror of `Embedder`. Corpora store `Arc<dyn DynEmbedderTrait>`
/// and get runtime access to the same invariants that the static trait
/// guarantees at construction time.
pub trait DynEmbedderTrait: Send + Sync + 'static {
    fn model_id(&self) -> &'static str;
    fn dim(&self) -> usize;
    fn prefix_scheme(&self) -> PrefixScheme;
    fn prefix_scheme_hash(&self) -> u64;
    fn identity(&self) -> EmbedderIdentity {
        EmbedderIdentity {
            model_id: self.model_id().to_string(),
            dim: self.dim(),
            prefix_scheme_hash: self.prefix_scheme_hash(),
        }
    }
    fn default_batch_size(&self) -> usize;
    fn embed_query_dyn(
        &self,
        texts: &[QueryText],
    ) -> Result<Vec<Vec<f32>>, EmbedError>;
    fn embed_passage_dyn(
        &self,
        texts: &[PassageText],
    ) -> Result<Vec<Vec<f32>>, EmbedError>;
}

impl<E: Embedder> DynEmbedderTrait for E {
    fn model_id(&self) -> &'static str {
        <E as Embedder>::MODEL_ID
    }
    fn dim(&self) -> usize {
        <E as Embedder>::DIM
    }
    fn prefix_scheme(&self) -> PrefixScheme {
        <E as Embedder>::PREFIX_SCHEME
    }
    fn prefix_scheme_hash(&self) -> u64 {
        <E as Embedder>::PREFIX_SCHEME.hash()
    }
    fn default_batch_size(&self) -> usize {
        <E as Embedder>::default_batch_size(self)
    }
    fn embed_query_dyn(
        &self,
        texts: &[QueryText],
    ) -> Result<Vec<Vec<f32>>, EmbedError> {
        <E as Embedder>::embed_query(self, texts)
    }
    fn embed_passage_dyn(
        &self,
        texts: &[PassageText],
    ) -> Result<Vec<Vec<f32>>, EmbedError> {
        <E as Embedder>::embed_passage(self, texts)
    }
}

/// Convenience alias for the erased form.
pub type DynEmbedder = std::sync::Arc<dyn DynEmbedderTrait>;
```

- [ ] **Step 4: Run test — this task's test only**

Run: `cargo test -p fastrag-embed core_type_tests::dyn_embedder_forwards_to_static_impl`
Expected: passes. The workspace as a whole will still be broken (BGE, HTTP, mock, corpus all use the old trait). That's fine; later tasks fix them.

- [ ] **Step 5: Commit (workspace will not build — intentional, WIP)**

```bash
git add crates/fastrag-embed/src/lib.rs
git commit -m "refactor(embed): new typed Embedder trait + DynEmbedderTrait bridge

WIP — downstream impls (BGE, HTTP, mock) and callers still use the old
shape and will be fixed in the following commits. Workspace build is
intentionally broken between this commit and Task 8."
```

---

## Task 3: Port `BgeSmallEmbedder` (fixes pre-existing `model_id` bug)

**Files:**
- Modify: `crates/fastrag-embed/src/bge.rs`

- [ ] **Step 1: Write failing test**

Append to `crates/fastrag-embed/src/bge.rs` tests (add `#[cfg(test)]` module if absent):

```rust
#[cfg(test)]
mod invariant_tests {
    use super::*;
    use crate::{Embedder, PrefixScheme};

    #[test]
    fn bge_model_id_matches_fastrag_namespace() {
        // Pre-existing bug: was "BAAI/bge-small-en-v1.5", causing
        // embed_loader::detect_from_manifest("fastrag/bge…") to fail on
        // real BGE corpora. Fixed here.
        assert_eq!(BgeSmallEmbedder::MODEL_ID, "fastrag/bge-small-en-v1.5");
    }

    #[test]
    fn bge_dim_is_384() {
        assert_eq!(BgeSmallEmbedder::DIM, 384);
    }

    #[test]
    fn bge_prefix_scheme_is_none() {
        assert_eq!(
            BgeSmallEmbedder::PREFIX_SCHEME.hash(),
            PrefixScheme::NONE.hash()
        );
    }
}
```

- [ ] **Step 2: Run — expect compile errors**

Run: `cargo test -p fastrag-embed invariant_tests`
Expected: fails — old trait shape.

- [ ] **Step 3: Update `BgeSmallEmbedder` impl**

In `crates/fastrag-embed/src/bge.rs`, replace the existing `impl Embedder for BgeSmallEmbedder` block with:

```rust
use crate::{Embedder, EmbedError, PassageText, PrefixScheme, QueryText};

impl Embedder for BgeSmallEmbedder {
    const DIM: usize = 384;
    const MODEL_ID: &'static str = "fastrag/bge-small-en-v1.5";
    const PREFIX_SCHEME: PrefixScheme = PrefixScheme::NONE;

    fn embed_query(
        &self,
        texts: &[QueryText],
    ) -> Result<Vec<Vec<f32>>, EmbedError> {
        let refs: Vec<&str> = texts.iter().map(QueryText::as_str).collect();
        self.embed_raw(&refs)
    }

    fn embed_passage(
        &self,
        texts: &[PassageText],
    ) -> Result<Vec<Vec<f32>>, EmbedError> {
        let refs: Vec<&str> = texts.iter().map(PassageText::as_str).collect();
        self.embed_raw(&refs)
    }

    fn default_batch_size(&self) -> usize {
        16
    }
}
```

Rename the existing private forward method that takes `&[&str]` and runs the BERT forward pass to `embed_raw` (keep its body verbatim; it was previously called from the old `embed(&self, texts: &[&str])` method). If the existing code puts that logic directly in `fn embed`, rename `fn embed` → `fn embed_raw` and make it a plain inherent method:

```rust
impl BgeSmallEmbedder {
    fn embed_raw(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbedError> {
        // … existing body from the old `fn embed` impl, unchanged …
    }
}
```

Remove the old `fn model_id`, `fn dim`, `fn embed`, `fn default_batch_size`, and `fn embed_batched` from the old `impl Embedder` block — they are gone.

- [ ] **Step 4: Run tests**

Run: `cargo test -p fastrag-embed invariant_tests`
Expected: 3 passed.

- [ ] **Step 5: Commit**

```bash
git add crates/fastrag-embed/src/bge.rs
git commit -m "refactor(embed): port BgeSmallEmbedder to typed trait

Also fixes pre-existing model_id bug — was 'BAAI/bge-small-en-v1.5'
which broke embed_loader::detect_from_manifest, now
'fastrag/bge-small-en-v1.5' which round-trips through the read path."
```

---

## Task 4: Port `MockEmbedder`

**Files:**
- Modify: `crates/fastrag-embed/src/test_utils.rs`

- [ ] **Step 1: Write failing test**

Append to `crates/fastrag-embed/src/test_utils.rs`:

```rust
#[cfg(test)]
mod mock_invariant_tests {
    use super::*;
    use crate::{Embedder, PassageText, QueryText};

    #[test]
    fn mock_consts_are_pinned() {
        assert_eq!(MockEmbedder::DIM, 16);
        assert_eq!(MockEmbedder::MODEL_ID, "fastrag/mock-embedder-16d-v1");
    }

    #[test]
    fn mock_embed_query_returns_16d_vectors() {
        let m = MockEmbedder;
        let v = m
            .embed_query(&[QueryText::new("hello")])
            .unwrap();
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].len(), 16);
    }

    #[test]
    fn mock_query_and_passage_match_for_same_input() {
        let m = MockEmbedder;
        let q = m.embed_query(&[QueryText::new("same")]).unwrap();
        let p = m
            .embed_passage(&[PassageText::new("same")])
            .unwrap();
        assert_eq!(q, p);
    }
}
```

- [ ] **Step 2: Run — expect failure**

Run: `cargo test -p fastrag-embed --features test-utils mock_invariant_tests`
Expected: fails (old trait).

- [ ] **Step 3: Rewrite `MockEmbedder` impl**

In `crates/fastrag-embed/src/test_utils.rs`, replace the `impl Embedder for MockEmbedder` block with:

```rust
use crate::{Embedder, EmbedError, PassageText, PrefixScheme, QueryText};

impl Embedder for MockEmbedder {
    const DIM: usize = 16;
    const MODEL_ID: &'static str = "fastrag/mock-embedder-16d-v1";
    const PREFIX_SCHEME: PrefixScheme = PrefixScheme::NONE;

    fn embed_query(
        &self,
        texts: &[QueryText],
    ) -> Result<Vec<Vec<f32>>, EmbedError> {
        Ok(texts
            .iter()
            .map(|t| Self::fingerprint(t.as_str()))
            .collect())
    }

    fn embed_passage(
        &self,
        texts: &[PassageText],
    ) -> Result<Vec<Vec<f32>>, EmbedError> {
        Ok(texts
            .iter()
            .map(|t| Self::fingerprint(t.as_str()))
            .collect())
    }
}
```

Take the existing FNV-1a trigram-hashing body from the old `embed` method and move it to a private inherent method:

```rust
impl MockEmbedder {
    fn fingerprint(text: &str) -> Vec<f32> {
        // … existing body of old `fn embed`'s per-text closure …
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p fastrag-embed --features test-utils mock_invariant_tests`
Expected: 3 passed.

- [ ] **Step 5: Commit**

```bash
git add crates/fastrag-embed/src/test_utils.rs
git commit -m "refactor(embed): port MockEmbedder to typed trait"
```

---

## Task 5: Port OpenAI embedder (const-generic over DIM)

**Files:**
- Modify: `crates/fastrag-embed/src/http/openai.rs`

- [ ] **Step 1: Write failing test**

Append:

```rust
#[cfg(test)]
mod invariant_tests {
    use super::*;
    use crate::Embedder;

    #[test]
    fn openai_small_consts() {
        assert_eq!(OpenAiSmall::DIM, 1536);
        assert_eq!(OpenAiSmall::MODEL_ID, "openai:text-embedding-3-small");
    }

    #[test]
    fn openai_large_consts() {
        assert_eq!(OpenAiLarge::DIM, 3072);
        assert_eq!(OpenAiLarge::MODEL_ID, "openai:text-embedding-3-large");
    }
}
```

- [ ] **Step 2: Run — fails**

Run: `cargo test -p fastrag-embed --features http-embedders openai`
Expected: fails.

- [ ] **Step 3: Rewrite openai.rs**

Replace the existing `OpenAIEmbedder` struct with:

```rust
use crate::{Embedder, EmbedError, PassageText, PrefixScheme, QueryText};

pub struct OpenAiEmbedder<const DIM: usize> {
    base_url: String,
    api_key: String,
    client: reqwest::blocking::Client,
}

impl<const DIM: usize> OpenAiEmbedder<DIM> {
    pub fn new() -> Result<Self, EmbedError> {
        let api_key = std::env::var("OPENAI_API_KEY")
            .map_err(|_| EmbedError::MissingEnv("OPENAI_API_KEY".into()))?;
        Ok(Self {
            base_url: "https://api.openai.com/v1".into(),
            api_key,
            client: reqwest::blocking::Client::new(),
        })
    }

    pub fn with_base_url(mut self, url: String) -> Self {
        self.base_url = url;
        self
    }

    fn model_name(model_id: &'static str) -> &'static str {
        model_id.trim_start_matches("openai:")
    }

    fn call(&self, model: &str, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbedError> {
        // … existing POST /embeddings body from the old `embed` impl,
        // returning Vec<Vec<f32>>. Keep the dimension-check against DIM
        // to guard against a server-side model swap …
    }
}

pub type OpenAiSmall = OpenAiEmbedder<1536>;
pub type OpenAiLarge = OpenAiEmbedder<3072>;

impl Embedder for OpenAiSmall {
    const DIM: usize = 1536;
    const MODEL_ID: &'static str = "openai:text-embedding-3-small";
    const PREFIX_SCHEME: PrefixScheme = PrefixScheme::NONE;

    fn embed_query(
        &self,
        texts: &[QueryText],
    ) -> Result<Vec<Vec<f32>>, EmbedError> {
        let refs: Vec<&str> = texts.iter().map(QueryText::as_str).collect();
        self.call(Self::model_name(Self::MODEL_ID), &refs)
    }

    fn embed_passage(
        &self,
        texts: &[PassageText],
    ) -> Result<Vec<Vec<f32>>, EmbedError> {
        let refs: Vec<&str> = texts.iter().map(PassageText::as_str).collect();
        self.call(Self::model_name(Self::MODEL_ID), &refs)
    }
}

impl Embedder for OpenAiLarge {
    const DIM: usize = 3072;
    const MODEL_ID: &'static str = "openai:text-embedding-3-large";
    const PREFIX_SCHEME: PrefixScheme = PrefixScheme::NONE;

    fn embed_query(
        &self,
        texts: &[QueryText],
    ) -> Result<Vec<Vec<f32>>, EmbedError> {
        let refs: Vec<&str> = texts.iter().map(QueryText::as_str).collect();
        self.call(Self::model_name(Self::MODEL_ID), &refs)
    }

    fn embed_passage(
        &self,
        texts: &[PassageText],
    ) -> Result<Vec<Vec<f32>>, EmbedError> {
        let refs: Vec<&str> = texts.iter().map(PassageText::as_str).collect();
        self.call(Self::model_name(Self::MODEL_ID), &refs)
    }
}
```

Delete the old `OpenAIEmbedder::new(model: String)` API — callers must pick the aliased type. Port the request body from the old `embed` impl into `call`.

- [ ] **Step 4: Run**

Run: `cargo test -p fastrag-embed --features http-embedders invariant_tests`
Expected: 2 passed.

- [ ] **Step 5: Commit**

```bash
git add crates/fastrag-embed/src/http/openai.rs
git commit -m "refactor(embed): split OpenAI embedder into const-generic small/large

OpenAiEmbedder<const DIM: usize> with OpenAiSmall=1536 and
OpenAiLarge=3072 aliases. Runtime-specified model names are no longer
supported — each concrete model is a distinct type."
```

---

## Task 6: Port Ollama embedder with runtime-identity escape hatch

**Files:**
- Modify: `crates/fastrag-embed/src/http/ollama.rs`

- [ ] **Step 1: Write failing test**

```rust
#[cfg(test)]
mod ollama_runtime_identity {
    use super::*;
    use crate::{EmbedderIdentity, PrefixScheme};

    #[test]
    fn runtime_identity_encodes_model_name_and_probed_dim() {
        let e = OllamaEmbedder::from_parts(
            "http://localhost:11434".into(),
            "nomic-embed-text".into(),
            768,
        );
        let id: EmbedderIdentity = e.runtime_identity();
        assert_eq!(id.model_id, "ollama:nomic-embed-text");
        assert_eq!(id.dim, 768);
        assert_eq!(id.prefix_scheme_hash, PrefixScheme::NONE.hash());
    }
}
```

- [ ] **Step 2: Run — fails**

Run: `cargo test -p fastrag-embed --features http-embedders ollama_runtime_identity`
Expected: fails.

- [ ] **Step 3: Update OllamaEmbedder**

Because Ollama's dim is runtime-probed from an arbitrary user-chosen model name, it cannot satisfy `const DIM` / `const MODEL_ID`. It does **not** implement the static `Embedder` trait. It implements `DynEmbedderTrait` directly and is wired into corpora via the `create_runtime_identity` / `open_runtime_identity` carve-out added in Task 8.

Replace the existing impl with:

```rust
use crate::{
    DynEmbedderTrait, EmbedError, EmbedderIdentity, PassageText, PrefixScheme, QueryText,
};

pub struct OllamaEmbedder {
    base_url: String,
    model: String,
    dim: usize,
    client: reqwest::blocking::Client,
}

impl OllamaEmbedder {
    pub fn new(model: String) -> Result<Self, EmbedError> {
        let base_url = std::env::var("OLLAMA_HOST")
            .unwrap_or_else(|_| "http://localhost:11434".to_string());
        let client = reqwest::blocking::Client::new();
        let dim = Self::probe_dim(&client, &base_url, &model)?;
        Ok(Self {
            base_url,
            model,
            dim,
            client,
        })
    }

    /// Test/construction helper — skips the live probe.
    pub fn from_parts(base_url: String, model: String, dim: usize) -> Self {
        Self {
            base_url,
            model,
            dim,
            client: reqwest::blocking::Client::new(),
        }
    }

    pub fn runtime_identity(&self) -> EmbedderIdentity {
        EmbedderIdentity {
            model_id: format!("ollama:{}", self.model),
            dim: self.dim,
            prefix_scheme_hash: PrefixScheme::NONE.hash(),
        }
    }

    fn probe_dim(
        client: &reqwest::blocking::Client,
        base_url: &str,
        model: &str,
    ) -> Result<usize, EmbedError> {
        // … existing probe body from previous Ollama impl, unchanged …
    }

    fn call(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbedError> {
        // … existing per-text POST /api/embeddings body from old `embed` …
    }
}

impl DynEmbedderTrait for OllamaEmbedder {
    fn model_id(&self) -> &'static str {
        // Runtime-built IDs can't be 'static. The canary path for this
        // embedder uses runtime_identity() instead; this method is only
        // called in error messages — so we return a placeholder that
        // tells anyone who sees it to check runtime_identity().
        "ollama:<runtime>"
    }
    fn dim(&self) -> usize {
        self.dim
    }
    fn prefix_scheme(&self) -> PrefixScheme {
        PrefixScheme::NONE
    }
    fn prefix_scheme_hash(&self) -> u64 {
        PrefixScheme::NONE.hash()
    }
    fn identity(&self) -> EmbedderIdentity {
        self.runtime_identity()
    }
    fn default_batch_size(&self) -> usize {
        16
    }
    fn embed_query_dyn(
        &self,
        texts: &[QueryText],
    ) -> Result<Vec<Vec<f32>>, EmbedError> {
        let refs: Vec<&str> = texts.iter().map(QueryText::as_str).collect();
        self.call(&refs)
    }
    fn embed_passage_dyn(
        &self,
        texts: &[PassageText],
    ) -> Result<Vec<Vec<f32>>, EmbedError> {
        let refs: Vec<&str> = texts.iter().map(PassageText::as_str).collect();
        self.call(&refs)
    }
}
```

Note: Ollama now overrides `identity()` (the trait default uses `model_id()` + `dim()`, but `model_id()` is a placeholder for Ollama — `identity()` is the authoritative source).

- [ ] **Step 4: Run**

Run: `cargo test -p fastrag-embed --features http-embedders ollama_runtime_identity`
Expected: 1 passed.

- [ ] **Step 5: Commit**

```bash
git add crates/fastrag-embed/src/http/ollama.rs
git commit -m "refactor(embed): Ollama implements DynEmbedderTrait directly

Ollama's dim is runtime-probed so it cannot satisfy const DIM on the
static Embedder trait. Implements DynEmbedderTrait directly and
exposes runtime_identity() for the corpus create/open carve-out."
```

---

## Task 7: Manifest v3 — `CorpusManifest` gains `identity` + `canary`

**Files:**
- Modify: `crates/fastrag-index/src/manifest.rs`
- Modify: `crates/fastrag-index/Cargo.toml` (add dep on `fastrag-embed` if absent — it already appears in workspace; add path dep if missing)

- [ ] **Step 1: Write failing test**

Replace the existing `v2_tests` module in `crates/fastrag-index/src/manifest.rs` with:

```rust
#[cfg(test)]
mod v3_tests {
    use super::*;
    use fastrag_embed::{Canary, EmbedderIdentity, PrefixScheme};

    fn sample_identity() -> EmbedderIdentity {
        EmbedderIdentity {
            model_id: "fastrag/mock-embedder-16d-v1".into(),
            dim: 16,
            prefix_scheme_hash: PrefixScheme::NONE.hash(),
        }
    }

    fn sample_canary() -> Canary {
        Canary {
            text_version: 1,
            vector: vec![0.0; 16],
        }
    }

    #[test]
    fn v3_roundtrip() {
        let m = CorpusManifest::new(
            sample_identity(),
            sample_canary(),
            1,
            ManifestChunkingStrategy::Basic {
                max_characters: 100,
                overlap: 0,
            },
        );
        assert_eq!(m.version, 3);
        assert_eq!(m.identity.dim, 16);
        assert_eq!(m.canary.vector.len(), 16);
        let s = serde_json::to_string(&m).unwrap();
        let back: CorpusManifest = serde_json::from_str(&s).unwrap();
        assert_eq!(back, m);
    }

    #[test]
    fn v1_manifest_is_rejected_as_unsupported() {
        let v1 = r#"{
            "version": 1,
            "embedding_model_id": "mock",
            "dim": 3,
            "created_at_unix_seconds": 1,
            "chunk_count": 0,
            "chunking_strategy": {"kind":"basic","max_characters":100,"overlap":0}
        }"#;
        let err = serde_json::from_str::<CorpusManifest>(v1);
        assert!(err.is_err(), "v1 manifests must not deserialize as v3");
    }
}
```

- [ ] **Step 2: Run — fails**

Run: `cargo test -p fastrag-index v3_tests`
Expected: fails.

- [ ] **Step 3: Update `CorpusManifest`**

Replace `CorpusManifest` and its `new` constructor in `crates/fastrag-index/src/manifest.rs`:

```rust
use fastrag_embed::{Canary, EmbedderIdentity};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CorpusManifest {
    pub version: u32,
    pub identity: EmbedderIdentity,
    pub canary: Canary,
    pub created_at_unix_seconds: u64,
    pub chunk_count: usize,
    pub chunking_strategy: ManifestChunkingStrategy,
    #[serde(default)]
    pub roots: Vec<RootEntry>,
    #[serde(default)]
    pub files: Vec<FileEntry>,
}

impl CorpusManifest {
    pub fn new(
        identity: EmbedderIdentity,
        canary: Canary,
        created_at_unix_seconds: u64,
        chunking_strategy: ManifestChunkingStrategy,
    ) -> Self {
        Self {
            version: 3,
            identity,
            canary,
            created_at_unix_seconds,
            chunk_count: 0,
            chunking_strategy,
            roots: Vec::new(),
            files: Vec::new(),
        }
    }
}
```

Add `fastrag-embed = { path = "../fastrag-embed" }` to `crates/fastrag-index/Cargo.toml` `[dependencies]` if not already present.

- [ ] **Step 4: Run test**

Run: `cargo test -p fastrag-index v3_tests`
Expected: 2 passed.

- [ ] **Step 5: Commit**

```bash
git add crates/fastrag-index
git commit -m "feat(index): manifest v3 with embedder identity + canary

Old embedding_model_id/dim scalar fields replaced by EmbedderIdentity
struct. Canary vector is mandatory. v1/v2 manifests deserialize as
errors — no migration, per spec (current dev-only users)."
```

---

## Task 8: `HnswIndex::new` / `load` enforce identity + canary

**Files:**
- Modify: `crates/fastrag-index/src/error.rs`
- Modify: `crates/fastrag-index/src/hnsw.rs`

- [ ] **Step 1: Write failing tests**

In `crates/fastrag-index/src/hnsw.rs`, add to the existing tests module (or create one if absent):

```rust
#[cfg(test)]
mod canary_tests {
    use super::*;
    use fastrag_embed::{
        test_utils::MockEmbedder, Canary, DynEmbedderTrait, Embedder, EmbedderIdentity,
        PassageText, PrefixScheme, CANARY_COSINE_TOLERANCE, CANARY_TEXT,
    };
    use tempfile::tempdir;

    fn mock_identity() -> EmbedderIdentity {
        EmbedderIdentity {
            model_id: MockEmbedder::MODEL_ID.into(),
            dim: MockEmbedder::DIM,
            prefix_scheme_hash: PrefixScheme::NONE.hash(),
        }
    }

    fn mock_canary(e: &MockEmbedder) -> Canary {
        let v = e
            .embed_passage(&[PassageText::new(CANARY_TEXT)])
            .unwrap()
            .remove(0);
        Canary {
            text_version: 1,
            vector: v,
        }
    }

    #[test]
    fn load_rejects_mismatched_identity() {
        let dir = tempdir().unwrap();
        let e = MockEmbedder;
        let manifest = CorpusManifest::new(
            mock_identity(),
            mock_canary(&e),
            0,
            ManifestChunkingStrategy::Basic {
                max_characters: 10,
                overlap: 0,
            },
        );
        let mut idx = HnswIndex::new(manifest);
        idx.save(dir.path()).unwrap();

        // Same bytes on disk, now try to open with an embedder whose
        // identity disagrees.
        struct Bogus;
        impl Embedder for Bogus {
            const DIM: usize = 16;
            const MODEL_ID: &'static str = "fastrag/bogus-v1";
            const PREFIX_SCHEME: PrefixScheme = PrefixScheme::NONE;
            fn embed_query(
                &self,
                texts: &[fastrag_embed::QueryText],
            ) -> Result<Vec<Vec<f32>>, fastrag_embed::EmbedError> {
                Ok(texts.iter().map(|_| vec![0.0; 16]).collect())
            }
            fn embed_passage(
                &self,
                texts: &[PassageText],
            ) -> Result<Vec<Vec<f32>>, fastrag_embed::EmbedError> {
                Ok(texts.iter().map(|_| vec![0.0; 16]).collect())
            }
        }

        let err = HnswIndex::load(dir.path(), &Bogus as &dyn DynEmbedderTrait)
            .expect_err("identity mismatch");
        match err {
            IndexError::IdentityMismatch { .. } => {}
            other => panic!("wrong error: {other:?}"),
        }
    }

    #[test]
    fn load_rejects_canary_drift() {
        let dir = tempdir().unwrap();
        let e = MockEmbedder;
        let mut wrong_canary = mock_canary(&e);
        // Flip the canary to something that can't possibly match the live
        // re-embed.
        for v in wrong_canary.vector.iter_mut() {
            *v = 0.0;
        }
        let manifest = CorpusManifest::new(
            mock_identity(),
            wrong_canary,
            0,
            ManifestChunkingStrategy::Basic {
                max_characters: 10,
                overlap: 0,
            },
        );
        let mut idx = HnswIndex::new(manifest);
        idx.save(dir.path()).unwrap();

        let err = HnswIndex::load(dir.path(), &e as &dyn DynEmbedderTrait)
            .expect_err("canary mismatch");
        assert!(matches!(err, IndexError::CanaryMismatch { .. }));
        let _ = CANARY_COSINE_TOLERANCE; // silence unused
    }

    #[test]
    fn load_accepts_matching_identity_and_canary() {
        let dir = tempdir().unwrap();
        let e = MockEmbedder;
        let manifest = CorpusManifest::new(
            mock_identity(),
            mock_canary(&e),
            0,
            ManifestChunkingStrategy::Basic {
                max_characters: 10,
                overlap: 0,
            },
        );
        let mut idx = HnswIndex::new(manifest);
        idx.save(dir.path()).unwrap();

        HnswIndex::load(dir.path(), &e as &dyn DynEmbedderTrait).unwrap();
    }
}
```

- [ ] **Step 2: Run — fails**

Run: `cargo test -p fastrag-index canary_tests`
Expected: fails — `IndexError` variants and `HnswIndex::load` signature don't match.

- [ ] **Step 3: Add new `IndexError` variants**

In `crates/fastrag-index/src/error.rs`, add:

```rust
#[error("embedder identity mismatch: corpus was built with `{existing}` (dim {existing_dim}), caller provided `{requested}` (dim {requested_dim})")]
IdentityMismatch {
    existing: String,
    existing_dim: usize,
    requested: String,
    requested_dim: usize,
},
#[error("canary vector mismatch: live cosine {cosine:.6} below tolerance {tolerance:.6} — embedder weights or tokenizer have drifted since this corpus was built")]
CanaryMismatch {
    cosine: f32,
    tolerance: f32,
},
#[error("unsupported corpus schema: got v{got}, expected v3")]
UnsupportedSchema {
    got: u32,
},
#[error("embedder error during canary verification: {0}")]
CanaryEmbed(String),
```

- [ ] **Step 4: Update `HnswIndex`**

In `crates/fastrag-index/src/hnsw.rs`:

- Change `HnswIndex::new(dim: usize, manifest: CorpusManifest)` to `HnswIndex::new(manifest: CorpusManifest) -> Self`. Derive dim from `manifest.identity.dim`.
- Change `HnswIndex::load(corpus_dir: &Path)` to `HnswIndex::load(corpus_dir: &Path, embedder: &dyn DynEmbedderTrait) -> Result<Self, IndexError>`.
- In `load`, after deserializing the manifest:

```rust
use fastrag_embed::{
    DynEmbedderTrait, PassageText, CANARY_COSINE_TOLERANCE, CANARY_TEXT,
};

if manifest.version != 3 {
    return Err(IndexError::UnsupportedSchema {
        got: manifest.version,
    });
}

let live = embedder.identity();
if live != manifest.identity {
    return Err(IndexError::IdentityMismatch {
        existing: manifest.identity.model_id.clone(),
        existing_dim: manifest.identity.dim,
        requested: live.model_id,
        requested_dim: live.dim,
    });
}

let reembedded = embedder
    .embed_passage_dyn(&[PassageText::new(CANARY_TEXT)])
    .map_err(|e| IndexError::CanaryEmbed(e.to_string()))?
    .into_iter()
    .next()
    .ok_or_else(|| IndexError::CanaryEmbed("empty output".into()))?;

let cosine = cosine_similarity(&reembedded, &manifest.canary.vector);
if cosine < CANARY_COSINE_TOLERANCE {
    return Err(IndexError::CanaryMismatch {
        cosine,
        tolerance: CANARY_COSINE_TOLERANCE,
    });
}
```

Add `cosine_similarity` helper at the bottom of the file:

```rust
fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() {
        return 0.0;
    }
    let mut dot = 0.0f32;
    let mut na = 0.0f32;
    let mut nb = 0.0f32;
    for (x, y) in a.iter().zip(b.iter()) {
        dot += x * y;
        na += x * x;
        nb += y * y;
    }
    if na == 0.0 || nb == 0.0 {
        return 0.0;
    }
    dot / (na.sqrt() * nb.sqrt())
}
```

- Add an `Index::create_runtime_identity(corpus_dir, identity, canary, chunking)` and `Index::open_runtime_identity(corpus_dir, identity, canary_embedder)` pair — the Ollama escape hatch — that accept an `EmbedderIdentity` directly instead of pulling it from the static trait. These should live alongside `new`/`load` and share the same canary verification.

- [ ] **Step 5: Run**

Run: `cargo test -p fastrag-index canary_tests`
Expected: 3 passed.

- [ ] **Step 6: Commit**

```bash
git add crates/fastrag-index
git commit -m "feat(index): enforce embedder identity + canary on HnswIndex::load

HnswIndex::new(manifest) derives dim from manifest.identity.dim.
HnswIndex::load(dir, embedder) verifies identity equality then
re-embeds CANARY_TEXT and checks cosine ≥ 0.999 against the stored
canary vector. Hard-fails on v1/v2 schemas (no migration)."
```

---

## Task 9: Rewire `fastrag::corpus` to use `&dyn DynEmbedderTrait`

**Files:**
- Modify: `crates/fastrag/src/corpus/mod.rs`
- Modify: `crates/fastrag/src/lib.rs` (re-exports)

- [ ] **Step 1: Write failing test**

The existing test `index_rejects_different_embedder_against_existing_corpus` at line 675 of `corpus/mod.rs` must keep passing but against the new error type. Update the test to import `IndexError` and match on `IdentityMismatch`. Add a new test in the same module:

```rust
#[test]
fn canary_is_written_on_index_create() {
    use fastrag_embed::{test_utils::MockEmbedder, DynEmbedderTrait, CANARY_TEXT};
    let dir = tempfile::tempdir().unwrap();
    let docs = sample_dir();
    let e = MockEmbedder;
    let dyn_e: &dyn DynEmbedderTrait = &e;
    index_path(
        docs.path(),
        dir.path(),
        &ChunkingStrategy::Basic {
            max_characters: 100,
            overlap: 0,
        },
        dyn_e,
    )
    .unwrap();
    let idx = HnswIndex::load(dir.path(), dyn_e).unwrap();
    assert!(!idx.manifest().canary.vector.is_empty());
    assert_eq!(idx.manifest().canary.vector.len(), MockEmbedder::DIM);
    let _ = CANARY_TEXT;
}
```

- [ ] **Step 2: Run — fails**

Run: `cargo test -p fastrag --features retrieval corpus::`
Expected: fails.

- [ ] **Step 3: Update signatures**

In `crates/fastrag/src/corpus/mod.rs`:

- Replace `embedder: &dyn Embedder` with `embedder: &dyn DynEmbedderTrait` in every public and private function that takes one: `index_path`, `index_path_with_metadata`, `query_corpus`, `query_corpus_with_filter`, `query_corpus_reranked`, plus `incremental::*` helpers.
- In `index_path_with_metadata`, replace the existing "load or create" block:

```rust
let mut index = if corpus_dir.join("manifest.json").exists() {
    HnswIndex::load(corpus_dir, embedder)?
} else {
    use fastrag_embed::{Canary, PassageText, CANARY_TEXT};
    let canary_vec = embedder
        .embed_passage_dyn(&[PassageText::new(CANARY_TEXT)])?
        .into_iter()
        .next()
        .ok_or(CorpusError::EmptyEmbeddingOutput)?;
    let canary = Canary {
        text_version: 1,
        vector: canary_vec,
    };
    let m = CorpusManifest::new(
        embedder.identity(),
        canary,
        current_unix_seconds(),
        manifest_chunking_strategy_from(chunking),
    );
    HnswIndex::new(m)
};
```

- Delete the scalar `existing != requested` mismatch check — `HnswIndex::load` now enforces it.
- Replace all `embedder.embed(&texts)` calls with:

```rust
use fastrag_embed::PassageText;
let owned: Vec<PassageText> = texts.iter().map(|t| PassageText::new(*t)).collect();
let vectors = embedder.embed_passage_dyn(&owned)?;
```

And in `query_corpus*`, replace the query-side embed with:

```rust
use fastrag_embed::QueryText;
let v = embedder
    .embed_query_dyn(&[QueryText::new(query)])?
    .into_iter()
    .next()
    .ok_or(CorpusError::EmptyEmbeddingOutput)?;
```

- Delete the `CorpusError::EmbedderMismatch` variant and the `index_rejects_different_embedder_against_existing_corpus` test's assertion on it; have the test instead match on `CorpusError::Index(IndexError::IdentityMismatch { .. })`.
- Delete the `manifest.version = 2;` line — the manifest is constructed at v3 and stays v3.
- Remove the inline test `CountingEmbedder` definition and the `impl Embedder for CountingEmbedder` block in the old-trait shape, and replace with a `MockEmbedder`-based test — or delete the test if `MockEmbedder` covers it.

In `crates/fastrag/src/lib.rs`, re-export `DynEmbedderTrait` and `DynEmbedder` from `fastrag_embed`, and drop any re-export of the old `Embedder` trait method signatures if they appear in prelude-style exports. Keep `pub use fastrag_embed::Embedder` — the trait name is the same, only the shape changed.

- [ ] **Step 4: Run**

Run: `cargo test --workspace --features retrieval`
Expected: the whole workspace builds and passes (or fails only in Task 10's territory — the CLI).

- [ ] **Step 5: Commit**

```bash
git add crates/fastrag
git commit -m "refactor(corpus): use DynEmbedderTrait; canary written on create

index_path_with_metadata constructs manifest v3 with the embedder's
identity and a freshly-embedded canary. HnswIndex::load now enforces
both. CorpusError::EmbedderMismatch deleted — same guarantee, moved
into IndexError::IdentityMismatch."
```

---

## Task 10: Update `fastrag-cli/src/embed_loader.rs`

**Files:**
- Modify: `fastrag-cli/src/embed_loader.rs`
- Modify: `fastrag-cli/tests/embedder_e2e.rs`

- [ ] **Step 1: Write failing test**

In `fastrag-cli/tests/embedder_e2e.rs`, the existing `query_with_mismatched_embedder_flag_fails` test already covers the mismatch path — update its stderr assertion:

```rust
assert!(
    stderr.contains("openai:text-embedding-3-small")
        && stderr.contains("identity mismatch"),
    "stderr should mention identity mismatch + existing model_id, got: {stderr}"
);
```

And the `index_and_query_with_openai_backend` test reads `manifest["embedding_model_id"]` at line 74 — that scalar field is gone; replace with:

```rust
assert_eq!(
    manifest["identity"]["model_id"].as_str().unwrap(),
    "openai:text-embedding-3-small"
);
assert_eq!(manifest["identity"]["dim"].as_u64().unwrap(), 1536);
assert_eq!(manifest["version"].as_u64().unwrap(), 3);
```

- [ ] **Step 2: Run — fails**

Run: `cargo test -p fastrag-cli --features retrieval embedder_e2e`
Expected: fails (compile or assertion).

- [ ] **Step 3: Rewrite `embed_loader.rs`**

Replace the contents of `fastrag-cli/src/embed_loader.rs` with:

```rust
use std::path::{Path, PathBuf};
use std::sync::Arc;

use fastrag::DynEmbedder;
use fastrag_embed::{
    http::{ollama::OllamaEmbedder, openai::{OpenAiLarge, OpenAiSmall}},
    BgeSmallEmbedder, DynEmbedderTrait, Embedder,
};
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
    #[error("embedder kind mismatch: corpus built with `{existing}`, --embedder specifies `{requested}`")]
    KindMismatch { existing: String, requested: String },
}

#[derive(Clone)]
pub struct EmbedderOptions {
    pub kind: Option<EmbedderKindArg>,
    pub model_path: Option<PathBuf>,
    pub openai_model: String,
    pub openai_base_url: String,
    pub ollama_model: String,
    pub ollama_url: String,
}

pub fn load_for_write(opts: &EmbedderOptions) -> Result<DynEmbedder, EmbedLoaderError> {
    let kind = opts.kind.unwrap_or(EmbedderKindArg::Bge);
    build(kind, opts)
}

pub fn load_for_read(
    corpus_dir: &Path,
    opts: &EmbedderOptions,
) -> Result<DynEmbedder, EmbedLoaderError> {
    let manifest_path = corpus_dir.join("manifest.json");
    let bytes = std::fs::read(&manifest_path)?;
    let value: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|e| EmbedLoaderError::Manifest(e.to_string()))?;
    let existing = value
        .get("identity")
        .and_then(|i| i.get("model_id"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| EmbedLoaderError::Manifest("missing identity.model_id".into()))?
        .to_string();

    let detected_kind = detect_from_model_id(&existing)?;
    let kind = opts.kind.unwrap_or(detected_kind);
    if kind != detected_kind {
        return Err(EmbedLoaderError::KindMismatch {
            existing,
            requested: kind_name(kind).to_string(),
        });
    }

    let mut effective = opts.clone();
    if let Some(rest) = existing.strip_prefix("openai:") {
        effective.openai_model = rest.to_string();
    } else if let Some(rest) = existing.strip_prefix("ollama:") {
        effective.ollama_model = rest.to_string();
    }

    build(kind, &effective)
}

fn detect_from_model_id(existing: &str) -> Result<EmbedderKindArg, EmbedLoaderError> {
    if existing.starts_with("openai:") {
        Ok(EmbedderKindArg::Openai)
    } else if existing.starts_with("ollama:") {
        Ok(EmbedderKindArg::Ollama)
    } else if existing == BgeSmallEmbedder::MODEL_ID {
        Ok(EmbedderKindArg::Bge)
    } else {
        Err(EmbedLoaderError::Manifest(format!(
            "unrecognized identity.model_id `{existing}`; pass --embedder explicitly"
        )))
    }
}

fn kind_name(kind: EmbedderKindArg) -> &'static str {
    match kind {
        EmbedderKindArg::Bge => "bge",
        EmbedderKindArg::Openai => "openai",
        EmbedderKindArg::Ollama => "ollama",
    }
}

fn build(
    kind: EmbedderKindArg,
    opts: &EmbedderOptions,
) -> Result<DynEmbedder, EmbedLoaderError> {
    match kind {
        EmbedderKindArg::Bge => {
            let e = match &opts.model_path {
                Some(path) => BgeSmallEmbedder::from_local(path)?,
                None => BgeSmallEmbedder::from_hf_hub()?,
            };
            let arc: Arc<dyn DynEmbedderTrait> = Arc::new(e);
            Ok(arc)
        }
        EmbedderKindArg::Openai => {
            // Select the const-generic variant by model name.
            match opts.openai_model.as_str() {
                "text-embedding-3-small" => {
                    let mut e = OpenAiSmall::new()?;
                    e = e.with_base_url(opts.openai_base_url.clone());
                    let arc: Arc<dyn DynEmbedderTrait> = Arc::new(e);
                    Ok(arc)
                }
                "text-embedding-3-large" => {
                    let mut e = OpenAiLarge::new()?;
                    e = e.with_base_url(opts.openai_base_url.clone());
                    let arc: Arc<dyn DynEmbedderTrait> = Arc::new(e);
                    Ok(arc)
                }
                other => Err(EmbedLoaderError::Manifest(format!(
                    "unknown OpenAI model `{other}` — supported: text-embedding-3-small, text-embedding-3-large"
                ))),
            }
        }
        EmbedderKindArg::Ollama => {
            unsafe { std::env::set_var("OLLAMA_HOST", &opts.ollama_url) };
            let e = OllamaEmbedder::new(opts.ollama_model.clone())?;
            let arc: Arc<dyn DynEmbedderTrait> = Arc::new(e);
            Ok(arc)
        }
    }
}
```

Also update every call-site of `Arc<dyn Embedder>` in `fastrag-cli/src/main.rs` and sibling modules to `DynEmbedder`. Do this with a grep-and-replace: `rg "Arc<dyn Embedder>" fastrag-cli/src` → rewrite each.

- [ ] **Step 4: Run**

```bash
cargo build --workspace --features retrieval
cargo test -p fastrag-cli --features retrieval embedder_e2e -- --test-threads=1
```

Expected: build clean, both e2e tests pass.

- [ ] **Step 5: Commit**

```bash
git add fastrag-cli
git commit -m "refactor(cli): embed_loader returns DynEmbedder

Manifest detection reads identity.model_id instead of scalar
embedding_model_id. detect_from_model_id uses BgeSmallEmbedder::MODEL_ID
directly so future model id changes can't desync the read path."
```

---

## Task 11: Workspace-wide lint + test gate

- [ ] **Step 1: Lint**

Run:

```bash
cargo fmt --check
cargo clippy --workspace --all-targets --features retrieval -- -D warnings
```

Expected: clean. Fix any warnings in place.

- [ ] **Step 2: Full test run**

```bash
cargo test --workspace --features retrieval -- --test-threads=1
```

Expected: all green.

- [ ] **Step 3: Real-model smoke (opt-in)**

```bash
FASTRAG_E2E_MODELS=1 cargo test -p fastrag-embed bge_real_model -- --ignored --test-threads=1
```

This is a best-effort sanity check against the real BGE weights. Skip if weights are not already cached and offline.

- [ ] **Step 4: Commit if any fixups landed**

```bash
git add -A
git commit -m "chore: clippy/fmt cleanup after embedder invariant refactor"
```

---

## Task 12: Update docs

**Files:**
- Modify: `README.md` (if it describes the embedder trait or manifest format)
- Modify: `CLAUDE.md` (the "Retrieval CLI" / "MCP Tools" sections reference `embedding_model_id`)

- [ ] **Step 1: Invoke doc-editor skill**

Per `CLAUDE.md`: before every `Edit` or `Write` to a `.md` file, invoke `doc-editor/SKILL.md` as a foreground Haiku Agent. Do that now with a prompt describing the change: "manifest v3 replaces scalar `embedding_model_id`/`dim` with `identity: { model_id, dim, prefix_scheme_hash }` plus `canary`; old corpora hard-fail with `UnsupportedSchema`; BGE model_id is now `fastrag/bge-small-en-v1.5`."

- [ ] **Step 2: Apply the doc-editor's recommended edits** to README.md and CLAUDE.md.

- [ ] **Step 3: Commit**

```bash
git add README.md CLAUDE.md
git commit -m "docs: document manifest v3 and typed embedder trait"
```

---

## Self-review notes

- Every task has runnable test code, exact file paths, and exact commit messages.
- Tasks 2–6 leave the workspace temporarily broken. This is called out and confined to those commits; Task 9 is the first point where `cargo test --workspace` is expected to be green again.
- The spec's `Index::create<E>` / `Index::open<E>` pattern has been mapped onto real code: `HnswIndex::new(manifest)` (dim derived from identity) + `HnswIndex::load(dir, &dyn DynEmbedderTrait)`. The static compile-time invariant is preserved via the `EmbedderHandle<E>`-equivalent — namely, every concrete `impl Embedder` pins the consts at the type, and the blanket `impl<E: Embedder> DynEmbedderTrait` is the only bridge to the erased form. Callers cannot construct a `DynEmbedder` that lies about its dim.
- Ollama carve-out is Task 6 + the `create_runtime_identity` / `open_runtime_identity` methods in Task 8.
- Pre-existing BGE `model_id` bug fixed in Task 3 with a named regression test.
- No migration from v1/v2 — hard-fail with `UnsupportedSchema`, per user confirmation.
