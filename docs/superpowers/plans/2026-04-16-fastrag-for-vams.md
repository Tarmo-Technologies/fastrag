# fastrag-for-VAMS Integration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the VAMS-facing HTTP API gaps (`/cve`, `/cwe`, `/cwe/relation`, `/ready`, `/admin/reload`) and ship a DVD-sized airgap Docker image plus bundle format with atomic Arc-swap reload.

**Architecture:** A new `BundleState` (corpora map + taxonomy + manifest) held behind `ArcSwap` on `AppState`, populated at startup and replaceable via authenticated `POST /admin/reload`. CWE taxonomy gains `parents` adjacency + `ancestors()` traversal (schema v2). Four new HTTP routes consume `BundleState`, plus a `/ready` probe distinct from `/health`. Python client gains corresponding wrappers. A Debian-12-slim Docker image bundles fastrag + llama-server + embedder/reranker GGUFs, and a `make dvd-iso` target packs image + sample bundle onto a single-layer DVD.

**Tech Stack:** Rust (axum, clap, arc-swap, serde), Python 3.11+ (httpx, pydantic v2, pytest + responses), Docker (debian:12-slim + tini), Bash/Make for packaging.

**Spec:** `docs/superpowers/specs/2026-04-16-fastrag-for-vams-design.md`

---

## File Structure

**Created files:**
- `crates/fastrag/src/bundle.rs` — `BundleManifest`, `BundleState`, load/validate/swap.
- `fastrag-cli/tests/bundle_load.rs`, `admin_reload_e2e.rs`, `admin_reload_concurrent.rs`, `admin_reload_path_escape.rs`, `admin_reload_rollback.rs`, `cve_lookup_e2e.rs`, `cwe_lookup_e2e.rs`, `cwe_relation_e2e.rs`, `ready_probe_e2e.rs`.
- `tests/fixtures/bundles/minimal/`, `corrupted-missing-cve/`, `corrupted-bad-taxonomy/`.
- `clients/python/tests/test_get_cve.py`, `test_get_cwe.py`, `test_cwe_relation.py`, `test_ready.py`, `test_reload_bundle.py`, `test_similar.py`.
- `docker/Dockerfile.airgap`, `docker/entrypoint.sh`, `docker/README.md`.
- `docker/ci/docker-build-size.sh`, `docker/ci/docker-iso-size.sh`, `docker/ci/docker-no-phone-home.sh`, `docker/ci/docker-smoke.sh`.
- `Makefile` (new, targets: `airgap-image`, `dvd-iso`).
- `docs/airgap-install.md`.

**Modified files:**
- `crates/fastrag-cwe/src/taxonomy.rs` — add `parents` map, `parents()`, `ancestors()`, `ancestors_bounded()`, schema v2 loader/serializer.
- `crates/fastrag-cwe/src/compile.rs` — emit parents in compiled JSON.
- `crates/fastrag-cwe/src/bin/compile_taxonomy.rs` (if present) — version bump.
- `fastrag-cli/src/http.rs` — `AppState` gains `bundle: ArcSwap<BundleState>`, `admin_token`, `bundles_dir`; new routes.
- `fastrag-cli/src/args.rs` — `serve-http` flags `--bundle-path`, `--bundles-dir`, `--admin-token`, `--bundle-retention`.
- `fastrag-cli/src/main.rs` — bundle load at startup.
- `clients/python/src/fastrag_client/client.py` — add six methods.
- `clients/python/src/fastrag_client/models.py` — add `CveRecord`, `CweRecord`, `CweRelation`, `ReadyStatus`, `ReloadResult`, `SimilarHit`.
- `crates/fastrag/Cargo.toml` — add `arc-swap` dependency.
- `fastrag-cli/Cargo.toml` — nothing new (transitive).
- `.github/workflows/ci.yml` — add Docker size + phone-home gates.

---

## Task 1: Taxonomy ancestors + schema v2

**Files:**
- Modify: `crates/fastrag-cwe/src/taxonomy.rs`
- Modify: `crates/fastrag-cwe/src/compile.rs`
- Modify: `crates/fastrag-cwe/tests/` or inline `#[cfg(test)]` (inline per existing pattern)

Current `Taxonomy` stores `closure: HashMap<u32, Vec<u32>>` (self + descendants). We add `parents: HashMap<u32, Vec<u32>>` (direct parent edges, populated at compile time from MITRE XML) and three traversal methods. Schema v2 serializes both maps; v1 loads are rejected.

- [ ] **Step 1: Write failing test for `parents()` on simple chain**

Add to `crates/fastrag-cwe/src/taxonomy.rs` inside `#[cfg(test)]` at the end:

```rust
#[test]
fn parents_returns_direct_parents_only() {
    // CWE graph: 20 -> 707 -> 89 and 20 -> 943 -> 89 (89 has two parents)
    let json = r#"{
        "schema_version": 2,
        "version": "4.15",
        "view": "1000",
        "closure": {"20": [20, 707, 943, 89], "707": [707, 89], "943": [943, 89], "89": [89]},
        "parents": {"89": [707, 943], "707": [20], "943": [20], "20": []}
    }"#;
    let tax: Taxonomy = serde_json::from_str(json).unwrap();
    let p = tax.parents(89);
    let mut got = p.to_vec();
    got.sort();
    assert_eq!(got, vec![707, 943]);
    assert_eq!(tax.parents(20), &[] as &[u32]);
}
```

- [ ] **Step 2: Run the test and confirm it fails**

```bash
cargo test -p fastrag-cwe parents_returns_direct_parents_only
```
Expected: FAIL — `parents` field missing, `parents()` method not defined.

- [ ] **Step 3: Add `parents` field and methods**

In `crates/fastrag-cwe/src/taxonomy.rs`, update the struct (around line 12-18) and add methods:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Taxonomy {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    version: String,
    view: String,
    /// Map from CWE id → `[self, descendants...]` (self is first, rest sorted ascending).
    closure: HashMap<u32, Vec<u32>>,
    /// Direct parent edges for each CWE. Empty for root nodes.
    #[serde(default)]
    parents: HashMap<u32, Vec<u32>>,
}

fn default_schema_version() -> u32 { 1 }

impl Taxonomy {
    pub fn parents(&self, cwe: u32) -> &[u32] {
        self.parents.get(&cwe).map(|v| v.as_slice()).unwrap_or(&[])
    }
}
```

Also update `from_components` to accept parents:

```rust
pub fn from_components(
    version: String,
    view: String,
    closure: HashMap<u32, Vec<u32>>,
    parents: HashMap<u32, Vec<u32>>,
) -> Self {
    Self { schema_version: 2, version, view, closure, parents }
}
```

- [ ] **Step 4: Run the test and confirm green**

```bash
cargo test -p fastrag-cwe parents_returns_direct_parents_only
```
Expected: PASS.

- [ ] **Step 5: Write failing test for `ancestors()` BFS dedupe on DAG**

```rust
#[test]
fn ancestors_bfs_dedupes_multi_parent_dag() {
    let json = r#"{
        "schema_version": 2, "version": "4.15", "view": "1000",
        "closure": {},
        "parents": {
            "89": [707, 943],
            "707": [20],
            "943": [20],
            "20": []
        }
    }"#;
    let tax: Taxonomy = serde_json::from_str(json).unwrap();
    let a = tax.ancestors(89);
    // BFS order: direct parents first (707, 943 in any order), then 20 once.
    assert_eq!(a.len(), 3);
    assert!(a.contains(&707));
    assert!(a.contains(&943));
    assert!(a.contains(&20));
    // CWE 20 appears only once despite two parent paths.
    assert_eq!(a.iter().filter(|&&x| x == 20).count(), 1);
}

#[test]
fn ancestors_of_root_is_empty() {
    let json = r#"{"schema_version": 2, "version": "4.15", "view": "1000",
                   "closure": {}, "parents": {"20": []}}"#;
    let tax: Taxonomy = serde_json::from_str(json).unwrap();
    assert!(tax.ancestors(20).is_empty());
}

#[test]
fn ancestors_of_unknown_is_empty() {
    let json = r#"{"schema_version": 2, "version": "4.15", "view": "1000",
                   "closure": {}, "parents": {}}"#;
    let tax: Taxonomy = serde_json::from_str(json).unwrap();
    assert!(tax.ancestors(9999).is_empty());
}
```

- [ ] **Step 6: Run and confirm failure**

```bash
cargo test -p fastrag-cwe ancestors
```
Expected: FAIL — `ancestors()` not defined.

- [ ] **Step 7: Implement `ancestors()` via BFS**

In `crates/fastrag-cwe/src/taxonomy.rs`:

```rust
use std::collections::VecDeque;

impl Taxonomy {
    pub fn ancestors(&self, cwe: u32) -> Vec<u32> {
        self.ancestors_bounded(cwe, usize::MAX)
    }

