//! Real CorpusDriver implementation backed by fastrag::corpus query_corpus_* variants.

use std::path::{Path, PathBuf};

use fastrag::corpus::LatencyBreakdown;
use fastrag_embed::DynEmbedderTrait;
use fastrag_rerank::Reranker;

use crate::error::EvalError;
use crate::matrix::{ConfigVariant, CorpusDriver};

pub struct RealCorpusDriver<'a> {
    pub ctx_corpus: PathBuf,
    pub raw_corpus: PathBuf,
    pub embedder: &'a dyn DynEmbedderTrait,
    pub reranker: &'a dyn Reranker,
}

impl<'a> CorpusDriver for RealCorpusDriver<'a> {
    fn query(
        &self,
        variant: ConfigVariant,
        question: &str,
        top_k: usize,
        breakdown: &mut LatencyBreakdown,
    ) -> Result<Vec<String>, EvalError> {
        let corpus: &Path = match variant {
            ConfigVariant::NoContextual => &self.raw_corpus,
            _ => &self.ctx_corpus,
        };
        let over_fetch = top_k * 3;
        let filter = std::collections::BTreeMap::new();

        let hits = match variant {
            ConfigVariant::Primary | ConfigVariant::NoContextual => {
                fastrag::corpus::query_corpus_hybrid_reranked(
                    corpus,
                    question,
                    top_k,
                    over_fetch,
                    self.embedder,
                    self.reranker,
                    &filter,
                    breakdown,
                )
                .map_err(|e| EvalError::Runner(format!("{e}")))?
            }
            ConfigVariant::NoRerank => fastrag::corpus::query_corpus_hybrid(
                corpus,
                question,
                top_k,
                self.embedder,
                &filter,
                breakdown,
            )
            .map_err(|e| EvalError::Runner(format!("{e}")))?,
            ConfigVariant::DenseOnly => fastrag::corpus::query_corpus_reranked(
                corpus,
                question,
                top_k,
                over_fetch,
                self.embedder,
                self.reranker,
                &filter,
                breakdown,
            )
            .map_err(|e| EvalError::Runner(format!("{e}")))?,
        };

        Ok(hits.into_iter().map(|h| h.entry.chunk_text).collect())
    }
}
