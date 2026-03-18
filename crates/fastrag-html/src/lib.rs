use scraper::{Html, Selector};

use fastrag_core::{
    Document, Element, ElementKind, FastRagError, FileFormat, Metadata, Parser, SourceInfo,
};

/// HTML parser using scraper. Strips nav, script, style elements and extracts content.
pub struct HtmlParser;

impl Parser for HtmlParser {
    fn supported_formats(&self) -> &[FileFormat] {
        &[FileFormat::Html]
    }

    fn parse(&self, input: &[u8], source: &SourceInfo) -> Result<Document, FastRagError> {
        let text =
            String::from_utf8(input.to_vec()).map_err(|e| FastRagError::Encoding(e.to_string()))?;

        let mut metadata = Metadata::new(source.format);
        metadata.source_file = source.filename.clone();

        let html = Html::parse_document(&text);

        if let Some(title_el) = Selector::parse("title")
            .ok()
            .and_then(|s| html.select(&s).next())
        {
            let title_text: String = title_el.text().collect();
            let title_text = title_text.trim().to_string();
            if !title_text.is_empty() {
                metadata.title = Some(title_text);
            }
        }

        let mut elements = Vec::new();

        let content_selectors = [
            ("h1", ElementKind::Title, 0u8),
            ("h2", ElementKind::Heading, 1),
            ("h3", ElementKind::Heading, 2),
            ("h4", ElementKind::Heading, 3),
            ("h5", ElementKind::Heading, 4),
            ("h6", ElementKind::Heading, 5),
        ];

        let body_selector = Selector::parse("article, main, [role='main'], .content, .post, body")
            .expect("valid selector");

        if let Some(body) = html.select(&body_selector).next() {
            for node in body.descendants() {
                if let Some(el) = node.value().as_element() {
                    let tag = el.name();

                    if matches!(
                        tag,
                        "script" | "style" | "nav" | "header" | "footer" | "noscript" | "svg"
                    ) {
                        continue;
                    }

                    let text: String = node
                        .children()
                        .filter_map(|c| c.value().as_text().map(|t| t.to_string()))
                        .collect::<Vec<_>>()
                        .join("")
                        .trim()
                        .to_string();

                    if text.is_empty() && tag != "table" && tag != "hr" && tag != "figure" {
                        continue;
                    }

                    let mut matched = false;
                    for (selector_tag, kind, depth) in &content_selectors {
                        if tag == *selector_tag {
                            elements.push(Element::new(kind.clone(), &text).with_depth(*depth));
                            matched = true;
                            break;
                        }
                    }

                    if !matched {
                        match tag {
                            "p" => {
                                elements.push(Element::new(ElementKind::Paragraph, &text));
                            }
                            "pre" | "code" => {
                                let full_text: String = scraper::ElementRef::wrap(node)
                                    .map(|er| er.text().collect::<String>())
                                    .unwrap_or(text.clone());
                                elements.push(Element::new(ElementKind::Code, full_text.trim()));
                            }
                            "blockquote" => {
                                let full_text: String = scraper::ElementRef::wrap(node)
                                    .map(|er| er.text().collect::<String>())
                                    .unwrap_or(text.clone());
                                elements
                                    .push(Element::new(ElementKind::BlockQuote, full_text.trim()));
                            }
                            "li" => {
                                elements.push(Element::new(ElementKind::ListItem, &text));
                            }
                            "hr" => {
                                elements.push(Element::new(ElementKind::HorizontalRule, ""));
                            }
                            "table" => {
                                let full_text: String = scraper::ElementRef::wrap(node)
                                    .map(|er| render_html_table(er))
                                    .unwrap_or(text.clone());
                                elements.push(Element::new(ElementKind::Table, full_text.trim()));
                            }
                            "figure" => {
                                if let Some(er) = scraper::ElementRef::wrap(node) {
                                    let img_sel = Selector::parse("img").expect("valid");
                                    let cap_sel = Selector::parse("figcaption").expect("valid");
                                    if let Some(img_el) = er.select(&img_sel).next() {
                                        let src = img_el.value().attr("src").unwrap_or("");
                                        let alt = img_el.value().attr("alt").unwrap_or("");
                                        let mut img_elem = Element::new(ElementKind::Image, src);
                                        img_elem
                                            .attributes
                                            .insert("alt".to_string(), alt.to_string());
                                        if let Some(cap_el) = er.select(&cap_sel).next() {
                                            let cap_text: String =
                                                cap_el.text().collect::<String>();
                                            let cap_text = cap_text.trim().to_string();
                                            if !cap_text.is_empty() {
                                                // Use temporary markers; actual IDs assigned by build_hierarchy
                                                let img_marker =
                                                    format!("__figure_{}", elements.len());
                                                let cap_marker =
                                                    format!("__figure_{}", elements.len() + 1);
                                                img_elem.attributes.insert(
                                                    "associated_caption_id".to_string(),
                                                    cap_marker,
                                                );
                                                elements.push(img_elem);
                                                let mut cap_elem =
                                                    Element::new(ElementKind::Paragraph, &cap_text);
                                                cap_elem.attributes.insert(
                                                    "associated_image_id".to_string(),
                                                    img_marker,
                                                );
                                                elements.push(cap_elem);
                                            } else {
                                                elements.push(img_elem);
                                            }
                                        } else {
                                            elements.push(img_elem);
                                        }
                                    }
                                }
                            }
                            "figcaption" => {
                                // Handled by <figure> branch above — skip standalone
                            }
                            "img" => {
                                let src = el.attr("src").unwrap_or("");
                                let alt = el.attr("alt").unwrap_or("");
                                let mut elem = Element::new(ElementKind::Image, src);
                                elem.attributes.insert("alt".to_string(), alt.to_string());
                                elements.push(elem);
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        // Extract footnotes from dedicated footnote sections
        extract_footnotes(&html, &mut elements);

        // Detect footnote references in paragraph text
        detect_footnote_refs(&html, &mut elements);

        Ok(Document { metadata, elements })
    }
}

/// Extract footnotes from footnote/endnote sections in the HTML.
fn extract_footnotes(html: &Html, elements: &mut Vec<Element>) {
    let footnote_section_selectors = [
        "section.footnotes",
        "div.footnotes",
        "div.endnotes",
        "ol.footnotes",
        "[role='doc-endnotes']",
    ];

    for sel_str in &footnote_section_selectors {
        if let Ok(sel) = Selector::parse(sel_str) {
            for section in html.select(&sel) {
                let item_sel = Selector::parse("li[id]").expect("valid");
                for item in section.select(&item_sel) {
                    let id = item.value().attr("id").unwrap_or("");
                    let text: String = item.text().collect::<String>();
                    let text = text.trim().to_string();
                    if text.is_empty() {
                        continue;
                    }
                    let mut el = Element::new(ElementKind::Footnote, &text);
                    if !id.is_empty() {
                        el.attributes
                            .insert("reference_id".to_string(), id.to_string());
                    }
                    // Avoid duplicate footnotes (same id)
                    if !elements.iter().any(|e| {
                        e.kind == ElementKind::Footnote
                            && e.attributes.get("reference_id") == Some(&id.to_string())
                    }) {
                        elements.push(el);
                    }
                }
            }
        }
    }
}

/// Detect footnote references (e.g. <sup><a href="#fn1">) in the document
/// and store them as an attribute on the parent paragraph elements.
fn detect_footnote_refs(html: &Html, elements: &mut [Element]) {
    // Collect all footnote ref IDs from <sup><a href="#fn..."> or <sup><a href="#en...">
    let sup_sel = Selector::parse("sup > a[href]").expect("valid");
    let mut refs_by_paragraph: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();

    for a_el in html.select(&sup_sel) {
        let href = a_el.value().attr("href").unwrap_or("");
        if !href.starts_with('#') {
            continue;
        }
        let ref_id = &href[1..];
        if ref_id.is_empty() {
            continue;
        }

        // Find the closest ancestor <p> text to associate
        if let Some(parent_p) = a_el.ancestors().find_map(|ancestor| {
            ancestor
                .value()
                .as_element()
                .filter(|e| e.name() == "p")
                .map(|_| {
                    let node_ref = scraper::ElementRef::wrap(ancestor).unwrap();
                    let text: String = node_ref
                        .children()
                        .filter_map(|c| c.value().as_text().map(|t| t.to_string()))
                        .collect::<Vec<_>>()
                        .join("")
                        .trim()
                        .to_string();
                    text
                })
        }) {
            refs_by_paragraph
                .entry(parent_p)
                .or_default()
                .push(ref_id.to_string());
        }
    }

    // Associate refs with matching paragraph elements
    for element in elements.iter_mut() {
        if element.kind != ElementKind::Paragraph {
            continue;
        }
        if let Some(refs) = refs_by_paragraph.get(&element.text) {
            element
                .attributes
                .insert("footnote_refs".to_string(), refs.join(","));
        }
    }
}

fn render_html_table(table: scraper::ElementRef) -> String {
    let mut out = String::new();
    let row_sel = Selector::parse("tr").expect("valid");
    let th_sel = Selector::parse("th").expect("valid");
    let td_sel = Selector::parse("td").expect("valid");

    let mut is_first_row = true;
    for row in table.select(&row_sel) {
        out.push('|');
        let cells: Vec<String> = row
            .select(&th_sel)
            .chain(row.select(&td_sel))
            .map(|c| c.text().collect::<String>().trim().to_string())
            .collect();

        for cell in &cells {
            out.push_str(&format!(" {cell} |"));
        }
        out.push('\n');

        if is_first_row {
            out.push('|');
            for _ in &cells {
                out.push_str(" --- |");
            }
            out.push('\n');
            is_first_row = false;
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_html(input: &str) -> Document {
        let parser = HtmlParser;
        let source = SourceInfo::new(FileFormat::Html).with_filename("test.html");
        parser.parse(input.as_bytes(), &source).unwrap()
    }

    #[test]
    fn supported_formats_returns_html() {
        assert_eq!(HtmlParser.supported_formats(), &[FileFormat::Html]);
    }

    #[test]
    fn title_tag_sets_metadata() {
        let doc =
            parse_html("<html><head><title>My Page</title></head><body><p>Hi</p></body></html>");
        assert_eq!(doc.metadata.title, Some("My Page".to_string()));
    }

    #[test]
    fn h1_becomes_title_depth0() {
        let doc = parse_html("<html><body><h1>Hello</h1></body></html>");
        let el = doc
            .elements
            .iter()
            .find(|e| e.kind == ElementKind::Title)
            .unwrap();
        assert_eq!(el.text, "Hello");
        assert_eq!(el.depth, 0);
    }

    #[test]
    fn h2_becomes_heading_depth1() {
        let doc = parse_html("<html><body><h2>Section</h2></body></html>");
        let el = doc
            .elements
            .iter()
            .find(|e| e.kind == ElementKind::Heading)
            .unwrap();
        assert_eq!(el.depth, 1);
    }

    #[test]
    fn h3_becomes_heading_depth2() {
        let doc = parse_html("<html><body><h3>Sub</h3></body></html>");
        let el = doc
            .elements
            .iter()
            .find(|e| e.kind == ElementKind::Heading)
            .unwrap();
        assert_eq!(el.depth, 2);
    }

    #[test]
    fn p_becomes_paragraph() {
        let doc = parse_html("<html><body><p>Content text</p></body></html>");
        let el = doc
            .elements
            .iter()
            .find(|e| e.kind == ElementKind::Paragraph)
            .unwrap();
        assert_eq!(el.text, "Content text");
    }

    #[test]
    fn script_and_style_stripped() {
        let doc = parse_html(
            "<html><body>\
             <script>alert('xss')</script>\
             <style>body { color: red; }</style>\
             <p>Content</p>\
             </body></html>",
        );
        assert!(
            !doc.elements
                .iter()
                .any(|e| e.text.contains("alert") || e.text.contains("color"))
        );
        assert!(
            doc.elements
                .iter()
                .any(|e| e.kind == ElementKind::Paragraph && e.text == "Content")
        );
    }

    #[test]
    fn li_becomes_list_item() {
        let doc =
            parse_html("<html><body><ul><li>Item one</li><li>Item two</li></ul></body></html>");
        let items: Vec<_> = doc
            .elements
            .iter()
            .filter(|e| e.kind == ElementKind::ListItem)
            .collect();
        assert!(items.len() >= 2);
        assert_eq!(items[0].text, "Item one");
    }

    #[test]
    fn pre_code_becomes_code() {
        let doc = parse_html("<html><body><pre><code>let x = 1;</code></pre></body></html>");
        assert!(
            doc.elements
                .iter()
                .any(|e| e.kind == ElementKind::Code && e.text.contains("let x = 1;"))
        );
    }

    #[test]
    fn table_becomes_table_element() {
        let doc = parse_html(
            "<html><body><table><tr><th>A</th><th>B</th></tr><tr><td>1</td><td>2</td></tr></table></body></html>",
        );
        let el = doc
            .elements
            .iter()
            .find(|e| e.kind == ElementKind::Table)
            .unwrap();
        assert!(el.text.contains("| A |"));
        assert!(el.text.contains("| 1 |"));
    }

    #[test]
    fn blockquote_becomes_blockquote() {
        let doc = parse_html("<html><body><blockquote>Wise words</blockquote></body></html>");
        assert!(
            doc.elements
                .iter()
                .any(|e| e.kind == ElementKind::BlockQuote && e.text.contains("Wise words"))
        );
    }

    #[test]
    fn invalid_utf8_returns_encoding_error() {
        let parser = HtmlParser;
        let source = SourceInfo::new(FileFormat::Html);
        let bad_bytes: &[u8] = &[0xFF, 0xFE, 0x80, 0x81];
        let result = parser.parse(bad_bytes, &source);
        assert!(result.is_err());
        match result.unwrap_err() {
            FastRagError::Encoding(_) => {}
            other => panic!("expected Encoding error, got: {other}"),
        }
    }

    #[test]
    fn figure_figcaption_association() {
        let doc = parse_html(
            "<html><body>\
             <figure><img src=\"chart.png\" alt=\"Chart\"><figcaption>Figure 1: Revenue</figcaption></figure>\
             </body></html>",
        );
        let img = doc
            .elements
            .iter()
            .find(|e| e.kind == ElementKind::Image)
            .unwrap();
        assert_eq!(img.text, "chart.png");
        assert!(img.attributes.contains_key("associated_caption_id"));

        let cap = doc
            .elements
            .iter()
            .find(|e| e.kind == ElementKind::Paragraph && e.text == "Figure 1: Revenue")
            .unwrap();
        assert!(cap.attributes.contains_key("associated_image_id"));
    }

    #[test]
    fn prefers_main_content_area() {
        let doc = parse_html(
            "<html><body>\
             <nav><a>Home</a></nav>\
             <main><h1>Title</h1><p>Main content</p></main>\
             <footer><p>Footer</p></footer>\
             </body></html>",
        );
        assert!(doc.elements.iter().any(|e| e.text == "Main content"));
    }

    // --- Footnote extraction ---

    #[test]
    fn footnotes_section_extracted() {
        let doc = parse_html(
            "<html><body>\
             <p>Some text</p>\
             <section class=\"footnotes\">\
             <ol>\
             <li id=\"fn1\">First footnote text.</li>\
             <li id=\"fn2\">Second footnote text.</li>\
             </ol>\
             </section>\
             </body></html>",
        );
        let footnotes: Vec<_> = doc
            .elements
            .iter()
            .filter(|e| e.kind == ElementKind::Footnote)
            .collect();
        assert_eq!(
            footnotes.len(),
            2,
            "expected 2 footnotes, got: {footnotes:?}"
        );
        assert_eq!(footnotes[0].text, "First footnote text.");
        assert_eq!(
            footnotes[0].attributes.get("reference_id"),
            Some(&"fn1".to_string())
        );
        assert_eq!(footnotes[1].text, "Second footnote text.");
    }

    #[test]
    fn footnote_refs_detected() {
        let doc = parse_html(
            "<html><body>\
             <p>Text with a reference<sup><a href=\"#fn1\">1</a></sup> here.</p>\
             <section class=\"footnotes\">\
             <ol><li id=\"fn1\">A footnote.</li></ol>\
             </section>\
             </body></html>",
        );
        let para = doc
            .elements
            .iter()
            .find(|e| e.kind == ElementKind::Paragraph && e.text.contains("Text with"))
            .unwrap();
        assert_eq!(
            para.attributes.get("footnote_refs"),
            Some(&"fn1".to_string()),
            "expected footnote_refs attribute, got: {:?}",
            para.attributes
        );
    }

    #[test]
    fn no_footnotes_no_crash() {
        let doc = parse_html("<html><body><p>Just a paragraph.</p></body></html>");
        let footnotes: Vec<_> = doc
            .elements
            .iter()
            .filter(|e| e.kind == ElementKind::Footnote)
            .collect();
        assert!(footnotes.is_empty());
    }

    #[test]
    fn endnotes_with_role_attribute() {
        let doc = parse_html(
            "<html><body>\
             <p>Content</p>\
             <div role=\"doc-endnotes\">\
             <ol><li id=\"en1\">An endnote.</li></ol>\
             </div>\
             </body></html>",
        );
        let footnotes: Vec<_> = doc
            .elements
            .iter()
            .filter(|e| e.kind == ElementKind::Footnote)
            .collect();
        assert_eq!(footnotes.len(), 1);
        assert_eq!(footnotes[0].text, "An endnote.");
        assert_eq!(
            footnotes[0].attributes.get("reference_id"),
            Some(&"en1".to_string())
        );
    }
}
