# Step 6 — Eval Harness Refresh + Gold Set (2026-04-11)

**Status:** design approved, pending implementation plan
**Roadmap:** `docs/superpowers/roadmap-2026-04-phase2-rewrite.md` — Phase 2 Step 6
**Research:** `docs/rag-research-2026-04.md`
**Depends on:** Step 1 (embedder invariant), Step 2 (llama.cpp backend), Step 3 (reranker), Step 4 (hybrid retrieval), Step 5 (contextual retrieval)

## Summary

Rebuild the eval harness against the full Phase 2 Steps 1–5 retrieval stack. Extend the existing `fastrag-eval` crate in place with a hand-curated 100+ gold set under `tests/gold/`, a 4-variant config matrix (primary + no-rerank + no-contextual + dense-only), per-stage latency percentiles, and a checked-in baseline with a 2% slack regression gate. Runs weekly on CI to keep the billing exposure bounded. Push CI gains a lightweight gold-set validation canary to guard the fixture.

No generation step, no groundedness metric, no automatic baseline promotion. The goal is to prove real code against a real (small) corpus on every retrieval-touching change, with a loud failure mode when quality regresses.

## Goals

1. New gold-set schema `{id, question, must_contain_cve_ids, must_contain_terms}` with ≥100 hand-curated entries under `tests/gold/questions.json`, alongside a ~50–100 doc security fixture corpus under `tests/gold/corpus/*.md`.
2. Extend `crates/fastrag-eval/` with three new modules: `gold_set.rs` (loader + union-of-top-k scorer), `matrix.rs` (4-variant orchestrator), `baseline.rs` (checked-in JSON diff + slack gate).
3. Instrument `crates/fastrag/src/corpus/mod.rs` with a `LatencyBreakdown` struct threaded through `query_corpus` to capture per-stage microseconds (embed / BM25 / HNSW / rerank / fuse). Callers that don't care pass a default and ignore the result.
4. Extend `fastrag eval` CLI with `--gold-set`, `--corpus-no-contextual`, `--config-matrix`, `--baseline` flags. Existing `--dataset-name` BEIR path keeps working.
5. Ship a checked-in baseline at `docs/eval-baselines/current.json`. A weekly CI job fails on any hit@5 or MRR@10 regression beyond 2% slack. Baseline refreshes are deliberate human commits.
6. New weekly workflow at `.github/workflows/weekly.yml` with a hard 45-minute timeout, gated on a 7-day `check-changes` proxy. Runs on Sundays at 06:00 UTC. Manual `workflow_dispatch` escape hatch included.
7. Push CI gains a gold-set validation canary that loads `tests/gold/questions.json` and asserts schema validity.

## Non-goals

- Generation, groundedness, refusal-rate metrics. No generation step exists in fastrag — nothing to measure.
- Automatic baseline promotion from CI. Baseline refreshes are committed by a human after reviewing the diff.
- BEIR config matrix. BEIR qrels don't carry CVE-ID or must-contain-term assertions; the matrix is gold-set only. BEIR runs continue to work via the existing `--dataset-name` path.
- Per-question regression gating. The gate fires on aggregate hit@5 and MRR@10 only. Per-question drift is visible in the JSON report for post-mortem but does not fail the build.
- Partial/streaming reports. The matrix report is all-or-nothing — if any variant errors, no JSON is written.
- Push CI gating on real eval. Push CI gets the gold-set schema canary only. The full matrix is weekly.
- New eval datasets beyond the hand-curated gold set. NVD / KEV / CWE loaders already exist in `fastrag-eval` and keep working via `--dataset-name`.

## Constraints

- **CI minute budget.** Weekly eval must stay under ~45 minutes wall-clock on `ubuntu-latest`. At 4 runs/month this consumes ~180 minutes/month — under 10% of the GitHub free tier.
- **Real stack only.** Eval must run the real embedder, real reranker, real hybrid retrieval, real contextualizer. Shim lesson #2 (tarmo-llm-rag): stubbed eval stacks can't catch ingest/query divergence. No mocks in the CI path.
- **Loud failure mode.** Any regression beyond slack fails the build red. No yellow warning tier. No silent skips when a run aborts.
- **Local-first baseline refresh.** The promotion flow must be runnable on a developer machine (`fastrag eval … --report current.json && git commit`). CI does not write back to the repo.
- **Pure-function scoring.** Gold-set scoring takes chunks in and returns a score struct — no I/O, no hidden state, trivial to unit test.

