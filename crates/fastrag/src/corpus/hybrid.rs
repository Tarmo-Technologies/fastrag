//! Hybrid retrieval (BM25 + dense RRF) with optional post-fusion temporal decay.
//!
//! Called from `query_corpus_with_filter_opts` when `QueryOpts::hybrid.enabled`
//! is set. Keeps the pure-function pieces (`decay_factor`, `apply_decay`)
//! separate from the I/O-bound `query_hybrid` so they can be unit-tested in
//! isolation.

#![allow(unused_imports)]

use std::time::Duration;

use chrono::{DateTime, NaiveDate, Utc};

use super::CorpusError;
use fastrag_index::fusion::{ScoredId, rrf_fuse};

#[derive(Debug, Clone)]
pub struct HybridOpts {
    pub enabled: bool,
    pub rrf_k: u32,
    pub overfetch_factor: usize,
    pub temporal: Option<TemporalOpts>,
}

impl Default for HybridOpts {
    fn default() -> Self {
        Self {
            enabled: false,
            rrf_k: 60,
            overfetch_factor: 4,
            temporal: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct TemporalOpts {
    pub date_field: String,
    pub halflife: Duration,
    pub weight_floor: f32,
    pub dateless_prior: f32,
    pub blend: BlendMode,
    pub now: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlendMode {
    Multiplicative,
    Additive,
}

/// Multiplicative decay factor in `[weight_floor, 1.0]`, or `dateless_prior`
/// when `age_days` is `None`. `halflife_days` must be > 0.
///
/// ```text
/// factor = alpha + (1 - alpha) * exp(-ln(2) * age_days / halflife)
/// ```
pub fn decay_factor(
    age_days: Option<f32>,
    halflife_days: f32,
    alpha: f32,
    dateless_prior: f32,
    _blend: BlendMode,
) -> f32 {
    match age_days {
        None => dateless_prior,
        Some(a) => {
            let a = a.max(0.0);
            let ln2: f32 = std::f32::consts::LN_2;
            alpha + (1.0 - alpha) * (-ln2 * a / halflife_days).exp()
        }
    }
}

#[cfg(test)]
mod decay_factor_tests {
    use super::*;

    #[test]
    fn age_zero_returns_one() {
        let f = decay_factor(Some(0.0), 30.0, 0.3, 0.5, BlendMode::Multiplicative);
        assert!((f - 1.0).abs() < 1e-6, "got {f}");
    }

    #[test]
    fn age_equal_halflife_returns_midpoint() {
        // alpha + (1-alpha)*0.5 = 0.3 + 0.35 = 0.65
        let f = decay_factor(Some(30.0), 30.0, 0.3, 0.5, BlendMode::Multiplicative);
        assert!((f - 0.65).abs() < 1e-6, "got {f}");
    }

    #[test]
    fn very_old_approaches_alpha_floor() {
        let f = decay_factor(Some(10_000.0), 30.0, 0.3, 0.5, BlendMode::Multiplicative);
        assert!(f > 0.299 && f < 0.301, "got {f}");
    }

    #[test]
    fn alpha_one_disables_decay() {
        let f = decay_factor(Some(9999.0), 30.0, 1.0, 0.5, BlendMode::Multiplicative);
        assert!((f - 1.0).abs() < 1e-6, "got {f}");
    }

    #[test]
    fn dateless_returns_prior() {
        let f = decay_factor(None, 30.0, 0.3, 0.5, BlendMode::Multiplicative);
        assert!((f - 0.5).abs() < 1e-6, "got {f}");
    }

    #[test]
    fn dateless_prior_independent_of_halflife() {
        let a = decay_factor(None, 30.0, 0.3, 0.42, BlendMode::Multiplicative);
        let b = decay_factor(None, 9000.0, 0.3, 0.42, BlendMode::Multiplicative);
        assert_eq!(a, b);
    }

    #[test]
    fn negative_age_clamps_to_one() {
        // Future-dated docs (negative age) treated as "today".
        let f = decay_factor(Some(-5.0), 30.0, 0.3, 0.5, BlendMode::Multiplicative);
        assert!((f - 1.0).abs() < 1e-6, "got {f}");
    }
}

/// Apply decay to every `ScoredId`. `dates` must be the same length as `fused`
/// (index-aligned). `None` entries get the dateless prior.
///
/// Returns a new vector sorted by descending final score.
pub fn apply_decay(
    fused: &[ScoredId],
    dates: &[Option<NaiveDate>],
    opts: &TemporalOpts,
) -> Vec<ScoredId> {
    assert_eq!(
        fused.len(),
        dates.len(),
        "apply_decay: fused and dates must be index-aligned"
    );
    if fused.is_empty() {
        return Vec::new();
    }

    let halflife_days = (opts.halflife.as_secs_f32() / 86_400.0).max(f32::EPSILON);
    let now_date = opts.now.date_naive();
    let alpha = opts.weight_floor;

    let mut out: Vec<ScoredId> = match opts.blend {
        BlendMode::Multiplicative => fused
            .iter()
            .zip(dates.iter())
            .map(|(hit, date)| {
                let age = date.map(|d| (now_date - d).num_days() as f32);
                let factor =
                    decay_factor(age, halflife_days, alpha, opts.dateless_prior, opts.blend);
                ScoredId {
                    id: hit.id,
                    score: hit.score * factor,
                }
            })
            .collect(),
        BlendMode::Additive => {
            // Min-max normalize RRF scores within the candidate set.
            // When all candidates have identical RRF score, every normalized
            // score is 0.5 (neutral) so the decay term decides ordering.
            let max = fused
                .iter()
                .map(|s| s.score)
                .fold(f32::NEG_INFINITY, f32::max);
            let min = fused.iter().map(|s| s.score).fold(f32::INFINITY, f32::min);
            let span = (max - min).max(f32::EPSILON);
            let normalize = |s: f32| -> f32 {
                if (max - min).abs() < f32::EPSILON {
                    0.5
                } else {
                    (s - min) / span
                }
            };
            let ln2: f32 = std::f32::consts::LN_2;
            fused
                .iter()
                .zip(dates.iter())
                .map(|(hit, date)| {
                    let norm_rrf = normalize(hit.score);
                    let decay_term = match date {
                        None => opts.dateless_prior,
                        Some(d) => {
                            let age = ((now_date - *d).num_days() as f32).max(0.0);
                            (-ln2 * age / halflife_days).exp()
                        }
                    };
                    let final_score = alpha * norm_rrf + (1.0 - alpha) * decay_term;
                    ScoredId {
                        id: hit.id,
                        score: final_score,
                    }
                })
                .collect()
        }
    };

    out.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    out
}

#[cfg(test)]
mod apply_decay_tests {
    use super::*;
    use chrono::TimeZone;

    fn opts(halflife_days: u64, alpha: f32, prior: f32) -> TemporalOpts {
        TemporalOpts {
            date_field: "published_date".into(),
            halflife: Duration::from_secs(halflife_days * 86_400),
            weight_floor: alpha,
            dateless_prior: prior,
            blend: BlendMode::Multiplicative,
            now: Utc.with_ymd_and_hms(2026, 4, 14, 0, 0, 0).unwrap(),
        }
    }

    fn ymd(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    #[test]
    fn uniform_age_preserves_order() {
        let fused = vec![
            ScoredId { id: 1, score: 0.9 },
            ScoredId { id: 2, score: 0.8 },
            ScoredId { id: 3, score: 0.7 },
        ];
        let same = Some(ymd(2026, 4, 1));
        let out = apply_decay(&fused, &[same, same, same], &opts(30, 0.3, 0.5));
        assert_eq!(out.iter().map(|s| s.id).collect::<Vec<_>>(), vec![1, 2, 3]);
    }

    #[test]
    fn fresh_outranks_equally_relevant_stale() {
        // Both have rrf_score 0.8; one is today, one is 10 years old.
        let fused = vec![
            ScoredId { id: 1, score: 0.8 }, // stale
            ScoredId { id: 2, score: 0.8 }, // fresh
        ];
        let dates = vec![Some(ymd(2016, 4, 14)), Some(ymd(2026, 4, 14))];
        let out = apply_decay(&fused, &dates, &opts(30, 0.3, 0.5));
        assert_eq!(out[0].id, 2, "fresh must rank first; got {out:?}");
        assert_eq!(out[1].id, 1);
    }

    #[test]
    fn dateless_interleaves_at_neutral_prior() {
        // halflife=30d, alpha=0.3, prior=0.5.
        // id=1 fresh -> factor=1.0 -> 1.0
        // id=2 dateless -> factor=0.5 -> 0.5
        // id=3 very stale (5y) -> factor→0.3 -> 0.3
        let fused = vec![
            ScoredId { id: 1, score: 1.0 },
            ScoredId { id: 2, score: 1.0 },
            ScoredId { id: 3, score: 1.0 },
        ];
        let dates = vec![Some(ymd(2026, 4, 14)), None, Some(ymd(2021, 4, 14))];
        let out = apply_decay(&fused, &dates, &opts(30, 0.3, 0.5));
        assert_eq!(out.iter().map(|s| s.id).collect::<Vec<_>>(), vec![1, 2, 3]);
    }

    #[test]
    fn empty_input_returns_empty() {
        let out = apply_decay(&[], &[], &opts(30, 0.3, 0.5));
        assert!(out.is_empty());
    }

    fn opts_additive(halflife_days: u64, alpha: f32, prior: f32) -> TemporalOpts {
        let mut o = opts(halflife_days, alpha, prior);
        o.blend = BlendMode::Additive;
        o
    }

    #[test]
    fn additive_fresh_beats_stale_with_equal_relevance() {
        // All rrf identical → normalized to 0.5. Decay term dominates.
        let fused = vec![
            ScoredId { id: 1, score: 0.8 }, // stale
            ScoredId { id: 2, score: 0.8 }, // fresh
        ];
        let dates = vec![Some(ymd(2016, 4, 14)), Some(ymd(2026, 4, 14))];
        let out = apply_decay(&fused, &dates, &opts_additive(30, 0.3, 0.5));
        assert_eq!(out[0].id, 2, "fresh must rank first in additive mode");
        assert_eq!(out[1].id, 1);
    }

    #[test]
    fn additive_high_relevance_can_outrank_fresh_but_less_relevant() {
        // alpha=0.7 — semantic weight heavy. Stale-but-relevant beats fresh-but-weak.
        // id=1: rrf=1.0 (norm=1.0), stale → final = 0.7*1.0 + 0.3*~0 = 0.70
        // id=2: rrf=0.1 (norm=0.0), fresh → final = 0.7*0.0 + 0.3*1.0 = 0.30
        let fused = vec![
            ScoredId { id: 1, score: 1.0 },
            ScoredId { id: 2, score: 0.1 },
        ];
        let dates = vec![Some(ymd(2016, 4, 14)), Some(ymd(2026, 4, 14))];
        let out = apply_decay(&fused, &dates, &opts_additive(30, 0.7, 0.5));
        assert_eq!(
            out[0].id, 1,
            "high-relevance stale beats low-relevance fresh under alpha=0.7"
        );
    }

    #[test]
    fn additive_dateless_uses_prior_as_decay_term() {
        // rrf all equal → normalized to 0.5.
        // id=1 fresh: final = 0.5*0.5 + 0.5*1.0 = 0.75
        // id=2 dateless (prior=0.5): final = 0.5*0.5 + 0.5*0.5 = 0.50
        // id=3 stale (~decay≈0): final = 0.5*0.5 + 0.5*~0 = 0.25
        let fused = vec![
            ScoredId { id: 1, score: 1.0 },
            ScoredId { id: 2, score: 1.0 },
            ScoredId { id: 3, score: 1.0 },
        ];
        let dates = vec![Some(ymd(2026, 4, 14)), None, Some(ymd(2016, 4, 14))];
        let out = apply_decay(&fused, &dates, &opts_additive(30, 0.5, 0.5));
        assert_eq!(out.iter().map(|s| s.id).collect::<Vec<_>>(), vec![1, 2, 3]);
    }
}

/// Extract a `NaiveDate` for one row of metadata by locating the named field.
/// Returns `None` when the field is absent or the value isn't a `Date`.
pub fn extract_date(
    fields: &[(String, fastrag_store::schema::TypedValue)],
    date_field: &str,
) -> Option<NaiveDate> {
    fields.iter().find_map(|(k, v)| {
        if k == date_field {
            match v {
                fastrag_store::schema::TypedValue::Date(d) => Some(*d),
                _ => None,
            }
        } else {
            None
        }
    })
}

#[cfg(test)]
mod extract_date_tests {
    use super::*;
    use fastrag_store::schema::TypedValue;

    fn field(name: &str, v: TypedValue) -> (String, TypedValue) {
        (name.to_string(), v)
    }

    #[test]
    fn returns_date_when_field_present() {
        let rows = vec![
            field("other", TypedValue::String("x".into())),
            field(
                "published_date",
                TypedValue::Date(NaiveDate::from_ymd_opt(2024, 6, 1).unwrap()),
            ),
        ];
        let d = extract_date(&rows, "published_date");
        assert_eq!(d, NaiveDate::from_ymd_opt(2024, 6, 1));
    }

    #[test]
    fn returns_none_when_field_missing() {
        let rows = vec![field("other", TypedValue::String("x".into()))];
        assert_eq!(extract_date(&rows, "published_date"), None);
    }

    #[test]
    fn returns_none_when_field_wrong_type() {
        let rows = vec![field(
            "published_date",
            TypedValue::String("2024-06-01".into()),
        )];
        assert_eq!(extract_date(&rows, "published_date"), None);
    }
}
