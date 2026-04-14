# CWE Hierarchy Expansion Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Query-time CWE descendant expansion so a query for CWE-89 also finds documents tagged with child CWEs like CWE-564.

**Architecture:** A new `fastrag-cwe` crate embeds a precomputed CWE-1000 descendant closure as JSON. A new `CweRewriter` walks filter ASTs and expands predicates on the configured CWE field. A free-text extraction pass adds a synthetic filter when the query contains CWE references. Behaviour is gated by `--cwe-expand` and defaults on when the corpus manifest records a `cwe_field`.

**Tech Stack:** Rust 2024, `serde_json` for taxonomy storage, `quick-xml` for MITRE XML parsing (already a workspace dep via `fastrag-eval`), `tantivy 0.22`, existing filter AST in `crates/fastrag/src/filter/`.

**Spec:** `docs/superpowers/specs/2026-04-13-cwe-hierarchy-expansion-design.md`

**Issue:** crook3dfingers/fastrag#47

---

## File Structure

**New:**
- `crates/fastrag-cwe/Cargo.toml`
- `crates/fastrag-cwe/src/lib.rs` — re-exports
- `crates/fastrag-cwe/src/taxonomy.rs` — `Taxonomy` struct, closure lookup, parse-from-bytes
- `crates/fastrag-cwe/src/data.rs` — `embedded()` via `include_bytes!`
- `crates/fastrag-cwe/src/bin/compile_taxonomy.rs` — offline regeneration tool
- `crates/fastrag-cwe/data/cwe-tree-v4.16.json` — committed precomputed closure
- `crates/fastrag-cwe/tests/fixtures/mini_cwe.xml` — tiny MITRE-shaped fixture for generator tests
- `crates/fastrag/src/filter/cwe_rewrite.rs` — `CweRewriter` AST walker
- `crates/fastrag/tests/cwe_expansion.rs` — end-to-end ingest+query test
- `fastrag-cli/tests/cwe_expand_e2e.rs` — CLI e2e
- `fastrag-cli/tests/cwe_expand_http_e2e.rs` — HTTP `/query` with `cwe_expand=true`

**Modified:**
- `Cargo.toml` (workspace) — add `crates/fastrag-cwe` member and workspace dep entry
- `crates/fastrag/Cargo.toml` — add `fastrag-cwe` dep (behind a `cwe-expand` feature)
- `crates/fastrag-index/src/manifest.rs` — add `cwe_field` and `cwe_taxonomy_version` to `CorpusManifest`
- `crates/fastrag/src/ingest/jsonl.rs` — plumb `cwe_field` through `JsonlIngestConfig`
- `crates/fastrag/src/filter/mod.rs` — `pub mod cwe_rewrite;`
- `crates/fastrag/src/corpus/mod.rs` — apply rewriter and free-text trigger in `query_corpus_with_filter`
- `fastrag-cli/src/args.rs` — `--cwe-field` on `Index`, `--cwe-expand`/`--no-cwe-expand` on `Query`, `--cwe-expand` on `ServeHttp`
- `fastrag-cli/src/main.rs` — pass flags through
- `fastrag-cli/src/http.rs` — parse `cwe_expand` query param
- `crates/fastrag/src/ingest/presets.rs` — set `cwe_field = "cwe_id"` in the tarmo-finding preset

---

## Task 1: Scaffold `fastrag-cwe` crate

**Files:**
- Create: `crates/fastrag-cwe/Cargo.toml`
- Create: `crates/fastrag-cwe/src/lib.rs`
- Modify: `Cargo.toml` (workspace root, lines 3–27 and 35–57)

- [ ] **Step 1: Create `crates/fastrag-cwe/Cargo.toml`**

```toml
[package]
name = "fastrag-cwe"
version.workspace = true
edition.workspace = true
license.workspace = true
repository.workspace = true

[dependencies]
serde = { workspace = true }
serde_json = { workspace = true }
thiserror = { workspace = true }
quick-xml = { version = "0.37", optional = true }

[[bin]]
name = "compile-taxonomy"
path = "src/bin/compile_taxonomy.rs"
required-features = ["compile-tool"]

[features]
compile-tool = ["dep:quick-xml"]
```

- [ ] **Step 2: Create `crates/fastrag-cwe/src/lib.rs`**

```rust
//! CWE taxonomy utilities. Provides a descendant-closure lookup compiled
//! from MITRE's CWE-1000 Research View.

pub mod data;
pub mod taxonomy;

pub use taxonomy::{Taxonomy, TaxonomyError};
```

- [ ] **Step 3: Add to workspace `Cargo.toml`**

In `/home/ubuntu/github/fastrag/Cargo.toml`, add to the `members` array (keep alphabetical-ish grouping with other fastrag- crates):

```toml
members = [
    ...existing...
    "crates/fastrag-cwe",
    ...
]
```

And add to `[workspace.dependencies]`:

```toml
fastrag-cwe = { path = "crates/fastrag-cwe", version = "0.1.0" }
```

- [ ] **Step 4: Stub `data.rs` and `taxonomy.rs` so the crate compiles**

Create `crates/fastrag-cwe/src/data.rs`:

```rust
//! Embedded taxonomy bytes. Populated in Task 4 once the committed JSON exists.
```

Create `crates/fastrag-cwe/src/taxonomy.rs`:

```rust
//! CWE descendant-closure taxonomy. Populated in Task 2.
```

- [ ] **Step 5: Verify workspace builds**

Run: `cargo build -p fastrag-cwe`
Expected: PASS (empty crate compiles).

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml crates/fastrag-cwe/
git commit -m "feat(cwe): scaffold fastrag-cwe crate

Empty crate skeleton with Cargo.toml and stub modules.