## Architecture

### File structure

```
crates/fastrag-eval/
  src/
    lib.rs              — existing; add re-exports for new modules
    dataset.rs          — existing (BEIR: EvalDocument, EvalQuery, Qrel)
    metrics.rs          — existing (recall@k, MRR, nDCG)
    runner.rs           — existing; extend with ScoringStrategy + LatencyBreakdown recording
    report.rs           — existing; add MatrixReport writer
    error.rs            — existing; add GoldSet + Matrix + Baseline variants
    gold_set.rs         — NEW: loader + union-of-top-k scorer
    matrix.rs           — NEW: 4-variant orchestrator
    baseline.rs         — NEW: JSON diff + 2% slack gate
  tests/
    gold_set_loader.rs  — NEW
    union_match.rs      — NEW
    baseline_diff.rs    — NEW
    matrix_stub.rs      — NEW

tests/gold/                     — NEW, workspace-level fixture
  corpus/
    01-libfoo-rce.md
    02-kev-bluekeep.md
    ...                         — 50–100 handwritten security docs
  questions.json                — 100+ hand-curated questions

docs/eval-baselines/
  current.json                  — NEW: checked-in approved baseline
  README.md                     — NEW: refresh + approval flow

fastrag-cli/
  src/main.rs                   — extend `fastrag eval` with new flags
  tests/
    eval_matrix_e2e.rs          — NEW, ignored, gated on FASTRAG_LLAMA_TEST + FASTRAG_RERANK_TEST
    eval_gold_set_rejects_invalid_e2e.rs  — NEW, no model cost
    fixtures/eval_mini/         — NEW, 5-doc / 10-question mini fixture for e2e

.github/workflows/weekly.yml    — NEW workflow
```

### Per-stage latency instrumentation

Touchpoints:

```
crates/fastrag/src/corpus/mod.rs       — thread LatencyBreakdown through query_corpus
crates/fastrag-tantivy/src/lib.rs      — time BM25 stage (no API change, internal Instant)
crates/fastrag-rerank/src/lib.rs       — time rerank stage (internal Instant)
crates/fastrag-embed/src/lib.rs        — time query embedding (internal Instant)
crates/fastrag-index/src/hnsw.rs       — time HNSW stage (internal Instant)
```

Only `query_corpus` gains a new parameter. The per-crate stages return their elapsed microseconds via the existing call chain by writing into the `&mut LatencyBreakdown` passed from above. CLI `query`, HTTP `/query`, and MCP `search_corpus` pass `&mut LatencyBreakdown::default()` and ignore the result — five `Instant::now()` calls per query.

### Core data types

