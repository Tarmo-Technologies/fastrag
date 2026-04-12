//! MetadataRejectFilter: drops documents whose `vuln_status` metadata field
//! is in the configured reject-status set.

use std::collections::{BTreeMap, BTreeSet};

use fastrag_core::Chunk;

/// A filter applied to a single document (represented as its chunks + metadata).
/// Returns `false` to drop the document entirely, `true` to keep it.
pub trait DocFilter: Send + Sync {
    fn keep(&self, chunks: &[Chunk], metadata: &BTreeMap<String, String>) -> bool;
}

/// Drops documents whose `vuln_status` metadata value is in the reject set.
///
/// Default reject set: `{"Rejected", "Disputed"}` — matching the NVD `vulnStatus`
/// field values per the NVD 2.0 API specification.
pub struct MetadataRejectFilter {
    pub reject_statuses: BTreeSet<String>,
}

impl Default for MetadataRejectFilter {
    fn default() -> Self {
        Self {
            reject_statuses: ["Rejected", "Disputed"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
        }
    }
}

impl DocFilter for MetadataRejectFilter {
    fn keep(&self, _chunks: &[Chunk], metadata: &BTreeMap<String, String>) -> bool {
        match metadata.get("vuln_status") {
            Some(status) => !self.reject_statuses.contains(status),
            None => true, // no vuln_status → not an NVD doc, keep it
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(status: &str) -> BTreeMap<String, String> {
        let mut m = BTreeMap::new();
        m.insert("vuln_status".to_string(), status.to_string());
        m
    }

    fn empty_chunks() -> Vec<Chunk> {
        vec![]
    }

    #[test]
    fn drops_rejected_status() {
        let filter = MetadataRejectFilter::default();
        assert!(!filter.keep(&empty_chunks(), &meta("Rejected")));
    }

    #[test]
    fn drops_disputed_status() {
        let filter = MetadataRejectFilter::default();
        assert!(!filter.keep(&empty_chunks(), &meta("Disputed")));
    }

    #[test]
    fn keeps_analyzed_status() {
        let filter = MetadataRejectFilter::default();
        assert!(filter.keep(&empty_chunks(), &meta("Analyzed")));
    }

    #[test]
    fn keeps_modified_status() {
        let filter = MetadataRejectFilter::default();
        assert!(filter.keep(&empty_chunks(), &meta("Modified")));
    }

    #[test]
    fn keeps_doc_without_vuln_status() {
        let filter = MetadataRejectFilter::default();
        assert!(filter.keep(&empty_chunks(), &BTreeMap::new()));
    }

    #[test]
    fn custom_reject_set_drops_awaiting_analysis() {
        let mut filter = MetadataRejectFilter::default();
        filter
            .reject_statuses
            .insert("Awaiting Analysis".to_string());
        assert!(!filter.keep(&empty_chunks(), &meta("Awaiting Analysis")));
        assert!(filter.keep(&empty_chunks(), &meta("Analyzed")));
    }
}
