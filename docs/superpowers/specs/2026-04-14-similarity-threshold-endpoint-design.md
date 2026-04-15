# Similarity Threshold Endpoint Design

> **Issue:** crook3dfingers/fastrag#52 — Phase 3 Step 12: Similarity threshold endpoint
> **Follow-up:** crook3dfingers/fastrag#56 — MinHash/SimHash verifier (deferred)
> **Date:** 2026-04-14

## Problem

VAMS dedup and cross-engagement pattern matching need a "find all documents above X similarity" primitive rather than "return top-K." The existing `/query` endpoint returns a fixed K regardless of semantic distance, which breaks dedup (misses duplicates beyond K, or includes unrelated rows when K is large and the corpus is small).

## Goals

- Ship `POST /similar` with raw-cosine threshold filtering over the dense retrieval path.
- Support single-corpus and multi-corpus (fan-out) modes in one request.
- Stable, predictable threshold semantics: one scale, one meaning, for every request.
- Reuse existing store / embedder / filter infrastructure; no new retrieval stack.

## Non-goals

- MinHash/SimHash verification (deferred to #56).
- Hybrid retrieval or temporal decay on `/similar` (explicitly rejected with 400).
- Reranking (changes score meaning).
- CWE hierarchy expansion (deferred — `/similar` is narrow by design).
- MCP tool or CLI command (deferred — HTTP only for this issue).

## Architecture

```
POST /similar
  │
  ▼
┌────────────────────────────────────────┐
│ similar_handler (fastrag-cli/src/http) │
│  - parse + validate body               │
│  - resolve target corpora              │
│  - attach tenant filter                │
│  - call similarity_search              │
└──────────────┬─────────────────────────┘
               ▼
┌────────────────────────────────────────┐
│ similarity_search                      │
│ (crates/fastrag/src/corpus/similar.rs) │
│  - embed text once                     │
│  - spawn_blocking per corpus:          │
│      adaptive overfetch loop           │
│      filter eval                       │
│      threshold cut                     │
│  - merge, sort desc by cosine          │
│  - cap at max_results                  │
│  - hydrate via scored_ids_to_dtos      │
└──────────────┬─────────────────────────┘
               ▼
          SimilarityResponse
```

All scoring is raw cosine from `Store::query_dense`. No hybrid, no decay, no rerank stages.

## Request schema

```json
POST /similar
Content-Type: application/json

{
  "text": "SQL injection in login form",
  "threshold": 0.85,
  "max_results": 10,
  "corpus": "acme-q1",
  "corpora": ["acme-q1", "acme-q2"],
  "filter": "source_tool = semgrep",
  "fields": "cosine_similarity,snippet"
}
```

### Field contract

| Field | Type | Required | Default | Notes |
|---|---|---|---|---|
| `text` | string | yes | — | Non-empty. Embedded once per request. |
| `threshold` | float | yes | — | Raw cosine, `[0.0, 1.0]`. |
| `max_results` | int | yes | — | `[1, 1000]`. Hard ceiling; this is not a bulk-export endpoint. |
| `corpus` | string | no | `"default"` if both `corpus` and `corpora` omitted | Mutually exclusive with `corpora`. |
| `corpora` | array of strings | no | — | Non-empty when present. Mutually exclusive with `corpus`. All names must exist in the registry. |
| `filter` | string OR JSON `FilterExpr` | no | — | Same parsing as `/query/batch`. |
| `fields` | string | no | — | Same include/exclude syntax as `/query`. |

### Validation (400 on failure, unless noted)

- `text` missing or empty.
- `threshold` outside `[0.0, 1.0]`.
- `max_results` outside `[1, 1000]`.
- Both `corpus` and `corpora` set.
- `corpora` set and empty.
- Any of the hybrid/decay fields (`hybrid`, `rrf_k`, `rrf_overfetch`, `time_decay_field`, `time_decay_halflife`, `time_decay_weight`, `time_decay_dateless_prior`, `time_decay_blend`) set → 400 with body `"/similar does not support hybrid or temporal decay; see /query"`.
- `rerank` field set → 400 with body `"/similar does not support reranking"`.
- `filter` fails to parse (string) or deserialize (JSON) → 400 with parse error.
- Named corpus missing from registry → **404** with body naming the missing corpus.

### Tenant middleware

The existing `TenantFilter` extension (`fastrag-cli/src/http.rs`) applies. When present, the tenant predicate is AND-ed into the user-supplied filter before the threshold cut. Identical semantics to `/query`.

## Response schema

```json
{
  "hits": [
    {
      "cosine_similarity": 0.934,
      "corpus": "acme-q1",
      "snippet": "...",
      "source": {
        "id": "finding-8821",
        "cwe_id": 89,
        "source_tool": "semgrep"
      }
    }
  ],
  "truncated": false,
  "stats": {
    "candidates_examined": 480,
    "above_threshold": 12,
    "returned": 10,
    "per_corpus": {
      "acme-q1": { "candidates_examined": 240, "above_threshold": 8 },
      "acme-q2": { "candidates_examined": 240, "above_threshold": 4 }
    }
  },
  "latency": {
    "embed_us": 1240,
    "hnsw_us": 8100,
    "total_us": 12300
  }
}
```

### Field contract

| Field | Notes |
|---|---|
| `hits[].cosine_similarity` | Raw cosine from `Store::query_dense`. Never polymorphic. |
| `hits[].corpus` | Always populated (single-corpus case stamps the resolved name). |
| `hits[].snippet` | Default `snippet_len = 150`. Not configurable in v1. |
| `hits[].source` | Raw metadata record. Respects `fields` projection. |
| `truncated` | `true` when the adaptive overfetch hit the server cap with the tail still above threshold. |
| `stats.candidates_examined` | Total rows pulled from stores (pre-filter). |
| `stats.above_threshold` | Total rows above threshold across all target corpora (pre-`max_results` cap). |
| `stats.returned` | `hits.len()`. Equals `min(above_threshold, max_results)` unless `truncated`. |
| `stats.per_corpus` | Present only when `corpora` (array) was used. |
| `latency.*` | Reuses `LatencyBreakdown` but populates only `embed_us`, `hnsw_us`, `total_us`. |

Sort order: descending `cosine_similarity`. Ties broken by `(corpus, source.id)` lexicographic for determinism.

## Core algorithm

```
similarity_search(request) -> SimilarityResponse:
  1. Embed request.text once -> vector
  2. targets := resolve(request.corpus / request.corpora)    // list of corpus paths
  3. per_corpus := targets.par_iter().map(|corpus| {
       spawn_blocking(|| similarity_search_one(corpus, vector, request, cap))
     }).collect()
  4. merged := per_corpus.flat_map(|pc| pc.hits).collect()
  5. merged.sort_by(|a, b| {
       b.cosine.total_cmp(&a.cosine)
         .then_with(|| a.corpus.cmp(&b.corpus))
         .then_with(|| a.source_id.cmp(&b.source_id))
     })
  6. above_threshold := merged.len()
  7. returned := merged.truncate(request.max_results)
  8. truncated := any(per_corpus.map(|pc| pc.truncated))
  9. hits := scored_ids_to_dtos(returned) with corpus name stamped
 10. return SimilarityResponse { hits, truncated, stats, latency }

similarity_search_one(corpus, vector, request, cap) -> PerCorpusResult:
  fetch_count := request.max_results * 10
  loop:
    candidates := store.query_dense(vector, fetch_count)
    if request.filter.is_some():
      candidates := metadata_fetch_then_filter(store, candidates, &request.filter)
    above := candidates.iter().filter(|(_, s)| *s >= request.threshold).collect()
    if above.len() >= request.max_results:
      return { hits: above, truncated: false, candidates_examined: candidates.len() }
    if candidates.is_empty() or candidates.last().1 < request.threshold:
      return { hits: above, truncated: false, candidates_examined: candidates.len() }
    if fetch_count >= cap:
      return { hits: above, truncated: true, candidates_examined: candidates.len() }
    fetch_count := min(fetch_count * 2, cap)
```

Stopping conditions, in order:
1. **Enough found:** `above.len() >= max_results`. We have at least as many above-threshold rows as the caller asked for.
2. **Tail exhausted:** lowest-scored candidate is already below threshold. No more above-threshold rows exist anywhere in the corpus at this vector.
3. **Cap hit:** `fetch_count >= server_cap`. Return what we have; set `truncated`.

The cap defaults to 10,000 and is tunable via `--similar-overfetch-cap` on `serve-http`.

## Parallelism

- Embed once, up front. Embedder is the single most expensive call; do not duplicate it.
- Fan out per-corpus work via `tokio::task::spawn_blocking`. Store open + HNSW query + metadata fetch are all blocking I/O + CPU; they must not run on the async runtime.
- Ordering is stable: merge happens after all per-corpus results return; the total sort is deterministic.

## Reuse

- `Store::query_dense` — existing.
- `FilterExpr::evaluate` + metadata fetch — lift the filtered branch of `query_corpus_with_filter_opts` (`crates/fastrag/src/corpus/mod.rs:1036–`). Extract as a helper (e.g., `filter_scored_ids`) callable from both `similar.rs` and the existing filtered path.
- `scored_ids_to_dtos` — existing, used as-is. The new code stamps `corpus` on each DTO before returning.
- `LatencyBreakdown` — existing struct; populate only the relevant fields.
- `TenantFilter` middleware — existing; no changes.

## Changes

### New files

- `crates/fastrag/src/corpus/similar.rs`
  Owns `similarity_search`, `similarity_search_one`, `SimilarityRequest`, `SimilarityResponse`, `SimilarityHit`, `SimilarityStats`, `PerCorpusStats`. Unit tests inline (`#[cfg(test)] mod tests`).

- `fastrag-cli/tests/similar_http_e2e.rs`
  HTTP integration tests using the existing axum test-server helper.

### Modified files

- `crates/fastrag/src/corpus/mod.rs`
  Add `pub mod similar;` + re-export the public types. Extract the filter-eval helper.

- `fastrag-cli/src/http.rs`
  Add `SimilarRequest` / `SimilarResponse` body structs, `similar_handler` async fn, route registration `.route("/similar", post(similar_handler))`, metrics (`fastrag_similar_total`, `fastrag_similar_duration_seconds`), and logging spans matching `/query`.

- `fastrag-cli/src/args.rs`
  Add `--similar-overfetch-cap` flag on the `serve-http` command (default 10_000). Plumb to `AppState`.

- `README.md`
  New "Similarity Search" section documenting the endpoint, threshold semantics, fan-out, and validation errors.

- `CLAUDE.md`
  Add the new `cargo test` line for `similar_http_e2e`.

## Testing

### Unit tests (inline in `similar.rs`)

1. **`threshold_filters_below_cutoff`** — 5 docs with known cosines, threshold 0.9 returns only the 2 above.
2. **`max_results_caps_above_threshold`** — 20 docs all above threshold, `max_results = 5`, response has exactly 5 hits.
3. **`adaptive_overfetch_doubles_until_tail_exhausted`** — 100 docs in corpus, 15 above threshold, `max_results = 20`; assert the loop doubled past `max_results * 10 = 200` only until the tail was exhausted, returned all 15.
4. **`truncated_flag_set_when_cap_hit`** — server cap 50, 200 docs all above threshold, assert `truncated = true` and `returned == max_results`.
5. **`fan_out_merges_across_corpora`** — 2 corpora with 3 above-threshold docs each (known cosines); assert merged order is the global sort, not per-corpus.
6. **`fan_out_embeds_once`** — counting `MockEmbedder`; 3 target corpora; assert embed call counter equals 1.
7. **`filter_applied_before_threshold`** — 10 docs, filter keeps 4, threshold keeps 2 of those 4; assert returned ids.
8. **`ties_broken_deterministically`** — 3 docs with identical cosine from different corpora; assert order is by `(corpus, source.id)` lexicographic.

### Integration tests (`fastrag-cli/tests/similar_http_e2e.rs`)

1. **`post_similar_happy_path`** — index JSONL, POST with threshold, assert `hits[].cosine_similarity >= threshold` for every hit, `stats.returned == expected`.
2. **`post_similar_fan_out`** — index two named corpora, POST with `corpora: [...]`, assert merged sorted response + `stats.per_corpus` populated for both.
3. **`post_similar_rejects_hybrid_params`** — POST with `hybrid: true`, assert 400 + error body mentions `/query`.
4. **`post_similar_rejects_both_corpus_and_corpora`** — POST with both set, assert 400.
5. **`post_similar_corpus_not_found`** — POST with unknown corpus name, assert 404 naming it.
6. **`post_similar_tenant_filter_applied`** — seed a corpus with two tenants, set the tenant header, assert hits are filtered to that tenant.
7. **`post_similar_truncated_flag`** — start server with `--similar-overfetch-cap 50`, index many matching docs, assert `truncated: true`.

All tests assert on concrete values (specific cosines, ids, counts). No `assert!(result.is_ok())` or mock-only assertions.

## Out of scope

- MinHash/SimHash verification — #56.
- MCP `find_similar` tool.
- CLI `fastrag similar` subcommand.
- Per-hit `snippet_len` override.
- Returning embeddings in the response.
- Pagination — `max_results` cap + `truncated` flag is deliberately instead of cursor pagination.

## Risk

- **Cap tuning.** Default 10k overfetch per corpus may be too conservative for large corpora. Mitigation: expose via `--similar-overfetch-cap`; surface `truncated` prominently so operators know when to raise it.
- **Embedder model change drift.** Cosines are not comparable across embedder models. Mitigation: callers must version their thresholds per embedder. Documented in README. Not solvable at this layer.
- **HNSW recall.** HNSW is approximate. The "tail exhausted" stopping condition trusts that the last-returned score is a valid upper bound on anything-not-yet-seen. In practice this can be violated for pathological queries near HNSW's recall boundary. Mitigation: the cap provides a hard bound; `truncated` surfaces the uncertainty. Accepted risk.
