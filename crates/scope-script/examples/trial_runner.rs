//! Fixed, opt-in downstream orchestration example; not a Scope core API.
#[path = "../../scope-ide/examples/selection_description_support/mod.rs"]
mod language;

use scope_ide::{
    scratch::ScratchBuffers,
    source_edit::{PermittedEditScope, SourceEditController, SourceSnapshot, SourceTarget},
    text::{Position, TextRange},
    tool::{AgentSession, semantic::SemanticToolProvider},
};
use scope_script::{ErrorKind, Host, Interpreter, Value, semantic::SemanticHost};

const ACTION: &str = "language.example.declarations.describe_selection";
const MAX_REPORT: usize = 65_536;
#[derive(Clone, Copy, Debug)]
enum Policy {
    Continue,
    Stop,
}
#[derive(Clone)]
struct Trial {
    id: &'static str,
    baseline: &'static str,
    operation: &'static str,
    selection: TextRange,
    arguments: Value,
    policy: Policy,
    max_invocations: usize,
    authorized: bool,
}
fn predefined() -> Vec<Trial> {
    ["first", "second"]
        .into_iter()
        .map(|id| Trial {
            id,
            baseline: "captured",
            operation: ACTION,
            selection: if id == "first" {
                TextRange::new(Position::new(0, 0), Position::new(0, 15))
            } else {
                TextRange::new(Position::new(1, 0), Position::new(1, 13))
            },
            arguments: Value::object([] as [(&str, Value); 0]),
            policy: if id == "first" {
                Policy::Continue
            } else {
                Policy::Stop
            },
            max_invocations: 1,
            authorized: true,
        })
        .collect()
}
fn failure(message: &str) -> ErrorKind {
    ErrorKind::Runtime(message.into())
}
fn identity(snapshot: SourceSnapshot<'_>, owner: &str) -> Value {
    Value::object([
        ("owner", owner.into()),
        ("source", format!("{:?}", snapshot.id).into()),
        ("revision", snapshot.version.to_string().into()),
        ("language", snapshot.language.into()),
    ])
}
// Checked accounting includes keys, containers and scalars; depth is bounded too.
fn size(value: &Value, depth: usize) -> Option<usize> {
    if depth > 64 {
        return None;
    }
    let add = |a: usize, b: usize| a.checked_add(b);
    match value {
        Value::String(s) => s.len().checked_add(16),
        Value::Array(values) => values
            .iter()
            .try_fold(16, |n, v| add(n, size(v, depth + 1)?)),
        Value::Object(values) => values
            .iter()
            .try_fold(16, |n, (k, v)| add(add(n, k.len())?, size(v, depth + 1)?)),
        _ => Some(16),
    }
}
struct Comparison<'a> {
    baseline: SourceSnapshot<'a>,
    current: SourceSnapshot<'a>,
    trials: Vec<Trial>,
    authorized: bool,
    used: bool,
    invocations: usize,
    retention: usize,
}
impl<'a> Comparison<'a> {
    fn new(baseline: SourceSnapshot<'a>, authorized: bool) -> Self {
        Self {
            baseline,
            current: baseline,
            trials: predefined(),
            authorized,
            used: false,
            invocations: 0,
            retention: MAX_REPORT,
        }
    }
    fn compare(&mut self) -> Result<Value, ErrorKind> {
        // Validate every specification and all preconditions before creating providers.
        if !self.authorized {
            return Err(failure("comparison authorization denied"));
        }
        if self.used {
            return Err(ErrorKind::LimitExceeded);
        }
        if self.trials.len() != 2 || self.trials[0].id == self.trials[1].id {
            return Err(failure("exactly two distinct predefined trials required"));
        }
        for (trial, expected) in self.trials.iter().zip(predefined()) {
            if trial.id != expected.id || trial.selection != expected.selection {
                return Err(failure("only predefined trial selections are permitted"));
            }
            if trial.baseline != "captured"
                || trial.operation != ACTION
                || trial.max_invocations != 1
                || trial.arguments != Value::object([] as [(&str, Value); 0])
            {
                return Err(failure("invalid predefined trial contract"));
            }
        }
        if self.current.id != self.baseline.id
            || self.current.version != self.baseline.version
            || self.current.text != self.baseline.text
            || self.current.language != self.baseline.language
        {
            return Err(failure("stale baseline"));
        }
        if self.baseline.text.len() > 65_536 {
            return Err(ErrorKind::LimitExceeded);
        }
        self.used = true;
        let service = language::DeclarationLanguage::default();
        let mut buffers = ScratchBuffers::default();
        let mut outcomes = Vec::with_capacity(2);
        let mut stopped = false;
        let mut retained = 0usize;
        for trial in &self.trials {
            // One collection, different owners. Baseline remains a read-only borrow.
            buffers.create(self.baseline.language, self.baseline.text);
            let target = buffers.selected().unwrap();
            assert_eq!(target.text().as_bytes(), self.baseline.text.as_bytes());
            let mut controller = SourceEditController::default();
            let mut outcome = Value::Null;
            let mut error = Value::Null;
            let mut status = "skipped";
            let mut metadata = Value::Null;
            if !stopped {
                self.invocations = self
                    .invocations
                    .checked_add(1)
                    .ok_or(ErrorKind::LimitExceeded)?;
                if self.invocations > 2 {
                    return Err(ErrorKind::LimitExceeded);
                }
                let result = (|| {
                    if !trial.authorized {
                        return Err(failure("trial authorization denied"));
                    }
                    let provider = SemanticToolProvider::new(
                        &service,
                        target,
                        &mut controller,
                        PermittedEditScope::Document(target.source_id()),
                        Some(trial.selection),
                        |auth| {
                            trial.authorized
                                && auth.source.id == target.source_id()
                                && auth.source.version == target.version()
                                && auth.source.text == self.baseline.text
                        },
                    )
                    .map_err(ErrorKind::Tool)?;
                    let session = AgentSession::builder()
                        .provider(provider)
                        .build()
                        .map_err(ErrorKind::Tool)?;
                    let mut host = SemanticHost::new(
                        session,
                        ACTION,
                        target.text(),
                        target.version(),
                        Some(true),
                    );
                    metadata = host.operations()?;
                    host.invoke("semantic", trial.operation, trial.arguments.clone())
                })();
                match result {
                    Ok(value) => {
                        status = "returned";
                        outcome = value;
                    }
                    Err(e) => {
                        status = "invocation_error";
                        error = e.to_string().into();
                    }
                }
            }
            assert_eq!(controller.list().count(), 0);
            let mut entry = Value::object([
                ("trial_id", trial.id.into()), ("baseline", identity(self.baseline, "baseline-collection")),
                ("trial_source", identity(target.snapshot(), "trial-collection")),
                ("provider", "semantic".into()), ("operation", trial.operation.into()),
                ("contract", metadata), ("arguments", trial.arguments.clone()),
                                ("selection", format!("{:?}", trial.selection).into()),
                ("policy", format!("{:?}", trial.policy).into()),
                ("max_invocations", 1i64.into()),
                ("isolation", "identical bytes; distinct scratch owner; fresh provider/session/host; no model".into()),
                ("status", status.into()), ("outcome", outcome), ("error", error),
            ]);
            let required = size(&entry, 0).and_then(|n| retained.checked_add(n));
            if required.is_none_or(|n| n > self.retention) {
                // Do not truncate or reclassify a downstream result. Explicitly report its omission.
                status = "resource_limit";
                entry = Value::object([
                    ("trial_id", trial.id.into()),
                    ("status", status.into()),
                    ("baseline", identity(self.baseline, "baseline-collection")),
                    (
                        "trial_source",
                        identity(target.snapshot(), "trial-collection"),
                    ),
                    ("provider", "semantic".into()),
                    ("operation", trial.operation.into()),
                    ("outcome", Value::Null),
                    (
                        "error",
                        "complete outcome exceeds retention budget; omitted".into(),
                    ),
                ]);
            } else {
                retained = required.unwrap();
            }
            stopped |= status != "returned" && matches!(trial.policy, Policy::Stop);
            outcomes.push(entry);
        }
        let report = Value::object([
            ("trials", Value::Array(outcomes)),
            ("invocations", (self.invocations as i64).into()),
            ("retention_limit", (self.retention as i64).into()),
        ]);
        // Fixed-size omission records and outer envelope have a separate reserve.
        if size(&report, 0).is_none_or(|n| n > self.retention.saturating_add(4096)) {
            return Err(ErrorKind::LimitExceeded);
        }
        Ok(report)
    }
}
impl Host for Comparison<'_> {
    fn operation_names(&self) -> Vec<(String, String)> {
        vec![("trials".into(), "compare".into())]
    }
    fn operations(&mut self) -> Result<Value, ErrorKind> {
        Ok(Value::Array(vec![Value::object([
            ("provider", "trials".into()),
            ("action", "compare".into()),
            ("effect", "Read".into()),
        ])]))
    }
    fn invoke(&mut self, provider: &str, action: &str, args: Value) -> Result<Value, ErrorKind> {
        if provider != "trials"
            || action != "compare"
            || args != Value::object([] as [(&str, Value); 0])
        {
            return Err(failure(
                "only predefined comparison with empty arguments is installed",
            ));
        }
        self.compare()
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let authorized = std::env::args().skip(1).eq(["--allow-read"]);
    let mut originals = ScratchBuffers::default();
    originals.create("example.declarations", language::SOURCE);
    let original = originals.selected().unwrap();
    let baseline = original.snapshot();
    let mut host = Comparison::new(baseline, authorized);
    let mut interpreter = Interpreter::new();
    interpreter.eval(include_str!("trial-comparison.scope"), &mut host)?;
    for value in interpreter.take_output() {
        println!("{value:?}");
    }
    assert_eq!(original.text(), baseline.text);
    assert_eq!(original.version(), baseline.version);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn baseline() -> SourceSnapshot<'static> {
        SourceSnapshot {
            id: scope_ide::source_edit::SourceId::Scratch(42),
            version: 7,
            language: "example.declarations",
            text: language::SOURCE,
        }
    }
    fn trials(report: &Value) -> &[Value] {
        let Value::Object(report) = report else {
            panic!()
        };
        let Value::Array(trials) = &report["trials"] else {
            panic!()
        };
        trials
    }
    fn field<'a>(value: &'a Value, key: &str) -> &'a Value {
        let Value::Object(object) = value else {
            panic!()
        };
        &object[key]
    }
    #[test]
    fn complete_script_and_preservation() {
        let mut host = Comparison::new(baseline(), true);
        let report = host.compare().unwrap();
        let t = trials(&report);
        assert_ne!(field(&t[0], "trial_source"), field(&t[1], "trial_source"));
        assert_ne!(field(&t[0], "outcome"), field(&t[1], "outcome"));
        // Independently dispatch each operation without the comparison envelope.
        for (trial, entry) in predefined().iter().zip(t) {
            let service = language::DeclarationLanguage::default();
            let mut buffers = ScratchBuffers::default();
            buffers.create(baseline().language, baseline().text);
            let target = buffers.selected().unwrap();
            let mut controller = SourceEditController::default();
            let provider = SemanticToolProvider::new(
                &service,
                target,
                &mut controller,
                PermittedEditScope::Document(target.source_id()),
                Some(trial.selection),
                |_| true,
            )
            .unwrap();
            let session = AgentSession::builder().provider(provider).build().unwrap();
            let mut direct =
                SemanticHost::new(session, ACTION, target.text(), target.version(), Some(true));
            assert_eq!(
                field(entry, "outcome"),
                &direct
                    .invoke("semantic", ACTION, trial.arguments.clone())
                    .unwrap()
            );
        }
        assert_eq!(field(&t[0], "status"), &Value::from("returned"));
        let mut host = Comparison::new(baseline(), true);
        let mut interpreter = Interpreter::new();
        let result = interpreter
            .eval(include_str!("trial-comparison.scope"), &mut host)
            .unwrap();
        assert_eq!(result, report);
        assert_eq!(interpreter.take_output(), vec![report]);
        assert!(host.compare().is_err());
        assert_eq!(host.invocations, 2);
    }
    #[test]
    fn validation_before_effects() {
        for count in [0, 1, 3] {
            let mut host = Comparison::new(baseline(), true);
            host.trials = (0..count).map(|_| predefined()[0].clone()).collect();
            assert!(host.compare().is_err());
            assert_eq!(host.invocations, 0);
        }
        let mut host = Comparison::new(baseline(), false);
        assert!(host.compare().is_err());
        assert_eq!(host.invocations, 0);
        let mut host = Comparison::new(baseline(), true);
        host.current.version += 1;
        assert!(host.compare().is_err());
        assert_eq!(host.invocations, 0);
        let mut host = Comparison::new(baseline(), true);
        host.trials[1].max_invocations = 2;
        assert!(host.compare().is_err());
        assert_eq!(host.invocations, 0);
    }
    #[test]
    fn failure_continue_stop_and_retention() {
        for policy in [Policy::Continue, Policy::Stop] {
            let mut host = Comparison::new(baseline(), true);
            host.trials[0].authorized = false;
            host.trials[0].policy = policy;
            let report = host.compare().unwrap();
            assert_eq!(
                field(&trials(&report)[0], "status"),
                &Value::from("invocation_error")
            );
            assert_eq!(
                host.invocations,
                if matches!(policy, Policy::Stop) { 1 } else { 2 }
            );
            assert_eq!(
                field(&trials(&report)[1], "status"),
                &Value::from(if matches!(policy, Policy::Stop) {
                    "skipped"
                } else {
                    "returned"
                })
            );
        }
        let mut host = Comparison::new(baseline(), true);
        host.retention = 0;
        let report = host.compare().unwrap();
        assert!(
            trials(&report)
                .iter()
                .all(|t| field(t, "status") == &Value::from("resource_limit"))
        );
        assert!(size(&report, 0).unwrap() < 4096);
    }
}
