use tantivy::schema::{
    FAST, Field, INDEXED, NumericOptions, STORED, STRING, Schema, SchemaBuilder, TextFieldIndexing,
    TextOptions,
};

/// Field handles for the Tantivy schema.
#[derive(Clone, Debug)]
pub struct FieldSet {
    pub id: Field,
    /// BM25-indexed body text. Holds the contextualized form (context prefix
    /// plus raw chunk) when Contextual Retrieval is enabled, otherwise the
    /// raw chunk text.
    pub chunk_text: Field,
    /// Raw chunk text preserved verbatim for display and CVE/CWE exact-match
    /// lookup. Stored but not indexed — the BM25 body already covers
    /// retrieval.
    pub display_text: Field,
    pub source_path: Field,
    pub section: Field,
    pub cve_id: Field,
    pub cwe: Field,
    pub metadata_json: Field,
}

/// Build the Tantivy schema for a FastRAG corpus.
pub fn build_schema() -> (Schema, FieldSet) {
    let mut builder = SchemaBuilder::new();

    let id = builder.add_u64_field("id", NumericOptions::default() | INDEXED | STORED | FAST);

    let text_options = TextOptions::default()
        .set_indexing_options(
            TextFieldIndexing::default()
                .set_tokenizer("default")
                .set_index_option(tantivy::schema::IndexRecordOption::WithFreqsAndPositions),
        )
        .set_stored();
    let chunk_text = builder.add_text_field("chunk_text", text_options);

    // Raw chunk text preserved verbatim — stored but not indexed. Serves the
    // display path and CVE/CWE exact-match regex. Stored-only avoids double-
    // indexing the same words twice when contextualization is disabled.
    let display_text = builder.add_text_field("display_text", STORED);

    let source_path = builder.add_text_field("source_path", STRING | STORED);
    let section = builder.add_text_field("section", STORED);

    // Security identifier fields — keyword (exact match), indexed for term queries.
    let cve_id = builder.add_text_field("cve_id", STRING | STORED);
    let cwe = builder.add_text_field("cwe", STRING | STORED);

    // Arbitrary metadata serialized as JSON — stored but not indexed.
    let metadata_json = builder.add_text_field("metadata_json", STORED);

    let schema = builder.build();

    let fields = FieldSet {
        id,
        chunk_text,
        display_text,
        source_path,
        section,
        cve_id,
        cwe,
        metadata_json,
    };

    (schema, fields)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_builds_with_all_fields() {
        let (schema, fields) = build_schema();

        assert_eq!(schema.get_field_name(fields.id), "id");
        assert_eq!(schema.get_field_name(fields.chunk_text), "chunk_text");
        assert_eq!(schema.get_field_name(fields.display_text), "display_text");
        assert_eq!(schema.get_field_name(fields.source_path), "source_path");
        assert_eq!(schema.get_field_name(fields.section), "section");
        assert_eq!(schema.get_field_name(fields.cve_id), "cve_id");
        assert_eq!(schema.get_field_name(fields.cwe), "cwe");
        assert_eq!(schema.get_field_name(fields.metadata_json), "metadata_json");
    }

    #[test]
    fn schema_has_eight_fields() {
        let (schema, _) = build_schema();
        // Count fields by iterating
        let count = schema.fields().count();
        assert_eq!(count, 8);
    }
}
