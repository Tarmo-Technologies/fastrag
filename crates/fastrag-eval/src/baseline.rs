//! Checked-in baseline + slack gate for eval regressions.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::buckets::BucketMetrics;
use crate::error::EvalError;
use crate::matrix::{ConfigVariant, MatrixReport};

pub const DEFAULT_SLACK: f64 = 0.02;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Baseline {
    pub schema_version: u32,
    pub git_rev: String,
    pub captured_at: String,
    pub runs: Vec<VariantBaseline>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub per_bucket_slack: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VariantBaseline {
    pub variant: ConfigVariant,
    pub hit_at_5: f64,
    pub mrr_at_10: f64,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub buckets:
        std::collections::BTreeMap<String, std::collections::BTreeMap<String, BucketMetrics>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Regression {
    pub variant: ConfigVariant,
    pub metric: String,
    pub baseline: f64,
    pub current: f64,
    pub delta: f64,
    pub slack: f64,
}

#[derive(Debug, Default)]
pub struct BaselineDiff {
    pub regressions: Vec<Regression>,
}

impl BaselineDiff {
    pub fn has_regressions(&self) -> bool {
        !self.regressions.is_empty()
    }

    pub fn render_report(&self) -> String {
        if self.regressions.is_empty() {
            return "## Baseline OK — no regressions\n".into();
        }
        let mut out = format!("## Baseline regressions ({})\n", self.regressions.len());
        for r in &self.regressions {
            let pct = ((r.current - r.baseline) / r.baseline) * 100.0;
            out.push_str(&format!(
                "- {:?} {}: {:.4} → {:.4} ({:+.2}%, slack ±{:.0}%)\n",
                r.variant,
                r.metric,
                r.baseline,
                r.current,
                pct,
                r.slack * 100.0,
            ));
        }
        out
    }
}

pub fn load_baseline(path: &Path) -> Result<Baseline, EvalError> {
    let bytes = std::fs::read(path).map_err(EvalError::from)?;
    serde_json::from_slice(&bytes).map_err(|e| EvalError::BaselineLoad {
        path: path.to_path_buf(),
        source: e,
    })
}

pub fn diff(report: &MatrixReport, baseline: &Baseline) -> Result<BaselineDiff, EvalError> {
    if report.schema_version != baseline.schema_version {
        return Err(EvalError::BaselineSchemaMismatch {
            baseline_version: baseline.schema_version,
            report_version: report.schema_version,
        });
    }

    let mut regressions = Vec::new();
    for base in &baseline.runs {
        let Some(run) = report.runs.iter().find(|r| r.variant == base.variant) else {
            eprintln!(
                "[baseline] skipping {:?} — not in current run",
                base.variant
            );
            continue;
        };

        check(
            &mut regressions,
            base.variant,
            "hit@5",
            base.hit_at_5,
            run.hit_at_5,
        );
        check(
            &mut regressions,
            base.variant,
            "MRR@10",
            base.mrr_at_10,
            run.mrr_at_10,
        );
    }

    let bucket_slack = baseline.per_bucket_slack.unwrap_or(DEFAULT_SLACK);
    for bv in &baseline.runs {
        let Some(run) = report.runs.iter().find(|r| r.variant == bv.variant) else {
            continue;
        };
        for (axis, bucket_map) in &bv.buckets {
            let Some(run_axis) = run.buckets.get(axis) else {
                continue;
            };
            for (value, baseline_m) in bucket_map {
                let Some(current_m) = run_axis.get(value) else {
                    continue;
                };
                let delta = current_m.hit_at_5 - baseline_m.hit_at_5;
                if delta + bucket_slack < 0.0 {
                    regressions.push(Regression {
                        variant: bv.variant,
                        metric: format!("hit_at_5[{axis}.{value}]"),
                        baseline: baseline_m.hit_at_5,
                        current: current_m.hit_at_5,
                        delta,
                        slack: bucket_slack,
                    });
                }
            }
        }
    }

    Ok(BaselineDiff { regressions })
}

/// Gates that enforce v1 temporal-decay contracts against a freshly-computed
/// `MatrixReport`. Returns `Err` with a descriptive `EvalError::TemporalGate` on
/// the first failing gate. Gates are skipped when a required variant is absent
/// from the report.
///
/// 1. `TemporalAuto.historical.hit@5 ≥ Primary.historical.hit@5 − 0.02`
/// 2. `TemporalAuto.neutral.hit@5 ≥ Primary.neutral.hit@5 − 0.01`
/// 3. `TemporalAuto.recency_seeking.mrr@10 ≥ Primary.recency_seeking.mrr@10`
/// 4. `TemporalOracle.recency_seeking.mrr@10 ≥ TemporalAuto.recency_seeking.mrr@10`
pub fn enforce_temporal_gates(report: &MatrixReport) -> Result<(), EvalError> {
    let hit5 = |v: ConfigVariant, bucket: &str| -> Option<f64> {
        report
            .runs
            .iter()
            .find(|r| r.variant == v)
            .and_then(|r| r.buckets.get("temporal_intent"))
            .and_then(|b| b.get(bucket))
            .map(|m| m.hit_at_5)
    };
    let mrr10 = |v: ConfigVariant, bucket: &str| -> Option<f64> {
        report
            .runs
            .iter()
            .find(|r| r.variant == v)
            .and_then(|r| r.buckets.get("temporal_intent"))
            .and_then(|b| b.get(bucket))
            .map(|m| m.mrr_at_10)
    };

    // Gate 1: abstain contract — historical regression ≤ 2pp.
    if let (Some(p), Some(a)) = (
        hit5(ConfigVariant::Primary, "historical"),
        hit5(ConfigVariant::TemporalAuto, "historical"),
    ) && a < p - 0.02
    {
        return Err(EvalError::TemporalGate(format!(
            "TemporalAuto historical hit@5 regressed >2pp vs primary: \
             primary={p:.4} auto={a:.4}"
        )));
    }

    // Gate 2: abstain contract — neutral regression ≤ 1pp (tighter).
    if let (Some(p), Some(a)) = (
        hit5(ConfigVariant::Primary, "neutral"),
        hit5(ConfigVariant::TemporalAuto, "neutral"),
    ) && a < p - 0.01
    {
        return Err(EvalError::TemporalGate(format!(
            "TemporalAuto neutral hit@5 regressed >1pp vs primary: \
             primary={p:.4} auto={a:.4}"
        )));
    }

    // Gate 3: directional improvement — recency_seeking mrr must not drop.
    if let (Some(p), Some(a)) = (
        mrr10(ConfigVariant::Primary, "recency_seeking"),
        mrr10(ConfigVariant::TemporalAuto, "recency_seeking"),
    ) && a < p
    {
        return Err(EvalError::TemporalGate(format!(
            "TemporalAuto recency_seeking mrr@10 regressed vs primary: \
             primary={p:.4} auto={a:.4}"
        )));
    }

    // Gate 4: oracle is upper bound on TemporalAuto.
    if let (Some(a), Some(o)) = (
        mrr10(ConfigVariant::TemporalAuto, "recency_seeking"),
        mrr10(ConfigVariant::TemporalOracle, "recency_seeking"),
    ) && o < a
    {
        return Err(EvalError::TemporalGate(format!(
            "TemporalOracle recency_seeking mrr@10 below TemporalAuto (upper-bound contract): \
             auto={a:.4} oracle={o:.4}"
        )));
    }

    Ok(())
}

fn check(
    out: &mut Vec<Regression>,
    variant: ConfigVariant,
    metric: &str,
    baseline: f64,
    current: f64,
) {
    let threshold = baseline * (1.0 - DEFAULT_SLACK);
    if current < threshold {
        out.push(Regression {
            variant,
            metric: metric.to_string(),
            baseline,
            current,
            delta: current - baseline,
            slack: DEFAULT_SLACK,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::matrix::*;

    fn mk_report(primary_hit5: f64, primary_mrr: f64) -> MatrixReport {
        let zero_pct = LatencyPercentiles {
            total: Percentiles {
                p50_us: 0,
                p95_us: 0,
                p99_us: 0,
            },
            embed: Percentiles {
                p50_us: 0,
                p95_us: 0,
                p99_us: 0,
            },
            bm25: Percentiles {
                p50_us: 0,
                p95_us: 0,
                p99_us: 0,
            },
            hnsw: Percentiles {
                p50_us: 0,
                p95_us: 0,
                p99_us: 0,
            },
            rerank: Percentiles {
                p50_us: 0,
                p95_us: 0,
                p99_us: 0,
            },
            fuse: Percentiles {
                p50_us: 0,
                p95_us: 0,
                p99_us: 0,
            },
        };
        MatrixReport {
            schema_version: 2,
            git_rev: "x".into(),
            captured_at: "x".into(),
            runs: vec![VariantReport {
                variant: ConfigVariant::Primary,
                hit_at_1: 0.0,
                hit_at_5: primary_hit5,
                hit_at_10: 0.0,
                mrr_at_10: primary_mrr,
                latency: zero_pct,
                per_question: vec![],
                buckets: Default::default(),
            }],
            rerank_delta: 0.0,
            contextual_delta: 0.0,
            hybrid_delta: 0.0,
            summary: Default::default(),
        }
    }

    fn mk_baseline(primary_hit5: f64, primary_mrr: f64) -> Baseline {
        Baseline {
            schema_version: 2,
            git_rev: "x".into(),
            captured_at: "x".into(),
            runs: vec![VariantBaseline {
                variant: ConfigVariant::Primary,
                hit_at_5: primary_hit5,
                mrr_at_10: primary_mrr,
                buckets: Default::default(),
            }],
            per_bucket_slack: None,
        }
    }

    #[test]
    fn exact_match_has_no_regressions() {
        let d = diff(&mk_report(0.82, 0.71), &mk_baseline(0.82, 0.71)).unwrap();
        assert!(!d.has_regressions());
    }

    #[test]
    fn exactly_two_percent_drop_passes_at_boundary() {
        // threshold = 0.82 * 0.98 = 0.8036
        // 0.8036 meets the threshold (>= comparison internally is `<` so we need > threshold)
        let d = diff(&mk_report(0.8036, 0.71), &mk_baseline(0.82, 0.71)).unwrap();
        assert!(
            !d.has_regressions(),
            "boundary should pass, got: {:?}",
            d.regressions
        );
    }

    #[test]
    fn just_past_two_percent_drop_is_a_regression() {
        let d = diff(&mk_report(0.80, 0.71), &mk_baseline(0.82, 0.71)).unwrap();
        assert_eq!(d.regressions.len(), 1);
        assert_eq!(d.regressions[0].metric, "hit@5");
    }

    #[test]
    fn schema_mismatch_fails_hard() {
        let mut r = mk_report(0.82, 0.71);
        r.schema_version = 99;
        let err = diff(&r, &mk_baseline(0.82, 0.71)).unwrap_err();
        assert!(format!("{err}").contains("schema"));
    }

    #[test]
    fn render_report_no_regressions_is_ok_line() {
        let d = BaselineDiff::default();
        assert!(d.render_report().contains("Baseline OK"));
    }

    #[test]
    fn partial_report_skips_missing_variants() {
        // Baseline has Primary + NoRerank; report only has Primary.
        let report = mk_report(0.82, 0.71);
        let baseline = Baseline {
            schema_version: 2,
            git_rev: "x".into(),
            captured_at: "x".into(),
            runs: vec![
                VariantBaseline {
                    variant: ConfigVariant::Primary,
                    hit_at_5: 0.82,
                    mrr_at_10: 0.71,
                    buckets: Default::default(),
                },
                VariantBaseline {
                    variant: ConfigVariant::NoRerank,
                    hit_at_5: 0.75,
                    mrr_at_10: 0.65,
                    buckets: Default::default(),
                },
            ],
            per_bucket_slack: None,
        };
        let d = diff(&report, &baseline).expect("should not error on missing variant");
        assert!(!d.has_regressions());
    }

    #[test]
    fn render_report_with_regression_names_variant_and_metric() {
        let d = diff(&mk_report(0.79, 0.60), &mk_baseline(0.82, 0.71)).unwrap();
        let out = d.render_report();
        assert!(out.contains("Primary"));
        assert!(out.contains("hit@5"));
        assert!(out.contains("MRR@10"));
    }
}

#[cfg(test)]
mod temporal_gate_tests {
    use super::*;
    use crate::buckets::BucketMetrics;
    use crate::matrix::{LatencyPercentiles, Percentiles, VariantReport};
    use std::collections::BTreeMap;

    fn zero_pct() -> LatencyPercentiles {
        LatencyPercentiles {
            total: Percentiles {
                p50_us: 0,
                p95_us: 0,
                p99_us: 0,
            },
            embed: Percentiles {
                p50_us: 0,
                p95_us: 0,
                p99_us: 0,
            },
            bm25: Percentiles {
                p50_us: 0,
                p95_us: 0,
                p99_us: 0,
            },
            hnsw: Percentiles {
                p50_us: 0,
                p95_us: 0,
                p99_us: 0,
            },
            rerank: Percentiles {
                p50_us: 0,
                p95_us: 0,
                p99_us: 0,
            },
            fuse: Percentiles {
                p50_us: 0,
                p95_us: 0,
                p99_us: 0,
            },
        }
    }

    fn bucket_metrics(hit5: f64, mrr: f64) -> BucketMetrics {
        BucketMetrics {
            hit_at_1: 0.0,
            hit_at_5: hit5,
            hit_at_10: hit5,
            mrr_at_10: mrr,
            n: 20,
        }
    }

    fn variant_report(
        variant: ConfigVariant,
        temporal_buckets: BTreeMap<String, BucketMetrics>,
    ) -> VariantReport {
        let mut buckets: BTreeMap<String, BTreeMap<String, BucketMetrics>> = BTreeMap::new();
        if !temporal_buckets.is_empty() {
            buckets.insert("temporal_intent".into(), temporal_buckets);
        }
        VariantReport {
            variant,
            hit_at_1: 0.0,
            hit_at_5: 0.0,
            hit_at_10: 0.0,
            mrr_at_10: 0.0,
            latency: zero_pct(),
            per_question: vec![],
            buckets,
        }
    }

    fn mk_report_v2(runs: Vec<VariantReport>) -> MatrixReport {
        MatrixReport {
            schema_version: 2,
            git_rev: "x".into(),
            captured_at: "x".into(),
            runs,
            rerank_delta: 0.0,
            contextual_delta: 0.0,
            hybrid_delta: 0.0,
            summary: Default::default(),
        }
    }

    fn mock_report_with_historical(primary_hit5: f64, auto_hit5: f64) -> MatrixReport {
        let mut pb = BTreeMap::new();
        pb.insert("historical".into(), bucket_metrics(primary_hit5, 0.5));
        let mut ab = BTreeMap::new();
        ab.insert("historical".into(), bucket_metrics(auto_hit5, 0.5));
        mk_report_v2(vec![
            variant_report(ConfigVariant::Primary, pb),
            variant_report(ConfigVariant::TemporalAuto, ab),
        ])
    }

    fn mock_report_with_neutral(primary_hit5: f64, auto_hit5: f64) -> MatrixReport {
        let mut pb = BTreeMap::new();
        pb.insert("neutral".into(), bucket_metrics(primary_hit5, 0.5));
        let mut ab = BTreeMap::new();
        ab.insert("neutral".into(), bucket_metrics(auto_hit5, 0.5));
        mk_report_v2(vec![
            variant_report(ConfigVariant::Primary, pb),
            variant_report(ConfigVariant::TemporalAuto, ab),
        ])
    }

    fn mock_report_with_recency_mrr_auto_vs_primary(
        primary_mrr: f64,
        auto_mrr: f64,
    ) -> MatrixReport {
        let mut pb = BTreeMap::new();
        pb.insert("recency_seeking".into(), bucket_metrics(0.5, primary_mrr));
        let mut ab = BTreeMap::new();
        ab.insert("recency_seeking".into(), bucket_metrics(0.5, auto_mrr));
        mk_report_v2(vec![
            variant_report(ConfigVariant::Primary, pb),
            variant_report(ConfigVariant::TemporalAuto, ab),
        ])
    }

    fn mock_report_with_oracle_and_auto(auto_mrr: f64, oracle_mrr: f64) -> MatrixReport {
        let mut ab = BTreeMap::new();
        ab.insert("recency_seeking".into(), bucket_metrics(0.5, auto_mrr));
        let mut ob = BTreeMap::new();
        ob.insert("recency_seeking".into(), bucket_metrics(0.5, oracle_mrr));
        // primary has same recency mrr as auto so gate 3 doesn't interfere
        let mut pb = BTreeMap::new();
        pb.insert("recency_seeking".into(), bucket_metrics(0.5, auto_mrr));
        mk_report_v2(vec![
            variant_report(ConfigVariant::Primary, pb),
            variant_report(ConfigVariant::TemporalAuto, ab),
            variant_report(ConfigVariant::TemporalOracle, ob),
        ])
    }

    #[test]
    fn temporal_auto_historical_regression_above_2pp_fails_gate() {
        let report = mock_report_with_historical(1.0, 0.97);
        let err = enforce_temporal_gates(&report).unwrap_err();
        assert!(
            err.to_string().to_lowercase().contains("historical"),
            "err should mention historical: {err}"
        );
    }

    #[test]
    fn temporal_auto_historical_regression_within_2pp_passes_gate() {
        let report = mock_report_with_historical(1.0, 0.985);
        assert!(enforce_temporal_gates(&report).is_ok());
    }

    #[test]
    fn temporal_auto_neutral_regression_above_1pp_fails_gate() {
        let report = mock_report_with_neutral(0.90, 0.88);
        let err = enforce_temporal_gates(&report).unwrap_err();
        assert!(err.to_string().to_lowercase().contains("neutral"));
    }

    #[test]
    fn temporal_auto_neutral_regression_within_1pp_passes_gate() {
        let report = mock_report_with_neutral(0.90, 0.893);
        assert!(enforce_temporal_gates(&report).is_ok());
    }

    #[test]
    fn temporal_auto_recency_mrr_regression_fails_gate() {
        let report = mock_report_with_recency_mrr_auto_vs_primary(0.60, 0.55);
        let err = enforce_temporal_gates(&report).unwrap_err();
        assert!(err.to_string().to_lowercase().contains("recency"));
    }

    #[test]
    fn temporal_auto_recency_mrr_match_or_beat_primary_passes() {
        let report = mock_report_with_recency_mrr_auto_vs_primary(0.60, 0.60);
        assert!(enforce_temporal_gates(&report).is_ok());
        let report2 = mock_report_with_recency_mrr_auto_vs_primary(0.60, 0.65);
        assert!(enforce_temporal_gates(&report2).is_ok());
    }

    #[test]
    fn oracle_recency_mrr_below_auto_fails_gate() {
        let report = mock_report_with_oracle_and_auto(0.70, 0.65);
        let err = enforce_temporal_gates(&report).unwrap_err();
        assert!(err.to_string().to_lowercase().contains("oracle"));
    }

    #[test]
    fn oracle_recency_mrr_at_or_above_auto_passes() {
        let report = mock_report_with_oracle_and_auto(0.60, 0.60);
        assert!(enforce_temporal_gates(&report).is_ok());
        let report2 = mock_report_with_oracle_and_auto(0.60, 0.70);
        assert!(enforce_temporal_gates(&report2).is_ok());
    }

    #[test]
    fn missing_variants_skip_gates_without_error() {
        // A report that only has Primary (no TemporalAuto, no TemporalOracle)
        // must not fail the gate — gates only fire when the variant exists.
        let mut pb = BTreeMap::new();
        pb.insert("historical".into(), bucket_metrics(0.9, 0.8));
        pb.insert("neutral".into(), bucket_metrics(0.9, 0.8));
        pb.insert("recency_seeking".into(), bucket_metrics(0.9, 0.8));
        let report = mk_report_v2(vec![variant_report(ConfigVariant::Primary, pb)]);
        assert!(enforce_temporal_gates(&report).is_ok());
    }
}

#[cfg(test)]
mod bucket_diff_tests {
    use super::*;
    use crate::buckets::BucketMetrics;
    use crate::matrix::{LatencyPercentiles, Percentiles, VariantReport};
    use std::collections::BTreeMap;

    fn zero_pct() -> LatencyPercentiles {
        LatencyPercentiles {
            total: Percentiles {
                p50_us: 0,
                p95_us: 0,
                p99_us: 0,
            },
            embed: Percentiles {
                p50_us: 0,
                p95_us: 0,
                p99_us: 0,
            },
            bm25: Percentiles {
                p50_us: 0,
                p95_us: 0,
                p99_us: 0,
            },
            hnsw: Percentiles {
                p50_us: 0,
                p95_us: 0,
                p99_us: 0,
            },
            rerank: Percentiles {
                p50_us: 0,
                p95_us: 0,
                p99_us: 0,
            },
            fuse: Percentiles {
                p50_us: 0,
                p95_us: 0,
                p99_us: 0,
            },
        }
    }

    fn mk_variant_with_bucket(hit5_overall: f64, bucket_hit5: f64) -> VariantReport {
        let mut buckets: BTreeMap<String, BTreeMap<String, BucketMetrics>> = BTreeMap::new();
        buckets.entry("style".into()).or_default().insert(
            "identifier".into(),
            BucketMetrics {
                hit_at_1: 0.0,
                hit_at_5: bucket_hit5,
                hit_at_10: bucket_hit5,
                mrr_at_10: bucket_hit5,
                n: 10,
            },
        );
        VariantReport {
            variant: ConfigVariant::Primary,
            hit_at_1: 0.0,
            hit_at_5: hit5_overall,
            hit_at_10: hit5_overall,
            mrr_at_10: hit5_overall,
            latency: zero_pct(),
            per_question: vec![],
            buckets,
        }
    }

    fn mk_baseline_with_bucket(overall_hit5: f64, bucket_hit5: f64) -> Baseline {
        let mut buckets: BTreeMap<String, BTreeMap<String, BucketMetrics>> = BTreeMap::new();
        buckets.entry("style".into()).or_default().insert(
            "identifier".into(),
            BucketMetrics {
                hit_at_1: 0.0,
                hit_at_5: bucket_hit5,
                hit_at_10: bucket_hit5,
                mrr_at_10: bucket_hit5,
                n: 10,
            },
        );
        Baseline {
            schema_version: 2,
            git_rev: "x".into(),
            captured_at: "now".into(),
            runs: vec![VariantBaseline {
                variant: ConfigVariant::Primary,
                hit_at_5: overall_hit5,
                mrr_at_10: overall_hit5,
                buckets,
            }],
            per_bucket_slack: Some(0.05),
        }
    }

    fn mk_report_v2(runs: Vec<VariantReport>) -> MatrixReport {
        MatrixReport {
            schema_version: 2,
            git_rev: "y".into(),
            captured_at: "later".into(),
            runs,
            rerank_delta: 0.0,
            contextual_delta: 0.0,
            hybrid_delta: 0.0,
            summary: Default::default(),
        }
    }

    #[test]
    fn per_bucket_regression_detected_when_over_slack() {
        // bucket dropped 10pp (0.9 → 0.8), overall flat.
        let baseline = mk_baseline_with_bucket(0.9, 0.9);
        let report = mk_report_v2(vec![mk_variant_with_bucket(0.9, 0.8)]);
        let diff = diff(&report, &baseline).unwrap();
        assert!(diff.has_regressions());
        assert!(
            diff.regressions
                .iter()
                .any(|r| r.metric.contains("style.identifier")),
            "expected per-bucket regression, got {:?}",
            diff.regressions
        );
    }

    #[test]
    fn per_bucket_within_slack_passes() {
        // bucket dropped 4pp (0.9 → 0.86), slack is 0.05.
        let baseline = mk_baseline_with_bucket(0.9, 0.9);
        let report = mk_report_v2(vec![mk_variant_with_bucket(0.9, 0.86)]);
        let diff = diff(&report, &baseline).unwrap();
        assert!(
            !diff
                .regressions
                .iter()
                .any(|r| r.metric.contains("style.identifier")),
            "should not flag bucket regression within slack, got {:?}",
            diff.regressions
        );
    }

    #[test]
    fn per_bucket_slack_defaults_to_overall_slack_when_unset() {
        let mut baseline = mk_baseline_with_bucket(0.9, 0.9);
        baseline.per_bucket_slack = None;
        // bucket dropped 3pp (0.9 → 0.87); DEFAULT_SLACK is 0.02 → should regress.
        let report = mk_report_v2(vec![mk_variant_with_bucket(0.9, 0.87)]);
        let diff = diff(&report, &baseline).unwrap();
        assert!(
            diff.regressions
                .iter()
                .any(|r| r.metric.contains("style.identifier")),
            "expected per-bucket regression at default slack, got {:?}",
            diff.regressions
        );
    }
}
