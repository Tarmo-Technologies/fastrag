use std::collections::HashMap;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::{EvalError, EvalResult};

pub const REPORT_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LatencyStats {
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
    pub mean_ms: f64,
    pub count: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryStats {
    pub peak_rss_bytes: u64,
    pub current_rss_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvalReport {
    pub dataset: String,
    pub embedder: String,
    pub chunking: String,
    pub metrics: HashMap<String, f64>,
    pub latency: LatencyStats,
    pub memory: MemoryStats,
    pub build_time_ms: u64,
    pub run_at_unix: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct EvalReportFile {
    pub schema_version: u32,
    #[serde(flatten)]
    pub report: EvalReport,
}

impl EvalReport {
    pub fn write_json(&self, path: impl AsRef<Path>) -> EvalResult<()> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        } else {
            return Err(EvalError::MissingReportParent {
                path: path.to_path_buf(),
            });
        }

        let file = EvalReportFile {
            schema_version: REPORT_SCHEMA_VERSION,
            report: self.clone(),
        };
        fs::write(path, serde_json::to_vec_pretty(&file)?)?;
        Ok(())
    }

    pub fn read_json(path: impl AsRef<Path>) -> EvalResult<Self> {
        let raw = fs::read_to_string(path)?;
        let file: EvalReportFile = serde_json::from_str(&raw)?;
        if file.schema_version != REPORT_SCHEMA_VERSION {
            return Err(EvalError::UnsupportedSchemaVersion {
                expected: REPORT_SCHEMA_VERSION,
                got: file.schema_version,
            });
        }
        Ok(file.report)
    }

    pub fn to_report_file(&self) -> EvalReportEnvelope {
        EvalReportEnvelope {
            schema_version: REPORT_SCHEMA_VERSION,
            report: self.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvalReportEnvelope {
    pub schema_version: u32,
    #[serde(flatten)]
    pub report: EvalReport,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_report() -> EvalReport {
        EvalReport {
            dataset: "tiny".to_string(),
            embedder: "mock".to_string(),
            chunking: "basic".to_string(),
            metrics: HashMap::from([("recall@10".to_string(), 1.0)]),
            latency: LatencyStats {
                p50_ms: 1.0,
                p95_ms: 2.0,
                p99_ms: 3.0,
                mean_ms: 1.5,
                count: 5,
            },
            memory: MemoryStats {
                peak_rss_bytes: 123,
                current_rss_bytes: 100,
            },
            build_time_ms: 42,
            run_at_unix: 99,
        }
    }

    #[test]
    fn json_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("report.json");
        let report = sample_report();
        report.write_json(&path).unwrap();
        let restored = EvalReport::read_json(&path).unwrap();
        assert_eq!(restored, report);
    }

    #[test]
    fn schema_version_present() {
        let report = sample_report();
        let json = serde_json::to_value(report.to_report_file()).unwrap();
        assert_eq!(json["schema_version"], 1);
    }
}