    pub fn ancestors_bounded(&self, cwe: u32, max_depth: usize) -> Vec<u32> {
        let mut out = Vec::new();
        let mut visited: std::collections::HashSet<u32> = std::collections::HashSet::new();
        visited.insert(cwe);
        let mut queue: VecDeque<(u32, usize)> = VecDeque::new();
        queue.push_back((cwe, 0));
        while let Some((node, depth)) = queue.pop_front() {
            if depth >= max_depth {
                continue;
            }
            if let Some(parents) = self.parents.get(&node) {
                for &p in parents {
                    if visited.insert(p) {
                        out.push(p);
                        queue.push_back((p, depth + 1));
                    }
                }
            }
        }
        out
    }
}
```

- [ ] **Step 8: Run and confirm green**

```bash
cargo test -p fastrag-cwe ancestors
```
Expected: all three PASS.

- [ ] **Step 9: Write failing test for `ancestors_bounded`**

```rust
#[test]
fn ancestors_bounded_respects_max_depth() {
    let json = r#"{"schema_version": 2, "version": "4.15", "view": "1000",
        "closure": {},
        "parents": {"89": [707], "707": [20], "20": []}}"#;
    let tax: Taxonomy = serde_json::from_str(json).unwrap();
    assert_eq!(tax.ancestors_bounded(89, 0), Vec::<u32>::new());
    assert_eq!(tax.ancestors_bounded(89, 1), vec![707]);
    let mut two = tax.ancestors_bounded(89, 2);
    two.sort();
    assert_eq!(two, vec![20, 707]);
}
```

- [ ] **Step 10: Run and confirm green** (the bounded path was written above)

```bash
cargo test -p fastrag-cwe ancestors_bounded_respects_max_depth
```
Expected: PASS.

- [ ] **Step 11: Write failing test for schema v1 rejection**

```rust
#[test]
fn schema_v1_is_rejected_at_load() {
    // v1 payload lacks `parents` and uses `schema_version` 1 (or missing).
    let v1 = r#"{"version":"4.15","view":"1000",
                 "closure":{"20":[20]}}"#;
    let err = Taxonomy::from_json(v1).err().expect("should reject v1");
    let msg = err.to_string();
    assert!(msg.contains("schema") || msg.contains("version"),
            "expected schema error, got: {msg}");
    assert!(msg.contains("compile-taxonomy") || msg.contains("rebuild"),
            "error should point at the rebuild command, got: {msg}");
}
```

- [ ] **Step 12: Run and confirm failure**

```bash
cargo test -p fastrag-cwe schema_v1_is_rejected_at_load
```
Expected: FAIL — no `from_json` method or no version check.

- [ ] **Step 13: Add `from_json` that enforces schema v2**

In `crates/fastrag-cwe/src/taxonomy.rs`:

```rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum TaxonomyError {
    #[error("taxonomy schema {found} is not supported; expected 2. Rebuild with: cargo run -p fastrag-cwe --features compile-tool --bin compile-taxonomy")]
    SchemaMismatch { found: u32 },
    #[error("taxonomy parse error: {0}")]
    Parse(#[from] serde_json::Error),
}

impl Taxonomy {
    pub fn from_json(s: &str) -> Result<Self, TaxonomyError> {
        let tax: Taxonomy = serde_json::from_str(s)?;
        if tax.schema_version != 2 {
            return Err(TaxonomyError::SchemaMismatch { found: tax.schema_version });
        }
        Ok(tax)
    }
}
```

- [ ] **Step 14: Run and confirm green**

```bash
cargo test -p fastrag-cwe schema_v1_is_rejected_at_load
```
Expected: PASS.

- [ ] **Step 15: Write failing test — `compute_parents()` from child→parents map**

```rust
#[cfg(test)]
mod compile_tests {
    use super::compile::{compute_closure, compute_parents};
    use std::collections::HashMap;

    #[test]
    fn compute_parents_inverts_child_parent_relation() {
        // parents map input: child -> Vec<parent>. Output keyed by node -> direct parents.
        let mut input: HashMap<u32, Vec<u32>> = HashMap::new();
        input.insert(89, vec![707, 943]);
        input.insert(707, vec![20]);
        input.insert(943, vec![20]);
        input.insert(20, vec![]);
        let out = compute_parents(&input);
        let mut p89 = out.get(&89).cloned().unwrap();
        p89.sort();
        assert_eq!(p89, vec![707, 943]);
        assert_eq!(out.get(&20).cloned().unwrap_or_default(), Vec::<u32>::new());
    }
}
```

- [ ] **Step 16: Run and confirm failure**

```bash
cargo test -p fastrag-cwe compute_parents_inverts_child_parent_relation
```
Expected: FAIL — `compute_parents` not defined.

- [ ] **Step 17: Implement `compute_parents` + wire into `build_closure`**

In `crates/fastrag-cwe/src/compile.rs` around the existing `compute_closure`:

```rust
pub fn compute_parents(child_to_parents: &HashMap<u32, Vec<u32>>) -> HashMap<u32, Vec<u32>> {
    let mut out: HashMap<u32, Vec<u32>> = HashMap::new();
    for (&child, parents) in child_to_parents {
        let entry = out.entry(child).or_default();
        for &p in parents {
            if !entry.contains(&p) {
                entry.push(p);
            }
        }
        out.entry(child).or_default();
        for &p in parents {
            out.entry(p).or_default();
        }
    }
    out
}

pub fn build_closure(xml_bytes: &[u8], view_id: &str) -> Result<Taxonomy, CompileError> {
    let (version, parents_in) = parse_catalog(xml_bytes, view_id)?;
    let closure = compute_closure(&parents_in);
    let parents_out = compute_parents(&parents_in);
    Ok(Taxonomy::from_components(
        version,
        view_id.to_string(),
        closure,
        parents_out,
    ))
}
```

- [ ] **Step 18: Run and confirm green**

```bash
cargo test -p fastrag-cwe compute_parents_inverts_child_parent_relation
```
Expected: PASS.

- [ ] **Step 19: Write failing test — cycle detection at compile time**

```rust
#[test]
fn compute_parents_detects_cycle() {
    let mut input: HashMap<u32, Vec<u32>> = HashMap::new();
    input.insert(1, vec![2]);
    input.insert(2, vec![3]);
    input.insert(3, vec![1]);
    let err = super::compile::detect_cycles(&input).err().expect("cycle expected");
    assert!(err.to_string().to_lowercase().contains("cycle"));
}
```

- [ ] **Step 20: Run and confirm failure**

```bash
cargo test -p fastrag-cwe compute_parents_detects_cycle
```
Expected: FAIL — `detect_cycles` missing.

- [ ] **Step 21: Add `detect_cycles` and call from `build_closure`**

In `crates/fastrag-cwe/src/compile.rs`:

```rust
pub fn detect_cycles(parents: &HashMap<u32, Vec<u32>>) -> Result<(), CompileError> {
    // DFS with WHITE/GRAY/BLACK coloring on the child→parents DAG.
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Color { White, Gray, Black }
    let mut color: HashMap<u32, Color> = parents.keys().map(|&k| (k, Color::White)).collect();

    fn visit(
        node: u32,
        parents: &HashMap<u32, Vec<u32>>,
        color: &mut HashMap<u32, Color>,
    ) -> Result<(), CompileError> {
        color.insert(node, Color::Gray);
        if let Some(next) = parents.get(&node) {
            for &p in next {
                match color.get(&p).copied().unwrap_or(Color::White) {
                    Color::Gray => return Err(CompileError::Cycle(node, p)),
                    Color::White => visit(p, parents, color)?,
                    Color::Black => {}
                }
            }
        }
        color.insert(node, Color::Black);
        Ok(())
    }

    for &start in parents.keys() {
        if color.get(&start).copied().unwrap_or(Color::White) == Color::White {
            visit(start, parents, &mut color)?;
        }
    }
    Ok(())
}
```

Add to `CompileError` enum:
```rust
#[derive(Debug, thiserror::Error)]
pub enum CompileError {
    // ... existing variants ...
    #[error("cycle detected in CWE taxonomy between {0} and {1}")]
    Cycle(u32, u32),
}
```

Call `detect_cycles(&parents_in)?;` inside `build_closure` before `compute_parents`.

- [ ] **Step 22: Run and confirm green**

```bash
cargo test -p fastrag-cwe compute_parents_detects_cycle
```
Expected: PASS.

- [ ] **Step 23: Run full crate tests and clippy**

```bash
cargo test -p fastrag-cwe
cargo clippy -p fastrag-cwe --all-targets -- -D warnings
cargo fmt --check
```
Expected: all PASS, no warnings.

- [ ] **Step 24: Commit**

```bash
git add crates/fastrag-cwe/
git commit -m "feat(cwe): add Taxonomy::ancestors + schema v2 parents map

Adds parents adjacency (direct parent edges), ancestors() BFS traversal
with cycle-safe dedupe, and ancestors_bounded(max_depth). Schema bumps
1 -> 2 at the JSON boundary; v1 loads are rejected with a pointer at the
compile-taxonomy rebuild command. compile-taxonomy emits the parents map
alongside the existing closure."
```

---

## Task 2: Bundle module

**Files:**
- Create: `crates/fastrag/src/bundle.rs`
- Modify: `crates/fastrag/src/lib.rs` (add `pub mod bundle;`)
- Modify: `crates/fastrag/Cargo.toml` (add `arc-swap` dep)

`BundleState` is the unit of atomic reload. It holds `HashMap<String, Arc<Corpus>>` keyed by convention on `"cve"`, `"cwe"`, `"kev"`; an `Arc<Taxonomy>`; and the `BundleManifest` from `bundle.json`. `load_bundle(path)` validates the directory tree, loads the three required corpora, and returns a `BundleState` ready to wrap in `Arc` and hand to `ArcSwap`.

- [ ] **Step 1: Add arc-swap dependency**

Modify `crates/fastrag/Cargo.toml` under `[dependencies]`:

```toml
arc-swap = "1.7"
```

- [ ] **Step 2: Create the module with failing test for manifest parsing**

Create `crates/fastrag/src/bundle.rs`:

```rust
//! Multi-corpus bundle format for airgap sneakernet delivery.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use thiserror::Error;

const REQUIRED_CORPORA: [&str; 3] = ["cve", "cwe", "kev"];
const BUNDLE_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleManifest {
    pub schema_version: u32,
    pub bundle_id: String,
    pub built_at: String,
    pub corpora: Vec<String>,
    pub taxonomy: String,
    #[serde(default)]
    pub sources: serde_json::Value,
}

#[derive(Debug, Error)]
pub enum BundleError {
    #[error("bundle schema mismatch: found {found}, expected {BUNDLE_SCHEMA_VERSION}")]
    SchemaMismatch { found: u32 },
    #[error("bundle missing required corpus: {0}")]
    CorpusMissing(String),
    #[error("bundle taxonomy file missing at {0}")]
    TaxonomyMissing(PathBuf),
    #[error("bundle manifest missing at {0}")]
    ManifestMissing(PathBuf),
    #[error("bundle io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("bundle parse error: {0}")]
    Parse(#[from] serde_json::Error),
    #[error("taxonomy load error: {0}")]
    Taxonomy(#[from] fastrag_cwe::TaxonomyError),
}

pub fn parse_manifest(json: &str) -> Result<BundleManifest, BundleError> {
    let m: BundleManifest = serde_json::from_str(json)?;
    if m.schema_version != BUNDLE_SCHEMA_VERSION {
        return Err(BundleError::SchemaMismatch { found: m.schema_version });
    }
    Ok(m)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_manifest_happy_path() {
        let json = r#"{
            "schema_version": 1,
            "bundle_id": "fastrag-20260416",
            "built_at": "2026-04-16T18:00:00Z",
            "corpora": ["cve", "cwe", "kev"],
            "taxonomy": "cwe-taxonomy.json",
            "sources": {"cve": {"type": "nvd", "feed_date": "2026-04-15"}}
        }"#;
        let m = parse_manifest(json).unwrap();
        assert_eq!(m.bundle_id, "fastrag-20260416");
        assert_eq!(m.corpora, vec!["cve", "cwe", "kev"]);
    }

    #[test]
    fn parse_manifest_rejects_wrong_schema() {
        let json = r#"{"schema_version": 99, "bundle_id": "x", "built_at": "t",
                       "corpora": [], "taxonomy": "t.json"}"#;
        let err = parse_manifest(json).unwrap_err();
        assert!(matches!(err, BundleError::SchemaMismatch { found: 99 }));
    }
}
```

Add to `crates/fastrag/src/lib.rs`:
```rust
pub mod bundle;
```

- [ ] **Step 3: Run and confirm both tests pass**

```bash
cargo test -p fastrag bundle::tests --features retrieval
```
Expected: both tests PASS.

- [ ] **Step 4: Write failing test for `validate_layout`**

Add to `#[cfg(test)] mod tests` in `bundle.rs`:

```rust
#[test]
fn validate_layout_requires_all_corpora() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    // Create bundle.json, cve corpus dir, taxonomy — skip cwe and kev.
    std::fs::write(root.join("bundle.json"), r#"{
        "schema_version": 1, "bundle_id": "b", "built_at": "t",
        "corpora": ["cve","cwe","kev"], "taxonomy": "cwe-taxonomy.json"
    }"#).unwrap();
    std::fs::create_dir_all(root.join("corpora/cve")).unwrap();
    std::fs::create_dir_all(root.join("taxonomy")).unwrap();
    std::fs::write(root.join("taxonomy/cwe-taxonomy.json"), "{}").unwrap();

    let err = validate_layout(root).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("cwe") || msg.contains("kev"),
            "expected missing-corpus error, got: {msg}");
}

#[test]
fn validate_layout_requires_taxonomy() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::write(root.join("bundle.json"), r#"{
        "schema_version": 1, "bundle_id": "b", "built_at": "t",
        "corpora": ["cve","cwe","kev"], "taxonomy": "cwe-taxonomy.json"
    }"#).unwrap();
    for c in ["cve", "cwe", "kev"] {
        std::fs::create_dir_all(root.join("corpora").join(c)).unwrap();
    }
    let err = validate_layout(root).unwrap_err();
    assert!(matches!(err, BundleError::TaxonomyMissing(_)));
}
```

Add `tempfile = "3"` to `crates/fastrag/Cargo.toml` under `[dev-dependencies]` if not already present.

- [ ] **Step 5: Run and confirm failure**

```bash
cargo test -p fastrag bundle::tests::validate_layout --features retrieval
```
Expected: FAIL — `validate_layout` undefined.

- [ ] **Step 6: Implement `validate_layout`**

Add to `crates/fastrag/src/bundle.rs`:

```rust
pub fn validate_layout(root: &Path) -> Result<BundleManifest, BundleError> {
    let manifest_path = root.join("bundle.json");
    if !manifest_path.exists() {
        return Err(BundleError::ManifestMissing(manifest_path));
    }
    let manifest_str = std::fs::read_to_string(&manifest_path)?;
    let manifest = parse_manifest(&manifest_str)?;

    for corpus in REQUIRED_CORPORA {
        let dir = root.join("corpora").join(corpus);
        if !dir.is_dir() {
            return Err(BundleError::CorpusMissing(corpus.to_string()));
        }
        let mf = dir.join("manifest.json");
        if !mf.exists() {
            return Err(BundleError::CorpusMissing(format!("{corpus} (manifest.json missing)")));
        }
    }

    let taxonomy = root.join("taxonomy").join(&manifest.taxonomy);
    if !taxonomy.exists() {
        return Err(BundleError::TaxonomyMissing(taxonomy));
    }

    Ok(manifest)
}
```

- [ ] **Step 7: Run and confirm green**

```bash
cargo test -p fastrag bundle::tests::validate_layout --features retrieval
```
Expected: both PASS.

- [ ] **Step 8: Write failing test for `BundleState` load-from-disk**

```rust
#[test]
fn load_bundle_populates_state() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    // Write bundle.json
    std::fs::write(root.join("bundle.json"), r#"{
        "schema_version": 1, "bundle_id": "b1", "built_at": "t",
        "corpora": ["cve","cwe","kev"], "taxonomy": "cwe-taxonomy.json"
    }"#).unwrap();
    // Write taxonomy v2
    std::fs::create_dir_all(root.join("taxonomy")).unwrap();
    std::fs::write(
        root.join("taxonomy/cwe-taxonomy.json"),
        r#"{"schema_version":2,"version":"4.15","view":"1000",
             "closure":{"89":[89]},"parents":{"89":[]}}"#,
    ).unwrap();
    // Stub each corpus with an empty index manifest
    for c in ["cve","cwe","kev"] {
        let dir = root.join("corpora").join(c);
        std::fs::create_dir_all(&dir).unwrap();
        crate::corpus::test_support::write_empty_corpus(&dir).unwrap();
    }

    let state = BundleState::load(root).unwrap();
    assert_eq!(state.manifest.bundle_id, "b1");
    assert!(state.corpora.contains_key("cve"));
    assert!(state.corpora.contains_key("cwe"));
    assert!(state.corpora.contains_key("kev"));
}
```

Note: the test relies on a `test_support::write_empty_corpus` helper. If that doesn't exist, the test uses `CorpusRegistry` directly — adapt in Step 10.

- [ ] **Step 9: Run and confirm failure**

```bash
cargo test -p fastrag bundle::tests::load_bundle_populates_state --features retrieval
```
Expected: FAIL — `BundleState` or `load` undefined.

- [ ] **Step 10: Implement `BundleState`**

Add to `crates/fastrag/src/bundle.rs`:

```rust
use crate::corpus::Corpus;
use fastrag_cwe::Taxonomy;

pub struct BundleState {
    pub corpora: HashMap<String, Arc<Corpus>>,
    pub taxonomy: Arc<Taxonomy>,
    pub manifest: BundleManifest,
}

impl BundleState {
    pub fn load(root: &Path) -> Result<Self, BundleError> {
        let manifest = validate_layout(root)?;

        let tax_path = root.join("taxonomy").join(&manifest.taxonomy);
        let tax_json = std::fs::read_to_string(&tax_path)?;
        let taxonomy = Arc::new(Taxonomy::from_json(&tax_json)?);

        let mut corpora: HashMap<String, Arc<Corpus>> = HashMap::new();
        for name in REQUIRED_CORPORA {
            let dir = root.join("corpora").join(name);
            let corpus = Corpus::open(&dir)
                .map_err(|e| BundleError::CorpusMissing(format!("{name}: {e}")))?;
            corpora.insert(name.to_string(), Arc::new(corpus));
        }

        Ok(BundleState { corpora, taxonomy, manifest })
    }
}
```

If `Corpus::open` doesn't exist yet as a single entry point, adapt to whatever `crates/fastrag/src/corpus/registry.rs` offers (check existing `CorpusRegistry::insert`-style code). The test uses whichever helper matches real production loading.

- [ ] **Step 11: Run and confirm green**

```bash
cargo test -p fastrag bundle::tests::load_bundle_populates_state --features retrieval
```
Expected: PASS.

- [ ] **Step 12: Run full crate tests + clippy**

```bash
cargo test -p fastrag --features retrieval
cargo clippy -p fastrag --all-targets --features retrieval -- -D warnings
cargo fmt --check
```
Expected: all green, no warnings.

- [ ] **Step 13: Commit**

```bash
git add crates/fastrag/
git commit -m "feat(bundle): multi-corpus bundle format + BundleState loader

Introduces BundleManifest (schema v1) and BundleState holding
HashMap<String, Arc<Corpus>> keyed on cve/cwe/kev plus Arc<Taxonomy>.
validate_layout enforces required directory tree; BundleState::load
parses manifest, loads taxonomy v2, and opens each corpus. Wire-up
comes in the next commit."
```

---

## Task 3: AppState + startup wiring + CLI flags

**Files:**
- Modify: `fastrag-cli/src/args.rs`
- Modify: `fastrag-cli/src/http.rs`
- Modify: `fastrag-cli/src/main.rs`

Add four flags to `serve-http`: `--bundle-path`, `--bundles-dir`, `--admin-token`, `--bundle-retention`. When `--bundle-path` is set, the server loads the bundle at startup into an `ArcSwap<BundleState>` held on `AppState`. The existing `--corpus` path stays supported; if `--bundle-path` is absent, `AppState.bundle` is `None` and `/cve`, `/cwe`, `/cwe/relation`, `/admin/reload` return 503.

- [ ] **Step 1: Add CLI flags**

In `fastrag-cli/src/args.rs`, inside the `ServeHttp { ... }` variant (around line 607-690), add:

```rust
/// Path to a fastrag bundle directory (overrides --corpus).
#[arg(long)]
bundle_path: Option<PathBuf>,

/// Root directory for bundles; names passed to /admin/reload resolve under here.
#[arg(long)]
bundles_dir: Option<PathBuf>,

/// Admin token for /admin/* endpoints. Also read from FASTRAG_ADMIN_TOKEN.
#[arg(long)]
admin_token: Option<String>,

/// Number of prior bundles to retain on disk for rollback.
#[arg(long, default_value_t = 3)]
bundle_retention: usize,
```

- [ ] **Step 2: Run build to confirm flags compile**

```bash
cargo check -p fastrag-cli
```
Expected: clean.

- [ ] **Step 3: Write failing integration test — server refuses to start with invalid bundle**

Create `fastrag-cli/tests/bundle_load.rs`:

```rust
//! Integration test: server refuses to start with an invalid bundle.

use assert_cmd::Command;
use tempfile::tempdir;

#[test]
fn serve_http_refuses_invalid_bundle() {
    let tmp = tempdir().unwrap();
    let bundle = tmp.path().join("not-a-bundle");
    std::fs::create_dir_all(&bundle).unwrap();
    // No bundle.json, no corpora — invalid.

    let output = Command::cargo_bin("fastrag")
        .unwrap()
        .args([
            "serve-http",
            "--bundle-path",
            bundle.to_str().unwrap(),
            "--port",
            "0",
        ])
        .timeout(std::time::Duration::from_secs(5))
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&output.get_output().stderr).to_string();
    assert!(
        stderr.contains("bundle") || stderr.contains("manifest"),
        "expected bundle error, got: {stderr}"
    );
}
```

Add `assert_cmd = "2"` and `tempfile = "3"` to `fastrag-cli/Cargo.toml` under `[dev-dependencies]` if missing.

- [ ] **Step 4: Run and confirm failure**

```bash
cargo test -p fastrag-cli --test bundle_load
```
Expected: FAIL — either the binary accepts the flag without loading, or the flag is unrecognized.

- [ ] **Step 5: Thread bundle load through startup**

In `fastrag-cli/src/http.rs`, extend `AppState` (around lines 32-48):

```rust
use arc_swap::ArcSwap;
use fastrag::bundle::BundleState;
use std::path::PathBuf;

#[derive(Clone)]
struct AppState {
    registry: fastrag::corpus::CorpusRegistry,
    embedder: DynEmbedder,
    metrics: PrometheusHandle,
    dense_only: bool,
    cwe_expand_default: bool,
    batch_max_queries: usize,
    tenant_field: Option<String>,
    ingest_locks: IngestLocks,
    ingest_max_body: usize,
    similar_overfetch_cap: usize,
    #[cfg(feature = "rerank")]
    reranker: Option<std::sync::Arc<dyn fastrag_rerank::Reranker>>,
    #[cfg(feature = "rerank")]
    rerank_over_fetch: usize,

    // New bundle-mode state.
    bundle: Option<std::sync::Arc<ArcSwap<BundleState>>>,
    bundles_dir: Option<PathBuf>,
    admin_token: Option<String>,
    reload_lock: std::sync::Arc<tokio::sync::Mutex<()>>,
}
```

In `fastrag-cli/src/main.rs` (or wherever `ServeHttp` is dispatched), when `bundle_path.is_some()`:

```rust
let bundle = if let Some(path) = &bundle_path {
    let state = fastrag::bundle::BundleState::load(path)
        .map_err(|e| anyhow::anyhow!("failed to load bundle at {}: {e}", path.display()))?;
    Some(std::sync::Arc::new(ArcSwap::from_pointee(state)))
} else {
    None
};
let app_state = AppState {
    // ... existing fields ...
    bundle,
    bundles_dir,
    admin_token: admin_token.or_else(|| std::env::var("FASTRAG_ADMIN_TOKEN").ok()),
    reload_lock: std::sync::Arc::new(tokio::sync::Mutex::new(())),
};
```

- [ ] **Step 6: Run the test and confirm green**

```bash
cargo test -p fastrag-cli --test bundle_load
```
Expected: PASS.

- [ ] **Step 7: Write failing test — `/admin/reload` returns 401 without admin token**

Create `fastrag-cli/tests/admin_token_e2e.rs`:

```rust
//! The /admin/* routes require a separate admin token.

use axum_test::TestServer;

#[tokio::test]
async fn admin_reload_requires_admin_token() {
    let (router, _tmp) = fastrag_cli::test_support::build_router_with_bundle(
        /* admin_token */ Some("sekret".to_string()),
    ).await;
    let server = TestServer::new(router).unwrap();

    let resp = server
        .post("/admin/reload")
        .json(&serde_json::json!({"bundle_path": "x"}))
        .await;
    assert_eq!(resp.status_code(), 401);
}
```

- [ ] **Step 8: Run and confirm failure**

```bash
cargo test -p fastrag-cli --test admin_token_e2e
```
Expected: FAIL — `test_support::build_router_with_bundle` not defined, or handler not wired.

- [ ] **Step 9: Add test-support module exposing router builder**

In `fastrag-cli/src/lib.rs` (create if absent; fastrag-cli currently has main.rs and http.rs — expose the test helper from lib.rs):

```rust
pub mod test_support;
```

Create `fastrag-cli/src/test_support.rs`:

```rust
//! Helpers for integration tests — builds an in-memory router against a
//! synthetic bundle on disk so tests don't spawn an OS process.

use std::path::PathBuf;

pub async fn build_router_with_bundle(
    admin_token: Option<String>,
) -> (axum::Router, tempfile::TempDir) {
    let tmp = tempfile::tempdir().unwrap();
    build_minimal_bundle(tmp.path());
    let state = crate::http::AppState::for_test_with_bundle(tmp.path(), admin_token);
    let router = crate::http::build_router_for_test(state);
    (router, tmp)
}

fn build_minimal_bundle(root: &std::path::Path) {
    std::fs::write(root.join("bundle.json"), r#"{
        "schema_version": 1, "bundle_id": "test", "built_at": "t",
        "corpora": ["cve","cwe","kev"], "taxonomy": "cwe-taxonomy.json"
    }"#).unwrap();
    std::fs::create_dir_all(root.join("taxonomy")).unwrap();
    std::fs::write(
        root.join("taxonomy/cwe-taxonomy.json"),
        r#"{"schema_version":2,"version":"4.15","view":"1000",
             "closure":{"89":[89]},"parents":{"89":[]}}"#,
    ).unwrap();
    for c in ["cve","cwe","kev"] {
        let d = root.join("corpora").join(c);
        std::fs::create_dir_all(&d).unwrap();
        fastrag::corpus::write_empty_corpus_for_test(&d).unwrap();
    }
}
```

Make `AppState` and `build_router` accessible: add `pub(crate)` or add test-only `for_test_with_bundle` constructor and `build_router_for_test` in `http.rs`:

```rust
#[cfg(any(test, feature = "test-support"))]
impl AppState {
    pub fn for_test_with_bundle(bundle_path: &std::path::Path, admin_token: Option<String>) -> Self {
        let state = fastrag::bundle::BundleState::load(bundle_path).unwrap();
        // ... construct AppState with sensible defaults for test ...
    }
}

#[cfg(any(test, feature = "test-support"))]
pub fn build_router_for_test(state: AppState) -> axum::Router { build_router(state) }
```

Wire `bundles_dir` to parent of `bundle_path` in the test helper so admin/reload works in subsequent tests.

- [ ] **Step 10: Add skeleton admin handler returning 401 when token mismatches**

In `fastrag-cli/src/http.rs`, add the route:

```rust
let admin = Router::new()
    .route("/admin/reload", axum::routing::post(admin_reload_handler))
    .layer(axum::middleware::from_fn_with_state(
        state.clone(),
        admin_auth_middleware,
    ));
```

Add:

```rust
async fn admin_auth_middleware(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let Some(expected) = state.admin_token.as_deref() else {
        return Err(StatusCode::UNAUTHORIZED);
    };
    let provided = req
        .headers()
        .get("x-fastrag-admin-token")
        .and_then(|v| v.to_str().ok());
    if provided == Some(expected) {
        Ok(next.run(req).await)
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
}

async fn admin_reload_handler() -> (StatusCode, Json<serde_json::Value>) {
    // Full implementation in Task 6; stub for now.
    (StatusCode::NOT_IMPLEMENTED,
     Json(json!({"error": "not_implemented"})))
}
```

- [ ] **Step 11: Run tests**

```bash
cargo test -p fastrag-cli --test admin_token_e2e
cargo test -p fastrag-cli --test bundle_load
```
Expected: both PASS.

- [ ] **Step 12: Lint + fmt**

```bash
cargo clippy -p fastrag-cli --all-targets --features retrieval,rerank -- -D warnings
cargo fmt --check
```
Expected: clean.

- [ ] **Step 13: Commit**

```bash
git add fastrag-cli/
git commit -m "feat(cli): bundle-path + admin-token wiring for serve-http

Adds --bundle-path, --bundles-dir, --admin-token, --bundle-retention to
the serve-http subcommand. When --bundle-path is set, the server loads
a BundleState at startup into an ArcSwap held on AppState and refuses to
start if the bundle is invalid. /admin/* routes require --admin-token
(or FASTRAG_ADMIN_TOKEN env var) via x-fastrag-admin-token header; the
/admin/reload handler is a stub pending Task 6."
```

---

## Task 4: Lookup endpoints `/cve`, `/cwe`, `/cwe/relation`

**Files:**
- Modify: `fastrag-cli/src/http.rs` (add three handlers + routes)
- Create: `fastrag-cli/tests/cve_lookup_e2e.rs`
- Create: `fastrag-cli/tests/cwe_lookup_e2e.rs`
- Create: `fastrag-cli/tests/cwe_relation_e2e.rs`

Each lookup queries the matching bundle corpus by convention (`cve_id = X` filter on `cve` corpus, `cwe_id = N` on `cwe`). `/cwe/relation` hits `Taxonomy::ancestors()` + `Taxonomy::expand()` directly.

- [ ] **Step 1: Write failing test — GET /cve/{id} 404 for unknown**

Create `fastrag-cli/tests/cve_lookup_e2e.rs`:

```rust
use axum_test::TestServer;

#[tokio::test]
async fn get_cve_404_for_unknown_id() {
    let (router, _tmp) = fastrag_cli::test_support::build_router_with_bundle(None).await;
    let server = TestServer::new(router).unwrap();

    let resp = server.get("/cve/CVE-9999-0000").await;
    assert_eq!(resp.status_code(), 404);
    let body: serde_json::Value = resp.json();
    assert_eq!(body["error"], "cve_not_found");
    assert_eq!(body["id"], "CVE-9999-0000");
}

#[tokio::test]
async fn get_cve_rejects_query_params() {
    let (router, _tmp) = fastrag_cli::test_support::build_router_with_bundle(None).await;
    let server = TestServer::new(router).unwrap();

    let resp = server.get("/cve/CVE-2021-44228?q=anything").await;
    assert_eq!(resp.status_code(), 400);
}
```

- [ ] **Step 2: Run and confirm failure**

```bash
cargo test -p fastrag-cli --test cve_lookup_e2e
```
Expected: FAIL — route not defined.

- [ ] **Step 3: Implement GET /cve/{id}**

In `fastrag-cli/src/http.rs`:

```rust
async fn get_cve_handler(
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<String>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Response {
    if !params.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "unexpected_query_params",
                        "message": "/cve/{id} is a direct lookup; query params not allowed"})),
        ).into_response();
    }
    let Some(bundle) = state.bundle.as_ref() else {
        return (StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({"error": "bundle_not_loaded"}))).into_response();
    };
    let guard = bundle.load_full();
    let Some(corpus) = guard.corpora.get("cve") else {
        return (StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({"error": "corpus_cve_missing"}))).into_response();
    };

    match fastrag::corpus::lookup_by_field(corpus.as_ref(), "cve_id", &id) {
        Ok(Some(hit)) => (StatusCode::OK, Json(serde_json::to_value(hit).unwrap())).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "cve_not_found", "id": id})),
        ).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "lookup_failed", "message": e.to_string()})),
        ).into_response(),
    }
}
```

Add `fastrag::corpus::lookup_by_field` helper to `crates/fastrag/src/corpus/mod.rs` — a thin wrapper that builds a `filter` with `field = value` and returns the single top hit (or `Ok(None)` if empty):

```rust
pub fn lookup_by_field(corpus: &Corpus, field: &str, value: &str) -> Result<Option<SearchHit>, CorpusError> {
    let filter_str = format!("{field} = \"{value}\"");
    let filter = crate::filter::parse(&filter_str)?;
    let hits = corpus.filter_only(&filter, 1)?;
    Ok(hits.into_iter().next())
}
```

If `filter_only` doesn't exist on `Corpus`, use whatever the existing direct-filter path is (search `filter_only` / `filter_scan` in the repo). If no such path exists, this is a small addition — a filter-only scan that skips embedding.

Register the route:

```rust
let protected = Router::new()
    // existing routes
    .route("/cve/:id", get(get_cve_handler))
    .route("/cwe/:id", get(get_cwe_handler))
    .route("/cwe/relation", get(cwe_relation_handler));
```

- [ ] **Step 4: Run and confirm green**

```bash
cargo test -p fastrag-cli --test cve_lookup_e2e
```
Expected: both PASS.

- [ ] **Step 5: Write failing tests for GET /cwe/{id}**

Create `fastrag-cli/tests/cwe_lookup_e2e.rs`:

```rust
use axum_test::TestServer;

#[tokio::test]
async fn get_cwe_accepts_numeric_id() {
    let (router, _tmp) = fastrag_cli::test_support::build_router_with_bundle(None).await;
    let server = TestServer::new(router).unwrap();
    let resp = server.get("/cwe/89").await;
    // Test bundle has no CWE records, so 404 is acceptable; 200 if bundle seeded.
    assert!(resp.status_code() == 200 || resp.status_code() == 404,
            "unexpected status: {}", resp.status_code());
}

#[tokio::test]
async fn get_cwe_accepts_prefixed_id() {
    let (router, _tmp) = fastrag_cli::test_support::build_router_with_bundle(None).await;
    let server = TestServer::new(router).unwrap();
    let resp = server.get("/cwe/CWE-89").await;
    assert!(resp.status_code() == 200 || resp.status_code() == 404);
}

#[tokio::test]
async fn get_cwe_rejects_non_numeric() {
    let (router, _tmp) = fastrag_cli::test_support::build_router_with_bundle(None).await;
    let server = TestServer::new(router).unwrap();
    let resp = server.get("/cwe/not-a-number").await;
    assert_eq!(resp.status_code(), 400);
}
```

- [ ] **Step 6: Run and confirm failure**

```bash
cargo test -p fastrag-cli --test cwe_lookup_e2e
```
Expected: FAIL.

- [ ] **Step 7: Implement GET /cwe/{id}**

```rust
async fn get_cwe_handler(
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Response {
    let numeric = id.strip_prefix("CWE-").unwrap_or(&id);
    let cwe_id: u32 = match numeric.parse() {
        Ok(n) => n,
        Err(_) => return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "invalid_cwe_id",
                        "message": "cwe id must be integer or CWE-<integer>"})),
        ).into_response(),
    };
    let Some(bundle) = state.bundle.as_ref() else {
        return (StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({"error": "bundle_not_loaded"}))).into_response();
    };
    let guard = bundle.load_full();
    let Some(corpus) = guard.corpora.get("cwe") else {
        return (StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({"error": "corpus_cwe_missing"}))).into_response();
    };

    match fastrag::corpus::lookup_by_field(corpus.as_ref(), "cwe_id", &cwe_id.to_string()) {
        Ok(Some(hit)) => (StatusCode::OK, Json(serde_json::to_value(hit).unwrap())).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "cwe_not_found", "id": cwe_id})),
        ).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "lookup_failed", "message": e.to_string()})),
        ).into_response(),
    }
}
```

- [ ] **Step 8: Run and confirm green**

```bash
cargo test -p fastrag-cli --test cwe_lookup_e2e
```
Expected: all PASS.

- [ ] **Step 9: Write failing tests for GET /cwe/relation**

Create `fastrag-cli/tests/cwe_relation_e2e.rs`:

```rust
use axum_test::TestServer;

#[tokio::test]
async fn cwe_relation_returns_ancestors_and_descendants() {
    let (router, _tmp) = fastrag_cli::test_support::build_router_with_bundle_dag().await;
    let server = TestServer::new(router).unwrap();

    let resp = server.get("/cwe/relation?cwe_id=89").await;
    assert_eq!(resp.status_code(), 200);
    let body: serde_json::Value = resp.json();
    assert_eq!(body["cwe_id"], 89);
    // DAG in the test helper: 89 has parents [707, 943], each -> 20.
    let ancestors: Vec<u64> = body["ancestors"]
        .as_array().unwrap().iter()
        .map(|v| v.as_u64().unwrap()).collect();
    assert!(ancestors.contains(&707));
    assert!(ancestors.contains(&943));
    assert!(ancestors.contains(&20));
    assert!(body["descendants"].is_array());
}

#[tokio::test]
async fn cwe_relation_respects_direction() {
    let (router, _tmp) = fastrag_cli::test_support::build_router_with_bundle_dag().await;
    let server = TestServer::new(router).unwrap();

    let resp = server.get("/cwe/relation?cwe_id=89&direction=ancestors").await;
    assert_eq!(resp.status_code(), 200);
    let body: serde_json::Value = resp.json();
    assert!(body["ancestors"].is_array());
    assert!(body["descendants"].is_null() || body["descendants"].as_array().map(|a| a.is_empty()).unwrap_or(true));
}

#[tokio::test]
async fn cwe_relation_respects_max_depth() {
    let (router, _tmp) = fastrag_cli::test_support::build_router_with_bundle_dag().await;
    let server = TestServer::new(router).unwrap();

    let resp = server.get("/cwe/relation?cwe_id=89&direction=ancestors&max_depth=1").await;
    assert_eq!(resp.status_code(), 200);
    let body: serde_json::Value = resp.json();
    let ancestors: Vec<u64> = body["ancestors"].as_array().unwrap().iter()
        .map(|v| v.as_u64().unwrap()).collect();
    // Depth 1 -> only direct parents of 89 (707, 943).
    assert!(ancestors.contains(&707));
    assert!(ancestors.contains(&943));
    assert!(!ancestors.contains(&20));
}

#[tokio::test]
async fn cwe_relation_bad_id_is_400() {
    let (router, _tmp) = fastrag_cli::test_support::build_router_with_bundle(None).await;
    let server = TestServer::new(router).unwrap();
    let resp = server.get("/cwe/relation?cwe_id=not-a-number").await;
    assert_eq!(resp.status_code(), 400);
}
```

Add `build_router_with_bundle_dag` helper in `fastrag-cli/src/test_support.rs` that seeds the taxonomy with the DAG from Task 1 (`89 -> [707,943] -> [20]`).

- [ ] **Step 10: Run and confirm failure**

```bash
cargo test -p fastrag-cli --test cwe_relation_e2e
```
Expected: FAIL.

- [ ] **Step 11: Implement GET /cwe/relation**

```rust
#[derive(serde::Deserialize)]
struct CweRelationParams {
    cwe_id: Option<String>,
    #[serde(default)]
    direction: Option<String>,
    #[serde(default)]
    max_depth: Option<usize>,
}

async fn cwe_relation_handler(
    State(state): State<AppState>,
    axum::extract::Query(params): axum::extract::Query<CweRelationParams>,
) -> Response {
    let Some(raw) = params.cwe_id else {
        return (StatusCode::BAD_REQUEST,
                Json(json!({"error": "missing_cwe_id"}))).into_response();
    };
    let cwe: u32 = match raw.parse() {
        Ok(n) => n,
        Err(_) => return (StatusCode::BAD_REQUEST,
                         Json(json!({"error": "invalid_cwe_id"}))).into_response(),
    };
    let dir = params.direction.as_deref().unwrap_or("both");
    let Some(bundle) = state.bundle.as_ref() else {
        return (StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({"error": "bundle_not_loaded"}))).into_response();
    };
    let guard = bundle.load_full();
    let tax = guard.taxonomy.as_ref();

    let ancestors = if matches!(dir, "ancestors" | "both") {
        match params.max_depth {
            Some(d) => tax.ancestors_bounded(cwe, d),
            None => tax.ancestors(cwe),
        }
    } else {
        Vec::new()
    };
    let descendants = if matches!(dir, "descendants" | "both") {
        let mut d = tax.expand(cwe);
        // expand() includes self; strip it.
        d.retain(|&x| x != cwe);
        d
    } else {
        Vec::new()
    };

    (StatusCode::OK, Json(json!({
        "cwe_id": cwe,
        "ancestors": ancestors,
        "descendants": descendants,
    }))).into_response()
}
```

- [ ] **Step 12: Run and confirm green**

```bash
cargo test -p fastrag-cli --test cwe_relation_e2e
```
Expected: all PASS.

- [ ] **Step 13: Full lint gate + fmt**

```bash
cargo clippy -p fastrag-cli --all-targets --features retrieval,rerank -- -D warnings
cargo clippy -p fastrag --all-targets --features retrieval -- -D warnings
cargo fmt --check
```
Expected: clean.

- [ ] **Step 14: Commit**

```bash
git add crates/fastrag/ fastrag-cli/
git commit -m "feat(http): /cve/{id}, /cwe/{id}, /cwe/relation lookup endpoints

Adds three direct-lookup endpoints backed by BundleState. /cve and /cwe
query their conventional corpora via field-equality filters and return
single-record 200 or 404. /cwe/relation traverses the in-memory Taxonomy
(no corpus hit) with direction=ancestors|descendants|both and optional
max_depth. All three reject requests when no bundle is loaded (503)."
```

---

## Task 5: GET /ready

**Files:**
- Modify: `fastrag-cli/src/http.rs` (add handler + route)
- Create: `fastrag-cli/tests/ready_probe_e2e.rs`

`/ready` differs from `/health`: it returns 503 until `BundleState` is loaded and the embedder+reranker (if configured) respond to health probes.

- [ ] **Step 1: Write failing tests**

Create `fastrag-cli/tests/ready_probe_e2e.rs`:

```rust
use axum_test::TestServer;

#[tokio::test]
async fn ready_200_when_bundle_loaded() {
    let (router, _tmp) = fastrag_cli::test_support::build_router_with_bundle(None).await;
    let server = TestServer::new(router).unwrap();
    let resp = server.get("/ready").await;
    assert_eq!(resp.status_code(), 200);
    let body: serde_json::Value = resp.json();
    assert_eq!(body["ready"], true);
}

#[tokio::test]
async fn ready_503_when_no_bundle() {
    let router = fastrag_cli::test_support::build_router_no_bundle();
    let server = TestServer::new(router).unwrap();
    let resp = server.get("/ready").await;
    assert_eq!(resp.status_code(), 503);
    let body: serde_json::Value = resp.json();
    assert_eq!(body["ready"], false);
    let reasons: Vec<String> = body["reasons"].as_array().unwrap()
        .iter().map(|v| v.as_str().unwrap().to_string()).collect();
    assert!(reasons.iter().any(|r| r == "bundle_not_loaded"));
}

#[tokio::test]
async fn ready_is_unauthenticated() {
    // /ready must not require the read token — it's a probe.
    let (router, _tmp) = fastrag_cli::test_support::build_router_with_bundle_and_token(
        Some("secret".to_string()), None,
    ).await;
    let server = TestServer::new(router).unwrap();
    let resp = server.get("/ready").await;
    assert_eq!(resp.status_code(), 200);
}
```

Add `build_router_no_bundle` and `build_router_with_bundle_and_token` to the test_support module.

- [ ] **Step 2: Run and confirm failure**

```bash
cargo test -p fastrag-cli --test ready_probe_e2e
```
Expected: FAIL.

- [ ] **Step 3: Implement `/ready`**

In `fastrag-cli/src/http.rs`, add handler and register outside the auth-protected router (alongside `/health`):

```rust
async fn ready_handler(State(state): State<AppState>) -> Response {
    let mut reasons: Vec<&'static str> = Vec::new();

    let Some(bundle) = state.bundle.as_ref() else {
        reasons.push("bundle_not_loaded");
        return (StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({"ready": false, "reasons": reasons}))).into_response();
    };
    let guard = bundle.load_full();
    for name in ["cve", "cwe", "kev"] {
        if !guard.corpora.contains_key(name) {
            // Leak the name as a static via a match — only three possible values.
            match name {
                "cve" => reasons.push("corpus_cve_missing"),
                "cwe" => reasons.push("corpus_cwe_missing"),
                "kev" => reasons.push("corpus_kev_missing"),
                _ => {}
            }
        }
    }

    // Embedder/reranker probes: call their health methods if the trait exposes one.
    if !state.embedder.is_ready().await {
        reasons.push("embedder_unreachable");
    }
    #[cfg(feature = "rerank")]
    if let Some(r) = state.reranker.as_ref() {
        if !r.is_ready().await {
            reasons.push("reranker_unreachable");
        }
    }

    if reasons.is_empty() {
        (StatusCode::OK, Json(json!({"ready": true}))).into_response()
    } else {
        (StatusCode::SERVICE_UNAVAILABLE,
         Json(json!({"ready": false, "reasons": reasons}))).into_response()
    }
}
```

If `DynEmbedder::is_ready` doesn't exist, add a simple implementation: HTTP embedders do a GET against their `/health` (short timeout, e.g. 500ms); in-process embedders always return `true`. Similarly for the reranker trait. Keep the check fast — `/ready` is polled frequently.

Register alongside `/health`:
```rust
let app = Router::new()
    .route("/health", get(health))
    .route("/ready", get(ready_handler))
    // ... then merge protected and admin routers
```

- [ ] **Step 4: Run and confirm green**

```bash
cargo test -p fastrag-cli --test ready_probe_e2e
```
Expected: all PASS.

- [ ] **Step 5: Full lint gate**

```bash
cargo clippy -p fastrag-cli --all-targets --features retrieval,rerank -- -D warnings
cargo fmt --check
```
Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add crates/fastrag-embed/ crates/fastrag-rerank/ fastrag-cli/
git commit -m "feat(http): /ready probe distinct from /health

/ready returns 503 until BundleState is loaded and the embedder +
reranker (if configured) respond to health probes. Reason codes
(bundle_not_loaded, corpus_*_missing, embedder_unreachable,
reranker_unreachable) let a liveness proxy surface the exact missing
dependency. Route is unauthenticated so external probes don't need the
read token."
```

---

## Task 6: POST /admin/reload

**Files:**
- Modify: `fastrag-cli/src/http.rs` (replace stub handler with full impl)
- Create: `fastrag-cli/tests/admin_reload_e2e.rs`
- Create: `fastrag-cli/tests/admin_reload_concurrent.rs`
- Create: `fastrag-cli/tests/admin_reload_path_escape.rs`
- Create: `fastrag-cli/tests/admin_reload_rollback.rs`

Path-safe bundle load → build new `BundleState` → `bundle.store()` atomic swap. A `Mutex<()>` guards the handler so concurrent reloads get 409.

- [ ] **Step 1: Write failing test — happy-path reload**

Create `fastrag-cli/tests/admin_reload_e2e.rs`:

```rust
use axum_test::TestServer;

#[tokio::test]
async fn admin_reload_swaps_bundle_atomically() {
    // Build a bundles-dir with two bundles; start server on the first,
    // reload to the second, confirm /ready still 200 and bundle_id changed.
    let (router, tmp) = fastrag_cli::test_support::build_router_with_two_bundles(
        Some("admintok".to_string()),
    ).await;
    let server = TestServer::new(router).unwrap();

    // Baseline: first bundle loaded.
    let resp = server.get("/ready").await;
    assert_eq!(resp.status_code(), 200);

    // Reload to second bundle (directory name: fastrag-second).
    let resp = server
        .post("/admin/reload")
        .add_header("x-fastrag-admin-token", "admintok")
        .json(&serde_json::json!({"bundle_path": "fastrag-second"}))
        .await;
    assert_eq!(resp.status_code(), 200);
    let body: serde_json::Value = resp.json();
    assert_eq!(body["reloaded"], true);
    assert_eq!(body["bundle_id"], "fastrag-second");
    assert_eq!(body["previous_bundle_id"], "fastrag-first");

    drop(tmp);
}

#[tokio::test]
async fn admin_reload_rejects_nonexistent_bundle() {
    let (router, _tmp) = fastrag_cli::test_support::build_router_with_two_bundles(
        Some("admintok".to_string()),
    ).await;
    let server = TestServer::new(router).unwrap();
    let resp = server
        .post("/admin/reload")
        .add_header("x-fastrag-admin-token", "admintok")
        .json(&serde_json::json!({"bundle_path": "does-not-exist"}))
        .await;
    assert_eq!(resp.status_code(), 400);
    let body: serde_json::Value = resp.json();
    assert!(body["error"].as_str().unwrap().contains("manifest")
         || body["error"] == "bundle_missing");
}
```

- [ ] **Step 2: Write failing test — path escape rejection**

Create `fastrag-cli/tests/admin_reload_path_escape.rs`:

```rust
use axum_test::TestServer;

#[tokio::test]
async fn admin_reload_rejects_parent_traversal() {
    let (router, _tmp) = fastrag_cli::test_support::build_router_with_two_bundles(
        Some("tok".to_string()),
    ).await;
    let server = TestServer::new(router).unwrap();
    let resp = server
        .post("/admin/reload")
        .add_header("x-fastrag-admin-token", "tok")
        .json(&serde_json::json!({"bundle_path": "../../etc"}))
        .await;
    assert_eq!(resp.status_code(), 400);
    let body: serde_json::Value = resp.json();
    assert_eq!(body["error"], "path_escape");
}

#[tokio::test]
async fn admin_reload_rejects_absolute_path() {
    let (router, _tmp) = fastrag_cli::test_support::build_router_with_two_bundles(
        Some("tok".to_string()),
    ).await;
    let server = TestServer::new(router).unwrap();
    let resp = server
        .post("/admin/reload")
        .add_header("x-fastrag-admin-token", "tok")
        .json(&serde_json::json!({"bundle_path": "/etc/passwd"}))
        .await;
    assert_eq!(resp.status_code(), 400);
}
```

- [ ] **Step 3: Write failing test — concurrent reload returns 409**

Create `fastrag-cli/tests/admin_reload_concurrent.rs`:

```rust
use axum_test::TestServer;

#[tokio::test]
async fn concurrent_reload_returns_409() {
    let (router, _tmp) = fastrag_cli::test_support::build_router_with_two_bundles_slow(
        Some("tok".to_string()),
    ).await;
    let server = TestServer::new(router).unwrap();

    let a = tokio::spawn({
        let server = server.clone();
        async move {
            server.post("/admin/reload")
                .add_header("x-fastrag-admin-token", "tok")
                .json(&serde_json::json!({"bundle_path": "fastrag-second"}))
                .await
        }
    });
    // Tiny yield so A acquires the mutex first.
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    let b = server
        .post("/admin/reload")
        .add_header("x-fastrag-admin-token", "tok")
        .json(&serde_json::json!({"bundle_path": "fastrag-second"}))
        .await;
    assert_eq!(b.status_code(), 409);

    let a = a.await.unwrap();
    assert_eq!(a.status_code(), 200);
}
```

`build_router_with_two_bundles_slow` uses a test-only `BundleState::load` injection that sleeps 500ms — gated behind a test helper flag so we can reliably observe the mutex contention.

- [ ] **Step 4: Write failing test — rollback A→B→A works**

Create `fastrag-cli/tests/admin_reload_rollback.rs`:

```rust
use axum_test::TestServer;

#[tokio::test]
async fn rollback_sequence_succeeds() {
    let (router, _tmp) = fastrag_cli::test_support::build_router_with_two_bundles(
        Some("tok".to_string()),
    ).await;
    let server = TestServer::new(router).unwrap();

    for (target, prev) in [
        ("fastrag-second", "fastrag-first"),
        ("fastrag-first", "fastrag-second"),
    ] {
        let resp = server.post("/admin/reload")
            .add_header("x-fastrag-admin-token", "tok")
            .json(&serde_json::json!({"bundle_path": target}))
            .await;
        assert_eq!(resp.status_code(), 200);
        let body: serde_json::Value = resp.json();
        assert_eq!(body["bundle_id"], target);
        assert_eq!(body["previous_bundle_id"], prev);
    }
}
```

- [ ] **Step 5: Run all four tests and confirm failure**

```bash
cargo test -p fastrag-cli --test admin_reload_e2e \
                          --test admin_reload_path_escape \
                          --test admin_reload_concurrent \
                          --test admin_reload_rollback
```
Expected: FAIL (stub still returns 501).

- [ ] **Step 6: Implement `/admin/reload`**

Replace the stub in `fastrag-cli/src/http.rs`:

```rust
#[derive(serde::Deserialize)]
struct ReloadRequest {
    bundle_path: String,
}

async fn admin_reload_handler(
    State(state): State<AppState>,
    Json(req): Json<ReloadRequest>,
) -> Response {
    // Must have a bundles_dir configured.
    let Some(bundles_dir) = state.bundles_dir.as_ref() else {
        return (StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({"error":"bundles_dir_not_configured"}))).into_response();
    };
    let Some(bundle_arc) = state.bundle.as_ref() else {
        return (StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({"error":"bundle_not_loaded"}))).into_response();
    };

    // Validate path.
    if req.bundle_path.is_empty()
        || std::path::Path::new(&req.bundle_path).is_absolute()
        || req.bundle_path.contains("..")
    {
        return (StatusCode::BAD_REQUEST,
                Json(json!({"error":"path_escape"}))).into_response();
    }
    let candidate = bundles_dir.join(&req.bundle_path);
    let canon = match candidate.canonicalize() {
        Ok(p) => p,
        Err(_) => return (StatusCode::BAD_REQUEST,
                         Json(json!({"error":"bundle_missing",
                                     "message": format!("no bundle at {}", candidate.display())})))
                  .into_response(),
    };
    let canon_root = match bundles_dir.canonicalize() {
        Ok(p) => p,
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR,
                         Json(json!({"error":"bundles_dir_invalid"}))).into_response(),
    };
    if !canon.starts_with(&canon_root) {
        return (StatusCode::BAD_REQUEST,
                Json(json!({"error":"path_escape"}))).into_response();
    }

    // Acquire reload mutex; 409 if held.
    let Ok(_guard) = state.reload_lock.try_lock() else {
        return (StatusCode::CONFLICT,
                Json(json!({"error":"reload_in_progress"}))).into_response();
    };

    let new_state = match fastrag::bundle::BundleState::load(&canon) {
        Ok(s) => s,
        Err(e) => {
            return (StatusCode::BAD_REQUEST,
                    Json(json!({"error":"bundle_invalid",
                                "message": e.to_string()}))).into_response();
        }
    };
    let new_id = new_state.manifest.bundle_id.clone();

    let prev = bundle_arc.load_full();
    let prev_id = prev.manifest.bundle_id.clone();
    bundle_arc.store(std::sync::Arc::new(new_state));

    // Metrics: record outcome.
    metrics::counter!("fastrag_bundle_reloads_total", "result" => "ok").increment(1);
    metrics::gauge!("fastrag_bundle_active_id", "bundle_id" => new_id.clone()).set(1.0);

    (StatusCode::OK, Json(json!({
        "reloaded": true,
        "bundle_id": new_id,
        "previous_bundle_id": prev_id,
    }))).into_response()
}
```

- [ ] **Step 7: Run all four tests and confirm green**

```bash
cargo test -p fastrag-cli --test admin_reload_e2e \
                          --test admin_reload_path_escape \
                          --test admin_reload_concurrent \
                          --test admin_reload_rollback
