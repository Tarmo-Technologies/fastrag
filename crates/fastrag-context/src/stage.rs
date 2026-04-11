//! Pipeline stage that runs the contextualizer over a slice of chunks and
//! writes results to the cache.
//!
//! Consumed by `fastrag::corpus::index_path_with_metadata` between chunking
//! and the dual-write ingest. In-place mutation matches the rest of the
//! pipeline so callers don't re-allocate chunk vectors.

use crate::CTX_VERSION;
use crate::ContextError;
use crate::cache::{CacheKey, CacheStatus, ContextCache};
use crate::contextualizer::Contextualizer;
use fastrag_core::Chunk;

/// Outcome counters for a single stage invocation. Callers aggregate across
/// documents to produce the `Contextualized: N ok / M fallback (X%)` summary
/// line the CLI prints after ingest.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct StageStats {
    pub ok: usize,
    pub failed: usize,
}

impl StageStats {
    pub fn total(&self) -> usize {
        self.ok + self.failed
    }
}

/// Transforms a slice of chunks in place: sets `contextualized_text` to a
/// `"{context}\n\n{raw}"` string on success, leaves it `None` on failure
/// (non-strict) or returns `Err` (strict).
///
/// Behaviors:
/// - Cache hit with `status='ok'`: reuse cached `context_text`.
/// - Cache hit with `status='failed'`: treat as fallback without re-calling
///   the contextualizer; strict mode surfaces the cached error.
/// - Cache miss: call the contextualizer, store the result (ok or failed).
///
/// Every row carries the raw chunk text and doc title so that a
/// `--retry-failed` pass can run against the SQLite file alone.
pub fn run_contextualize_stage(
    contextualizer: &dyn Contextualizer,
    cache: &mut ContextCache,
    doc_title: &str,
    chunks: &mut [Chunk],
    strict: bool,
) -> Result<StageStats, ContextError> {
    let mut stats = StageStats::default();

    for chunk in chunks.iter_mut() {
        let chunk_hash: [u8; 32] = *blake3::hash(chunk.text.as_bytes()).as_bytes();
        let key = CacheKey {
            chunk_hash,
            ctx_version: CTX_VERSION,
            model_id: contextualizer.model_id(),
            prompt_version: contextualizer.prompt_version(),
        };

        // Cache lookup
        if let Some(row) = cache.get(key)? {
            match row.status {
                CacheStatus::Ok => {
                    if let Some(ctx_text) = row.context_text {
                        chunk.contextualized_text = Some(prefix(&ctx_text, &chunk.text));
                        stats.ok += 1;
                        continue;
                    }
                    // `ok` row with null context_text is corrupt — treat as failure.
                    stats.failed += 1;
                    continue;
                }
                CacheStatus::Failed => {
                    if strict {
                        return Err(ContextError::Template(
                            row.error
                                .unwrap_or_else(|| "cached contextualization failure".to_string()),
                        ));
                    }
                    stats.failed += 1;
                    continue;
                }
            }
        }

        // Cache miss — call the contextualizer.
        match contextualizer.contextualize(doc_title, &chunk.text) {
            Ok(ctx_text) => {
                cache.put_ok(key, &chunk.text, doc_title, &ctx_text)?;
                chunk.contextualized_text = Some(prefix(&ctx_text, &chunk.text));
                stats.ok += 1;
            }
            Err(e) => {
                if strict {
                    return Err(e);
                }
                let error_str = e.to_string();
                cache.mark_failed(key, &chunk.text, doc_title, &error_str)?;
                stats.failed += 1;
            }
        }
    }
    Ok(stats)
}

