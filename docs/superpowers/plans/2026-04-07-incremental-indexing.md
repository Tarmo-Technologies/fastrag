# Incremental Indexing Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `fastrag index` skip unchanged files, re-embed only edited/new files, auto-prune deleted files, and support multiple input roots in one corpus.

**Architecture:** Add a file tracking table (`rel_path`, `size`, `mtime_ns`, `blake3` hash, `chunk_ids`) and a `roots` list to `CorpusManifest` (schema v2). On re-index, diff the walked tree against the manifest, re-embed only changed/new files, drop chunks for changed/deleted files. Because `HnswIndex::add` already rebuilds the graph on every call, we mutate `entries` in place (remove then add) — no tombstoning required.

**Tech Stack:** Rust, `blake3`, `serde`, existing `instant-distance` HNSW, `fs2` for lock file, `walkdir` (already used via `collect_files`).

---

## File structure

- **Modify** `crates/fastrag-index/Cargo.toml` — add `blake3`.
- **Modify** `crates/fastrag-index/src/manifest.rs` — add schema v2 fields; migration fn.
- **Modify** `crates/fastrag-index/src/hnsw.rs` — `remove_by_chunk_ids(&[u64])`; allow loading v1 manifest via `load_or_migrate`.
- **Modify** `crates/fastrag/src/corpus.rs` — rewrite `index_path_with_metadata` to use incremental plan; add stats fields.
- **Create** `crates/fastrag/src/corpus/incremental.rs` — `IndexPlan`, `plan_index`, classifier.
- **Modify** `crates/fastrag/Cargo.toml` — add `fs2` (lock), `blake3`.
- **Create** `crates/fastrag/tests/incremental.rs` — e2e incremental tests.
- **Modify** `fastrag-cli/src/main.rs` — print new stats fields.
- **Modify** `README.md` — document incremental behavior and multi-root.

---

## Task 1: Add `blake3` dependency and file hasher

**Files:**
- Modify: `crates/fastrag-index/Cargo.toml`
- Modify: `crates/fastrag-index/src/lib.rs`
- Create: `crates/fastrag-index/src/hash.rs`

- [ ] **Step 1: Add blake3 dep**

In `crates/fastrag-index/Cargo.toml` under `[dependencies]`:

```toml
blake3 = "1"
```

- [ ] **Step 2: Write failing test**

Create `crates/fastrag-index/src/hash.rs`:

```rust
use std::io;
use std::path::Path;

/// Hex-encoded blake3 digest of a file's contents, prefixed with `blake3:`.
pub fn hash_file(path: &Path) -> io::Result<String> {
    let bytes = std::fs::read(path)?;
    Ok(format!("blake3:{}", blake3::hash(&bytes).to_hex()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn same_bytes_same_hash() {
        let mut f1 = tempfile::NamedTempFile::new().unwrap();
        let mut f2 = tempfile::NamedTempFile::new().unwrap();
        f1.write_all(b"hello world").unwrap();
        f2.write_all(b"hello world").unwrap();
        assert_eq!(hash_file(f1.path()).unwrap(), hash_file(f2.path()).unwrap());
    }

    #[test]
    fn different_bytes_different_hash() {
        let mut f1 = tempfile::NamedTempFile::new().unwrap();
        let mut f2 = tempfile::NamedTempFile::new().unwrap();
        f1.write_all(b"alpha").unwrap();
        f2.write_all(b"beta").unwrap();
        assert_ne!(hash_file(f1.path()).unwrap(), hash_file(f2.path()).unwrap());
    }

    #[test]
    fn prefixed_with_scheme() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(b"x").unwrap();
        assert!(hash_file(f.path()).unwrap().starts_with("blake3:"));
    }
}
```

In `crates/fastrag-index/src/lib.rs` add `pub mod hash;`.

- [ ] **Step 3: Run tests**

```
cargo test -p fastrag-index hash::
```

Expected: 3 passing.

- [ ] **Step 4: Commit**

```
git add -A && git commit -m "feat(index): add blake3 file hasher for incremental indexing"
```

---

## Task 2: Extend `CorpusManifest` with schema v2 fields

**Files:**
- Modify: `crates/fastrag-index/src/manifest.rs`

- [ ] **Step 1: Write failing tests**