```rust
// gold_set.rs
pub struct GoldSet {
    pub version: u32,
    pub entries: Vec<GoldSetEntry>,
}

pub struct GoldSetEntry {
    pub id: String,
    pub question: String,
    pub must_contain_cve_ids: Vec<String>,
    pub must_contain_terms: Vec<String>,
    pub notes: Option<String>,
}

pub struct EntryScore {
    pub hit_at_1: bool,
    pub hit_at_5: bool,
    pub hit_at_10: bool,
    pub reciprocal_rank: f64,
    pub missing_cve_ids: Vec<String>,
    pub missing_terms: Vec<String>,
}

// matrix.rs
pub enum ConfigVariant {
    Primary,          // hybrid + rerank + contextual
    NoRerank,         // hybrid + contextual, rerank off
    NoContextual,     // hybrid + rerank, contextual off (raw corpus)
    DenseOnly,        // dense-only, rerank on, contextual on
}

pub struct MatrixReport {
    pub schema_version: u32,
    pub git_rev: String,
    pub captured_at: String,
    pub manifest_identity: EmbedderIdentity,
    pub contextualizer: Option<ContextualizerBlock>,
    pub runs: Vec<VariantReport>,
    pub rerank_delta: f64,        // Primary.hit@5 − NoRerank.hit@5
    pub contextual_delta: f64,    // Primary.hit@5 − NoContextual.hit@5
    pub hybrid_delta: f64,        // Primary.hit@5 − DenseOnly.hit@5
    pub cache_delta: CacheDelta,
}

pub struct VariantReport {
    pub variant: ConfigVariant,
    pub hit_at_1: f64,
    pub hit_at_5: f64,
    pub hit_at_10: f64,
    pub mrr_at_10: f64,
    pub latency: LatencyPercentiles,
    pub per_question: Vec<QuestionResult>,
}

pub struct LatencyPercentiles {
    pub total: Percentiles,
    pub embed: Percentiles,
    pub bm25: Percentiles,
    pub hnsw: Percentiles,
    pub rerank: Percentiles,
    pub fuse: Percentiles,
}

pub struct Percentiles {
    pub p50_us: u64,
    pub p95_us: u64,
    pub p99_us: u64,
}

// corpus/mod.rs
pub struct LatencyBreakdown {
    pub embed_us: u64,
    pub bm25_us: u64,
    pub hnsw_us: u64,
    pub rerank_us: u64,
    pub fuse_us: u64,
    pub total_us: u64,
}

// baseline.rs
pub struct Baseline {
    pub schema_version: u32,
    pub git_rev: String,
    pub captured_at: String,
    pub runs: Vec<VariantBaseline>,
}

pub struct VariantBaseline {
    pub variant: ConfigVariant,
    pub hit_at_5: f64,
    pub mrr_at_10: f64,
}

pub struct BaselineDiff {
    pub regressions: Vec<Regression>,
}

pub struct Regression {
    pub variant: ConfigVariant,
    pub metric: &'static str,
    pub baseline: f64,
    pub current: f64,
    pub delta: f64,
    pub slack: f64,
}
```

### CLI surface

```
fastrag eval --gold-set tests/gold/questions.json \
             --corpus <ctx-corpus> \
             --corpus-no-contextual <raw-corpus> \
             --config-matrix \
             --baseline docs/eval-baselines/current.json \
             --report target/eval/report.json
```

New flags on the existing `eval` subcommand:

- `--gold-set <path>` — gold-set JSON loader. Mutually exclusive with `--dataset-name`.
- `--corpus <path>` — built corpus (contextualized if `--config-matrix`).
- `--corpus-no-contextual <path>` — second corpus for the NoContextual variant. Required when `--config-matrix` is set; validated at CLI parse time.
- `--config-matrix` — run all 4 variants. Requires `--gold-set`.
- `--baseline <path>` — compare against JSON baseline; non-zero exit on >2% regression.

Existing `--dataset-name` / `--dataset` / `--report` / `--embedder` / `--top-k` / `--max-docs` / `--max-queries` flags unchanged.

## Data flow

### Weekly CI run

```
.github/workflows/weekly.yml::eval
  ├─ checkout + cache GGUFs + cache ONNX reranker
  ├─ install llama-server (b8739, same tag as nightly jobs)
  │
  ├─ BUILD PHASE — two corpora from tests/gold/corpus/
  │   ├─ fastrag index tests/gold/corpus \
  │   │     --corpus $TMP/corpus-ctx \
  │   │     --embedder qwen3-q8 --contextualize
  │   └─ fastrag index tests/gold/corpus \
  │         --corpus $TMP/corpus-raw \
  │         --embedder qwen3-q8
  │
  ├─ EVAL PHASE — single invocation
  │   fastrag eval \
  │     --gold-set tests/gold/questions.json \
  │     --corpus $TMP/corpus-ctx \
  │     --corpus-no-contextual $TMP/corpus-raw \
  │     --config-matrix \
  │     --baseline docs/eval-baselines/current.json \
  │     --report $TMP/report.json
  │
  ├─ UPLOAD — $TMP/report.json as artifact (if: always())
  └─ GATE — non-zero exit from baseline diff fails the job
```

### Inside `fastrag eval --config-matrix`

