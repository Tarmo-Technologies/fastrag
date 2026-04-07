use fastrag_eval::{load_cwe_top25, load_nfcorpus, load_nvd, load_scifact};

fn download_enabled() -> bool {
    std::env::var("FASTRAG_EVAL_DOWNLOAD").ok().as_deref() == Some("1")
}

#[test]
#[ignore]
fn nfcorpus_download_path_loads() {
    if !download_enabled() {
        return;
    }

    let dataset = load_nfcorpus().unwrap();
    assert_eq!(dataset.name, "nfcorpus");
    assert_eq!(dataset.documents.len(), 3633);
    assert_eq!(dataset.queries.len(), 323);
    assert_eq!(dataset.qrels.len(), 12334);
    assert_eq!(dataset.documents[0].id, "MED-10");
}

#[test]
#[ignore]
fn scifact_download_path_loads() {
    if !download_enabled() {
        return;
    }

    let dataset = load_scifact().unwrap();
    assert_eq!(dataset.name, "scifact");
    assert_eq!(dataset.documents.len(), 5183);
    assert_eq!(dataset.queries.len(), 300);
    assert_eq!(dataset.qrels.len(), 339);
    assert_eq!(dataset.documents[0].id, "4983");
}

#[test]
#[ignore]
fn nvd_download_path_loads() {
    if !download_enabled() {
        return;
    }

    let dataset = load_nvd().unwrap();
    assert_eq!(dataset.name, "nvd");
    assert!(dataset.documents.len() > 40_000);
    assert!(dataset.queries.is_empty());
    assert!(dataset.qrels.is_empty());
    assert!(dataset.documents[0].id.starts_with("CVE-"));
}

#[test]
#[ignore]
fn cwe_download_path_loads() {
    if !download_enabled() {
        return;
    }

    let dataset = load_cwe_top25().unwrap();
    assert_eq!(dataset.name, "cwe");
    assert_eq!(dataset.documents.len(), 25);
    assert!(dataset.queries.is_empty());
    assert!(dataset.qrels.is_empty());
    assert_eq!(dataset.documents[0].id, "CWE-79");
}