Refs: #47"
```

---

## Task 2: `Taxonomy` struct and closure lookup (TDD)

**Files:**
- Modify: `crates/fastrag-cwe/src/taxonomy.rs`

- [ ] **Step 1: Write the failing tests**

Replace `crates/fastrag-cwe/src/taxonomy.rs` with:

```rust
use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum TaxonomyError {
    #[error("malformed taxonomy JSON: {0}")]
    Parse(#[from] serde_json::Error),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Taxonomy {
    version: String,
    view: String,
    /// Map from CWE id → `[self, descendants...]` (self is first, rest sorted ascending).
    closure: HashMap<u32, Vec<u32>>,
}

impl Taxonomy {
    pub fn from_json(bytes: &[u8]) -> Result<Self, TaxonomyError> {
        let parsed: Taxonomy = serde_json::from_slice(bytes)?;
        Ok(parsed)
    }

    pub fn version(&self) -> &str {
        &self.version
    }

    pub fn view(&self) -> &str {
        &self.view
    }

    /// Return the closure for `cwe`. Falls back to a single-element slice
    /// holding `cwe` when the id is not present in the taxonomy.
    pub fn expand(&self, cwe: u32) -> Vec<u32> {
        match self.closure.get(&cwe) {
            Some(ids) => ids.clone(),
            None => vec![cwe],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_json() -> &'static str {
        r#"{
            "version": "4.16-test",
            "view": "1000",
            "closure": {
                "89": [89, 564, 943],
                "79": [79, 80, 81]
            }
        }"#
    }

    #[test]
    fn parses_version_and_view() {
        let tx = Taxonomy::from_json(fixture_json().as_bytes()).unwrap();
        assert_eq!(tx.version(), "4.16-test");
        assert_eq!(tx.view(), "1000");
    }

    #[test]
    fn expand_known_id_returns_closure() {
        let tx = Taxonomy::from_json(fixture_json().as_bytes()).unwrap();
        let got = tx.expand(89);
        assert!(got.contains(&89), "expand(89) missing self: {got:?}");
        assert!(got.contains(&564), "expand(89) missing child 564: {got:?}");
        assert!(got.contains(&943), "expand(89) missing child 943: {got:?}");
    }

    #[test]
    fn expand_unknown_id_returns_singleton() {
        let tx = Taxonomy::from_json(fixture_json().as_bytes()).unwrap();
        assert_eq!(tx.expand(9999), vec![9999]);
    }

    #[test]
    fn expand_is_idempotent_on_repeat_calls() {
        let tx = Taxonomy::from_json(fixture_json().as_bytes()).unwrap();
        let first = tx.expand(79);
        let second = tx.expand(79);
        assert_eq!(first, second);
    }

    #[test]
    fn malformed_json_errors() {
        let result = Taxonomy::from_json(b"not json");
        assert!(matches!(result, Err(TaxonomyError::Parse(_))));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p fastrag-cwe --lib`
Expected: compile error or test failures (the type already exists with the impl above — this step is primarily a sanity check. If tests fail because the crate didn't compile in Task 1, fix the stub). If all tests pass, move on.

- [ ] **Step 3: Implementation is complete from Step 1**

No additional code needed — Task 2 writes tests and implementation together because the `Taxonomy` struct is trivially a serde container. The tests from Step 1 exercise `from_json`, `version`, `view`, `expand` on both known and unknown ids, and error handling.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p fastrag-cwe --lib`
Expected: PASS. 5 tests.

- [ ] **Step 5: Commit**

```bash
git add crates/fastrag-cwe/src/taxonomy.rs
git commit -m "feat(cwe): Taxonomy struct with descendant-closure lookup

Parse embedded JSON, expand(id) returns [self, ...descendants], falls
back to [id] on unknown CWEs.

Refs: #47"
```

---

## Task 3: Compile-taxonomy binary tool (TDD on XML parse)

**Files:**
- Create: `crates/fastrag-cwe/src/bin/compile_taxonomy.rs`
- Create: `crates/fastrag-cwe/tests/fixtures/mini_cwe.xml`
- Create: `crates/fastrag-cwe/tests/compile_tool.rs`

- [ ] **Step 1: Create the fixture XML**

Create `crates/fastrag-cwe/tests/fixtures/mini_cwe.xml`:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<Weakness_Catalog Version="4.16-test">
  <Weaknesses>
    <Weakness ID="89" Name="SQL Injection">
      <Related_Weaknesses>
        <Related_Weakness Nature="ChildOf" CWE_ID="943" View_ID="1000"/>
      </Related_Weaknesses>
    </Weakness>
    <Weakness ID="564" Name="Hibernate Injection">
      <Related_Weaknesses>
        <Related_Weakness Nature="ChildOf" CWE_ID="89" View_ID="1000"/>
      </Related_Weaknesses>
    </Weakness>
    <Weakness ID="943" Name="Improper Neutralization of Special Elements in Data Query Logic">
      <Related_Weaknesses>
        <Related_Weakness Nature="ChildOf" CWE_ID="74" View_ID="1000"/>
      </Related_Weaknesses>
    </Weakness>
    <Weakness ID="74" Name="Injection">
      <Related_Weaknesses/>
    </Weakness>
  </Weaknesses>
</Weakness_Catalog>
```

This encodes the chain: 564 → 89 → 943 → 74. Expected closures:
- 74: {74, 943, 89, 564}
- 943: {943, 89, 564}
- 89: {89, 564}
- 564: {564}

- [ ] **Step 2: Write the failing integration test**

Create `crates/fastrag-cwe/tests/compile_tool.rs`:

```rust
//! Test the XML → closure-JSON transformation used by `compile-taxonomy`.

use std::path::PathBuf;

#[test]
fn builds_closure_from_mini_fixture() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mini_cwe.xml");
    let bytes = std::fs::read(&fixture).expect("fixture exists");

    let taxonomy = fastrag_cwe::compile::build_closure(&bytes, "1000")
        .expect("build_closure succeeds");

    assert_eq!(taxonomy.version(), "4.16-test");
    assert_eq!(taxonomy.view(), "1000");

    let c74 = taxonomy.expand(74);
    assert!(c74.contains(&74));
    assert!(c74.contains(&943));
    assert!(c74.contains(&89));
    assert!(c74.contains(&564));

    let c89 = taxonomy.expand(89);
    assert_eq!(c89.len(), 2, "expand(89) should include 89 and 564");
    assert!(c89.contains(&89));
    assert!(c89.contains(&564));

    let c564 = taxonomy.expand(564);
    assert_eq!(c564, vec![564]);
}
```

- [ ] **Step 3: Run the test to verify it fails**

Run: `cargo test -p fastrag-cwe --test compile_tool --features compile-tool`
Expected: FAIL — module `compile` doesn't exist.

- [ ] **Step 4: Add the `compile` module to the library**

Append to `crates/fastrag-cwe/src/lib.rs`:

```rust
#[cfg(feature = "compile-tool")]
pub mod compile;
```

Create `crates/fastrag-cwe/src/compile.rs`:

```rust
//! XML → closure-JSON compilation. Only built when the `compile-tool`
//! feature is enabled; not part of the runtime library.

use std::collections::{HashMap, HashSet};
use std::io::BufReader;

use quick_xml::Reader;
use quick_xml::events::Event;
use thiserror::Error;

use crate::taxonomy::Taxonomy;

#[derive(Debug, Error)]
pub enum CompileError {
    #[error("xml parse error: {0}")]
    Xml(String),
    #[error("catalog version attribute missing")]
    MissingVersion,
}

/// Parse a MITRE CWE XML catalog and build the descendant closure for `view_id`.
/// `view_id` is the string "1000" for the Research View.
pub fn build_closure(xml_bytes: &[u8], view_id: &str) -> Result<Taxonomy, CompileError> {
    let (version, parents) = parse_catalog(xml_bytes, view_id)?;
    let closure = compute_closure(&parents);
    Ok(Taxonomy::from_components(version, view_id.to_string(), closure))
}

/// Returns (version, child_id → [parent_id, ...]) for edges matching `view_id`.
fn parse_catalog(xml_bytes: &[u8], view_id: &str) -> Result<(String, HashMap<u32, Vec<u32>>), CompileError> {
    let mut reader = Reader::from_reader(BufReader::new(xml_bytes));
    reader.config_mut().trim_text(true);

    let mut version: Option<String> = None;
    let mut parents: HashMap<u32, Vec<u32>> = HashMap::new();
    let mut current_cwe: Option<u32> = None;
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let name = local_name(e.name().as_ref()).to_vec();
                if version.is_none() && name == b"Weakness_Catalog" {
                    version = read_attr(e, b"Version");
                } else if name == b"Weakness" {
                    current_cwe = read_attr(e, b"ID").and_then(|s| s.parse().ok());
                } else if name == b"Related_Weakness" {
                    if let (Some(child), Some(nature), Some(view), Some(parent)) = (
                        current_cwe,
                        read_attr(e, b"Nature"),
                        read_attr(e, b"View_ID"),
                        read_attr(e, b"CWE_ID").and_then(|s| s.parse().ok()),
                    ) {
                        if nature == "ChildOf" && view == view_id {
                            parents.entry(child).or_default().push(parent);
                        }
                    }
                }
            }
            Ok(Event::Empty(ref e)) => {
                // Same handling as Start for self-closing tags.
                let name = local_name(e.name().as_ref()).to_vec();
                if name == b"Related_Weakness" {
                    if let (Some(child), Some(nature), Some(view), Some(parent)) = (
                        current_cwe,
                        read_attr(e, b"Nature"),
                        read_attr(e, b"View_ID"),
                        read_attr(e, b"CWE_ID").and_then(|s| s.parse().ok()),
                    ) {
                        if nature == "ChildOf" && view == view_id {
                            parents.entry(child).or_default().push(parent);
                        }
                    }
                }
            }
            Ok(Event::End(ref e)) => {
                if local_name(e.name().as_ref()) == b"Weakness" {
                    current_cwe = None;
                }
            }
            Ok(Event::Eof) => break,
            Err(err) => return Err(CompileError::Xml(err.to_string())),
            _ => {}
        }
        buf.clear();
    }

    let version = version.ok_or(CompileError::MissingVersion)?;
    Ok((version, parents))
}

/// Invert a child→parents map and compute descendant closure for each node.
/// Each closure is `[self, ...descendants]` with the self element first.
fn compute_closure(parents: &HashMap<u32, Vec<u32>>) -> HashMap<u32, Vec<u32>> {
    // Build children map.
    let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
    let mut all_nodes: HashSet<u32> = HashSet::new();
    for (child, ps) in parents {
        all_nodes.insert(*child);
        for p in ps {
            all_nodes.insert(*p);
            children.entry(*p).or_default().push(*child);
        }
    }

    // BFS descendants per node.
    let mut closures: HashMap<u32, Vec<u32>> = HashMap::new();
    for &node in &all_nodes {
        let mut seen: HashSet<u32> = HashSet::new();
        seen.insert(node);
        let mut queue: Vec<u32> = vec![node];
        let mut idx = 0;
        while idx < queue.len() {
            let cur = queue[idx];
            idx += 1;
            if let Some(ch) = children.get(&cur) {
                for &c in ch {
                    if seen.insert(c) {
                        queue.push(c);
                    }
                }
            }
        }
        // Self first, then ascending descendants.
        let mut rest: Vec<u32> = queue.into_iter().filter(|id| *id != node).collect();
        rest.sort_unstable();
        let mut out = Vec::with_capacity(rest.len() + 1);
        out.push(node);
        out.extend(rest);
        closures.insert(node, out);
    }

    closures
}

fn local_name(name: &[u8]) -> &[u8] {
    match name.iter().rposition(|b| *b == b':') {
        Some(idx) => &name[idx + 1..],
        None => name,
    }
}

