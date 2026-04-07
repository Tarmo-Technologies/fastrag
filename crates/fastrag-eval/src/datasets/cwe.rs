use std::collections::HashSet;
use std::fs;
use std::io::BufReader;
use std::path::{Path, PathBuf};

use quick_xml::Reader;
use quick_xml::events::Event;

use crate::{EvalDataset, EvalDocument, EvalError, EvalResult};

use super::common::{cache_root, download_to_path, extract_zip, file_name_from_url, sha256_file};

const CWE_URL: &str = "https://cwe.mitre.org/data/xml/cwec_latest.xml.zip";

const CWE_TOP25: &[&str] = &[
    "79", "89", "787", "20", "125", "78", "416", "22", "352", "434", "862", "476", "287", "190",
    "502", "77", "119", "798", "918", "306", "362", "269", "94", "863", "276",
];

pub fn load_cwe_top25() -> EvalResult<EvalDataset> {
    let root = cache_root("cwe")?;
    let archive_name = file_name_from_url(CWE_URL)?;
    let archive_path = root.join(&archive_name);
    let extract_dir = root.join("extracted");

    ensure_cwe_archive(&archive_path)?;
    if !has_xml(&extract_dir)? {
        if extract_dir.exists() {
            fs::remove_dir_all(&extract_dir)?;
        }
        extract_zip(&archive_path, &extract_dir)?;
    }

    let xml_path = find_xml_file(&extract_dir)?;
    load_from(&xml_path)
}

fn load_from(path: &Path) -> EvalResult<EvalDataset> {
    let file = fs::File::open(path)?;
    let mut reader = Reader::from_reader(BufReader::new(file));
    reader.config_mut().trim_text(true);

    let top25: HashSet<&str> = CWE_TOP25.iter().copied().collect();
    let mut buf = Vec::new();
    let mut documents = Vec::new();
    let mut current: Option<CweWeakness> = None;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) if tag_matches(e.name().as_ref(), b"Weakness") => {
                let id = read_attr(e, b"ID").unwrap_or_default();
                if top25.contains(id.as_str()) {
                    current = Some(CweWeakness {
                        id: format!("CWE-{id}"),
                        title: read_attr(e, b"Name").unwrap_or_default(),
                        description: String::new(),
                        extended_description: String::new(),
                    });
                } else {
                    current = None;
                }
            }
            Ok(Event::Start(ref e))
                if current.is_some() && tag_matches(e.name().as_ref(), b"Name") =>
            {
                let text = collect_text(&mut reader, e.name().as_ref())?;
                if let Some(current) = current.as_mut()
                    && current.title.is_empty()
                {
                    current.title = text;
                }
            }
            Ok(Event::Start(ref e))
                if current.is_some() && tag_matches(e.name().as_ref(), b"Description") =>
            {
                let text = collect_text(&mut reader, e.name().as_ref())?;
                if let Some(current) = current.as_mut() {
                    current.description = text;
                }
            }
            Ok(Event::Start(ref e))
                if current.is_some() && tag_matches(e.name().as_ref(), b"Extended_Description") =>
            {
                let text = collect_text(&mut reader, e.name().as_ref())?;
                if let Some(current) = current.as_mut() {
                    current.extended_description = text;
                }
            }
            Ok(Event::End(ref e)) if tag_matches(e.name().as_ref(), b"Weakness") => {
                if let Some(current) = current.take() {
                    let mut text = current.description.trim().to_string();
                    let extended = current.extended_description.trim().to_string();
                    if !extended.is_empty() {
                        if !text.is_empty() {
                            text.push('\n');
                        }
                        text.push_str(&extended);
                    }
                    documents.push(EvalDocument {
                        id: current.id,
                        title: Some(current.title.trim().to_string()),
                        text,
                    });
                }
            }
            Ok(Event::Eof) => break,
            Err(err) => return Err(EvalError::Xml(err.to_string())),
            _ => {}
        }
        buf.clear();
    }

    Ok(EvalDataset {
        name: "cwe".to_string(),
        documents,
        queries: Vec::new(),
        qrels: Vec::new(),
    })
}

