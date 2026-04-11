//! Contextualization prompt template.
//!
//! Adapted from the prompt published by Anthropic for Contextual Retrieval
//! (September 2024). We ask the model to situate a single chunk within its
//! source document and return only the contextual preface — no prose
//! scaffolding, no meta commentary.
//!
//! Bump [`PROMPT_VERSION`] whenever the prompt text below changes in a way
//! that should invalidate cached results. [`crate::CTX_VERSION`] is an
//! independent counter for cache-schema changes.

/// Version of the prompt text below. Part of the cache primary key so edits
/// here automatically invalidate prior rows.
pub const PROMPT_VERSION: u32 = 1;

/// The contextualization prompt template. Substitution tokens:
/// `{doc_title}` — the title of the source document, or an empty string if
/// the document has no known title. `{chunk}` — the raw chunk text.
///
/// Adapted from Anthropic's Contextual Retrieval blog post (2024-09). The
/// model is asked to produce ONLY the short context preface and nothing
/// else, so the response can be prepended directly to the chunk before
/// embedding and BM25 indexing.
pub const PROMPT: &str = "\
<document>
<title>{doc_title}</title>
<chunk>
{chunk}
</chunk>
</document>

Please give a short succinct context (50-100 tokens) to situate this chunk \
within the overall document above, for the purposes of improving search \
retrieval of the chunk. Answer only with the succinct context and nothing \
else.";

/// Format [`PROMPT`] with the given title and chunk.
///
/// An empty `doc_title` is valid and produces a document block with an empty
/// `<title>` element. The returned string never contains the literal tokens
/// `{doc_title}` or `{chunk}`.
pub fn format_prompt(doc_title: &str, chunk: &str) -> String {
    PROMPT
        .replace("{doc_title}", doc_title)
        .replace("{chunk}", chunk)
}

/// BLAKE3 hex digest of [`PROMPT`]. Stamped onto the corpus manifest so
/// `corpus-info` and `--retry-failed` can detect prompt text drift even when
/// [`PROMPT_VERSION`] was not bumped.
pub fn prompt_hash_hex() -> String {
    blake3::hash(PROMPT.as_bytes()).to_hex().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_with_title_substitutes_both_tokens() {
        let out = format_prompt("CVE-2024-12345", "buffer overflow in foo()");
        assert!(out.contains("<title>CVE-2024-12345</title>"));
        assert!(out.contains("buffer overflow in foo()"));
        assert!(!out.contains("{doc_title}"));
        assert!(!out.contains("{chunk}"));
    }

    #[test]
    fn format_with_empty_title_produces_empty_title_tag() {
        let out = format_prompt("", "some chunk text");
        assert!(out.contains("<title></title>"));
        assert!(out.contains("some chunk text"));
        assert!(!out.contains("{doc_title}"));
    }

    #[test]
    fn prompt_version_is_non_zero() {
        // Bumping to 0 would silently alias the "no contextualizer" sentinel
        // used in the cache key; PROMPT_VERSION must always be >= 1 for a
        // real prompt.
        const { assert!(PROMPT_VERSION >= 1) };
    }
}
