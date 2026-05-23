use std::fs;
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crate should have workspace parent")
        .parent()
        .expect("workspace should have repository parent")
        .to_path_buf()
}

#[test]
fn install_transaction_doc_names_public_surface_and_atomicity() {
    let root = repo_root();
    let doc = fs::read_to_string(root.join("docs/behaviors/install/transaction.md"))
        .expect("read install transaction behavior doc");

    for expected in [
        "curl -fsSL https://github.com/moradology/m80/releases/latest/download/install.sh | sudo sh",
        "curl -fsSL https://github.com/moradology/m80/releases/download/<version>/install.sh | sudo sh",
        "`latest` is only a resolver",
        "Privilege is only for final host writes",
        "The active pointer flips last",
        "Same-version reinstall is",
        "Downgrades,",
        "below-safety-floor targets are refused",
        "`m80 install-status` is the local repair view",
        "../release/install-finalization-transaction.md",
    ] {
        assert!(
            doc.contains(expected),
            "install transaction doc must contain {expected:?}"
        );
    }
}
