//! Story lsp-server, acceptance 5: `examples/lsp/server.ignite` built as `ignite-lsp` on each
//! target and driven over stdio, with this build's `ignite` as its compiler (`--ignite`). A
//! buffer with a type error publishes one E040 at the range of the offending text, in UTF-16
//! code units; the fixed buffer publishes an empty list; `shutdown` then `exit` ends with
//! status 0. The inline tests of `server.ignite` cover the rest of the story over a pipe.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use ignite::Target;

use crate::common;

const TIMEOUT: Duration = Duration::from_secs(60);

fn build(target: Target) -> PathBuf {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/lsp/server.ignite");
    let tmp = Path::new(env!("CARGO_TARGET_TMPDIR"));
    let options = match target {
        Target::Rust => ignite::BuildOptions {
            target: Target::Rust,
            out: Some(tmp.join("lsp")),
            ..Default::default()
        },
        Target::Llvm => ignite::BuildOptions {
            target: Target::Llvm,
            out: Some(tmp.join("lsp-llvm")),
            runtime: Some(common::runtime_archive()),
            ..Default::default()
        },
    };
    let output = ignite::build(&source, &options).unwrap_or_else(|e| panic!("{e}"));
    output
        .executables
        .iter()
        .find(|path| path.file_name().and_then(|s| s.to_str()) == Some("ignite-lsp"))
        .unwrap_or_else(|| panic!("no ignite-lsp in {:?}", output.executables))
        .clone()
}

fn server(target: Target) -> &'static Path {
    static BUILT: [OnceLock<PathBuf>; 2] = [OnceLock::new(), OnceLock::new()];
    let cell = match target {
        Target::Rust => &BUILT[0],
        Target::Llvm => &BUILT[1],
    };
    cell.get_or_init(|| build(target))
}

fn framed(body: &str) -> Vec<u8> {
    let mut out = format!("Content-Length: {}\r\n\r\n", body.len()).into_bytes();
    out.extend_from_slice(body.as_bytes());
    out
}

/// The bodies of the framed messages in `bytes`.
fn bodies(mut bytes: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    while !bytes.is_empty() {
        let end = bytes
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .unwrap_or_else(|| panic!("no header end in {:?}", String::from_utf8_lossy(bytes)));
        let header = std::str::from_utf8(&bytes[..end]).unwrap();
        let length: usize = header
            .strip_prefix("Content-Length: ")
            .and_then(|n| n.parse().ok())
            .unwrap_or_else(|| panic!("header {header:?}"));
        let body = &bytes[end + 4..end + 4 + length];
        out.push(String::from_utf8(body.to_vec()).unwrap());
        bytes = &bytes[end + 4 + length..];
    }
    out
}

fn json_string(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn diagnostics_round_trip(target: Target) {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("lsp-e2e-{target:?}"));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("a.ignite");
    let uri = format!("file://{}", path.display());
    let broken = "package a\n\nfn main() -> Int = \"é😀\"\n";
    let fixed = "package a\n\nfn main() -> Int = 1\n";
    let messages = [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"processId":null,"rootUri":null,"capabilities":{"general":{"positionEncodings":["utf-16"]}}}}"#.to_string(),
        r#"{"jsonrpc":"2.0","method":"initialized","params":{}}"#.to_string(),
        format!(
            r#"{{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{{"textDocument":{{"uri":{},"languageId":"ignite","version":1,"text":{}}}}}}}"#,
            json_string(&uri),
            json_string(broken)
        ),
        format!(
            r#"{{"jsonrpc":"2.0","method":"textDocument/didChange","params":{{"textDocument":{{"uri":{},"version":2}},"contentChanges":[{{"text":{}}}]}}}}"#,
            json_string(&uri),
            json_string(fixed)
        ),
        r#"{"jsonrpc":"2.0","id":2,"method":"shutdown"}"#.to_string(),
        r#"{"jsonrpc":"2.0","method":"exit"}"#.to_string(),
    ];
    let mut child = Command::new(server(target))
        .arg("--ignite")
        .arg(env!("CARGO_BIN_EXE_ignite"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    for message in &messages {
        stdin.write_all(&framed(message)).unwrap();
    }
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if started.elapsed() > TIMEOUT {
            let _ = child.kill();
            panic!("ignite-lsp did not exit within {TIMEOUT:?}");
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    drop(stdin);
    let mut stdout = Vec::new();
    child
        .stdout
        .take()
        .unwrap()
        .read_to_end(&mut stdout)
        .unwrap();
    let mut stderr = String::new();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut stderr)
        .unwrap();
    assert_eq!(status.code(), Some(0), "stderr: {stderr}");
    let replies = bodies(&stdout);
    let uri_json = json_string(&uri);
    assert_eq!(
        replies,
        [
            r#"{"jsonrpc":"2.0","id":1,"result":{"capabilities":{"positionEncoding":"utf-16","textDocumentSync":{"openClose":true,"change":1}},"serverInfo":{"name":"ignite-lsp"}}}"#.to_string(),
            format!(
                r#"{{"jsonrpc":"2.0","method":"textDocument/publishDiagnostics","params":{{"uri":{uri_json},"version":1,"diagnostics":[{{"range":{{"start":{{"line":2,"character":19}},"end":{{"line":2,"character":24}}}},"severity":1,"code":"E040","source":"ignite","message":"type mismatch: expected `Int`, found `Text`"}}]}}}}"#
            ),
            format!(
                r#"{{"jsonrpc":"2.0","method":"textDocument/publishDiagnostics","params":{{"uri":{uri_json},"version":2,"diagnostics":[]}}}}"#
            ),
            r#"{"jsonrpc":"2.0","id":2,"result":null}"#.to_string(),
        ],
        "stderr: {stderr}"
    );
}

macro_rules! both_targets {
    ($($case:ident),* $(,)?) => {
        mod rust {
            $(#[test]
            fn $case() {
                super::$case(ignite::Target::Rust);
            })*
        }
        mod llvm {
            $(#[test]
            fn $case() {
                super::$case(ignite::Target::Llvm);
            })*
        }
    };
}

both_targets!(diagnostics_round_trip);
