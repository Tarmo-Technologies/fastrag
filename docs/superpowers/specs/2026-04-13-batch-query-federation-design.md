# Batch Query + Multi-Corpus Federation Design

## Goal

Ship `POST /batch-query` (#43) and multi-corpus federation (#46) as two sequential phases. Batch query ships first against the single-corpus server; federation follows and extends batch query with a per-query `corpus` field.

## Architecture

No new crates. All changes land in existing files:

| File | Change |
|------|--------|
| `fastrag-cli/src/http.rs` | New routes, `AppState` refactor, tenant middleware |
| `fastrag-cli/src/args.rs` | `--corpus name=path` (repeatable), `--tenant-field`, `--batch-max-queries` |
| `crates/fastrag/src/corpus/mod.rs` | New `batch_query()` function |
| `crates/fastrag/src/corpus/registry.rs` | New: `CorpusRegistry`, `CorpusHandle`, lazy loading |
| `crates/fastrag/src/corpus/federation.rs` | New: RRF merge across corpus result sets |

---

## Phase A — Batch Query (#43)

### Endpoint

`POST /batch-query` (JSON body, auth-protected same as `/query`).

**Request:**
```json
{
  "queries": [
    {"q": "SQL injection in Apache Struts", "top_k": 5, "filter": "severity = HIGH"},
    {"q": "CVE-2024-1234 remediation", "top_k": 3},
    {"q": "deserialization RCE", "filter": {"in": {"field": "severity", "values": ["HIGH","CRITICAL"]}}, "top_k": 5}
  ]
}
```

- `filter` accepts both the existing string syntax (`"severity = HIGH"`) and JSON AST (same `FilterExpr` the query engine already parses).
- Per-query `top_k` is required (no global default — callers set it explicitly per query).
- `corpus` field accepted but ignored until Phase B.

**Response (HTTP 200 always, partial failure):**
```json
{
  "results": [
    {"index": 0, "hits": [...]},
    {"index": 1, "error": "bad filter: unexpected token at position 3"},
    {"index": 2, "hits": [...]}
  ]
}
```

### Shared-lib function

```rust
// crates/fastrag/src/corpus/mod.rs
pub fn batch_query(
    corpus_dir: &Path,
    embeddings: &[Vec<f32>],       // pre-computed, one per query
    params: &[BatchQueryParams],   // top_k, filter per query
    #[cfg(feature = "rerank")] reranker: Option<&dyn Reranker>,
) -> Vec<Result<Vec<SearchHitDto>, CorpusError>>
```

The HTTP handler embeds all query texts in a single llama-server call, then calls `batch_query()`. Embedding happens in the HTTP layer; retrieval is corpus-layer-only.

### Parallelism

Retrieval fans out via `rayon::par_iter()` across the query params. Embedding is a single batched call (llama-server already supports batch embedding).

### CLI flag

`--batch-max-queries N` (default: 100). Requests exceeding this limit → 400.

### Error semantics

| Error type | Behaviour |
|------------|-----------|
| Malformed filter | Error at that query's index, others proceed, HTTP 200 |
| Embedding failure | HTTP 503 — whole batch fails (can't embed partial) |
| Retrieval failure on one query | Error at that query's index, others proceed, HTTP 200 |

---

## Phase B — Multi-Corpus Federation (#46)

### CLI syntax change

Current: `--corpus ./path` (single corpus)

New: `--corpus name=path` (repeatable)
```bash
fastrag serve-http \
  --corpus nvd=/data/nvd \
  --corpus findings=/data/findings \
  --port 8081 --token $FASTRAG_TOKEN
```

**Back-compat:** `--corpus ./path` with no `=` → treated as `--corpus default=./path`. Existing single-corpus deployments continue working.

### CorpusRegistry

```rust
// crates/fastrag/src/corpus/registry.rs
pub struct CorpusRegistry {
    corpora: HashMap<String, CorpusHandle>,
    max_memory: Option<u64>,   // LRU eviction when set
}

pub struct CorpusHandle {
    path: PathBuf,
    state: CorpusState,        // Unloaded | Loaded(Arc<LoadedCorpus>)
}
```

Corpora load on first query, not at startup. `--max-corpus-memory` sets an LRU eviction cap (bytes).

### GET /corpora

```json
{
  "corpora": [
    {"name": "nvd",      "status": "loaded",   "entries": 42100, "path": "/data/nvd"},
    {"name": "findings", "status": "unloaded",  "entries": null,  "path": "/data/findings"}
  ]
}
```

### Query routing

`GET /query?q=...&corpus=nvd` — target a specific corpus. Omitting `corpus` queries the corpus named `default`; if no corpus is named `default`, the server returns 400 with a message listing available corpus names.

`POST /batch-query` gains per-query `corpus` field. Each query routes to its named corpus. Cross-corpus batch (one request spanning multiple corpora with RRF merge) is out of scope.

### Tenant enforcement

`--tenant-field engagement_id` at startup. Every protected request must include `X-Fastrag-Tenant: <value>`. The server injects a mandatory `AND engagement_id = <value>` filter before retrieval. Applied server-side; client cannot bypass.

- Missing header when `--tenant-field` is set → 401
- Tenant enforcement applies to `/query`, `/batch-query`, and future ingest endpoints

Implementation: axum middleware layer that mutates the filter expression before the handler runs.

### Federation RRF (deferred)

Cross-corpus RRF merge (one query spanning multiple corpora) is defined in #46 but is complex enough to defer to a follow-up. Phase B ships: named registry + lazy loading + `GET /corpora` + per-query corpus routing + tenant enforcement. RRF merge tracked as a follow-on.

---

## Testing

### Batch query
- Unit: `batch_query()` with pre-computed embeddings produces identical results to sequential `query_corpus_with_filter()` calls
- Integration: 3-query batch where query 1 has a bad filter → `results[0]` has `error`, `results[1]` and `results[2]` have hits, HTTP 200
- Integration: batch embedding vectors match individual embedding calls (same text → same vector)

### Federation
- Integration: server started with 2 named corpora; `GET /corpora` returns both; `/query?corpus=nvd` returns hits from NVD corpus only
- Unit: `--corpus ./path` (no `=`) parses as `name="default"`, `path="./path"`
- Integration: tenant enforcement — request without `X-Fastrag-Tenant` header → 401; with header → filter applied, only tenant-matching results returned
- Unit: `CorpusHandle` state machine — `Unloaded` → `Loaded` on first query, `Unloaded` again after LRU eviction

---

## Implementation Order

1. `batch_query()` in `crates/fastrag/src/corpus/mod.rs` with unit tests
2. `POST /batch-query` handler in `http.rs` with integration tests
3. `CorpusRegistry` + `CorpusHandle` in `corpus/registry.rs` with unit tests
4. `AppState` refactor to use `CorpusRegistry`, `--corpus name=path` CLI change
5. `GET /corpora` endpoint
6. Tenant enforcement middleware
7. Per-query `corpus` field in `/query` and `/batch-query`