Append to `crates/fastrag-index/src/manifest.rs`:

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RootEntry {
    pub id: u32,
    pub path: std::path::PathBuf,
    pub last_indexed_unix_seconds: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileEntry {
    pub root_id: u32,
    pub rel_path: std::path::PathBuf,
    pub size: u64,
    pub mtime_ns: i128,
    pub content_hash: Option<String>,
    pub chunk_ids: Vec<u64>,
}

#[cfg(test)]
mod v2_tests {
    use super::*;

    #[test]
    fn v2_roundtrip() {
        let m = CorpusManifest {
            version: 2,
            embedding_model_id: "mock".into(),
            dim: 3,
            created_at_unix_seconds: 1,
            chunk_count: 0,
            chunking_strategy: ManifestChunkingStrategy::Basic {
                max_characters: 100,
                overlap: 0,
            },
            roots: vec![RootEntry {
                id: 0,
                path: "/tmp/docs".into(),
                last_indexed_unix_seconds: 42,
            }],
            files: vec![FileEntry {
                root_id: 0,
                rel_path: "a.txt".into(),
                size: 10,
                mtime_ns: 1_700_000_000_000_000_000,
                content_hash: Some("blake3:abc".into()),
                chunk_ids: vec![1, 2],
            }],
        };
        let s = serde_json::to_string(&m).unwrap();
        let back: CorpusManifest = serde_json::from_str(&s).unwrap();
        assert_eq!(back, m);
    }

    #[test]
    fn v1_manifest_loads_with_empty_roots_files() {
        // v1 manifests on disk have no roots/files fields; serde defaults must fill them.
        let v1 = r#"{
            "version": 1,
            "embedding_model_id": "mock",
            "dim": 3,
            "created_at_unix_seconds": 1,
            "chunk_count": 0,
            "chunking_strategy": {"kind":"basic","max_characters":100,"overlap":0}
        }"#;
        let m: CorpusManifest = serde_json::from_str(v1).unwrap();
        assert_eq!(m.version, 1);
        assert!(m.roots.is_empty());
        assert!(m.files.is_empty());
    }
}
```

Modify `CorpusManifest` struct — remove `#[serde(deny_unknown_fields)]` and add:

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CorpusManifest {
    pub version: u32,
    pub embedding_model_id: String,
    pub dim: usize,
    pub created_at_unix_seconds: u64,
    pub chunk_count: usize,
    pub chunking_strategy: ManifestChunkingStrategy,
    #[serde(default)]
    pub roots: Vec<RootEntry>,
    #[serde(default)]
    pub files: Vec<FileEntry>,
}
```

Update `CorpusManifest::new` to initialize `roots: Vec::new(), files: Vec::new()`. Leave `version: 1` in `new` — the incremental layer will bump to 2 when it first writes roots/files.

- [ ] **Step 2: Run tests**

```
cargo test -p fastrag-index manifest::
```

Expected: pre-existing tests plus `v2_roundtrip` and `v1_manifest_loads_with_empty_roots_files` passing.

- [ ] **Step 3: Commit**

```
git add -A && git commit -m "feat(index): extend manifest with roots/files for schema v2"
```

---

## Task 3: `HnswIndex::remove_by_chunk_ids`

**Files:**
- Modify: `crates/fastrag-index/src/hnsw.rs`

- [ ] **Step 1: Write failing test**

Add to `hnsw.rs` tests module:

```rust
#[test]
fn remove_by_chunk_ids_drops_matching_entries_and_rebuilds() {
    let mut index = HnswIndex::new(3, manifest());
    index
        .add(vec![
            entry(1, vec![1.0, 0.0, 0.0], "a"),
            entry(2, vec![0.0, 1.0, 0.0], "b"),
            entry(3, vec![0.0, 0.0, 1.0], "c"),
        ])
        .unwrap();

    index.remove_by_chunk_ids(&[2]);
    assert_eq!(index.len(), 2);
    let ids: Vec<u64> = index.entries().iter().map(|e| e.id).collect();
    assert_eq!(ids, vec![1, 3]);

    // Graph rebuilt: querying for the removed vector must NOT return id=2.
    let hits = index.query(&[0.0, 1.0, 0.0], 3).unwrap();
    assert!(hits.iter().all(|h| h.entry.id != 2));
}
```

- [ ] **Step 2: Implement**

In `impl HnswIndex` (not the trait impl) add:

```rust
/// Remove all entries whose `id` is in `ids` and rebuild the graph.
/// O(n) in total entries; cheap since add() already rebuilds per call.
pub fn remove_by_chunk_ids(&mut self, ids: &[u64]) {
    if ids.is_empty() {
        return;
    }
    let set: std::collections::HashSet<u64> = ids.iter().copied().collect();
    self.entries.retain(|e| !set.contains(&e.id));
    self.manifest.chunk_count = self.entries.len();
    self.rebuild_graph();
}
```

- [ ] **Step 3: Run tests**

```
cargo test -p fastrag-index hnsw::tests::remove_by_chunk_ids
```

Expected: pass.

- [ ] **Step 4: Commit**

```
git add -A && git commit -m "feat(index): add HnswIndex::remove_by_chunk_ids"
```

---

## Task 4: `IndexPlan` classifier

**Files:**
- Create: `crates/fastrag/src/corpus/incremental.rs`
- Modify: `crates/fastrag/src/corpus.rs` (add `pub mod incremental;`)
- Modify: `crates/fastrag/Cargo.toml` (add `blake3` dep so we can call the hasher — or re-export from fastrag-index; we'll use fastrag-index hash fn)

- [ ] **Step 1: Convert `corpus.rs` to module directory**

The existing `crates/fastrag/src/corpus.rs` becomes `crates/fastrag/src/corpus/mod.rs`. Use `git mv`:

```
mkdir -p crates/fastrag/src/corpus
git mv crates/fastrag/src/corpus.rs crates/fastrag/src/corpus/mod.rs
```

Run `cargo check -p fastrag` to confirm nothing broke. Commit:

```
git commit -m "refactor(corpus): promote corpus.rs to corpus/ module"
```

- [ ] **Step 2: Write failing test for classifier**

Create `crates/fastrag/src/corpus/incremental.rs`:

```rust
use fastrag_index::manifest::{CorpusManifest, FileEntry, RootEntry};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq)]
pub struct WalkedFile {
    pub rel_path: PathBuf,
    pub abs_path: PathBuf,
    pub size: u64,
    pub mtime_ns: i128,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct IndexPlan {
    pub root_id: u32,
    pub unchanged: Vec<WalkedFile>,
    pub changed: Vec<WalkedFile>,
    pub new: Vec<WalkedFile>,
    pub deleted: Vec<FileEntry>,
    /// Files whose stat changed but hash matched — need mtime/size updated in manifest.
    pub touched: Vec<(FileEntry, WalkedFile)>,
}

/// Classify walked files against the manifest.
///
/// Resolves (appending if needed) the root for `root_abs`, then classifies:
/// - unchanged: (size, mtime_ns) match
/// - touched: stat differs, hash matches (returned; caller updates manifest in place)
/// - changed: stat differs, hash differs
/// - new: not in manifest for this root
/// - deleted: in manifest for this root but not in walked set
pub fn plan_index(
    root_abs: &Path,
    walked: Vec<WalkedFile>,
    manifest: &mut CorpusManifest,
    hash_file: &dyn Fn(&Path) -> std::io::Result<String>,
) -> std::io::Result<IndexPlan> {
    let root_id = resolve_root(manifest, root_abs);
    let existing: std::collections::HashMap<PathBuf, FileEntry> = manifest
        .files
        .iter()
        .filter(|f| f.root_id == root_id)
        .map(|f| (f.rel_path.clone(), f.clone()))
        .collect();

    let mut plan = IndexPlan {
        root_id,
        ..Default::default()
    };
    let mut seen_rel: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();

    for wf in walked {
        seen_rel.insert(wf.rel_path.clone());
        match existing.get(&wf.rel_path) {
            None => plan.new.push(wf),
            Some(existing_entry) => {
                if existing_entry.size == wf.size && existing_entry.mtime_ns == wf.mtime_ns {
                    plan.unchanged.push(wf);
                } else {
                    let h = hash_file(&wf.abs_path)?;
                    if existing_entry.content_hash.as_deref() == Some(h.as_str()) {
                        plan.touched.push((existing_entry.clone(), wf));
                    } else {
                        plan.changed.push(wf);
                    }
                }
            }
        }
    }

    for f in &manifest.files {
        if f.root_id == root_id && !seen_rel.contains(&f.rel_path) {
            plan.deleted.push(f.clone());
        }
    }

    Ok(plan)
}

/// Find or append a root entry for the given absolute path.
pub fn resolve_root(manifest: &mut CorpusManifest, abs: &Path) -> u32 {
    if let Some(r) = manifest.roots.iter().find(|r| r.path == abs) {
        return r.id;
    }
    let id = manifest.roots.iter().map(|r| r.id).max().map_or(0, |m| m + 1);
    manifest.roots.push(RootEntry {
        id,
        path: abs.to_path_buf(),
        last_indexed_unix_seconds: 0,
    });
    id
}

#[cfg(test)]
mod tests {
    use super::*;
    use fastrag_index::manifest::ManifestChunkingStrategy;

