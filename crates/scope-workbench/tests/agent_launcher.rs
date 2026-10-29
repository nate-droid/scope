#![cfg(feature = "agent")]
//! Stock executable + real CellAgent adapter, with a deterministic local HTTP peer.
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

struct Mock {
    endpoint: String,
    worker: thread::JoinHandle<Vec<Value>>,
}
impl Mock {
    fn new(replies: Vec<(u16, Value)>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let worker = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(10);
            let mut requests = Vec::new();
            for (status, decision) in replies {
                let mut stream = loop {
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(error)
                            if error.kind() == std::io::ErrorKind::WouldBlock
                                && Instant::now() < deadline =>
                        {
                            thread::sleep(Duration::from_millis(5))
                        }
                        result => panic!("mock request missing: {result:?}"),
                    }
                };
                stream
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                requests.push(read_request(&mut stream));
                let body = if status == 200 {
                    json!({"id":"mock", "object":"chat.completion", "created":0, "model":"mock-script", "choices":[{"index":0,"message":{"role":"assistant","content":decision.to_string()},"finish_reason":"stop"}], "usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}})
                } else { json!({"error":{"message":"deterministic backend failure", "type":"server_error"}}) }.to_string();
                write!(stream, "HTTP/1.1 {status} Mock\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            }
            requests
        });
        Self { endpoint, worker }
    }
    fn finish(self) -> Vec<Value> {
        self.worker.join().unwrap()
    }
}
fn read_request(stream: &mut TcpStream) -> Value {
    let mut bytes = Vec::new();
    let end = loop {
        let mut byte = [0];
        stream.read_exact(&mut byte).unwrap();
        bytes.push(byte[0]);
        assert!(bytes.len() < 32 * 1024);
        if bytes.ends_with(b"\r\n\r\n") {
            break bytes.len();
        }
    };
    let headers = String::from_utf8(bytes.clone()).unwrap();
    assert!(
        headers.starts_with("POST /v1/chat/completions "),
        "{headers}"
    );
    let length: usize = headers
        .lines()
        .find_map(|line| {
            let (key, value) = line.split_once(':')?;
            key.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse().unwrap())
        })
        .unwrap();
    bytes.resize(end + length, 0);
    stream.read_exact(&mut bytes[end..]).unwrap();
    serde_json::from_slice(&bytes[end..]).unwrap()
}
fn shell(endpoint: &str, edit: bool, input: &str) -> String {
    let root = tempfile::tempdir().unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_scope"));
    command.args([
        "--agent-local-binding",
        "--agent-model",
        "mock-script",
        "--agent-base-url",
        endpoint,
        "--allow-read",
    ]);
    if edit {
        command.arg("--allow-edit");
    }
    let mut child = command
        .arg("shell")
        .current_dir(root.path())
        .env("NO_PROXY", "127.0.0.1,localhost,::1")
        .env("no_proxy", "127.0.0.1,localhost,::1")
        .env("XDG_STATE_HOME", root.path())
        .env_remove("SCOPE_HISTORY_FILE")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(output.status.success(), "{stdout}\n{stderr}");
    assert_eq!(
        std::fs::read_dir(root.path()).unwrap().count(),
        0,
        "no FS grants: {stdout}"
    );
    stdout
}
fn draft(source: &str) -> String {
    format!(":scope scratch new scope-script\n:scope edit\n{source}\n:done\n")
}
fn tool(operation: &str, arguments: Value) -> (u16, Value) {
    (
        200,
        json!({"type":"tool", "name":format!("language.scope-script.{operation}"), "arguments":arguments}),
    )
}

#[test]
fn stock_launcher_reads_selected_script_not_bound_target_or_app() {
    let mock = Mock::new(vec![
        tool("inspect", json!({})),
        (200, json!({"type":"complete", "content":"read complete"})),
    ]);
    let input = format!(
        "{}:scope semantic-target 0\n{}:scope agent explain\n:scope quit\n",
        draft("print(999)"),
        draft("print(42)")
    );
    let output = shell(&mock.endpoint, false, &input);
    assert!(!output.contains("error:"), "{output}");
    let requests = mock.finish();
    assert_eq!(requests.len(), 2);
    assert!(
        requests[1].to_string().contains("print(42)"),
        "{requests:?}"
    );
    assert!(
        !requests[1].to_string().contains("print(999)"),
        "{requests:?}"
    );
    assert!(output.contains("read complete"), "{output}");
    assert!(
        !output.contains("script: "),
        "inspection must not execute: {output}"
    );
}

