//! Real CorpusDriver implementation backed by fastrag HNSW query functions.
//!
//! Pre-loads both HNSW indices (contextualized + raw) once at construction
//! time rather than re-opening them per query. This avoids repeated canary
//! verification embeds, cutting embedder HTTP calls in half.

use std::path::Path;

use fastrag::VectorIndex;
use fastrag::corpus::LatencyBreakdown;
use fastrag::{HnswIndex, VectorHit};
use fastrag_embed::{DynEmbedderTrait, QueryText};
use fastrag_rerank::Reranker;

use crate::error::EvalError;
use crate::matrix::{ConfigVariant, CorpusDriver};

pub struct RealCorpusDriver<'a> {
    ctx_index: HnswIndex,
    raw_index: HnswIndex,
    pub embedder: &'a dyn DynEmbedderTrait,
    pub reranker: &'a dyn Reranker,
}

impl<'a> RealCorpusDriver<'a> {
    /// Load both corpus indices up-front. The canary verification embed runs
    /// once per corpus here, not once per query.
    pub fn load(
        ctx_corpus: &Path,
        raw_corpus: &Path,
        embedder: &'a dyn DynEmbedderTrait,
        reranker: &'a dyn Reranker,
    ) -> Result<Self, EvalError> {
        let ctx_index = HnswIndex::load(ctx_corpus, embedder)
            .map_err(|e| EvalError::Runner(format!("load ctx corpus: {e}")))?;
        let raw_index = HnswIndex::load(raw_corpus, embedder)
            .map_err(|e| EvalError::Runner(format!("load raw corpus: {e}")))?;
        Ok(Self {
            ctx_index,
            raw_index,
            embedder,
            reranker,
        })
    }
}

impl CorpusDriver for RealCorpusDriver<'_> {
    fn embed_queries(&self, questions: &[&str]) -> Result<Vec<Vec<f32>>, EvalError> {
        let qt: Vec<QueryText> = questions.iter().map(|q| QueryText::new(*q)).collect();
        self.embedder
            .embed_query_dyn(&qt)
            .map_err(|e| EvalError::Runner(format!("batch embed: {e}")))
    }

    fn query(
        &self,
        variant: ConfigVariant,
        _question: &str,
        query_vector: &[f32],
        top_k: usize,
        breakdown: &mut LatencyBreakdown,
    ) -> Result<Vec<String>, EvalError> {
        let index = match variant {
            ConfigVariant::NoContextual => &self.raw_index,
            _ => &self.ctx_index,
        };

        let t_hnsw = std::time::Instant::now();
        let hits: Vec<VectorHit> = index
            .query(query_vector, top_k)
            .map_err(|e| EvalError::Runner(format!("hnsw: {e}")))?;
        breakdown.hnsw_us = t_hnsw.elapsed().as_micros() as u64;
        breakdown.finalize();

        // VectorHit has no text — return IDs as strings.
        // TODO: Hydrate from Store once the Store query path is wired.
        Ok(hits.into_iter().map(|h| h.id.to_string()).collect())
    }
}
