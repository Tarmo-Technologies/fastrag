//! Real CorpusDriver implementation backed by fastrag::corpus query functions.
//!
//! Pre-loads both hybrid indices (contextualized + raw) once at construction
//! time rather than re-opening them per query. This avoids repeated canary
//! verification embeds, cutting embedder HTTP calls in half and eliminating
//! stale-connection failures that occurred when the embedder sat idle during
//! long reranker calls.

use std::path::Path;

use fastrag::VectorIndex;
use fastrag::corpus::LatencyBreakdown;
use fastrag::corpus::hybrid::HybridIndex;
use fastrag_embed::{DynEmbedderTrait, QueryText};
use fastrag_rerank::Reranker;

use crate::error::EvalError;
use crate::matrix::{ConfigVariant, CorpusDriver};

pub struct RealCorpusDriver<'a> {
    ctx_index: HybridIndex,
    raw_index: HybridIndex,
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
        let ctx_index = HybridIndex::load(ctx_corpus, embedder)
            .map_err(|e| EvalError::Runner(format!("load ctx corpus: {e}")))?;
        let raw_index = HybridIndex::load(raw_corpus, embedder)
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
        question: &str,
        query_vector: &[f32],
        top_k: usize,
        breakdown: &mut LatencyBreakdown,
    ) -> Result<Vec<String>, EvalError> {
        let index = match variant {
            ConfigVariant::NoContextual => &self.raw_index,
            _ => &self.ctx_index,
        };
        let over_fetch = top_k * 3;

        let hits = match variant {
            ConfigVariant::Primary | ConfigVariant::NoContextual => {
                // Hybrid search + rerank
                let first_stage = index
                    .query_hybrid_timed(question, query_vector, over_fetch, breakdown)
                    .map_err(|e| EvalError::Runner(format!("hybrid search: {e}")))?;
                let t = std::time::Instant::now();
                let mut reranked = self
                    .reranker
                    .rerank(question, first_stage)
                    .map_err(|e| EvalError::Runner(format!("rerank: {e}")))?;
                breakdown.rerank_us = t.elapsed().as_micros() as u64;
                reranked.truncate(top_k);
                breakdown.finalize();
                reranked
            }
            ConfigVariant::NoRerank => {
                // Hybrid search only
                let result = index
                    .query_hybrid_timed(question, query_vector, top_k, breakdown)
                    .map_err(|e| EvalError::Runner(format!("hybrid search: {e}")))?;
                breakdown.finalize();
                result
            }
            ConfigVariant::DenseOnly => {
                // Dense HNSW + rerank (no BM25)
                let t_hnsw = std::time::Instant::now();
                let first_stage = index
                    .hnsw()
                    .query(query_vector, over_fetch)
                    .map_err(|e| EvalError::Runner(format!("hnsw: {e}")))?;
                breakdown.hnsw_us = t_hnsw.elapsed().as_micros() as u64;
                let t = std::time::Instant::now();
                let mut reranked = self
                    .reranker
                    .rerank(question, first_stage)
                    .map_err(|e| EvalError::Runner(format!("rerank: {e}")))?;
                breakdown.rerank_us = t.elapsed().as_micros() as u64;
                reranked.truncate(top_k);
                breakdown.finalize();
                reranked
            }
        };

        Ok(hits.into_iter().map(|h| h.entry.chunk_text).collect())
    }
}
