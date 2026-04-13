# Corpus Statistics API Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship `GET /stats?corpus=<name>` (#51) returning corpus health metrics — entry counts, field cardinality/ranges, disk usage, embedding model, chunking config, timestamps — computed on request from Tantivy fast-field columns and manifest metadata.

**Architecture:** Four layers: (1) `TantivyStore::field_stats()` scans fast-field columns per segment for cardinality and numeric min/max, (2) `Store::field_stats()` delegates to tantivy, (3) `corpus_stats()` in the corpus module assembles the full response from Store or HnswIndex + manifest + disk size, (4) `GET /stats` HTTP handler acquires per-corpus read-lock and calls `corpus_stats()` in `spawn_blocking`.

**Tech Stack:** Rust, tantivy 0.22 (columnar fast-field API), axum, serde_json, tokio

---

## File Map

| File | Change |
|------|--------|
| `crates/fastrag-store/src/tantivy.rs` | Add `FieldStat`, `FieldStatType` types; add `TantivyStore::field_stats()` method; add unit tests |
| `crates/fastrag-store/src/lib.rs` | Add `Store::field_stats()` delegation; re-export `FieldStat`, `FieldStatType` |
| `crates/fastrag/src/corpus/mod.rs` | Add `CorpusStats` and sub-types; add `corpus_stats()` function; add `disk_bytes()` helper |
| `fastrag-cli/src/http.rs` | Add `StatsQueryParams`, `stats_handler`; register `GET /stats` route |
| `fastrag-cli/tests/stats_e2e.rs` | New file: 4 integration tests |

---

### Task 1: `TantivyStore::field_stats()` with unit tests

**Files:**
- Modify: `crates/fastrag-store/src/tantivy.rs`

- [ ] **Step 1: Add `FieldStat` and `FieldStatType` types**

After the existing `UserFieldHandle` struct (line 42), add:

```rust
/// Per-field statistics computed from Tantivy fast-field columns.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldStat {
    pub name: String,
    pub field_type: FieldStatType,
    pub cardinality: u64,
}

/// Type-specific stat payload.
#[derive(Debug, Clone, PartialEq)]
pub enum FieldStatType {
    Text,
    Numeric { min: f64, max: f64 },
}
```

- [ ] **Step 2: Write failing unit tests**

In the existing `#[cfg(test)] mod tests` block (line 361), add two tests:

