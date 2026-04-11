//! `--config-matrix` dispatch: wire CLI args → RealCorpusDriver → run_matrix.

use std::path::PathBuf;

use fastrag_cli::args::RerankerKindArg;
use fastrag_cli::embed_loader::{self, EmbedderOptions};
use fastrag_cli::rerank_loader;
use fastrag_eval::{
    EvalError,
    baseline::{diff, load_baseline},
    gold_set,
    matrix::run_matrix,
    matrix_real::RealCorpusDriver,
    write_matrix_report,
};

pub fn run_config_matrix(
    gold_set_path: Option<PathBuf>,
    corpus: Option<PathBuf>,
    corpus_no_contextual: Option<PathBuf>,
    report_path: PathBuf,
    top_k: usize,
    baseline_path: Option<PathBuf>,
) -> Result<(), EvalError> {
    let gs_path = gold_set_path.ok_or(EvalError::MatrixRequiresGoldSet)?;
    let ctx_corpus = corpus
        .ok_or_else(|| EvalError::GoldSetInvalid("--config-matrix requires --corpus".into()))?;
    let raw_corpus = corpus_no_contextual.ok_or(EvalError::MatrixMissingRawCorpus)?;

    let gs = gold_set::load(&gs_path)?;

    // Auto-detect embedder from corpus manifest.
    let opts = EmbedderOptions {
        kind: None,
        model_path: None,
        openai_model: "text-embedding-3-small".into(),
        openai_base_url: "https://api.openai.com/v1".into(),
        ollama_model: "nomic-embed-text".into(),
        ollama_url: "http://localhost:11434".into(),
    };
    let embedder = embed_loader::load_for_read(&ctx_corpus, &opts)
        .map_err(|e| EvalError::Runner(format!("loading embedder: {e}")))?;

    // Load reranker — requires `rerank` feature.
    let reranker = rerank_loader::load_reranker(RerankerKindArg::Onnx)
        .map_err(|e| EvalError::Runner(format!("loading reranker: {e}")))?;

    let driver = RealCorpusDriver {
        ctx_corpus,
        raw_corpus,
        embedder: embedder.as_ref(),
        reranker: reranker.as_ref(),
    };

    let matrix_report = run_matrix(&driver, &gs, top_k)?;

    write_matrix_report(&matrix_report, &report_path)?;

    println!("Wrote matrix report to {}", report_path.display());

    if let Some(bpath) = baseline_path {
        let baseline = load_baseline(&bpath)?;
        let bdiff = diff(&matrix_report, &baseline)?;
        eprintln!("{}", bdiff.render_report());
        if bdiff.has_regressions() {
            std::process::exit(1);
        }
    }

    Ok(())
}