fn ensure_cwe_archive(archive_path: &Path) -> EvalResult<()> {
    let checksum_path = checksum_sidecar(archive_path);
    if archive_path.exists() {
        if checksum_path.exists() {
            let expected = fs::read_to_string(&checksum_path)?.trim().to_string();
            let actual = sha256_file(archive_path)?;
            if actual == expected {
                return Ok(());
            }
            fs::remove_file(archive_path)?;
        } else {
            let digest = sha256_file(archive_path)?;
            fs::write(&checksum_path, format!("{digest}\n"))?;
            return Ok(());
        }
    }

    download_to_path(CWE_URL, archive_path, None)?;
    let digest = sha256_file(archive_path)?;
    fs::write(&checksum_path, format!("{digest}\n"))?;
    Ok(())
}

fn checksum_sidecar(path: &Path) -> PathBuf {
    let file_name = path.file_name().expect("archive file name");
    path.with_file_name(format!("{}.sha256", file_name.to_string_lossy()))
}

fn has_xml(path: &Path) -> EvalResult<bool> {
    if !path.exists() {
        return Ok(false);
    }
    find_xml_file(path).map(|_| true).or_else(|err| match err {
        EvalError::MalformedDataset(_) => Ok(false),
        other => Err(other),
    })
}

fn find_xml_file(root: &Path) -> EvalResult<PathBuf> {
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(".xml"))
            {
                return Ok(path);
            }
        }
    }

    Err(EvalError::MalformedDataset(format!(
        "could not find extracted CWE XML under {}",
        root.display()
    )))
}

fn read_attr(e: &quick_xml::events::BytesStart<'_>, name: &[u8]) -> Option<String> {
    e.attributes().flatten().find_map(|attr| {
        if attr.key.as_ref() == name {
            Some(String::from_utf8_lossy(&attr.value).to_string())
        } else {
            None
        }
    })
}

fn tag_matches(name: &[u8], local: &[u8]) -> bool {
    local_name(name) == local
}

fn local_name(name: &[u8]) -> &[u8] {
    match name.iter().rposition(|byte| *byte == b':') {
        Some(index) => &name[index + 1..],
        None => name,
    }
}

fn collect_text(reader: &mut Reader<BufReader<fs::File>>, end: &[u8]) -> EvalResult<String> {
    let mut buf = Vec::new();
    let mut text = String::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Text(ref e)) => {
                text.push_str(&e.unescape().unwrap_or_default());
            }
            Ok(Event::CData(ref e)) => {
                text.push_str(&String::from_utf8_lossy(e.as_ref()));
            }
            Ok(Event::End(ref e)) if e.name().as_ref() == end => break,
            Ok(Event::Eof) => {
                return Err(EvalError::Xml(format!(
                    "unexpected EOF while reading {}",
                    String::from_utf8_lossy(end)
                )));
            }
            Err(err) => return Err(EvalError::Xml(err.to_string())),
            _ => {}
        }
        buf.clear();
    }
    Ok(text)
}

#[derive(Debug)]
struct CweWeakness {
    id: String,
    title: String,
    description: String,
    extended_description: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_path() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/datasets/cwe_mini.xml")
    }

    #[test]
    fn loads_offline_fixture() {
        let dataset = load_from(&fixture_path()).unwrap();
        assert_eq!(dataset.name, "cwe");
        assert_eq!(dataset.documents.len(), 3);
        assert_eq!(dataset.queries.len(), 0);
        assert_eq!(dataset.qrels.len(), 0);
        assert_eq!(dataset.documents[0].id, "CWE-79");
        assert_eq!(
            dataset.documents[1].title,
            Some("Improper Input Validation".to_string())
        );
        assert!(
            dataset.documents[2]
                .text
                .contains("Improper neutralization of array indices")
        );
    }
}
