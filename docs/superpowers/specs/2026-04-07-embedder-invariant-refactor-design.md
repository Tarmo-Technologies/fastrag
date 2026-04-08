# Embedder Invariant Refactor (Phase 2 Step 1)

**Status:** Design — awaiting review
**Date:** 2026-04-07
**Supersedes:** nothing directly; first sub-project of the rewritten Phase 2 roadmap (`docs/superpowers/roadmap-2026-04-phase2-rewrite.md`, commit 439790b).
**Drives:** absorbs the pre-existing `BgeSmallEmbedder::model_id` vs. `detect_from_manifest` mismatch bug as a regression test.

## Goal

Make the chromadb-style dim-4 silent-trap bug (shim lesson #1) unrepresentable at the point where it historically happened — embedder construction — without forcing every CLI subcommand handler, MCP tool, and eval function to become generic. Plus: catch real-world drift (quantization change, tokenizer update, model file swap) at index open via a canary vector.

This is foundational. Every later Phase 2 step (new backends, new models, reranker, hybrid retrieval, Contextual Retrieval) lands on top of the invariant this spec establishes.

## Non-goals

- Any new embedder (nomic, arctic, gemma, e5) — that is Step 2.
- Any eval or benchmark work — the refactor ships with the same retrieval quality as today.
- Any change to the vector store, chunking, or retrieval flow.
- Migration of pre-refactor corpora — per user direction, fastrag is pre-1.0 with zero external users; old on-disk corpora hard-fail with `UnsupportedSchema` and must be deleted and re-indexed.
- Schema version negotiation beyond a single bump `v1 → v2`.
- Removing the legacy `embedding_model_id` top-level manifest field — it stays as a transitional alias until a follow-up PR confirms nothing reads it.
- CLI flag changes. `--embedder`, `--openai-model`, `--ollama-url`, etc. all stay identical.

## Architecture

### Core types

The `Embedder` trait gains associated consts and typed inputs. Because associated consts are not object-safe, the trait splits into a static side (`Embedder`) and a dyn-safe side (`DynEmbedderTrait`), with a blanket impl bridging the two.

```rust
// crates/fastrag-embed/src/lib.rs

pub struct QueryText(pub String);
pub struct PassageText(pub String);

pub struct PrefixScheme {
    pub query: Option<&'static str>,
    pub passage: Option<&'static str>,
}

impl PrefixScheme {
    pub const fn hash(&self) -> u64 {
        // const fnv-1a over the two optional prefixes. Stable across runs
        // and across architectures. Recorded in the manifest so prefix
        // drift is caught without storing the prefixes themselves.
    }
}

pub trait Embedder: Send + Sync + 'static {
    const DIM: usize;
    const MODEL_ID: &'static str;
    const PREFIX_SCHEME: PrefixScheme;

    fn embed_query(&self, texts: &[QueryText]) -> Result<Vec<Vec<f32>>, EmbedError>;
    fn embed_passage(&self, texts: &[PassageText]) -> Result<Vec<Vec<f32>>, EmbedError>;

    fn default_batch_size(&self) -> usize { 32 }
}

pub trait DynEmbedderTrait: Send + Sync {
    fn dim(&self) -> usize;
    fn model_id(&self) -> &'static str;
    fn prefix_scheme_hash(&self) -> u64;
    fn embed_query(&self, texts: &[QueryText]) -> Result<Vec<Vec<f32>>, EmbedError>;
    fn embed_passage(&self, texts: &[PassageText]) -> Result<Vec<Vec<f32>>, EmbedError>;
    fn default_batch_size(&self) -> usize;
}

impl<E: Embedder> DynEmbedderTrait for E {
    fn dim(&self) -> usize { E::DIM }
    fn model_id(&self) -> &'static str { E::MODEL_ID }
    fn prefix_scheme_hash(&self) -> u64 { E::PREFIX_SCHEME.hash() }
    fn embed_query(&self, texts: &[QueryText]) -> Result<Vec<Vec<f32>>, EmbedError> {
        <Self as Embedder>::embed_query(self, texts)
    }
    fn embed_passage(&self, texts: &[PassageText]) -> Result<Vec<Vec<f32>>, EmbedError> {
        <Self as Embedder>::embed_passage(self, texts)
    }
    fn default_batch_size(&self) -> usize { <Self as Embedder>::default_batch_size(self) }
}

pub type DynEmbedder = Arc<dyn DynEmbedderTrait>;
```

### Typed construction boundary

```rust
pub struct EmbedderHandle<E: Embedder> {
    inner: Arc<E>,
}

impl<E: Embedder> EmbedderHandle<E> {
    pub fn new(e: E) -> Self { Self { inner: Arc::new(e) } }

    pub fn identity(&self) -> EmbedderIdentity {
        EmbedderIdentity {
            model_id: E::MODEL_ID.to_string(),
            dim: E::DIM,
            prefix_scheme_hash: E::PREFIX_SCHEME.hash(),
        }
    }

    pub fn embed_query(&self, texts: &[QueryText]) -> Result<Vec<Vec<f32>>, EmbedError> {
        self.inner.embed_query(texts)
    }

    pub fn embed_passage(&self, texts: &[PassageText]) -> Result<Vec<Vec<f32>>, EmbedError> {
        self.inner.embed_passage(texts)
    }

    pub fn erase(self) -> DynEmbedder {
        self.inner as Arc<dyn DynEmbedderTrait>
    }
}
```

`EmbedderHandle<E>` is the only way to construct an `Index`. It proves statically that the identity the manifest records, the dim the vector store allocates, and the embedder that actually runs all come from the same `E`. After that proof is discharged inside `Index::create` / `Index::open`, the handle erases into `DynEmbedder` for runtime storage and dispatch.

### Manifest + canary

```rust
// crates/fastrag-index/src/manifest.rs

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct EmbedderIdentity {
    pub model_id: String,
    pub dim: usize,
    pub prefix_scheme_hash: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Canary {
    pub text_version: u32,          // currently 1
    pub vector: Vec<f32>,           // dim = identity.dim
    pub cosine_tolerance: f32,      // default 0.999, pinned after real-model smoke
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Manifest {
    pub schema_version: u32,        // bumped to 2 by this refactor
    pub identity: EmbedderIdentity,
    pub canary: Canary,
    pub doc_count: u64,
    // Transitional: legacy reader compatibility. Populated from identity.model_id
    // at write time. Removed in a follow-up PR once nothing reads it.
    pub embedding_model_id: String,
}
```

The canary text is a fixed constant in `fastrag-index`:

```rust
pub const CANARY_TEXT: &str =
    "fastrag canary v1: the quick brown fox jumps over the lazy dog";
pub const CANARY_TEXT_VERSION: u32 = 1;
pub const CANARY_DEFAULT_TOLERANCE: f32 = 0.999;
```

### Index construction

```rust
// crates/fastrag-index/src/lib.rs

impl Index {
    pub fn create<E: Embedder>(
        dir: &Path,
        handle: EmbedderHandle<E>,
    ) -> Result<Self, IndexError> {
        let identity = handle.identity();
        let canary_vec = {
            let text = PassageText(CANARY_TEXT.to_string());
            handle.embed_passage(&[text])?.into_iter().next()
                .ok_or(IndexError::CanaryMismatch { cosine: 0.0, tolerance: CANARY_DEFAULT_TOLERANCE })?
        };
        if canary_vec.len() != E::DIM {
            return Err(IndexError::Embed(EmbedError::UnexpectedDim {
                expected: E::DIM,
                got: canary_vec.len(),
            }));
        }
        let manifest = Manifest {
            schema_version: 2,
            identity: identity.clone(),
            canary: Canary {
                text_version: CANARY_TEXT_VERSION,
                vector: canary_vec,
                cosine_tolerance: CANARY_DEFAULT_TOLERANCE,
            },
            doc_count: 0,
            embedding_model_id: identity.model_id.clone(),
        };
        manifest.write(dir)?;
        // ... create empty vector store ...
        Ok(Self { manifest, vector_store, embedder: handle.erase() })
    }

    pub fn open<E: Embedder>(
        dir: &Path,
        handle: EmbedderHandle<E>,
    ) -> Result<Self, IndexError> {
        let manifest = Manifest::read(dir)?;
        if manifest.schema_version != 2 {
            return Err(IndexError::UnsupportedSchema {
                found: manifest.schema_version,
                expected: 2,
            });
        }
        let requested_identity = handle.identity();
        if manifest.identity != requested_identity {
            return Err(IndexError::IdentityMismatch {
                existing: manifest.identity,
                requested: requested_identity,
            });
        }
        verify_canary(&handle, &manifest.canary)?;
        // ... load vector store ...
        Ok(Self { manifest, vector_store, embedder: handle.erase() })
    }
}

fn verify_canary<E: Embedder>(
    handle: &EmbedderHandle<E>,
    canary: &Canary,
) -> Result<(), IndexError> {
    let text = PassageText(CANARY_TEXT.to_string());
    let vectors = handle.embed_passage(&[text])?;
    let fresh = vectors.into_iter().next()
        .ok_or(IndexError::CanaryMismatch { cosine: 0.0, tolerance: canary.cosine_tolerance })?;
    if fresh.len() != canary.vector.len() {
        return Err(IndexError::CanaryMismatch { cosine: 0.0, tolerance: canary.cosine_tolerance });
    }
    let cosine = cosine_similarity(&fresh, &canary.vector);
    if cosine < canary.cosine_tolerance {
        return Err(IndexError::CanaryMismatch { cosine, tolerance: canary.cosine_tolerance });
    }
    Ok(())
}
```

### CLI dispatch stays dynamic above the construction line

`fastrag-cli/src/embed_loader.rs` is rewritten. The old `load_for_write` / `load_for_read` functions that returned `Arc<dyn Embedder>` are removed. In their place: `create_new` and `open_existing` each match on the backend kind, build the concrete `E`, wrap it in `EmbedderHandle<E>`, and return `Index` directly.

```rust
pub fn create_new(corpus_dir: &Path, opts: &EmbedderOptions) -> Result<Index, EmbedLoaderError> {
    match opts.kind.unwrap_or(EmbedderKind::Bge) {
        EmbedderKind::Bge => {
            let handle = EmbedderHandle::new(build_bge(opts)?);
            Index::create(corpus_dir, handle).map_err(Into::into)
        }
        EmbedderKind::OpenAi => {
            let handle = EmbedderHandle::new(build_openai(opts)?);
            Index::create(corpus_dir, handle).map_err(Into::into)
        }
        EmbedderKind::Ollama => {
            let handle = EmbedderHandle::new(build_ollama(opts)?);
            Index::create(corpus_dir, handle).map_err(Into::into)
        }
    }
}

pub fn open_existing(corpus_dir: &Path, opts: &EmbedderOptions) -> Result<Index, EmbedLoaderError> {
    // Peek at the manifest identity (no canary read) to pick the right backend
    // when opts.kind is None; otherwise trust the user's kind.
    let manifest_identity = Manifest::peek_identity(corpus_dir)?;
    let kind = opts.kind.unwrap_or_else(|| detect_kind(&manifest_identity.model_id));
    match kind {
        EmbedderKind::Bge => {
            let handle = EmbedderHandle::new(build_bge(opts)?);
            Index::open(corpus_dir, handle).map_err(Into::into)
        }
        EmbedderKind::OpenAi => {
            let handle = EmbedderHandle::new(build_openai(opts)?);
            Index::open(corpus_dir, handle).map_err(Into::into)
        }
        EmbedderKind::Ollama => {
            let handle = EmbedderHandle::new(build_ollama(opts)?);
            Index::open(corpus_dir, handle).map_err(Into::into)
        }
    }
}
```

Each match arm monomorphizes `Index::create::<ConcreteType>` / `Index::open::<ConcreteType>`, so the concrete type carries `DIM` / `MODEL_ID` / `PREFIX_SCHEME` from its `impl Embedder` as compile-time constants. The CLI gets an `Index` back and every downstream handler stays `fn foo(idx: &Index, ...)` — no generics, no lifetimes, no `impl Trait` returns. The generic parameter lives for exactly the duration of the construction call.

### Embedders updated

- **`BgeSmallEmbedder`** — `MODEL_ID = "fastrag/bge-small-en-v1.5"` (fixes the pre-existing bug), `DIM = 384`, `PREFIX_SCHEME` with both prefixes `None`. Internal `embed` helper renamed to `forward` and called by both `embed_query` and `embed_passage` default-passthrough-style (BGE doesn't distinguish).
- **`OpenAiEmbedder`** — this is where the runtime-dim trick from #31a meets the compile-time const. Options:
  - (a) make it a generic struct `OpenAiEmbedder<const DIM: usize>` with concrete type aliases `OpenAiSmall = OpenAiEmbedder<1536>`, `OpenAiLarge = OpenAiEmbedder<3072>`. The CLI picks the right alias based on `opts.openai_model`. Model_id becomes a runtime method on top of the const, but `MODEL_ID` must be static — so we restrict each alias to a single real model.
  - (b) keep `OpenAiEmbedder` as one type but have each aliased constant generate its own zero-sized marker struct via a macro. Heavier on code gen, no user-visible difference.
  - **Chosen:** (a), because #31a already covers only `text-embedding-3-small` (1536) and `text-embedding-3-large` (3072) in its static dim table. Expanding that table in the future is a per-alias addition, which is exactly the friction we want — adding a new OpenAI model means one new type alias and its own `Embedder` impl. That makes dim drift a compile error instead of a silent table miss.
- **`OllamaEmbedder`** — problem: dim is probed at runtime from the Ollama server. No way to promote to a const. Options:
  - (a) drop runtime dim probing; require the user to declare the dim via a new `--ollama-dim` flag, build a runtime-validated `OllamaEmbedder<const DIM: usize>` via a macro-expanded set of known models.
  - (b) leave `OllamaEmbedder` outside the new trait — it implements `DynEmbedderTrait` directly (hand-written, not via the blanket impl), reporting its dim / model_id at runtime. The CLI still wraps it in an `EmbedderHandle`-equivalent that constructs an `EmbedderIdentity` at runtime and feeds it to `Index::create_runtime_identity` / `Index::open_runtime_identity` — a separate, explicitly-unsafe-in-docs pair of constructors.
  - **Chosen:** (b), but confined to Ollama. The rationale is honest: Ollama's contract is "whatever the server has loaded right now" and a compile-time const lies about that. We explicitly carve out a runtime-identity path and document that it gives up the compile-time guarantee in exchange for interoperability with a dynamic server. The canary still runs, so drift is still caught at open time. The CLI enforces that this path is only reachable via `--embedder ollama`.
- **`MockEmbedder`** — `MODEL_ID = "fastrag/mock"`, `DIM = 16`, `PREFIX_SCHEME` with both `None`. Straightforward; used only in tests.

### The Ollama carve-out, stated plainly

The runtime-identity path on `Index` is an explicit compromise. `Index::create_runtime_identity(dir, identity, dyn_embedder)` takes a pre-built `EmbedderIdentity` struct and a `DynEmbedder`, skips the compile-time generic binding, and writes the manifest + canary the same way `create::<E>` does. It is public but gated behind a doc comment stating: "Use only for embedders whose dim is inherently runtime-determined (e.g. Ollama, where the model identity is whatever the server has loaded). For any embedder with a knowable-at-compile-time dim, use `create::<E>` instead." `open_runtime_identity` mirrors it.

This is ugly. It is also necessary — fastrag shipped Ollama in #31a and ripping it out to keep the invariant clean would break existing use. The canary catches every real-world drift scenario on the Ollama path anyway; what we lose is the "can't write wrong-dim code" static guarantee, not the "can't open a corrupt corpus" runtime guarantee.

## Errors

```rust
// crates/fastrag-index/src/error.rs

#[derive(Debug, thiserror::Error)]
pub enum IndexError {
    #[error("corpus was built with embedder `{existing:?}`, caller requested `{requested:?}`")]
    IdentityMismatch {
        existing: EmbedderIdentity,
        requested: EmbedderIdentity,
    },

    #[error("canary cosine {cosine:.5} below tolerance {tolerance:.5} — \
             embedder weights or tokenizer have drifted since this corpus was built; \
             delete the corpus directory and re-index")]
    CanaryMismatch { cosine: f32, tolerance: f32 },

    #[error("corpus schema version {found} not supported by this build \
             (expected {expected}); pre-1.0 corpora are not migrated — \
             delete the corpus directory and re-index")]
    UnsupportedSchema { found: u32, expected: u32 },

    #[error(transparent)]
    Embed(#[from] fastrag_embed::EmbedError),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error("manifest parse: {0}")]
    Manifest(String),
}
```

The existing `CorpusError::EmbedderMismatch` from #31a is replaced by `IndexError::IdentityMismatch` because the identity struct carries strictly more information. Any call site that constructed `CorpusError::EmbedderMismatch` now either returns `IndexError::IdentityMismatch` directly or wraps it. The `EmbedLoaderError::Mismatch` in `embed_loader.rs` is kept as a thin wrapper that `From`-converts from `IndexError::IdentityMismatch` to preserve the existing CLI error formatting, or deleted if `IndexError`'s Display is already suitable — decide during implementation.

## Testing

All tests are expressible without real model weights. The whole point of Section 1's type split is that `MockEmbedder` exercises every code path the real embedders touch.

### Unit — `fastrag-embed`
- `PrefixScheme::hash` is stable: pin a concrete hash for `{None, None}`, `{Some("query: "), Some("passage: ")}`, and two BGE-style / E5-style variants as regression assertions.
- `QueryText` and `PassageText` are distinct types: a compile-fail test (`trybuild`-style or a `#[cfg(test)] fn _not_compile()`) asserting you can't pass a `QueryText` where a `PassageText` is expected. If `trybuild` is too heavy, an inline comment documenting the invariant plus a positive test that the type-checker accepts correct usage is acceptable.
- `MockEmbedder` implements `Embedder` with `DIM = 16`, `MODEL_ID = "fastrag/mock"`, and both prefixes `None`.
- The blanket impl of `DynEmbedderTrait` for `MockEmbedder` compiles and reports `dim() == 16`, `model_id() == "fastrag/mock"`, `prefix_scheme_hash()` matching the `PrefixScheme::hash` pinned value above.

### Unit — `fastrag-index`
- `Index::create::<MockEmbedder>` against a tempdir: writes manifest v2, canary vector has length 16, canary cosine is 1.0 against itself when immediately re-verified.
- `Index::open::<MockEmbedder>` against the same tempdir: success.
- `Index::open::<MockEmbedder>` against a manifest whose `identity.model_id` is mutated to `"fastrag/not-mock"` on disk: `IdentityMismatch`.
- `Index::open::<MockEmbedder>` against a manifest whose `identity.dim` is mutated to 384 on disk: `IdentityMismatch`.
- `Index::open::<MockEmbedder>` against a manifest whose `canary.vector` is mutated to zeros on disk: `CanaryMismatch`.
- `Index::open::<MockEmbedder>` against a manifest with `schema_version: 1`: `UnsupportedSchema { found: 1, expected: 2 }`.
- Dyn-safety sanity: `fn _require_dyn_safe(_: &dyn DynEmbedderTrait) {}` exists, compiles, and the `impl DynEmbedderTrait for MockEmbedder` satisfies it.

### Integration — `fastrag-cli`
- `create_new` then `open_existing` round-trip with `--embedder openai` using the wiremock harness from #31a: success.
- Cross-kind mismatch: `create_new` with `--embedder bge`, `open_existing` with `--embedder openai` flag: `IdentityMismatch` (or the `EmbedLoaderError` wrapping it), non-zero exit, stderr mentions both identities.
- Pre-existing BGE regression test: `BgeSmallEmbedder::MODEL_ID` is asserted equal to `"fastrag/bge-small-en-v1.5"`. This test fails if the constant drifts again.

### Real-model smoke, `#[ignore]`-gated on `FASTRAG_E2E_MODELS=1`
- Create a BGE corpus with one document, close, reopen. Canary cosine ≥ 0.9999. This is the run that pins `CANARY_DEFAULT_TOLERANCE` — if the first real run shows systematic drift we adjust once, document the observed value, and leave it.
- Same with Ollama against a local server (optional, only if the dev has one running): verify the `create_runtime_identity` / `open_runtime_identity` path works and catches identity drift if the Ollama model is swapped between create and open.

## Acceptance criteria

1. `cargo test --workspace --features retrieval` green.
2. `cargo clippy --workspace --all-targets --features retrieval -- -D warnings` clean.
3. A test calling `Index::open::<BgeSmallEmbedder>` against a manifest whose `identity.model_id` is `"openai:text-embedding-3-small"` fails with `IdentityMismatch` before any canary work.
4. A test that corrupts a stored canary vector fails with `CanaryMismatch`.
5. A real-BGE create + open round-trip under `FASTRAG_E2E_MODELS=1` passes with cosine > 0.9999 between the stored canary and a recomputed one.
6. `grep 'Arc<dyn Embedder\b' crates/fastrag/ fastrag-cli/src/` returns only hits inside `Index` storage and the `DynEmbedder` type alias — not in subcommand handler signatures.
7. `BgeSmallEmbedder::MODEL_ID` is the compile-time constant `"fastrag/bge-small-en-v1.5"` with a regression test that fails on drift.
8. The CLI surface (`--embedder`, `--openai-model`, `--openai-base-url`, `--ollama-model`, `--ollama-url`) is unchanged. Existing shell scripts and the #31a tests still pass without modification beyond any necessary updates to `EmbedLoaderError` error formatting.
9. Manifest schema version is 2. `Manifest::read` on a schema-1 file returns `UnsupportedSchema` with the message "delete the corpus directory and re-index".

## Risks

- **`const fn` availability for `PrefixScheme::hash`:** Rust 1.75+ supports enough `const fn` for a small FNV-1a loop. If the stable toolchain fastrag pins is older, fall back to a `once_cell::sync::Lazy<u64>` computed from the same inputs and a runtime-sanity check in tests. Both are documented in the implementation plan.
- **Ollama runtime-identity carve-out adds a second construction path on `Index`.** This is a real complexity cost and was considered carefully. The alternative (drop Ollama, macro-generate dim-typed aliases for known Ollama models) is worse because Ollama's server-side model swap is a legitimate use case and fastrag shipped support for it in #31a. The carve-out is explicit, narrowly scoped, and the canary still runs on that path.
- **OpenAI generic-dim split** (`OpenAiEmbedder<1536>`, `OpenAiEmbedder<3072>`) touches #31a's backend code. The existing wiremock tests will need to pick the right concrete alias per assertion. This is work, but mechanical.
- **Canary cosine tolerance** is pinned at 0.999 but the first real run may need to adjust. Acceptance criterion 5 allows > 0.9999; if the observed drift is worse than that on real BGE, the tolerance is relaxed *once*, the observed value is recorded in a code comment, and the change is explained in the commit message. No runtime configurability of the tolerance.

## What this spec deliberately leaves alone

Every one of these is a legitimate concern that belongs to a later step:

- Nomic, arctic, embedding-gemma presets — Step 2.
- fastembed-rs as a backend — Step 2.
- Reranker — Step 3.
- Tantivy / BM25 / hybrid retrieval — Step 4.
- Contextual Retrieval — Step 5.
- Eval harness refresh + gold set — Step 6.
- Corpus hygiene filters — Step 7.

Attempting any of them inside Step 1 is scope creep and will be rejected in review.
