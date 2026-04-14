# CWE Hierarchy Expansion — Design Spec

**Issue**: #47 (Phase 3 Step 7)
**Status**: Design approved, pending implementation plan
**Date**: 2026-04-13

## Problem

A query for CWE-89 (SQL Injection) should also find advisories tagged with child CWEs like CWE-564 (Hibernate Injection). Current CWE matching is exact-only: exact CWE ID equality, no taxonomic relationship.

MITRE publishes a CWE taxonomy with multiple views (Research, Development, Hardware). This spec covers the primary Research view (CWE-1000), which captures software-development `ChildOf` relationships.

## Approach

**Query-time descendant expansion.** A precomputed descendant-closure table is embedded in the binary. At query time, any filter predicate on the CWE field is rewritten to expand each referenced CWE into its descendant set before filter evaluation.

### Why query-time over ingest-time materialization

| Dimension | Query-time expansion (chosen) | Ingest-time ancestors |
|---|---|---|
| Query latency | sub-ms (hash lookup + `TermSetQuery`) | sub-ms (single term) |
| Index size | unchanged | +5–15 array values per CWE-tagged doc |
| Taxonomy updates | swap JSON + restart | full reindex |
| Multiple views | cheap (load another tree) | multiplies index cost |
| Ingest cost | none | per-doc tree walk |

Query latency is effectively tied. The differentiator is operational flexibility: taxonomy data updates yearly, and materialized ancestors lock the index to a specific taxonomy version.

## Architecture

### New crate: `crates/fastrag-cwe/`

```
crates/fastrag-cwe/
├── Cargo.toml
├── data/
│   └── cwe-tree-v4.16.json       # precomputed closure, committed
├── build.rs                       # validates JSON at compile time
└── src/
    ├── lib.rs                     # public API
    ├── taxonomy.rs                # Taxonomy struct, closure lookup
    └── data.rs                    # include_bytes! + lazy parse
```

Public API:

```rust
pub struct Taxonomy {
    version: String,
    view: String,
    closure: HashMap<u32, Vec<u32>>,  // CWE id → [self, descendants...]
}

impl Taxonomy {
    pub fn embedded() -> &'static Taxonomy;
    pub fn expand(&self, cwe: u32) -> &[u32];  // returns [self] if unknown
    pub fn version(&self) -> &str;
}
```

### Taxonomy data format

```json
{
  "version": "4.16",
  "view": "1000",
  "closure": {
    "89": [89, 564, 943, 1286, ...],
    "79": [79, 80, 81, 83, 84, 85, 86, 87, 692],
    ...
  }
}
```

Precomputed closure, not adjacency list: O(1) expansion, no runtime traversal, no cycle handling at query time. CWE-1000 has ~900 nodes; full closure stays under 500 KB.

### Taxonomy regeneration

`cargo run -p fastrag-cwe --bin compile-taxonomy -- --in cwec_v4.16.xml --out data/cwe-tree-v4.16.json` parses MITRE's XML, walks `ChildOf` edges in view CWE-1000, and writes the closure. Reuses XML download plumbing from `fastrag-eval/src/datasets/cwe.rs`. Not part of the runtime — invoked only when refreshing the taxonomy.

### Filter rewrite

New module: `crates/fastrag/src/filter/cwe_rewrite.rs`.

```rust
pub struct CweRewriter<'a> {
    taxonomy: &'a Taxonomy,
    cwe_field: &'a str,
}

impl CweRewriter<'_> {
    pub fn rewrite(&self, expr: FilterExpr) -> FilterExpr { ... }
}
```

Rewrite rules (only when `field == cwe_field`):

| Before | After |
|---|---|
| `Eq(cwe_id, 89)` | `In(cwe_id, [89, 564, 943, ...])` |
| `In(cwe_id, [89, 79])` | `In(cwe_id, [89, 564, ..., 79, 80, 81, ...])` (union, deduped) |
| `Neq(cwe_id, 89)` | `NotIn(cwe_id, [89, 564, 943, ...])` |
| `NotIn(cwe_id, [89])` | `NotIn(cwe_id, [89, 564, 943, ...])` |
| `Gt`/`Gte`/`Lt`/`Lte` on cwe_id | unchanged |

Unknown CWEs: `taxonomy.expand(9999)` returns `[9999]`, so rewrite is a no-op on values the taxonomy doesn't know — custom or future CWEs don't break queries.

Integration point: a single call in `query_corpus_with_filter` in `crates/fastrag/src/corpus/mod.rs`, after filter parsing, before evaluation.

