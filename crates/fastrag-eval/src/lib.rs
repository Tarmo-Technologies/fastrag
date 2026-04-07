mod dataset;
mod error;
mod metrics;
mod report;
mod runner;

pub use dataset::{EvalDataset, EvalDocument, EvalQuery, Qrel};
pub use error::{EvalError, EvalResult};
pub use metrics::{hit_rate_at_k, mrr_at_k, ndcg_at_k, recall_at_k};
pub use report::{EvalReport, LatencyStats, MemoryStats};
pub use runner::{Runner, index_documents};
