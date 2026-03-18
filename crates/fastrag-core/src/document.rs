use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::format::FileFormat;

/// A parsed document containing metadata and structured elements.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    pub metadata: Metadata,
    pub elements: Vec<Element>,
}

/// Metadata about the source document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Metadata {
    pub source_file: Option<String>,
    pub format: FileFormat,
    pub title: Option<String>,
    pub author: Option<String>,
    pub page_count: Option<usize>,
    pub created_at: Option<String>,
    #[serde(flatten)]
    pub custom: HashMap<String, String>,
}

impl Metadata {
    pub fn new(format: FileFormat) -> Self {
        Self {
            source_file: None,
            format,
            title: None,
            author: None,
            page_count: None,
            created_at: None,
            custom: HashMap::new(),
        }
    }
}

/// A single structural element extracted from a document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Element {
    pub kind: ElementKind,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub section: Option<String>,
    pub depth: u8,
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub attributes: HashMap<String, String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<String>,
}

impl Element {
    pub fn new(kind: ElementKind, text: impl Into<String>) -> Self {
        Self {
            kind,
            text: text.into(),
            page: None,
            section: None,
            depth: 0,
            attributes: HashMap::new(),
            id: String::new(),
            parent_id: None,
            children: Vec::new(),
        }
    }

    pub fn with_depth(mut self, depth: u8) -> Self {
        self.depth = depth;
        self
    }

    pub fn with_page(mut self, page: usize) -> Self {
        self.page = Some(page);
        self
    }

    pub fn with_section(mut self, section: impl Into<String>) -> Self {
        self.section = Some(section.into());
        self
    }
}

impl Document {
    /// Assign sequential IDs and build parent-child hierarchy based on heading structure.
    pub fn build_hierarchy(&mut self) {
        // Pass 1: assign sequential IDs
        for (i, el) in self.elements.iter_mut().enumerate() {
            el.id = format!("el-{i}");
        }

        // Pass 2: assign parent_id using heading stack
        // Stack entries: (heading_depth, element_index)
        let mut stack: Vec<(u8, usize)> = Vec::new();

        for i in 0..self.elements.len() {
            let kind = self.elements[i].kind.clone();
            let depth = self.elements[i].depth;

            match kind {
                ElementKind::Title | ElementKind::Heading => {
                    // Effective depth: Title=0, Heading uses its depth field
                    let effective_depth = if kind == ElementKind::Title { 0 } else { depth };

                    // Pop stack entries with depth >= current
                    while let Some(&(d, _)) = stack.last() {
                        if d >= effective_depth {
                            stack.pop();
                        } else {
                            break;
                        }
                    }

                    // If stack is non-empty, this heading is a child of the top
                    if let Some(&(_, parent_idx)) = stack.last() {
                        let parent_id = self.elements[parent_idx].id.clone();
                        self.elements[i].parent_id = Some(parent_id);
                    }

                    stack.push((effective_depth, i));
                }
                _ => {
                    // Content element: parent is top of stack
                    if let Some(&(_, parent_idx)) = stack.last() {
                        let parent_id = self.elements[parent_idx].id.clone();
                        self.elements[i].parent_id = Some(parent_id);
                    }
                }
            }
        }

        // Pass 3: populate children vecs from parent_id references
        // Collect (parent_index, child_id) pairs first to avoid borrow issues
        let mut child_map: Vec<(usize, String)> = Vec::new();
        for el in &self.elements {
            if let Some(ref pid) = el.parent_id
                && let Some(parent_idx) = self.elements.iter().position(|e| e.id == *pid)
            {
                child_map.push((parent_idx, el.id.clone()));
            }
        }
        for (parent_idx, child_id) in child_map {
            self.elements[parent_idx].children.push(child_id);
        }
    }
}

