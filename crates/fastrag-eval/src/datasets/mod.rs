mod common;

pub mod beir;
pub mod cwe;
pub mod nfcorpus;
pub mod nvd;
pub mod scifact;

pub use cwe::load_cwe_top25;
pub use nfcorpus::load_nfcorpus;
pub use nvd::{load_nvd, load_nvd_corpus_with_queries};
pub use scifact::load_scifact;

use crate::{EvalDataset, EvalError, EvalResult};

/// Identifier for a built-in eval dataset, used by the CLI's `--dataset-name` flag and
/// `scripts/run-eval.sh` so the harness can dispatch to the right loader without checking
/// pre-materialized JSON into the repo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DatasetName {
    NfCorpus,
    SciFact,
    Nvd,
    CweTop25,
}

impl DatasetName {
    pub fn as_str(self) -> &'static str {
        match self {
            DatasetName::NfCorpus => "nfcorpus",
            DatasetName::SciFact => "scifact",
            DatasetName::Nvd => "nvd",
            DatasetName::CweTop25 => "cwe",
        }
    }

    pub fn parse(value: &str) -> EvalResult<Self> {
        match value {
            "nfcorpus" => Ok(DatasetName::NfCorpus),
            "scifact" => Ok(DatasetName::SciFact),
            "nvd" => Ok(DatasetName::Nvd),
            "cwe" | "cwe-top-25" => Ok(DatasetName::CweTop25),
            other => Err(EvalError::MalformedDataset(format!(
                "unknown dataset name: {other}"
            ))),
        }
    }
}

/// Dispatch to the correct dataset loader. Each loader is responsible for caching its
/// download under the user's XDG cache directory, so repeated runs are cheap.
pub fn load_by_name(name: DatasetName) -> EvalResult<EvalDataset> {
    match name {
        DatasetName::NfCorpus => load_nfcorpus(),
        DatasetName::SciFact => load_scifact(),
        DatasetName::Nvd => load_nvd(),
        DatasetName::CweTop25 => load_cwe_top25(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_known_names() {
        assert_eq!(
            DatasetName::parse("nfcorpus").unwrap(),
            DatasetName::NfCorpus
        );
        assert_eq!(DatasetName::parse("scifact").unwrap(), DatasetName::SciFact);
        assert_eq!(DatasetName::parse("nvd").unwrap(), DatasetName::Nvd);
        assert_eq!(DatasetName::parse("cwe").unwrap(), DatasetName::CweTop25);
        assert_eq!(
            DatasetName::parse("cwe-top-25").unwrap(),
            DatasetName::CweTop25
        );
    }

    #[test]
    fn parse_rejects_unknown() {
        assert!(DatasetName::parse("imdb").is_err());
    }

    #[test]
    fn as_str_roundtrip() {
        for name in [
            DatasetName::NfCorpus,
            DatasetName::SciFact,
            DatasetName::Nvd,
            DatasetName::CweTop25,
        ] {
            assert_eq!(DatasetName::parse(name.as_str()).unwrap(), name);
        }
    }
}