```
main.rs::eval_cmd
  load GoldSet from --gold-set and validate at load time
  load Baseline from --baseline (if set)
  open ctx_corpus and raw_corpus
  snapshot cache_stats_start on both corpora

  for variant in [Primary, NoRerank, NoContextual, DenseOnly]:
      corpus = match variant {
          NoContextual => raw_corpus,
          _ => ctx_corpus,
      };
      query_opts = variant.to_query_options();
      variant_report = run_variant(corpus, query_opts, &gold_set)?;
      matrix_report.runs.push(variant_report);

  snapshot cache_stats_end
  matrix_report.cache_delta = end - start
  matrix_report.rerank_delta = hit5(Primary) - hit5(NoRerank)
  matrix_report.contextual_delta = hit5(Primary) - hit5(NoContextual)
  matrix_report.hybrid_delta = hit5(Primary) - hit5(DenseOnly)

  write matrix_report → --report path

  if let Some(baseline) = baseline {
      diff = baseline::diff(&matrix_report, &baseline);
      eprintln!("{}", diff.render_report());
      if diff.has_regressions() { exit(1); }
  }
```

### Per-variant run

```
run_variant(corpus, query_opts, gold_set) -> VariantReport
  init histograms: total, embed, bm25, hnsw, rerank, fuse (hdrhistogram, max 60_000_000 us)

  for entry in gold_set.entries():
      let mut breakdown = LatencyBreakdown::default();
      let hits = query_corpus(corpus, &entry.question, top_k=10, query_opts, &mut breakdown)?;
      let top_k_texts: Vec<&str> = hits.iter().map(|h| h.raw_text.as_str()).collect();
      let entry_score = gold_set::score_entry(entry, &top_k_texts);

      total_histogram.record(breakdown.total_us);
      embed_histogram.record(breakdown.embed_us);
      bm25_histogram.record(breakdown.bm25_us);
      hnsw_histogram.record(breakdown.hnsw_us);
      rerank_histogram.record(breakdown.rerank_us);
      fuse_histogram.record(breakdown.fuse_us);

      per_question.push(QuestionResult {
          id: entry.id.clone(),
          hit_at_1: entry_score.hit_at_1,
          hit_at_5: entry_score.hit_at_5,
          hit_at_10: entry_score.hit_at_10,
          reciprocal_rank: entry_score.reciprocal_rank,
          missing_cve_ids: entry_score.missing_cve_ids,
          missing_terms: entry_score.missing_terms,
          latency: breakdown,
      });

  VariantReport {
      variant,
      hit_at_1: mean(per_question.hit_at_1),
      hit_at_5: mean(per_question.hit_at_5),
      hit_at_10: mean(per_question.hit_at_10),
      mrr_at_10: mean(per_question.reciprocal_rank),
      latency: LatencyPercentiles { ... },
      per_question,
  }
```

Run order is Primary → NoRerank → NoContextual → DenseOnly. Sequential, not parallel — keeps per-stage latency numbers from contaminating each other on a shared machine.

### Union-of-top-k hit semantics

Given an entry with `must_contain_cve_ids: [A, B]` and `must_contain_terms: [X, Y]`, and a ranked result `[chunk0, chunk1, chunk2, ...]`:

1. For each `k` in `[1, 5, 10]`, concatenate `chunk[0..k]` into one buffer.
2. CVE matching: run `(?i)CVE-\d{4}-\d+` against the buffer. Every id in `must_contain_cve_ids` must appear.
3. Term matching: case-insensitive substring match per term. Every term in `must_contain_terms` must appear.
4. `hit_at_k = true` iff all assertions satisfied at that `k`.
5. Reciprocal rank: smallest `k` in `[1..=10]` at which all assertions satisfy; `1/k`. Zero if never satisfied.

### Baseline diff — concrete walk