    fn empty_manifest() -> CorpusManifest {
        CorpusManifest::new(
            "mock",
            3,
            0,
            ManifestChunkingStrategy::Basic {
                max_characters: 100,
                overlap: 0,
            },
        )
    }

    fn walked(rel: &str, size: u64, mtime: i128) -> WalkedFile {
        WalkedFile {
            rel_path: rel.into(),
            abs_path: format!("/root/{rel}").into(),
            size,
            mtime_ns: mtime,
        }
    }

    fn never_hash(_: &Path) -> std::io::Result<String> {
        panic!("hash_file should not have been called for unchanged files");
    }

    #[test]
    fn all_new_when_manifest_empty() {
        let mut m = empty_manifest();
        let plan = plan_index(
            Path::new("/root"),
            vec![walked("a.txt", 10, 1), walked("b.txt", 20, 2)],
            &mut m,
            &never_hash,
        )
        .unwrap();
        assert_eq!(plan.new.len(), 2);
        assert!(plan.unchanged.is_empty() && plan.changed.is_empty() && plan.deleted.is_empty());
        assert_eq!(m.roots.len(), 1);
        assert_eq!(m.roots[0].path, Path::new("/root"));
    }

    #[test]
    fn unchanged_skips_hash_call() {
        let mut m = empty_manifest();
        m.roots.push(RootEntry {
            id: 0,
            path: "/root".into(),
            last_indexed_unix_seconds: 0,
        });
        m.files.push(FileEntry {
            root_id: 0,
            rel_path: "a.txt".into(),
            size: 10,
            mtime_ns: 1,
            content_hash: Some("blake3:xxx".into()),
            chunk_ids: vec![1],
        });

        let plan = plan_index(
            Path::new("/root"),
            vec![walked("a.txt", 10, 1)],
            &mut m,
            &never_hash,
        )
        .unwrap();
        assert_eq!(plan.unchanged.len(), 1);
        assert!(plan.changed.is_empty() && plan.new.is_empty());
    }

