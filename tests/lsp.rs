//! Story lsp-server, acceptance 5: `examples/lsp/server.ignite` built as `ignite-lsp` on each
//! target and driven over stdio, with this build's `ignite` as its compiler (`--ignite`). A
//! buffer with a type error after multibyte characters on its line publishes one E040 at the
//! range of the offending text, in UTF-16 code units by default and in bytes once UTF-8 is
//! negotiated; the fixed buffer publishes an empty list; `shutdown` then `exit` ends with
//! status 0. An output past a pipe buffer (2,000 diagnostics) is read while the server runs;
//! a body over the 16 MiB limit is skipped with an error and a broken header ends the server
//! with one stderr line. The inline tests of `server.ignite` cover the rest of the story over
//! a pipe.

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

/// What a session gave: the exit status, the bodies the server wrote, its stderr.
struct Session {
    code: Option<i32>,
    replies: Vec<String>,
    stderr: String,
}

/// `input` written to the server of `target` on stdin, then the end of input; stdout and
/// stderr are read while it runs, so an output past a pipe buffer cannot stall it.
fn run_session(target: Target, input: Vec<u8>) -> Session {
    let mut child = Command::new(server(target))
        .arg("--ignite")
        .arg(env!("CARGO_BIN_EXE_ignite"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    // A server that ends before reading all of `input` closes its stdin: EPIPE is expected.
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(&input);
    });
    let stdout = drain(child.stdout.take().unwrap());
    let stderr = drain(child.stderr.take().unwrap());
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
    writer.join().unwrap();
    let stdout = stdout.join().unwrap();
    let stderr = String::from_utf8_lossy(&stderr.join().unwrap()).into_owned();
    Session {
        code: status.code(),
        replies: bodies(&stdout),
        stderr,
    }
}

/// A thread that reads `pipe` to its end.
fn drain(mut pipe: impl Read + Send + 'static) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut out = Vec::new();
        pipe.read_to_end(&mut out).unwrap();
        out
    })
}

const INITIALIZE_UTF16: &str = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"processId":null,"rootUri":null,"capabilities":{"general":{"positionEncodings":["utf-16"]}}}}"#;
const INITIALIZE_UTF8: &str = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"processId":null,"rootUri":null,"capabilities":{"general":{"positionEncodings":["utf-8","utf-16"]}}}}"#;
const INITIALIZED: &str = r#"{"jsonrpc":"2.0","method":"initialized","params":{}}"#;
const SHUTDOWN: &str = r#"{"jsonrpc":"2.0","id":2,"method":"shutdown"}"#;
const EXIT: &str = r#"{"jsonrpc":"2.0","method":"exit"}"#;
const SHUT_DOWN: &str = r#"{"jsonrpc":"2.0","id":2,"result":null}"#;

fn capabilities(encoding: &str) -> String {
    format!(
        r#"{{"jsonrpc":"2.0","id":1,"result":{{"capabilities":{{"positionEncoding":"{encoding}","textDocumentSync":{{"openClose":true,"change":1}}}},"serverInfo":{{"name":"ignite-lsp"}}}}}}"#
    )
}

/// A buffer whose E040 (the argument `"x"`) starts after a 4-byte, a 2-byte and a 3-byte
/// character on its line: UTF-16 columns 29 to 32, UTF-8 columns 34 to 37, line 3.
const BROKEN: &str =
    "package a\n\nfn f(a: Text, b: Int) -> Int = b\nfn main() -> Int = f(\"😀é€\", \"x\")\n";
const FIXED: &str = "package a\n\nfn main() -> Int = 1\n";

/// The file URI of `name` in a directory of its own for `target` and `case`.
fn document(target: Target, case: &str, name: &str) -> String {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("lsp-e2e-{case}-{target:?}"));
    std::fs::create_dir_all(&dir).unwrap();
    format!("file://{}", dir.join(name).display())
}

fn opened(uri: &str, version: i64, text: &str) -> String {
    format!(
        r#"{{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{{"textDocument":{{"uri":{},"languageId":"ignite","version":{version},"text":{}}}}}}}"#,
        json_string(uri),
        json_string(text)
    )
}

fn changed(uri: &str, version: i64, text: &str) -> String {
    format!(
        r#"{{"jsonrpc":"2.0","method":"textDocument/didChange","params":{{"textDocument":{{"uri":{},"version":{version}}},"contentChanges":[{{"text":{}}}]}}}}"#,
        json_string(uri),
        json_string(text)
    )
}

fn published(uri: &str, version: i64, diagnostics: &str) -> String {
    format!(
        r#"{{"jsonrpc":"2.0","method":"textDocument/publishDiagnostics","params":{{"uri":{},"version":{version},"diagnostics":[{diagnostics}]}}}}"#,
        json_string(uri)
    )
}

