/// Extracts security identifiers (CVE, CWE) from free-text queries.
///
/// Used by the hybrid retrieval pipeline to short-circuit exact term lookups
/// in Tantivy before general BM25/dense search.
use regex::Regex;
use std::sync::LazyLock;

static CVE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\bCVE-\d{4}-\d{4,7}\b").unwrap());

static CWE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\bCWE-\d+\b").unwrap());

/// A security identifier extracted from query text.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SecurityId {
    /// CVE identifier, e.g. `CVE-2024-1234`.
    Cve(String),
    /// CWE identifier, e.g. `CWE-79`.
    Cwe(String),
}

/// Extract all CVE and CWE identifiers from `text`.
///
/// Returns an empty vec for non-security text (cheap no-op — just two regex
/// scans). Identifiers are uppercased for consistency.
pub fn extract_security_identifiers(text: &str) -> Vec<SecurityId> {
    let mut ids = Vec::new();

    for m in CVE_RE.find_iter(text) {
        ids.push(SecurityId::Cve(m.as_str().to_uppercase()));
    }
    for m in CWE_RE.find_iter(text) {
        ids.push(SecurityId::Cwe(m.as_str().to_uppercase()));
    }

    ids
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_cve_from_text() {
        let ids = extract_security_identifiers("Check CVE-2024-1234 for details");
        assert_eq!(ids, vec![SecurityId::Cve("CVE-2024-1234".into())]);
    }

    #[test]
    fn extracts_cwe_from_text() {
        let ids = extract_security_identifiers("This relates to CWE-79");
        assert_eq!(ids, vec![SecurityId::Cwe("CWE-79".into())]);
    }

    #[test]
    fn extracts_multiple_identifiers() {
        let ids =
            extract_security_identifiers("CVE-2023-44487 and CVE-2024-0001 are related to CWE-400");
        assert_eq!(
            ids,
            vec![
                SecurityId::Cve("CVE-2023-44487".into()),
                SecurityId::Cve("CVE-2024-0001".into()),
                SecurityId::Cwe("CWE-400".into()),
            ]
        );
    }

    #[test]
    fn case_insensitive() {
        let ids = extract_security_identifiers("see cve-2024-5678 and cwe-89");
        assert_eq!(
            ids,
            vec![
                SecurityId::Cve("CVE-2024-5678".into()),
                SecurityId::Cwe("CWE-89".into()),
            ]
        );
    }

    #[test]
    fn empty_on_non_security_text() {
        let ids = extract_security_identifiers("Rust is a systems programming language");
        assert!(ids.is_empty());
    }

    #[test]
    fn handles_long_cve_ids() {
        // CVE IDs can have 4-7 digit suffixes
        let ids = extract_security_identifiers("CVE-2024-1234567");
        assert_eq!(ids, vec![SecurityId::Cve("CVE-2024-1234567".into())]);
    }

    #[test]
    fn rejects_partial_matches() {
        // Should not match partial CVE-like strings
        let ids = extract_security_identifiers("CVE-2024-12 is too short");
        assert!(ids.is_empty());

        // Should not match embedded in words
        let ids = extract_security_identifiers("prefixCVE-2024-1234suffix");
        // "prefix" immediately before CVE means \b won't match
        assert!(ids.is_empty());
    }
}