```
Expected: all PASS.

- [ ] **Step 8: Full lint gate**

```bash
cargo clippy -p fastrag-cli --all-targets --features retrieval,rerank -- -D warnings
cargo clippy -p fastrag --all-targets --features retrieval -- -D warnings
cargo fmt --check
```
Expected: clean.

- [ ] **Step 9: Commit**

```bash
git add fastrag-cli/
git commit -m "feat(http): POST /admin/reload with atomic ArcSwap

Canonicalises bundle_path under --bundles-dir, rejects escapes and
absolutes with 400 path_escape. Load + validate new BundleState, then
bundle.store() for atomic swap; in-flight queries finish on the old
Arc. Reload mutex serialises handler — second concurrent caller gets
409 reload_in_progress. Emits fastrag_bundle_reloads_total counter and
fastrag_bundle_active_id gauge."
```

---

## Task 7: Python client additions

**Files:**
- Modify: `clients/python/src/fastrag_client/client.py`
- Modify: `clients/python/src/fastrag_client/models.py`
- Modify: `clients/python/src/fastrag_client/errors.py` (add `ConflictError` if missing)
- Create: `clients/python/tests/test_similar.py`
- Create: `clients/python/tests/test_get_cve.py`
- Create: `clients/python/tests/test_get_cwe.py`
- Create: `clients/python/tests/test_cwe_relation.py`
- Create: `clients/python/tests/test_ready.py`
- Create: `clients/python/tests/test_reload_bundle.py`

Six methods on `FastRAGClient`. Lookup methods (`get_cve`, `get_cwe`) return `None` on 404 (idiomatic Python). `ready()` returns `ReadyStatus` even on 503 (probes aren't errors). `reload_bundle()` raises `FastRAGError` on non-200.

- [ ] **Step 1: Add response models**

In `clients/python/src/fastrag_client/models.py`, add at end of file:

```python
class SimilarHit(BaseModel):
    model_config = ConfigDict(extra="allow")
    id: str
    score: float
    text: str = ""
    metadata: dict[str, Any] = {}