/// The kind of structural element.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ElementKind {
    Title,
    Heading,
    Paragraph,
    Table,
    Code,
    List,
    ListItem,
    Image,
    BlockQuote,
    HorizontalRule,
    Unknown,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn element_new_defaults() {
        let el = Element::new(ElementKind::Paragraph, "hello");
        assert_eq!(el.kind, ElementKind::Paragraph);
        assert_eq!(el.text, "hello");
        assert_eq!(el.depth, 0);
        assert_eq!(el.page, None);
        assert_eq!(el.section, None);
        assert!(el.attributes.is_empty());
    }

    #[test]
    fn element_with_depth() {
        let el = Element::new(ElementKind::Heading, "h").with_depth(2);
        assert_eq!(el.depth, 2);
    }

    #[test]
    fn element_with_page() {
        let el = Element::new(ElementKind::Paragraph, "p").with_page(5);
        assert_eq!(el.page, Some(5));
    }

    #[test]
    fn element_with_section() {
        let el = Element::new(ElementKind::Paragraph, "p").with_section("intro");
        assert_eq!(el.section, Some("intro".to_string()));
    }

    #[test]
    fn element_builder_chaining() {
        let el = Element::new(ElementKind::Code, "x = 1")
            .with_depth(1)
            .with_page(3)
            .with_section("code");
        assert_eq!(el.depth, 1);
        assert_eq!(el.page, Some(3));
        assert_eq!(el.section, Some("code".to_string()));
        assert_eq!(el.text, "x = 1");
    }

    #[test]
    fn element_new_has_empty_hierarchy_fields() {
        let el = Element::new(ElementKind::Paragraph, "hello");
        assert!(el.id.is_empty());
        assert_eq!(el.parent_id, None);
        assert!(el.children.is_empty());
    }

    #[test]
    fn build_hierarchy_assigns_sequential_ids() {
        let mut doc = Document {
            metadata: Metadata::new(FileFormat::Html),
            elements: vec![
                Element::new(ElementKind::Paragraph, "a"),
                Element::new(ElementKind::Paragraph, "b"),
                Element::new(ElementKind::Paragraph, "c"),
            ],
        };
        doc.build_hierarchy();
        assert_eq!(doc.elements[0].id, "el-0");
        assert_eq!(doc.elements[1].id, "el-1");
        assert_eq!(doc.elements[2].id, "el-2");
    }

    #[test]
    fn build_hierarchy_heading_parents_content() {
        let mut doc = Document {
            metadata: Metadata::new(FileFormat::Html),
            elements: vec![
                Element::new(ElementKind::Title, "Title"),
                Element::new(ElementKind::Paragraph, "intro"),
                Element::new(ElementKind::Heading, "Section 1").with_depth(1),
                Element::new(ElementKind::Paragraph, "body"),
            ],
        };
        doc.build_hierarchy();
        // intro's parent is Title
        assert_eq!(doc.elements[1].parent_id, Some("el-0".to_string()));
        // Section 1's parent is Title (depth 1 > depth 0)
        assert_eq!(doc.elements[2].parent_id, Some("el-0".to_string()));
        // body's parent is Section 1
        assert_eq!(doc.elements[3].parent_id, Some("el-2".to_string()));
    }

    #[test]
    fn build_hierarchy_same_level_resets() {
        let mut doc = Document {
            metadata: Metadata::new(FileFormat::Html),
            elements: vec![
                Element::new(ElementKind::Heading, "H1-a").with_depth(1),
                Element::new(ElementKind::Paragraph, "under a"),
                Element::new(ElementKind::Heading, "H1-b").with_depth(1),
                Element::new(ElementKind::Paragraph, "under b"),
            ],
        };
        doc.build_hierarchy();
        // "under a" is child of H1-a
        assert_eq!(doc.elements[1].parent_id, Some("el-0".to_string()));
        // H1-b has no parent (same level, stack popped)
        assert_eq!(doc.elements[2].parent_id, None);
        // "under b" is child of H1-b
        assert_eq!(doc.elements[3].parent_id, Some("el-2".to_string()));
    }

    #[test]
    fn build_hierarchy_nested_headings() {
        let mut doc = Document {
            metadata: Metadata::new(FileFormat::Html),
            elements: vec![
                Element::new(ElementKind::Heading, "H1").with_depth(1),
                Element::new(ElementKind::Heading, "H2").with_depth(2),
                Element::new(ElementKind::Paragraph, "content"),
            ],
        };
        doc.build_hierarchy();
        // H2's parent is H1
        assert_eq!(doc.elements[1].parent_id, Some("el-0".to_string()));
        // content's parent is H2
        assert_eq!(doc.elements[2].parent_id, Some("el-1".to_string()));
    }

    #[test]
    fn build_hierarchy_children_populated() {
        let mut doc = Document {
            metadata: Metadata::new(FileFormat::Html),
            elements: vec![
                Element::new(ElementKind::Title, "Doc Title"),
                Element::new(ElementKind::Paragraph, "intro"),
                Element::new(ElementKind::Heading, "Sec").with_depth(1),
            ],
        };
        doc.build_hierarchy();
        // Title should have children: el-1 (Paragraph) and el-2 (Heading)
        assert_eq!(doc.elements[0].children, vec!["el-1", "el-2"]);
    }

    #[test]
    fn json_output_includes_hierarchy() {
        let mut doc = Document {
            metadata: Metadata::new(FileFormat::Html),
            elements: vec![
                Element::new(ElementKind::Title, "Title"),
                Element::new(ElementKind::Paragraph, "text"),
            ],
        };
        doc.build_hierarchy();
        let json = serde_json::to_string(&doc).unwrap();
        assert!(json.contains("\"id\":\"el-0\""), "missing id in json");
        assert!(
            json.contains("\"parent_id\":\"el-0\""),
            "missing parent_id in json"
        );
        assert!(
            json.contains("\"children\":[\"el-1\"]"),
            "missing children in json: {json}"
        );
    }

    #[test]
    fn metadata_new_defaults() {
        let m = Metadata::new(FileFormat::Html);
        assert_eq!(m.format, FileFormat::Html);
        assert_eq!(m.source_file, None);
        assert_eq!(m.title, None);
        assert_eq!(m.author, None);
        assert_eq!(m.page_count, None);
        assert_eq!(m.created_at, None);
        assert!(m.custom.is_empty());
    }
}
