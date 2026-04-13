# Phase 2 Closeout Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close out Phase 2 by shipping NVD security queries (#35), a CI eval-regression gate (#36), and data-driven chunking defaults (#30).

**Architecture:** Three sequential landings. #35 creates the NVD query set and refreshes baselines. #36 adds an `eval-gate` CI job backed by `actions/cache` pre-built corpora. #30 adds a local chunking sweep script, runs it, and updates defaults.

**Tech Stack:** Rust, GitHub Actions, `actions/cache@v4`, llama-server (GGUF), `fastrag-eval` crate.

---

### Task 1: NVD Security Query Set — Create Query File

**Files:**
- Create: `crates/fastrag-eval/src/datasets/security_queries.json`

The NVD fixture at `crates/fastrag-eval/tests/fixtures/datasets/nvd_mini.json.gz` contains 5 CVEs:
- CVE-2023-11111: buffer overflow in demo parser (RCE)
- CVE-2023-22222: JWT verification accepts unsigned tokens
- CVE-2023-33333: cache poisoning
- CVE-2023-44444: path traversal in upload handler
- CVE-2023-55555: authorization bypass in admin dashboard

- [ ] **Step 1: Create security_queries.json**

```json
{
  "queries": [
    {"id": "nvd-q01", "text": "What is CVE-2023-11111?"},
    {"id": "nvd-q02", "text": "buffer overflow remote code execution in parser"},
    {"id": "nvd-q03", "text": "JWT token verification bypass"},
    {"id": "nvd-q04", "text": "What is CVE-2023-33333?"},
    {"id": "nvd-q05", "text": "cache poisoning vulnerability"},
    {"id": "nvd-q06", "text": "path traversal file overwrite upload"},
    {"id": "nvd-q07", "text": "What is CVE-2023-55555?"},
    {"id": "nvd-q08", "text": "administrative dashboard authorization bypass"},
    {"id": "nvd-q09", "text": "remote code execution vulnerabilities 2023"},
    {"id": "nvd-q10", "text": "web application input validation flaws"}
  ],
  "qrels": [
    {"query_id": "nvd-q01", "doc_id": "CVE-2023-11111", "relevance": 2},
    {"query_id": "nvd-q02", "doc_id": "CVE-2023-11111", "relevance": 2},
    {"query_id": "nvd-q03", "doc_id": "CVE-2023-22222", "relevance": 2},
    {"query_id": "nvd-q04", "doc_id": "CVE-2023-33333", "relevance": 2},
    {"query_id": "nvd-q05", "doc_id": "CVE-2023-33333", "relevance": 2},
    {"query_id": "nvd-q06", "doc_id": "CVE-2023-44444", "relevance": 2},
    {"query_id": "nvd-q07", "doc_id": "CVE-2023-55555", "relevance": 2},
    {"query_id": "nvd-q08", "doc_id": "CVE-2023-55555", "relevance": 2},
    {"query_id": "nvd-q09", "doc_id": "CVE-2023-11111", "relevance": 1},
    {"query_id": "nvd-q10", "doc_id": "CVE-2023-44444", "relevance": 1}
  ]
}
```

- [ ] **Step 2: Validate the JSON loads with the existing test**

Run: `cargo test -p fastrag-eval joins_security_queries_and_qrels -- --nocapture`
Expected: PASS (existing test validates the SecurityQueriesFile schema)

### Task 2: NVD Security Query Set — Wire Auto-Discovery

**Files:**
- Modify: `crates/fastrag-eval/src/datasets/nvd.rs` (lines 16-21, 90-101)
- Test: `crates/fastrag-eval/src/datasets/nvd.rs` (in-file `#[cfg(test)]`)

- [ ] **Step 1: Write a failing test for bundled query auto-discovery**

Add to the `#[cfg(test)] mod tests` block in `crates/fastrag-eval/src/datasets/nvd.rs`:

```rust
#[test]
fn load_nvd_includes_bundled_queries() {
    let dataset = load_nvd_from_corpus_paths_with_bundled_queries(
        "nvd-test",
        &[fixture_path()],
    ).unwrap();
    assert!(!dataset.queries.is_empty(), "bundled queries should be loaded");
    assert!(!dataset.qrels.is_empty(), "bundled qrels should be loaded");
    // Verify a known query exists
    assert!(dataset.queries.iter().any(|q| q.id == "nvd-q01"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p fastrag-eval load_nvd_includes_bundled_queries -- --nocapture`
Expected: FAIL — `load_nvd_from_corpus_paths_with_bundled_queries` does not exist.