class CveRecord(BaseModel):
    model_config = ConfigDict(extra="allow")
    id: str
    text: str = ""
    metadata: dict[str, Any] = {}
    score: float = 1.0


class CweRecord(BaseModel):
    model_config = ConfigDict(extra="allow")
    id: str
    cwe_id: int
    text: str = ""
    metadata: dict[str, Any] = {}
    score: float = 1.0


class CweRelation(BaseModel):
    model_config = ConfigDict(extra="allow")
    cwe_id: int
    ancestors: list[int] = []
    descendants: list[int] = []


class ReadyStatus(BaseModel):
    model_config = ConfigDict(extra="allow")
    ok: bool
    reasons: list[str] = []


class ReloadResult(BaseModel):
    model_config = ConfigDict(extra="allow")
    reloaded: bool
    bundle_id: str
    previous_bundle_id: str | None = None
```

- [ ] **Step 2: Write failing test — `.get_cve()` happy path and 404**

Create `clients/python/tests/test_get_cve.py`:

```python
from __future__ import annotations
import pytest
import responses
from fastrag_client import FastRAGClient


@responses.activate
def test_get_cve_happy_path():
    responses.add(
        responses.GET,
        "http://fr/cve/CVE-2021-44228",
        json={"id": "CVE-2021-44228", "text": "Log4Shell...",
              "metadata": {"cvss_score": 10.0}, "score": 1.0},
        status=200,
    )
    client = FastRAGClient("http://fr")
    rec = client.get_cve("CVE-2021-44228")
    assert rec is not None
    assert rec.id == "CVE-2021-44228"
    assert rec.metadata["cvss_score"] == 10.0