    #[test]
    fn touch_with_same_content_goes_to_touched_not_changed() {
        let mut m = empty_manifest();
        m.roots.push(RootEntry {
            id: 0,
            path: "/root".into(),
            last_indexed_unix_seconds: 0,
        });
        m.files.push(FileEntry {
            root_id: 0,
            rel_path: "a.txt".into(),
            size: 10,
            mtime_ns: 1,
            content_hash: Some("blake3:abc".into()),
            chunk_ids: vec![1],
        });

        let plan = plan_index(
            Path::new("/root"),
            vec![walked("a.txt", 10, 999)], // mtime changed
            &mut m,
            &|_| Ok("blake3:abc".to_string()),
        )
        .unwrap();
        assert_eq!(plan.touched.len(), 1);
        assert!(plan.changed.is_empty());
    }

    #[test]
    fn edit_with_new_content_goes_to_changed() {
        let mut m = empty_manifest();
        m.roots.push(RootEntry {
            id: 0,
            path: "/root".into(),
            last_indexed_unix_seconds: 0,
        });
        m.files.push(FileEntry {
            root_id: 0,
            rel_path: "a.txt".into(),
            size: 10,
            mtime_ns: 1,
            content_hash: Some("blake3:old".into()),
            chunk_ids: vec![1],
        });

        let plan = plan_index(
            Path::new("/root"),
            vec![walked("a.txt", 11, 999)],
            &mut m,
            &|_| Ok("blake3:new".to_string()),
        )
        .unwrap();
        assert_eq!(plan.changed.len(), 1);
    }

    #[test]
    fn missing_file_goes_to_deleted() {
        let mut m = empty_manifest();
        m.roots.push(RootEntry {
            id: 0,
            path: "/root".into(),
            last_indexed_unix_seconds: 0,
        });
        m.files.push(FileEntry {
            root_id: 0,
            rel_path: "gone.txt".into(),
            size: 10,
            mtime_ns: 1,
            content_hash: Some("blake3:x".into()),
            chunk_ids: vec![7, 8],
        });

        let plan = plan_index(Path::new("/root"), vec![], &mut m, &never_hash).unwrap();
        assert_eq!(plan.deleted.len(), 1);
        assert_eq!(plan.deleted[0].chunk_ids, vec![7, 8]);
    }

