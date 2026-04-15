//! Synthetic dedup precision/recall gate.
//!
//! Generates deterministic paraphrases of a fixed seed corpus; runs /similar
//! with and without the verifier; asserts the verifier does not worsen
//! precision and holds recall. Ground truth is resolved by chunk_text
//! content (a unique anchor phrase per seed), which avoids depending on the
//! `store` metadata feature here.
#![cfg(feature = "retrieval")]

use std::collections::BTreeMap;

use fastrag::ChunkingStrategy;
use fastrag::corpus::verify::{VerifyConfig, VerifyMethod};
use fastrag::corpus::{SimilarityHit, SimilarityRequest, similarity_search};
use fastrag::ingest::engine::index_jsonl;
use fastrag::ingest::jsonl::JsonlIngestConfig;
use fastrag_embed::test_utils::MockEmbedder;

/// (seed text, anchor substring found in every paraphrase of that seed).
/// The anchor is used as the ground-truth marker for precision/recall.
const SEEDS: &[(&str, &str)] = &[
    (
        "SQL injection in the login form allows authentication bypass.",
        "login form",
    ),
    (
        "Cross-site scripting in the search bar via reflected parameter.",
        "search bar",
    ),
    (
        "Insecure direct object reference exposes user profile data.",
        "direct object",
    ),
    (
        "Server-side request forgery in the webhook URL handler.",
        "webhook",
    ),
    (
        "Command injection in the file-upload renaming routine.",
        "file-upload",
    ),
    (
        "XML external entity expansion in the invoice parser.",
        "invoice",
    ),
    (
        "Path traversal in the static asset handler.",
        "static asset",
    ),
    ("Open redirect in the post-login return URL.", "post-login"),
];

/// Deterministic paraphrase: whitespace jitter, punctuation variance, case
/// flip, trivial synonym swap. Pure function — no RNG.
fn paraphrase(seed: &str, variant: u8) -> String {
    let mut s = seed.to_string();
    match variant {
        0 => s,
        1 => s.to_uppercase(),
        2 => s.replace(' ', "  "),
        3 => s.replace('.', " ."),
        _ => {
            s = s.replace("bypass", "circumvention");
            s = s.replace("exposes", "leaks");
            s
        }
    }
}

const VARIANTS: u8 = 5; // 0..=4 inclusive => 5 paraphrases per seed

fn build_benchmark() -> (tempfile::TempDir, std::path::PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let jsonl = tmp.path().join("docs.jsonl");
    let mut lines: Vec<String> = Vec::new();
    for (i, (seed, _anchor)) in SEEDS.iter().enumerate() {
        for v in 0..VARIANTS {
            let body = paraphrase(seed, v);
            lines.push(serde_json::json!({ "id": format!("s{i}_v{v}"), "body": body }).to_string());
        }
    }
    for j in 0..20 {
        lines.push(
            serde_json::json!({
                "id": format!("noise{j}"),
                "body": format!(
                    "unrelated advisory content number {j} about a different topic entirely"
                ),
            })
            .to_string(),
        );
    }
    std::fs::write(&jsonl, lines.join("\n")).unwrap();
    let corpus = tmp.path().join("corpus");
    let cfg = JsonlIngestConfig {
        text_fields: vec!["body".into()],
        id_field: "id".into(),
        metadata_fields: vec![],
        metadata_types: BTreeMap::new(),
        array_fields: vec![],
        cwe_field: None,
    };
    index_jsonl(
        &jsonl,
        &corpus,
        &ChunkingStrategy::Basic {
            max_characters: 500,
            overlap: 0,
        },
        &MockEmbedder as &dyn fastrag_embed::DynEmbedderTrait,
        &cfg,
    )
    .unwrap();
    (tmp, corpus)
}

fn precision_recall(hits: &[SimilarityHit], anchor: &str) -> (f32, f32) {
    let anchor_lc = anchor.to_lowercase();
    let tp = hits
        .iter()
        .filter(|h| h.dto.chunk_text.to_lowercase().contains(&anchor_lc))
        .count();
    let precision = if hits.is_empty() {
        1.0
    } else {
        tp as f32 / hits.len() as f32
    };
    // Total true-near-dupes per seed = VARIANTS (0..=4).
    let recall = tp as f32 / VARIANTS as f32;
    (precision, recall)
}

#[test]
fn verifier_holds_precision_and_recall_on_synthetic_paraphrases() {
    let (_t, corpus) = build_benchmark();

    let mut ann_p = 0.0f32;
    let mut ann_r = 0.0f32;
    let mut ver_p = 0.0f32;
    let mut ver_r = 0.0f32;

    for (seed, anchor) in SEEDS.iter() {
        let base = SimilarityRequest {
            text: (*seed).to_string(),
            threshold: 0.5,
            max_results: 20,
            targets: vec![("default".into(), corpus.clone())],
            filter: None,
            snippet_len: 0,
            overfetch_cap: 10_000,
            verify: None,
        };
        let ann = similarity_search(&MockEmbedder, &base).unwrap();
        let (p, r) = precision_recall(&ann.hits, anchor);
        ann_p += p;
        ann_r += r;

        let verified_req = SimilarityRequest {
            verify: Some(VerifyConfig {
                method: VerifyMethod::MinHash,
                threshold: 0.3,
            }),
            ..base
        };
        let verified = similarity_search(&MockEmbedder, &verified_req).unwrap();
        let (p, r) = precision_recall(&verified.hits, anchor);
        ver_p += p;
        ver_r += r;
    }

    let n = SEEDS.len() as f32;
    let ann_p = ann_p / n;
    let ann_r = ann_r / n;
    let ver_p = ver_p / n;
    let ver_r = ver_r / n;

    eprintln!("ANN-only   p={ann_p:.3} r={ann_r:.3}");
    eprintln!("Verified   p={ver_p:.3} r={ver_r:.3}");

    // Spec contract: verifier must not worsen precision, and must hold recall.
    // NOTE: If this gate becomes flaky under MockEmbedder's crude cosine, or
    // if verified p99 /similar latency hits 2x baseline, switch to pre-computed
    // signatures (Option B/C from the design).
    assert!(
        ver_p >= ann_p - 1e-6,
        "verifier must not worsen precision (ann={ann_p}, ver={ver_p})"
    );
    assert!(
        ver_r >= 0.85,
        "verifier must hold recall >= 0.85 (ver_r={ver_r})"
    );
}
