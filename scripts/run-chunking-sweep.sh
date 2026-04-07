#!/usr/bin/env bash
# Eval-driven chunking strategy sweep for fastrag (#30).
#
# Iterates strategy × chunk_size × chunk_overlap and writes one report JSON
# per combination into $OUT, then emits a summary table on stdout.
#
# WARNING: full sweep is 4 strategies × 4 sizes × 3 overlaps × 2 datasets =
# 96 runs. On the v0-phase1 reference box (pure-CPU candle, BGE-small) one
# nfcorpus 500/50 sample run takes ~5 minutes — full sweep ≈ 8 hours. Use
# --strategies / --sizes / --overlaps / --datasets to scope.
#
# Usage:
#   scripts/run-chunking-sweep.sh                     # full sweep
#   scripts/run-chunking-sweep.sh \
#       --strategies basic --sizes 500,1000,1500 \
#       --overlaps 0,200 --datasets nfcorpus         # focused

set -euo pipefail

OUT="docs/chunking-sweep"
STRATEGIES="basic,by-title,recursive"
SIZES="500,800,1000,1500"
OVERLAPS="0,100,200"
DATASETS="nfcorpus,scifact"
EMBEDDER="bge-small"
TOP_K=20
MAX_DOCS=500
MAX_QUERIES=50

while [[ $# -gt 0 ]]; do
    case "$1" in
        --out) OUT="$2"; shift 2;;
        --strategies) STRATEGIES="$2"; shift 2;;
        --sizes) SIZES="$2"; shift 2;;
        --overlaps) OVERLAPS="$2"; shift 2;;
        --datasets) DATASETS="$2"; shift 2;;
        --embedder) EMBEDDER="$2"; shift 2;;
        --top-k) TOP_K="$2"; shift 2;;
        --max-docs) MAX_DOCS="$2"; shift 2;;
        --max-queries) MAX_QUERIES="$2"; shift 2;;
        -h|--help) sed -n '2,18p' "$0"; exit 0;;
        *) echo "unknown flag: $1" >&2; exit 2;;
    esac
done

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT"

GIT_REV="$(git rev-parse --short=12 HEAD 2>/dev/null || echo unknown)"
export FASTRAG_GIT_REV="$GIT_REV"

mkdir -p "$OUT"
echo "==> Building fastrag (release, --features eval)"
cargo build --release -p fastrag-cli --features eval >/dev/null
BIN="$REPO_ROOT/target/release/fastrag"

IFS=',' read -r -a STRATS <<< "$STRATEGIES"
IFS=',' read -r -a SIZE_ARR <<< "$SIZES"
IFS=',' read -r -a OV_ARR <<< "$OVERLAPS"
IFS=',' read -r -a DS_ARR <<< "$DATASETS"

SUMMARY="$OUT/summary.tsv"
printf "dataset\tstrategy\tsize\toverlap\trecall@10\tndcg@10\tp95_ms\tbuild_ms\tpeak_rss_mb\n" > "$SUMMARY"

for ds in "${DS_ARR[@]}"; do
    for strat in "${STRATS[@]}"; do
        for size in "${SIZE_ARR[@]}"; do
            for ov in "${OV_ARR[@]}"; do
                tag="${ds}_${strat}_${size}_${ov}"
                report="$OUT/${tag}.json"
                echo "==> [$tag]"
                "$BIN" eval \
                    --dataset-name "$ds" \
                    --report "$report" \
                    --embedder "$EMBEDDER" \
                    --chunking "$strat" \
                    --chunk-size "$size" \
                    --chunk-overlap "$ov" \
                    --top-k "$TOP_K" \
                    --max-docs "$MAX_DOCS" \
                    --max-queries "$MAX_QUERIES"

                python3 - "$report" "$ds" "$strat" "$size" "$ov" "$SUMMARY" <<'PY'
import json, sys
report, ds, strat, size, ov, summary = sys.argv[1:7]
r = json.load(open(report))
m = r["metrics"]
row = f"{ds}\t{strat}\t{size}\t{ov}\t{m.get('recall@10', 0):.4f}\t{m.get('ndcg@10', 0):.4f}\t{r['latency']['p95_ms']:.1f}\t{r['build_time_ms']}\t{r['memory']['peak_rss_bytes']/1e6:.0f}\n"
with open(summary, 'a') as fh:
    fh.write(row)
print("    " + row.strip())
PY
            done
        done
    done
done

echo ""
echo "==> Sweep complete: $SUMMARY"
column -t -s $'\t' "$SUMMARY"