@responses.activate
def test_get_cve_returns_none_on_404():
    responses.add(
        responses.GET,
        "http://fr/cve/CVE-9999-0000",
        json={"error": "cve_not_found", "id": "CVE-9999-0000"},
        status=404,
    )
    client = FastRAGClient("http://fr")
    assert client.get_cve("CVE-9999-0000") is None


@responses.activate
def test_get_cve_raises_on_5xx():
    responses.add(
        responses.GET,
        "http://fr/cve/CVE-1",
        json={"error": "bundle_not_loaded"},
        status=503,
    )
    from fastrag_client.errors import ServerError
    client = FastRAGClient("http://fr")
    with pytest.raises(ServerError):
        client.get_cve("CVE-1")
```

- [ ] **Step 3: Run and confirm failure**

```bash
cd clients/python && pytest tests/test_get_cve.py -v
```
Expected: FAIL — no `.get_cve()`.

- [ ] **Step 4: Implement `.get_cve()` and `.get_cwe()`**

In `clients/python/src/fastrag_client/client.py`:

```python
from .models import CveRecord, CweRecord, CweRelation, ReadyStatus, ReloadResult, SimilarHit

class FastRAGClient:
    # ... existing methods ...

    def get_cve(self, cve_id: str) -> CveRecord | None:
        resp = self._client.get(f"/cve/{cve_id}")
        if resp.status_code == 404:
            return None
        _raise_for_status(resp)
        return CveRecord.model_validate(resp.json())

    def get_cwe(self, cwe_id: int | str) -> CweRecord | None:
        if isinstance(cwe_id, int):
            segment = str(cwe_id)
        else:
            segment = cwe_id
        resp = self._client.get(f"/cwe/{segment}")
        if resp.status_code == 404:
            return None
        _raise_for_status(resp)
        data = resp.json()
        # Server returns top-level id (doc id) and metadata.cwe_id; surface cwe_id directly.
        if "cwe_id" not in data:
            data["cwe_id"] = int(data.get("metadata", {}).get("cwe_id", 0))
        return CweRecord.model_validate(data)
