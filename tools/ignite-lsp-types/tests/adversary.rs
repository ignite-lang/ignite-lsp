//! Adversary cases (review of story:lsp-types): meta models the generator must refuse by
//! name, exit 1, instead of crashing.

use crate::common::scratch;

use std::process::Command;

/// A request whose params are the type alias `Loop`, defined as `Loop[]`: no `or` stands in
/// the cycle, so no union is registered before the alias is lowered again.
const SELF_ALIAS: &str = r#"{
  "metaData": { "version": "3.17.0" },
  "requests": [
    {
      "method": "demo/loop",
      "params": { "kind": "reference", "name": "Loop" },
      "result": { "kind": "base", "name": "null" }
    }
  ],
  "notifications": [],
  "structures": [],
  "enumerations": [],
  "typeAliases": [
    { "name": "Loop", "type": { "kind": "array", "element": { "kind": "reference", "name": "Loop" } } }
  ]
}"#;

/// Two type aliases that name each other.
const MUTUAL_ALIAS: &str = r#"{
  "metaData": { "version": "3.17.0" },
  "requests": [
    {
      "method": "demo/loop",
      "params": { "kind": "reference", "name": "Ping" },
      "result": { "kind": "base", "name": "null" }
    }
  ],
  "notifications": [],
  "structures": [],
  "enumerations": [],
  "typeAliases": [
    { "name": "Ping", "type": { "kind": "reference", "name": "Pong" } },
    { "name": "Pong", "type": { "kind": "reference", "name": "Ping" } }
  ]
}"#;

fn run(name: &str, meta_model: &str) -> (Option<i32>, String) {
    let dir = scratch(&format!("adversary-{name}"));
    let input = dir.join("metaModel.json");
    std::fs::write(&input, meta_model).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ignite-lsp-types"))
        .arg("--meta-model")
        .arg(&input)
        .arg("--root")
        .arg("demo/loop")
        .arg("--out")
        .arg(dir.join("out.ignite"))
        .output()
        .unwrap();
    (
        output.status.code(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

#[test]
fn a_type_alias_that_contains_itself_is_refused_not_a_stack_overflow() {
    let (code, stderr) = run("self-alias", SELF_ALIAS);
    assert_eq!(code, Some(1), "{stderr}");
    assert!(stderr.contains("Loop"), "{stderr}");
}

#[test]
fn two_type_aliases_naming_each_other_are_refused_not_a_stack_overflow() {
    let (code, stderr) = run("mutual-alias", MUTUAL_ALIAS);
    assert_eq!(code, Some(1), "{stderr}");
    assert!(
        stderr.contains("Ping") || stderr.contains("Pong"),
        "{stderr}"
    );
}
