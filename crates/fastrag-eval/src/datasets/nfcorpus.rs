#[cfg(test)]
use std::path::Path;

use crate::{EvalDataset, EvalResult};

const NAME: &str = "nfcorpus";
const URL: &str = "https://public.ukp.informatik.tu-darmstadt.de/thakur/BEIR/datasets/nfcorpus.zip";
const SHA256: &str = "efe5be03f8c5b86a5870102d0599d227c8c6e2484328e68c6522560385671b0b";

pub fn load_nfcorpus() -> EvalResult<EvalDataset> {
    super::beir::load(NAME, URL, SHA256)
}

#[cfg(test)]
fn load_from(path: &Path) -> EvalResult<EvalDataset> {
    super::beir::load_from_dir(NAME, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_dir() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/datasets/nfcorpus_mini")
    }

    #[test]
    fn loads_offline_fixture() {
        let dataset = load_from(&fixture_dir()).unwrap();
        assert_eq!(dataset.name, "nfcorpus");
        assert_eq!(dataset.documents.len(), 5);
        assert_eq!(dataset.queries.len(), 3);
        assert_eq!(dataset.qrels.len(), 3);
        assert_eq!(dataset.documents[0].id, "MED-10");
        assert_eq!(dataset.queries[1].text, "heart disease statin evidence");
        assert_eq!(dataset.qrels[2].doc_id, "MED-50");
    }
}
