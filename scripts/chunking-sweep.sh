#!/usr/bin/env bash
set -euo pipefail

# Chunking strategy sweep — runs locally, requires llama-server + GGUF models.
# Usage: ./scripts/chunking-sweep.sh
# Output: target/chunking-sweep/results.tsv

GOLD_CORPUS="tests/gold/corpus"
GOLD_SET="tests/gold/questions.json"
OUT_DIR="target/chunking-sweep"
RESULTS="$OUT_DIR/results.tsv"
MODEL_DIR="${FASTRAG_MODEL_DIR:-$HOME/.cache/fastrag/models}"
MODEL_PATH="${FASTRAG_CHUNKING_MODEL:-$MODEL_DIR/Qwen3-Embedding-0.6B-Q8_0.gguf}"
CONFIG_ROOT="$(mktemp -d -t fastrag-chunking-config-XXXXXX)"
CONFIG_DIR="$CONFIG_ROOT/fastrag"
CONFIG_PATH="$CONFIG_DIR/fastrag.toml"

STRATEGIES=(basic by-title recursive)
SIZES=(500 800 1000 1500)
OVERLAPS=(0 100 200)

mkdir -p "$OUT_DIR"
trap 'rm -rf "$CONFIG_ROOT"' EXIT

if [[ ! -f "$MODEL_PATH" ]]; then
  echo "chunking sweep model not found at $MODEL_PATH" >&2
  echo "Set FASTRAG_CHUNKING_MODEL=/path/to/Qwen3-Embedding-0.6B-Q8_0.gguf or FASTRAG_MODEL_DIR accordingly." >&2
  exit 1
fi

mkdir -p "$CONFIG_DIR"
cat > "$CONFIG_PATH" <<EOF
[embedder]
default_profile = "chunking-sweep"

[embedder.profiles.chunking-sweep]
backend = "llama-cpp"
model = "$MODEL_PATH"
EOF
export XDG_CONFIG_HOME="$CONFIG_ROOT"

printf "strategy\tsize\toverlap\tchunks\tindex_bytes\thit_at_1\thit_at_5\thit_at_10\tmrr_at_10\n" > "$RESULTS"

TOTAL=$(( ${#STRATEGIES[@]} * ${#SIZES[@]} * ${#OVERLAPS[@]} ))
COUNT=0

for strategy in "${STRATEGIES[@]}"; do
  for size in "${SIZES[@]}"; do
    for overlap in "${OVERLAPS[@]}"; do
      COUNT=$((COUNT + 1))
      echo "[$COUNT/$TOTAL] strategy=$strategy size=$size overlap=$overlap"

      CORPUS_DIR="$OUT_DIR/corpus-${strategy}-${size}-${overlap}"
      CTX_DIR="${CORPUS_DIR}-ctx"
      RAW_DIR="${CORPUS_DIR}-raw"

      # Build contextualized corpus
      cargo run --release -p fastrag-cli --bin fastrag \
        --features retrieval,rerank,contextual,contextual-llama -- \
        index "$GOLD_CORPUS" \
        --corpus "$CTX_DIR" \
        --config "$CONFIG_PATH" \
        --embedder-profile chunking-sweep \
        --contextualize \
        --chunk-strategy "$strategy" \
        --chunk-size "$size" \
        --chunk-overlap "$overlap" \
        2>/dev/null

      # Build raw corpus
      cargo run --release -p fastrag-cli --bin fastrag \
        --features retrieval,rerank,contextual,contextual-llama -- \
        index "$GOLD_CORPUS" \
        --corpus "$RAW_DIR" \
        --config "$CONFIG_PATH" \
        --embedder-profile chunking-sweep \
        --chunk-strategy "$strategy" \
        --chunk-size "$size" \
        --chunk-overlap "$overlap" \
        2>/dev/null

      # Run eval matrix
      REPORT="$OUT_DIR/report-${strategy}-${size}-${overlap}.json"
      cargo run --release -p fastrag-cli --bin fastrag \
        --features eval,retrieval,rerank,rerank-llama,contextual,contextual-llama -- \
        eval \
        --gold-set "$GOLD_SET" \
        --corpus "$CTX_DIR" \
        --corpus-no-contextual "$RAW_DIR" \
        --config-matrix \
        --variants primary \
        --report "$REPORT" \
        2>/dev/null

      # Extract metrics from report JSON
      CHUNKS=$(find "$CTX_DIR" -name entries.bin -exec wc -c {} \; | awk '{print $1}')
      INDEX_BYTES=$(du -sb "$CTX_DIR" | cut -f1)
      HIT1=$(python3 -c "import json; r=json.load(open('$REPORT')); print(r['runs'][0]['hit_at_1'])")
      HIT5=$(python3 -c "import json; r=json.load(open('$REPORT')); print(r['runs'][0]['hit_at_5'])")
      HIT10=$(python3 -c "import json; r=json.load(open('$REPORT')); print(r['runs'][0]['hit_at_10'])")
      MRR=$(python3 -c "import json; r=json.load(open('$REPORT')); print(r['runs'][0]['mrr_at_10'])")

      printf "%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n" \
        "$strategy" "$size" "$overlap" "$CHUNKS" "$INDEX_BYTES" \
        "$HIT1" "$HIT5" "$HIT10" "$MRR" >> "$RESULTS"

      # Clean up corpora to save disk
      rm -rf "$CTX_DIR" "$RAW_DIR"
    done
  done
done

echo ""
echo "Sweep complete. Results: $RESULTS"
echo ""
echo "Top 5 by hit@5 (Primary variant):"
sort -t$'\t' -k7 -rn "$RESULTS" | head -6
