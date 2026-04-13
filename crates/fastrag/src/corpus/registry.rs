use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// State of a corpus handle.
pub enum CorpusState {
    Unloaded,
    Loaded,
}

/// Handle to a single named corpus entry.
pub struct CorpusHandle {
    pub path: PathBuf,
    pub state: CorpusState,
}

/// Thread-safe registry of named corpora with lazy loading.
pub struct CorpusRegistry {
    inner: Arc<Mutex<HashMap<String, CorpusHandle>>>,
}

impl CorpusRegistry {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn register(&self, name: impl Into<String>, path: PathBuf) {
        let mut map = self.inner.lock().expect("CorpusRegistry mutex poisoned");
        map.insert(
            name.into(),
            CorpusHandle {
                path,
                state: CorpusState::Unloaded,
            },
        );
    }

    pub fn corpus_path(&self, name: &str) -> Option<PathBuf> {
        let map = self.inner.lock().expect("CorpusRegistry mutex poisoned");
        map.get(name).map(|h| h.path.clone())
    }

    /// Returns `(name, path, loaded)` for every registered corpus.
    pub fn list(&self) -> Vec<(String, PathBuf, bool)> {
        let map = self.inner.lock().expect("CorpusRegistry mutex poisoned");
        let mut entries: Vec<(String, PathBuf, bool)> = map
            .iter()
            .map(|(name, handle)| {
                let loaded = matches!(handle.state, CorpusState::Loaded);
                (name.clone(), handle.path.clone(), loaded)
            })
            .collect();
        // Sort by name for deterministic output.
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        entries
    }

    /// Parse a corpus argument of the form `name=path` or bare `path`.
    ///
    /// - `"nvd=/data/nvd"` → `("nvd", PathBuf::from("/data/nvd"))`
    /// - `"./corpus"` → `("default", PathBuf::from("./corpus"))`
    ///
    /// Only the first `=` is treated as the separator; `=` in the path is
    /// preserved.
    pub fn parse_corpus_arg(s: &str) -> (String, PathBuf) {
        match s.find('=') {
            Some(idx) => {
                let name = s[..idx].to_string();
                let path = PathBuf::from(&s[idx + 1..]);
                (name, path)
            }
            None => ("default".to_string(), PathBuf::from(s)),
        }
    }
}

impl Clone for CorpusRegistry {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl Default for CorpusRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_and_list() {
        let registry = CorpusRegistry::new();
        registry.register("alpha", PathBuf::from("/data/alpha"));
        registry.register("beta", PathBuf::from("/data/beta"));

        let entries = registry.list();
        assert_eq!(entries.len(), 2);

        // Sorted by name: alpha, beta
        assert_eq!(entries[0].0, "alpha");
        assert_eq!(entries[0].1, PathBuf::from("/data/alpha"));
        assert!(!entries[0].2, "alpha should start unloaded");

        assert_eq!(entries[1].0, "beta");
        assert_eq!(entries[1].1, PathBuf::from("/data/beta"));
        assert!(!entries[1].2, "beta should start unloaded");
    }

    #[test]
    fn corpus_path_returns_none_for_unknown() {
        let registry = CorpusRegistry::new();
        registry.register("known", PathBuf::from("/data/known"));
        assert_eq!(registry.corpus_path("unknown"), None);
    }

    #[test]
    fn corpus_path_returns_path_for_known() {
        let registry = CorpusRegistry::new();
        registry.register("nvd", PathBuf::from("/data/nvd"));
        assert_eq!(
            registry.corpus_path("nvd"),
            Some(PathBuf::from("/data/nvd"))
        );
    }

    #[test]
    fn parse_corpus_arg_with_equals() {
        let (name, path) = CorpusRegistry::parse_corpus_arg("nvd=/data/nvd");
        assert_eq!(name, "nvd");
        assert_eq!(path, PathBuf::from("/data/nvd"));
    }

    #[test]
    fn parse_corpus_arg_without_equals_is_default() {
        let (name, path) = CorpusRegistry::parse_corpus_arg("./corpus");
        assert_eq!(name, "default");
        assert_eq!(path, PathBuf::from("./corpus"));
    }

    #[test]
    fn parse_corpus_arg_path_with_equals_in_path() {
        let (name, path) = CorpusRegistry::parse_corpus_arg("nvd=/data/nvd=2024");
        assert_eq!(name, "nvd");
        assert_eq!(path, PathBuf::from("/data/nvd=2024"));
    }

    #[test]
    fn clone_shares_state() {
        let registry = CorpusRegistry::new();
        registry.register("shared", PathBuf::from("/data/shared"));
        let clone = registry.clone();
        // Register via clone, visible in original
        clone.register("added", PathBuf::from("/data/added"));
        assert_eq!(
            registry.corpus_path("added"),
            Some(PathBuf::from("/data/added"))
        );
    }
}
