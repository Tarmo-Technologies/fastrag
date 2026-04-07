use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{EvalError, EvalResult};

pub const DATASET_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvalDocument {
    pub id: String,
    pub title: Option<String>,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvalQuery {
    pub id: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Qrel {
    pub query_id: String,
    pub doc_id: String,
    pub relevance: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvalDataset {
    pub name: String,
    pub documents: Vec<EvalDocument>,
    pub queries: Vec<EvalQuery>,
    pub qrels: Vec<Qrel>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct EvalDatasetFile {
    pub schema_version: u32,
    #[serde(flatten)]
    pub dataset: EvalDataset,
}

impl EvalDataset {
    pub fn load(path: impl AsRef<Path>) -> EvalResult<Self> {
        let path = path.as_ref();
        let raw = fs::read_to_string(path)?;
        let file: EvalDatasetFile = serde_json::from_str(&raw)?;
        if file.schema_version != DATASET_SCHEMA_VERSION {
            return Err(EvalError::UnsupportedSchemaVersion {
                expected: DATASET_SCHEMA_VERSION,
                got: file.schema_version,
            });
        }
        Ok(file.dataset)
    }

    pub fn write_json(&self, path: impl AsRef<Path>) -> EvalResult<()> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let file = EvalDatasetFile {
            schema_version: DATASET_SCHEMA_VERSION,
            dataset: self.clone(),
        };
        fs::write(path, serde_json::to_vec_pretty(&file)?)?;
        Ok(())
    }
}

impl From<PathBuf> for EvalDataset {
    fn from(path: PathBuf) -> Self {
        Self::load(path).expect("dataset should load")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_tiny_fixture() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tiny.json");
        let dataset = EvalDataset::load(&path).unwrap();
        assert_eq!(dataset.name, "tiny-synthetic");
        assert_eq!(dataset.documents.len(), 10);
        assert_eq!(dataset.queries.len(), 5);
        assert_eq!(dataset.qrels.len(), 5);
    }

    #[test]
    fn load_rejects_malformed_json() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.json");
        fs::write(&path, "{not-json").unwrap();
        let err = EvalDataset::load(&path).unwrap_err();
        assert!(err.to_string().contains("json error"));
    }
}
