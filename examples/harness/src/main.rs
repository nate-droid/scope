use scope_ide::{
    app::App,
    tool::{
        AgentSession, ScopeToolProvider, SessionCatalog, ToolCall, ToolDefinition, ToolEffect,
        ToolError, ToolExecutionEvent, ToolProvider, ToolResult, ToolValue,
    },
    workspace::Workspace,
};
use std::{
    fs, io,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

const ORIGINAL: &str = "fn old() {}\n";
const EDITED: &str = "fn main() {}\n";

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> io::Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        loop {
            let path = std::env::temp_dir().join(format!(
                "scope-harness-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&path) {
                Ok(()) => {
                    let fixture = Self(path);
                    fs::write(fixture.0.join("main.scope"), ORIGINAL)?;
                    return Ok(fixture);
                }
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e),
            }
        }
    }
    fn text(&self) -> String {
        fs::read_to_string(self.0.join("main.scope")).expect("fixture remains readable")
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct FixtureTools(Vec<ToolDefinition>);
impl FixtureTools {
    fn new() -> Self {
        Self(vec![ToolDefinition::new(
            "harness.ping",
            "Return a fixed fixture health marker",
        )])
    }
}
impl ToolProvider for FixtureTools {
    fn tools(&self) -> &[ToolDefinition] {
        &self.0
    }
    fn provider_name(&self) -> Option<&str> {
        Some("harness")
    }
    fn output_description(&self, tool_name: &str) -> Option<&str> {
        (tool_name == "harness.ping").then_some("The string pong.")
    }
    fn invoke(&mut self, call: ToolCall) -> Result<ToolResult, ToolError> {
        if call.name != "harness.ping" {
            return Err(ToolError::UnknownTool { name: call.name });
        }
        if let Some(argument) = call.arguments.keys().next() {
            return Err(ToolError::InvalidArgument {
                tool: call.name,
                argument: argument.clone(),
                message: "no arguments accepted".into(),
            });
        }
        Ok(ToolResult::new("pong".into()))
    }
}

fn field<'a>(result: &'a ToolResult, name: &str) -> Option<&'a ToolValue> {
    match &result.value {
        ToolValue::Object(fields) => fields.get(name),
        _ => None,
    }
}

// This adapter owns decisions, not approval. Results stay in memory, outside the trace.
struct ScriptedDecisions;
impl ScriptedDecisions {
    fn next(
        &self,
        catalog: &SessionCatalog,
        step: usize,
        snapshot: Option<&ToolResult>,
    ) -> Result<Option<ToolCall>, String> {
        // A read-only environment can finish after observing the fixture.
        if step == 3 && catalog.lookup("scope.document.replace").is_none() {
            return Ok(None);
        }
        let proposal = match step {
            1 => Some(ToolCall::new("harness.ping")),
            2 => Some(
                ToolCall::new("scope.document.read").with_argument("path", "main.scope".into()),
            ),
            3 => {
                let snapshot = snapshot.ok_or("missing read snapshot")?;
                if field(snapshot, "text").and_then(ToolValue::as_str) != Some(ORIGINAL) {
                    return Err("script expects original fixture text".into());
                }
                let revision = field(snapshot, "revision")
                    .and_then(ToolValue::as_str)
                    .ok_or("snapshot lacks disk revision")?;
                let mut call = ToolCall::new("scope.document.replace")
                    .with_argument("path", "main.scope".into())
                    .with_argument("expected_revision", revision.into())
                    .with_argument("start_line", 0usize.into())
                    .with_argument("start_column", 3usize.into())
                    .with_argument("end_line", 0usize.into())
                    .with_argument("end_column", 6usize.into())
                    .with_argument("replacement", "main".into())
                    .with_argument("save", true.into());
                for (source, target) in [
                    ("document_id", "expected_document_id"),
                    ("version", "expected_version"),
                ] {
                    if let Some(value) = field(snapshot, source) {
                        call = call.with_argument(target, value.clone());
                    }
                }
                Some(call)
            }
            _ => None,
        };
        if let Some(call) = &proposal {
            if catalog.lookup(&call.name).is_none() {
                return Err(format!("required tool not discovered: {}", call.name));
            }
        }
        Ok(proposal)
    }
}