fn read_attr(e: &quick_xml::events::BytesStart<'_>, key: &[u8]) -> Option<String> {
    e.attributes().flatten().find_map(|a| {
        if a.key.as_ref() == key {
            Some(String::from_utf8_lossy(&a.value).to_string())
        } else {
            None
        }
    })
}
```

- [ ] **Step 5: Add the `from_components` constructor to `Taxonomy`**

In `crates/fastrag-cwe/src/taxonomy.rs`, add to `impl Taxonomy`:

```rust
impl Taxonomy {
    // ...existing methods...

    /// Internal constructor used by the compile tool. Not part of the public
    /// runtime API.
    pub fn from_components(version: String, view: String, closure: HashMap<u32, Vec<u32>>) -> Self {
        Self { version, view, closure }
    }
}
```

- [ ] **Step 6: Make `quick-xml` required when `compile-tool` feature is on**

In `crates/fastrag-cwe/Cargo.toml`, update:

```toml
[features]
compile-tool = ["dep:quick-xml"]

[dependencies.quick-xml]
version = "0.37"
optional = true
```

(Already in the scaffold from Task 1, but verify.)

- [ ] **Step 7: Create the binary**

Create `crates/fastrag-cwe/src/bin/compile_taxonomy.rs`:

```rust
//! Offline taxonomy regeneration tool. Parses a MITRE CWE XML catalog and
//! writes the descendant closure as JSON.
//!
//! Usage:
//!   cargo run -p fastrag-cwe --features compile-tool --bin compile-taxonomy -- \
//!     --in path/to/cwec_v4.16.xml \
//!     --out crates/fastrag-cwe/data/cwe-tree-v4.16.json

use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let mut input: Option<PathBuf> = None;
    let mut output: Option<PathBuf> = None;
    let mut view = String::from("1000");

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--in" => input = args.next().map(PathBuf::from),
            "--out" => output = args.next().map(PathBuf::from),
            "--view" => view = args.next().unwrap_or(view),
            "--help" | "-h" => {
                println!("Usage: compile-taxonomy --in INPUT.xml --out OUTPUT.json [--view 1000]");
                return Ok(());
            }
            other => return Err(format!("unknown arg: {other}").into()),
        }
    }

    let input = input.ok_or("--in required")?;
    let output = output.ok_or("--out required")?;

    let xml = std::fs::read(&input)?;
    let taxonomy = fastrag_cwe::compile::build_closure(&xml, &view)?;
    let json = serde_json::to_string_pretty(&taxonomy)?;
    std::fs::write(&output, json)?;
    println!(
        "wrote taxonomy (version={}, view={}) to {}",
        taxonomy.version(),
        taxonomy.view(),
        output.display()
    );
    Ok(())
}
```

- [ ] **Step 8: Run the integration test**

Run: `cargo test -p fastrag-cwe --test compile_tool --features compile-tool`
Expected: PASS.

- [ ] **Step 9: Verify unit tests still pass**

Run: `cargo test -p fastrag-cwe --features compile-tool`
Expected: all tests PASS.

- [ ] **Step 10: Commit**

```bash
git add crates/fastrag-cwe/Cargo.toml crates/fastrag-cwe/src/ crates/fastrag-cwe/tests/
git commit -m "feat(cwe): compile-taxonomy tool for MITRE XML → closure JSON

Offline binary parses the CWE catalog and writes a precomputed
descendant closure for the given view. Tested on a 4-node mini
fixture exercising the 564→89→943→74 chain.

Refs: #47"
```

---

## Task 4: Generate and commit the production taxonomy JSON

**Files:**
- Create: `crates/fastrag-cwe/data/cwe-tree-v4.16.json` (large, generated)
- Modify: `crates/fastrag-cwe/src/data.rs`
- Modify: `crates/fastrag-cwe/src/taxonomy.rs` (add `embedded()` + tests)

- [ ] **Step 1: Download MITRE CWE XML to a scratch path**

The `fastrag-eval` crate already has a downloader. For this one-shot, a direct `curl` is simplest:

```bash
mkdir -p /tmp/cwe-src
curl -L -o /tmp/cwe-src/cwec_latest.xml.zip https://cwe.mitre.org/data/xml/cwec_latest.xml.zip
unzip -o /tmp/cwe-src/cwec_latest.xml.zip -d /tmp/cwe-src/
ls /tmp/cwe-src/cwec_v*.xml
```

Expected: one file like `cwec_v4.16.xml`. Note the version number for the output filename.

- [ ] **Step 2: Run the compile tool**

```bash
mkdir -p crates/fastrag-cwe/data
cargo run -p fastrag-cwe --features compile-tool --bin compile-taxonomy -- \
    --in /tmp/cwe-src/cwec_v4.16.xml \
    --out crates/fastrag-cwe/data/cwe-tree-v4.16.json \
    --view 1000
```

Expected output: `wrote taxonomy (version=4.16, view=1000) to crates/fastrag-cwe/data/cwe-tree-v4.16.json`. If the actual version string differs (e.g., `4.17`), use that filename instead and update references in Tasks 4 and 5 accordingly.

- [ ] **Step 3: Sanity-check the generated file**

```bash
python3 -c "import json; t = json.load(open('crates/fastrag-cwe/data/cwe-tree-v4.16.json')); \
    print('view=', t['view']); \
    print('version=', t['version']); \
    print('entries=', len(t['closure'])); \
    print('CWE-89 closure size=', len(t['closure'].get('89', []))); \
    print('CWE-79 closure size=', len(t['closure'].get('79', [])))"
```

Expected: `view=1000`, hundreds of entries, CWE-89 closure size ≥ 2, CWE-79 closure size ≥ 2.

- [ ] **Step 4: Write the failing test for `embedded()`**

Append to `crates/fastrag-cwe/src/taxonomy.rs` tests module:

```rust
    #[test]
    fn embedded_loads_and_contains_known_cwes() {
        let tx = super::super::data::embedded();
        // View must match what we generated.
        assert_eq!(tx.view(), "1000");
        // Sanity: CWE-89 (SQL Injection) exists in view 1000.
        let c89 = tx.expand(89);
        assert!(c89.contains(&89), "CWE-89 closure missing self");
        assert!(c89.len() >= 2, "CWE-89 should have at least one descendant");
        // Unknown id falls through.
        assert_eq!(tx.expand(999_999), vec![999_999]);
    }
```

- [ ] **Step 5: Run to verify it fails**

Run: `cargo test -p fastrag-cwe --lib -- embedded_loads_and_contains_known_cwes`
Expected: FAIL — `data::embedded` doesn't exist yet.

- [ ] **Step 6: Implement `embedded()`**

Replace `crates/fastrag-cwe/src/data.rs` with:

```rust
//! Embedded CWE taxonomy. Bytes are compiled into the binary via `include_bytes!`
//! so no filesystem access is needed at runtime.

use std::sync::OnceLock;

use crate::Taxonomy;

const TAXONOMY_BYTES: &[u8] = include_bytes!("../data/cwe-tree-v4.16.json");

static TAXONOMY: OnceLock<Taxonomy> = OnceLock::new();

/// Return the embedded CWE taxonomy. Parsed lazily on first call and cached.
///
/// Panics if the embedded JSON is malformed. This is a build-time invariant:
/// the JSON is generated by `compile-taxonomy` and committed. If this panics,
/// the committed data is corrupt.
pub fn embedded() -> &'static Taxonomy {
    TAXONOMY.get_or_init(|| {
        Taxonomy::from_json(TAXONOMY_BYTES).expect("embedded CWE taxonomy must parse")
    })
}
```

- [ ] **Step 7: Run tests to verify they pass**

Run: `cargo test -p fastrag-cwe --lib`
Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add crates/fastrag-cwe/data/ crates/fastrag-cwe/src/data.rs crates/fastrag-cwe/src/taxonomy.rs
git commit -m "feat(cwe): commit CWE-1000 v4.16 closure and embed at compile time

data/cwe-tree-v4.16.json generated via compile-taxonomy from the
MITRE catalog (CWE-1000 Research View). Loaded lazily via OnceLock
on first embedded() call.

Refs: #47"
```

---

## Task 5: Extend `CorpusManifest` with `cwe_field` and `cwe_taxonomy_version`

**Files:**
- Modify: `crates/fastrag-index/src/manifest.rs:6-22`

- [ ] **Step 1: Write the failing test**

Append to the existing `#[cfg(test)] mod v5_tests` in `crates/fastrag-index/src/manifest.rs`:

