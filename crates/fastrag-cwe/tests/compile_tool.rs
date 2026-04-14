//! Test the XML → closure-JSON transformation used by `compile-taxonomy`.

#![cfg(feature = "compile-tool")]

use std::path::PathBuf;

#[test]
fn builds_closure_from_mini_fixture() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mini_cwe.xml");
    let bytes = std::fs::read(&fixture).expect("fixture exists");

    let taxonomy =
        fastrag_cwe::compile::build_closure(&bytes, "1000").expect("build_closure succeeds");

    assert_eq!(taxonomy.version(), "4.16-test");
    assert_eq!(taxonomy.view(), "1000");

    let c74 = taxonomy.expand(74);
    assert!(c74.contains(&74));
    assert!(c74.contains(&943));
    assert!(c74.contains(&89));
    assert!(c74.contains(&564));

    let c89 = taxonomy.expand(89);
    assert_eq!(c89.len(), 2, "expand(89) should include 89 and 564");
    assert!(c89.contains(&89));
    assert!(c89.contains(&564));

    let c564 = taxonomy.expand(564);
    assert_eq!(c564, vec![564]);
}
