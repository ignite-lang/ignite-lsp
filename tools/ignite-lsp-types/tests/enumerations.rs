//! Story lsp-server: a list of a closed enumeration skips the values it does not know (LSP
//! 3.17, "Enumerations"), and `--root-type` generates an enumeration no method reaches.

use crate::common::{PACKAGE, repo, scratch};

use std::process::Command;

use ignite_lsp_types::generate_with_types;

/// One request whose params hold a list of a closed enumeration and one value of it.
const TAGS: &str = r#"{
  "metaData": { "version": "3.17.0" },
  "requests": [
    {
      "method": "demo/tags",
      "params": { "kind": "reference", "name": "TagParams" },
      "result": { "kind": "base", "name": "null" }
    }
  ],
  "notifications": [],
  "structures": [
    {
      "name": "TagParams",
      "properties": [
        { "name": "valueSet", "type": { "kind": "array", "element": { "kind": "reference", "name": "Tag" } } },
        { "name": "kinds", "type": { "kind": "array", "element": { "kind": "reference", "name": "Kind" } } },
        { "name": "one", "type": { "kind": "reference", "name": "Tag" } }
      ]
    }
  ],
  "enumerations": [
    {
      "name": "Tag",
      "type": { "kind": "base", "name": "integer" },
      "values": [{ "name": "Unnecessary", "value": 1 }, { "name": "Deprecated", "value": 2 }]
    },
    {
      "name": "Kind",
      "type": { "kind": "base", "name": "string" },
      "values": [{ "name": "Plain", "value": "plain" }],
      "supportsCustomValues": true
    },
    {
      "name": "Codes",
      "type": { "kind": "base", "name": "integer" },
      "values": [{ "name": "NotReady", "value": -32002 }],
      "supportsCustomValues": true
    }
  ],
  "typeAliases": []
}"#;

fn generated(types: &[&str]) -> String {
    let types: Vec<String> = types.iter().map(|t| t.to_string()).collect();
    generate_with_types(TAGS, &["demo/tags".to_string()], &types, PACKAGE).unwrap()
}

/// The text of the Ignite fn `name` in `module`, up to the next blank line.
fn function<'m>(module: &'m str, name: &str) -> &'m str {
    let start = module
        .find(&format!("fn {name}("))
        .unwrap_or_else(|| panic!("no fn {name} in\n{module}"));
    let rest = &module[start..];
    &rest[..rest.find("\n\n").unwrap_or(rest.len())]
}

#[test]
fn a_list_of_a_closed_enumeration_skips_an_unknown_value() {
    let module = generated(&[]);
    let decode = function(&module, "decode_list_of_tag");
    assert!(decode.contains("match Tag.from_json(item) {"), "{decode}");
    assert!(decode.contains("Err(_) => {}"), "{decode}");
    assert!(!decode.contains('?'), "{decode}");
}

#[test]
fn a_single_closed_value_and_a_list_of_an_open_enumeration_stay_as_they_were() {
    let module = generated(&[]);
    assert!(module.contains("_ => Err(\"Tag: not one of its values\")"));
    let open = function(&module, "decode_list_of_kind");
    assert!(open.contains("out.push(Kind.from_json(item)"), "{open}");
}

#[test]
fn a_root_type_is_generated_and_named_in_the_header() {
    let without = generated(&[]);
    assert!(!without.contains("enum Codes"));
    assert!(!without.contains("// Root types:"));
    let with = generated(&["Codes"]);
    assert!(with.contains("enum Codes {"), "{with}");
    assert!(
        with.contains("  Json.Number(-32002) => Ok(Codes.NotReady)"),
        "{with}"
    );
    assert!(with.contains("// Root types: Codes.\n"));
}

#[test]
fn an_unknown_root_type_is_refused_by_name_exit_1() {
    let dir = scratch("root-type");
    let input = dir.join("metaModel.json");
    std::fs::write(&input, TAGS).unwrap();
    let out = dir.join("out.ignite");
    let output = Command::new(env!("CARGO_BIN_EXE_ignite-lsp-types"))
        .arg("--meta-model")
        .arg(&input)
        .args(["--root", "demo/tags", "--root-type", "Nowhere", "--out"])
        .arg(&out)
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert!(stderr.contains("`Nowhere`"), "{stderr}");
    assert!(!out.exists());
}

#[test]
fn the_vendored_error_codes_are_generated() {
    let text = std::fs::read_to_string(repo().join("examples/lsp/lsp_types.ignite")).unwrap();
    assert!(text.contains("  Json.Number(-32002) => Ok(ErrorCodes.ServerNotInitialized)"));
    assert!(text.contains("  Json.Number(-32601) => Ok(ErrorCodes.MethodNotFound)"));
    assert!(text.contains("enum LSPErrorCodes {"));
}
