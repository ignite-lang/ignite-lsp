//! The committed module is the generator's output: regenerating it from the vendored meta
//! model gives the same bytes. `IGNITE_BLESS=1 cargo test -p ignite-lsp-types --test main
//! drift::` rewrites it (or run `task lsp-types`).

use crate::common::{PACKAGE, ROOT_TYPES, ROOTS, meta_model, repo, root_types, roots, scratch};

use std::process::Command;

use ignite_lsp_types::generate_with_types;

const MODULE: &str = "src/lsp_types.ignite";

#[test]
fn the_committed_module_is_the_generators_output() {
    let generated = generate_with_types(&meta_model(), &roots(), &root_types(), PACKAGE).unwrap();
    let path = repo().join(MODULE);
    if std::env::var_os("IGNITE_BLESS").is_some() {
        std::fs::write(&path, &generated).unwrap();
        return;
    }
    let committed = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{}: {e}; bless with IGNITE_BLESS=1", path.display()));
    assert!(
        committed == generated,
        "{MODULE} differs from the generator's output; regenerate with `task lsp-types`"
    );
}

#[test]
fn the_binary_writes_what_the_library_generates() {
    let dir = scratch("binary");
    let out = dir.join("lsp_types.ignite");
    let mut command = Command::new(env!("CARGO_BIN_EXE_ignite-lsp-types"));
    command
        .arg("--meta-model")
        .arg(repo().join("docs/lsp/metaModel.json"))
        .arg("--out")
        .arg(&out);
    for root in ROOTS {
        command.arg("--root").arg(root);
    }
    for name in ROOT_TYPES {
        command.arg("--root-type").arg(name);
    }
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let written = std::fs::read_to_string(&out).unwrap();
    assert_eq!(
        written,
        generate_with_types(&meta_model(), &roots(), &root_types(), PACKAGE).unwrap()
    );
}

#[test]
fn the_header_names_the_source_digest_and_the_roots() {
    let text = meta_model();
    let generated = generate_with_types(&text, &roots(), &root_types(), PACKAGE).unwrap();
    let digest = ignite_lsp_types::sha256_hex(&text);
    assert!(generated.contains(&format!("sha256 {digest}")));
    assert!(generated.contains(&format!("// Roots: {}.", ROOTS.join(", "))));
    assert!(generated.contains(&format!("// Root types: {}.", ROOT_TYPES.join(", "))));
}

/// Story acceptance 4: one changed field in a copy of the meta model changes the output, so
/// the drift test above would fail on it.
#[test]
fn changing_one_field_of_the_meta_model_changes_the_output() {
    let text = meta_model();
    let needle = "\"name\": \"processId\"";
    assert_eq!(text.matches(needle).count(), 1, "the needle must be unique");
    let changed = text.replace(needle, "\"name\": \"processIdentifier\"");
    let before = generate_with_types(&text, &roots(), &root_types(), PACKAGE).unwrap();
    let after = generate_with_types(&changed, &roots(), &root_types(), PACKAGE).unwrap();
    assert_ne!(before, after);
    assert!(before.contains("  process_id: Option<Int>\n"));
    assert!(after.contains("  process_identifier: Option<Int>\n"));
    assert!(after.contains("\"processIdentifier\""));
    assert!(!after.contains("\"processId\""));
}