```rust
#[test]
fn field_stats_returns_cardinality_and_range() {
    let dir = TempDir::new().unwrap();
    let mut dyn_schema = DynamicSchema::new();
    dyn_schema
        .merge(FieldDef {
            name: "severity".to_string(),
            typed: TypedKind::String,
            indexed: true,
            stored: true,
            positions: false,
        })
        .unwrap();
    dyn_schema
        .merge(FieldDef {
            name: "cvss".to_string(),
            typed: TypedKind::Numeric,
            indexed: true,
            stored: true,
            positions: false,
        })
        .unwrap();

    let store = TantivyStore::create(dir.path(), &dyn_schema).unwrap();
    let core = store.core();

    let mut writer = store.writer().unwrap();

    // Doc 1: severity=HIGH, cvss=9.8
    {
        let mut doc = TantivyDocument::default();
        doc.add_u64(core.id, 1);
        doc.add_text(core.external_id, "v1");
        doc.add_text(core.content_hash, "h1");
        doc.add_u64(core.chunk_index, 0);
        doc.add_text(core.source_path, "/t.txt");
        doc.add_text(core.source, "{}");
        doc.add_text(core.chunk_text, "chunk one");
        doc.add_text(store.user_field("severity").unwrap().field, "HIGH");
        doc.add_f64(store.user_field("cvss").unwrap().field, 9.8);
        writer.add_document(doc).unwrap();
    }
    // Doc 2: severity=LOW, cvss=3.1
    {
        let mut doc = TantivyDocument::default();
        doc.add_u64(core.id, 2);
        doc.add_text(core.external_id, "v2");
        doc.add_text(core.content_hash, "h2");
        doc.add_u64(core.chunk_index, 0);
        doc.add_text(core.source_path, "/t2.txt");
        doc.add_text(core.source, "{}");
        doc.add_text(core.chunk_text, "chunk two");
        doc.add_text(store.user_field("severity").unwrap().field, "LOW");
        doc.add_f64(store.user_field("cvss").unwrap().field, 3.1);
        writer.add_document(doc).unwrap();
    }
    // Doc 3: severity=HIGH (duplicate), cvss=7.5
    {
        let mut doc = TantivyDocument::default();
        doc.add_u64(core.id, 3);
        doc.add_text(core.external_id, "v3");
        doc.add_text(core.content_hash, "h3");
        doc.add_u64(core.chunk_index, 0);
        doc.add_text(core.source_path, "/t3.txt");
        doc.add_text(core.source, "{}");
        doc.add_text(core.chunk_text, "chunk three");
        doc.add_text(store.user_field("severity").unwrap().field, "HIGH");
        doc.add_f64(store.user_field("cvss").unwrap().field, 7.5);
        writer.add_document(doc).unwrap();
    }
    writer.commit().unwrap();
    store.reload().unwrap();

    let stats = store.field_stats();

    // severity: text, cardinality >= 2 (HIGH, LOW)
    let sev = stats.iter().find(|s| s.name == "severity").expect("severity stat");
    assert!(matches!(sev.field_type, FieldStatType::Text));
    assert!(sev.cardinality >= 2, "severity cardinality: {}", sev.cardinality);

    // cvss: numeric, min=3.1, max=9.8, cardinality >= 3
    let cvss = stats.iter().find(|s| s.name == "cvss").expect("cvss stat");
    match &cvss.field_type {
        FieldStatType::Numeric { min, max } => {
            assert!((*min - 3.1).abs() < 0.01, "cvss min: {min}");
            assert!((*max - 9.8).abs() < 0.01, "cvss max: {max}");
        }
        _ => panic!("cvss should be Numeric"),
    }
    assert!(cvss.cardinality >= 3, "cvss cardinality: {}", cvss.cardinality);
}

#[test]
fn field_stats_empty_index() {
    let dir = TempDir::new().unwrap();
    let mut dyn_schema = DynamicSchema::new();
    dyn_schema
        .merge(FieldDef {
            name: "severity".to_string(),
            typed: TypedKind::String,
            indexed: true,
            stored: true,
            positions: false,
        })
        .unwrap();
    let store = TantivyStore::create(dir.path(), &dyn_schema).unwrap();
    let stats = store.field_stats();

    let sev = stats.iter().find(|s| s.name == "severity");
    if let Some(s) = sev {
        assert_eq!(s.cardinality, 0);
    }
}
```

