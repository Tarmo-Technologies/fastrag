use fastrag_core::{Element, ElementKind, FastRagError, FileFormat};
use pdf::content::Op;

/// Detect whether a page is scanned (image-only, no meaningful text).
///
/// Returns `true` when the text character count is below a threshold
/// AND at least one image op (`InlineImage` or `XObject`) is present.
pub fn is_scanned_page(ops: &[Op]) -> bool {
    let mut text_char_count: usize = 0;
    let mut has_image = false;

    for op in ops {
        match op {
            Op::TextDraw { text } => {
                if let Ok(s) = text.to_string() {
                    text_char_count += s.trim().len();
                }
            }
            Op::TextDrawAdjusted { array } => {
                for item in array {
                    if let pdf::content::TextDrawAdjusted::Text(t) = item {
                        if let Ok(s) = t.to_string() {
                            text_char_count += s.trim().len();
                        }
                    }
                }
            }
            Op::InlineImage { .. } | Op::XObject { .. } => {
                has_image = true;
            }
            _ => {}
        }
    }

    text_char_count < 10 && has_image
}

/// OCR a single page by rendering it to pixels and running Tesseract.
///
/// # Arguments
/// * `pdf_bytes` — The raw PDF file bytes
/// * `page_index` — Zero-based page index to OCR
/// * `dpi` — Resolution for rendering (e.g. 150)
///
/// # Returns
/// A vector of `Paragraph` elements extracted from the OCR text.
pub fn ocr_page(
    _pdf_bytes: &[u8],
    page_index: u32,
    _dpi: u32,
) -> Result<Vec<Element>, FastRagError> {
    // Use pdfium-render to render the page
    let pdfium = pdfium_render::prelude::Pdfium::new(
        pdfium_render::prelude::Pdfium::bind_to_statically_linked_library().map_err(|e| {
            FastRagError::Parse {
                format: FileFormat::Pdf,
                message: format!("pdfium init: {e}"),
            }
        })?,
    );

    let doc = pdfium
        .load_pdf_from_byte_slice(_pdf_bytes, None)
        .map_err(|e| FastRagError::Parse {
            format: FileFormat::Pdf,
            message: format!("pdfium load: {e}"),
        })?;

    let page = doc
        .pages()
        .get(page_index as u16)
        .map_err(|e| FastRagError::Parse {
            format: FileFormat::Pdf,
            message: format!("pdfium page {}: {e}", page_index + 1),
        })?;

    let bitmap = page
        .render_with_config(
            &pdfium_render::prelude::PdfRenderConfig::new()
                .set_target_width((_dpi as i32 * 85 / 10) as u16) // ~8.5 inches at given DPI
                .set_maximum_height((_dpi as i32 * 11) as u16), // ~11 inches at given DPI
        )
        .map_err(|e| FastRagError::Parse {
            format: FileFormat::Pdf,
            message: format!("pdfium render: {e}"),
        })?;

    let image = bitmap.as_image();
    let rgba = image.to_rgba8();
    let (width, height) = rgba.dimensions();

    // Run Tesseract OCR on the rendered image
    let mut tess =
        tesseract::Tesseract::new(None, Some("eng")).map_err(|e| FastRagError::Parse {
            format: FileFormat::Pdf,
            message: format!("tesseract init: {e}"),
        })?;

    tess = tess
        .set_frame(
            rgba.as_raw(),
            width as i32,
            height as i32,
            4,
            (width * 4) as i32,
        )
        .map_err(|e| FastRagError::Parse {
            format: FileFormat::Pdf,
            message: format!("tesseract set_frame: {e}"),
        })?;

    let ocr_text = tess.get_text().map_err(|e| FastRagError::Parse {
        format: FileFormat::Pdf,
        message: format!("tesseract get_text: {e}"),
    })?;

    let mut elements = Vec::new();
    for para in ocr_text.split("\n\n") {
        let trimmed = para.trim();
        if !trimmed.is_empty() {
            elements.push(
                Element::new(ElementKind::Paragraph, trimmed).with_page(page_index as usize + 1),
            );
        }
    }

    Ok(elements)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn test_is_scanned_no_text_has_image() {
        // A page with only an XObject (image) and no text → scanned
        let ops = vec![Op::XObject { name: "Im0".into() }];
        assert!(is_scanned_page(&ops));
    }

    #[test]
    fn test_is_scanned_has_text() {
        // A page with meaningful text → not scanned
        let ops = vec![Op::TextDraw {
            text: pdf::primitive::PdfString::new(b"Hello World, this is text".to_vec().into()),
        }];
        assert!(!is_scanned_page(&ops));
    }

    #[test]
    fn test_is_scanned_no_image_no_text() {
        // A page with neither text nor images → not scanned (no image)
        let ops = vec![Op::Save, Op::Restore];
        assert!(!is_scanned_page(&ops));
    }

    #[test]
    fn test_is_scanned_inline_image() {
        // An InlineImage op with no text → scanned
        // We can't easily construct an InlineImage, so test with XObject only
        let ops = vec![
            Op::XObject { name: "Im0".into() },
            Op::TextDraw {
                text: pdf::primitive::PdfString::new(b"Hi".to_vec().into()),
            },
        ];
        // "Hi" is 2 chars < 10, plus has image → scanned
        assert!(is_scanned_page(&ops));
    }

    #[test]
    fn test_scanned_pdf_page_count() {
        use crate::PdfParser;
        use fastrag_core::{FileFormat, Parser, SourceInfo};

        let pdf_bytes = include_bytes!("../../../tests/fixtures/sample_scanned.pdf");
        let parser = PdfParser;
        let source = SourceInfo::new(FileFormat::Pdf).with_filename("sample_scanned.pdf");
        let doc = parser.parse(pdf_bytes, &source).unwrap();

        assert_eq!(doc.metadata.page_count, Some(1));
    }
}