```rust
    #[test]
    fn v5_with_cwe_field_roundtrip() {
        let mut m = CorpusManifest::new(
            sample_identity(),
            sample_canary(),
            1,
            ManifestChunkingStrategy::Basic {
                max_characters: 100,
                overlap: 0,
            },
        );
        m.cwe_field = Some("cwe_id".to_string());
        m.cwe_taxonomy_version = Some("4.16".to_string());
        let s = serde_json::to_string(&m).unwrap();
        let back: CorpusManifest = serde_json::from_str(&s).unwrap();
        assert_eq!(back, m);
        assert_eq!(back.cwe_field.as_deref(), Some("cwe_id"));
        assert_eq!(back.cwe_taxonomy_version.as_deref(), Some("4.16"));
    }

    #[test]
    fn v5_without_cwe_fields_deserializes() {
        // Older writer without the new fields.
        let m_ref = CorpusManifest::new(
            sample_identity(),
            sample_canary(),
            1,
            ManifestChunkingStrategy::Basic {
                max_characters: 100,
                overlap: 0,
            },
        );
        let mut value = serde_json::to_value(&m_ref).unwrap();
        value.as_object_mut().unwrap().remove("cwe_field");
        value.as_object_mut().unwrap().remove("cwe_taxonomy_version");
        let json = serde_json::to_string(&value).unwrap();
        let m: CorpusManifest = serde_json::from_str(&json).unwrap();
        assert!(m.cwe_field.is_none());
        assert!(m.cwe_taxonomy_version.is_none());
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p fastrag-index manifest::v5_tests::v5_with_cwe_field_roundtrip`
Expected: FAIL — field doesn't exist.

- [ ] **Step 3: Add the fields**

