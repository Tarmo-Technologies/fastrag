use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

#[derive(Parser)]
#[command(name = "fastrag", about = "Fast document parser for AI/RAG pipelines")]
#[command(version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Parse one or more files
    Parse {
        /// File or directory to parse
        path: String,

        /// Output format
        #[arg(short, long, default_value = "markdown")]
        format: OutputFormatArg,

        /// Output directory (for batch mode)
        #[arg(short, long)]
        output: Option<String>,

        /// Number of parallel workers
        #[arg(short = 'j', long, default_value = "4")]
        workers: usize,

        /// Chunking strategy (none, basic, by-title, recursive)
        #[arg(long, default_value = "none")]
        chunk_strategy: ChunkStrategyArg,

        /// Maximum characters per chunk
        #[arg(long, default_value = "1000")]
        chunk_size: usize,

        /// Number of overlapping characters between consecutive chunks
        #[arg(long, default_value = "0")]
        chunk_overlap: usize,

        /// Comma-separated list of separators for recursive strategy (use \n for newline)
        #[arg(long)]
        chunk_separators: Option<String>,

        /// Similarity threshold for semantic chunking (0.0 to 1.0)
        #[arg(long)]
        similarity_threshold: Option<f32>,

        /// Percentile threshold for semantic chunking (0.0 to 100.0)
        #[arg(long)]
        percentile_threshold: Option<f32>,

        /// Context template for chunk context injection (e.g. "{document_title} > {section}\n\n{chunk_text}")
        #[arg(long)]
        context_template: Option<String>,

        /// Stream elements incrementally (JSONL output, skips hierarchy/captions)
        #[arg(long)]
        stream: bool,

        /// Detect document language
        #[arg(long)]
        detect_language: bool,
    },

    /// List supported formats
    Formats,

    /// Start MCP server for AI assistant integration
    #[cfg(feature = "mcp")]
    Serve,

    /// Index a corpus of files for semantic search
    #[cfg(feature = "retrieval")]
    Index {
        /// File or directory to index
        input: PathBuf,

        /// Corpus directory used for persistence
        #[arg(long)]
        corpus: PathBuf,

        /// Chunking strategy (none, basic, by-title, recursive, semantic)
        #[arg(long, default_value = "basic")]
        chunk_strategy: ChunkStrategyArg,

        /// Maximum characters per chunk
        #[arg(long, default_value_t = 1000)]
        chunk_size: usize,

        /// Number of overlapping characters between consecutive chunks
        #[arg(long, default_value_t = 0)]
        chunk_overlap: usize,

        /// Comma-separated list of separators for recursive strategy
        #[arg(long)]
        chunk_separators: Option<String>,

        /// Similarity threshold for semantic chunking (0.0 to 1.0)
        #[arg(long)]
        similarity_threshold: Option<f32>,

        /// Percentile threshold for semantic chunking (0.0 to 100.0)
        #[arg(long)]
        percentile_threshold: Option<f32>,

        /// Optional local model path
        #[arg(long)]
        model_path: Option<PathBuf>,
    },

    /// Query an indexed corpus
    #[cfg(feature = "retrieval")]
    Query {
        /// Search query
        query: String,

        /// Corpus directory used for persistence
        #[arg(long)]
        corpus: PathBuf,

        /// Number of results to return
        #[arg(long, default_value_t = 5)]
        top_k: usize,

        /// Output format
        #[arg(short, long, default_value = "json")]
        format: OutputFormatArg,

        /// Optional local model path
        #[arg(long)]
        model_path: Option<PathBuf>,
    },

    /// Show corpus metadata
    #[cfg(feature = "retrieval")]
    CorpusInfo {
        /// Corpus directory used for persistence
        #[arg(long)]
        corpus: PathBuf,
    },

    /// Start HTTP retrieval server
    #[cfg(feature = "retrieval")]
    ServeHttp {
        /// Corpus directory used for persistence
        #[arg(long)]
        corpus: PathBuf,

        /// Port to bind
        #[arg(long, default_value_t = 8081)]
        port: u16,

        /// Optional local model path
        #[arg(long)]
        model_path: Option<PathBuf>,
    },
}

#[derive(Clone, ValueEnum)]
pub enum OutputFormatArg {
    Markdown,
    Json,
    Jsonl,
    Text,
    Html,
}

#[derive(Clone, ValueEnum, PartialEq)]
pub enum ChunkStrategyArg {
    None,
    Basic,
    ByTitle,
    Recursive,
    Semantic,
}
