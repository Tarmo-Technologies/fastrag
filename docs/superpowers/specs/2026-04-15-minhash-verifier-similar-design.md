# MinHash Verifier for POST /similar

> **Issue:** crook3dfingers/fastrag#56 — Phase 3 follow-up: MinHash/SimHash verifier for /similar (opt-in dedup)
> **Parent issue:** crook3dfingers/fastrag#52 — Similarity threshold endpoint (ANN-only)
> **Date:** 2026-04-15

## Problem

`POST /similar` (shipped in #52) filters ANN candidates by cosine threshold. For cross-engagement pattern matching and loose dedup, cosine-only is adequate. For strict dedup — the VAMS use case of collapsing duplicate findings across scanner outputs — cosine thresholds drift with embedding model changes, text length, and distribution tails, producing false positives near the threshold boundary.

A cheap verification stage on top of the ANN candidates fixes the boundary noise without changing the ANN stage's semantics.

## Goals

- Add an opt-in `verify` block to `POST /similar` that runs a MinHash Jaccard check over the cosine-surviving candidates.
- Deterministic, seedable, reproducible — identical input produces byte-identical signatures.
- Zero ingest-path change. Zero schema migration. Zero new persisted fields.
- Benchmarkable: synthetic precision/recall gate in CI, real-data benchmark behind an env flag.

## Non-goals

- SimHash (the `method` enum leaves room for a future addition).
- Pre-computed signatures at ingest (revisit only if benchmark shows a latency problem).
- Verifier on `/query` (this issue is `/similar`-scoped).
- MCP or CLI surface for dedup (HTTP only — VAMS is a server-to-server caller).

## Decisions

### D1 — On-the-fly signature computation (not pre-computed)

Compute both the query signature and each candidate signature at request time. Candidate sets after the cosine threshold are small (tens to a few thousand), and 128-permutation MinHash over a paragraph of char-5grams takes tens of microseconds. No ingest-path change, no schema evolution, no corpus rebuild.

**Revisit trigger:** if `/similar` p99 latency with `verify` enabled exceeds 2× the ANN-only baseline on either benchmark, add pre-computed signatures as a follow-up (persisted via `user_fields` on `ChunkRecord`, gated by a `signature_present` flag in the corpus manifest).

### D2 — Char-5gram shingling (not word-trigram)

VAMS findings are short (scanner titles plus brief descriptions) and vary in tokenization across tools (punctuation, casing, whitespace). Char-5gram is more robust to that noise than word-trigrams on short text. Lowercase, strip ASCII punctuation, collapse whitespace, then emit overlapping 5-character windows.

### D3 — Post-hoc verification, no adaptive overfetch

The verifier runs over whatever cosine-surviving set the existing ANN overfetch loop returns. If the verifier drops candidates below `max_results`, the response is short — `dropped_by_verifier` in stats tells the caller why. The VAMS use case is "how many near-duplicates exist," not "give me exactly 10." Adaptive-overfetch-with-verifier can be added later if the benchmark justifies it.

### D4 — Nested `verify` block with `method` discriminator

Matches the issue body example verbatim. `method: "minhash"` is the only accepted value in v1; unknown values return HTTP 400. Nesting gives SimHash a future home without breaking the request shape.

### D5 — MinHash primitive: blake3 base hash + 128 linear-permutation pairs

blake3 is already in the workspace dep graph. The classic MinHash construction `h_i(x) = (a_i · h(x) + b_i) mod p` needs one hash per shingle, not 128 — blake3 the shingle once to a `u64`, then apply 128 deterministic `(a_i, b_i)` pairs seeded from a fixed constant. No new dependency, 128× fewer hash invocations than a naive per-permutation-hash construction.

## Architecture

```
POST /similar { ..., verify: { method, threshold } }
  │
  ▼
similar_handler (fastrag-cli/src/http.rs)
  - parse + validate body (reject unknown method, out-of-range threshold)
  - call similarity_search
  │
  ▼
similarity_search_one (fastrag/src/corpus/similar.rs)
  - ANN candidate generation + cosine threshold (unchanged)
  - if verify present:
      compute query signature once
      for each candidate:
        fetch chunk_text (already available in the hit)
        compute candidate signature
        drop if jaccard < verify.threshold
        else attach verify_score to the hit
  - merge across corpora, sort by cosine, truncate to max_results
```

New module: `crates/fastrag/src/corpus/verify.rs`. Not a new crate — the primitive is ~300 LoC and is `/similar`-specific.

## MinHash parameters (fixed at compile time)

| Parameter | Value | Rationale |
|---|---|---|
| Permutations | 128 | ~0.088 stderr on Jaccard estimates, tight at threshold ≥ 0.7 |
| Shingling | char-5gram, lowercase + punctuation-stripped + whitespace-collapsed | Robust to scanner tokenization noise on short text |
| Base hash | blake3 → first 8 bytes as `u64` | Already in workspace dep graph; fast enough |
| Permutation seeds | 128 `(a_i, b_i)` pairs from a fixed `const SEED: u64` | Deterministic across runs and builds |
| Modulus | 2^61 - 1 (Mersenne prime) | Standard MinHash construction |
| Signature | `[u64; 128]` | No allocation per shingle |

Degenerate inputs:
- Empty or sub-5-char text → single zero-shingle signature. Pairs of empty inputs therefore Jaccard to 1.0, which is correct.
- Unicode passes through at the byte level — non-ASCII just produces more shingles.

## API

### Request (additive)

```json
{
  "text": "SQL injection in login form",
  "threshold": 0.85,
  "max_results": 10,
  "filter": "source_tool = semgrep",
  "verify": { "method": "minhash", "threshold": 0.7 }
}
```

- `verify`: optional object. Absent → exact current behavior.
- `verify.method`: enum, `"minhash"` only in v1. Unknown values → 400.
- `verify.threshold`: f32 in `[0.0, 1.0]`. Out of range → 400.

### Response additions

- `SimilarityHit.verify_score: Option<f32>` — serialized when present, omitted otherwise.
- `SimilarityStats.dropped_by_verifier: usize` — count across all corpora in a federated request.

## Testing

### Unit tests (`verify.rs`)

- Shingling: fixed inputs produce fixed shingle multisets (including the short/empty degenerate case).
- Signature determinism: same input → byte-identical `[u64; 128]` across runs.
- Jaccard sanity: identical signatures → 1.0; disjoint shingle sets → near zero within 128-perm stderr; known overlap ratio → expected Jaccard ± stderr.
- Seed regression: hard-coded expected signature for a fixture string, so accidental seed or permutation-count changes fail loudly.

### Integration tests (`fastrag/tests/minhash_verify.rs`)

- ANN returns N candidates, verifier drops to M, `hits.len() == M` and `dropped_by_verifier == N - M`.
- `verify.threshold = 0.0` is a no-op; `verify.threshold = 1.0` keeps only shingle-equivalent chunks.
- Federated request: `dropped_by_verifier` counts drops across all corpora.

### HTTP e2e (`fastrag-cli/tests/similar_verify_http_e2e.rs`)

- Happy path: nested `verify` block, both cosine and Jaccard filter, `verify_score` appears on hits.
- Error paths: unknown `method`, out-of-range threshold, non-numeric threshold, method missing → 400 with a clear message.
- Backward compatibility: requests without `verify` produce identical responses to the pre-change `similar_http_e2e` fixtures.

### Benchmarks

**Synthetic gate (`fastrag/tests/dedup_synthetic_gate.rs`):**

- Generator: deterministic paraphrase script over the NVD gold set — whitespace jitter, clause reorder, punctuation variance, synonym swap from a small hard-coded table. Generator and output both checked in for reproducibility.
- Assertion: at `cosine=0.85, jaccard=0.7`, verified-precision ≥ ANN-precision + 0.10 and recall ≥ 0.90.
- Runs on every build. Catches verifier regression or pointlessness in CI.

**VAMS gold set (`fastrag/tests/dedup_vams_gold.rs`):**

- `FASTRAG_DEDUP_GOLD=1`-gated, follows the existing `FASTRAG_NVD_TEST` / `FASTRAG_RERANK_TEST` pattern.
- Consumes `tests/fixtures/dedup/vams_pairs.jsonl` — labeled pairs of real scanner findings, shape `{a: string, b: string, is_duplicate: bool}`.
- Reports precision / recall / F1 at the chosen thresholds.
- Fixture shape defined in this issue; actual labeled pairs populated in a follow-up once we have VAMS output to label. The test is wired and compilable now so the benchmark path exists.

## Escape hatch for latency

If the synthetic gate or the VAMS benchmark shows p99 `/similar` latency with `verify` ≥ 2× the ANN-only baseline:

1. Add a `signatures` column in `ChunkRecord` (via `user_fields`, no Tantivy schema rebuild).
2. Compute signatures during ingest, alongside embeddings.
3. Add `signature_present: bool` in the corpus manifest.
4. Query path checks the flag and falls back to on-the-fly computation per-candidate when absent (mixed-corpus federation stays correct).

Option B/C from brainstorming; documented here so the seam is known, not built now.

## Out of scope

- SimHash (future `method: "simhash"`).
- Pre-computed signatures (per escape hatch above).
- CLI command for dedup.
- MCP tool for dedup.
- Verifier on `/query`.
- Adaptive overfetch that grows until the verifier produces `max_results` hits.

## Acceptance criteria

- [ ] `POST /similar` accepts an opt-in `verify` block; unknown methods and out-of-range thresholds return 400.
- [ ] MinHash signature computation is deterministic and seedable; a regression test pins a fixture signature.
- [ ] Candidates failing the Jaccard threshold are dropped before `max_results` is applied; `dropped_by_verifier` reports the count.
- [ ] ADR D1 (on-the-fly vs pre-compute) is recorded in this doc with the benchmark revisit trigger.
- [ ] Synthetic precision/recall gate passes in CI at the documented thresholds.
- [ ] VAMS gold-set benchmark is wired and runs under `FASTRAG_DEDUP_GOLD=1` (fixture-populated later).
- [ ] Response shape additions (`verify_score`, `dropped_by_verifier`) are reflected in README/docs endpoint tables.
- [ ] Backward compatibility: requests without `verify` produce byte-identical responses to pre-change fixtures.
