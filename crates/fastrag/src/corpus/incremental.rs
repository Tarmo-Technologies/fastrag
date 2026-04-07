use fastrag_index::{CorpusManifest, FileEntry, RootEntry};
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
    let id = manifest
        .roots
        .iter()
        .map(|r| r.id)
        .max()
        .map_or(0, |m| m + 1);
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
    use fastrag_index::ManifestChunkingStrategy;

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
            vec![walked("a.txt", 10, 999)],
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
        assert!(plan.deleted.is_empty());
        assert_eq!(m.roots.len(), 2);
    }
}