/// Canonical form of a contextualized chunk: the generated context, then a
/// blank line, then the raw chunk text. The blank line is intentional so the
/// embedder / BM25 tokenizer treats context and body as separate paragraphs.
fn prefix(context_text: &str, raw: &str) -> String {
    format!("{context_text}\n\n{raw}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contextualizer::ContextualizerMeta;
    use crate::test_utils::MockContextualizer;

    fn mk_chunk(text: &str, index: usize) -> Chunk {
        Chunk {
            elements: vec![],
            text: text.to_string(),
            char_count: text.chars().count(),
            section: None,
            index,
            contextualized_text: None,
        }
    }

    fn open_cache() -> (ContextCache, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let cache = ContextCache::open(&dir.path().join("ctx.sqlite")).unwrap();
        (cache, dir)
    }

    #[test]
    fn every_third_call_fails_produces_two_thirds_success() {
        let (mut cache, _dir) = open_cache();
        let ctx = MockContextualizer::fail_every(3);
        let mut chunks = (0..9)
            .map(|i| mk_chunk(&format!("chunk-{i}"), i))
            .collect::<Vec<_>>();

        let stats = run_contextualize_stage(&ctx, &mut cache, "Doc", &mut chunks, false).unwrap();

        assert_eq!(stats.total(), 9);
        assert_eq!(stats.failed, 3, "every 3rd of 9 should fail");
        assert_eq!(stats.ok, 6);

        let with_ctx = chunks
            .iter()
            .filter(|c| c.contextualized_text.is_some())
            .count();
        assert_eq!(with_ctx, 6);

        // Cache reflects the split.
        let failed_rows: Vec<_> = cache.iter_failed().unwrap().collect();
        assert_eq!(failed_rows.len(), 3);
        let (ok, failed) = cache.row_count().unwrap();
        assert_eq!(ok, 6);
        assert_eq!(failed, 3);
    }

    #[test]
    fn strict_mode_aborts_on_first_failure() {
        let (mut cache, _dir) = open_cache();
        let ctx = MockContextualizer::fail_every(2);
        let mut chunks = (0..9)
            .map(|i| mk_chunk(&format!("chunk-{i}"), i))
            .collect::<Vec<_>>();

        let err = run_contextualize_stage(&ctx, &mut cache, "Doc", &mut chunks, true);
        assert!(err.is_err(), "strict mode should surface an error");

        // First chunk succeeded, second failed → abort. No third-chunk row.
        let (ok, _failed) = cache.row_count().unwrap();
        assert_eq!(ok, 1, "only the first succeeded call should persist");
    }

    #[test]
    fn cache_hit_ok_skips_contextualizer() {
        let (mut cache, _dir) = open_cache();

        // Pre-populate the cache with a canned ok row keyed under the
        // MockContextualizer::panicking identity.
        let panicking = MockContextualizer::panicking();
        let chunk_text = "prepopulated-chunk";
        let hash_bytes: [u8; 32] = *blake3::hash(chunk_text.as_bytes()).as_bytes();
        cache
            .put_ok(
                CacheKey {
                    chunk_hash: hash_bytes,
                    ctx_version: CTX_VERSION,
                    model_id: panicking.model_id(),
                    prompt_version: panicking.prompt_version(),
                },
                chunk_text,
                "Doc",
                "PREFILLED CTX",
            )
            .unwrap();

        let mut chunks = vec![mk_chunk(chunk_text, 0)];
        let stats =
            run_contextualize_stage(&panicking, &mut cache, "Doc", &mut chunks, false).unwrap();
        assert_eq!(stats.ok, 1);
        assert_eq!(stats.failed, 0);
        assert_eq!(
            chunks[0].contextualized_text.as_deref(),
            Some("PREFILLED CTX\n\nprepopulated-chunk"),
        );
    }

    #[test]
    fn cache_hit_failed_non_strict_is_fallback_and_does_not_call_contextualizer() {
        let (mut cache, _dir) = open_cache();

        let panicking = MockContextualizer::panicking();
        let chunk_text = "previously-failed-chunk";
        let hash_bytes: [u8; 32] = *blake3::hash(chunk_text.as_bytes()).as_bytes();
        cache
            .mark_failed(
                CacheKey {
                    chunk_hash: hash_bytes,
                    ctx_version: CTX_VERSION,
                    model_id: panicking.model_id(),
                    prompt_version: panicking.prompt_version(),
                },
                chunk_text,
                "Doc",
                "prior failure",
            )
            .unwrap();

        let mut chunks = vec![mk_chunk(chunk_text, 0)];
        let stats =
            run_contextualize_stage(&panicking, &mut cache, "Doc", &mut chunks, false).unwrap();
        assert_eq!(stats.ok, 0);
        assert_eq!(stats.failed, 1);
        assert!(chunks[0].contextualized_text.is_none());
    }

    #[test]
    fn cache_hit_failed_strict_surfaces_error() {
        let (mut cache, _dir) = open_cache();

        let panicking = MockContextualizer::panicking();
        let chunk_text = "previously-failed-chunk";
        let hash_bytes: [u8; 32] = *blake3::hash(chunk_text.as_bytes()).as_bytes();
        cache
            .mark_failed(
                CacheKey {
                    chunk_hash: hash_bytes,
                    ctx_version: CTX_VERSION,
                    model_id: panicking.model_id(),
                    prompt_version: panicking.prompt_version(),
                },
                chunk_text,
                "Doc",
                "prior failure body",
            )
            .unwrap();

        let mut chunks = vec![mk_chunk(chunk_text, 0)];
        let result = run_contextualize_stage(&panicking, &mut cache, "Doc", &mut chunks, true);
        assert!(result.is_err(), "strict mode should surface cached failure");
    }

    #[test]
    fn successful_runs_persist_raw_text_and_doc_title_for_retry() {
        let (mut cache, _dir) = open_cache();
        let ctx = MockContextualizer::always_ok();
        let mut chunks = vec![mk_chunk("body-a", 0), mk_chunk("body-b", 1)];

        let stats =
            run_contextualize_stage(&ctx, &mut cache, "Doc Title", &mut chunks, false).unwrap();
        assert_eq!(stats.ok, 2);
        assert_eq!(stats.failed, 0);

        // Retrieve a row and verify both raw_text and doc_title landed in the
        // cache — critical for --retry-failed running against the SQLite file
        // alone.
        let key = CacheKey {
            chunk_hash: *blake3::hash(b"body-a").as_bytes(),
            ctx_version: CTX_VERSION,
            model_id: ctx.model_id(),
            prompt_version: ctx.prompt_version(),
        };
        let row = cache.get(key).unwrap().unwrap();
        assert_eq!(row.raw_text, "body-a");
        assert_eq!(row.doc_title, "Doc Title");
    }
}
