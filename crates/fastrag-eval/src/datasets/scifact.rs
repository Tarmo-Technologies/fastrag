#[cfg(test)]
use std::path::Path;

use crate::{EvalDataset, EvalResult};

const NAME: &str = "scifact";
const URL: &str = "https://public.ukp.informatik.tu-darmstadt.de/thakur/BEIR/datasets/scifact.zip";
const SHA256: &str = "536e14446a0ba56ed1398ab1055f39fe852686ecad24a6306c80c490fa8e0165";

pub fn load_scifact() -> EvalResult<EvalDataset> {
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
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/datasets/scifact_mini")
    }

    #[test]
    fn loads_offline_fixture() {
        let dataset = load_from(&fixture_dir()).unwrap();
        assert_eq!(dataset.name, "scifact");
        assert_eq!(dataset.documents.len(), 5);
        assert_eq!(dataset.queries.len(), 3);
        assert_eq!(dataset.qrels.len(), 3);
        assert_eq!(dataset.documents[0].id, "4983");
        assert_eq!(dataset.queries[0].text, "microstructural development MRI");
        assert_eq!(dataset.qrels[1].doc_id, "190750");
    }
}
