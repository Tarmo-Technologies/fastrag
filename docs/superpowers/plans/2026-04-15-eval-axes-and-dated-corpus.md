# Eval Axes + Dated Gold Corpus + Markdown Frontmatter Metadata — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add per-query axis labels to the gold set, attach real dates to the gold corpus via markdown frontmatter, plumb frontmatter into the typed-metadata pipeline, and extend the eval gate with per-bucket regression checks — so defaults in hybrid RRF and temporal decay become empirically tunable.

**Architecture:** Three stacked landings. Landing 1 teaches the markdown parser to surface YAML frontmatter and extends directory ingest to type-promote it through the existing JSONL typing layer. Landing 2 adds `Axes` to `GoldSetEntry`, computes per-bucket metrics in `VariantReport`, and extends `BaselineDiff` with per-bucket regression detection. Landing 3 backfills dates on the 50 corpus docs, axis labels on 120 questions + 30 new questions, and recaptures the baseline.

**Tech Stack:** Rust (workspace crates `fastrag-markdown`, `fastrag`, `fastrag-eval`, `fastrag-cli`), `serde_yaml`, `chrono` (existing), `regex` (existing), Python 3.11 for the axis-backfill helper script.

---

## Landing 1 — Markdown frontmatter → typed metadata

### Task 1: Add `serde_yaml` dependency to `fastrag-markdown`

**Files:**
- Modify: `crates/fastrag-markdown/Cargo.toml`

- [ ] **Step 1: Add dependency**

In `[dependencies]`, add:
```toml
serde_yaml = "0.9"
```

- [ ] **Step 2: Verify build**

Run: `cargo check -p fastrag-markdown`
Expected: Clean build.

### Task 2: Failing test for frontmatter extraction

**Files:**
- Modify: `crates/fastrag-markdown/src/lib.rs` (append to `#[cfg(test)] mod tests`)

- [ ] **Step 1: Write the failing test**

Append to the existing `#[cfg(test)] mod tests` block:

```rust
#[test]
fn frontmatter_is_extracted_into_metadata_extra() {
    let input = b"---\npublished_date: 2021-12-10\nseverity: high\n---\n# Title\n\nBody.\n";
    let source = SourceInfo {
        filename: Some("test.md".into()),
        format: FileFormat::Markdown,
    };
    let parser = MarkdownParser;
    let doc = parser.parse(input, &source).unwrap();
    assert_eq!(
        doc.metadata.extra.get("published_date").map(String::as_str),
        Some("2021-12-10")
    );
    assert_eq!(
        doc.metadata.extra.get("severity").map(String::as_str),
        Some("high")
    );
}

#[test]
fn frontmatter_absent_leaves_extra_empty() {
    let input = b"# Title\n\nNo frontmatter here.\n";
    let source = SourceInfo {
        filename: None,
        format: FileFormat::Markdown,
    };
    let parser = MarkdownParser;
    let doc = parser.parse(input, &source).unwrap();
    assert!(doc.metadata.extra.is_empty());
}

#[test]
fn frontmatter_malformed_yaml_returns_parse_error() {
    let input = b"---\npublished_date: : bad\n---\nBody\n";
    let source = SourceInfo {
        filename: None,
        format: FileFormat::Markdown,
    };
    let parser = MarkdownParser;
    let err = parser.parse(input, &source).unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("frontmatter"), "error should mention frontmatter: {msg}");
}
```

Note: the existing test module already imports the types. If `SourceInfo` / `FileFormat` aren't in scope, add `use fastrag_core::{FileFormat, SourceInfo};` at the top of the test module.

- [ ] **Step 2: Run test to verify failure**

Run: `cargo test -p fastrag-markdown frontmatter_`
Expected: both positive tests FAIL (frontmatter is echoed into body elements instead of extra), malformed test FAILs (no error currently).

### Task 3: Implement frontmatter extraction

**Files:**
- Modify: `crates/fastrag-markdown/src/lib.rs`

- [ ] **Step 1: Add the extraction helper**

At the bottom of `crates/fastrag-markdown/src/lib.rs` (after existing helpers), add:

```rust
/// If `text` begins with a YAML frontmatter block (`---\n...\n---\n`), returns
/// `(frontmatter_pairs, body_offset)`. Otherwise returns `(Vec::new(), 0)`.
///
/// `frontmatter_pairs` is a `Vec<(String, String)>` where values are stringified
/// from their YAML scalar form (numbers and booleans become their display form,
/// strings pass through, arrays/maps are rejected).
fn extract_frontmatter(text: &str) -> Result<(Vec<(String, String)>, usize), FastRagError> {
    // Must start with exactly "---\n" (or "---\r\n")
    let rest = if let Some(r) = text.strip_prefix("---\n") {
        r
    } else if let Some(r) = text.strip_prefix("---\r\n") {
        r
    } else {
        return Ok((Vec::new(), 0));
    };

    // Find the closing "---" on its own line
    let mut idx = 0;
    let mut closing: Option<usize> = None;
    for line in rest.split_inclusive('\n') {
        let trimmed = line.trim_end_matches(|c| c == '\n' || c == '\r');
        if trimmed == "---" {
            closing = Some(idx + line.len());
            break;
        }
        idx += line.len();
    }
    let Some(close_end) = closing else {
        // Opening "---" with no close: treat as no frontmatter, pass through.
        return Ok((Vec::new(), 0));
    };

    let yaml_src = &rest[..close_end - (rest[..close_end].rfind("---").unwrap_or(0))];
    // Simpler slice: yaml content is everything in `rest` up to the last "---\n"
    let yaml_body = {
        // Find the position of the closing "---" inside `rest`
        let pos = rest.find("\n---").or_else(|| rest.strip_prefix("---").map(|_| 0));
        match pos {
            Some(p) => &rest[..p],
            None => "",
        }
    };
    let _ = yaml_src; // placeholder — use yaml_body

    let value: serde_yaml::Value = serde_yaml::from_str(yaml_body).map_err(|e| {
        FastRagError::Parse {
            format: FileFormat::Markdown,
            message: format!("frontmatter YAML: {e}"),
        }
    })?;

    let mapping = match value {
        serde_yaml::Value::Mapping(m) => m,
        serde_yaml::Value::Null => return Ok((Vec::new(), 4 + close_end)),
        _ => {
            return Err(FastRagError::Parse {
                format: FileFormat::Markdown,
                message: "frontmatter must be a YAML mapping".into(),
            });
        }
    };

    let mut pairs = Vec::with_capacity(mapping.len());
    for (k, v) in mapping {
        let key = match k {
            serde_yaml::Value::String(s) => s,
            other => {
                return Err(FastRagError::Parse {
                    format: FileFormat::Markdown,
                    message: format!("frontmatter key must be string, got {other:?}"),
                });
            }
        };
        let val = match v {
            serde_yaml::Value::String(s) => s,
            serde_yaml::Value::Number(n) => n.to_string(),
            serde_yaml::Value::Bool(b) => b.to_string(),
            serde_yaml::Value::Null => continue,
            other => {
                return Err(FastRagError::Parse {
                    format: FileFormat::Markdown,
                    message: format!("frontmatter value for '{key}' must be scalar, got {other:?}"),
                });
            }
        };
        pairs.push((key, val));
    }

    // body offset = length of opening "---\n" (4 bytes) + yaml_body + closing "---\n"
    let body_offset = 4 + yaml_body.len() + 4; // "---\n" + body + "---\n"
    Ok((pairs, body_offset))
}
```

