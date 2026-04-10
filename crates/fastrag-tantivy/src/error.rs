use thiserror::Error;

#[derive(Debug, Error)]
pub enum TantivyIndexError {
    #[error("tantivy: {0}")]
    Tantivy(#[from] tantivy::TantivyError),
    #[error("I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("schema field missing: {0}")]
    SchemaFieldMissing(String),
    #[error("query parse: {0}")]
    QueryParse(#[from] tantivy::query::QueryParserError),
}