```
baseline: {
  Primary:      { hit_at_5: 0.82, mrr_at_10: 0.71 },
  NoRerank:     { hit_at_5: 0.74, mrr_at_10: 0.63 },
  NoContextual: { hit_at_5: 0.71, mrr_at_10: 0.60 },
  DenseOnly:    { hit_at_5: 0.65, mrr_at_10: 0.55 },
}

fresh matrix_report:
  Primary.hit_at_5  = 0.83 → threshold 0.82 * 0.98 = 0.8036 → 0.83 ≥ threshold → pass
  Primary.mrr_at_10 = 0.69 → threshold 0.71 * 0.98 = 0.6958 → 0.69 < threshold → REGRESSION
  NoRerank.hit_at_5 = 0.75 → pass
  ...

diff returns BaselineDiff {
  regressions: [
    Regression {
      variant: Primary, metric: "MRR@10",
      baseline: 0.71, current: 0.69, delta: -0.02, slack: 0.02,
    },
  ],
}

render_report() →
  "## Baseline regressions (1)
   - Primary MRR@10: 0.71 → 0.69 (−2.8%, slack ±2%)"

exit(1)
```

### Baseline refresh flow

```
# Run locally with the real stack
fastrag eval --gold-set tests/gold/questions.json \
             --corpus ~/corpora/gold-ctx \
             --corpus-no-contextual ~/corpora/gold-raw \
             --config-matrix \
             --report docs/eval-baselines/current.json

# Review the diff
git diff docs/eval-baselines/current.json

# Commit in the same PR as the improvement that earned the new numbers
git add docs/eval-baselines/current.json
git commit -m "eval: refresh baseline after <improvement>"
```

CI does not push back to the repo.

## CI wiring

### `.github/workflows/weekly.yml` (new)

```yaml
name: Weekly (eval harness)

on:
  schedule:
    - cron: "0 6 * * 0"   # Sundays 06:00 UTC
  workflow_dispatch:

env:
  CARGO_TERM_COLOR: always

jobs:
  check-changes:
    runs-on: ubuntu-latest
    if: github.event_name == 'workflow_dispatch' || github.event_name == 'schedule'
    outputs:
      has_changes: ${{ github.event_name == 'workflow_dispatch' || steps.check.outputs.has_changes }}
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0
      - name: Check for commits in last 7 days
        id: check
        run: |
          if git log --since="7 days ago" --oneline | grep -q .; then
            echo "has_changes=true" >> "$GITHUB_OUTPUT"
          else
            echo "has_changes=false" >> "$GITHUB_OUTPUT"
            echo "No commits in last 7 days — skipping weekly eval."
          fi

  eval:
    needs: check-changes
    if: needs.check-changes.outputs.has_changes == 'true'
    runs-on: ubuntu-latest
    timeout-minutes: 45
    env:
      FASTRAG_LLAMA_TEST: "1"
      FASTRAG_RERANK_TEST: "1"
    steps:
      # install llama-server, cache GGUFs + ONNX reranker,
      # build both corpora, run fastrag eval --config-matrix --baseline,
      # upload $TMP/report.json as artifact (if: always()),
      # non-zero exit on regression
```

### Push CI additions

One canary test in push CI:

```rust
#[test]
fn tests_gold_questions_json_is_valid() {
    let path = workspace_root().join("tests/gold/questions.json");
    let gs = gold_set::load(&path).expect("tests/gold/questions.json must validate");
    assert!(gs.entries.len() >= 100);
}
```

Runs as part of `cargo test --workspace`. Catches malformed gold-set commits before the weekly run sees them.

### Budget math

| Cadence | Minutes/run | Runs/month | Minutes/month |
|---|---|---|---|
| Nightly | ~45 | ~30 | ~1,350 |
| **Weekly** | **~45** | **~4** | **~180** |

GitHub free tier is 2,000 minutes/month. Weekly keeps eval under 10% of the budget.

## Error handling

### Error taxonomy

```rust
#[derive(thiserror::Error, Debug)]
pub enum EvalError {
    // existing
    #[error("dataset error: {0}")]
    Dataset(String),
    #[error("runner error: {0}")]
    Runner(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    // new — gold set
    #[error("gold set parse error at {path}: {source}")]
    GoldSetParse { path: PathBuf, source: serde_json::Error },
    #[error("gold set validation failed: {0}")]
    GoldSetInvalid(String),

    // new — matrix
    #[error("matrix variant {variant:?} failed: {source}")]
    MatrixVariant { variant: ConfigVariant, source: Box<EvalError> },
    #[error("--config-matrix requires --gold-set")]
    MatrixRequiresGoldSet,
    #[error("--config-matrix requires --corpus-no-contextual")]
    MatrixMissingRawCorpus,

    // new — baseline
    #[error("baseline load error at {path}: {source}")]
    BaselineLoad { path: PathBuf, source: serde_json::Error },
    #[error("baseline schema mismatch: baseline covers {baseline:?}, report has {report:?}")]
    BaselineSchemaMismatch { baseline: Vec<ConfigVariant>, report: Vec<ConfigVariant> },

    // new — latency
    #[error("histogram record error: {0}")]
    Histogram(#[from] hdrhistogram::errors::RecordError),
}
```