- [ ] **Step 3: Implement bundled query loading**

In `crates/fastrag-eval/src/datasets/nvd.rs`, add a constant for the embedded query set path and a new function. Modify `load_nvd()` to use it.

At the top of the file, after the existing constants:

```rust
const BUNDLED_SECURITY_QUERIES: &str =
    include_str!("security_queries.json");
```

Add the new function after `load_nvd_from_corpus_paths`:

```rust
fn load_nvd_from_corpus_paths_with_bundled_queries(
    name: &str,
    paths: &[PathBuf],
) -> EvalResult<EvalDataset> {
    let mut documents = Vec::new();
    for path in paths {
        documents.extend(load_nvd_documents(path)?);
    }

    let query_file: SecurityQueriesFile =
        serde_json::from_str(BUNDLED_SECURITY_QUERIES).map_err(|e| {
            EvalError::MalformedDataset(format!("bundled security_queries.json: {e}"))
        })?;

    let corpus_ids: HashSet<String> = documents.iter().map(|d| d.id.clone()).collect();
    let mut queries = Vec::with_capacity(query_file.queries.len());
    for q in &query_file.queries {
        queries.push(EvalQuery {
            id: q.id.clone(),
            text: q.text.clone(),
        });
    }

    let mut qrels = Vec::new();
    for qrel in &query_file.qrels {
        if corpus_ids.contains(&qrel.doc_id) {
            qrels.push(Qrel {
                query_id: qrel.query_id.clone(),
                doc_id: qrel.doc_id.clone(),
                relevance: qrel.relevance,
            });
        }
    }

    // Drop queries that have no matching qrels (corpus doesn't contain their target docs)
    let active_query_ids: HashSet<&str> = qrels.iter().map(|q| q.query_id.as_str()).collect();
    queries.retain(|q| active_query_ids.contains(q.id.as_str()));

    Ok(EvalDataset {
        name: name.to_string(),
        documents,
        queries,
        qrels,
    })
}
```

Update `load_nvd()` to call the new function:

```rust
pub fn load_nvd() -> EvalResult<EvalDataset> {
    let root = cache_root("nvd")?;
    let feed_2023 = ensure_nvd_feed(&root, NVD_2023_URL)?;
    let feed_2024 = ensure_nvd_feed(&root, NVD_2024_URL)?;
    load_nvd_from_corpus_paths_with_bundled_queries("nvd", &[feed_2023, feed_2024])
}
```

Note: `include_str!` path is relative to the file location. Since `nvd.rs` is in `src/datasets/`, the path to the JSON file in the same directory is just `"security_queries.json"`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p fastrag-eval load_nvd_includes_bundled_queries -- --nocapture`
Expected: PASS

- [ ] **Step 5: Run full workspace tests + clippy**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets --features retrieval,rerank,hybrid,contextual,eval -- -D warnings`
Expected: All green.

- [ ] **Step 6: Commit**

```bash
git add crates/fastrag-eval/src/datasets/security_queries.json crates/fastrag-eval/src/datasets/nvd.rs
git commit -m "feat(eval): bundled NVD security query set

load_nvd() now auto-discovers bundled queries so NVD baselines
report retrieval metrics instead of index-footprint-only numbers.

Closes #35"
```

### Task 3: CI Eval-Regression Gate — Weekly Cache Persist

**Files:**
- Modify: `.github/workflows/weekly.yml`

- [ ] **Step 1: Add cache-save step to weekly workflow**

After the "Build raw corpus" step (line 105) and before the "Run eval matrix" step, add a cache save step. Also modify the existing corpus build steps to use a stable path for caching.

Replace the corpus build paths from `$RUNNER_TEMP/corpus-ctx` and `$RUNNER_TEMP/corpus-raw` with a path under the workspace that `actions/cache` can persist:

In `.github/workflows/weekly.yml`, add environment variables at the job level (under `eval:` → `env:`):

```yaml
      EVAL_CORPUS_CTX: eval-corpus/ctx
      EVAL_CORPUS_RAW: eval-corpus/raw
```

Update the "Build contextualized corpus" step to use `${{ github.workspace }}/eval-corpus/ctx`:

```yaml
      - name: Build contextualized corpus
        run: |
          cargo run --release -p fastrag-cli --bin fastrag \
            --features retrieval,rerank,hybrid,contextual,contextual-llama -- \
            index tests/gold/corpus \
            --corpus eval-corpus/ctx \
            --embedder qwen3-q8 \
            --contextualize
```

Update the "Build raw corpus" step similarly:

```yaml
      - name: Build raw corpus
        run: |
          cargo run --release -p fastrag-cli --bin fastrag \
            --features retrieval,rerank,hybrid,contextual,contextual-llama -- \
            index tests/gold/corpus \
            --corpus eval-corpus/raw \
            --embedder qwen3-q8
```

Add a cache save step after "Build raw corpus":

```yaml
      - name: Save eval corpus cache
        uses: actions/cache/save@v4
        with:
          path: eval-corpus
          key: eval-corpus-${{ hashFiles('tests/gold/corpus/**', 'crates/fastrag-embed/**', 'crates/fastrag-index/**') }}
```

Update the eval matrix step to use the new paths:

```yaml
      - name: Run eval matrix
        id: run_eval
        run: |
          BASELINE_ARG=""
          if [ -f docs/eval-baselines/current.json ]; then
            BASELINE_ARG="--baseline docs/eval-baselines/current.json"
          else
            echo "No baseline committed yet — running without regression gate."
          fi
          cargo run --release -p fastrag-cli --bin fastrag \
            --features eval,retrieval,rerank,rerank-llama,hybrid,contextual,contextual-llama -- \
            eval \
            --gold-set tests/gold/questions.json \
            --corpus eval-corpus/ctx \
            --corpus-no-contextual eval-corpus/raw \
            --config-matrix \
            --variants primary,no_rerank \
            $BASELINE_ARG \
            --report "$RUNNER_TEMP/matrix.json"
```

- [ ] **Step 2: Verify YAML is valid**