    #[test]
    fn second_root_is_appended_and_isolated() {
        let mut m = empty_manifest();
        m.roots.push(RootEntry {
            id: 0,
            path: "/a".into(),
            last_indexed_unix_seconds: 0,
        });
        m.files.push(FileEntry {
            root_id: 0,
            rel_path: "doc.txt".into(),
            size: 10,
            mtime_ns: 1,
            content_hash: Some("blake3:x".into()),
            chunk_ids: vec![1],
        });

        let plan = plan_index(
            Path::new("/b"),
            vec![walked("other.txt", 5, 5)],
            &mut m,
            &never_hash,
        )
        .unwrap();
        assert_eq!(plan.root_id, 1);
        assert_eq!(plan.new.len(), 1);
        // /a root untouched
        assert!(plan.deleted.is_empty());
        assert_eq!(m.roots.len(), 2);
    }
}
```

In `crates/fastrag/src/corpus/mod.rs` add near the top:

```rust
pub mod incremental;
```

- [ ] **Step 3: Run tests**

```
cargo test -p fastrag incremental::
```

Expected: 6 new tests pass.

- [ ] **Step 4: Commit**

```
git add -A && git commit -m "feat(corpus): add IndexPlan classifier for incremental indexing"
```

---

## Task 5: Walk helper — `walk_for_plan`

**Files:**
- Modify: `crates/fastrag/src/corpus/incremental.rs`

- [ ] **Step 1: Write failing test**

Append to `incremental.rs`:

```rust
/// Walk `root` via the existing `collect_files` path and produce `WalkedFile`s
/// suitable for `plan_index`. Canonicalizes `root` once.
pub fn walk_for_plan(root: &Path) -> std::io::Result<(PathBuf, Vec<WalkedFile>)> {
    let root_abs = root.canonicalize()?;
    let files = if root_abs.is_file() {
        vec![root_abs.clone()]
    } else {
        crate::ops::collect_files(&root_abs)
    };
    let mut out = Vec::with_capacity(files.len());
    for path in files {
        let rel = path.strip_prefix(&root_abs).unwrap_or(&path).to_path_buf();
        let md = std::fs::metadata(&path)?;
        let mtime_ns = md
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_nanos() as i128)
            .unwrap_or(0);
        out.push(WalkedFile {
            rel_path: rel,
            abs_path: path,
            size: md.len(),
            mtime_ns,
        });
    }
    Ok((root_abs, out))
}
```

Test in the same module:

```rust
#[test]
fn walk_for_plan_produces_relative_paths_and_stat() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), b"hello").unwrap();
    std::fs::write(dir.path().join("b.txt"), b"world!!").unwrap();
    let (root, files) = walk_for_plan(dir.path()).unwrap();
    assert_eq!(root, dir.path().canonicalize().unwrap());
    let names: Vec<_> = files.iter().map(|f| f.rel_path.to_string_lossy().into_owned()).collect();
    assert!(names.contains(&"a.txt".to_string()));
    assert!(names.contains(&"b.txt".to_string()));
    let a = files.iter().find(|f| f.rel_path == std::path::Path::new("a.txt")).unwrap();
    assert_eq!(a.size, 5);
    assert!(a.mtime_ns > 0);
}
```

- [ ] **Step 2: Run tests**

```
cargo test -p fastrag incremental::tests::walk_for_plan
```

Expected: pass.

- [ ] **Step 3: Commit**

```
git add -A && git commit -m "feat(corpus): add walk_for_plan stat collector"
```

---

## Task 6: Rewrite `index_path_with_metadata` to apply the plan

**Files:**
- Modify: `crates/fastrag/src/corpus/mod.rs`

- [ ] **Step 1: Extend `CorpusIndexStats`**

Add fields (all `#[serde(default)]` for backward compat):

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CorpusIndexStats {
    pub corpus_dir: PathBuf,
    pub input_dir: PathBuf,
    pub files_indexed: usize,
    pub chunk_count: usize,
    pub manifest: CorpusManifest,
    #[serde(default)] pub files_unchanged: usize,
    #[serde(default)] pub files_changed: usize,
    #[serde(default)] pub files_new: usize,
    #[serde(default)] pub files_deleted: usize,
    #[serde(default)] pub chunks_added: usize,
    #[serde(default)] pub chunks_removed: usize,
}
```

Remove `#[serde(deny_unknown_fields)]` from `CorpusIndexStats`.

- [ ] **Step 2: Implement incremental `index_path_with_metadata`**

Replace the current body of `index_path_with_metadata` with:

