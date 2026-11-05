use std::path::{Path, PathBuf};

/// The repository root.
pub fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

/// The roots of `task lsp-types` (story:lsp-types).
pub const ROOTS: &[&str] = &[
    "initialize",
    "initialized",
    "shutdown",
    "exit",
    "textDocument/didOpen",
    "textDocument/didChange",
    "textDocument/didClose",
    "textDocument/publishDiagnostics",
    "$/cancelRequest",
];

pub const PACKAGE: &str = "examples.lsp.lsp_types";

pub fn roots() -> Vec<String> {
    ROOTS.iter().map(|r| r.to_string()).collect()
}

pub fn meta_model() -> String {
    std::fs::read_to_string(repo().join("docs/lsp/metaModel.json")).unwrap()
}

/// A scratch directory under the crate's target directory, emptied.
pub fn scratch(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
