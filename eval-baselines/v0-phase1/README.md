# v0-phase1 retrieval baselines

Frozen reference numbers for fastrag at the start of Phase 2 (post-#23/#24, pre-#26).
**Do not hand-edit.** Regenerate with:

```bash
scripts/run-eval.sh
```

## Why these are sampled, not full

Phase 2 Step 3 (#25) targets a deterministic, diff-able snapshot. The current embedder
(BGE-small via candle, pure CPU, no MKL/BLAS acceleration) takes roughly 45 minutes per
3 600-doc BEIR run on the dev box, so the v0 snapshot is locked at a deterministic
sample (`--max-docs 500 --max-queries 50`) instead of the full corpus. The same sample
is used by every later run, so quality regressions are still detectable — just against
the sample, not against published BGE-small numbers.

Full-dataset baselines wait on embedder perf work (sorted-length batching or candle
MKL feature). Tracked as a follow-up; do not bypass by editing this directory.

| Dataset  | Source         | Sample           | What is measured |
|----------|----------------|------------------|------------------|
| nfcorpus | BEIR           | 500 docs / 50 q  | recall, MRR, nDCG, latency, RSS |
| scifact  | BEIR           | 500 docs / 50 q  | recall, MRR, nDCG, latency, RSS |
| nvd      | NIST 2023+2024 | 500 docs (corpus only) | index footprint only — built-in `load_nvd` has no security query set |
| cwe      | MITRE Top 25   | full (corpus only) | index footprint only — dataset has no queries |

The NVD security query set ships separately (`load_nvd_corpus_with_queries`); a follow-up
issue will land a checked-in security-queries JSON so NVD baselines include retrieval
metrics in v1.

## Schema

Each `*.json` is a `fastrag-eval::EvalReport` envelope with `schema_version: 2`,
`git_rev`, `fastrag_version`, and `top_k` recorded so the file is self-describing.

## How to diff

`scripts/run-eval.sh --out /tmp/eval-cmp` writes a fresh set of reports. A future CI
job will diff `metrics["recall@10"]`, `metrics["ndcg@10"]`, `latency.p95_ms`, and
`memory.peak_rss_bytes` against this directory and fail on >2 % regression.