#[test]
fn stock_launcher_edit_is_pending_reviewable_and_checked_on_accept() {
    let mock = Mock::new(vec![tool("replace", json!({"candidate":"print(2)\n"}))]);
    let input = format!(
        "{}:scope agent edit \"change one to two\"\n:scope source-diff 1\n:scope show\n:scope source-accept 1\n:scope show\n:scope quit\n",
        draft("print(1)")
    );
    let output = shell(&mock.endpoint, true, &input);
    assert!(!output.contains("error:"), "{output}");
    assert_eq!(mock.finish().len(), 1);
    assert!(
        output.contains("source proposal 1 ready; pending; source unchanged; not saved"),
        "{output}"
    );
    assert!(output.contains("complete-script-syntax"), "{output}");
    assert!(output.contains("+print(2)"), "{output}");
    assert!(output.contains("print(1)"), "{output}");
    assert!(output.contains("print(2)"), "{output}");
    assert!(
        output.contains("source proposal 1 applied to buffer 0"),
        "{output}"
    );
    assert!(
        !output.contains("script: "),
        "proposal/accept must not execute: {output}"
    );
}

#[test]
fn stock_launcher_refuses_stale_proposal_without_overwriting_live_source() {
    let mock = Mock::new(vec![tool("replace", json!({"candidate":"print(2)\n"}))]);
    let input = format!(
        "{}:scope agent edit \"change\"\n:scope line 1 \"print(3)\"\n:scope source-accept 1\n:scope show\n:scope quit\n",
        draft("print(1)")
    );
    let output = shell(&mock.endpoint, true, &input);
    mock.finish();
    assert!(output.contains("error:"), "{output}");
    assert!(output.contains("print(3)"), "{output}");
    assert!(!output.contains("applied to"), "{output}");
}

#[test]
fn stock_launcher_missing_wrong_or_closed_source_and_ungranted_edits_do_not_contact_backend() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
    let input = format!(
        ":scope agent explain\n:scope scratch new text\n:scope agent explain\n{}:scope agent edit \"change\"\n:scope discard\n:scope agent explain\n:scope quit\n",
        draft("print(1)")
    );
    let output = shell(&endpoint, false, &input);
    assert!(output.contains("--allow-edit"), "{output}");
    assert!(output.contains("language does not match"), "{output}");
    assert!(
        listener.accept().is_err(),
        "preflight must not contact endpoint"
    );
}

#[test]
fn stock_launcher_default_does_not_configure_agents_or_fall_back_to_app() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let root = tempfile::tempdir().unwrap();
    let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
    // Endpoint/model settings alone are not opt-in or read/edit authority.
    let mut child = Command::new(env!("CARGO_BIN_EXE_scope"))
        .args([
            "--agent-model",
            "mock-script",
            "--agent-base-url",
            &endpoint,
            "shell",
        ])
        .current_dir(root.path())
        .env("XDG_STATE_HOME", root.path())
        .env_remove("SCOPE_HISTORY_FILE")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let input = format!(
        "{}:scope agent explain\n:scope agent edit \"change\"\n:scope quit\n",
        draft("print(1)")
    );
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.contains("explicit agent opt-in; no App fallback"),
        "{stdout}"
    );
    assert!(stdout.contains("--allow-edit"), "{stdout}");
    assert!(
        listener.accept().is_err(),
        "no backend contact without opt-in"
    );
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
}

#[test]
fn stock_launcher_invalid_syntax_requires_correction_before_pending_proposal() {
    let mock = Mock::new(vec![
        tool("replace", json!({"candidate":"if true {"})),
        tool("replace", json!({"candidate":"print(2)\n"})),
    ]);
    let input = format!(
        "{}:scope agent edit \"change\"\n:scope source-proposals\n:scope show\n:scope quit\n",
        draft("print(1)")
    );
    let output = shell(&mock.endpoint, true, &input);
    assert!(output.contains("ready; pending"), "{output}");
    let requests = mock.finish();
    assert_eq!(requests.len(), 2);
    assert!(
        output.contains("print(1)"),
        "candidate remains unapplied: {output}"
    );
}

#[test]
fn stock_launcher_backend_failure_is_reported_without_fallback_or_mutation() {
    let mock = Mock::new(vec![(503, Value::Null)]);
    let input = format!(
        "{}:scope agent explain\n:scope show\n:scope quit\n",
        draft("print(7)")
    );
    let output = shell(&mock.endpoint, false, &input);
    assert!(
        output.contains("deterministic backend failure") || output.contains("503"),
        "{output}"
    );
    mock.finish();
    assert!(output.contains("error:"), "{output}");
    assert!(output.contains("print(7)"), "{output}");
    assert!(!output.contains("App evaluation"), "{output}");
}