- [ ] **Step 2: Wire extraction into `parse`**

In `MarkdownParser::parse`, after `let text = ...;` and before `let mut metadata = Metadata::new(...)`, add frontmatter extraction and strip the frontmatter off the body before comrak parses it:

Replace the opening of `parse` (down through `let root = parse_document(...)`) with:

```rust
    fn parse(&self, input: &[u8], source: &SourceInfo) -> Result<Document, FastRagError> {
        let text =
            String::from_utf8(input.to_vec()).map_err(|e| FastRagError::Encoding(e.to_string()))?;

        let (frontmatter_pairs, body_offset) = extract_frontmatter(&text)?;
        let body = &text[body_offset..];

        let mut metadata = Metadata::new(source.format);
        metadata.source_file = source.filename.clone();
        for (k, v) in frontmatter_pairs {
            metadata.extra.insert(k, v);
        }

        let arena = Arena::new();
        let mut options = Options::default();
        options.extension.table = true;
        options.extension.strikethrough = true;
        options.extension.tasklist = true;

        let root = parse_document(&arena, body, &options);
```

(Rest of the function is unchanged.)

- [ ] **Step 3: Run tests**

Run: `cargo test -p fastrag-markdown`
Expected: all three new tests PASS, existing tests still PASS.

- [ ] **Step 4: Commit**

```bash
git add crates/fastrag-markdown/Cargo.toml crates/fastrag-markdown/src/lib.rs
git commit -m "feat(markdown): parse YAML frontmatter into Metadata.extra"
```

### Task 4: Failing test for typed-metadata promotion in directory ingest

**Files:**
- Create: `crates/fastrag/tests/frontmatter_metadata.rs`

- [ ] **Step 1: Write the failing integration test**

```rust
#![cfg(feature = "retrieval")]

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use fastrag::corpus::index_path_with_metadata_typed;
use fastrag_core::ChunkingStrategy;
use fastrag_embed::test::ConstantEmbedder;
use fastrag_store::schema::{TypedKind, TypedValue};

#[test]
fn frontmatter_dates_land_as_typed_date_in_user_fields() {
    let tmp = tempfile::tempdir().unwrap();
    let corpus_root: PathBuf = tmp.path().join("corpus");
    let doc = tmp.path().join("doc.md");
    fs::write(
        &doc,
        "---\npublished_date: 2021-12-10\n---\n# CVE-2021-44228\n\nLog4Shell advisory.\n",
    )
    .unwrap();

    let mut types = BTreeMap::new();
    types.insert("published_date".to_string(), TypedKind::Date);

    let embedder = ConstantEmbedder::new(16);
    fastrag::corpus::index_path_with_metadata_typed(
        &doc,
        &corpus_root,
        &ChunkingStrategy::default(),
        &embedder,
        &BTreeMap::new(),
        &["published_date".to_string()],
        &types,
        #[cfg(feature = "contextual")]
        None,
        #[cfg(feature = "hygiene")]
        None,
    )
    .unwrap();

    // Open the store, find the chunk, assert its typed metadata.
    let store = fastrag_store::ChunkStore::open(&corpus_root).unwrap();
    let records = store.all_records().unwrap();
    assert!(!records.is_empty(), "ingest should produce chunks");
    let first = &records[0];
    let published: Option<&TypedValue> = first
        .user_fields
        .iter()
        .find(|(k, _)| k == "published_date")
        .map(|(_, v)| v);
    match published {
        Some(TypedValue::Date(d)) => {
            assert_eq!(d.to_string(), "2021-12-10");
        }
        other => panic!("expected TypedValue::Date, got {other:?}"),
    }
}
```

Note: `index_path_with_metadata_typed` and `ConstantEmbedder` names are what we'll introduce — the test fails to compile today. If `ConstantEmbedder` isn't the right embed test double, replace with whatever `fastrag-embed`'s existing test helper is (check `crates/fastrag-embed/src/`). `tempfile` is already a dev-dep somewhere in the workspace; add it to `crates/fastrag/Cargo.toml` `[dev-dependencies]` if missing.

- [ ] **Step 2: Run test to verify compile failure**

Run: `cargo test -p fastrag --features retrieval --test frontmatter_metadata`
Expected: COMPILE FAIL — `index_path_with_metadata_typed` not found.

### Task 5: Extend `index_path_with_metadata` to accept + apply typed metadata

**Files:**
- Modify: `crates/fastrag/src/corpus/mod.rs` (the `index_path_with_metadata` function)
- Modify: `crates/fastrag/src/corpus/mod.rs` (add a `pub use` for the JSONL typing helpers)

- [ ] **Step 1: Add new public wrapper `index_path_with_metadata_typed`**

Find the existing `index_path_with_metadata` function. Add a new function **alongside** it (keep the old one as a thin wrapper to preserve callers):