### Behaviors

- **Gold-set validation errors** — surfaced at load time before any corpus is opened, with entry id in the message. Examples: empty question, duplicate id, malformed CVE regex, zero assertions (would trivially hit@1).
- **Missing `--corpus-no-contextual` in matrix mode** — validated at CLI parse time via `MatrixMissingRawCorpus`. Clear message tells the user to build the second corpus.
- **Manifest version mismatch** — `HnswIndex::load` returns Step 5's rebuild-message error; matrix runner wraps it in `MatrixVariant` so the CI log shows which variant was loading.
- **Per-variant failure** — matrix aborts with `MatrixVariant`. No partial report is written. The JSON is all-or-nothing.
- **Per-query failure** — `query_corpus` returning an error is a retrieval-stack bug. The eval aborts with the question id in the error context for reproduction.
- **Empty top-k result** — distinct from a query error. Scored as a miss (`hit_at_k = false`, `RR = 0`), logged at `debug!`, run continues.
- **Baseline file missing** — hard error. Opting into `--baseline` means the gate is expected.
- **Baseline schema mismatch** — `BaselineSchemaMismatch` with refresh instructions. No automatic migration.
- **Histogram overflow** — values past 60,000,000 us (60 s) clamp and log at `warn!` with the question id. A single query over 60 s is a canary that deserves attention, not a hard failure.
- **CI timeout (45 min)** — GitHub kills the job. No report. Investigate via the Actions log — per-phase timing is printed by the eval binary so you can tell which corpus build or variant ran long.

### Explicit non-behaviors

- No automatic baseline promotion.
- No retry on per-query errors.
- No partial matrix report on abort.
- No regression warning tier — metrics either regress beyond slack (fail) or they don't (pass).

## Testing

### Unit tests (`#[cfg(test)]`, in-file)

**`gold_set.rs`** — load validation (empty question, duplicate id, malformed CVE, zero assertions), `score_entry` hit semantics (single-chunk hit, multi-chunk union, case-insensitive term match, CVE regex match, total miss), purity (same inputs → same outputs).

**`matrix.rs`** — `ConfigVariant::to_query_options` tuple check per variant, `run_matrix` with stub corpus runs all 4 variants in order, error propagation via `MatrixVariant`, `MatrixReport::render_summary` golden-tested against a checked-in string.

**`baseline.rs`** — exact-match report (zero regressions), boundary test (exactly 2% drop = pass, 2.01% drop = one regression), multiple regressions sorted by variant, schema mismatch fails hard, `render_report` snapshot test.

**Latency histograms** — `u64::MAX` input clamps to max and warns, `LatencyBreakdown::default()` zero-initialized, percentile extraction matches pre-computed values.

### Integration tests (`crates/fastrag-eval/tests/`)

- `gold_set_loader.rs` — real JSON fixture covering every validation branch with explicit error-message assertions.
- `union_match.rs` — synthetic chunks driving `score_entry` through two-chunk union, three-chunk union, pronoun-resolution miss shape.
- `baseline_diff.rs` — checked-in baseline + "good run" + "bad run" JSON fixtures; asserts good passes, bad produces the exact regression set. Catches serde drift.
- `matrix_stub.rs` — stub `Corpus` returning canned top-k chunks by question id. Drives `run_matrix` through all 4 variants at zero model cost. Asserts order, histogram recording counts, delta computation timing.

### E2E tests (`fastrag-cli/tests/`, ignored, gated on `FASTRAG_LLAMA_TEST=1` + `FASTRAG_RERANK_TEST=1`)

