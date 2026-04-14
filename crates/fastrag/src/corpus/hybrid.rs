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