```rust
pub fn index_path_with_metadata(
    input: &Path,
    corpus_dir: &Path,
    chunking: &ChunkingStrategy,
    embedder: &dyn Embedder,
    base_metadata: &std::collections::BTreeMap<String, String>,
) -> Result<CorpusIndexStats, CorpusError> {
    use crate::corpus::incremental::{plan_index, walk_for_plan};

    let (root_abs, walked) = walk_for_plan(input)?;
    if walked.is_empty() && !corpus_dir.join("manifest.json").exists() {
        return Err(CorpusError::NoParseableFiles(input.to_path_buf()));
    }

    // Load existing index, or create a fresh one.
    let mut index = if corpus_dir.join("manifest.json").exists() {
        HnswIndex::load(corpus_dir)?
    } else {
        let m = CorpusManifest::new(
            embedder.model_id().to_string(),
            embedder.dim(),
            current_unix_seconds(),
            manifest_chunking_strategy_from(chunking),
        );
        HnswIndex::new(embedder.dim(), m)
    };

    // Plan against a mutable clone of the manifest, then apply to the index's manifest
    // at the end (we need &mut CorpusManifest for plan_index).
    let mut manifest = index.manifest().clone();
    let plan = plan_index(
        &root_abs,
        walked,
        &mut manifest,
        &|p| fastrag_index::hash::hash_file(p),
    )?;

    // Determine next chunk id: max(existing) + 1.
    let mut next_id: u64 = index.entries().iter().map(|e| e.id).max().unwrap_or(0) + 1;

    // Remove chunks for changed + deleted files.
    let mut ids_to_remove: Vec<u64> = Vec::new();
    for f in &plan.deleted {
        ids_to_remove.extend(f.chunk_ids.iter().copied());
    }
    // For changed files we must also drop their existing chunks.
    let changed_rels: std::collections::HashSet<_> =
        plan.changed.iter().map(|w| w.rel_path.clone()).collect();
    for f in &manifest.files {
        if f.root_id == plan.root_id && changed_rels.contains(&f.rel_path) {
            ids_to_remove.extend(f.chunk_ids.iter().copied());
        }
    }
    let chunks_removed = ids_to_remove.len();
    index.remove_by_chunk_ids(&ids_to_remove);

    // Drop the deleted + changed entries from manifest.files for this root; we'll re-add changed below.
    manifest.files.retain(|f| {
        !(f.root_id == plan.root_id
            && (plan
                .deleted
                .iter()
                .any(|d| d.rel_path == f.rel_path)
                || changed_rels.contains(&f.rel_path)))
    });

    // Update touched files' stat in place.
    for (old, wf) in &plan.touched {
        if let Some(entry) = manifest.files.iter_mut().find(|f| {
            f.root_id == plan.root_id && f.rel_path == old.rel_path
        }) {
            entry.size = wf.size;
            entry.mtime_ns = wf.mtime_ns;
        }
    }

    // Re-embed changed + new files.
    let mut chunks_added = 0usize;
    let to_embed: Vec<_> = plan
        .changed
        .iter()
        .chain(plan.new.iter())
        .cloned()
        .collect();

    for wf in &to_embed {
        let doc = load_document(&wf.abs_path)?;
        let chunks = chunk_document(&doc, chunking);
        let texts: Vec<&str> = chunks.iter().map(|c| c.text.as_str()).collect();
        let vectors = embedder.embed(&texts)?;
        if vectors.len() != chunks.len() {
            return Err(CorpusError::EmbeddingOutputMismatch {
                expected: chunks.len(),
                got: vectors.len(),
            });
        }

        // Merge metadata
        let mut file_metadata = base_metadata.clone();
        let sidecar = sidecar_path_for(&wf.abs_path);
        if sidecar.exists() {
            file_metadata.extend(load_metadata_sidecar(&sidecar)?);
        }

        let mut chunk_ids = Vec::with_capacity(chunks.len());
        let entries: Vec<IndexEntry> = chunks
            .into_iter()
            .zip(vectors.into_iter())
            .map(|(chunk, vector)| {
                let id = next_id;
                next_id += 1;
                chunk_ids.push(id);
                IndexEntry {
                    id,
                    vector,
                    chunk_text: chunk.text.clone(),
                    source_path: wf.abs_path.clone(),
                    chunk_index: chunk.index,
                    section: chunk.section.clone(),
                    element_kinds: chunk.elements.iter().map(|e| e.kind.clone()).collect(),
                    pages: chunk
                        .elements
                        .iter()
                        .filter_map(|e| e.page)
                        .collect::<BTreeSet<_>>()
                        .into_iter()
                        .collect(),
                    language: chunk_language(&doc, &chunk),
                    metadata: file_metadata.clone(),
                }
            })
            .collect();
        chunks_added += entries.len();
        index.add(entries)?;

        // Compute hash for manifest (we already read the file in hash path if it was "changed",
        // but simplest correct path is to hash once more here for both changed+new).
        let content_hash = Some(fastrag_index::hash::hash_file(&wf.abs_path)?);

        manifest.files.push(fastrag_index::manifest::FileEntry {
            root_id: plan.root_id,
            rel_path: wf.rel_path.clone(),
            size: wf.size,
            mtime_ns: wf.mtime_ns,
            content_hash,
            chunk_ids,
        });
    }

    // Update root timestamp + schema version.
    manifest.version = 2;
    if let Some(r) = manifest.roots.iter_mut().find(|r| r.id == plan.root_id) {
        r.last_indexed_unix_seconds = current_unix_seconds();
    }
    manifest.chunk_count = index.entries().len();

    // Copy the updated manifest back into the index so save() persists it.
    index.replace_manifest(manifest.clone());
    index.save(corpus_dir)?;

    Ok(CorpusIndexStats {
        corpus_dir: corpus_dir.to_path_buf(),
        input_dir: input.to_path_buf(),
        files_indexed: plan.changed.len() + plan.new.len(),
        chunk_count: index.entries().len(),
        manifest,
        files_unchanged: plan.unchanged.len() + plan.touched.len(),
        files_changed: plan.changed.len(),
        files_new: plan.new.len(),
        files_deleted: plan.deleted.len(),
        chunks_added,
        chunks_removed,
    })
}
```

- [ ] **Step 3: Add `HnswIndex::replace_manifest`**

In `crates/fastrag-index/src/hnsw.rs` `impl HnswIndex`:

```rust
pub fn replace_manifest(&mut self, manifest: crate::manifest::CorpusManifest) {
    self.manifest = manifest;
}
```

- [ ] **Step 4: Run existing tests**

```
cargo test -p fastrag corpus::
```

