# Chunking Strategy Sweep (#30)

Eval-driven evaluation of chunking strategy × `max_characters` × `overlap` against
the v0-phase1 baselines. The current default (`Basic`, 1000 chars, 0 overlap) is a
guess; this page tracks the data that justifies the production default.

## Status

**Sweep automation:** ready (`scripts/run-chunking-sweep.sh`).
**Full results:** pending — see "Compute budget" below.

The sweep script writes one report JSON per combination plus a `summary.tsv` table.
Run it locally with:

```bash
scripts/run-chunking-sweep.sh                         # full grid
scripts/run-chunking-sweep.sh \
    --strategies basic,recursive --sizes 500,1000 \
    --overlaps 0,200 --datasets nfcorpus              # focused subset
```

## Sweep grid

| Axis | Values |
|------|--------|
| Strategy | `basic`, `by-title`, `recursive` |
| `max_characters` | 500, 800, 1000, 1500 |
| `overlap` | 0, 100, 200 |
| Datasets | nfcorpus, scifact (sampled 500/50, same as v0-phase1) |
| Top-k | 20 |

`Semantic` chunking is excluded from the v1 sweep — it requires an embedder pass
during chunking and would balloon wall-clock by another order of magnitude on the
CPU box. It will be added once embedder perf work lands.

## Compute budget

The full grid is 3 strategies × 4 sizes × 3 overlaps × 2 datasets = **72 runs**.
On the v0-phase1 reference box (pure-CPU candle, BGE-small), one nfcorpus 500/50
sample run takes ~5 minutes after the sorted-batching speedup (76eb4ca). 72 runs ≈
**6 hours** of wallclock — too long for an interactive session.

Closing the gap requires either:

1. The candle MKL/Accelerate path landing in #34, or
2. A faster reference machine for the sweep.

Either path is tracked in the issue body. Until then, this page documents the
methodology and the script; partial sweeps can be committed under `docs/chunking-sweep/`
as they're produced.

## Results

_Pending. The first row in the table below is the existing v0-phase1 baseline,
included as the control point against which sweep results will be diffed._

| Dataset | Strategy | Size | Overlap | recall@10 | nDCG@10 | p95 ms | build s | peak RSS MB |
|---------|----------|-----:|--------:|----------:|--------:|-------:|--------:|------------:|
| nfcorpus | basic | 1000 | 0 | 0.128 | 0.109 | 26.1 | 390 | 188 |
| scifact  | basic | 1000 | 0 | 0.080 | 0.057 | 46.2 | 327 | 160 |

## Picking the default

Once the sweep finishes, the new default is whichever (strategy, size, overlap)
combination maximizes nDCG@10 averaged across nfcorpus and scifact, subject to:

- Build time ≤ 2× the current baseline
- Peak RSS ≤ 1.5× the current baseline
- Does not regress recall@10 vs baseline on either dataset

The chosen default lands as a follow-up PR that updates `crates/fastrag-cli/src/args.rs`
and adds a CHANGELOG migration note (existing corpora must be reindexed to benefit).
