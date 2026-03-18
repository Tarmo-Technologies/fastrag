use fastrag_core::{Element, ElementKind};
use pdf::content::{Op, TextDrawAdjusted};

/// A text fragment with its absolute position on the page.
#[derive(Debug, Clone)]
pub struct PositionedText {
    pub x: f32,
    pub y: f32,
    pub text: String,
}

/// A detected table candidate with rows of cell strings.
#[derive(Debug, Clone)]
pub struct TableCandidate {
    pub rows: Vec<Vec<String>>,
    pub page: usize,
}

/// Tolerance for grouping text items into the same row (points).
pub const ROW_Y_TOLERANCE: f32 = 3.0;

/// Tolerance for grouping x-coordinates into the same column (points).
pub const COL_X_TOLERANCE: f32 = 10.0;

/// Minimum number of rows to qualify as a table.
pub const MIN_TABLE_ROWS: usize = 2;

/// Minimum number of columns to qualify as a table.
pub const MIN_TABLE_COLS: usize = 2;

/// Extract positioned text fragments from PDF content stream ops.
///
/// Tracks the current text matrix via `SetTextMatrix` and `MoveTextPosition`,
/// and captures text from `TextDraw` and `TextDrawAdjusted` ops.
pub fn collect_positioned_text(ops: &[Op]) -> Vec<PositionedText> {
    let mut result = Vec::new();
    let mut current_x: f32 = 0.0;
    let mut current_y: f32 = 0.0;
    let mut leading: f32 = 0.0;

    for op in ops {
        match op {
            Op::SetTextMatrix { matrix } => {
                current_x = matrix.e;
                current_y = matrix.f;
            }
            Op::MoveTextPosition { translation } => {
                current_x += translation.x;
                current_y += translation.y;
            }
            Op::Leading { leading: l } => {
                leading = *l;
            }
            Op::TextNewline => {
                current_x = 0.0;
                current_y -= leading;
            }
            Op::TextDraw { text } => {
                if let Ok(s) = text.to_string() {
                    let trimmed = s.trim().to_string();
                    if !trimmed.is_empty() {
                        result.push(PositionedText {
                            x: current_x,
                            y: current_y,
                            text: trimmed,
                        });
                        current_x += s.len() as f32 * 5.0;
                    }
                }
            }
            Op::TextDrawAdjusted { array } => {
                let mut combined = String::new();
                for item in array {
                    if let TextDrawAdjusted::Text(t) = item
                        && let Ok(s) = t.to_string()
                    {
                        combined.push_str(&s);
                    }
                }
                let trimmed = combined.trim().to_string();
                if !trimmed.is_empty() {
                    result.push(PositionedText {
                        x: current_x,
                        y: current_y,
                        text: trimmed,
                    });
                    current_x += combined.len() as f32 * 5.0;
                }
            }
            _ => {}
        }
    }

    result
}

/// Cluster positioned text items into rows based on y-coordinate proximity.
///
/// Items within `ROW_Y_TOLERANCE` of each other are grouped into the same row.
/// Returns rows sorted by y descending (top of page first in PDF coordinates).
pub fn cluster_into_rows(items: &[PositionedText]) -> Vec<Vec<PositionedText>> {
    if items.is_empty() {
        return Vec::new();
    }

    let mut sorted: Vec<_> = items.to_vec();
    sorted.sort_by(|a, b| b.y.partial_cmp(&a.y).unwrap_or(std::cmp::Ordering::Equal));

    let mut rows: Vec<Vec<PositionedText>> = Vec::new();
    let mut current_row: Vec<PositionedText> = vec![sorted[0].clone()];
    let mut current_y = sorted[0].y;

    for item in sorted.iter().skip(1) {
        if (item.y - current_y).abs() < ROW_Y_TOLERANCE {
            current_row.push(item.clone());
        } else {
            // Sort current row by x before pushing
            current_row.sort_by(|a, b| a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal));
            rows.push(current_row);
            current_row = vec![item.clone()];
            current_y = item.y;
        }
    }
    // Don't forget the last row
    current_row.sort_by(|a, b| a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal));
    rows.push(current_row);

    rows
}