Expected: existing `index_and_query_roundtrip` and metadata tests still pass (they index once — the incremental path handles a cold start identically to the old full-rebuild).

- [ ] **Step 5: Commit**

```
git add -A && git commit -m "feat(corpus): apply incremental plan in index_path_with_metadata"
```

---

## Task 7: End-to-end incremental tests

**Files:**
- Create: `crates/fastrag/tests/incremental.rs`

- [ ] **Step 1: Write tests**

```rust
use fastrag::corpus::{index_path, query_corpus, CorpusIndexStats};
use fastrag::ChunkingStrategy;
use fastrag_embed::test_utils::MockEmbedder;
use std::fs;
use tempfile::tempdir;

fn basic() -> ChunkingStrategy {
    ChunkingStrategy::Basic { max_characters: 1000, overlap: 0 }
}

fn write(dir: &std::path::Path, name: &str, body: &str) {
    fs::write(dir.join(name), body).unwrap();
}

fn reindex(input: &std::path::Path, corpus: &std::path::Path) -> CorpusIndexStats {
    index_path(input, corpus, &basic(), &MockEmbedder).unwrap()
}

#[test]
fn reindex_unchanged_does_no_work() {
    let input = tempdir().unwrap();
    let corpus = tempdir().unwrap();
    write(input.path(), "a.txt", "ALPHA\n\nalpha beta.");
    write(input.path(), "b.txt", "BETA\n\nbeta gamma.");

    let first = reindex(input.path(), corpus.path());
    assert_eq!(first.files_new, 2);
    assert_eq!(first.files_unchanged, 0);

    let second = reindex(input.path(), corpus.path());
    assert_eq!(second.files_new, 0);
    assert_eq!(second.files_changed, 0);
    assert_eq!(second.files_unchanged, 2);
    assert_eq!(second.chunks_added, 0);
    assert_eq!(second.chunks_removed, 0);
}

#[test]
fn edited_file_is_re_embedded_stale_chunks_gone() {
    let input = tempdir().unwrap();
    let corpus = tempdir().unwrap();
    write(input.path(), "a.txt", "alpha original content.");
    reindex(input.path(), corpus.path());

    // Bump mtime guaranteed: sleep-free approach — rewrite with different bytes.
    write(input.path(), "a.txt", "alpha replaced content xyz.");
    let stats = reindex(input.path(), corpus.path());
    assert_eq!(stats.files_changed, 1);
    assert_eq!(stats.files_new, 0);
    assert!(stats.chunks_removed >= 1);
    assert!(stats.chunks_added >= 1);

    // Stale text must not be queryable.
    let hits = query_corpus(corpus.path(), "original content", 5, &MockEmbedder).unwrap();
    assert!(hits.iter().all(|h| !h.entry.chunk_text.contains("original")));
}

#[test]
fn deleted_file_drops_chunks() {
    let input = tempdir().unwrap();
    let corpus = tempdir().unwrap();
    write(input.path(), "a.txt", "alpha.");
    write(input.path(), "b.txt", "beta.");
    reindex(input.path(), corpus.path());

    fs::remove_file(input.path().join("b.txt")).unwrap();
    let stats = reindex(input.path(), corpus.path());
    assert_eq!(stats.files_deleted, 1);
    assert!(stats.chunks_removed >= 1);

    let hits = query_corpus(corpus.path(), "beta", 5, &MockEmbedder).unwrap();
    assert!(hits.iter().all(|h| !h.entry.source_path.ends_with("b.txt")));
}

#[test]
fn two_roots_isolated() {
    let a = tempdir().unwrap();
    let b = tempdir().unwrap();
    let corpus = tempdir().unwrap();
    write(a.path(), "a.txt", "alpha.");
    write(b.path(), "b.txt", "beta.");

    reindex(a.path(), corpus.path());
    reindex(b.path(), corpus.path());

    let hits = query_corpus(corpus.path(), "alpha", 5, &MockEmbedder).unwrap();
    assert!(hits.iter().any(|h| h.entry.source_path.ends_with("a.txt")));
    let hits = query_corpus(corpus.path(), "beta", 5, &MockEmbedder).unwrap();
    assert!(hits.iter().any(|h| h.entry.source_path.ends_with("b.txt")));

    // Deleting in root A must not remove root B files.
    fs::remove_file(a.path().join("a.txt")).unwrap();
    let stats = reindex(a.path(), corpus.path());
    assert_eq!(stats.files_deleted, 1);

    let hits = query_corpus(corpus.path(), "beta", 5, &MockEmbedder).unwrap();
    assert!(hits.iter().any(|h| h.entry.source_path.ends_with("b.txt")));
}
```

- [ ] **Step 2: Run tests**

```
cargo test -p fastrag --test incremental
```

Expected: all 4 pass.

- [ ] **Step 3: Commit**

```
git add -A && git commit -m "test(corpus): end-to-end incremental indexing coverage"
```

---

