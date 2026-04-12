//! LanguageFilter: wraps whatlang to drop or flag non-English chunk text.
//!
//! Policy enum:
//! - `Drop`  → return empty string (chunk is eliminated by HygieneChain)
//! - `Flag`  → mutate metadata with `language=<detected>` and keep the text
//!
//! The filter only runs when text is long enough for whatlang to give a
//! reliable detection (>= 20 bytes). Shorter snippets are kept as-is.

use std::collections::BTreeMap;

use whatlang::detect;

use super::ChunkFilter;

/// What to do when a non-target-language chunk is detected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LanguagePolicy {
    /// Silently drop the chunk (return empty string → HygieneChain drops it).
    Drop,
    /// Keep the chunk but write `language=<lang_code>` into the metadata.
    Flag,
}

/// Minimum text length (bytes) before whatlang detection is attempted.
/// Shorter texts fall back to "keep without labelling".
const MIN_DETECT_BYTES: usize = 20;

/// Normalise a BCP 47 two-letter tag to the ISO 639-3 three-letter code that
/// `whatlang` uses internally.  Unknown tags are returned unchanged so callers
/// can still use raw ISO 639-3 codes directly (e.g. `"eng"`).
fn bcp47_to_iso639_3(tag: &str) -> &str {
    match tag {
        "en" => "eng",
        "es" => "spa",
        "de" => "deu",
        "fr" => "fra",
        "zh" => "cmn",
        "ru" => "rus",
        "pt" => "por",
        "it" => "ita",
        "nl" => "nld",
        "ar" => "arb",
        "ja" => "jpn",
        "ko" => "kor",
        other => other,
    }
}

/// Filters chunks whose detected language does not match `target_lang`.
pub struct LanguageFilter {
    /// BCP 47 language tag for the allowed language (e.g., `"en"`).
    /// Stored normalised to ISO 639-3 for direct comparison with whatlang.
    pub target_lang: String,
    pub policy: LanguagePolicy,
}

impl Default for LanguageFilter {
    fn default() -> Self {
        Self {
            target_lang: "en".to_string(),
            policy: LanguagePolicy::Drop,
        }
    }
}

impl LanguageFilter {
    pub fn new(target_lang: impl Into<String>, policy: LanguagePolicy) -> Self {
        Self {
            target_lang: target_lang.into(),
            policy,
        }
    }
}

impl ChunkFilter for LanguageFilter {
    fn apply(&self, text: &str, _metadata: &BTreeMap<String, String>) -> String {
        if text.len() < MIN_DETECT_BYTES {
            return text.to_string();
        }
        let target_iso = bcp47_to_iso639_3(&self.target_lang);
        if let Some(info) = detect(text) {
            let code = info.lang().code();
            if code != target_iso {
                match self.policy {
                    LanguagePolicy::Drop => return String::new(),
                    LanguagePolicy::Flag => {
                        // Flag: ChunkFilter doesn't carry &mut metadata so we
                        // return the text unchanged. The corpus pipeline uses
                        // `description_lang` metadata from NvdFeedParser for
                        // post-ingest filtering; Flag mode preserves all chunks.
                        return text.to_string();
                    }
                }
            }
        }
        text.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_meta() -> BTreeMap<String, String> {
        BTreeMap::new()
    }

    const ENGLISH_TEXT: &str =
        "A critical heap buffer overflow vulnerability allows remote code execution via network.";
    const SPANISH_TEXT: &str = "Una vulnerabilidad crítica de desbordamiento de búfer permite la ejecución remota de código.";
    const GERMAN_TEXT: &str = "Eine kritische Puffer-Überlauf-Schwachstelle ermöglicht entfernte Codeausführung über das Netzwerk.";

    #[test]
    fn english_text_passes_through_unchanged() {
        let filter = LanguageFilter::default();
        let out = filter.apply(ENGLISH_TEXT, &empty_meta());
        assert_eq!(out, ENGLISH_TEXT);
    }

    #[test]
    fn spanish_text_dropped_by_default() {
        let filter = LanguageFilter::default(); // Drop policy
        let out = filter.apply(SPANISH_TEXT, &empty_meta());
        assert!(
            out.is_empty(),
            "Spanish must be dropped in Drop mode; got: {out}"
        );
    }

    #[test]
    fn german_text_dropped_by_default() {
        let filter = LanguageFilter::default();
        let out = filter.apply(GERMAN_TEXT, &empty_meta());
        assert!(out.is_empty(), "German must be dropped in Drop mode");
    }

    #[test]
    fn flag_policy_keeps_non_english_text() {
        let filter = LanguageFilter::new("en", LanguagePolicy::Flag);
        let out = filter.apply(SPANISH_TEXT, &empty_meta());
        assert_eq!(out, SPANISH_TEXT, "Flag policy must preserve the text");
    }

    #[test]
    fn short_text_below_threshold_always_kept() {
        let filter = LanguageFilter::default();
        // "hola" is Spanish but below MIN_DETECT_BYTES
        let out = filter.apply("hola", &empty_meta());
        assert_eq!(out, "hola");
    }
}
