use std::collections::HashSet;
use std::fs;
use std::path::Path;

use serde::Deserialize;

use crate::{EvalDataset, EvalDocument, EvalError, EvalQuery, EvalResult, Qrel};

use super::common::{
    cache_root, download_to_path, extract_zip, file_name_from_url, find_path, read_jsonl,
    sha256_file,
};

pub fn load(name: &str, url: &str, sha256: &str) -> EvalResult<EvalDataset> {
    let root = cache_root(name)?;
    let archive_name = file_name_from_url(url)?;
    let archive_path = root.join(&archive_name);
    let extract_dir = root.join("extracted");

    ensure_beir_archive(&archive_path, url, sha256)?;
    if !has_beir_split(&extract_dir)? {
        if extract_dir.exists() {
            fs::remove_dir_all(&extract_dir)?;
        }
        extract_zip(&archive_path, &extract_dir)?;
    }

    load_from_dir(name, &extract_dir)
}

pub(crate) fn load_from_dir(name: &str, path: &Path) -> EvalResult<EvalDataset> {
    let corpus_path = find_path(path, Path::new("corpus.jsonl"))?;
    let queries_path = find_path(path, Path::new("queries.jsonl"))?;
    let qrels_path = find_path(path, Path::new("qrels/test.tsv"))?;

    let corpus_rows: Vec<BeirCorpusRow> = read_jsonl(&corpus_path)?;
    let mut documents = Vec::with_capacity(corpus_rows.len());
    let mut corpus_ids = HashSet::new();
    for row in corpus_rows {
        let id = row.id;
        corpus_ids.insert(id.clone());
        documents.push(EvalDocument {
            id,
            title: row
                .title
                .map(|title| title.trim().to_string())
                .filter(|title| !title.is_empty()),
            text: row.text.trim().to_string(),
        });
    }

    let qrels = read_qrels(&qrels_path)?;
    let query_ids: HashSet<String> = qrels.iter().map(|qrel| qrel.query_id.clone()).collect();
    let qrel_ids: HashSet<String> = qrels.iter().map(|qrel| qrel.doc_id.clone()).collect();

    let query_rows: Vec<BeirQueryRow> = read_jsonl(&queries_path)?;
    let mut queries = Vec::new();
    let mut seen_queries = HashSet::new();
    for row in query_rows {
        if query_ids.contains(&row.id) {
            seen_queries.insert(row.id.clone());
            queries.push(EvalQuery {
                id: row.id,
                text: row.text.trim().to_string(),
            });
        }
    }

    if !query_ids.is_subset(&seen_queries) {
        let mut missing: Vec<_> = query_ids.difference(&seen_queries).cloned().collect();
        missing.sort();
        return Err(EvalError::MalformedDataset(format!(
            "{name}: qrels reference missing queries: {:?}",
            missing
        )));
    }
    if !qrel_ids.is_subset(&corpus_ids) {
        let mut missing: Vec<_> = qrel_ids.difference(&corpus_ids).cloned().collect();
        missing.sort();
        return Err(EvalError::MalformedDataset(format!(
            "{name}: qrels reference missing corpus ids: {:?}",
            missing
        )));
    }

    Ok(EvalDataset {
        name: name.to_string(),
        documents,
        queries,
        qrels,
    })
}

fn ensure_beir_archive(archive_path: &Path, url: &str, expected_sha256: &str) -> EvalResult<()> {
    if archive_path.exists() {
        let got = sha256_file(archive_path)?;
        if got == expected_sha256 {
            return Ok(());
        }
        fs::remove_file(archive_path)?;
    }
    download_to_path(url, archive_path, Some(expected_sha256))
}

fn has_beir_split(path: &Path) -> EvalResult<bool> {
    if !path.exists() {
        return Ok(false);
    }
    let corpus = find_path(path, Path::new("corpus.jsonl"));
    let queries = find_path(path, Path::new("queries.jsonl"));
    let qrels = find_path(path, Path::new("qrels/test.tsv"));
    Ok(corpus.is_ok() && queries.is_ok() && qrels.is_ok())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BeirCorpusRow {
    #[serde(rename = "_id")]
    id: String,
    title: Option<String>,
    text: String,
    #[serde(default, rename = "metadata")]
    _metadata: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BeirQueryRow {
    #[serde(rename = "_id")]
    id: String,
    text: String,
    #[serde(default, rename = "metadata")]
    _metadata: Option<serde_json::Value>,
}

fn read_qrels(path: &Path) -> EvalResult<Vec<Qrel>> {
    let contents = fs::read_to_string(path)?;
    let mut lines = contents.lines();
    let header = lines.next().ok_or_else(|| {
        EvalError::MalformedDataset(format!("empty qrels file: {}", path.display()))
    })?;
    if header.trim() != "query-id\tcorpus-id\tscore" {
        return Err(EvalError::MalformedDataset(format!(
            "unexpected qrels header in {}: {header}",
            path.display()
        )));
    }

    let mut qrels = Vec::new();
    for (line_no, line) in lines.enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let columns: Vec<_> = line.split('\t').collect();
        if columns.len() != 3 {
            return Err(EvalError::MalformedDataset(format!(
                "invalid qrels row {} in {}: {line}",
                line_no + 2,
                path.display()
            )));
        }
        let relevance = columns[2].parse::<u32>().map_err(|err| {
            EvalError::MalformedDataset(format!(
                "invalid qrels relevance in {} row {}: {}",
                path.display(),
                line_no + 2,
                err
            ))
        })?;
        qrels.push(Qrel {
            query_id: columns[0].to_string(),
            doc_id: columns[1].to_string(),
            relevance,
        });
    }

    Ok(qrels)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture_dir(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/datasets")
            .join(name)
    }

    #[test]
    fn beir_loader_uses_qrels_test_split() {
        let dataset = load_from_dir("scifact", &fixture_dir("scifact_mini")).unwrap();
        assert_eq!(dataset.queries.len(), 3);
        assert_eq!(dataset.qrels.len(), 3);
        assert_eq!(dataset.documents.len(), 5);
        assert_eq!(dataset.queries[0].id, "0");
    }

    #[test]
    fn qrels_parser_rejects_wrong_header() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.tsv");
        fs::write(&path, "bad\theader\n").unwrap();
        let err = read_qrels(&path).unwrap_err();
        assert!(err.to_string().contains("unexpected qrels header"));
    }
}