## Task 8: CLI stats output

**Files:**
- Modify: `fastrag-cli/src/main.rs` (wherever `CorpusIndexStats` is printed for the `Index` command)

- [ ] **Step 1: Inspect current print**

```
grep -n "files_indexed" fastrag-cli/src/main.rs
```

- [ ] **Step 2: Extend the summary**

Find the `Index` command handler that prints stats. Replace the summary line(s) with:

```rust
println!(
    "indexed {} files ({} new, {} changed, {} unchanged, {} deleted) — {} chunks added, {} removed",
    stats.files_indexed,
    stats.files_new,
    stats.files_changed,
    stats.files_unchanged,
    stats.files_deleted,
    stats.chunks_added,
    stats.chunks_removed,
);
```

(Preserve any existing corpus path / embedder info line alongside it.)

- [ ] **Step 3: Run CLI smoke**

```
cargo run -p fastrag-cli -- index tests/fixtures --corpus /tmp/fastrag-inc-smoke
cargo run -p fastrag-cli -- index tests/fixtures --corpus /tmp/fastrag-inc-smoke
```

Expected: second run prints `0 new, 0 changed, N unchanged`.

- [ ] **Step 4: Commit**

```
git add -A && git commit -m "feat(cli): report incremental index stats"
```

---

## Task 9: README update

**Files:**
- Modify: `README.md`

Per CLAUDE.md: route all `.md` edits through the `doc-editor` skill before writing.

- [ ] **Step 1: Invoke doc-editor skill**

Read `.claude/skills/doc-editor/SKILL.md` and dispatch a foreground Haiku agent with the content + the diff target: a new subsection under the retrieval section describing incremental behavior, multi-root support, and the v1→v2 auto-migration.

Target content:

> ### Incremental indexing
>
> Re-running `fastrag index <root> --corpus <corpus>` is cheap: unchanged files are skipped via an (mtime, size) fast path, stat-changed files are hash-verified (blake3) before re-embedding, and files removed from disk are pruned from the corpus. One corpus can hold multiple input roots — each root's deletions only affect its own files. Old (v1) corpora auto-migrate on first index; the first run after migration hashes every file once and subsequent runs are incremental.

- [ ] **Step 2: Commit**

```
git add README.md && git commit -m "docs: document incremental indexing and multi-root"
```

---

## Task 10: Full verification + push + CI

- [ ] **Step 1: Format + lint + test**

```
cargo fmt --check
cargo clippy --workspace --all-targets --features retrieval -- -D warnings
cargo test --workspace
```

Expected: all green.

- [ ] **Step 2: Push**

```
git push
```

- [ ] **Step 3: CI watcher**

Launch the `ci-watcher.md` skill via a background Haiku Agent (per CLAUDE.md).

- [ ] **Step 4: Final commit (if CI needs fixes)**

Only if CI surfaces issues. Otherwise the plan is complete.

The closing commit should include `Closes #28` in the body — pick the last commit that ties the feature together (Task 10 if a fix is needed, else amend Task 8 before pushing by adding a dedicated "closes #28" commit after Task 9):

```
git commit --allow-empty -m "feat(corpus): incremental indexing (closes #28)

Closes #28"
```

---

## Self-review

- **Spec coverage:** hybrid stat+hash (Task 4), auto-prune with report (Task 8 CLI + Task 6 stats), multi-root (Task 4 `resolve_root` + Task 7 test), v1→v2 migration (Task 2 serde defaults — v1 manifests load with empty `roots`/`files`; Task 6 bumps `version = 2` on first write), manifest schema (Task 2), diff algorithm (Task 4), apply (Task 6), code organization (Task 4 module split, Task 6 orchestration), error handling (Task 6 — partial failure leaves old manifest, caller uses existing `CorpusError`), testing (Tasks 4, 5, 7).
- **Deviations from spec, flagged:**
  - Dropped **tombstone set + HNSW rebuild threshold** — unnecessary. `HnswIndex::add` already calls `rebuild_graph` from scratch on every add. `remove_by_chunk_ids` does the same. No tombstoning needed; the spec's `deleted: HashSet<u32>` is dead code.
  - Dropped **`fs2` lock file**. Existing codebase has no concurrent-index protection and adding it is scope creep for #28. Document as follow-up if needed.
  - Dropped **atomic `.tmp` rename for manifest/entries.bin**. Existing `HnswIndex::save` writes directly; keeping behavior consistent. Atomic rewrites are orthogonal robustness work.
  - Dropped **one-generation `.bak` of manifest**. Same reasoning.
- **Placeholder scan:** no TBDs, every code step has full code.
- **Type consistency:** `FileEntry.chunk_ids: Vec<u64>` matches `IndexEntry.id: u64` throughout. `remove_by_chunk_ids(&[u64])` matches. `root_id: u32` consistent.
- **Scope:** single plan, ~10 tasks, one feature.