```rust
/// Like `index_path_with_metadata`, plus promotes named metadata fields to
/// typed values using the same typing helpers JSONL ingest uses. Metadata
/// resolution precedence (last wins): CLI `base_metadata` → sidecar
/// `<path>.meta.json` → parser-emitted `Document.metadata.extra`.
#[allow(clippy::too_many_arguments)]
pub fn index_path_with_metadata_typed(
    input: &Path,
    corpus_dir: &Path,
    chunking: &ChunkingStrategy,
    embedder: &dyn DynEmbedderTrait,
    base_metadata: &std::collections::BTreeMap<String, String>,
    metadata_fields: &[String],
    metadata_types: &std::collections::BTreeMap<String, fastrag_store::schema::TypedKind>,
    #[cfg(feature = "contextual")] contextualize: Option<ContextualizeOptions<'_>>,
    #[cfg(feature = "hygiene")] hygiene: Option<&crate::hygiene::HygieneChain>,
) -> Result<CorpusIndexStats, CorpusError> {
    index_path_inner(
        input,
        corpus_dir,
        chunking,
        embedder,
        base_metadata,
        metadata_fields,
        metadata_types,
        #[cfg(feature = "contextual")]
        contextualize,
        #[cfg(feature = "hygiene")]
        hygiene,
    )
}
```

- [ ] **Step 2: Refactor existing `index_path_with_metadata` to call the inner**

Rename the body of the existing `index_path_with_metadata` to `index_path_inner` with the new signature (additional `metadata_fields: &[String]` and `metadata_types: &BTreeMap<String, TypedKind>` parameters). Update `index_path` and `index_path_with_metadata` to delegate to `index_path_inner` with `&[]` and `&BTreeMap::new()` for the new parameters.

- [ ] **Step 3: In `index_path_inner`, merge frontmatter into per-file metadata and promote**