#[derive(Clone, Copy)]
enum Approval {
    ApproveFixtureWrite,
    DenyWrites,
}
impl Approval {
    fn permits(self, definition: &ToolDefinition, proposal: &ToolCall) -> bool {
        definition.effect == ToolEffect::Read
            || (matches!(self, Self::ApproveFixtureWrite)
                && definition.name == "scope.document.replace"
                && proposal.arguments.get("path").and_then(ToolValue::as_str) == Some("main.scope"))
    }
}

#[derive(Debug)]
struct TraceEntry {
    run_id: u64,
    step_id: usize,
    event: ToolExecutionEvent,
}
#[derive(Debug, PartialEq, Eq)]
enum Stop {
    Complete,
    StepBudget,
    ToolFailure,
}
struct Report {
    stop: Stop,
    trace: Vec<TraceEntry>,
    error: Option<ToolError>,
    custom_called: bool,
}

fn drive(
    session: &mut AgentSession<'_>,
    run_id: u64,
    budget: usize,
    approval: Approval,
    mut before_dispatch: impl FnMut(usize),
) -> Result<Report, String> {
    let adapter = ScriptedDecisions;
    let discovered = session.catalog();
    let mut snapshot = None;
    let mut report = Report {
        stop: Stop::Complete,
        trace: Vec::new(),
        error: None,
        custom_called: false,
    };
    for step in 1..=budget {
        let Some(proposal) = adapter.next(&discovered, step, snapshot.as_ref())? else {
            return Ok(report);
        };
        before_dispatch(step);
        let result = session.invoke_observed_with_call_policy(
            proposal,
            |definition, call| approval.permits(definition, call),
            |event| {
                report.trace.push(TraceEntry {
                    run_id,
                    step_id: step,
                    event: event.clone(),
                })
            },
        );
        match result {
            Ok(result) if step == 1 => {
                report.custom_called = result.value == ToolValue::from("pong")
            }
            Ok(result) if step == 2 => snapshot = Some(result),
            Ok(_) => {}
            Err(error) => {
                report.stop = Stop::ToolFailure;
                report.error = Some(error);
                return Ok(report);
            }
        }
    }
    // Completion does not require an extra dispatch or consume a tool step.
    if adapter
        .next(&discovered, budget.saturating_add(1), snapshot.as_ref())?
        .is_some()
    {
        report.stop = Stop::StepBudget;
    }
    Ok(report)
}