fn mismatch(start: usize, end: usize) -> String {
    format!(
        r#"{{"range":{{"start":{{"line":3,"character":{start}}},"end":{{"line":3,"character":{end}}}}},"severity":1,"code":"E040","source":"ignite","message":"type mismatch: expected `Int`, found `Text`"}}"#
    )
}

fn input(messages: &[String]) -> Vec<u8> {
    messages.iter().flat_map(|m| framed(m)).collect()
}

/// Open, change and shut down under `initialize`; the E040's range in `encoding`'s units is
/// `start` to `end`.
fn round_trip(
    target: Target,
    case: &str,
    initialize: &str,
    encoding: &str,
    start: usize,
    end: usize,
) {
    let uri = document(target, case, "a.ignite");
    let messages = [
        initialize.to_string(),
        INITIALIZED.to_string(),
        opened(&uri, 1, BROKEN),
        changed(&uri, 2, FIXED),
        SHUTDOWN.to_string(),
        EXIT.to_string(),
    ];
    let session = run_session(target, input(&messages));
    assert_eq!(session.code, Some(0), "stderr: {}", session.stderr);
    assert_eq!(
        session.replies,
        [
            capabilities(encoding),
            published(&uri, 1, &mismatch(start, end)),
            published(&uri, 2, ""),
            SHUT_DOWN.to_string(),
        ],
        "stderr: {}",
        session.stderr
    );
}

fn diagnostics_round_trip(target: Target) {
    round_trip(target, "utf16", INITIALIZE_UTF16, "utf-16", 29, 32);
}

fn diagnostics_in_utf8_once_negotiated(target: Target) {
    round_trip(target, "utf8", INITIALIZE_UTF8, "utf-8", 34, 37);
}

/// 2,000 type errors: a publishDiagnostics of about 300 KB, past any pipe buffer.
fn an_output_past_a_pipe_buffer_is_read_while_the_server_runs(target: Target) {
    let uri = document(target, "many", "many.ignite");
    let mut text = String::from("package many\n\n");
    for i in 0..2000 {
        text.push_str(&format!("fn f{i}() -> Int = \"t\"\n"));
    }
    let messages = [
        INITIALIZE_UTF16.to_string(),
        opened(&uri, 1, &text),
        SHUTDOWN.to_string(),
        EXIT.to_string(),
    ];
    let session = run_session(target, input(&messages));
    assert_eq!(session.code, Some(0), "stderr: {}", session.stderr);
    assert_eq!(session.replies.len(), 3, "stderr: {}", session.stderr);
    assert!(session.replies[1].len() > 65536);
    assert_eq!(session.replies[1].matches(r#""code":"E040""#).count(), 2000);
    assert_eq!(session.replies[2], SHUT_DOWN);
}

/// A body one byte over the 16 MiB limit is read past and answered under a null id; then a
/// header whose Content-Length is no number ends the server with one stderr line, status 1.
fn a_body_over_the_limit_is_skipped_and_a_broken_header_is_named_on_stderr(target: Target) {
    let mut bytes = framed(INITIALIZE_UTF16);
    let length = 16 * 1024 * 1024 + 1;
    bytes.extend_from_slice(format!("Content-Length: {length}\r\n\r\n").as_bytes());
    bytes.resize(bytes.len() + length, b'x');
    bytes.extend_from_slice(&framed(SHUTDOWN));
    bytes.extend_from_slice(b"Content-Length: abc\r\n\r\n{}");
    bytes.extend_from_slice(&framed(EXIT));
    let session = run_session(target, bytes);
    assert_eq!(session.code, Some(1), "stderr: {}", session.stderr);
    assert_eq!(
        session.replies,
        [
            capabilities("utf-16"),
            format!(
                r#"{{"jsonrpc":"2.0","id":null,"error":{{"code":-32600,"message":"a message body of {length} bytes is over the limit of 16777216 bytes"}}}}"#
            ),
            SHUT_DOWN.to_string(),
        ],
        "stderr: {}",
        session.stderr
    );
    assert_eq!(
        session.stderr,
        format!(
            "ignite-lsp: a message body of {length} bytes is over the limit of 16777216 bytes: skipped\n\
             ignite-lsp: a message header has an invalid Content-Length; the input cannot be read further, exiting\n"
        )
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

both_targets!(
    diagnostics_round_trip,
    diagnostics_in_utf8_once_negotiated,
    an_output_past_a_pipe_buffer_is_read_while_the_server_runs,
    a_body_over_the_limit_is_skipped_and_a_broken_header_is_named_on_stderr,
);
