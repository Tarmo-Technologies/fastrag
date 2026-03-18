pub mod chunking;
pub mod document;
pub mod error;
pub mod format;
#[cfg(feature = "language-detection")]
pub mod language;
pub mod output;

pub use chunking::{
    Chunk, ChunkingStrategy, ContextInjection, cosine_similarity, default_embedder,
    default_separators,
};
pub use document::{BoundingBox, Document, Element, ElementKind, Metadata, is_caption_text};
pub use error::FastRagError;
pub use format::{FileFormat, SourceInfo};
pub use output::OutputFormat;

/// Every format parser implements this trait.
pub trait Parser: Send + Sync {
    /// Returns the file formats this parser can handle.
    fn supported_formats(&self) -> &[FileFormat];

    /// Parse raw bytes into a structured Document.
    fn parse(&self, input: &[u8], source: &SourceInfo) -> Result<Document, FastRagError>;

    /// Stream elements incrementally instead of building a complete Document.
    ///
    /// The default implementation calls `parse()` then yields elements one by one.
    /// Parsers can override this for true incremental processing (e.g., page-by-page).
    ///
    /// Note: streaming mode skips `build_hierarchy()` and `associate_captions()`.
    fn parse_stream<'a>(
        &'a self,
        input: &'a [u8],
        source: &'a SourceInfo,
    ) -> Result<Box<dyn Iterator<Item = Result<Element, FastRagError>> + 'a>, FastRagError> {
        let doc = self.parse(input, source)?;
        Ok(Box::new(doc.elements.into_iter().map(Ok)))
    }
}