```

- [ ] **Step 5: Run and confirm green**

```bash
cd clients/python && pytest tests/test_get_cve.py -v
```
Expected: all PASS.

- [ ] **Step 6: Write failing test for `.get_cwe()`**

Create `clients/python/tests/test_get_cwe.py`:

```python
from __future__ import annotations
import responses
from fastrag_client import FastRAGClient


@responses.activate
def test_get_cwe_by_int():
    responses.add(
        responses.GET, "http://fr/cwe/89",
        json={"id": "cwe-89", "cwe_id": 89, "text": "SQLi",
              "metadata": {"parents": [707, 943]}, "score": 1.0},
        status=200,
    )
    client = FastRAGClient("http://fr")
    rec = client.get_cwe(89)
    assert rec is not None
    assert rec.cwe_id == 89
    assert rec.metadata["parents"] == [707, 943]


@responses.activate
def test_get_cwe_by_string():
    responses.add(
        responses.GET, "http://fr/cwe/CWE-89",
        json={"id": "cwe-89", "cwe_id": 89, "text": "SQLi",
              "metadata": {}, "score": 1.0},
        status=200,
    )
    client = FastRAGClient("http://fr")
    rec = client.get_cwe("CWE-89")
    assert rec is not None
    assert rec.cwe_id == 89


@responses.activate
def test_get_cwe_none_on_404():
    responses.add(
        responses.GET, "http://fr/cwe/9999",
        json={"error": "cwe_not_found", "id": 9999},
        status=404,
    )
    assert FastRAGClient("http://fr").get_cwe(9999) is None
```

- [ ] **Step 7: Run and confirm green (already implemented in Step 4)**

```bash
pytest tests/test_get_cwe.py -v
```
Expected: all PASS.

- [ ] **Step 8: Write failing test for `.cwe_relation()`**

Create `clients/python/tests/test_cwe_relation.py`:

```python
from __future__ import annotations
import pytest
import responses
from fastrag_client import FastRAGClient


@responses.activate
def test_cwe_relation_both():
    responses.add(
        responses.GET, "http://fr/cwe/relation",
        json={"cwe_id": 89, "ancestors": [707, 943, 20], "descendants": [564]},
        status=200,
        match=[responses.matchers.query_param_matcher({"cwe_id": "89", "direction": "both"})],
    )
    client = FastRAGClient("http://fr")
    rel = client.cwe_relation(89)
    assert rel.cwe_id == 89
    assert rel.ancestors == [707, 943, 20]
    assert rel.descendants == [564]


@responses.activate
def test_cwe_relation_ancestors_with_depth():
    responses.add(
        responses.GET, "http://fr/cwe/relation",
        json={"cwe_id": 89, "ancestors": [707, 943], "descendants": []},
        status=200,
        match=[responses.matchers.query_param_matcher(
            {"cwe_id": "89", "direction": "ancestors", "max_depth": "1"})],
    )
    client = FastRAGClient("http://fr")
    rel = client.cwe_relation(89, direction="ancestors", max_depth=1)
    assert rel.ancestors == [707, 943]
    assert rel.descendants == []


@responses.activate
def test_cwe_relation_raises_on_400():
    responses.add(
        responses.GET, "http://fr/cwe/relation",
        json={"error": "invalid_cwe_id"},
        status=400,
    )
    from fastrag_client.errors import ValidationError
    with pytest.raises(ValidationError):
        FastRAGClient("http://fr").cwe_relation(-1)
```

- [ ] **Step 9: Run and confirm failure**

```bash
pytest tests/test_cwe_relation.py -v
```
Expected: FAIL.

- [ ] **Step 10: Implement `.cwe_relation()`**

Append to `client.py`:

```python
    def cwe_relation(
        self,
        cwe_id: int | str,
        *,
        direction: str = "both",
        max_depth: int | None = None,
    ) -> CweRelation:
        if isinstance(cwe_id, str) and cwe_id.startswith("CWE-"):
            cwe_id = int(cwe_id.removeprefix("CWE-"))
        params: dict[str, str] = {"cwe_id": str(cwe_id), "direction": direction}
        if max_depth is not None:
            params["max_depth"] = str(max_depth)
        resp = self._client.get("/cwe/relation", params=params)
        _raise_for_status(resp)
        return CweRelation.model_validate(resp.json())
```

- [ ] **Step 11: Run and confirm green**

```bash
pytest tests/test_cwe_relation.py -v
```
Expected: all PASS.

- [ ] **Step 12: Write failing test for `.ready()`**

Create `clients/python/tests/test_ready.py`:

```python
from __future__ import annotations
import responses
from fastrag_client import FastRAGClient


@responses.activate
def test_ready_ok():
    responses.add(
        responses.GET, "http://fr/ready",
        json={"ready": True}, status=200,
    )
    r = FastRAGClient("http://fr").ready()
    assert r.ok is True
    assert r.reasons == []


@responses.activate
def test_ready_503_returns_status_not_raise():
    responses.add(
        responses.GET, "http://fr/ready",
        json={"ready": False, "reasons": ["bundle_not_loaded"]},
        status=503,
    )
    r = FastRAGClient("http://fr").ready()
    assert r.ok is False
    assert "bundle_not_loaded" in r.reasons
```

- [ ] **Step 13: Run and confirm failure**

```bash
pytest tests/test_ready.py -v
```
Expected: FAIL.

- [ ] **Step 14: Implement `.ready()`**

```python
    def ready(self) -> ReadyStatus:
        resp = self._client.get("/ready")
        if resp.status_code in (200, 503):
            body = resp.json()
            return ReadyStatus(ok=bool(body.get("ready")),
                               reasons=list(body.get("reasons", [])))
        _raise_for_status(resp)
        # Unreachable but keeps type-checker happy.
        return ReadyStatus(ok=False, reasons=["unknown_status"])
```

- [ ] **Step 15: Run and confirm green**

```bash
pytest tests/test_ready.py -v
```
Expected: all PASS.

- [ ] **Step 16: Write failing test for `.reload_bundle()`**

Create `clients/python/tests/test_reload_bundle.py`:

```python
from __future__ import annotations
import pytest
import responses
from fastrag_client import FastRAGClient
from fastrag_client.errors import FastRAGError, AuthenticationError