**`eval_matrix_e2e.rs`** — runs against `fastrag-cli/tests/fixtures/eval_mini/` (5 docs + 10 questions, distinct from the full `tests/gold/` fixture — the e2e must finish in ~5 min). Flow:

1. Build `ctx_corpus` with `--contextualize`.
2. Build `raw_corpus` without.
3. Run `fastrag eval --config-matrix --report <tmp>`.
4. Parse the JSON; assert:
   - All 4 variants present
   - Every `hit_at_5` is a finite f64 in `[0.0, 1.0]`
   - `rerank_delta`, `contextual_delta`, `hybrid_delta` populated
   - Each per-stage histogram recorded exactly `gold_set.entries().len() * 4` times
   - `cache_delta` present

Baseline gate is not exercised on the mini fixture (numbers too small to stabilize). `baseline_diff.rs` covers that path with checked-in fixtures.

**`eval_gold_set_rejects_invalid_e2e.rs`** — runs `fastrag eval --gold-set <invalid.json>` and asserts non-zero exit with the offending entry id in the error message.

### Push CI canary

`tests_gold_questions_json_is_valid` — loads `tests/gold/questions.json` in a push-CI unit test and runs validation. Guarantees the committed fixture stays well-formed regardless of weekly cadence.

### Explicit non-tests

- **No test asserting Primary > DenseOnly on the fixture.** Ordering is a quality claim enforced by the baseline gate, not a hard-coded invariant.
- **No test asserting specific hit@5 values on the fixture.** Those live in `docs/eval-baselines/current.json`, not test code. Tests assert the shape of the report, not its values.
- **No mocked embedder / reranker in e2e.** Shim lesson #2 — mocked stacks miss ingest/query divergence. E2e uses the real Qwen3 embedder + real gte-reranker-modernbert against the mini fixture.
- **No rubber-stamp tests.** Every assertion must fail if the implementation is broken or no-op. The per-stage latency count assertion is the canary that catches a variant silently skipping its run.
- **No generation / groundedness / refusal tests.** No generation step exists.

## Rollout

1. Land `gold_set.rs` + unit tests + push-CI canary. Requires the gold-set JSON fixture to exist — write a minimal ~10-entry starter fixture first, then grow to 100+ in a later commit once the loader is proven.
2. Land `LatencyBreakdown` instrumentation + `query_corpus` signature change. All existing callers updated to pass `&mut LatencyBreakdown::default()`. Retrieval tests green.
3. Land `matrix.rs` + `matrix_stub.rs` integration test. No real-model cost yet.
4. Land `baseline.rs` + `baseline_diff.rs` integration test. Checked-in report fixtures.
5. Land CLI wiring on `fastrag eval`. Mini-fixture + `eval_matrix_e2e.rs` under FASTRAG_LLAMA_TEST gating.
6. Grow `tests/gold/questions.json` to ≥100 entries and `tests/gold/corpus/` to ~50–100 docs. Commit `docs/eval-baselines/current.json` captured from a local run.
7. Land `.github/workflows/weekly.yml`. Update `CLAUDE.md` Build & Test section. Update `README.md` with an Eval section pointing at the new flags and the baseline refresh flow.

Each landing is a separate commit on `main` in the listed order. No worktrees. `cargo test`, `cargo clippy`, `cargo fmt` gates run locally before every push.

## Open questions for the implementation plan

1. Does `query_corpus` currently take `QueryOptions` or separate flag args? The signature change in Rollout step 2 must match today's API, not the spec's shorthand.
2. The existing `fastrag-eval::runner` already owns its own histogram for total latency. Does the new per-stage histogram set live alongside it or replace it? Leaning toward alongside — total stays as the primary headline metric, per-stage is the drill-down.
3. Is `EmbedderIdentity` already serializable to JSON, or does the report need a manual serialization helper? Expected to be serializable given Step 1's manifest work, but worth verifying.
4. What's the exact `QuestionResult::latency` shape in the JSON — flat or nested? Flat (one field per stage) matches the `LatencyBreakdown` struct; nested matches the `LatencyPercentiles` shape. Pick one for consistency.

These resolve in the writing-plans phase, not here.
