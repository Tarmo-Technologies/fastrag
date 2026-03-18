use fastrag_core::{Element, ElementKind};
use pdf::content::Op;
use pdf::object::{Resolve, Resources, XObject};

/// Classify an image as "chart" or "figure" based on aspect ratio and size.
pub fn classify_image(width: u32, height: u32) -> &'static str {
    if width == 0 || height == 0 {
        return "figure";
    }
    let ratio = width as f32 / height as f32;
    if width > 200 && (1.2..=3.0).contains(&ratio) {
        "chart"
    } else {
        "figure"
    }
}

/// Extract image elements from a page's content stream ops and resources.
pub fn extract_images(
    ops: &[Op],
    resources: &Resources,
    resolver: &impl Resolve,
    page_num: u32,
) -> Vec<Element> {
    let mut elements = Vec::new();

    for op in ops {
        match op {
            Op::XObject { name } => {
                if let Some(xobj_ref) = resources.xobjects.get(name)
                    && let Ok(xobj) = resolver.get(*xobj_ref)
                    && let XObject::Image(ref img) = *xobj
                {
                    let width = img.width;
                    let height = img.height;
                    let image_type = classify_image(width, height);
                    let mut el =
                        Element::new(ElementKind::Image, "").with_page(page_num as usize + 1);
                    el.attributes.insert("width".to_string(), width.to_string());
                    el.attributes
                        .insert("height".to_string(), height.to_string());
                    el.attributes
                        .insert("image_type".to_string(), image_type.to_string());
                    el.attributes.insert("name".to_string(), name.to_string());
                    elements.push(el);
                }
            }
            Op::InlineImage { image } => {
                let width = image.width;
                let height = image.height;
                let image_type = classify_image(width, height);
                let mut el = Element::new(ElementKind::Image, "").with_page(page_num as usize + 1);
                el.attributes.insert("width".to_string(), width.to_string());
                el.attributes
                    .insert("height".to_string(), height.to_string());
                el.attributes
                    .insert("image_type".to_string(), image_type.to_string());
                elements.push(el);
            }
            _ => {}
        }
    }

    elements
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_chart_wide_landscape() {
        // 400x200 → ratio 2.0, width > 200 → "chart"
        assert_eq!(classify_image(400, 200), "chart");
    }

    #[test]
    fn test_classify_figure_small_square() {
        // 50x50 → ratio 1.0, width ≤ 200 → "figure"
        assert_eq!(classify_image(50, 50), "figure");
    }

    #[test]
    fn test_classify_chart_wide_banner() {
        // 800x300 → ratio ~2.67, width > 200 → "chart"
        assert_eq!(classify_image(800, 300), "chart");
    }

    #[test]
    fn test_classify_figure_tall() {
        // 300x600 → ratio 0.5, outside 1.2-3.0 → "figure"
        assert_eq!(classify_image(300, 600), "figure");
    }

    #[test]
    fn test_classify_figure_zero_dimensions() {
        assert_eq!(classify_image(0, 0), "figure");
        assert_eq!(classify_image(400, 0), "figure");
    }

    #[test]
    fn test_extract_images_integration() {
        use crate::PdfParser;
        use fastrag_core::{FileFormat, Parser, SourceInfo};

        let pdf_bytes = include_bytes!("../../../tests/fixtures/sample_images.pdf");
        let parser = PdfParser;
        let source = SourceInfo::new(FileFormat::Pdf).with_filename("sample_images.pdf");
        let doc = parser.parse(pdf_bytes, &source).unwrap();

        let images: Vec<_> = doc
            .elements
            .iter()
            .filter(|e| e.kind == ElementKind::Image)
            .collect();

        assert_eq!(images.len(), 2, "expected 2 images, got {}", images.len());

        // First image: 400x200 (chart)
        let img1 = &images[0];
        assert_eq!(img1.attributes.get("width").unwrap(), "400");
        assert_eq!(img1.attributes.get("height").unwrap(), "200");
        assert_eq!(img1.attributes.get("image_type").unwrap(), "chart");

        // Second image: 100x100 (figure)
        let img2 = &images[1];
        assert_eq!(img2.attributes.get("width").unwrap(), "100");
        assert_eq!(img2.attributes.get("height").unwrap(), "100");
        assert_eq!(img2.attributes.get("image_type").unwrap(), "figure");
    }
}
