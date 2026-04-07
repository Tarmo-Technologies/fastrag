#!/usr/bin/env bash
# Reproduce the fastrag Phase 2 v0 retrieval baselines.
#
# Runs `fastrag eval` against each built-in dataset (loaders cache downloads
# under XDG_CACHE_HOME) and writes JSON reports into a baseline directory.
# Every Phase 2 PR after #25 is diffed against this snapshot.
#
# Usage:
#   scripts/run-eval.sh                              # full baseline
#   scripts/run-eval.sh --out /tmp/eval-smoke \
#       --datasets nfcorpus                          # single-dataset smoke
#   scripts/run-eval.sh --embedder bge-small \
#       --chunking basic --top-k 20

set -euo pipefail

OUT="eval-baselines/v0-phase1"
EMBEDDER="bge-small"
CHUNKING="basic"
CHUNK_SIZE=1000
TOP_K=20
DATASETS="nfcorpus,scifact,nvd,cwe"
# Sampled baselines: candle's pure-CPU BGE forward is the bottleneck on this hardware,
# so v0-phase1 freezes a deterministic *sample* of each dataset. Numbers diff against
# this sample, not the published full-dataset numbers; full baselines wait on embedder
# perf work (tracked separately). Pass --max-docs 0 / --max-queries 0 for full runs.
MAX_DOCS=500
MAX_QUERIES=50

while [[ $# -gt 0 ]]; do
    case "$1" in
        --out) OUT="$2"; shift 2;;
        --embedder) EMBEDDER="$2"; shift 2;;
        --chunking) CHUNKING="$2"; shift 2;;
        --chunk-size) CHUNK_SIZE="$2"; shift 2;;
        --top-k) TOP_K="$2"; shift 2;;
        --datasets) DATASETS="$2"; shift 2;;
        --max-docs) MAX_DOCS="$2"; shift 2;;
        --max-queries) MAX_QUERIES="$2"; shift 2;;
        -h|--help)
            sed -n '2,15p' "$0"
            exit 0
            ;;
        *) echo "unknown flag: $1" >&2; exit 2;;
    esac
done

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT"

GIT_REV="$(git rev-parse --short=12 HEAD 2>/dev/null || echo unknown)"
RUSTC_VERSION="$(rustc --version)"
export FASTRAG_GIT_REV="$GIT_REV"

mkdir -p "$OUT"

echo "==> Building fastrag (release, --features eval)"
cargo build --release -p fastrag-cli --features eval >/dev/null

BIN="$REPO_ROOT/target/release/fastrag"

IFS=',' read -r -a DATASET_ARR <<< "$DATASETS"
for ds in "${DATASET_ARR[@]}"; do
    report="$OUT/${ds}.json"
    echo "==> [$ds] running eval -> $report"
    sample_args=()
    if [[ "$MAX_DOCS" != "0" ]]; then
        sample_args+=(--max-docs "$MAX_DOCS")
    fi
    if [[ "$MAX_QUERIES" != "0" ]]; then
        sample_args+=(--max-queries "$MAX_QUERIES")
    fi
    "$BIN" eval \
        --dataset-name "$ds" \
        --report "$report" \
        --embedder "$EMBEDDER" \
        --chunking "$CHUNKING" \
        --chunk-size "$CHUNK_SIZE" \
        --top-k "$TOP_K" \
        "${sample_args[@]}"

    # One-line summary so the user sees progress without re-reading JSON.
    python3 - "$report" <<'PY'
import json, sys
r = json.load(open(sys.argv[1]))
m = r["metrics"]
lat = r["latency"]
print(f"    recall@10={m.get('recall@10', 0):.4f}  ndcg@10={m.get('ndcg@10', 0):.4f}  "
      f"mrr@10={m.get('mrr@10', 0):.4f}  p95={lat['p95_ms']:.1f}ms  "
      f"build={r['build_time_ms']}ms  peak_rss={r['memory']['peak_rss_bytes']/1e6:.0f}MB")
PY
done

cat <<INFO

==> Baseline run complete
    out         : $OUT
    git_rev     : $GIT_REV
    embedder    : $EMBEDDER
    chunking    : $CHUNKING (chunk_size=$CHUNK_SIZE)
    top_k       : $TOP_K
    rustc       : $RUSTC_VERSION
INFO
