//! Story acceptance 3: a root that reaches a type the generator cannot map makes it exit
//! non-zero, naming the type; nothing is written.

use crate::common::{PACKAGE, scratch};

use std::process::Command;

use ignite_lsp_types::{Error, generate};

/// A meta model of one request whose params hold a `tuple`, which has no mapping.
const TUPLE: &str = r#"{
  "metaData": { "version": "3.17.0" },
  "requests": [
    {
      "method": "demo/range",
      "params": { "kind": "reference", "name": "RangeParams" },
      "result": { "kind": "base", "name": "null" }
    }
  ],
  "notifications": [],
  "structures": [
    {
      "name": "RangeParams",
      "properties": [
        { "name": "label", "type": { "kind": "base", "name": "string" } },
        {
          "name": "span",
          "type": {
            "kind": "tuple",
            "items": [{ "kind": "base", "name": "uinteger" }, { "kind": "base", "name": "uinteger" }]
          }
        }
      ]
    }
  ],
  "enumerations": [],
  "typeAliases": []
}"#;

fn run(name: &str, meta_model: &str, roots: &[&str]) -> (i32, String, bool) {
    let dir = scratch(&format!("refusal-{name}"));
    let input = dir.join("metaModel.json");
    std::fs::write(&input, meta_model).unwrap();
    let out = dir.join("out.ignite");
    let mut command = Command::new(env!("CARGO_BIN_EXE_ignite-lsp-types"));
    command
        .arg("--meta-model")
        .arg(&input)
        .arg("--out")
        .arg(&out);
    for root in roots {
        command.arg("--root").arg(root);
    }
    let output = command.output().unwrap();
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stderr).into_owned(),
        out.exists(),
    )
}

#[test]
fn an_unmappable_type_exits_non_zero_naming_it() {
    let (code, stderr, written) = run("tuple", TUPLE, &["demo/range"]);
    assert_eq!(code, 1, "{stderr}");
    assert!(stderr.contains("tuple"), "{stderr}");
    assert!(stderr.contains("RangeParams.span"), "{stderr}");
    assert!(!written, "a refused run writes nothing");
}

#[test]
fn an_unknown_reference_is_refused_with_its_name() {
    let model = TUPLE.replace(
        r#""params": { "kind": "reference", "name": "RangeParams" }"#,
        r#""params": { "kind": "reference", "name": "MissingParams" }"#,
    );
    match generate(&model, &["demo/range".to_string()], PACKAGE) {
        Err(Error::Refused(message)) => assert!(message.contains("MissingParams"), "{message}"),
        other => panic!("expected a refusal, got {other:?}"),
    }
}

#[test]
fn an_unknown_root_is_refused() {
    let (code, stderr, _) = run("root", TUPLE, &["demo/nothing"]);
    assert_eq!(code, 1, "{stderr}");
    assert!(stderr.contains("demo/nothing"), "{stderr}");
}

#[test]
fn a_type_no_root_reaches_is_not_refused() {
    let model = TUPLE.replace(
        r#""params": { "kind": "reference", "name": "RangeParams" }"#,
        r#""params": null"#,
    );
    let module = generate(&model, &["demo/range".to_string()], PACKAGE).unwrap();
    assert!(module.contains("const METHOD_DEMO_RANGE: Text = \"demo/range\""));
    assert!(!module.contains("RangeParams"));
}

#[test]
fn a_malformed_meta_model_exits_2() {
    let (code, stderr, written) = run("malformed", "{\"requests\": [", &["demo/range"]);
    assert_eq!(code, 2, "{stderr}");
    assert!(!written);
}