/// Detect table candidates from clustered rows.
///
/// A table candidate is a consecutive run of rows where each row has at least
/// `MIN_TABLE_COLS` items, with at least `MIN_TABLE_ROWS` rows.
/// Column alignment is detected by clustering x-coordinates.
pub fn detect_tables(rows: &[Vec<PositionedText>]) -> Vec<TableCandidate> {
    let mut candidates = Vec::new();
    let mut run_start: Option<usize> = None;

    for (i, row) in rows.iter().enumerate() {
        if row.len() >= MIN_TABLE_COLS {
            if run_start.is_none() {
                run_start = Some(i);
            }
        } else {
            if let Some(start) = run_start
                && i - start >= MIN_TABLE_ROWS
            {
                candidates.push(build_table_candidate(&rows[start..i]));
            }
            run_start = None;
        }
    }
    // Handle run that extends to the end
    if let Some(start) = run_start
        && rows.len() - start >= MIN_TABLE_ROWS
    {
        candidates.push(build_table_candidate(&rows[start..]));
    }

    candidates
}

/// Build a TableCandidate from a slice of rows by aligning columns.
fn build_table_candidate(rows: &[Vec<PositionedText>]) -> TableCandidate {
    // Collect all x-coordinates
    let mut all_x: Vec<f32> = rows.iter().flat_map(|r| r.iter().map(|t| t.x)).collect();
    all_x.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    // Cluster x-coordinates into column positions
    let col_positions = cluster_x_positions(&all_x);
    let num_cols = col_positions.len();

    let mut grid: Vec<Vec<String>> = Vec::new();
    for row in rows {
        let mut cells = vec![String::new(); num_cols];
        for item in row {
            let col_idx = nearest_column(&col_positions, item.x);
            if !cells[col_idx].is_empty() {
                cells[col_idx].push(' ');
            }
            cells[col_idx].push_str(&item.text);
        }
        grid.push(cells);
    }

    TableCandidate {
        rows: grid,
        page: 0, // caller sets this
    }
}

/// Cluster x-coordinates within COL_X_TOLERANCE into representative column positions.
fn cluster_x_positions(sorted_xs: &[f32]) -> Vec<f32> {
    if sorted_xs.is_empty() {
        return Vec::new();
    }

    let mut clusters: Vec<(f32, usize)> = Vec::new(); // (sum, count)
    let mut current_sum = sorted_xs[0];
    let mut current_count: usize = 1;
    let mut current_center = sorted_xs[0];

    for &x in sorted_xs.iter().skip(1) {
        if (x - current_center).abs() < COL_X_TOLERANCE {
            current_sum += x;
            current_count += 1;
            current_center = current_sum / current_count as f32;
        } else {
            clusters.push((current_sum, current_count));
            current_sum = x;
            current_count = 1;
            current_center = x;
        }
    }
    clusters.push((current_sum, current_count));

    clusters
        .iter()
        .map(|(sum, count)| sum / *count as f32)
        .collect()
}

/// Find the nearest column index for a given x-coordinate.
fn nearest_column(col_positions: &[f32], x: f32) -> usize {
    col_positions
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| {
            (x - **a)
                .abs()
                .partial_cmp(&(x - **b).abs())
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|(i, _)| i)
        .unwrap_or(0)
}

/// Render a TableCandidate as a markdown table Element.
pub fn render_table_element(candidate: &TableCandidate) -> Element {
    let mut md = String::new();

    for (i, row) in candidate.rows.iter().enumerate() {
        md.push_str("| ");
        md.push_str(&row.join(" | "));
        md.push_str(" |");

        if i == 0 {
            // Add separator after header
            md.push('\n');
            md.push_str("| ");
            md.push_str(
                &row.iter()
                    .map(|_| "---".to_string())
                    .collect::<Vec<_>>()
                    .join(" | "),
            );
            md.push_str(" |");
        }

        if i < candidate.rows.len() - 1 {
            md.push('\n');
        }
    }

    Element::new(ElementKind::Table, md).with_page(candidate.page)
}