### Free-text CWE extraction trigger

`crates/fastrag-index/src/identifiers.rs` regex-extracts CWE references from free-text queries. When `--cwe-expand` is on and a CWE field is configured:

1. Free-text query flows to embeddings and BM25 as before.
2. Extracted CWE IDs are converted to a synthetic `In(cwe_field, [...expanded])` filter.
3. The synthetic filter is AND-combined with any user-supplied filter.

### Corpus manifest

The manifest written at ingest time records the CWE field name, so the query path knows what to rewrite:

```json
{
  "cwe_field": "cwe_id",
  "cwe_taxonomy_version": "4.16"
}
```

`cwe_field` is set from `--cwe-field` at ingest (introduced in #41). `cwe_taxonomy_version` is pinned at index build time.

## CLI surface

```bash
fastrag query "sql injection" --corpus ./corpus --cwe-expand
fastrag query "sql injection" --corpus ./corpus --no-cwe-expand
fastrag serve-http --corpus ./corpus --cwe-expand
```

Default policy:
- **Off** for generic corpora.
- **On** when the corpus manifest declares a `cwe_field` (signal: ingested with `--security-profile` or `--cwe-field`).
- User flag overrides either default.

HTTP:
- `--cwe-expand` flag on `serve-http` sets the server-wide default.
- `?cwe_expand=true|false` on `/query` overrides per request.

## Error handling

- Missing taxonomy JSON at compile time: `build.rs` fails the build.
- Malformed taxonomy JSON: parse error surfaces at first `Taxonomy::embedded()` call. Panic is acceptable — data is embedded and validated at build time.
- Unknown CWE ID at query time: silent pass-through.
- `--cwe-expand` set but no `cwe_field` in manifest: `tracing::warn!` once per process, treat as no-op.
- Invalid `cwe_expand` query param on HTTP: 400 Bad Request with a clear message.

## Testing

TDD red-green-refactor, per repo discipline.

### Unit tests

`fastrag-cwe/src/taxonomy.rs`:
- `expand(89)` includes 89 and known descendants (assert on concrete IDs like 564).
- `expand(9999)` returns `[9999]`.
- `expand(1000)` (root of view) returns hundreds of CWEs.
- Closure is self-inclusive and idempotent.
- Version parses correctly.

`fastrag/src/filter/cwe_rewrite.rs`:
- `Eq(cwe_id, 89)` rewrites to `In` with correct set.
- `In(cwe_id, [89, 79])` rewrites to deduped union.
- `Neq` and `NotIn` rewrite correctly.
- Non-cwe fields pass through unchanged.
- Nested AST (`And`, `Or`, `Not`) recurses correctly.
- No-op when `cwe_field` is `None`.

### Integration tests

- `crates/fastrag/tests/cwe_expansion.rs` — ingest a small JSONL corpus with parent and child CWEs, query parent with `--cwe-expand`, assert child-tagged docs return.
- `fastrag-cli/tests/cwe_expand_e2e.rs` — CLI end-to-end: index → query → verify expanded matches.
- HTTP test: `POST /query` with `cwe_expand=true` and `filter=cwe_id=89` returns docs tagged with descendants.

### Eval validation

- Add gold-set entries where the query mentions a parent CWE but relevant docs are tagged with child CWEs.
- Run `fastrag eval` matrix with `--cwe-expand` vs baseline.
- Document improvement in eval report before flipping the on-by-default behavior for security-profile corpora.

## Scope boundaries (non-goals)

- Only CWE-1000 (Research View) for v1. Other views (Development, Hardware, CWE-700) are follow-ups.
- No runtime taxonomy reload — taxonomy is embedded. Refresh = rebuild.
- No cross-taxonomy expansion (no CAPEC ↔ CWE, no CVE ↔ CWE joins).
- No upward (ancestor) expansion. Docs tagged with a child CWE are reachable from queries on the parent; the reverse is out of scope.
- `SecurityId::Cwe` in `identifiers.rs` continues to return strings; the rewriter normalizes to `u32` at the integration seam.

## Acceptance criteria

- [ ] CWE taxonomy loaded from committed JSON, version-tracked in manifest
- [ ] Descendant closure correct for CWE-1000 view (`ChildOf` relationships)
- [ ] Query for CWE-89 retrieves documents tagged with child CWEs
- [ ] `--cwe-expand` flag toggles behavior
- [ ] Eval proves improvement on security-specific test set without regression
- [ ] All existing tests pass