In `crates/fastrag-index/src/manifest.rs`, update the `CorpusManifest` struct:

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CorpusManifest {
    pub version: u32,
    pub identity: EmbedderIdentity,
    pub canary: Canary,
    pub created_at_unix_seconds: u64,
    pub chunk_count: usize,
    pub chunking_strategy: ManifestChunkingStrategy,
    #[serde(default)]
    pub roots: Vec<RootEntry>,
    #[serde(default)]
    pub files: Vec<FileEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contextualizer: Option<ContextualizerManifest>,
    /// Name of the record field that carries the CWE numeric id. Set at
    /// ingest time via `--cwe-field`. When present, query-time CWE
    /// expansion defaults on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwe_field: Option<String>,
    /// Version string of the CWE taxonomy used when this corpus was built.
    /// Written by the ingest path when `cwe_field` is set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwe_taxonomy_version: Option<String>,
}
```

Update `CorpusManifest::new` to initialize them:

```rust
impl CorpusManifest {
    pub fn new(
        identity: EmbedderIdentity,
        canary: Canary,
        created_at_unix_seconds: u64,
        chunking_strategy: ManifestChunkingStrategy,
    ) -> Self {
        Self {
            version: 5,
            identity,
            canary,
            created_at_unix_seconds,
            chunk_count: 0,
            chunking_strategy,
            roots: Vec::new(),
            files: Vec::new(),
            contextualizer: None,
            cwe_field: None,
            cwe_taxonomy_version: None,
        }
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p fastrag-index`
Expected: PASS. All prior manifest tests plus the two new ones.

- [ ] **Step 5: Commit**

```bash
git add crates/fastrag-index/src/manifest.rs
git commit -m "feat(index): add cwe_field + cwe_taxonomy_version to CorpusManifest

Optional fields, absent on legacy manifests (serde default). Populated
at ingest time when --cwe-field is passed.

Refs: #47"
```

---

## Task 6: `--cwe-field` ingest flag and preset plumbing

**Files:**
- Modify: `crates/fastrag/src/ingest/jsonl.rs` (`JsonlIngestConfig` struct + default_config helper)
- Modify: `crates/fastrag/src/ingest/presets.rs` (add `cwe_field` to preset)
- Modify: `fastrag-cli/src/args.rs` (add `cwe_field` flag to `Index`)
- Modify: `fastrag-cli/src/main.rs` (pass through + write to manifest)

- [ ] **Step 1: Extend `JsonlIngestConfig`**

In `crates/fastrag/src/ingest/jsonl.rs`, at the struct definition:

```rust
#[derive(Debug, Clone)]
pub struct JsonlIngestConfig {
    pub text_fields: Vec<String>,
    pub id_field: String,
    pub metadata_fields: Vec<String>,
    pub metadata_types: BTreeMap<String, TypedKind>,
    pub array_fields: Vec<String>,
    /// Name of the record field holding the CWE numeric id. Written to the
    /// corpus manifest so query-time expansion can find it.
    pub cwe_field: Option<String>,
}
```

Update every construction site of `JsonlIngestConfig` in this file to initialize `cwe_field: None`. Grep: `rg -n "JsonlIngestConfig \{" crates/fastrag/` and update each.

Likely sites:
- `crates/fastrag/src/ingest/jsonl.rs:268` (default_config helper in tests)
- `crates/fastrag/src/ingest/presets.rs` (tarmo-finding preset)

- [ ] **Step 2: Set `cwe_field` in the tarmo-finding preset**

In `crates/fastrag/src/ingest/presets.rs`, update the return:

```rust
pub fn tarmo_finding_preset() -> JsonlIngestConfig {
    JsonlIngestConfig {
        // ...existing fields...
        array_fields: vec![
            // ...existing...
        ],
        cwe_field: Some("cwe_id".into()),
    }
}
```

And add a test:

```rust
    #[test]
    fn tarmo_preset_sets_cwe_field() {
        let cfg = tarmo_finding_preset();
        assert_eq!(cfg.cwe_field.as_deref(), Some("cwe_id"));
    }
```

- [ ] **Step 3: Add `--cwe-field` to the CLI `Index` subcommand**

In `fastrag-cli/src/args.rs`, alongside the other JSONL fields (near `id_field`, `metadata_fields`, etc., around line 258-281):

```rust
        /// JSONL: name of the field holding the CWE numeric id. Enables
        /// query-time CWE hierarchy expansion for this corpus.
        #[cfg(feature = "store")]
        #[arg(long)]
        cwe_field: Option<String>,
```

- [ ] **Step 4: Plumb through `main.rs`**

In `fastrag-cli/src/main.rs`, find the `Index` match arm handling (search for `cwe_field` near where `id_field` is consumed). Update the `JsonlIngestConfig` construction to pass `cwe_field`. And after the ingest writes the manifest, set `manifest.cwe_field` and `manifest.cwe_taxonomy_version`.

Use `rg -n "JsonlIngestConfig" fastrag-cli/` to locate the call site. Where the config is built, add:

```rust
let cwe_field = cwe_field.clone();  // from CLI arg
let cwe_field = match (cwe_field, preset.as_ref()) {
    (Some(s), _) => Some(s),
    (None, _) => cfg.cwe_field.clone(),  // preset may set it
};
```

(Adjust names to match the actual variable bindings in `main.rs`.)

After ingest completes, where the manifest is finalized/written, set:

```rust
if let Some(field) = cwe_field {
    manifest.cwe_field = Some(field);
    manifest.cwe_taxonomy_version = Some(fastrag_cwe::data::embedded().version().to_string());
}
```

This requires adding `fastrag-cwe` as a dependency of `fastrag-cli` (and/or `fastrag`):

Update `fastrag-cli/Cargo.toml`:

```toml
[dependencies]
# ...existing...
fastrag-cwe = { workspace = true }
```

- [ ] **Step 5: Run tests**

Run: `cargo test -p fastrag --lib ingest::presets`
Expected: PASS.

Run: `cargo build -p fastrag-cli --features "store,retrieval"`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/fastrag/src/ingest/ fastrag-cli/src/args.rs fastrag-cli/src/main.rs fastrag-cli/Cargo.toml
git commit -m "feat(ingest): --cwe-field flag, plumb into manifest

Records the CWE field name and taxonomy version in the corpus
manifest. Tarmo-finding preset sets cwe_field='cwe_id' by default.

Refs: #47"
```

---

## Task 7: `CweRewriter` filter AST walker (TDD)

**Files:**
- Create: `crates/fastrag/src/filter/cwe_rewrite.rs`
- Modify: `crates/fastrag/src/filter/mod.rs`
- Modify: `crates/fastrag/Cargo.toml` (add `fastrag-cwe` dep)

- [ ] **Step 1: Add dependency**

In `crates/fastrag/Cargo.toml`:

```toml
[dependencies]
# ...existing...
fastrag-cwe = { workspace = true }
```

- [ ] **Step 2: Wire the module**

In `crates/fastrag/src/filter/mod.rs`, add:

```rust
pub mod ast;
pub mod cwe_rewrite;
pub mod eval;
pub mod parser;

pub use ast::FilterExpr;
pub use cwe_rewrite::CweRewriter;
pub use eval::matches;
pub use parser::{FilterParseError, parse};
```

- [ ] **Step 3: Write the failing test module**

Create `crates/fastrag/src/filter/cwe_rewrite.rs`:

```rust
//! AST walker that expands filter predicates on the configured CWE field
//! into their descendant closures, using an embedded MITRE taxonomy.

use std::collections::BTreeSet;

use fastrag_cwe::Taxonomy;
use fastrag_store::schema::TypedValue;

use super::ast::FilterExpr;

/// Rewriter that expands equality/membership predicates on the CWE field
/// to include all descendant CWEs. Non-CWE predicates and range operators
/// pass through unchanged.
pub struct CweRewriter<'a> {
    taxonomy: &'a Taxonomy,
    cwe_field: &'a str,
}

impl<'a> CweRewriter<'a> {
    pub fn new(taxonomy: &'a Taxonomy, cwe_field: &'a str) -> Self {
        Self { taxonomy, cwe_field }
    }

    /// Recursively rewrite `expr`. Consumes and returns a fresh tree; nodes
    /// not affected are cloned unchanged.
    pub fn rewrite(&self, expr: FilterExpr) -> FilterExpr {
        match expr {
            FilterExpr::Eq { field, value } if field == self.cwe_field => {
                if let Some(n) = as_cwe_u32(&value) {
                    let values = expand_to_typed(self.taxonomy, &[n]);
                    FilterExpr::In { field, values }
                } else {
                    FilterExpr::Eq { field, value }
                }
            }
            FilterExpr::Neq { field, value } if field == self.cwe_field => {
                if let Some(n) = as_cwe_u32(&value) {
                    let values = expand_to_typed(self.taxonomy, &[n]);
                    FilterExpr::NotIn { field, values }
                } else {
                    FilterExpr::Neq { field, value }
                }
            }
            FilterExpr::In { field, values } if field == self.cwe_field => {
                let ids = collect_cwe_u32(&values);
                if ids.is_empty() {
                    FilterExpr::In { field, values }
                } else {
                    let expanded = expand_to_typed(self.taxonomy, &ids);
                    FilterExpr::In { field, values: expanded }
                }
            }
            FilterExpr::NotIn { field, values } if field == self.cwe_field => {
                let ids = collect_cwe_u32(&values);
                if ids.is_empty() {
                    FilterExpr::NotIn { field, values }
                } else {
                    let expanded = expand_to_typed(self.taxonomy, &ids);
                    FilterExpr::NotIn { field, values: expanded }
                }
            }
            FilterExpr::And(children) => {
                FilterExpr::And(children.into_iter().map(|c| self.rewrite(c)).collect())
            }
            FilterExpr::Or(children) => {
                FilterExpr::Or(children.into_iter().map(|c| self.rewrite(c)).collect())
            }
            FilterExpr::Not(inner) => FilterExpr::Not(Box::new(self.rewrite(*inner))),
            other => other,
        }
    }
}

fn as_cwe_u32(v: &TypedValue) -> Option<u32> {
    match v {
        TypedValue::Numeric(n) if *n >= 0.0 && n.fract() == 0.0 && *n <= u32::MAX as f64 => {
            Some(*n as u32)
        }
        TypedValue::String(s) => s.parse::<u32>().ok(),
        _ => None,
    }
}

fn collect_cwe_u32(values: &[TypedValue]) -> Vec<u32> {
    values.iter().filter_map(as_cwe_u32).collect()
}

fn expand_to_typed(tx: &Taxonomy, ids: &[u32]) -> Vec<TypedValue> {
    let mut merged: BTreeSet<u32> = BTreeSet::new();
    for id in ids {
        for d in tx.expand(*id) {
            merged.insert(d);
        }
    }
    merged
        .into_iter()
        .map(|n| TypedValue::Numeric(n as f64))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tiny_taxonomy() -> Taxonomy {
        let json = r#"{
            "version": "test",
            "view": "1000",
            "closure": {
                "89":  [89, 564, 943],
                "79":  [79, 80, 81]
            }
        }"#;
        Taxonomy::from_json(json.as_bytes()).unwrap()
    }

    fn numeric_values(vs: &[TypedValue]) -> Vec<u32> {
        vs.iter()
            .map(|v| match v {
                TypedValue::Numeric(n) => *n as u32,
                other => panic!("expected Numeric, got {other:?}"),
            })
            .collect()
    }

    #[test]
    fn eq_on_cwe_field_expands_to_in() {
        let tx = tiny_taxonomy();
        let r = CweRewriter::new(&tx, "cwe_id");
        let input = FilterExpr::Eq {
            field: "cwe_id".into(),
            value: TypedValue::Numeric(89.0),
        };
        let out = r.rewrite(input);
        match out {
            FilterExpr::In { field, values } => {
                assert_eq!(field, "cwe_id");
                let mut ids = numeric_values(&values);
                ids.sort();
                assert_eq!(ids, vec![89, 564, 943]);
            }
            other => panic!("expected In, got {other:?}"),
        }
    }

    #[test]
    fn in_on_cwe_field_expands_and_dedups() {
        let tx = tiny_taxonomy();
        let r = CweRewriter::new(&tx, "cwe_id");
        let input = FilterExpr::In {
            field: "cwe_id".into(),
            values: vec![TypedValue::Numeric(89.0), TypedValue::Numeric(79.0)],
        };
        let out = r.rewrite(input);
        match out {
            FilterExpr::In { field, values } => {
                assert_eq!(field, "cwe_id");
                let mut ids = numeric_values(&values);
                ids.sort();
                assert_eq!(ids, vec![79, 80, 81, 89, 564, 943]);
            }
            other => panic!("expected In, got {other:?}"),
        }
    }

    #[test]
    fn neq_on_cwe_field_expands_to_not_in() {
        let tx = tiny_taxonomy();
        let r = CweRewriter::new(&tx, "cwe_id");
        let input = FilterExpr::Neq {
            field: "cwe_id".into(),
            value: TypedValue::Numeric(89.0),
        };
        let out = r.rewrite(input);
        match out {
            FilterExpr::NotIn { field, values } => {
                assert_eq!(field, "cwe_id");
                let mut ids = numeric_values(&values);
                ids.sort();
                assert_eq!(ids, vec![89, 564, 943]);
            }
            other => panic!("expected NotIn, got {other:?}"),
        }
    }

    #[test]
    fn non_cwe_field_passes_through_unchanged() {
        let tx = tiny_taxonomy();
        let r = CweRewriter::new(&tx, "cwe_id");
        let input = FilterExpr::Eq {
            field: "severity".into(),
            value: TypedValue::String("HIGH".into()),
        };
        let out = r.rewrite(input.clone());
        assert_eq!(out, input);
    }

    #[test]
    fn range_operators_on_cwe_pass_through() {
        let tx = tiny_taxonomy();
        let r = CweRewriter::new(&tx, "cwe_id");
        let input = FilterExpr::Gt {
            field: "cwe_id".into(),
            value: TypedValue::Numeric(89.0),
        };
        let out = r.rewrite(input.clone());
        assert_eq!(out, input);
    }

    #[test]
    fn nested_and_or_not_recurses() {
        let tx = tiny_taxonomy();
        let r = CweRewriter::new(&tx, "cwe_id");
        let input = FilterExpr::And(vec![
            FilterExpr::Eq {
                field: "severity".into(),
                value: TypedValue::String("HIGH".into()),
            },
            FilterExpr::Or(vec![
                FilterExpr::Eq {
                    field: "cwe_id".into(),
                    value: TypedValue::Numeric(89.0),
                },
                FilterExpr::Not(Box::new(FilterExpr::Neq {
                    field: "cwe_id".into(),
                    value: TypedValue::Numeric(79.0),
                })),
            ]),
        ]);
        let out = r.rewrite(input);
        // Walk and count: at least one In with cwe_id, one NotIn with cwe_id.
        let mut found_in = false;
        let mut found_not_in = false;
        fn visit(e: &FilterExpr, fi: &mut bool, fni: &mut bool) {
            match e {
                FilterExpr::In { field, .. } if field == "cwe_id" => *fi = true,
                FilterExpr::NotIn { field, .. } if field == "cwe_id" => *fni = true,
                FilterExpr::And(cs) | FilterExpr::Or(cs) => cs.iter().for_each(|c| visit(c, fi, fni)),
                FilterExpr::Not(inner) => visit(inner, fi, fni),
                _ => {}
            }
        }
        visit(&out, &mut found_in, &mut found_not_in);
        assert!(found_in, "expected an In on cwe_id after rewrite");
        assert!(found_not_in, "expected a NotIn on cwe_id after rewrite");
    }

    #[test]
    fn unknown_cwe_preserves_value_as_singleton() {
        let tx = tiny_taxonomy();
        let r = CweRewriter::new(&tx, "cwe_id");
        let input = FilterExpr::Eq {
            field: "cwe_id".into(),
            value: TypedValue::Numeric(9999.0),
        };
        let out = r.rewrite(input);
        match out {
            FilterExpr::In { field, values } => {
                assert_eq!(field, "cwe_id");
                let ids = numeric_values(&values);
                assert_eq!(ids, vec![9999]);
            }
            other => panic!("expected In singleton, got {other:?}"),
        }
    }
}
```

- [ ] **Step 4: Run to verify tests fail**

Run: `cargo test -p fastrag --lib filter::cwe_rewrite`
Expected: initial compile should succeed (implementation is in Step 3). If any tests fail, fix them. If all 7 pass in one shot, that's fine — the tests and implementation are co-located here because every branch of the rewriter has concrete expected output derived from a tiny hand-specified taxonomy.

- [ ] **Step 5: Run the full fastrag test suite to check no regressions**

Run: `cargo test -p fastrag --lib`
Expected: all tests PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/fastrag/Cargo.toml crates/fastrag/src/filter/
git commit -m "feat(filter): CweRewriter expands CWE predicates via taxonomy

AST walker. Eq/In on the CWE field become In-with-descendants; Neq/NotIn
become NotIn-with-descendants; other fields and range ops pass through.
Seven unit tests exercise every branch.

Refs: #47"
```

---

## Task 8: Apply `CweRewriter` in the query path

**Files:**
- Modify: `crates/fastrag/src/corpus/mod.rs` (`query_corpus_with_filter` around line 914)

- [ ] **Step 1: Read the existing query fn**

Review `crates/fastrag/src/corpus/mod.rs` lines 877–1008 to re-confirm the filter is consumed at the branch where `filter.is_none()` short-circuits. The rewriter must run before the filter reaches `crate::filter::matches`.

- [ ] **Step 2: Add a new pub fn signature and keep the existing one backward-compatible**

Edit `query_corpus_with_filter` to accept an optional `cwe_expand` flag. Rather than changing every caller, add a sibling `query_corpus_with_filter_opts` that carries an opts struct, and make the old function delegate with expansion off.

At the top of `corpus/mod.rs`, introduce:

```rust
/// Options for the filter-aware query path.
#[derive(Debug, Clone, Default)]
pub struct QueryOpts {
    /// When `true` and the corpus manifest has a `cwe_field`, expand CWE
    /// predicates via the embedded taxonomy before filter evaluation.
    pub cwe_expand: bool,
}
```

Then refactor `query_corpus_with_filter` to delegate:

```rust
pub fn query_corpus_with_filter(
    corpus_dir: &Path,
    query: &str,
    top_k: usize,
    embedder: &dyn DynEmbedderTrait,
    filter: Option<&crate::filter::FilterExpr>,
    breakdown: &mut LatencyBreakdown,
    snippet_len: usize,
) -> Result<Vec<SearchHitDto>, CorpusError> {
    query_corpus_with_filter_opts(
        corpus_dir, query, top_k, embedder, filter,
        &QueryOpts::default(), breakdown, snippet_len,
    )
}

pub fn query_corpus_with_filter_opts(
    corpus_dir: &Path,
    query: &str,
    top_k: usize,
    embedder: &dyn DynEmbedderTrait,
    filter: Option<&crate::filter::FilterExpr>,
    opts: &QueryOpts,
    breakdown: &mut LatencyBreakdown,
    snippet_len: usize,
) -> Result<Vec<SearchHitDto>, CorpusError> {
    // ... existing body ...
}
```

- [ ] **Step 3: Rewrite the filter before use**

Inside `query_corpus_with_filter_opts`, after loading the store and *before* the `if filter.is_none()` branch:

```rust
// Load manifest to inspect cwe_field.
let manifest = store.manifest().clone();

// Maybe rewrite the filter for CWE expansion.
let effective_filter: Option<crate::filter::FilterExpr> = match (filter, opts.cwe_expand, manifest.cwe_field.as_deref()) {
    (Some(f), true, Some(cwe_field)) => {
        let tx = fastrag_cwe::data::embedded();
        let rewriter = crate::filter::CweRewriter::new(tx, cwe_field);
        Some(rewriter.rewrite(f.clone()))
    }
    (Some(f), true, None) => {
        // Flag set but no CWE field configured. Warn once.
        tracing::warn!(target: "fastrag::cwe", "--cwe-expand set but corpus has no cwe_field; ignoring");
        Some(f.clone())
    }
    (Some(f), false, _) => Some(f.clone()),
    (None, true, Some(cwe_field)) => {
        // Free-text trigger: extract CWE ids and synthesize a filter.
        synthesize_cwe_filter_from_query(query, cwe_field)
    }
    (None, _, _) => None,
};

let filter = effective_filter.as_ref();
```

Then change every downstream reference from `filter.unwrap()` to use this `filter` binding. The existing overfetch loop uses `let filter_expr = filter.unwrap();` — that still works.

- [ ] **Step 4: Implement `synthesize_cwe_filter_from_query`**

At the bottom of `corpus/mod.rs`:

```rust
/// Extract CWE ids from the free-text query string and build an In filter
/// on `cwe_field`. Returns None when no CWE is present.
fn synthesize_cwe_filter_from_query(
    query: &str,
    cwe_field: &str,
) -> Option<crate::filter::FilterExpr> {
    use fastrag_index::identifiers::{SecurityId, extract_security_identifiers};
    use fastrag_store::schema::TypedValue;

    let ids: Vec<u32> = extract_security_identifiers(query)
        .into_iter()
        .filter_map(|id| match id {
            SecurityId::Cwe(s) => s.strip_prefix("CWE-").and_then(|n| n.parse::<u32>().ok()),
            _ => None,
        })
        .collect();
    if ids.is_empty() {
        return None;
    }

    // Expand through the taxonomy so "CWE-89 in login" matches docs tagged CWE-564.
    let tx = fastrag_cwe::data::embedded();
    let mut merged: std::collections::BTreeSet<u32> = std::collections::BTreeSet::new();
    for id in ids {
        for d in tx.expand(id) {
            merged.insert(d);
        }
    }
    let values: Vec<TypedValue> = merged.into_iter().map(|n| TypedValue::Numeric(n as f64)).collect();
    Some(crate::filter::FilterExpr::In {
        field: cwe_field.to_string(),
        values,
    })
}
```

- [ ] **Step 5: Handle user filter + free-text trigger combined**

Update the match above so that when a user filter is present AND the query has CWEs AND `cwe_expand` is on, the two filters are AND-combined:

Replace the `(Some(f), true, Some(cwe_field))` arm with:

```rust
(Some(f), true, Some(cwe_field)) => {
    let tx = fastrag_cwe::data::embedded();
    let rewriter = crate::filter::CweRewriter::new(tx, cwe_field);
    let rewritten = rewriter.rewrite(f.clone());
    // If free-text also mentions CWEs, AND the synthetic filter on top.
    match synthesize_cwe_filter_from_query(query, cwe_field) {
        Some(extra) => Some(crate::filter::FilterExpr::And(vec![rewritten, extra])),
        None => Some(rewritten),
    }
}
```

- [ ] **Step 6: Build and check**

Run: `cargo build -p fastrag --features retrieval`
Expected: PASS.

Run: `cargo test -p fastrag --lib --features retrieval`
Expected: existing tests PASS (we haven't added integration tests yet — those come in Task 12).

- [ ] **Step 7: Commit**

```bash
git add crates/fastrag/src/corpus/mod.rs
git commit -m "feat(corpus): apply CweRewriter + free-text trigger in query path

query_corpus_with_filter_opts takes a QueryOpts struct. When
cwe_expand is on and the manifest records a cwe_field, user filters
are rewritten via the taxonomy and CWE ids in the free-text query
are AND-combined as a synthetic In filter.

Refs: #47"
```

---

## Task 9: CLI — `--cwe-expand` on `Query` and `ServeHttp`

**Files:**
- Modify: `fastrag-cli/src/args.rs` (`Query` + `ServeHttp` variants)
- Modify: `fastrag-cli/src/main.rs` (read flag, build `QueryOpts`)
- Modify: `fastrag-cli/src/http.rs` (parse `cwe_expand` query param)

- [ ] **Step 1: Add flag to `Query`**

In `fastrag-cli/src/args.rs`, inside the `Query` variant (around line 286):

```rust
        /// Enable query-time CWE hierarchy expansion. Requires the corpus
        /// to have been ingested with --cwe-field. Overrides the default,
        /// which is on for CWE-aware corpora.
        #[arg(long, overrides_with = "no_cwe_expand")]
        cwe_expand: bool,

        /// Disable query-time CWE hierarchy expansion.
        #[arg(long = "no-cwe-expand", overrides_with = "cwe_expand")]
        no_cwe_expand: bool,
```

- [ ] **Step 2: Add flag to `ServeHttp`**

In `fastrag-cli/src/args.rs`, in the `ServeHttp` variant:

```rust
        /// Default for CWE hierarchy expansion. Per-request override via
        /// `cwe_expand` query parameter.
        #[arg(long)]
        cwe_expand: bool,
```

- [ ] **Step 3: Resolve effective flag in `main.rs`**

In `fastrag-cli/src/main.rs`, find the `Query` match arm. Determine effective expand:

```rust
// Resolve effective cwe_expand:
//   - explicit --cwe-expand: on
//   - explicit --no-cwe-expand: off
//   - neither: default-on when manifest has cwe_field
let manifest_cwe_field = {
    let mbytes = std::fs::read(corpus.join("manifest.json")).ok();
    mbytes
        .and_then(|b| serde_json::from_slice::<fastrag_index::CorpusManifest>(&b).ok())
        .and_then(|m| m.cwe_field)
};
let effective_expand = if no_cwe_expand {
    false
} else if cwe_expand {
    true
} else {
    manifest_cwe_field.is_some()
};

let opts = fastrag::corpus::QueryOpts { cwe_expand: effective_expand };
```

Then replace the `query_corpus_with_filter` call with `query_corpus_with_filter_opts` passing `&opts`.

If there's a public re-export — check with `rg -n "pub use .*query_corpus" crates/fastrag/src/lib.rs` — add `QueryOpts` and `query_corpus_with_filter_opts` to the re-exports if needed.

- [ ] **Step 4: Thread through HTTP server state**

In `fastrag-cli/src/http.rs`, add `cwe_expand: bool` to whatever server-state struct carries the defaults. On request, read `cwe_expand` query param and override. The existing `/query` handler likely parses `top_k`, `snippet_len` similarly — follow that pattern.

Pseudo-sketch (adapt names to actual code):

```rust
#[derive(Deserialize)]
struct QueryParams {
    q: String,
    top_k: Option<usize>,
    filter: Option<String>,
    snippet_len: Option<usize>,
    cwe_expand: Option<bool>,  // NEW
}

// ... in handler:
let effective_expand = params.cwe_expand.unwrap_or(state.cwe_expand_default);
let opts = fastrag::corpus::QueryOpts { cwe_expand: effective_expand };
// ... use opts ...
```

- [ ] **Step 5: Build**

Run: `cargo build -p fastrag-cli --features "retrieval,store"`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add fastrag-cli/src/
git commit -m "feat(cli): --cwe-expand / --no-cwe-expand on query and serve-http

Defaults on when manifest has cwe_field. HTTP accepts per-request
override via cwe_expand query param.

Refs: #47"
```

---

## Task 10: End-to-end integration test (Rust)

**Files:**
- Create: `crates/fastrag/tests/cwe_expansion.rs`

- [ ] **Step 1: Write the failing test**

Create `crates/fastrag/tests/cwe_expansion.rs`:

```rust
//! End-to-end: ingest a tiny JSONL corpus that tags documents with parent and
//! child CWEs, query for the parent CWE with cwe_expand on, and assert the
//! child-tagged doc appears in the results.

use std::fs;
use std::path::PathBuf;

use fastrag::corpus::{QueryOpts, query_corpus_with_filter_opts};
use fastrag::filter::FilterExpr;
use fastrag::LatencyBreakdown;
use fastrag_embed::testing::MockEmbedder;
use fastrag_store::schema::TypedValue;

// Write a JSONL file with three finding records:
//   - id "A", cwe_id 89  (parent: SQL Injection)
//   - id "B", cwe_id 564 (child of 89: Hibernate Injection)
//   - id "C", cwe_id 79  (unrelated: XSS)
fn write_fixture(path: &std::path::Path) {
    let lines = [
        r#"{"id":"A","title":"sqli in login","description":"SQL injection","cwe_id":89}"#,
        r#"{"id":"B","title":"hibernate injection","description":"hql sqli","cwe_id":564}"#,
        r#"{"id":"C","title":"xss","description":"stored xss","cwe_id":79}"#,
    ];
    fs::write(path, lines.join("\n")).unwrap();
}

#[test]
fn cwe_expansion_returns_child_tagged_docs() {
    let tmp = tempfile::tempdir().unwrap();
    let corpus = tmp.path().join("corpus");
    let jsonl = tmp.path().join("findings.jsonl");
    write_fixture(&jsonl);

    let embedder = MockEmbedder::new(16);

    // Ingest with cwe_field=cwe_id.
    let cfg = fastrag::ingest::jsonl::JsonlIngestConfig {
        text_fields: vec!["title".into(), "description".into()],
        id_field: "id".into(),
        metadata_fields: vec!["cwe_id".into()],
        metadata_types: std::collections::BTreeMap::from([
            ("cwe_id".into(), fastrag_store::schema::TypedKind::Numeric),
        ]),
        array_fields: vec![],
        cwe_field: Some("cwe_id".into()),
    };
    fastrag::corpus::index_jsonl(&jsonl, &corpus, &embedder, &cfg).unwrap();

    // Sanity: manifest records cwe_field.
    let mbytes = fs::read(corpus.join("manifest.json")).unwrap();
    let manifest: fastrag_index::CorpusManifest = serde_json::from_slice(&mbytes).unwrap();
    assert_eq!(manifest.cwe_field.as_deref(), Some("cwe_id"));
    assert!(manifest.cwe_taxonomy_version.is_some());

    // Query with filter cwe_id = 89, expansion ON.
    let filter = FilterExpr::Eq {
        field: "cwe_id".into(),
        value: TypedValue::Numeric(89.0),
    };
    let opts = QueryOpts { cwe_expand: true };
    let mut b = LatencyBreakdown::default();
    let hits_expanded = query_corpus_with_filter_opts(
        &corpus, "query", 10, &embedder, Some(&filter), &opts, &mut b, 0,
    ).unwrap();
    let ids_expanded: std::collections::HashSet<String> =
        hits_expanded.iter().map(|h| h.external_id.clone()).collect();
    assert!(ids_expanded.contains("A"), "parent doc A missing: {ids_expanded:?}");
    assert!(ids_expanded.contains("B"), "child-CWE doc B missing: {ids_expanded:?}");
    assert!(!ids_expanded.contains("C"), "unrelated doc C should not match: {ids_expanded:?}");

    // Query with expansion OFF: only A.
    let opts_off = QueryOpts { cwe_expand: false };
    let mut b = LatencyBreakdown::default();
    let hits_plain = query_corpus_with_filter_opts(
        &corpus, "query", 10, &embedder, Some(&filter), &opts_off, &mut b, 0,
    ).unwrap();
    let ids_plain: std::collections::HashSet<String> =
        hits_plain.iter().map(|h| h.external_id.clone()).collect();
    assert!(ids_plain.contains("A"));
    assert!(!ids_plain.contains("B"), "child-CWE doc must NOT match without expansion");
}

#[test]
fn free_text_trigger_synthesizes_filter() {
    let tmp = tempfile::tempdir().unwrap();
    let corpus = tmp.path().join("corpus");
    let jsonl = tmp.path().join("findings.jsonl");
    write_fixture(&jsonl);
    let embedder = MockEmbedder::new(16);

    let cfg = fastrag::ingest::jsonl::JsonlIngestConfig {
        text_fields: vec!["title".into(), "description".into()],
        id_field: "id".into(),
        metadata_fields: vec!["cwe_id".into()],
        metadata_types: std::collections::BTreeMap::from([
            ("cwe_id".into(), fastrag_store::schema::TypedKind::Numeric),
        ]),
        array_fields: vec![],
        cwe_field: Some("cwe_id".into()),
    };
    fastrag::corpus::index_jsonl(&jsonl, &corpus, &embedder, &cfg).unwrap();

    // Free-text query mentioning CWE-89 with NO explicit filter. Expansion on.
    let opts = QueryOpts { cwe_expand: true };
    let mut b = LatencyBreakdown::default();
    let hits = query_corpus_with_filter_opts(
        &corpus, "vulnerability CWE-89 in login form", 10,
        &embedder, None, &opts, &mut b, 0,
    ).unwrap();
    let ids: std::collections::HashSet<String> =
        hits.iter().map(|h| h.external_id.clone()).collect();
    assert!(ids.contains("A"));
    assert!(ids.contains("B"));
    assert!(!ids.contains("C"), "unrelated XSS should not match when query mentions CWE-89");
}
```

If `tempfile`, `MockEmbedder`, or `index_jsonl` names differ, adjust. Use `rg -n "pub fn index_jsonl\|MockEmbedder\|pub fn new" crates/` to locate the right names. The test should be placed where existing integration tests live; check `ls crates/fastrag/tests/` first.

- [ ] **Step 2: Add dev-deps if missing**

If `tempfile` isn't already a dev-dep on `crates/fastrag/Cargo.toml`, add:

```toml
[dev-dependencies]
tempfile = "3"
serde_json = { workspace = true }
```

- [ ] **Step 3: Run to verify**

Run: `cargo test -p fastrag --test cwe_expansion --features retrieval`
Expected: PASS (both tests).

If either test fails, read the output, fix the bug in Task 7/8/6 (whichever is at fault), then re-run.

- [ ] **Step 4: Commit**

```bash
git add crates/fastrag/tests/cwe_expansion.rs crates/fastrag/Cargo.toml
git commit -m "test(cwe): end-to-end expansion and free-text trigger

Ingest a JSONL with parent CWE-89 and child CWE-564 docs; verify
that cwe_expand=true returns both when filtering on 89, and off
returns only the parent.

Refs: #47"
```

---

## Task 11: CLI e2e test

**Files:**
- Create: `fastrag-cli/tests/cwe_expand_e2e.rs`

- [ ] **Step 1: Write the test**

Create `fastrag-cli/tests/cwe_expand_e2e.rs`:

```rust
//! End-to-end CLI test: `fastrag index --cwe-field cwe_id` writes the
//! manifest, `fastrag query --cwe-expand` returns expanded hits.

use std::fs;
use std::process::Command;

fn bin() -> String {
    env!("CARGO_BIN_EXE_fastrag").to_string()
}

#[test]
fn cli_index_sets_cwe_field_and_query_expands() {
    let tmp = tempfile::tempdir().unwrap();
    let corpus = tmp.path().join("corpus");
    let jsonl = tmp.path().join("f.jsonl");
    fs::write(
        &jsonl,
        r#"{"id":"A","title":"sqli","cwe_id":89}
{"id":"B","title":"hibernate","cwe_id":564}
{"id":"C","title":"xss","cwe_id":79}
"#,
    ).unwrap();

    let status = Command::new(bin())
        .args([
            "index",
            jsonl.to_str().unwrap(),
            "--corpus", corpus.to_str().unwrap(),
            "--format", "jsonl",
            "--text-fields", "title",
            "--id-field", "id",
            "--metadata-fields", "cwe_id",
            "--metadata-types", "cwe_id=numeric",
            "--cwe-field", "cwe_id",
        ])
        .status()
        .unwrap();
    assert!(status.success(), "index failed");

    // Manifest sanity.
    let mbytes = fs::read(corpus.join("manifest.json")).unwrap();
    let mtext = String::from_utf8_lossy(&mbytes);
    assert!(mtext.contains("\"cwe_field\":\"cwe_id\""), "manifest missing cwe_field: {mtext}");
    assert!(mtext.contains("cwe_taxonomy_version"), "manifest missing cwe_taxonomy_version");

    // Query with --cwe-expand and --filter cwe_id=89.
    let out = Command::new(bin())
        .args([
            "query", "sqli",
            "--corpus", corpus.to_str().unwrap(),
            "--top-k", "10",
            "--filter", "cwe_id=89",
            "--cwe-expand",
            "--output", "json",
        ])
        .output()
        .unwrap();
    assert!(out.status.success(), "query failed: {}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("\"A\""), "result missing A: {stdout}");
    assert!(stdout.contains("\"B\""), "result missing B (child CWE): {stdout}");
    assert!(!stdout.contains("\"C\""), "result should not contain C: {stdout}");
}
```

- [ ] **Step 2: Run**

Run: `cargo test -p fastrag-cli --test cwe_expand_e2e --features "retrieval,store"`
Expected: PASS.

If the CLI rejects `--filter cwe_id=89` syntax, check the filter parser — string syntax may require quoting or different separator. Adjust to whatever `crates/fastrag/src/filter/parser.rs` actually accepts.

- [ ] **Step 3: Commit**

```bash
git add fastrag-cli/tests/cwe_expand_e2e.rs
git commit -m "test(cli): end-to-end cwe-expand via index + query commands

Refs: #47"
```

---

## Task 12: HTTP e2e test

**Files:**
- Create: `fastrag-cli/tests/cwe_expand_http_e2e.rs`

- [ ] **Step 1: Write the test**

Model after an existing HTTP integration test. Find one with `rg -l "serve_http\|serve-http\|bind" fastrag-cli/tests/`. Adapt its scaffolding.

The test should:
1. Ingest the same 3-doc fixture.
2. Spawn `serve-http` in the background bound to a random port.
3. POST/GET `/query` with `cwe_expand=true` and filter `cwe_id=89`.
4. Assert `A` and `B` are present, `C` is not.
5. Kill the server.

Keep the test gated behind the same features the existing HTTP tests use.

- [ ] **Step 2: Run**

Run: `cargo test -p fastrag-cli --test cwe_expand_http_e2e --features "retrieval,store"`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add fastrag-cli/tests/cwe_expand_http_e2e.rs
git commit -m "test(cli): HTTP e2e for cwe_expand query parameter

Refs: #47"
```

---

## Task 13: Eval gold-set entries for CWE hierarchy

**Files:**
- Modify: `crates/fastrag-eval/tests/fixtures/security_gold.json` (or wherever gold sets live — `rg -l "gold" crates/fastrag-eval/` to locate)

- [ ] **Step 1: Locate existing gold set**

Run: `ls crates/fastrag-eval/tests/fixtures/ 2>/dev/null; rg -l "qrels\|gold" crates/fastrag-eval/`

Pick the security-oriented gold set.

- [ ] **Step 2: Add entries**

Add 3–5 entries where:
- Query text references a parent CWE (e.g., "SQL injection CWE-89")
- Expected relevant doc ids are tagged with a child CWE (e.g., CWE-564)

The structure of the gold set is specific to the repo — follow the existing JSON schema. If unclear, consult `crates/fastrag-eval/src/datasets/common.rs` and the matrix test.

- [ ] **Step 3: Verify the eval harness still loads**

Run: `cargo test -p fastrag-eval --test gold_set_loader`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add crates/fastrag-eval/
git commit -m "test(eval): gold-set entries exercising CWE hierarchy expansion

Queries reference parent CWEs with relevant docs tagged with child
CWEs. Used to validate --cwe-expand improves recall without
regressing precision.

Refs: #47"
```

---

## Task 14: Final lint and workspace verification

- [ ] **Step 1: Format**

Run: `cargo fmt --all`
Expected: no changes, or commit if there are.

- [ ] **Step 2: Clippy on the feature combinations used by the feature**

Run: `cargo clippy --workspace --all-targets --features retrieval,rerank,hybrid,contextual,eval,nvd,hygiene -- -D warnings`
Expected: PASS.

- [ ] **Step 3: Full test suite**

Run: `cargo test --workspace --features retrieval`
Expected: PASS.

- [ ] **Step 4: Feature-specific test**

Run: `cargo test -p fastrag-cwe`
Run: `cargo test -p fastrag-cwe --features compile-tool`
Run: `cargo test -p fastrag --test cwe_expansion --features retrieval`
Run: `cargo test -p fastrag-cli --test cwe_expand_e2e --features "retrieval,store"`
Expected: all PASS.

- [ ] **Step 5: Update README and docs**

Per user memory: "Always update README/docs when adding or changing features." Add a section to `README.md` under the existing retrieval/security content:

```markdown
### CWE Hierarchy Expansion

When a corpus is ingested with `--cwe-field <name>`, the field is recorded
in the manifest and query-time CWE descendant expansion is enabled by
default. A query for CWE-89 (SQL Injection) also retrieves documents
tagged with child CWEs like CWE-564 (Hibernate Injection).

Override per-query:

    fastrag query "sqli patterns" --corpus ./corpus --cwe-expand
    fastrag query "sqli patterns" --corpus ./corpus --no-cwe-expand

Via HTTP, pass `cwe_expand=true|false` as a query parameter on `/query`.

The taxonomy is MITRE CWE-1000 (Research View), embedded in the binary
at build time. Regenerate with:

    cargo run -p fastrag-cwe --features compile-tool --bin compile-taxonomy -- \
        --in path/to/cwec_v4.XX.xml \
        --out crates/fastrag-cwe/data/cwe-tree-v4.XX.json
```

Also update `CLAUDE.md` test-command section to include the new commands (see Task 14 Step 4 test invocations).

- [ ] **Step 6: Final commit (combined docs + closing commit)**

```bash
git add README.md CLAUDE.md
git commit -m "docs: CWE hierarchy expansion usage and taxonomy regen

Closes #47"
```

- [ ] **Step 7: Push to remote, run ci-watcher**

```bash
git push
```

Then invoke the ci-watcher skill (see `CLAUDE.md` Skills section) as a background Haiku Agent to monitor CI.

---

## Summary of tasks

1. Scaffold `fastrag-cwe` crate
2. `Taxonomy` struct with closure lookup (unit tests)
3. Compile-taxonomy binary tool (XML → JSON, fixture test)
4. Generate and commit production taxonomy JSON + `embedded()` loader
5. Extend `CorpusManifest` with `cwe_field` + `cwe_taxonomy_version`
6. `--cwe-field` ingest flag and preset plumbing
7. `CweRewriter` filter AST walker (unit tests covering every branch)
8. Apply `CweRewriter` + free-text trigger in `query_corpus_with_filter_opts`
9. CLI `--cwe-expand` / `--no-cwe-expand` + HTTP `cwe_expand` param
10. Rust integration test (ingest → query → expanded vs. plain)
11. CLI e2e test
12. HTTP e2e test
13. Eval gold-set entries
14. Lint, full test suite, docs, commit, push, ci-watcher