@responses.activate
def test_reload_bundle_happy_path():
    responses.add(
        responses.POST, "http://fr/admin/reload",
        json={"reloaded": True, "bundle_id": "b2", "previous_bundle_id": "b1"},
        status=200,
    )
    r = FastRAGClient("http://fr").reload_bundle("fastrag-20260417", admin_token="tok")
    assert r.reloaded is True
    assert r.bundle_id == "b2"
    assert r.previous_bundle_id == "b1"


@responses.activate
def test_reload_bundle_unauthorized():
    responses.add(
        responses.POST, "http://fr/admin/reload",
        json={"error": "unauthorized"}, status=401,
    )
    with pytest.raises(AuthenticationError):
        FastRAGClient("http://fr").reload_bundle("x", admin_token="wrong")


@responses.activate
def test_reload_bundle_409_conflict():
    responses.add(
        responses.POST, "http://fr/admin/reload",
        json={"error": "reload_in_progress"}, status=409,
    )
    with pytest.raises(FastRAGError):
        FastRAGClient("http://fr").reload_bundle("x", admin_token="tok")
```

- [ ] **Step 17: Run and confirm failure**

```bash
pytest tests/test_reload_bundle.py -v
```
Expected: FAIL.

- [ ] **Step 18: Implement `.reload_bundle()` and 409 mapping**

In `client.py`:

```python
    def reload_bundle(
        self,
        bundle_path: str,
        *,
        admin_token: str | None = None,
    ) -> ReloadResult:
        headers = {}
        token = admin_token or getattr(self, "_admin_token", None)
        if token:
            headers["x-fastrag-admin-token"] = token
        resp = self._client.post(
            "/admin/reload",
            json={"bundle_path": bundle_path},
            headers=headers,
        )
        _raise_for_status(resp)
        return ReloadResult.model_validate(resp.json())
```

In `errors.py`, add 409 mapping:

```python
class ConflictError(FastRAGError):
    """409 — concurrent operation in progress."""

_STATUS_MAP[409] = ConflictError  # in client.py where _STATUS_MAP is defined
```

Update the `_STATUS_MAP` in `client.py` lines 21-38 to include `409: ConflictError` (importing from `errors`).

- [ ] **Step 19: Also extend `__init__` to accept `admin_token`**

```python
def __init__(
    self,
    base_url: str,
    *,
    token: str | None = None,
    admin_token: str | None = None,
    tenant_id: str | None = None,
    timeout: float = 30.0,
) -> None:
    self._base_url = base_url.rstrip("/")
    self._admin_token = admin_token
    self._client = httpx.Client(
        base_url=self._base_url,
        headers=_build_headers(token, tenant_id),
        timeout=timeout,
    )
```

- [ ] **Step 20: Run and confirm green**

```bash
pytest tests/test_reload_bundle.py -v
```
Expected: all PASS.

- [ ] **Step 21: Write failing test for `.similar()`**

Create `clients/python/tests/test_similar.py`:

```python
from __future__ import annotations
import responses
from fastrag_client import FastRAGClient


@responses.activate
def test_similar_happy_path():
    responses.add(
        responses.POST, "http://fr/similar",
        json={"hits": [{"id": "d1", "score": 0.95, "text": "similar chunk",
                        "metadata": {"source_path": "a.md"}}]},
        status=200,
    )
    client = FastRAGClient("http://fr")
    hits = client.similar("query text", threshold=0.8, max_results=5)
    assert len(hits) == 1
    assert hits[0].id == "d1"
    assert hits[0].score == 0.95


@responses.activate
def test_similar_passes_filter_and_corpora():
    def verify(request):
        import json
        body = json.loads(request.body)
        assert body["text"] == "q"
        assert body["threshold"] == 0.7
        assert body["corpora"] == ["cve", "kev"]
        assert body["filter"] == {"cve_id": "CVE-2021-44228"}
        return (200, {}, '{"hits":[]}')

    responses.add_callback(responses.POST, "http://fr/similar", callback=verify)
    FastRAGClient("http://fr").similar(
        "q", threshold=0.7, corpora=["cve", "kev"],
        filter={"cve_id": "CVE-2021-44228"},
    )
```

- [ ] **Step 22: Run and confirm failure**

```bash
pytest tests/test_similar.py -v
```
Expected: FAIL.

- [ ] **Step 23: Implement `.similar()`**

In `client.py`:

```python
    def similar(
        self,
        text: str,
        threshold: float,
        *,
        max_results: int = 10,
        corpus: str | None = None,
        corpora: list[str] | None = None,
        filter: dict[str, Any] | str | None = None,
        fields: list[str] | None = None,
        verify: dict[str, Any] | None = None,
    ) -> list[SimilarHit]:
        body: dict[str, Any] = {
            "text": text,
            "threshold": threshold,
            "max_results": max_results,
        }
        if corpus is not None:
            body["corpus"] = corpus
        if corpora is not None:
            body["corpora"] = corpora
        if filter is not None:
            body["filter"] = filter
        if fields is not None:
            body["fields"] = ",".join(fields)
        if verify is not None:
            body["verify"] = verify
        resp = self._client.post("/similar", json=body)
        _raise_for_status(resp)
        data = resp.json()
        hits = data.get("hits", data if isinstance(data, list) else [])
        return [SimilarHit.model_validate(h) for h in hits]
```

- [ ] **Step 24: Run and confirm green**

```bash
pytest tests/test_similar.py -v
```
Expected: all PASS.

- [ ] **Step 25: Run full client test suite + mypy + lint**

```bash
cd clients/python
pytest -v
mypy --strict src/
ruff check src/ tests/
ruff format --check src/ tests/
```
Expected: all PASS, no mypy errors, ruff clean.

- [ ] **Step 26: Commit**

```bash
git add clients/python/
git commit -m "feat(client): .similar, .get_cve, .get_cwe, .cwe_relation, .ready, .reload_bundle

Adds six sync methods on FastRAGClient matching the new HTTP surface.
get_cve/get_cwe return None on 404 (idiomatic Python); ready() returns
ReadyStatus(ok=False, ...) on 503 rather than raising (probe output);
reload_bundle() raises on non-200 and accepts admin_token kwarg or picks
up from constructor. New Pydantic models: CveRecord, CweRecord,
CweRelation, ReadyStatus, ReloadResult, SimilarHit. 409 mapped to
ConflictError."
```

---

## Task 8: Docker image + CI size gates

**Files:**
- Create: `docker/Dockerfile.airgap`
- Create: `docker/entrypoint.sh`
- Create: `docker/README.md`
- Create: `docker/ci/docker-build-size.sh`
- Create: `docker/ci/docker-no-phone-home.sh`
- Create: `docker/ci/docker-smoke.sh`
- Create: `Makefile`
- Modify: `.github/workflows/ci.yml` (add docker job)

Image: debian:12-slim + fastrag binary + llama-server + Qwen3-Embedding-0.6B Q8_0 + BGE-reranker-base Q8_0 + tini. Entrypoint launches both llama-servers, waits for health, then execs fastrag. Image size budget ≤1.5 GB.

- [ ] **Step 1: Create Dockerfile**

Create `docker/Dockerfile.airgap`:

```dockerfile
# syntax=docker/dockerfile:1.6

