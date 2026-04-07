# Retrieval Evaluation Baselines

`fastrag-eval` writes JSON reports under `eval-baselines/v0-phase1/`. They are the
immutable Phase 2 starting point — every later change (auth, filtering, rerank, chunking
tuning, embedder swaps) is diffed against these numbers. Regenerate with
`scripts/run-eval.sh`.

## v0-phase1 — sampled, BGE-small / basic chunking

- Embedder: `BAAI/bge-small-en-v1.5` (candle CPU, no MKL)
- Chunking: `basic(max_characters=1000, overlap=0)`
- top-k: 20
- Sample: `--max-docs 500 --max-queries 50` (see `eval-baselines/v0-phase1/README.md`)
- git rev: `21f7f2db2885`
- rustc: `1.94.0`

| Dataset  | Docs | Queries | recall@1 | recall@5 | recall@10 | recall@20 | MRR@10 | nDCG@10 | p50 ms | p95 ms | p99 ms | build s | peak RSS MB |
|----------|-----:|--------:|---------:|---------:|----------:|----------:|-------:|--------:|-------:|-------:|-------:|--------:|------------:|
| nfcorpus |  500 |      50 |    0.053 |    0.122 |     0.128 |     0.129 |  0.133 |   0.109 |   12.3 |   26.1 |   28.9 |     390 |         188 |
| scifact  |  500 |      50 |    0.040 |    0.060 |     0.080 |     0.080 |  0.050 |   0.057 |   19.8 |   46.2 |   58.2 |     327 |         160 |
| nvd      |  500 |     n/a |        — |        — |         — |         — |      — |       — |      — |      — |      — |     187 |         283 |
| cwe      |  full |     n/a |       — |        — |         — |         — |      — |       — |      — |      — |      — |       8 |         151 |

NVD and CWE are corpus-only in v0 — neither built-in loader ships queries. They lock in
**index footprint** (build time, peak RSS) only; retrieval metrics land in v1 once the
NVD security query set is checked in.

## Sanity check

NFCorpus / SciFact recall@10 are well below published BGE-small numbers (~0.30 / ~0.65)
because we're sampling 500 docs out of 3 600 / 5 100. Most relevant docs for each
sampled query are *not* in the truncated index, so recall is bounded above by the qrels
intersection. The numbers are diff-able as a deterministic snapshot — they are **not**
comparable to full-corpus published results.

Closing the gap to published numbers requires either:

1. embedder perf work (sorted-length batching or candle MKL feature) so full corpora run
   in minutes instead of hours, then re-baseline as v1; or
2. running full baselines once on a faster box and committing them as v1.

Either path is tracked as a follow-up to #25.

## Reproducing

```bash
# Default — sampled v0 baselines, ~25 minutes on a CPU box
scripts/run-eval.sh

# Single dataset, custom output
scripts/run-eval.sh --out /tmp/eval-cmp --datasets nfcorpus

# Full datasets (slow on CPU; see embedder perf follow-up)
scripts/run-eval.sh --max-docs 0 --max-queries 0
```

Each run writes one JSON per dataset under `--out`. The report files include
`schema_version`, `git_rev`, `fastrag_version`, and `top_k` so they are self-describing
across machines.
