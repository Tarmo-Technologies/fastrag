//! NVD 2.0 serde types — lifted from fastrag-eval and extended.
//! Full types defined in Task 4; this stub allows the skeleton to compile.

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct NvdFeed {
    #[serde(default)]
    pub vulnerabilities: Vec<NvdVulnerability>,
}

#[derive(Debug, Deserialize)]
pub struct NvdVulnerability {
    pub cve: Option<NvdCve>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NvdCve {
    pub id: Option<String>,
    #[serde(rename = "vulnStatus")]
    pub vuln_status: Option<String>,
    pub published: Option<String>,
    #[serde(default)]
    pub descriptions: Vec<NvdDescription>,
    pub metrics: Option<NvdMetrics>,
    pub configurations: Option<Vec<NvdConfiguration>>,
    pub references: Option<Vec<serde_json::Value>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NvdDescription {
    pub lang: String,
    pub value: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NvdMetrics {
    #[serde(rename = "cvssMetricV31", default)]
    pub cvss_metric_v31: Option<Vec<NvdCvssMetric>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NvdCvssMetric {
    #[serde(rename = "cvssData")]
    pub cvss_data: Option<NvdCvssData>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NvdCvssData {
    #[serde(rename = "baseSeverity")]
    pub base_severity: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NvdConfiguration {
    pub nodes: Option<Vec<NvdNode>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NvdNode {
    #[serde(rename = "cpeMatch", default)]
    pub cpe_match: Option<Vec<NvdCpeMatch>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NvdCpeMatch {
    pub criteria: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_path() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/nvd_slice.json")
    }

    #[test]
    fn round_trips_five_cve_fixture() {
        let bytes = std::fs::read(fixture_path()).expect("fixture must exist");
        let feed: NvdFeed = serde_json::from_slice(&bytes).expect("must parse");
        assert_eq!(feed.vulnerabilities.len(), 5);
        let ids: Vec<&str> = feed
            .vulnerabilities
            .iter()
            .filter_map(|v| v.cve.as_ref())
            .filter_map(|c| c.id.as_deref())
            .collect();
        assert!(ids.contains(&"CVE-2021-44228"), "Log4Shell must be present");
        assert!(
            ids.contains(&"CVE-2024-10001"),
            "Rejected CVE must be present"
        );
    }

    #[test]
    fn rejected_cve_has_correct_status() {
        let bytes = std::fs::read(fixture_path()).unwrap();
        let feed: NvdFeed = serde_json::from_slice(&bytes).unwrap();
        let rejected = feed
            .vulnerabilities
            .iter()
            .find(|v| v.cve.as_ref().and_then(|c| c.id.as_deref()) == Some("CVE-2024-10001"))
            .and_then(|v| v.cve.as_ref())
            .unwrap();
        assert_eq!(rejected.vuln_status.as_deref(), Some("Rejected"));
        assert!(
            rejected.descriptions[0].value.contains("REJECT"),
            "description must contain REJECT marker"
        );
    }

    #[test]
    fn log4shell_has_critical_cvss() {
        let bytes = std::fs::read(fixture_path()).unwrap();
        let feed: NvdFeed = serde_json::from_slice(&bytes).unwrap();
        let log4shell = feed
            .vulnerabilities
            .iter()
            .find(|v| v.cve.as_ref().and_then(|c| c.id.as_deref()) == Some("CVE-2021-44228"))
            .and_then(|v| v.cve.as_ref())
            .unwrap();
        let severity = log4shell
            .metrics
            .as_ref()
            .and_then(|m| m.cvss_metric_v31.as_ref())
            .and_then(|v| v.first())
            .and_then(|e| e.cvss_data.as_ref())
            .and_then(|d| d.base_severity.as_ref())
            .map(String::as_str);
        assert_eq!(severity, Some("CRITICAL"));
    }

    #[test]
    fn status_counts_match_fixture() {
        let bytes = std::fs::read(fixture_path()).unwrap();
        let feed: NvdFeed = serde_json::from_slice(&bytes).unwrap();
        let statuses: Vec<&str> = feed
            .vulnerabilities
            .iter()
            .filter_map(|v| v.cve.as_ref())
            .filter_map(|c| c.vuln_status.as_deref())
            .collect();
        let analyzed = statuses.iter().filter(|&&s| s == "Analyzed").count();
        let rejected = statuses.iter().filter(|&&s| s == "Rejected").count();
        let disputed = statuses.iter().filter(|&&s| s == "Disputed").count();
        let modified = statuses.iter().filter(|&&s| s == "Modified").count();
        assert_eq!(analyzed, 2, "expected 2 Analyzed");
        assert_eq!(rejected, 1, "expected 1 Rejected");
        assert_eq!(disputed, 1, "expected 1 Disputed");
        assert_eq!(modified, 1, "expected 1 Modified");
    }

    #[test]
    fn log4shell_cpe_vendor_is_apache() {
        let bytes = std::fs::read(fixture_path()).unwrap();
        let feed: NvdFeed = serde_json::from_slice(&bytes).unwrap();
        let log4shell = feed
            .vulnerabilities
            .iter()
            .find(|v| v.cve.as_ref().and_then(|c| c.id.as_deref()) == Some("CVE-2021-44228"))
            .and_then(|v| v.cve.as_ref())
            .unwrap();
        let criteria = log4shell
            .configurations
            .as_ref()
            .and_then(|c| c.first())
            .and_then(|cfg| cfg.nodes.as_ref())
            .and_then(|nodes| nodes.first())
            .and_then(|node| node.cpe_match.as_ref())
            .and_then(|matches| matches.first())
            .and_then(|m| m.criteria.as_ref())
            .map(String::as_str);
        assert_eq!(criteria, Some("cpe:2.3:a:apache:log4j:*:*:*:*:*:*:*:*"));
    }

    #[test]
    fn published_field_parses_correctly() {
        let bytes = std::fs::read(fixture_path()).unwrap();
        let feed: NvdFeed = serde_json::from_slice(&bytes).unwrap();
        let log4shell = feed
            .vulnerabilities
            .iter()
            .find(|v| v.cve.as_ref().and_then(|c| c.id.as_deref()) == Some("CVE-2021-44228"))
            .and_then(|v| v.cve.as_ref())
            .unwrap();
        assert_eq!(
            log4shell.published.as_deref(),
            Some("2021-12-10T10:15:00.000")
        );
    }
}