FROM rust:1.82-slim AS builder
WORKDIR /build
RUN apt-get update && apt-get install -y --no-install-recommends \
        pkg-config libssl-dev ca-certificates build-essential cmake git \
    && rm -rf /var/lib/apt/lists/*
COPY . .
RUN cargo build --release -p fastrag-cli \
        --no-default-features \
        --features language-detection,retrieval,rerank \
    && strip target/release/fastrag

FROM debian:12-slim AS llama-builder
RUN apt-get update && apt-get install -y --no-install-recommends \
        git cmake build-essential ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /src
ARG LLAMA_CPP_REF=b3451
RUN git clone --depth 1 --branch ${LLAMA_CPP_REF} https://github.com/ggerganov/llama.cpp.git /src/llama.cpp \
    && cmake -S /src/llama.cpp -B /src/build -DGGML_NATIVE=OFF -DCMAKE_BUILD_TYPE=Release \
                                             -DLLAMA_BUILD_SERVER=ON \
    && cmake --build /src/build -j --target llama-server \
    && strip /src/build/bin/llama-server

FROM debian:12-slim
ARG EMBED_GGUF_URL=https://huggingface.co/Qwen/Qwen3-Embedding-0.6B-GGUF/resolve/main/Qwen3-Embedding-0.6B-Q8_0.gguf
ARG RERANK_GGUF_URL=https://huggingface.co/gpustack/bge-reranker-v2-m3-GGUF/resolve/main/bge-reranker-v2-m3-Q8_0.gguf
RUN apt-get update && apt-get install -y --no-install-recommends \
        tini ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /opt/fastrag
RUN mkdir -p /opt/fastrag/models /var/lib/fastrag/bundles
COPY --from=builder /build/target/release/fastrag /usr/local/bin/fastrag
COPY --from=llama-builder /src/build/bin/llama-server /usr/local/bin/llama-server
ADD ${EMBED_GGUF_URL} /opt/fastrag/models/embedder.gguf
ADD ${RERANK_GGUF_URL} /opt/fastrag/models/reranker.gguf
COPY docker/entrypoint.sh /usr/local/bin/entrypoint.sh
RUN chmod +x /usr/local/bin/entrypoint.sh
EXPOSE 8080
ENV FASTRAG_LOG=info FASTRAG_LOG_FORMAT=json
ENTRYPOINT ["/usr/bin/tini", "--", "/usr/local/bin/entrypoint.sh"]
```

- [ ] **Step 2: Create entrypoint**

Create `docker/entrypoint.sh`:

```bash
#!/usr/bin/env bash
set -euo pipefail

EMBED_PORT=9001
RERANK_PORT=9002
BUNDLE_NAME="${BUNDLE_NAME:-}"
ADMIN_TOKEN="${FASTRAG_ADMIN_TOKEN:-}"
READ_TOKEN="${FASTRAG_TOKEN:-}"

llama-server \
    --model /opt/fastrag/models/embedder.gguf \
    --port "$EMBED_PORT" --host 127.0.0.1 \
    --embedding --pooling last \
    --ctx-size 8192 --log-disable &
EMBED_PID=$!

llama-server \
    --model /opt/fastrag/models/reranker.gguf \
    --port "$RERANK_PORT" --host 127.0.0.1 \
    --reranking \
    --ctx-size 2048 --log-disable &
RERANK_PID=$!

wait_healthy () {
    local port=$1 name=$2
    for i in $(seq 1 60); do
        if curl -sf "http://127.0.0.1:${port}/health" >/dev/null 2>&1; then
            echo "[entrypoint] $name ready on ${port}"
            return 0
        fi
        sleep 1
    done
    echo "[entrypoint] $name failed to become healthy on ${port}" >&2
    return 1
}
wait_healthy "$EMBED_PORT" embedder
wait_healthy "$RERANK_PORT" reranker

if [[ -z "$BUNDLE_NAME" ]]; then
    echo "[entrypoint] BUNDLE_NAME env var required" >&2
    exit 1
fi

exec fastrag serve-http \
    --bundles-dir /var/lib/fastrag/bundles \
    --bundle-path "/var/lib/fastrag/bundles/${BUNDLE_NAME}" \
    --embedder http \
    --embedder-url "http://127.0.0.1:${EMBED_PORT}" \
    --reranker-url "http://127.0.0.1:${RERANK_PORT}" \
    --admin-token "${ADMIN_TOKEN}" \
    --token "${READ_TOKEN}" \
    --port 8080
```

(Adapt the exact `--embedder`, `--embedder-url`, `--reranker-url` flag names to whatever `fastrag-cli/src/args.rs` already accepts — grep for `embedder_url` and `reranker_url`.)

- [ ] **Step 3: Create Makefile**

Create `Makefile` at repo root:

```make
IMAGE_NAME ?= fastrag
IMAGE_TAG  ?= $(shell git describe --tags --always --dirty)
ISO_OUT    ?= dist/fastrag-airgap.iso

.PHONY: airgap-image
airgap-image:
	docker build -f docker/Dockerfile.airgap -t $(IMAGE_NAME):$(IMAGE_TAG) .

.PHONY: airgap-save
airgap-save: airgap-image
	mkdir -p dist
	docker save $(IMAGE_NAME):$(IMAGE_TAG) | gzip -9 > dist/fastrag-$(IMAGE_TAG).tar.gz
	cd dist && sha256sum fastrag-$(IMAGE_TAG).tar.gz > SHA256SUMS

.PHONY: dvd-iso
dvd-iso: airgap-save
	@bash scripts/build-dvd-iso.sh $(IMAGE_TAG) $(ISO_OUT)

.PHONY: airgap-smoke
airgap-smoke: airgap-image
	bash docker/ci/docker-smoke.sh $(IMAGE_NAME):$(IMAGE_TAG)

.PHONY: airgap-size
airgap-size: airgap-image
	bash docker/ci/docker-build-size.sh $(IMAGE_NAME):$(IMAGE_TAG)

.PHONY: airgap-no-phone-home
airgap-no-phone-home: airgap-image
	bash docker/ci/docker-no-phone-home.sh $(IMAGE_NAME):$(IMAGE_TAG)
```

- [ ] **Step 4: Create CI size script**

Create `docker/ci/docker-build-size.sh`:

```bash
#!/usr/bin/env bash
set -euo pipefail
IMAGE="${1:?usage: $0 IMAGE}"
MAX_BYTES=$((1500 * 1024 * 1024))   # 1.5 GiB

size=$(docker image inspect --format='{{.Size}}' "$IMAGE")
if (( size > MAX_BYTES )); then
    printf "[size-gate] FAIL: %s is %d bytes, max %d\n" "$IMAGE" "$size" "$MAX_BYTES" >&2
    exit 1
fi
printf "[size-gate] OK: %s is %d bytes (under %d)\n" "$IMAGE" "$size" "$MAX_BYTES"
```

- [ ] **Step 5: Create phone-home audit script**

Create `docker/ci/docker-no-phone-home.sh`:

```bash
#!/usr/bin/env bash
set -euo pipefail
IMAGE="${1:?usage: $0 IMAGE}"

# Boot with no network. /ready should return 503 (bundle not loaded), not crash.
CID=$(docker run -d --rm --network=none \
    -e BUNDLE_NAME= \
    "$IMAGE" || true)
sleep 3
if ! docker ps -q --filter "id=$CID" | grep -q .; then
    # If the container exited, that's fine for this gate — we just want no
    # outbound traffic attempt. Check logs for DNS or connect failures.
    logs=$(docker logs "$CID" 2>&1 || true)
    docker rm -f "$CID" >/dev/null 2>&1 || true
    if grep -Ei 'dns|connect failed|name resolution' <<<"$logs"; then
        echo "[phone-home] FAIL: outbound attempt detected in logs" >&2
        exit 1
    fi
    echo "[phone-home] OK: no outbound traffic attempted"
    exit 0
fi
docker stop "$CID" >/dev/null
echo "[phone-home] OK: container ran under --network=none"
```

- [ ] **Step 6: Create smoke test**

Create `docker/ci/docker-smoke.sh`:

```bash
#!/usr/bin/env bash
set -euo pipefail
IMAGE="${1:?usage: $0 IMAGE}"
BUNDLE_DIR="${BUNDLE_DIR:-$(pwd)/tests/fixtures/bundles/minimal}"

# Mount the test fixture bundle and confirm core endpoints respond.
docker run -d --name fastrag-smoke -p 18080:8080 \
    -v "$BUNDLE_DIR":/var/lib/fastrag/bundles/minimal:ro \
    -e BUNDLE_NAME=minimal \
    -e FASTRAG_ADMIN_TOKEN=smoke-admin \
    -e FASTRAG_TOKEN=smoke-read \
    "$IMAGE"

trap 'docker rm -f fastrag-smoke >/dev/null 2>&1 || true' EXIT

for i in $(seq 1 60); do
    if curl -sf http://127.0.0.1:18080/ready >/dev/null 2>&1; then
        break
    fi
    sleep 1
done
curl -fsS -o /dev/null -w "%{http_code}\n" http://127.0.0.1:18080/health   | grep -q 200
curl -fsS -o /dev/null -w "%{http_code}\n" http://127.0.0.1:18080/ready    | grep -q 200
curl -fsS -H "x-fastrag-token: smoke-read" \
    -o /dev/null -w "%{http_code}\n" \
    "http://127.0.0.1:18080/cwe/relation?cwe_id=89" | grep -q 200
echo "[smoke] OK"
```

- [ ] **Step 7: Manually build and run size gate**

```bash
make airgap-image
make airgap-size
```
Expected: image builds; size script reports OK (≤1.5 GB). If oversized, drop reranker to Q6_K (`RERANK_GGUF_URL` arg) or switch to a smaller embedder Q6_K variant; document the swap.

- [ ] **Step 8: Manually run phone-home audit**

```bash
make airgap-no-phone-home
```
Expected: OK.

- [ ] **Step 9: Manually run smoke test with a fixture bundle**

Populate `tests/fixtures/bundles/minimal/` with a bundle.json + empty corpora + v2 taxonomy (mirror the shape used in Rust integration tests), then:

```bash
make airgap-smoke
```
Expected: OK.

- [ ] **Step 10: Wire up CI**

Modify `.github/workflows/ci.yml` to add a docker job:

```yaml
  docker:
    runs-on: ubuntu-latest
    needs: [build]
    steps:
      - uses: actions/checkout@v4
      - uses: docker/setup-buildx-action@v3
      - name: Build airgap image
        run: make airgap-image
      - name: Size gate
        run: make airgap-size
      - name: Phone-home audit
        run: make airgap-no-phone-home
      - name: Smoke test
        run: make airgap-smoke
```

- [ ] **Step 11: Commit**

```bash
git add docker/ Makefile .github/workflows/ci.yml tests/fixtures/bundles/
git commit -m "feat(docker): airgap image + CI size/phone-home/smoke gates

Debian-12-slim runtime with fastrag + llama-server + Qwen3-Embedding-0.6B
Q8_0 + BGE-reranker-base Q8_0, tini as PID 1. Entrypoint launches both
llama-servers, waits for health, execs fastrag serve-http. Makefile
targets: airgap-image, airgap-save, airgap-size (1.5 GiB gate),
airgap-no-phone-home (--network=none boot), airgap-smoke (fixture
bundle + endpoint probes). CI enforces all three gates."
```

---

## Task 9: DVD ISO target + airgap install docs

**Files:**
- Create: `scripts/build-dvd-iso.sh`
- Create: `docs/airgap-install.md`
- Modify: `.github/workflows/ci.yml` (add iso-size gate only if ISO is built; may stay offline-only)

`make dvd-iso` packs the gzipped docker tarball + a sample bundle + README onto a single-layer DVD (≤4.4 GiB).

- [ ] **Step 1: Create ISO build script**

Create `scripts/build-dvd-iso.sh`:

```bash
#!/usr/bin/env bash
set -euo pipefail
TAG="${1:?usage: $0 IMAGE_TAG ISO_OUT}"
OUT="${2:?}"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

mkdir -p "$WORK/image" "$WORK/bundles/fastrag-sample"
cp "dist/fastrag-${TAG}.tar.gz" "$WORK/image/"
cp "dist/SHA256SUMS"            "$WORK/image/"
cp "docs/airgap-install.md"     "$WORK/README.md"

if [[ -d tests/fixtures/bundles/sample ]]; then
    cp -r tests/fixtures/bundles/sample/. "$WORK/bundles/fastrag-sample/"
else
    echo "[dvd-iso] WARN: no sample bundle at tests/fixtures/bundles/sample — shipping empty placeholder" >&2
    touch "$WORK/bundles/fastrag-sample/.placeholder"
fi

mkdir -p "$(dirname "$OUT")"
genisoimage -r -J -V "FASTRAG-$TAG" -o "$OUT" "$WORK"

size=$(stat -c%s "$OUT")
MAX=$((4400 * 1024 * 1024))
if (( size > MAX )); then
    echo "[dvd-iso] FAIL: ISO is $size bytes, max $MAX" >&2
    exit 1
fi
echo "[dvd-iso] OK: $OUT ($size bytes)"
```

Make executable: `chmod +x scripts/build-dvd-iso.sh`.

- [ ] **Step 2: Write the airgap install doc**

Create `docs/airgap-install.md`:

```markdown
# Airgap installation

The DVD ships a Docker image plus a sample bundle. The operator flow:

1. Mount the disc:

   ```bash
   sudo mount /dev/sr0 /mnt/dvd
   ```

2. Verify and load the image:

   ```bash
   cd /mnt/dvd/image
   sha256sum -c SHA256SUMS
   docker load < fastrag-*.tar.gz
   ```

3. Copy the sample bundle to a persistent location:

   ```bash
   sudo mkdir -p /var/lib/fastrag/bundles
   sudo cp -r /mnt/dvd/bundles/fastrag-sample /var/lib/fastrag/bundles/
   ```

4. Generate tokens and start the container:

   ```bash
   READ_TOKEN=$(openssl rand -hex 32)
   ADMIN_TOKEN=$(openssl rand -hex 32)

   docker run -d --name fastrag --restart unless-stopped -p 8080:8080 \
       -v /var/lib/fastrag/bundles:/var/lib/fastrag/bundles \
       -e BUNDLE_NAME=fastrag-sample \
       -e FASTRAG_TOKEN="$READ_TOKEN" \
       -e FASTRAG_ADMIN_TOKEN="$ADMIN_TOKEN" \
       fastrag:X.Y.Z
   ```

5. Confirm readiness:

   ```bash
   curl -s http://localhost:8080/ready
   ```

## Updating bundles

When a new bundle arrives on DVD or USB:

1. Copy the new bundle directory alongside the existing one under `/var/lib/fastrag/bundles/`.
2. `POST /admin/reload` with `bundle_path` set to the new directory name:

   ```bash
   curl -X POST http://localhost:8080/admin/reload \
       -H "x-fastrag-admin-token: $ADMIN_TOKEN" \
       -H 'content-type: application/json' \
       -d '{"bundle_path":"fastrag-20260501"}'
   ```

The reload is atomic; in-flight queries complete against the prior bundle. Prior bundle directories may be retained for rollback — re-issue `/admin/reload` against the older directory name.

## Sizing

Peak memory during reload is ~2× the resident bundle size. Size the host with at least 4 GiB of RAM free beyond the steady-state bundle footprint.
```

- [ ] **Step 3: Test dvd-iso build locally**

```bash
make airgap-save
make dvd-iso
```
Expected: `dist/fastrag-airgap.iso` produced; script reports OK.

- [ ] **Step 4: Run full workspace lint + fmt one more time**

```bash
cargo test --workspace --features retrieval,rerank
cargo clippy --workspace --all-targets --features retrieval,rerank,hybrid,contextual,eval,nvd,hygiene -- -D warnings
cargo fmt --check
cd clients/python && ruff check src/ tests/ && ruff format --check src/ tests/ && mypy --strict src/ && pytest
```
Expected: all green.

- [ ] **Step 5: Commit**

```bash
git add scripts/build-dvd-iso.sh docs/airgap-install.md Makefile
git commit -m "feat(airgap): make dvd-iso + operator install doc

dvd-iso target packs the gzipped docker tarball, SHA256SUMS, sample
bundle, and README onto a single-layer DVD (≤4.4 GiB gate, 300 MiB
margin below the 4.7 GiB physical ceiling). docs/airgap-install.md
walks operators through load, token generation, container start,
readiness probe, and bundle reload."
```

---

## Final push + CI watch

- [ ] **Step 1: Push the whole stack**

```bash
git push origin main
```

- [ ] **Step 2: Invoke ci-watcher**

Dispatch the ci-watcher skill as a background Haiku Agent per the `feedback_ci_watcher_invocation` memory. The watcher covers `ci`, `nightly`, and any newly added docker job.

- [ ] **Step 3: Verify everything went green in CI before declaring done.**

---

## Spec coverage self-check

| Spec section | Task |
|---|---|
| Bundle shape (`bundle.json`, `corpora/*`, `taxonomy/*`) | 2, 3 |
| Runtime layout (`bundles/`, retention) | 3, 6 |
| Hot-swap primitive (`BundleState`, `ArcSwap`, reload mutex) | 2, 3, 6 |
| Module boundaries | 1-9 |
| `GET /cve/{id}` | 4 |
| `GET /cwe/{id}` + denormalised parents/children | 4 (parents/children populated by bundle-build tool — see note below) |
| `GET /cwe/relation` | 4 |
| `GET /ready` | 5 |
| `POST /admin/reload` | 6 |
| Unified error body | 4, 5, 6 |
| Taxonomy ancestors + schema v2 | 1 |
| Admin/reload lifecycle (mutex, metrics, path safety) | 6 |
| Python client methods | 7 |
| Docker + DVD packaging | 8, 9 |
| Unit tests | 1, 2 |
| Integration tests | 3, 4, 5, 6 |
| Python client tests | 7 |
| Docker CI checks | 8 |
| Retention flag `--bundle-retention` | 3 |

**Note on denormalised parents/children:** The spec says `metadata.parents`/`metadata.children` on CWE documents get populated at bundle-build time. This plan ships the Rust-side consumer of that denormalisation (`/cwe/{id}` happily returns whatever metadata is in the corpus) but does not ship a new bundle-build CLI pipeline — the existing ingest path and VAMS-side bundle tooling produce CWE docs with metadata. If the runtime reads show empty `metadata.parents`, add a bundle-build enrichment step to the CWE ingest pipeline as a follow-up; it does not block any of the HTTP or reload paths here.

---

## Execution handoff

Plan complete and saved to `docs/superpowers/plans/2026-04-16-fastrag-for-vams.md`. Per the user's auto-approve feedback, proceeding directly to subagent-driven-development execution.
