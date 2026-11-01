use super::*;
use crate::agent::{AgentLimits, AgentSession, AgentStop, read_only_policy, run_session};
use crate::tool::{ToolCall, ToolDefinition, ToolError, ToolProvider, ToolResult, ToolValue};
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

struct CountingTool {
    definitions: Vec<ToolDefinition>,
    calls: Arc<AtomicUsize>,
}
impl ToolProvider for CountingTool {
    fn tools(&self) -> &[ToolDefinition] {
        &self.definitions
    }
    fn invoke(&mut self, _: ToolCall) -> Result<ToolResult, ToolError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(ToolResult::new(ToolValue::Null))
    }
}
fn session() -> (AgentSession<'static>, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let session = AgentSession::builder()
        .provider(CountingTool {
            definitions: vec![ToolDefinition::new("probe", "count dispatches")],
            calls: calls.clone(),
        })
        .build()
        .unwrap();
    (session, calls)
}

fn read_request(stream: &mut TcpStream) -> Value {
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let mut bytes = Vec::new();
    let mut chunk = [0; 4096];
    let header_end = loop {
        let n = stream.read(&mut chunk).unwrap();
        assert_ne!(n, 0, "request ended before headers");
        bytes.extend_from_slice(&chunk[..n]);
        assert!(bytes.len() <= 1024 * 1024);
        if let Some(index) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
            break index + 4;
        }
    };
    let headers = std::str::from_utf8(&bytes[..header_end]).unwrap();
    assert!(headers.starts_with("POST /api/v1/chat HTTP/1.1\r\n"));
    let length: usize = headers
        .lines()
        .find_map(|line| {
            let (key, value) = line.split_once(':')?;
            key.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse().unwrap())
        })
        .expect("request content-length");
    assert!(length <= 1024 * 1024);
    while bytes.len() < header_end + length {
        let n = stream.read(&mut chunk).unwrap();
        assert_ne!(n, 0, "request ended before body");
        bytes.extend_from_slice(&chunk[..n]);
    }
    serde_json::from_slice(&bytes[header_end..header_end + length]).unwrap()
}

// Scoped joining plus bounded accept/read/write/client timeouts also bounds failure paths.
fn with_server(responses: Vec<Value>, test: impl FnOnce(&mut LmStudioAgent)) -> Vec<Value> {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    thread::scope(|scope| {
        let server = scope.spawn(move || {
            let mut requests = Vec::new();
            for response in responses {
                let deadline = Instant::now() + Duration::from_secs(4);
                let mut stream = loop {
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            assert!(Instant::now() < deadline, "mock request timed out");
                            thread::sleep(Duration::from_millis(5));
                        }
                        Err(e) => panic!("mock accept: {e}"),
                    }
                };
                requests.push(read_request(&mut stream));
                let body = response.to_string();
                write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
            }
            requests
        });
        let mut model = LmStudioAgent {
            chat: NativeChat::with_timeout(
                "fixture",
                &origin,
                ReasoningMode::On,
                Duration::from_secs(3),
            )
            .unwrap(),
            invocations: 0,
            last_response: None,
            last_error: None,
        };
        test(&mut model);
        server.join().unwrap()
    })
}
fn response(output: Value) -> Value {
    json!({"output":output, "stats":{"input_tokens":17,"total_output_tokens":9,"reasoning_output_tokens":4},"finish":"fixture-stop"})
}
fn visible(text: Value) -> Value {
    response(
        json!([{"type":"reasoning","content":"private fixture reasoning"},{"type":"message","content":text}]),
    )
}
const COMPLETE: &str = r#"{"type":"complete","content":"done"}"#;
const TOOL: &str = r#"{"type":"tool","name":"probe","arguments":{}}"#;