Run: `python3 -c "import yaml; yaml.safe_load(open('.github/workflows/weekly.yml'))"`
Expected: No error.

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/weekly.yml
git commit -m "ci(weekly): persist eval corpus to actions/cache for PR gate"
```

### Task 4: CI Eval-Regression Gate — PR Job

**Files:**
- Modify: `.github/workflows/ci.yml`

- [ ] **Step 1: Add eval-gate job to ci.yml**

Append the following job to `.github/workflows/ci.yml`:

```yaml

  eval-gate:
    runs-on: ubuntu-latest
    if: github.event_name == 'pull_request'
    timeout-minutes: 10
    env:
      FASTRAG_LLAMA_TEST: "1"
      FASTRAG_RERANK_TEST: "1"
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2

      - name: Restore eval corpus cache
        id: corpus_cache
        uses: actions/cache/restore@v4
        with:
          path: eval-corpus
          key: eval-corpus-${{ hashFiles('tests/gold/corpus/**', 'crates/fastrag-embed/**', 'crates/fastrag-index/**') }}

      - name: Skip gate on cache miss
        if: steps.corpus_cache.outputs.cache-hit != 'true'
        run: |
          echo "::warning::eval corpus cache miss — gate skipped, will run in next weekly"
          exit 0

      - name: Install llama-server
        if: steps.corpus_cache.outputs.cache-hit == 'true'
        run: |
          LLAMA_TAG="b8739"
          curl -fsSL "https://github.com/ggml-org/llama.cpp/releases/download/${LLAMA_TAG}/llama-${LLAMA_TAG}-bin-ubuntu-x64.tar.gz" -o llama.tar.gz
          tar xzf llama.tar.gz
          sudo mkdir -p /opt/llama
          sudo cp -a llama-${LLAMA_TAG}/* /opt/llama/
          sudo ln -sf /opt/llama/llama-server /usr/local/bin/llama-server
          echo "LD_LIBRARY_PATH=/opt/llama" >> "$GITHUB_ENV"

      - name: Cache GGUF reranker model
        if: steps.corpus_cache.outputs.cache-hit == 'true'
        uses: actions/cache@v4
        with:
          path: ~/.cache/fastrag/models/bge-reranker-v2-m3-q8_0.gguf
          key: gguf-bge-reranker-v2-m3-q8

      - name: Download BGE reranker GGUF
        if: steps.corpus_cache.outputs.cache-hit == 'true'
        run: |
          MODEL_DIR="$HOME/.cache/fastrag/models"
          MODEL_FILE="bge-reranker-v2-m3-q8_0.gguf"
          if [ ! -f "$MODEL_DIR/$MODEL_FILE" ]; then
            mkdir -p "$MODEL_DIR"
            curl -fsSL "https://huggingface.co/klnstpr/bge-reranker-v2-m3-Q8_0-GGUF/resolve/main/$MODEL_FILE" -o "$MODEL_DIR/$MODEL_FILE"
          fi

      - name: Check for eval regression waiver
        if: steps.corpus_cache.outputs.cache-hit == 'true'
        id: waiver
        run: |
          TRAILER=$(git log -1 --format='%(trailers:key=Eval-Regression-Justified,valueonly)')
          if [ -n "$TRAILER" ]; then
            echo "waived=true" >> "$GITHUB_OUTPUT"
            echo "::notice::Eval regression waived: $TRAILER"
          else
            echo "waived=false" >> "$GITHUB_OUTPUT"
          fi

      - name: Run eval matrix (query-only)
        if: steps.corpus_cache.outputs.cache-hit == 'true' && steps.waiver.outputs.waived != 'true'
        run: |
          cargo run --release -p fastrag-cli --bin fastrag \
            --features eval,retrieval,rerank,rerank-llama,hybrid,contextual,contextual-llama -- \
            eval \
            --gold-set tests/gold/questions.json \
            --corpus eval-corpus/ctx \
            --corpus-no-contextual eval-corpus/raw \
            --config-matrix \
            --variants primary,no_rerank \
            --baseline docs/eval-baselines/current.json \
            --report "$RUNNER_TEMP/matrix.json"
```

Note: The job does NOT download the embedding GGUF. The corpus is pre-built and cached — only the reranker GGUF is needed for query-time reranking.

- [ ] **Step 2: Verify YAML is valid**

Run: `python3 -c "import yaml; yaml.safe_load(open('.github/workflows/ci.yml'))"`
Expected: No error.

- [ ] **Step 3: Run existing tests to confirm no breakage**

Run: `cargo test --workspace`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "ci: add eval-gate PR job with cached corpus regression check

Restores pre-built eval corpus from actions/cache (populated by
weekly job). Runs query-only matrix eval and diffs against baseline.
Supports Eval-Regression-Justified commit trailer for waivers.

Closes #36"
```

### Task 5: Update Eval Baselines README

**Files:**
- Modify: `docs/eval-baselines/README.md`

- [ ] **Step 1: Update README to document NVD query coverage and PR gate**

Add a section about the PR eval gate and NVD query set. After the existing content, add:

```markdown

## PR eval gate

The `eval-gate` job in `ci.yml` runs on every pull request. It restores a
pre-built corpus from `actions/cache` (populated by the weekly workflow) and
runs a query-only matrix eval against `current.json`. No embedding models are
downloaded — only the reranker GGUF is needed for query-time scoring.

On cache miss the gate is skipped with a warning annotation. The next weekly
run rebuilds and caches the corpus.

### Waiver

Add `Eval-Regression-Justified: <reason>` as a git commit trailer to skip
the regression gate for a specific commit.

## NVD query coverage

`load_nvd()` auto-discovers a bundled security query set at
`crates/fastrag-eval/src/datasets/security_queries.json`. NVD baselines now
report recall@10, MRR@10, and nDCG@10 alongside index footprint metrics.
```

- [ ] **Step 2: Commit**

```bash
git add docs/eval-baselines/README.md
git commit -m "docs: document PR eval gate and NVD query coverage"
```

### Task 6: Chunking Sweep Script

**Files:**
- Create: `scripts/chunking-sweep.sh`

- [ ] **Step 1: Create the sweep script**

```bash
#!/usr/bin/env bash
set -euo pipefail

# Chunking strategy sweep — runs locally, requires llama-server + GGUF models.
# Usage: ./scripts/chunking-sweep.sh
# Output: target/chunking-sweep/results.tsv

GOLD_CORPUS="tests/gold/corpus"
GOLD_SET="tests/gold/questions.json"
OUT_DIR="target/chunking-sweep"
RESULTS="$OUT_DIR/results.tsv"

STRATEGIES=(basic by-title recursive)
SIZES=(500 800 1000 1500)
OVERLAPS=(0 100 200)

mkdir -p "$OUT_DIR"

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
        --features retrieval,rerank,hybrid,contextual,contextual-llama -- \
        index "$GOLD_CORPUS" \
        --corpus "$CTX_DIR" \
        --embedder qwen3-q8 \
        --contextualize \
        --chunk-strategy "$strategy" \
        --chunk-size "$size" \
        --chunk-overlap "$overlap" \
        2>/dev/null

      # Build raw corpus
      cargo run --release -p fastrag-cli --bin fastrag \
        --features retrieval,rerank,hybrid,contextual,contextual-llama -- \
        index "$GOLD_CORPUS" \
        --corpus "$RAW_DIR" \
        --embedder qwen3-q8 \
        --chunk-strategy "$strategy" \
        --chunk-size "$size" \
        --chunk-overlap "$overlap" \
        2>/dev/null

      # Run eval matrix
      REPORT="$OUT_DIR/report-${strategy}-${size}-${overlap}.json"
      cargo run --release -p fastrag-cli --bin fastrag \
        --features eval,retrieval,rerank,rerank-llama,hybrid,contextual,contextual-llama -- \
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
```

- [ ] **Step 2: Make executable**

Run: `chmod +x scripts/chunking-sweep.sh`

- [ ] **Step 3: Commit**

```bash
git add scripts/chunking-sweep.sh
git commit -m "feat(eval): add chunking strategy sweep script"
```

### Task 7: Run Chunking Sweep and Update Defaults

**Files:**
- Modify: `fastrag-cli/src/args.rs` (lines 142-150, 84-93)
- Modify: `crates/fastrag-core/src/chunking.rs`
- Create: `docs/chunking-eval.md`

This task is executed locally on the dev box with llama-server running.

- [ ] **Step 1: Run the sweep**

Run: `./scripts/chunking-sweep.sh`
Expected: ~2-3 hours. Results in `target/chunking-sweep/results.tsv`.

- [ ] **Step 2: Analyze results and pick winner**

Open `target/chunking-sweep/results.tsv`. Sort by hit@5 descending, tiebreak by MRR@10, tiebreak by fewer chunks. The winner becomes the new default.

- [ ] **Step 3: Create docs/chunking-eval.md with sweep results**

Write a markdown file containing:
- The full TSV data as a markdown table
- The winner and why it was chosen
- Comparison against the old default (basic/1000/0)

- [ ] **Step 4: Update defaults if winner differs from current**

If the winner differs from `basic/1000/0`, update:

In `fastrag-cli/src/args.rs`, update the `default_value_t` for `chunk_size` and `chunk_overlap` in both the Parse (line 89, 93) and Index (line 146, 150) command args. Update `default_value` for `chunk_strategy` if the strategy changes.

In `crates/fastrag-core/src/chunking.rs`, add a doc comment on `ChunkingStrategy` noting the recommended default and the eval data backing it.

- [ ] **Step 5: Run tests with new defaults**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets --features retrieval,rerank,hybrid,contextual,eval -- -D warnings`
Expected: All green.

- [ ] **Step 6: Refresh baselines with new defaults**

Rebuild the gold corpus with the new chunking defaults and re-capture baselines:

```bash
# Rebuild corpora with new defaults
cargo run --release -p fastrag-cli --bin fastrag \
  --features retrieval,rerank,hybrid,contextual,contextual-llama -- \
  index tests/gold/corpus --corpus /tmp/eval-ctx --embedder qwen3-q8 --contextualize

cargo run --release -p fastrag-cli --bin fastrag \
  --features retrieval,rerank,hybrid,contextual,contextual-llama -- \
  index tests/gold/corpus --corpus /tmp/eval-raw --embedder qwen3-q8

# Re-capture baseline
cargo run --release -p fastrag-cli --bin fastrag \
  --features eval,retrieval,rerank,rerank-llama,hybrid,contextual,contextual-llama -- \
  eval \
  --gold-set tests/gold/questions.json \
  --corpus /tmp/eval-ctx \
  --corpus-no-contextual /tmp/eval-raw \
  --config-matrix \
  --report docs/eval-baselines/current.json
```

- [ ] **Step 7: Commit**

```bash
git add docs/chunking-eval.md fastrag-cli/src/args.rs crates/fastrag-core/src/chunking.rs docs/eval-baselines/current.json
git commit -m "feat(eval): data-driven chunking defaults from sweep results

Sweep of 36 strategy/size/overlap combos against the 110-entry gold set.
New default: <WINNER_STRATEGY>/<WINNER_SIZE>/<WINNER_OVERLAP>.
Full results documented in docs/chunking-eval.md.

Closes #30"
```

### Task 8: Close Phase 2 Epic

**Files:** None (GitHub only)

- [ ] **Step 1: Close epic #33**

```bash
gh issue close 33 --comment "All Phase 2 steps shipped. Steps 1-7 landed via roadmap rewrite. Trailing issues #35, #36, #30 closed out in this final batch. Phase 3 issues (#43-#52) are open."
```

- [ ] **Step 2: Run final lint + test gate**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets --features retrieval,rerank,hybrid,contextual,eval -- -D warnings && cargo fmt --check`
Expected: All green.

- [ ] **Step 3: Push**

Run: `git push`

- [ ] **Step 4: Watch CI**

Invoke `ci-watcher` skill as a background Haiku agent to monitor all workflows.
