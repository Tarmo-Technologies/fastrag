use std::path::PathBuf;

use fastrag_eval::{EvalDataset, Runner};

use crate::args::{EvalChunkingArg, EvalEmbedderArg};
use fastrag_embed::Embedder;

pub async fn run_eval(
    dataset: PathBuf,
    report: PathBuf,
    embedder: EvalEmbedderArg,
    top_k: usize,
    chunking: EvalChunkingArg,
    chunk_size: usize,
) -> Result<(), fastrag_eval::EvalError> {
    let dataset = EvalDataset::load(&dataset)?;
    let embedder: Box<dyn Embedder> = match embedder {
        EvalEmbedderArg::Mock => Box::new(fastrag_embed::test_utils::MockEmbedder),
        EvalEmbedderArg::BgeSmall => Box::new(fastrag_embed::BgeSmallEmbedder::from_hf_hub()?),
    };
    let chunking = match chunking {
        EvalChunkingArg::Basic => fastrag::ChunkingStrategy::Basic {
            max_characters: chunk_size,
            overlap: 0,
        },
        EvalChunkingArg::ByTitle => fastrag::ChunkingStrategy::ByTitle {
            max_characters: chunk_size,
            overlap: 0,
        },
    };

    let report_value = Runner::new(embedder.as_ref(), chunking, &dataset, top_k).run()?;
    print_report(&report_value);
    report_value.write_json(&report)?;
    println!("Wrote report JSON to {}", report.display());
    Ok(())
}

fn print_report(report: &fastrag_eval::EvalReport) {
    println!();
    println!("| Field | Value |");
    println!("| --- | ---: |");
    println!("| dataset | {} |", report.dataset);
    println!("| embedder | {} |", report.embedder);
    println!("| chunking | {} |", report.chunking);
    println!("| build_time_ms | {} |", report.build_time_ms);
    println!("| run_at_unix | {} |", report.run_at_unix);
    println!("| peak_rss_bytes | {} |", report.memory.peak_rss_bytes);
    println!(
        "| current_rss_bytes | {} |",
        report.memory.current_rss_bytes
    );
    println!("| p50_ms | {:.6} |", report.latency.p50_ms);
    println!("| p95_ms | {:.6} |", report.latency.p95_ms);
    println!("| p99_ms | {:.6} |", report.latency.p99_ms);
    println!("| mean_ms | {:.6} |", report.latency.mean_ms);
    println!("| count | {} |", report.latency.count);

    let mut metrics = report.metrics.iter().collect::<Vec<_>>();
    metrics.sort_by(|a, b| a.0.cmp(b.0));
    for (name, value) in metrics {
        println!("| {} | {:.6} |", name, value);
    }
}