#[test]
fn native_malformed_decisions_consume_budget_without_dispatch_and_retain_metadata() {
    let cases = [
        response(json!([{"type":"reasoning","content":TOOL}])),
        visible(json!("{\"type\":")),
        visible(json!("")),
        visible(Value::Null),
        visible(json!(format!("{TOOL}\n{TOOL}"))),
        visible(json!(format!("Here is a tool: {TOOL}"))),
        visible(json!(
            r#"{"type":"tool","name":"probe","arguments":{},"extra":true}"#
        )),
    ];
    for body in cases {
        let requests = with_server(vec![body.clone()], |model| {
            let (mut session, calls) = session();
            let run = run_session(
                model,
                &mut session,
                "inspect",
                AgentLimits {
                    max_decisions: 1,
                    max_malformed: 2,
                    ..AgentLimits::default()
                },
                read_only_policy,
            )
            .unwrap();
            assert_eq!(run.stop, AgentStop::DecisionBudget);
            assert_eq!((run.decisions, run.malformed, run.tool_calls), (1, 1, 0));
            assert_eq!(calls.load(Ordering::SeqCst), 0);
            let settings = model.trace_settings().unwrap();
            assert_eq!(settings["invocations"], 1);
            assert_eq!(settings["backend_accepted"], true);
            assert!(settings["effective_mode"].is_null());
            assert!(settings["last_error"].is_null());
            assert_eq!(settings["observed_reasoning_items"], 1);
            assert_eq!(settings["last_response"]["raw"], body);
            assert_eq!(settings["last_response"]["usage"]["input_tokens"], 17);
            assert_eq!(settings["last_response"]["usage"]["total_output_tokens"], 9);
            assert_eq!(
                settings["last_response"]["usage"]["reasoning_output_tokens"],
                4
            );
            assert_eq!(settings["last_response"]["finish"], "fixture-stop");
        });
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0]["model"], "fixture");
        assert_eq!(requests[0]["reasoning"], "on");
        assert_eq!(requests[0]["store"], false);
        assert_eq!(requests[0]["stream"], false);
        assert!(
            requests[0]["system_prompt"]
                .as_str()
                .unwrap()
                .contains("exactly one JSON object")
        );
        assert_eq!(
            requests[0]["max_output_tokens"],
            AgentLimits::default().max_tokens_per_decision
        );
    }
}

#[test]
fn native_partial_recovers_through_strict_feedback_to_valid_completion() {
    let requests = with_server(
        vec![visible(json!("{\"type\":")), visible(json!(COMPLETE))],
        |model| {
            let (mut session, calls) = session();
            let run = run_session(
                model,
                &mut session,
                "inspect",
                AgentLimits::default(),
                read_only_policy,
            )
            .unwrap();
            assert_eq!(run.stop, AgentStop::Completed);
            assert_eq!((run.decisions, run.malformed, run.tool_calls), (2, 1, 0));
            assert_eq!(calls.load(Ordering::SeqCst), 0);
            assert_eq!(model.invocations, 2);
            assert_eq!(model.last_response.as_ref().unwrap().visible, COMPLETE);
        },
    );
    let transcript: Value = serde_json::from_str(requests[1]["input"].as_str().unwrap()).unwrap();
    let feedback: Value = serde_json::from_str(
        transcript.as_array().unwrap().last().unwrap()["content"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(feedback["category"], "incomplete_response");
}

#[test]
fn native_multiple_visible_items_fail_closed_before_dispatch() {
    with_server(
        vec![response(
            json!([{"type":"message","content":TOOL},{"type":"message","content":COMPLETE}]),
        )],
        |model| {
            let (mut session, calls) = session();
            assert!(
                run_session(
                    model,
                    &mut session,
                    "inspect",
                    AgentLimits::default(),
                    read_only_policy
                )
                .is_err()
            );
            assert_eq!(calls.load(Ordering::SeqCst), 0);
            let settings = model.trace_settings().unwrap();
            assert_eq!(settings["invocations"], 1);
            assert!(settings["last_response"].is_null());
            assert_eq!(settings["backend_accepted"], false);
            assert!(
                settings["last_error"]
                    .as_str()
                    .unwrap()
                    .contains("multiple visible")
            );
        },
    );
}

#[test]
fn native_capability_registers_with_real_cell_linker_without_guest_execution() {
    let chat = NativeChat::new("fixture", "http://127.0.0.1:1", ReasoningMode::Default).unwrap();
    let cell = cell::Cell::new().unwrap().with(chat).unwrap();
    assert_eq!(cell.capabilities(), &["cell:lmstudio/chat@0.1.0"]);
}