- [ ] **Step 3: Run tests — expect compile failure (method doesn't exist)**

Run: `cargo test -p fastrag-store -- field_stats`
Expected: compile error — `field_stats` not found on `TantivyStore`

- [ ] **Step 4: Implement `TantivyStore::field_stats()`**

Add this method to the `impl TantivyStore` block, after `bm25_search` (line 356):

```rust
/// Compute per-field cardinality and min/max from Tantivy fast-field columns.
///
/// Only `String` (STRING|FAST, i.e. non-tokenized) and `Numeric` (f64 FAST) user
/// fields are included. Bool, Date, and Array fields are skipped.
///
/// Cardinality is approximate: per-segment distinct counts are summed for text
/// fields, which over-counts values appearing in multiple segments.
pub fn field_stats(&self) -> Vec<FieldStat> {
    let searcher = self.reader.searcher();
    let mut results = Vec::new();

    for handle in &self.user_fields {
        match handle.typed {
            TypedKind::String if !handle.name.is_empty() => {
                // STRING | FAST fields have str fast columns; tokenized (positions)
                // text fields do not.
                let mut total_cardinality: u64 = 0;
                for segment_reader in searcher.segment_readers() {
                    let fast_fields = segment_reader.fast_fields();
                    if let Ok(Some(str_col)) = fast_fields.str(&handle.name) {
                        total_cardinality += str_col.num_terms() as u64;
                    }
                }
                results.push(FieldStat {
                    name: handle.name.clone(),
                    field_type: FieldStatType::Text,
                    cardinality: total_cardinality,
                });
            }
            TypedKind::Numeric => {
                let mut global_min = f64::INFINITY;
                let mut global_max = f64::NEG_INFINITY;
                let mut distinct = std::collections::BTreeSet::<u64>::new();

                for segment_reader in searcher.segment_readers() {
                    let fast_fields = segment_reader.fast_fields();
                    if let Ok(Some(col)) = fast_fields.column_opt::<f64>(&handle.name) {
                        let seg_min = col.min_value();
                        let seg_max = col.max_value();
                        if seg_min < global_min {
                            global_min = seg_min;
                        }
                        if seg_max > global_max {
                            global_max = seg_max;
                        }
                        for doc_id in 0..segment_reader.max_doc() {
                            for val in col.values_for_doc(doc_id) {
                                distinct.insert(val.to_bits());
                            }
                        }
                    }
                }

                let cardinality = distinct.len() as u64;
                if global_min > global_max {
                    global_min = 0.0;
                    global_max = 0.0;
                }
                results.push(FieldStat {
                    name: handle.name.clone(),
                    field_type: FieldStatType::Numeric {
                        min: global_min,
                        max: global_max,
                    },
                    cardinality,
                });
            }
            _ => {} // Skip Bool, Date, Array
        }
    }
    results
}
```

**Tantivy 0.22 API notes:**
- `fast_fields().str(&name)` → `Result<Option<StrColumn>>`. `StrColumn` has `num_terms()` → O(1) per-segment cardinality.
- `fast_fields().column_opt::<f64>(&name)` → `Result<Option<Column<f64>>>`. Column has `min_value()`, `max_value()` (O(1)), and `values_for_doc(doc_id)` (iterator).
- `segment_reader.max_doc()` gives total doc slots including deleted — correct for fast-field iteration since columns are indexed by raw doc_id.

If `column_opt::<f64>` doesn't compile in tantivy 0.22, use `fast_fields().f64(&handle.name)` which returns `Result<Column<f64>>` (no Option wrapper). Adapt accordingly.

- [ ] **Step 5: Run tests — expect green**

Run: `cargo test -p fastrag-store -- field_stats`
Expected: both tests pass

- [ ] **Step 6: Commit**

```bash
git add crates/fastrag-store/src/tantivy.rs
git commit -m "feat(store): add TantivyStore::field_stats() for per-field cardinality and range"
```

---

### Task 2: `Store::field_stats()` + `corpus_stats()` function

**Files:**
- Modify: `crates/fastrag-store/src/lib.rs`
- Modify: `crates/fastrag/src/corpus/mod.rs`

- [ ] **Step 1: Add `Store::field_stats()` in `lib.rs`**

In `crates/fastrag-store/src/lib.rs`, after the `manifest()` method (line 441), add:

```rust
/// Compute per-field statistics from Tantivy fast-field columns.
pub fn field_stats(&self) -> Vec<crate::tantivy::FieldStat> {
    self.tantivy.field_stats()
}
```

- [ ] **Step 2: Add response types in `corpus/mod.rs`**

In `crates/fastrag/src/corpus/mod.rs`, after the `CorpusInfo` struct (line 148), add:

```rust
/// Health metrics for a corpus, returned by `GET /stats`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CorpusStats {
    pub corpus: String,
    pub entries: EntryStats,
    pub chunks: usize,
    pub disk_bytes: u64,
    pub embedding: EmbeddingInfo,
    pub chunking: ChunkingInfo,
    pub timestamps: TimestampInfo,
    pub fields: Vec<FieldStatDto>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EntryStats {
    pub live: usize,
    pub tombstoned: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EmbeddingInfo {
    pub model_id: String,
    pub dimensions: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChunkingInfo {
    pub strategy: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_characters: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overlap: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TimestampInfo {
    pub created_unix: u64,
    pub last_indexed_unix: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FieldStatDto {
    pub name: String,
    #[serde(rename = "type")]
    pub field_type: String,
    pub cardinality: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
}
```

- [ ] **Step 3: Add helper functions and `corpus_stats()`**

Below the new types, add:

```rust
/// Total disk usage of a corpus directory (recursive, covers tantivy_index subdir).
fn disk_bytes(corpus_dir: &Path) -> u64 {
    fn walk(dir: &Path, total: &mut u64) {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() {
                    if let Ok(meta) = std::fs::metadata(&path) {
                        *total += meta.len();
                    }
                } else if path.is_dir() {
                    walk(&path, total);
                }
            }
        }
    }
    let mut total: u64 = 0;
    walk(corpus_dir, &mut total);
    total
}

#[cfg(feature = "index")]
fn chunking_info(strategy: &ManifestChunkingStrategy) -> ChunkingInfo {
    match strategy {
        ManifestChunkingStrategy::Basic {
            max_characters,
            overlap,
        } => ChunkingInfo {
            strategy: "basic".to_string(),
            max_characters: Some(*max_characters),
            overlap: Some(*overlap),
        },
        ManifestChunkingStrategy::ByTitle {
            max_characters,
            overlap,
        } => ChunkingInfo {
            strategy: "by-title".to_string(),
            max_characters: Some(*max_characters),
            overlap: Some(*overlap),
        },
        ManifestChunkingStrategy::RecursiveCharacter {
            max_characters,
            overlap,
            ..
        } => ChunkingInfo {
            strategy: "recursive".to_string(),
            max_characters: Some(*max_characters),
            overlap: Some(*overlap),
        },
        ManifestChunkingStrategy::Semantic {
            max_characters, ..
        } => ChunkingInfo {
            strategy: "semantic".to_string(),
            max_characters: Some(*max_characters),
            overlap: None,
        },
    }
}

/// Compute corpus health statistics.
///
/// Supports Store-backed corpora (with `schema.json`) and HNSW-only legacy
/// corpora. HNSW-only corpora return an empty `fields` array.
#[cfg(feature = "store")]
pub fn corpus_stats(corpus_dir: &Path, corpus_name: &str) -> Result<CorpusStats, CorpusError> {
    use fastrag_store::tantivy::FieldStatType;

    let has_store = corpus_dir.join("schema.json").exists();
    let disk = disk_bytes(corpus_dir);

    if has_store {
        let store = fastrag_store::Store::open_no_embedder(corpus_dir)?;
        let manifest = store.manifest().clone();

        let fields: Vec<FieldStatDto> = store
            .field_stats()
            .into_iter()
            .map(|fs| match &fs.field_type {
                FieldStatType::Text => FieldStatDto {
                    name: fs.name,
                    field_type: "text".to_string(),
                    cardinality: fs.cardinality,
                    min: None,
                    max: None,
                },
                FieldStatType::Numeric { min, max } => FieldStatDto {
                    name: fs.name,
                    field_type: "numeric".to_string(),
                    cardinality: fs.cardinality,
                    min: Some(*min),
                    max: Some(*max),
                },
            })
            .collect();

        let last_indexed = manifest
            .roots
            .iter()
            .map(|r| r.last_indexed_unix_seconds)
            .max()
            .unwrap_or(manifest.created_at_unix_seconds);

        Ok(CorpusStats {
            corpus: corpus_name.to_string(),
            entries: EntryStats {
                live: store.live_count(),
                tombstoned: store.tombstone_count(),
            },
            chunks: manifest.chunk_count,
            disk_bytes: disk,
            embedding: EmbeddingInfo {
                model_id: manifest.identity.model_id.clone(),
                dimensions: manifest.identity.dim,
            },
            chunking: chunking_info(&manifest.chunking_strategy),
            timestamps: TimestampInfo {
                created_unix: manifest.created_at_unix_seconds,
                last_indexed_unix: last_indexed,
            },
            fields,
        })
    } else {
        // HNSW-only: read manifest directly, no Store to open.
        let manifest_bytes = std::fs::read(corpus_dir.join("manifest.json"))?;
        let manifest: crate::CorpusManifest =
            serde_json::from_slice(&manifest_bytes)?;

        let last_indexed = manifest
            .roots
            .iter()
            .map(|r| r.last_indexed_unix_seconds)
            .max()
            .unwrap_or(manifest.created_at_unix_seconds);

        Ok(CorpusStats {
            corpus: corpus_name.to_string(),
            entries: EntryStats {
                live: manifest.chunk_count,
                tombstoned: 0,
            },
            chunks: manifest.chunk_count,
            disk_bytes: disk,
            embedding: EmbeddingInfo {
                model_id: manifest.identity.model_id.clone(),
                dimensions: manifest.identity.dim,
            },
            chunking: chunking_info(&manifest.chunking_strategy),
            timestamps: TimestampInfo {
                created_unix: manifest.created_at_unix_seconds,
                last_indexed_unix: last_indexed,
            },
            fields: vec![],
        })
    }
}
```

**Note on HNSW-only fallback:** Rather than creating a passthrough embedder to load HnswIndex (complex and leaks memory), the HNSW-only path reads `manifest.json` directly and uses `chunk_count` as the entry count proxy. This is slightly inaccurate if tombstones exist but avoids significant complexity.

- [ ] **Step 4: Verify compilation**

Run: `cargo check --workspace --features retrieval,rerank`
Expected: compiles

- [ ] **Step 5: Commit**

```bash
git add crates/fastrag-store/src/lib.rs crates/fastrag/src/corpus/mod.rs
git commit -m "feat(corpus): add corpus_stats() with CorpusStats response types"
```

---

### Task 3: `GET /stats` HTTP handler

**Files:**
- Modify: `fastrag-cli/src/http.rs`

- [ ] **Step 1: Add `StatsQueryParams`**

After `DeleteQueryParams` (line 460), add:

```rust
#[derive(Debug, Deserialize)]
struct StatsQueryParams {
    #[serde(default = "default_corpus")]
    corpus: String,
}
```

- [ ] **Step 2: Add `stats_handler` function**

After `list_corpora` (line 704), add:

```rust
async fn stats_handler(
    State(state): State<AppState>,
    Query(params): Query<StatsQueryParams>,
) -> Result<Json<serde_json::Value>, Response> {
    let corpus_dir = state.registry.corpus_path(&params.corpus).ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": format!("corpus not found: {}", params.corpus) })),
        )
            .into_response()
    })?;

    let lock = get_or_create_lock(&state.ingest_locks, &params.corpus);
    let _read_guard = lock.read().await;

    let corpus_name = params.corpus.clone();
    let stats = tokio::task::spawn_blocking(move || {
        fastrag::corpus::corpus_stats(&corpus_dir, &corpus_name)
    })
    .await
    .map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, format!("join: {e}")).into_response()
    })?
    .map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, format!("stats: {e}")).into_response()
    })?;

    Ok(Json(serde_json::to_value(stats).unwrap()))
}
```

Uses `spawn_blocking` because `Store::open_no_embedder` does blocking file I/O.

- [ ] **Step 3: Register the route**

In the protected router (line 378-393), add after the `/corpora` route:

```rust
.route("/stats", get(stats_handler))
```

- [ ] **Step 4: Verify compilation**

Run: `cargo check -p fastrag-cli`
Expected: compiles

- [ ] **Step 5: Commit**

```bash
git add fastrag-cli/src/http.rs
git commit -m "feat(http): add GET /stats endpoint for corpus health metrics"
```

---

### Task 4: Integration tests

**Files:**
- Create: `fastrag-cli/tests/stats_e2e.rs`

- [ ] **Step 1: Create test file with 4 tests**

```rust
//! Integration tests for GET /stats.

use std::sync::Arc;

use fastrag::corpus::CorpusRegistry;
use fastrag_cli::http::{HttpRerankerConfig, serve_http_with_registry};
use fastrag_embed::test_utils::MockEmbedder;

async fn spawn_server(registry: CorpusRegistry) -> std::net::SocketAddr {
    let embedder: fastrag::DynEmbedder = Arc::new(MockEmbedder);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        serve_http_with_registry(
            registry, listener, embedder, None, false,
            HttpRerankerConfig::default(), 100, None, 52_428_800,
        )
        .await
        .unwrap();
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    addr
}

#[tokio::test]
async fn stats_after_ingest() {
    let corpus_dir = tempfile::tempdir().unwrap();
    let registry = CorpusRegistry::new();
    registry.register("default", corpus_dir.path().to_path_buf());
    let addr = spawn_server(registry).await;
    let client = reqwest::Client::new();

    // Ingest 2 records with severity (text) and cvss (numeric).
    let body = concat!(
        r#"{"id":"v1","body":"SQL injection vuln","severity":"HIGH","cvss":9.8}"#, "\n",
        r#"{"id":"v2","body":"buffer overflow","severity":"LOW","cvss":3.1}"#, "\n",
    );
    let resp = client
        .post(format!(
            "http://{}/ingest?id_field=id&text_fields=body&metadata_fields=severity,cvss&metadata_types=cvss=numeric",
            addr
        ))
        .header("content-type", "application/x-ndjson")
        .body(body)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // GET /stats
    let resp = client
        .get(format!("http://{}/stats", addr))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let stats: serde_json::Value = resp.json().await.unwrap();

    assert_eq!(stats["corpus"], "default");
    assert_eq!(stats["entries"]["live"].as_u64().unwrap(), 2);
    assert_eq!(stats["entries"]["tombstoned"].as_u64().unwrap(), 0);
    assert!(stats["disk_bytes"].as_u64().unwrap() > 0);
    assert!(stats["embedding"]["dimensions"].as_u64().unwrap() > 0);
    assert!(stats["timestamps"]["created_unix"].as_u64().is_some());

    let fields = stats["fields"].as_array().expect("fields array");
    let sev = fields.iter().find(|f| f["name"] == "severity");
    assert!(sev.is_some(), "severity missing from stats fields");
    assert_eq!(sev.unwrap()["type"], "text");
    assert!(sev.unwrap()["cardinality"].as_u64().unwrap() > 0);

    let cvss = fields.iter().find(|f| f["name"] == "cvss");
    assert!(cvss.is_some(), "cvss missing from stats fields");
    assert_eq!(cvss.unwrap()["type"], "numeric");
    assert!(cvss.unwrap()["min"].as_f64().is_some());
    assert!(cvss.unwrap()["max"].as_f64().is_some());
}

#[tokio::test]
async fn stats_unknown_corpus_returns_404() {
    let corpus_dir = tempfile::tempdir().unwrap();
    let registry = CorpusRegistry::new();
    registry.register("default", corpus_dir.path().to_path_buf());
    let addr = spawn_server(registry).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("http://{}/stats?corpus=nonexistent", addr))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body["error"].as_str().unwrap().contains("corpus not found"));
}

#[tokio::test]
async fn stats_reflects_delete() {
    let corpus_dir = tempfile::tempdir().unwrap();
    let registry = CorpusRegistry::new();
    registry.register("default", corpus_dir.path().to_path_buf());
    let addr = spawn_server(registry).await;
    let client = reqwest::Client::new();

    // Ingest 2 records.
    let body = concat!(
        r#"{"id":"v1","body":"first record"}"#, "\n",
        r#"{"id":"v2","body":"second record"}"#, "\n",
    );
    let resp = client
        .post(format!("http://{}/ingest?id_field=id&text_fields=body", addr))
        .header("content-type", "application/x-ndjson")
        .body(body)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // Stats before delete: live=2, tombstoned=0
    let resp = client.get(format!("http://{}/stats", addr)).send().await.unwrap();
    assert_eq!(resp.status(), 200);
    let stats: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(stats["entries"]["live"].as_u64().unwrap(), 2);
    assert_eq!(stats["entries"]["tombstoned"].as_u64().unwrap(), 0);

    // Delete one record
    let resp = client
        .delete(format!("http://{}/ingest/v1?corpus=default", addr))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // Stats after delete: live=1, tombstoned=1
    let resp = client.get(format!("http://{}/stats", addr)).send().await.unwrap();
    assert_eq!(resp.status(), 200);
    let stats: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(stats["entries"]["live"].as_u64().unwrap(), 1, "live after delete");
    assert_eq!(stats["entries"]["tombstoned"].as_u64().unwrap(), 1, "tombstoned after delete");
}

#[tokio::test]
async fn stats_uninitialized_corpus_returns_500() {
    let corpus_dir = tempfile::tempdir().unwrap();
    let registry = CorpusRegistry::new();
    registry.register("default", corpus_dir.path().to_path_buf());
    let addr = spawn_server(registry).await;
    let client = reqwest::Client::new();

    // Corpus dir exists but has no manifest.json — corpus_stats will fail.
    let resp = client.get(format!("http://{}/stats", addr)).send().await.unwrap();
    assert_eq!(resp.status(), 500, "uninitialized corpus should be 500");
}
```

- [ ] **Step 2: Run integration tests**

Run: `cargo test -p fastrag-cli --test stats_e2e`
Expected: all 4 pass

- [ ] **Step 3: Run full workspace tests for regressions**

Run: `cargo test --workspace --features retrieval,rerank`
Expected: all pass

- [ ] **Step 4: Commit**

```bash
git add fastrag-cli/tests/stats_e2e.rs
git commit -m "test(http): add GET /stats integration tests"
```

---

### Task 5: Local gate, push, CI

- [ ] **Step 1: Run clippy**

Run: `cargo clippy --workspace --all-targets --features retrieval,rerank -- -D warnings`
Expected: no warnings

- [ ] **Step 2: Run fmt check**

Run: `cargo fmt --check`
Expected: no diffs

- [ ] **Step 3: Push**

```bash
git push
```

- [ ] **Step 4: Run ci-watcher**

Invoke the ci-watcher skill as a background Haiku agent.

---

## Verification

```bash
# Unit tests (Store layer)
cargo test -p fastrag-store -- field_stats

# Integration tests (HTTP layer)
cargo test -p fastrag-cli --test stats_e2e

# Full workspace
cargo test --workspace --features retrieval,rerank

# Lint gate
cargo clippy --workspace --all-targets --features retrieval,rerank -- -D warnings
cargo fmt --check

# Manual smoke test
echo '{"id":"cve-1","body":"log4j RCE","severity":"CRITICAL","cvss":9.8}' | \
  curl -s -X POST 'http://localhost:8081/ingest?id_field=id&text_fields=body&metadata_fields=severity,cvss&metadata_types=cvss=numeric' \
       -H 'Content-Type: application/x-ndjson' --data-binary @-
curl -s 'http://localhost:8081/stats' | jq .
```