/// Collect the set of positioned text items that are part of detected tables.
/// Returns (table_elements, remaining_non_table_positioned_texts).
pub fn extract_tables_from_ops(ops: &[Op], page_num: u32) -> (Vec<Element>, Vec<PositionedText>) {
    let positioned = collect_positioned_text(ops);
    let rows = cluster_into_rows(&positioned);
    let tables = detect_tables(&rows);

    if tables.is_empty() {
        return (Vec::new(), positioned);
    }

    // Build set of table text coordinates to exclude from paragraphs
    let mut table_elements = Vec::new();
    let mut table_texts: std::collections::HashSet<usize> = std::collections::HashSet::new();

    for mut candidate in tables {
        candidate.page = page_num as usize + 1;

        // Track which positioned text items belong to this table
        // (approximate: items that contributed to any cell)
        for row in &rows {
            if row.len() >= MIN_TABLE_COLS {
                for item in row {
                    // Find matching positioned text index
                    for (idx, pt) in positioned.iter().enumerate() {
                        if (pt.x - item.x).abs() < 0.01
                            && (pt.y - item.y).abs() < 0.01
                            && pt.text == item.text
                        {
                            table_texts.insert(idx);
                        }
                    }
                }
            }
        }

        table_elements.push(render_table_element(&candidate));
    }

    let remaining: Vec<PositionedText> = positioned
        .iter()
        .enumerate()
        .filter(|(i, _)| !table_texts.contains(i))
        .map(|(_, pt)| pt.clone())
        .collect();

    (table_elements, remaining)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cluster_into_rows() {
        let items = vec![
            PositionedText {
                x: 10.0,
                y: 100.0,
                text: "A".into(),
            },
            PositionedText {
                x: 50.0,
                y: 100.3,
                text: "B".into(),
            },
            PositionedText {
                x: 10.0,
                y: 50.0,
                text: "C".into(),
            },
            PositionedText {
                x: 50.0,
                y: 50.1,
                text: "D".into(),
            },
            PositionedText {
                x: 10.0,
                y: 25.0,
                text: "E".into(),
            },
            PositionedText {
                x: 50.0,
                y: 25.2,
                text: "F".into(),
            },
        ];
        let rows = cluster_into_rows(&items);
        assert_eq!(rows.len(), 3, "expected 3 rows, got {}", rows.len());
        assert_eq!(rows[0].len(), 2);
        assert_eq!(rows[1].len(), 2);
        assert_eq!(rows[2].len(), 2);
        // First row (highest y) should be A, B
        assert_eq!(rows[0][0].text, "A");
        assert_eq!(rows[0][1].text, "B");
    }

    #[test]
    fn test_no_table_single_row() {
        let items = vec![
            PositionedText {
                x: 10.0,
                y: 100.0,
                text: "A".into(),
            },
            PositionedText {
                x: 50.0,
                y: 100.0,
                text: "B".into(),
            },
            PositionedText {
                x: 90.0,
                y: 100.0,
                text: "C".into(),
            },
        ];
        let rows = cluster_into_rows(&items);
        assert_eq!(rows.len(), 1);
        let tables = detect_tables(&rows);
        assert!(tables.is_empty(), "single row should not be a table");
    }

    #[test]
    fn test_detect_tables_2x2() {
        let rows = vec![
            vec![
                PositionedText {
                    x: 10.0,
                    y: 100.0,
                    text: "A".into(),
                },
                PositionedText {
                    x: 200.0,
                    y: 100.0,
                    text: "B".into(),
                },
            ],
            vec![
                PositionedText {
                    x: 10.0,
                    y: 80.0,
                    text: "C".into(),
                },
                PositionedText {
                    x: 200.0,
                    y: 80.0,
                    text: "D".into(),
                },
            ],
        ];
        let tables = detect_tables(&rows);
        assert_eq!(tables.len(), 1);
        assert_eq!(tables[0].rows.len(), 2);
        assert_eq!(tables[0].rows[0], vec!["A", "B"]);
        assert_eq!(tables[0].rows[1], vec!["C", "D"]);
    }

    #[test]
    fn test_render_markdown_format() {
        let candidate = TableCandidate {
            rows: vec![
                vec!["Col1".into(), "Col2".into()],
                vec!["val1".into(), "val2".into()],
            ],
            page: 1,
        };
        let el = render_table_element(&candidate);
        assert_eq!(el.kind, ElementKind::Table);
        assert_eq!(el.page, Some(1));
        let expected = "| Col1 | Col2 |\n| --- | --- |\n| val1 | val2 |";
        assert_eq!(el.text, expected, "got:\n{}", el.text);
    }

    #[test]
    fn test_extract_table_integration() {
        use crate::PdfParser;
        use fastrag_core::{FileFormat, Parser, SourceInfo};

        let pdf_bytes = include_bytes!("../../../tests/fixtures/sample_table.pdf");
        let parser = PdfParser;
        let source = SourceInfo::new(FileFormat::Pdf).with_filename("sample_table.pdf");
        let doc = parser.parse(pdf_bytes, &source).unwrap();

        let tables: Vec<_> = doc
            .elements
            .iter()
            .filter(|e| e.kind == ElementKind::Table)
            .collect();

        assert_eq!(
            tables.len(),
            1,
            "expected 1 table, got {}. All elements: {:?}",
            tables.len(),
            doc.elements
                .iter()
                .map(|e| (&e.kind, &e.text))
                .collect::<Vec<_>>()
        );

        let table_text = &tables[0].text;
        assert!(
            table_text.contains("| Name | Score | Grade |"),
            "table header missing, got:\n{}",
            table_text
        );
        assert!(
            table_text.contains("| Alice | 95 | A |"),
            "Alice row missing, got:\n{}",
            table_text
        );
    }
}
