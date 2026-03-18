use fastrag_core::{
    Document, Element, ElementKind, FastRagError, FileFormat, Metadata, Parser, SourceInfo,
};

/// RTF parser using the `rtf-parser` crate for lexing, with custom paragraph detection.
pub struct RtfParser;

impl Parser for RtfParser {
    fn supported_formats(&self) -> &[FileFormat] {
        &[FileFormat::Rtf]
    }

    fn parse(&self, input: &[u8], source: &SourceInfo) -> Result<Document, FastRagError> {
        let text = std::str::from_utf8(input).map_err(|e| FastRagError::Encoding(e.to_string()))?;

        let tokens = rtf_parser::lexer::Lexer::scan(text).map_err(|e| FastRagError::Parse {
            format: FileFormat::Rtf,
            message: format!("RTF lexer error: {e:?}"),
        })?;

        let mut metadata = Metadata::new(source.format);
        metadata.source_file = source.filename.clone();

        // Extract text with paragraph breaks from tokens
        let paragraphs = extract_paragraphs(&tokens);

        let mut elements = Vec::new();
        for para in &paragraphs {
            let trimmed = para.trim();
            if trimmed.is_empty() {
                continue;
            }
            if elements.is_empty() && !trimmed.contains('\n') && trimmed.len() <= 100 {
                metadata.title = Some(trimmed.to_string());
                elements.push(Element::new(ElementKind::Title, trimmed));
            } else {
                elements.push(Element::new(ElementKind::Paragraph, trimmed));
            }
        }

        Ok(Document { metadata, elements })
    }
}

/// Walk tokens to extract text, splitting on `\par` control words.
fn extract_paragraphs(tokens: &[rtf_parser::tokens::Token]) -> Vec<String> {
    use rtf_parser::tokens::{ControlWord, Token};

    let mut paragraphs = Vec::new();
    let mut current = String::new();
    let mut depth = 0i32;
    let mut skip_depth: Option<i32> = None;

    for token in tokens {
        match token {
            Token::OpeningBracket => {
                depth += 1;
            }
            Token::ClosingBracket => {
                if skip_depth == Some(depth) {
                    skip_depth = None;
                }
                depth -= 1;
            }
            // Skip header groups (font table, color table, stylesheet, file table)
            Token::ControlSymbol((
                ControlWord::FontTable
                | ControlWord::ColorTable
                | ControlWord::StyleSheet
                | ControlWord::FileTable,
                _,
            )) => {
                skip_depth = Some(depth);
            }
            Token::ControlSymbol((ControlWord::Par, _)) if skip_depth.is_none() => {
                if !current.trim().is_empty() {
                    paragraphs.push(std::mem::take(&mut current));
                }
                current.clear();
            }
            Token::PlainText(s) if skip_depth.is_none() => {
                current.push_str(s);
            }
            _ => {}
        }
    }

    if !current.trim().is_empty() {
        paragraphs.push(current);
    }

    paragraphs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supported_formats_returns_rtf() {
        assert_eq!(RtfParser.supported_formats(), &[FileFormat::Rtf]);
    }

    #[test]
    fn parse_simple_rtf() {
        let rtf = r"{\rtf1\ansi Hello, World!}";
        let parser = RtfParser;
        let source = SourceInfo::new(FileFormat::Rtf).with_filename("test.rtf");
        let doc = parser.parse(rtf.as_bytes(), &source).unwrap();
        assert!(!doc.elements.is_empty());
        let all_text: String = doc.elements.iter().map(|e| e.text.clone()).collect();
        assert!(
            all_text.contains("Hello"),
            "expected 'Hello' in text, got: {all_text}"
        );
    }

    #[test]
    fn parse_rtf_with_multiple_paragraphs() {
        let rtf_bytes = include_bytes!("../../../tests/fixtures/sample.rtf");
        let parser = RtfParser;
        let source = SourceInfo::new(FileFormat::Rtf);
        let doc = parser.parse(rtf_bytes, &source).unwrap();
        assert!(
            doc.elements.len() >= 2,
            "expected ≥2 elements, got {}",
            doc.elements.len()
        );
    }

    #[test]
    fn invalid_rtf_returns_error() {
        let parser = RtfParser;
        let source = SourceInfo::new(FileFormat::Rtf);
        let result = parser.parse(b"not rtf at all", &source);
        assert!(result.is_err());
    }

    #[test]
    fn parse_fixture_file() {
        let rtf_bytes = include_bytes!("../../../tests/fixtures/sample.rtf");
        let parser = RtfParser;
        let source = SourceInfo::new(FileFormat::Rtf).with_filename("sample.rtf");
        let doc = parser.parse(rtf_bytes, &source).unwrap();
        assert!(!doc.elements.is_empty());
    }
}