fn run_fixture(
    approval: Approval,
    budget: usize,
    stale: bool,
) -> Result<(Report, String), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let mut app = App::new(Workspace::new(fixture.0.to_string_lossy().into_owned())?)?;
    let mut session = AgentSession::builder()
        .app(&mut app)
        .provider(ScopeToolProvider::for_session())
        .provider(FixtureTools::new())
        .build()?;
    static NEXT_RUN: AtomicU64 = AtomicU64::new(1);
    let run_id = NEXT_RUN.fetch_add(1, Ordering::Relaxed);
    let report = drive(&mut session, run_id, budget, approval, |step| {
        if stale && step == 3 {
            fs::write(fixture.0.join("main.scope"), "fn external() {}\n").expect("fixture change");
        }
    })?;
    Ok((report, fixture.text()))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (report, text) = run_fixture(Approval::ApproveFixtureWrite, 3, false)?;
    for entry in &report.trace {
        println!(
            "run={} step={} {:?}",
            entry.run_id, entry.step_id, entry.event
        );
    }
    if report.stop != Stop::Complete || text != EDITED {
        return Err("script did not complete".into());
    }
    println!("Completed approved fixture edit; temporary workspace cleaned up.");
    // Keep the deny branch exercised in the binary as well as tests.
    let (denied, unchanged) = run_fixture(Approval::DenyWrites, 3, false)?;
    if !matches!(denied.error, Some(ToolError::Denied { .. })) || unchanged != ORIGINAL {
        return Err("denied write changed the fixture".into());
    }
    println!("Denied fixture write left disk unchanged.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use scope_ide::tool::ToolExecutionOutcome;

    #[test]
    fn decisions_adapt_to_discovered_tools() {
        let fixture = Fixture::new().unwrap();
        let mut app =
            App::new(Workspace::new(fixture.0.to_string_lossy().into_owned()).unwrap()).unwrap();
        let session = AgentSession::for_app(&mut app).unwrap();
        let mut catalog = session.catalog();
        let adapter = ScriptedDecisions;
        assert!(adapter
            .next(&catalog, 1, None)
            .unwrap_err()
            .contains("harness.ping"));
        catalog
            .tools
            .retain(|entry| entry.definition.name != "scope.document.replace");
        assert_eq!(adapter.next(&catalog, 3, None).unwrap(), None);
        assert_eq!(fixture.text(), ORIGINAL);
    }

    #[test]
    fn approval_checks_call_path() {
        let definition =
            ToolDefinition::new("scope.document.replace", "edit").effect(ToolEffect::Write);
        let call =
            ToolCall::new("scope.document.replace").with_argument("path", "other.scope".into());
        assert!(!Approval::ApproveFixtureWrite.permits(&definition, &call));
    }

    #[test]
    fn successful_write() {
        let (report, text) = run_fixture(Approval::ApproveFixtureWrite, 3, false).unwrap();
        assert_eq!(report.stop, Stop::Complete);
        assert_eq!(text, EDITED);
        assert_eq!(report.trace.len(), 6);
        for (index, pair) in report.trace.chunks_exact(2).enumerate() {
            assert_eq!(pair[0].run_id, report.trace[0].run_id);
            assert_eq!(pair[1].run_id, pair[0].run_id);
            assert_eq!(pair[0].step_id, index + 1);
            assert!(
                matches!(pair[0].event, ToolExecutionEvent::Started { call_id, .. } if call_id == (index + 1) as u64)
            );
            assert!(
                matches!(pair[1].event, ToolExecutionEvent::Finished { call_id, outcome: ToolExecutionOutcome::Success, .. } if call_id == (index + 1) as u64)
            );
        }
    }
    #[test]
    fn denied_write() {
        let (report, text) = run_fixture(Approval::DenyWrites, 3, false).unwrap();
        assert!(matches!(report.error, Some(ToolError::Denied { .. })));
        assert_eq!(text, ORIGINAL);
        assert!(matches!(
            report.trace.last().unwrap().event,
            ToolExecutionEvent::Finished {
                outcome: ToolExecutionOutcome::Denied,
                ..
            }
        ));
    }
    #[test]
    fn stale_conflict() {
        let (report, text) = run_fixture(Approval::ApproveFixtureWrite, 3, true).unwrap();
        assert!(matches!(report.error, Some(ToolError::Conflict { .. })));
        assert_eq!(text, "fn external() {}\n");
        assert!(matches!(
            report.trace.last().unwrap().event,
            ToolExecutionEvent::Finished {
                outcome: ToolExecutionOutcome::Conflict,
                ..
            }
        ));
    }
    #[test]
    fn custom_provider_call() {
        let (report, text) = run_fixture(Approval::DenyWrites, 1, false).unwrap();
        assert!(report.custom_called);
        assert_eq!(report.trace.len(), 2);
        assert_eq!(text, ORIGINAL);
    }
    #[test]
    fn step_budget() {
        for budget in 0..=2 {
            let (report, text) = run_fixture(Approval::ApproveFixtureWrite, budget, false).unwrap();
            assert_eq!(report.stop, Stop::StepBudget);
            assert_eq!(report.trace.len(), budget * 2);
            assert_eq!(text, ORIGINAL);
        }
    }
}
