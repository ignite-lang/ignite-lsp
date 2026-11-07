//! `src/server.ignite` built as `ignite-lsp` on each target by an installed `ignite` (the
//! environment variable `IGNITE`, set by `task test`) and driven over stdio, with that same
//! `ignite` as its compiler (`--ignite`). A buffer with a type error after multibyte characters
//! on its line publishes one E040 at the range of the offending text, in UTF-16 code units by
//! default and in bytes once UTF-8 is negotiated; the fixed buffer publishes an empty list;
//! `shutdown` then `exit` ends with status 0. An output past a pipe buffer (2,000 diagnostics)
//! is read while the server runs; a body over the 16 MiB limit is skipped with an error and a
//! broken header ends the server with one stderr line. The inline tests of `server.ignite`
//! cover the rest over a pipe.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

const TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Clone, Copy, Debug)]
enum Target {
    Rust,
    Llvm,
}

/// The `ignite` binary that builds the server and that the server runs: `IGNITE`, absolute.
fn ignite() -> &'static Path {
    static IGNITE: OnceLock<PathBuf> = OnceLock::new();
    IGNITE.get_or_init(|| {
        let path = std::env::var_os("IGNITE")
            .map(PathBuf::from)
            .unwrap_or_else(|| panic!("IGNITE must name the ignite binary (`task test` sets it)"));
        path.canonicalize()
            .unwrap_or_else(|e| panic!("IGNITE={}: {e}", path.display()))
    })
}