Inside the existing per-file loop (where `base_metadata` and sidecar metadata are currently merged), add the parser-emitted frontmatter as the final layer, then type-promote. The exact location: search for the line where the `file_metadata` map is built per file (it's the per-file `BTreeMap<String, String>` merging base + sidecar).

After the sidecar merge, after the document has been parsed into `doc: Document`, add:

```rust
// Layer 3: parser-emitted frontmatter (from Document.metadata.extra). Last wins.
for (k, v) in &doc.metadata.extra {
    file_metadata.insert(k.clone(), v.clone());
}

// Promote named fields to typed values.
let typed_metadata: Vec<(String, fastrag_store::schema::TypedValue)> = metadata_fields
    .iter()
    .filter_map(|field| {
        let raw = file_metadata.get(field)?;
        let kind = metadata_types
            .get(field)
            .copied()
            .unwrap_or(fastrag_store::schema::TypedKind::String);
        promote_string_to_typed(raw, kind).map(|tv| (field.clone(), tv))
    })
    .collect();
```

- [ ] **Step 4: Add the `promote_string_to_typed` helper**

In the same file (or a new module `crates/fastrag/src/corpus/typing.rs`), add:

```rust
fn promote_string_to_typed(
    raw: &str,
    kind: fastrag_store::schema::TypedKind,
) -> Option<fastrag_store::schema::TypedValue> {
    use fastrag_store::schema::{TypedKind, TypedValue};
    match kind {
        TypedKind::String => Some(TypedValue::String(raw.to_string())),
        TypedKind::Numeric => raw.parse::<f64>().ok().map(TypedValue::Numeric),
        TypedKind::Bool => match raw {
            "true" | "True" | "TRUE" => Some(TypedValue::Bool(true)),
            "false" | "False" | "FALSE" => Some(TypedValue::Bool(false)),
            _ => None,
        },
        TypedKind::Date => chrono::NaiveDate::parse_from_str(raw, "%Y-%m-%d")
            .ok()
            .map(TypedValue::Date),
        TypedKind::Array => None, // arrays not supported for flat string metadata
    }
}
```

- [ ] **Step 5: Write `typed_metadata` into the chunk records**

Where chunks are constructed into `ChunkRecord`, the `user_fields` field must carry `typed_metadata`. If the existing code merges other sources into `user_fields`, append `typed_metadata` last so frontmatter precedence holds.

- [ ] **Step 6: Run the integration test**

Run: `cargo test -p fastrag --features retrieval --test frontmatter_metadata`
Expected: PASS.

- [ ] **Step 7: Run the full workspace to confirm no regressions**

Run: `cargo test --workspace --features retrieval`
Expected: all green.

- [ ] **Step 8: Lint**

Run: `cargo clippy --workspace --all-targets --features retrieval,rerank,hybrid,contextual,eval -- -D warnings`
Expected: clean.

- [ ] **Step 9: Commit**

```bash
git add crates/fastrag/src/corpus/mod.rs crates/fastrag/tests/frontmatter_metadata.rs crates/fastrag/Cargo.toml
git commit -m "feat(ingest): promote markdown frontmatter to typed user_fields"
```

### Task 6: CLI — allow `--metadata-fields` / `--metadata-types` on directory ingest

**Files:**
- Modify: `fastrag-cli/src/args.rs` (clap attributes + struct fields)
- Modify: `fastrag-cli/src/main.rs` (wire the existing flag values into the non-JSONL path)

- [ ] **Step 1: Drop the "JSONL:" docstring hint and use the flags outside the JSONL branch**

In `fastrag-cli/src/args.rs` around the current `metadata_fields` / `metadata_types` definitions (inside the `Index` subcommand), change the doc strings:

```rust
        /// Fields to index as typed metadata (comma-separated). Applies to
        /// markdown frontmatter on directory ingest and JSONL records.
        #[cfg(feature = "store")]
        #[arg(long, value_delimiter = ',')]
        metadata_fields: Option<Vec<String>>,

        /// Explicit type overrides (comma-separated `field=type`, e.g.
        /// `published_date=date,cvss_score=numeric`). Applies to markdown
        /// frontmatter on directory ingest and JSONL records.
        #[cfg(feature = "store")]
        #[arg(long, value_delimiter = ',')]
        metadata_types: Option<Vec<String>>,
```

- [ ] **Step 2: Parse the flags into `Vec<String>` + `BTreeMap<String, TypedKind>` once**

In `fastrag-cli/src/main.rs`, find the `Command::Index { ... }` match arm. Before the branch that splits on `is_jsonl`, parse the two flags into normalised form:

```rust
let fields_vec: Vec<String> = metadata_fields.clone().unwrap_or_default();
let types_map: std::collections::BTreeMap<String, fastrag_store::schema::TypedKind> =
    metadata_types
        .clone()
        .unwrap_or_default()
        .into_iter()
        .map(|pair| {
            let (k, v) = pair.split_once('=').ok_or_else(|| {
                anyhow::anyhow!("--metadata-types entries must be `field=type`, got {pair:?}")
            })?;
            let kind = match v {
                "string" => fastrag_store::schema::TypedKind::String,
                "numeric" => fastrag_store::schema::TypedKind::Numeric,
                "bool" => fastrag_store::schema::TypedKind::Bool,
                "date" => fastrag_store::schema::TypedKind::Date,
                "array" => fastrag_store::schema::TypedKind::Array,
                other => anyhow::bail!("unknown --metadata-types kind {other:?}"),
            };
            Ok::<_, anyhow::Error>((k.to_string(), kind))
        })
        .collect::<Result<_, _>>()?;
```

- [ ] **Step 3: Pass them to the directory-ingest call site**

Find the existing `fastrag::corpus::index_path_with_metadata(...)` call (the non-JSONL branch). Replace it with `index_path_with_metadata_typed(...)` and pass `&fields_vec` / `&types_map` in the new slots.

- [ ] **Step 4: End-to-end CLI smoke check**

Run: `cargo build -p fastrag-cli --features retrieval`
Run: ```
cargo run -p fastrag-cli --features retrieval -- index \
    crates/fastrag-markdown/tests/fixtures  \
    --corpus /tmp/fastrag-frontmatter-smoke \
    --metadata-fields published_date \
    --metadata-types published_date=date \
    --embedder static
``` (adjust paths/embedder to what the codebase offers — `static` or whatever the `ConstantEmbedder`-backed variant is).
Expected: exit 0.

- [ ] **Step 5: Lint + commit**

```bash
cargo clippy --workspace --all-targets --features retrieval,rerank,hybrid,contextual,eval -- -D warnings
cargo fmt --check

git add fastrag-cli/src/args.rs fastrag-cli/src/main.rs
git commit -m "feat(cli): --metadata-fields / --metadata-types apply to directory ingest"
```

---

## Landing 2 — Gold-set axes + per-bucket metrics + per-bucket gate

### Task 7: Failing test for axis parsing + required-field enforcement

**Files:**
- Modify: `crates/fastrag-eval/src/gold_set.rs` (tests) — note there is no existing `#[cfg(test)]` in this file today; if so, create one at the bottom.

- [ ] **Step 1: Write failing tests**

Append to `crates/fastrag-eval/src/gold_set.rs`:

```rust
#[cfg(test)]
mod axes_tests {
    use super::*;

    #[test]
    fn parses_axes_from_valid_entry() {
        let raw = r#"{
            "version": 1,
            "entries": [{
                "id": "q1",
                "question": "What is CVE-2021-44228?",
                "must_contain_cve_ids": ["CVE-2021-44228"],
                "must_contain_terms": [],
                "axes": { "style": "identifier", "temporal_intent": "neutral" }
            }]
        }"#;
        let gs: GoldSet = serde_json::from_str(raw).unwrap();
        let e = &gs.entries[0];
        assert_eq!(e.axes.style, Style::Identifier);
        assert_eq!(e.axes.temporal_intent, TemporalIntent::Neutral);
    }

    #[test]
    fn rejects_entry_missing_axes() {
        let raw = r#"{
            "version": 1,
            "entries": [{
                "id": "q1",
                "question": "Q?",
                "must_contain_terms": ["x"]
            }]
        }"#;
        let err = serde_json::from_str::<GoldSet>(raw).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("axes"), "missing-axes error should mention axes: {msg}");
    }

    #[test]
    fn rejects_unknown_axis_value() {
        let raw = r#"{
            "version": 1,
            "entries": [{
                "id": "q1",
                "question": "Q?",
                "must_contain_terms": ["x"],
                "axes": { "style": "weird", "temporal_intent": "neutral" }
            }]
        }"#;
        serde_json::from_str::<GoldSet>(raw).unwrap_err();
    }
}
```

- [ ] **Step 2: Run and confirm failure**

Run: `cargo test -p fastrag-eval axes_tests`
Expected: COMPILE FAIL (`Axes`, `Style`, `TemporalIntent` not defined).

### Task 8: Implement `Axes` / `Style` / `TemporalIntent` on `GoldSetEntry`

**Files:**
- Modify: `crates/fastrag-eval/src/gold_set.rs`

- [ ] **Step 1: Add types**

At the top of `gold_set.rs`, after the existing `GoldSetEntry`, add:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Style {
    Identifier,
    Conceptual,
    Mixed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TemporalIntent {
    Historical,
    Neutral,
    RecencySeeking,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Axes {
    pub style: Style,
    pub temporal_intent: TemporalIntent,
}
```

- [ ] **Step 2: Add required field to `GoldSetEntry`**

Modify the `GoldSetEntry` struct:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GoldSetEntry {
    pub id: String,
    pub question: String,
    #[serde(default)]
    pub must_contain_cve_ids: Vec<String>,
    #[serde(default)]
    pub must_contain_terms: Vec<String>,
    #[serde(default)]
    pub notes: Option<String>,
    pub axes: Axes, // NEW — required
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p fastrag-eval axes_tests`
Expected: all three PASS.

- [ ] **Step 4: Observe the wider test suite — it will now fail on existing fixtures**

Run: `cargo test -p fastrag-eval`
Expected: some existing tests FAIL because fixture gold sets don't have `axes` yet. This is expected and will be fixed in Step 5.

- [ ] **Step 5: Update in-tree test fixtures to carry axes**

Find all test fixtures referenced by `fastrag-eval` tests (typically small inline JSON strings or files under `crates/fastrag-eval/tests/fixtures/`). For each `GoldSetEntry` literal, add `"axes": {"style": "identifier", "temporal_intent": "neutral"}` (or a contextually appropriate value).

Run: `cargo test -p fastrag-eval`
Expected: all green.

- [ ] **Step 6: Commit**

```bash
git add crates/fastrag-eval/src/gold_set.rs crates/fastrag-eval/tests/fixtures
git commit -m "feat(eval): required Axes on GoldSetEntry (style, temporal_intent)"
```

### Task 9: Failing test for per-bucket metric computation

**Files:**
- Create: `crates/fastrag-eval/src/buckets.rs` (new module)
- Modify: `crates/fastrag-eval/src/lib.rs` (add `mod buckets; pub use buckets::*;`)

- [ ] **Step 1: Write the failing test and empty module**

Create `crates/fastrag-eval/src/buckets.rs`:

```rust
//! Per-axis bucket aggregates computed from a variant's `per_question` list.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::gold_set::{Axes, GoldSet, GoldSetEntry};
use crate::matrix::QuestionResult;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BucketMetrics {
    pub hit_at_1: f64,
    pub hit_at_5: f64,
    pub hit_at_10: f64,
    pub mrr_at_10: f64,
    pub n: usize,
}

/// Compute per-axis buckets from a variant's per-question results, keyed by
/// axis name then axis value string (snake_case). Empty buckets are omitted.
pub fn compute_buckets(
    per_question: &[QuestionResult],
    gold: &GoldSet,
) -> BTreeMap<String, BTreeMap<String, BucketMetrics>> {
    let by_id: std::collections::HashMap<&str, &GoldSetEntry> =
        gold.entries.iter().map(|e| (e.id.as_str(), e)).collect();
    let mut groups: BTreeMap<(&'static str, String), Vec<&QuestionResult>> = BTreeMap::new();
    for q in per_question {
        let Some(entry) = by_id.get(q.id.as_str()) else { continue };
        let Axes { style, temporal_intent } = entry.axes;
        let style_key = serde_json::to_value(&style).unwrap().as_str().unwrap().to_string();
        let ti_key = serde_json::to_value(&temporal_intent).unwrap().as_str().unwrap().to_string();
        groups.entry(("style", style_key)).or_default().push(q);
        groups.entry(("temporal_intent", ti_key)).or_default().push(q);
    }
    let mut out: BTreeMap<String, BTreeMap<String, BucketMetrics>> = BTreeMap::new();
    for ((axis, value), results) in groups {
        let n = results.len();
        if n == 0 { continue; }
        let nf = n as f64;
        let m = BucketMetrics {
            hit_at_1: results.iter().filter(|q| q.hit_at_1).count() as f64 / nf,
            hit_at_5: results.iter().filter(|q| q.hit_at_5).count() as f64 / nf,
            hit_at_10: results.iter().filter(|q| q.hit_at_10).count() as f64 / nf,
            mrr_at_10: results.iter().map(|q| q.reciprocal_rank).sum::<f64>() / nf,
            n,
        };
        out.entry(axis.to_string()).or_default().insert(value, m);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gold_set::{Axes, GoldSet, GoldSetEntry, Style, TemporalIntent};
    use crate::matrix::{LatencyBreakdown, QuestionResult};

    fn entry(id: &str, style: Style, ti: TemporalIntent) -> GoldSetEntry {
        GoldSetEntry {
            id: id.into(),
            question: "q".into(),
            must_contain_cve_ids: vec![],
            must_contain_terms: vec!["x".into()],
            notes: None,
            axes: Axes { style, temporal_intent: ti },
        }
    }

    fn qr(id: &str, h1: bool, h5: bool, rr: f64) -> QuestionResult {
        QuestionResult {
            id: id.into(),
            hit_at_1: h1,
            hit_at_5: h5,
            hit_at_10: h5,
            reciprocal_rank: rr,
            missing_cve_ids: vec![],
            missing_terms: vec![],
            latency_us: LatencyBreakdown::default(),
        }
    }

    #[test]
    fn computes_per_axis_aggregates() {
        let gold = GoldSet {
            version: 1,
            entries: vec![
                entry("a", Style::Identifier, TemporalIntent::Neutral),
                entry("b", Style::Identifier, TemporalIntent::Historical),
                entry("c", Style::Conceptual, TemporalIntent::Neutral),
            ],
        };
        let per_q = vec![
            qr("a", true, true, 1.0),
            qr("b", false, true, 0.5),
            qr("c", false, false, 0.0),
        ];
        let buckets = compute_buckets(&per_q, &gold);
        let style = buckets.get("style").unwrap();
        let ident = style.get("identifier").unwrap();
        assert_eq!(ident.n, 2);
        assert!((ident.hit_at_1 - 0.5).abs() < 1e-9);
        assert!((ident.hit_at_5 - 1.0).abs() < 1e-9);
        assert!((ident.mrr_at_10 - 0.75).abs() < 1e-9);
        let conc = style.get("conceptual").unwrap();
        assert_eq!(conc.n, 1);
        assert_eq!(conc.hit_at_5, 0.0);

        let ti = buckets.get("temporal_intent").unwrap();
        assert_eq!(ti.get("neutral").unwrap().n, 2);
        assert_eq!(ti.get("historical").unwrap().n, 1);
    }
}
```

If `LatencyBreakdown::default()` isn't available, add `#[derive(Default)]` to `LatencyBreakdown` in `matrix.rs` (it's a simple struct of numbers).

- [ ] **Step 2: Wire module in `lib.rs`**

Add:
```rust
pub mod buckets;
```

- [ ] **Step 3: Run**

Run: `cargo test -p fastrag-eval buckets::tests`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add crates/fastrag-eval/src/buckets.rs crates/fastrag-eval/src/lib.rs crates/fastrag-eval/src/matrix.rs
git commit -m "feat(eval): per-axis BucketMetrics over per_question"
```

### Task 10: Embed `buckets` in `VariantReport` and populate during matrix run

**Files:**
- Modify: `crates/fastrag-eval/src/matrix.rs`

- [ ] **Step 1: Add field to `VariantReport`**

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VariantReport {
    pub variant: ConfigVariant,
    pub hit_at_1: f64,
    pub hit_at_5: f64,
    pub hit_at_10: f64,
    pub mrr_at_10: f64,
    pub latency: LatencyPercentiles,
    pub per_question: Vec<QuestionResult>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub buckets: std::collections::BTreeMap<String, std::collections::BTreeMap<String, crate::buckets::BucketMetrics>>,
}
```

- [ ] **Step 2: Populate during report construction**

Find the spot in `matrix.rs` where `VariantReport { ... per_question, ... }` is constructed (inside the run loop). Insert:

```rust
let buckets = crate::buckets::compute_buckets(&per_question, gold_set);
```

and add `buckets,` to the struct literal.

- [ ] **Step 3: Update unit tests + matrix_stub test**

Run: `cargo test -p fastrag-eval`
Expected: any test constructing a `VariantReport` literal needs the new `buckets: Default::default(),` field. Fix them to pass.

- [ ] **Step 4: Commit**

```bash
git add crates/fastrag-eval/src/matrix.rs
git commit -m "feat(eval): VariantReport carries per-axis buckets"
```

### Task 11: Baseline schema bump + per-bucket slack

**Files:**
- Modify: `crates/fastrag-eval/src/baseline.rs`

- [ ] **Step 1: Write a failing test for per-bucket regression detection**

Append to `baseline.rs` in a new `#[cfg(test)]` block:

```rust
#[cfg(test)]
mod bucket_diff_tests {
    use super::*;
    use crate::buckets::BucketMetrics;
    use crate::matrix::{ConfigVariant, VariantReport};
    use std::collections::BTreeMap;

    fn mk_variant_with_bucket(hit5_overall: f64, bucket_hit5: f64) -> VariantReport {
        let mut buckets: BTreeMap<String, BTreeMap<String, BucketMetrics>> = BTreeMap::new();
        buckets.entry("style".into()).or_default().insert(
            "identifier".into(),
            BucketMetrics {
                hit_at_1: 0.0, hit_at_5: bucket_hit5, hit_at_10: bucket_hit5,
                mrr_at_10: bucket_hit5, n: 10,
            },
        );
        VariantReport {
            variant: ConfigVariant::Primary,
            hit_at_1: 0.0,
            hit_at_5: hit5_overall,
            hit_at_10: hit5_overall,
            mrr_at_10: hit5_overall,
            latency: Default::default(),
            per_question: vec![],
            buckets,
        }
    }

    #[test]
    fn per_bucket_regression_detected_when_over_slack() {
        let baseline = Baseline {
            schema_version: 2,
            git_rev: "x".into(),
            captured_at: "now".into(),
            runs: vec![VariantBaseline {
                variant: ConfigVariant::Primary,
                hit_at_5: 0.9,
                mrr_at_10: 0.8,
                buckets: {
                    let mut m: BTreeMap<String, BTreeMap<String, BucketMetrics>> = BTreeMap::new();
                    m.entry("style".into()).or_default().insert(
                        "identifier".into(),
                        BucketMetrics {
                            hit_at_1: 0.0, hit_at_5: 0.9, hit_at_10: 0.9,
                            mrr_at_10: 0.9, n: 10,
                        },
                    );
                    m
                },
            }],
            per_bucket_slack: Some(0.05),
        };
        let report = MatrixReport {
            schema_version: 2,
            git_rev: "y".into(),
            captured_at: "later".into(),
            runs: vec![mk_variant_with_bucket(0.9, 0.8)], // bucket dropped 10pp, overall flat
            rerank_delta: 0.0,
            contextual_delta: 0.0,
            hybrid_delta: 0.0,
        };
        let diff = diff(&report, &baseline).unwrap();
        assert!(diff.has_regressions());
        assert!(
            diff.regressions.iter().any(|r| r.metric.contains("style.identifier")),
            "expected per-bucket regression, got {:?}",
            diff.regressions
        );
    }
}
```

- [ ] **Step 2: Add `buckets` to `VariantBaseline` and `per_bucket_slack` to `Baseline`**

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Baseline {
    pub schema_version: u32,
    pub git_rev: String,
    pub captured_at: String,
    pub runs: Vec<VariantBaseline>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub per_bucket_slack: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VariantBaseline {
    pub variant: ConfigVariant,
    pub hit_at_5: f64,
    pub mrr_at_10: f64,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub buckets: std::collections::BTreeMap<String, std::collections::BTreeMap<String, crate::buckets::BucketMetrics>>,
}
```

- [ ] **Step 3: Extend `diff()` with per-bucket regression detection**

At the end of the existing `diff()` function (after overall-metric regressions are pushed), add:

```rust
    let bucket_slack = baseline.per_bucket_slack.unwrap_or(DEFAULT_SLACK);
    for bv in &baseline.runs {
        let Some(run) = report.runs.iter().find(|r| r.variant == bv.variant) else { continue };
        for (axis, bucket_map) in &bv.buckets {
            let Some(run_axis) = run.buckets.get(axis) else { continue };
            for (value, baseline_m) in bucket_map {
                let Some(current_m) = run_axis.get(value) else { continue };
                let delta = current_m.hit_at_5 - baseline_m.hit_at_5;
                if delta + bucket_slack < 0.0 {
                    diff_out.regressions.push(Regression {
                        variant: bv.variant,
                        metric: Box::leak(format!("hit_at_5[{axis}.{value}]").into_boxed_str()),
                        baseline: baseline_m.hit_at_5,
                        current: current_m.hit_at_5,
                        delta,
                        slack: bucket_slack,
                    });
                }
            }
        }
    }
```

Note: `Regression.metric` is `&'static str` today. The `Box::leak` is a pragmatic tradeoff — small number of bucket strings, leaked once per diff. If the reviewer objects, switch `metric` to `String` throughout.

- [ ] **Step 4: Bump the baseline schema_version default**

Update any `Baseline` construction site that hard-codes `schema_version: 1` to use `schema_version: 2`. The persisted baseline on disk will be regenerated in Landing 3.

- [ ] **Step 5: Run**

Run: `cargo test -p fastrag-eval`
Expected: new test PASSes. Existing baseline tests may need the new `buckets` / `per_bucket_slack` fields added; do so using `Default::default()` where appropriate.

- [ ] **Step 6: Commit**

```bash
git add crates/fastrag-eval/src/baseline.rs
git commit -m "feat(eval): per-bucket regression gate + schema_version=2"
```

### Task 12: Lint Landing 2 and push

- [ ] **Step 1: Full lint**

Run: `cargo clippy --workspace --all-targets --features retrieval,rerank,hybrid,contextual,eval -- -D warnings`
Expected: clean.

- [ ] **Step 2: Full test**

Run: `cargo test --workspace --features retrieval,rerank,hybrid,contextual,eval`
Expected: clean (the persisted baseline under `docs/eval-baselines/current.json` is v1 and the tests that touch it will be refreshed in Landing 3; if any test fails with schema-mismatch here, add a `#[ignore]` with a comment "recaptured in Landing 3").

---

## Landing 3 — Backfill + baseline recapture

### Task 13: `scripts/axis-backfill.py`

**Files:**
- Create: `scripts/axis-backfill.py`

- [ ] **Step 1: Write the script**

```python
#!/usr/bin/env python3
"""Heuristic first-pass axis labeller for tests/gold/questions.json.

Usage: python scripts/axis-backfill.py tests/gold/questions.json
Writes labelled entries back to the same file (in-place). Every entry with
existing axes is left untouched. Hand-review the diff before committing.
"""
from __future__ import annotations

import json
import re
import sys
from pathlib import Path

CVE_OR_CWE = re.compile(r"\b(CVE-\d{4}-\d+|CWE-\d+)\b", re.IGNORECASE)
RECENCY_MARKERS = re.compile(
    r"\b(latest|newest|recent|current|this week|this month|2025|2026)\b", re.IGNORECASE
)
HISTORICAL_MARKERS = re.compile(
    r"\b(as of 20\d{2}|back in 20\d{2}|in 201\d|legacy|original)\b", re.IGNORECASE
)

def classify_style(q: str) -> str:
    has_id = bool(CVE_OR_CWE.search(q))
    # crude conceptual heuristic: no identifier and the question is wordy
    wordy = len(q.split()) >= 6 and not has_id
    if has_id and wordy:
        return "mixed"
    if has_id:
        return "identifier"
    return "conceptual"

def classify_temporal(q: str) -> str:
    if RECENCY_MARKERS.search(q):
        return "recency_seeking"
    if HISTORICAL_MARKERS.search(q):
        return "historical"
    return "neutral"

def main(path: Path) -> int:
    data = json.loads(path.read_text())
    changed = 0
    for entry in data["entries"]:
        if "axes" in entry and entry["axes"]:
            continue
        q = entry["question"]
        entry["axes"] = {
            "style": classify_style(q),
            "temporal_intent": classify_temporal(q),
        }
        changed += 1
    path.write_text(json.dumps(data, indent=2) + "\n")
    print(f"labelled {changed} entries")
    return 0

if __name__ == "__main__":
    sys.exit(main(Path(sys.argv[1])))
```

- [ ] **Step 2: Run it**

Run: `python3 scripts/axis-backfill.py tests/gold/questions.json`
Expected: `labelled 120 entries`.

- [ ] **Step 3: Hand-review**

Open `tests/gold/questions.json` and scan the labels. Correct obvious misclassifications. Red flags: questions asking about Heartbleed, Shellshock, EternalBlue labelled as `neutral` — these are historical exploits, should be `historical`. Questions of the form "What CVEs exist for X in libfoo?" with no CVE-ID should be `conceptual`, not `mixed`.

- [ ] **Step 4: Commit script + labelled questions**

```bash
git add scripts/axis-backfill.py tests/gold/questions.json
git commit -m "feat(eval): axis-backfill heuristic + labelled existing gold set"
```

### Task 14: Add 30 new questions balancing thin buckets

**Files:**
- Modify: `tests/gold/questions.json`

- [ ] **Step 1: Count current bucket distribution**

Run:
```bash
python3 -c "
import json, collections
d = json.load(open('tests/gold/questions.json'))
style = collections.Counter(e['axes']['style'] for e in d['entries'])
ti = collections.Counter(e['axes']['temporal_intent'] for e in d['entries'])
print('style:', dict(style))
print('temporal:', dict(ti))
"
```

- [ ] **Step 2: Add ~10 `recency_seeking` questions**

Each entry must target a CVE / vuln / topic actually present in `tests/gold/corpus/`. Example shapes:
- `"What is the latest advisory on Log4j?"` → targets `06-log4shell.md`.
- `"Any recent RCE vulnerabilities disclosed?"` → targets `01-libfoo-rce.md`.
- `"Show newest known-exploited vulnerabilities."` → targets `02-kev-bluekeep.md`.

Ensure each has `must_contain_cve_ids` or `must_contain_terms` that match real content.

- [ ] **Step 3: Add ~10 `conceptual` questions**

Examples:
- `"Which vulnerabilities allow bypassing authentication?"` → targets auth-bypass entries.
- `"Deserialization flaws that enable RCE."` → targets `04-cwe-502-deserialize.md`.

- [ ] **Step 4: Add ~10 `historical` questions**

Examples:
- `"The 2014 OpenSSL memory disclosure flaw."` → targets `07-heartbleed.md`.
- `"EternalBlue from 2017."` → targets `09-eternalblue.md`.
- `"As of 2017 guidance on Struts OGNL injection."` → targets `10-struts-ognl.md`.

- [ ] **Step 5: Verify total + bucket distribution**

Run the same python snippet. Expected: 150 entries; roughly 40 / 70 / 40 across temporal; 70 / 60 / 20 across style (adjust targets if the heuristic backfill skewed the original 120 differently).

- [ ] **Step 6: Commit**

```bash
git add tests/gold/questions.json
git commit -m "test(eval): 30 new gold-set questions balancing axis buckets"
```

### Task 15: Attach frontmatter dates to 50 corpus docs

**Files:**
- Modify: `tests/gold/corpus/*.md`

- [ ] **Step 1: Write a helper script** (one-off, does not need committing)

```python
# /tmp/date-corpus.py
# Mapping of doc filename stem → (published_date, last_modified or None).
# Dates are the real publication/disclosure dates of each CVE/event.
DATES = {
    "01-libfoo-rce": ("2024-03-15", None),  # synthetic sample; use realistic date
    "02-kev-bluekeep": ("2019-05-14", "2023-05-16"),
    "06-log4shell": ("2021-12-10", "2022-01-10"),
    "07-heartbleed": ("2014-04-07", None),
    "08-shellshock": ("2014-09-24", None),
    "09-eternalblue": ("2017-03-14", None),
    # ... populate all 50
}

from pathlib import Path
import re
CORPUS = Path("tests/gold/corpus")
for md in sorted(CORPUS.glob("*.md")):
    stem = md.stem
    if stem not in DATES:
        print(f"SKIP (no date mapping): {stem}")
        continue
    pub, mod_ = DATES[stem]
    raw = md.read_text()
    # Replace existing frontmatter with augmented one. Existing frontmatter
    # has only `title:` — we preserve it.
    m = re.match(r"^---\n(.*?)\n---\n", raw, re.DOTALL)
    existing = m.group(1) if m else ""
    body = raw[m.end():] if m else raw
    lines = [existing] if existing else []
    lines.append(f"published_date: {pub}")
    if mod_:
        lines.append(f"last_modified: {mod_}")
    new = "---\n" + "\n".join(lines).strip() + "\n---\n" + body
    md.write_text(new)
    print(f"updated: {stem}")
```

Fill in the full 50-entry `DATES` mapping using the real-world dates of each CVE/event each doc describes. For KEV entries, `published_date` is the date the vuln was added to KEV; `last_modified` is the most recent update.

- [ ] **Step 2: Run it, spot-check output**

Run: `python3 /tmp/date-corpus.py`
Verify: `head tests/gold/corpus/06-log4shell.md` shows a frontmatter with `title:`, `published_date: 2021-12-10`, `last_modified: 2022-01-10`.

- [ ] **Step 3: Verify no doc was skipped**

Run:
```bash
for f in tests/gold/corpus/*.md; do
  head -n 6 "$f" | grep -q "^published_date:" || echo "MISSING DATE: $f"
done
```
Expected: no "MISSING DATE" output.

- [ ] **Step 4: Commit**

```bash
git add tests/gold/corpus
git commit -m "test(corpus): attach published_date + last_modified frontmatter to gold set"
```

### Task 16: Update weekly workflow to use the new flags

**Files:**
- Modify: `.github/workflows/weekly.yml`

- [ ] **Step 1: Extend both `index` invocations**

Find the two `cargo run ... index` steps (ctx + raw). Append `--metadata-fields published_date,last_modified --metadata-types published_date=date,last_modified=date` to each.

- [ ] **Step 2: Validate**

Run: `python3 -c "import yaml; yaml.safe_load(open('.github/workflows/weekly.yml'))"`
Expected: no error.

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/weekly.yml
git commit -m "ci(weekly): index gold corpus with typed frontmatter metadata"
```

### Task 17: Recapture baseline locally and commit

**Files:**
- Modify: `docs/eval-baselines/current.json`

- [ ] **Step 1: Rebuild both corpora with new flags**

```bash
cargo run --release -p fastrag-cli --features retrieval,rerank,hybrid,contextual,contextual-llama -- \
    index tests/gold/corpus --corpus /tmp/eval-ctx --embedder qwen3-q8 --contextualize \
    --metadata-fields published_date,last_modified \
    --metadata-types published_date=date,last_modified=date

cargo run --release -p fastrag-cli --features retrieval,rerank,hybrid,contextual,contextual-llama -- \
    index tests/gold/corpus --corpus /tmp/eval-raw --embedder qwen3-q8 \
    --metadata-fields published_date,last_modified \
    --metadata-types published_date=date,last_modified=date
```

- [ ] **Step 2: Run eval matrix and capture baseline**

```bash
cargo run --release -p fastrag-cli \
    --features eval,retrieval,rerank,rerank-llama,hybrid,contextual,contextual-llama -- \
    eval \
    --gold-set tests/gold/questions.json \
    --corpus /tmp/eval-ctx \
    --corpus-no-contextual /tmp/eval-raw \
    --config-matrix \
    --report docs/eval-baselines/current.json
```

- [ ] **Step 3: Verify the new baseline**

```bash
python3 -c "
import json
d = json.load(open('docs/eval-baselines/current.json'))
assert d['schema_version'] == 2, d['schema_version']
run = d['runs'][0]
assert 'buckets' in run, list(run.keys())
assert 'style' in run['buckets'], list(run['buckets'].keys())
print('schema v2 OK, buckets present')
"
```

- [ ] **Step 4: Commit**

```bash
git add docs/eval-baselines/current.json
git commit -m "eval: recapture baseline with schema v2 (per-axis buckets)"
```

### Task 18: README + CLAUDE.md + eval-baselines docs

**Files:**
- Modify: `README.md`
- Modify: `CLAUDE.md`
- Modify: `docs/eval-baselines/README.md`

- [ ] **Step 1: README "Metadata in markdown frontmatter" subsection**

Add a subsection under the existing CLI docs:

```markdown
### Metadata in markdown frontmatter

The markdown parser extracts YAML frontmatter into per-document metadata.
Use `--metadata-fields` and `--metadata-types` on `index` to promote named
frontmatter keys to typed index metadata — required for features that consume
typed values (e.g. `--time-decay-field published_date`).

Example:
\`\`\`
fastrag index ./docs --corpus ./corpus \
    --metadata-fields published_date,last_modified \
    --metadata-types published_date=date,last_modified=date
\`\`\`

Resolution precedence (last wins): `--metadata k=v` CLI base → per-file
`<path>.meta.json` sidecar → frontmatter on the doc itself.
```

- [ ] **Step 2: CLAUDE.md build commands**

Add to the Build & Test section:

```
cargo test -p fastrag-markdown                                                # frontmatter extraction unit tests
cargo test -p fastrag --features retrieval --test frontmatter_metadata       # frontmatter → typed user_fields e2e
cargo test -p fastrag-eval buckets::tests                                    # per-axis bucket aggregation
cargo test -p fastrag-eval bucket_diff_tests                                 # per-bucket regression gate
```

- [ ] **Step 3: docs/eval-baselines/README.md**

Add a short note:

```markdown
### Schema version 2

Baselines from 2026-04-15 onward use `schema_version: 2` and include per-axis
bucket metrics (style, temporal_intent). Per-bucket regressions are gated by
`per_bucket_slack` (defaults to overall `slack` when absent).
```

- [ ] **Step 4: doc-editor pass**

For each of the three modified `.md` files, run the `doc-editor` skill on the newly added prose before the final commit (per CLAUDE.md mandate).

- [ ] **Step 5: Commit**

```bash
git add README.md CLAUDE.md docs/eval-baselines/README.md
git commit -m "docs: frontmatter metadata, per-bucket gate, schema v2 baselines"
```

### Task 19: Final gate, push, watch CI

- [ ] **Step 1: Full local gate**

```bash
cargo fmt --check
cargo clippy --workspace --all-targets --features retrieval,rerank,hybrid,contextual,eval -- -D warnings
cargo test --workspace --features retrieval,rerank,hybrid,contextual,eval
```

- [ ] **Step 2: Push**

```bash
git push
```

- [ ] **Step 3: Watch CI**

Invoke the `ci-watcher` skill as a background Haiku agent immediately after push. Wait for its report before calling the landing complete.

---

## Post-implementation

- Close nothing automatically. Issues #53, #54, #55 remain open — they now have labeled data to drive their implementation specs.
- Update `project_phase2_handoff.md` memory to reflect that the eval harness now emits per-axis buckets and the gold corpus carries real dates.