/// `ignite build src/server.ignite --target <target>`; the executable it reports as built
/// whose name is `ignite-lsp`.
fn build(target: Target) -> PathBuf {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/server.ignite");
    let tmp = Path::new(env!("CARGO_TARGET_TMPDIR"));
    let (name, out) = match target {
        Target::Rust => ("rust", tmp.join("lsp-rust")),
        Target::Llvm => ("llvm", tmp.join("lsp-llvm")),
    };
    let output = Command::new(ignite())
        .arg("build")
        .arg(&source)
        .arg("--target")
        .arg(name)
        .arg("--out")
        .arg(&out)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "ignite build --target {name}: {}\n{stdout}{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    stdout
        .lines()
        .filter_map(|line| line.strip_prefix("built "))
        .map(PathBuf::from)
        .find(|path| path.file_name().and_then(|s| s.to_str()) == Some("ignite-lsp"))
        .unwrap_or_else(|| panic!("no ignite-lsp in {stdout:?}"))
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
        .arg(ignite())
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
        r#"{{"jsonrpc":"2.0","id":1,"result":{{"capabilities":{{"positionEncoding":"{encoding}","textDocumentSync":{{"openClose":true,"change":1,"save":true}},"hoverProvider":true,"definitionProvider":true,"documentSymbolProvider":true}},"serverInfo":{{"name":"ignite-lsp"}}}}}}"#
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
        ping(3),
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
            pong(3),
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

/// A request the server does not implement: it makes the server check the buffers that wait
/// before it answers.
fn ping(id: i64) -> String {
    format!(r#"{{"jsonrpc":"2.0","id":{id},"method":"x/ping"}}"#)
}

fn pong(id: i64) -> String {
    format!(
        r#"{{"jsonrpc":"2.0","id":{id},"error":{{"code":-32601,"message":"method not found: x/ping"}}}}"#
    )
}

/// `steps` written to the server of `target` one after another, each followed by its pause,
/// then the end of input.
fn run_steps(target: Target, compiler: &Path, steps: Vec<(Vec<u8>, Duration)>) -> Session {
    let mut child = Command::new(server(target))
        .arg("--ignite")
        .arg(compiler)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || {
        for (bytes, pause) in steps {
            if stdin.write_all(&bytes).is_err() {
                return;
            }
            let _ = stdin.flush();
            std::thread::sleep(pause);
        }
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
    Session {
        code: status.code(),
        replies: bodies(&stdout.join().unwrap()),
        stderr: String::from_utf8_lossy(&stderr.join().unwrap()).into_owned(),
    }
}

/// The versions of the publishDiagnostics among `replies`, in order.
fn published_versions(replies: &[String]) -> Vec<i64> {
    replies
        .iter()
        .filter(|r| r.contains(r#""method":"textDocument/publishDiagnostics""#))
        .filter_map(|r| {
            let at = r.find(r#""version":"#)? + r#""version":"#.len();
            let digits: String = r[at..].chars().take_while(char::is_ascii_digit).collect();
            digits.parse().ok()
        })
        .collect()
}

/// Nine changes written in one write of under 4,096 bytes (`PIPE_BUF`, so one read takes all
/// of it): the server checks once, at the newest version.
fn changes_that_arrive_together_are_checked_once(target: Target) {
    let uri = document(target, "together", "a.ignite");
    let mut messages = vec![
        INITIALIZE_UTF16.to_string(),
        INITIALIZED.to_string(),
        opened(&uri, 1, FIXED),
    ];
    for version in 2..=10 {
        messages.push(changed(&uri, version, FIXED));
    }
    messages.push(SHUTDOWN.to_string());
    messages.push(EXIT.to_string());
    let bytes = input(&messages);
    assert!(bytes.len() < 4096, "{} bytes", bytes.len());
    let session = run_session(target, bytes);
    assert_eq!(session.code, Some(0), "stderr: {}", session.stderr);
    assert_eq!(
        session.replies,
        [
            capabilities("utf-16"),
            published(&uri, 10, ""),
            SHUT_DOWN.to_string()
        ],
        "stderr: {}",
        session.stderr
    );
}

/// A check runs once the input is quiet, without a request; changes typed while a check runs
/// are checked together after it, and the published versions only grow. The compiler is
/// `ignite` behind a script that waits 300 ms first, far longer than the 2 ms between two
/// changes.
fn checks_follow_typing_and_never_go_back(target: Target) {
    let uri = document(target, "typing", "a.ignite");
    let slow = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("slow-ignite-{target:?}"));
    std::fs::write(
        &slow,
        format!(
            "#!/bin/sh\nsleep 0.3\nexec '{}' \"$@\"\n",
            ignite().display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&slow, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let pause = Duration::from_millis(2);
    let mut steps = vec![
        (framed(INITIALIZE_UTF16), pause),
        (
            framed(&opened(&uri, 1, BROKEN)),
            Duration::from_millis(1500),
        ),
    ];
    for version in 2..=8 {
        let text = if version % 2 == 0 { FIXED } else { BROKEN };
        steps.push((framed(&changed(&uri, version, text)), pause));
    }
    steps.push((Vec::new(), Duration::from_millis(1500)));
    steps.push((input(&[SHUTDOWN.to_string(), EXIT.to_string()]), pause));
    let session = run_steps(target, &slow, steps);
    assert_eq!(session.code, Some(0), "stderr: {}", session.stderr);
    let versions = published_versions(&session.replies);
    assert_eq!(versions.first(), Some(&1), "{:?}", session.replies);
    assert_eq!(versions.last(), Some(&8), "{:?}", session.replies);
    assert!(
        versions.windows(2).all(|w| w[0] < w[1]),
        "versions {versions:?}"
    );
    assert!(versions.len() < 8, "every change was checked: {versions:?}");
    assert_eq!(session.replies.last().map(String::as_str), Some(SHUT_DOWN));
}

/// A type error, then a syntax error appended at the end of the file: the E002 is drawn over
/// the last character before the end, and the E040 stays, marked stale; an E003's range is the
/// unknown name.
fn a_syntax_error_keeps_the_type_errors_and_no_range_is_empty(target: Target) {
    let uri = document(target, "syntax", "a.ignite");
    let typed = "fn main() -> Int = \"x\"\n";
    let broken = format!("{typed}fn zz() -> Int = nope + \n");
    let messages = [
        INITIALIZE_UTF16.to_string(),
        opened(&uri, 1, typed),
        ping(3),
        changed(&uri, 2, &broken),
        ping(4),
        changed(&uri, 3, "fn main() -> Int = nope + 1\n"),
        SHUTDOWN.to_string(),
        EXIT.to_string(),
    ];
    let session = run_session(target, input(&messages));
    assert_eq!(session.code, Some(0), "stderr: {}", session.stderr);
    let stale_mismatch = r#"{"range":{"start":{"line":0,"character":19},"end":{"line":0,"character":22}},"severity":2,"code":"E040","source":"ignite","message":"(before the syntax error) type mismatch: expected `Int`, found `Text`"}"#;
    assert_eq!(
        session.replies,
        [
            capabilities("utf-16"),
            published(
                &uri,
                1,
                r#"{"range":{"start":{"line":0,"character":19},"end":{"line":0,"character":22}},"severity":1,"code":"E040","source":"ignite","message":"type mismatch: expected `Int`, found `Text`"}"#
            ),
            pong(3),
            published(
                &uri,
                2,
                &format!(
                    r#"{{"range":{{"start":{{"line":1,"character":22}},"end":{{"line":1,"character":23}}}},"severity":1,"code":"E002","source":"ignite","message":"unexpected end of file, expected expression"}},{stale_mismatch}"#
                )
            ),
            pong(4),
            published(
                &uri,
                3,
                r#"{"range":{"start":{"line":0,"character":19},"end":{"line":0,"character":23}},"severity":1,"code":"E003","source":"ignite","message":"unknown name `nope`"}"#
            ),
            SHUT_DOWN.to_string(),
        ],
        "stderr: {}",
        session.stderr
    );
}

fn request(id: i64, method: &str, uri: &str, line: usize, character: usize) -> String {
    format!(
        r#"{{"jsonrpc":"2.0","id":{id},"method":"{method}","params":{{"textDocument":{{"uri":{}}},"position":{{"line":{line},"character":{character}}}}}}}"#,
        json_string(uri)
    )
}

const PROGRAM: &str = "package e\n\ntype Pair {\n  left: Int\n  right: Int\n}\n\nfn twice(n: Int) -> Int = n * 2\n\nfn sum(pair: Pair) -> Int {\n  let total = pair.left + pair.right\n  twice(total)\n}\n";

/// Hover, definition and the outline of a saved file, then of an unsaved change that moves
/// every line down, both from the installed `ignite emit --typed-json`.
fn hover_definition_and_outline_follow_the_buffer(target: Target) {
    let uri = document(target, "queries", "e.ignite");
    std::fs::write(uri.strip_prefix("file://").unwrap(), PROGRAM).unwrap();
    let moved = format!("// moved\n{PROGRAM}");
    let messages = [
        INITIALIZE_UTF16.to_string(),
        opened(&uri, 1, PROGRAM),
        request(3, "textDocument/hover", &uri, 11, 3),
        request(4, "textDocument/definition", &uri, 11, 3),
        request(5, "textDocument/hover", &uri, 10, 22),
        request(6, "textDocument/definition", &uri, 10, 22),
        format!(
            r#"{{"jsonrpc":"2.0","id":7,"method":"textDocument/documentSymbol","params":{{"textDocument":{{"uri":{}}}}}}}"#,
            json_string(&uri)
        ),
        changed(&uri, 2, &moved),
        request(8, "textDocument/definition", &uri, 12, 9),
        SHUTDOWN.to_string(),
        EXIT.to_string(),
    ];
    let session = run_session(target, input(&messages));
    assert_eq!(session.code, Some(0), "stderr: {}", session.stderr);
    let versions = published_versions(&session.replies);
    assert_eq!(versions.last(), Some(&2), "{:?}", session.replies);
    let replies: Vec<&String> = session
        .replies
        .iter()
        .filter(|r| !r.contains("publishDiagnostics"))
        .collect();
    let location = |id: i64, line: usize, start: usize, end: usize| {
        format!(
            r#"{{"jsonrpc":"2.0","id":{id},"result":{{"uri":{},"range":{{"start":{{"line":{line},"character":{start}}},"end":{{"line":{line},"character":{end}}}}}}}}}"#,
            json_string(&uri)
        )
    };
    assert_eq!(replies.len(), 8, "{replies:?}");
    assert_eq!(
        replies[1],
        r#"{"jsonrpc":"2.0","id":3,"result":{"contents":{"kind":"markdown","value":"```ignite\nfn twice(n: Int) -> Int\n```"},"range":{"start":{"line":11,"character":2},"end":{"line":11,"character":7}}}}"#
    );
    assert_eq!(*replies[2], location(4, 7, 3, 8));
    assert_eq!(
        replies[3],
        r#"{"jsonrpc":"2.0","id":5,"result":{"contents":{"kind":"markdown","value":"```ignite\nPair.left: Int\n```"},"range":{"start":{"line":10,"character":19},"end":{"line":10,"character":23}}}}"#
    );
    assert_eq!(*replies[4], location(6, 3, 2, 6));
    assert!(
        replies[5].starts_with(r#"{"jsonrpc":"2.0","id":7,"result":[{"name":"Pair","detail":"type","kind":23,"range":{"start":{"line":2,"character":0},"end":{"line":5,"character":1}},"selectionRange":{"start":{"line":2,"character":5},"end":{"line":2,"character":9}},"children":[{"name":"left""#),
        "{}",
        replies[5]
    );
    assert!(replies[5].contains(r#"{"name":"sum","detail":"(pair: Pair) -> Int","kind":12,"#));
    assert_eq!(*replies[6], location(8, 11, 6, 11));
    assert_eq!(replies[7], SHUT_DOWN);
}

macro_rules! both_targets {
    ($($case:ident),* $(,)?) => {
        mod rust {
            $(#[test]
            fn $case() {
                super::$case(super::Target::Rust);
            })*
        }
        mod llvm {
            $(#[test]
            fn $case() {
                super::$case(super::Target::Llvm);
            })*
        }
    };
}

both_targets!(
    diagnostics_round_trip,
    diagnostics_in_utf8_once_negotiated,
    an_output_past_a_pipe_buffer_is_read_while_the_server_runs,
    a_body_over_the_limit_is_skipped_and_a_broken_header_is_named_on_stderr,
    changes_that_arrive_together_are_checked_once,
    checks_follow_typing_and_never_go_back,
    a_syntax_error_keeps_the_type_errors_and_no_range_is_empty,
    hover_definition_and_outline_follow_the_buffer,
);
