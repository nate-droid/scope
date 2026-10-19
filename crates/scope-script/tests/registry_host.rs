#[path = "../examples/support/mod.rs"]
mod support;

use scope_ide::{
    document::{
        DocumentId,
        session::{DocumentSession, DocumentSnapshot},
    },
    tool::{ToolArguments, ToolValue, operations::*},
    workspace::Workspace,
};
use scope_script::{ErrorKind, Interpreter, Value, host::RegistryHost};
use std::{cell::Cell, rc::Rc};

#[test]
fn retained_history_is_bounded_and_can_be_drained_by_host() {
    let mut fixture = Fixture::new(Mode::Inspect);
    let calls = fixture.calls.clone();
    let policy = |_: OperationAuthorization<'_>| true;
    let mut host = fixture.host(Some(&policy));
    let mut interpreter = Interpreter::new();
    for _ in 0..64 {
        interpreter.eval(INVOKE, &mut host).unwrap();
    }
    assert!(matches!(
        interpreter.eval(INVOKE, &mut host).unwrap_err().kind,
        ErrorKind::LimitExceeded
    ));
    assert_eq!(calls.get(), 64);
    host.results.clear();
    interpreter.eval(INVOKE, &mut host).unwrap();
    assert_eq!(calls.get(), 65);
}

const TEXT: &str = "Actual snapshot: café 🦀\n";
const INVOKE: &str =
    "scope.invoke(\"example_inspection\", \"inspect\", {options: {include_text: true}})";

#[derive(Clone, Copy)]
enum Mode {
    Inspect,
    Fail,
    InvalidOutput,
}
struct ObservedProvider {
    inner: support::InspectionProvider,
    calls: Rc<Cell<usize>>,
    mode: Mode,
}
impl OperationProvider for ObservedProvider {
    fn provider_id(&self) -> &str {
        self.inner.provider_id()
    }
    fn descriptors(&self) -> &[OperationDescriptor] {
        self.inner.descriptors()
    }
    fn invoke(
        &self,
        action: &str,
        snapshot: &DocumentSnapshot,
        args: &ToolArguments,
    ) -> Result<ToolValue, OperationFailure> {
        self.calls.set(self.calls.get() + 1);
        assert_eq!(snapshot.text, TEXT);
        match self.mode {
            Mode::Inspect => self.inner.invoke(action, snapshot, args),
            Mode::Fail => Err(OperationFailure {
                message: "inspection failed".into(),
            }),
            Mode::InvalidOutput => Ok(Value::object([
                ("text", TEXT.into()),
                ("bytes", "not an integer".into()),
            ])),
        }
    }
}
struct Fixture {
    directory: tempfile::TempDir,
    session: DocumentSession,
    registry: OperationRegistry,
    calls: Rc<Cell<usize>>,
}
impl Fixture {
    fn new(mode: Mode) -> Self {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("source.txt"), TEXT).unwrap();
        let session = DocumentSession::open(
            Workspace::read_only(directory.path().to_string_lossy()).unwrap(),
            DocumentId::new(42),
            "source.txt",
        )
        .unwrap();
        let calls = Rc::new(Cell::new(0));
        let mut registry = OperationRegistry::new(OperationLimits::default());
        registry
            .register(Box::new(ObservedProvider {
                inner: support::InspectionProvider::new(),
                calls: calls.clone(),
                mode,
            }))
            .unwrap();
        Self {
            directory,
            session,
            registry,
            calls,
        }
    }
    fn host<'a>(
        &'a mut self,
        policy: Option<&'a dyn Fn(OperationAuthorization<'_>) -> bool>,
    ) -> RegistryHost<'a> {
        let expected = self.session.snapshot().unwrap().revision;
        RegistryHost {
            registry: &self.registry,
            session: &mut self.session,
            expected,
            policy,
            results: vec![],
        }
    }
}
fn allow(_: OperationAuthorization<'_>) -> bool {
    true
}
fn operation_error(source: &str, host: &mut RegistryHost<'_>) -> OperationError {
    match Interpreter::new().eval(source, host).unwrap_err().kind {
        ErrorKind::Operation(error) => error,
        other => panic!("expected structured operation error, got {other:?}"),
    }
}

#[test]
fn structured_discovery_invocation_projection_and_retained_identity() {
    let mut fixture = Fixture::new(Mode::Inspect);
    let snapshot = fixture.session.snapshot().unwrap();
    let policy_calls = Cell::new(0);
    let policy = |context: OperationAuthorization<'_>| {
        policy_calls.set(policy_calls.get() + 1);
        assert_eq!(context.snapshot.text, TEXT);
        assert_eq!(context.call.operation.provider_id, "example_inspection");
        assert!(context.descriptor.authorization.contains("inspect_source"));
        true
    };
    let mut host = fixture.host(Some(&policy));
    let mut interpreter = Interpreter::new();
    assert_eq!(
        interpreter
            .eval(
                "let ops = scope.operations(); ops[0].input.fields.options.fields.include_text",
                &mut host
            )
            .unwrap(),
        "boolean".into()
    );
    assert_eq!(policy_calls.get(), 0);
    assert!(host.results.is_empty());
    assert_eq!(
        interpreter
            .eval("ops[0].output.fields.bytes", &mut host)
            .unwrap(),
        "integer".into()
    );
    assert_eq!(interpreter.eval("let result = scope.invoke(ops[0].provider, ops[0].action, {options: {include_text: true}}); [result.text, result[\"bytes\"]]", &mut host).unwrap(), Value::Array(vec![TEXT.into(), Value::Integer(TEXT.len() as i64)]));
    assert_eq!(host.results.len(), 1);
    assert_eq!(host.results[0].document_id, snapshot.document_id);
    assert_eq!(host.results[0].revision, snapshot.revision);
    assert_eq!(
        interpreter.eval("result", &mut host).unwrap(),
        host.results[0].value
    );
    assert_eq!(policy_calls.get(), 1);
    drop(host);
    assert_eq!(fixture.calls.get(), 1);
}

#[test]
fn invalid_nested_arguments_are_rejected_before_policy_and_provider() {
    let mut fixture = Fixture::new(Mode::Inspect);
    let policy_calls = Cell::new(0);
    let policy = |_: OperationAuthorization<'_>| {
        policy_calls.set(policy_calls.get() + 1);
        true
    };
    let mut host = fixture.host(Some(&policy));
    for (args, expected) in [
        (
            "{options: {include_text: 1}}",
            OperationValidation::TypeMismatch,
        ),
        ("{options: {}}", OperationValidation::MissingRequiredField),
        (
            "{options: {include_text: true, extra: false}}",
            OperationValidation::UnknownField,
        ),
    ] {
        let source = format!("scope.invoke(\"example_inspection\", \"inspect\", {args})");
        assert!(
            matches!(operation_error(&source, &mut host), OperationError::InvalidInput(actual) if actual == expected)
        );
    }
    assert!(host.results.is_empty());
    assert_eq!(policy_calls.get(), 0);
    drop(host);
    assert_eq!(fixture.calls.get(), 0);
}

#[test]
fn missing_operations_and_absent_or_denied_policy_never_dispatch() {
    let mut fixture = Fixture::new(Mode::Inspect);
    let policy_calls = Cell::new(0);
    let deny = |_: OperationAuthorization<'_>| {
        policy_calls.set(policy_calls.get() + 1);
        false
    };
    for policy in [
        None,
        Some(&deny as &dyn Fn(OperationAuthorization<'_>) -> bool),
    ] {
        let mut host = fixture.host(policy);
        for source in [
            "scope.invoke(\"missing\", \"inspect\", {})",
            "scope.invoke(\"example_inspection\", \"missing\", {})",
        ] {
            assert!(matches!(
                operation_error(source, &mut host),
                OperationError::UnknownOperation
            ));
        }
        assert!(matches!(
            operation_error(INVOKE, &mut host),
            OperationError::Unauthorized
        ));
        assert!(host.results.is_empty());
    }
    assert_eq!(policy_calls.get(), 1);
    assert_eq!(fixture.calls.get(), 0);
}

#[test]
fn host_file_edit_invalidates_revision_without_dispatch_or_losing_results() {
    let mut fixture = Fixture::new(Mode::Inspect);
    let path = fixture.directory.path().join("source.txt");
    let mut host = fixture.host(Some(&allow));
    Interpreter::new().eval(INVOKE, &mut host).unwrap();
    let retained = host.results.clone();
    std::fs::write(path, "Changed by the Rust host, not the script").unwrap();
    assert!(matches!(
        operation_error(INVOKE, &mut host),
        OperationError::StaleRevision
    ));
    assert_eq!(host.results, retained);
    drop(host);
    assert_eq!(fixture.calls.get(), 1);
}

#[test]
fn provider_failure_and_invalid_output_remain_structured_and_are_not_retained() {
    for mode in [Mode::Fail, Mode::InvalidOutput] {
        let mut fixture = Fixture::new(mode);
        let mut host = fixture.host(Some(&allow));
        let error = operation_error(INVOKE, &mut host);
        match mode {
            Mode::Fail => assert!(
                matches!(error, OperationError::ProviderFailure(OperationFailure { message }) if message == "inspection failed")
            ),
            Mode::InvalidOutput => assert!(matches!(
                error,
                OperationError::InvalidOutput(OperationValidation::TypeMismatch)
            )),
            Mode::Inspect => unreachable!(),
        }
        assert!(host.results.is_empty());
        drop(host);
        assert_eq!(fixture.calls.get(), 1);
    }
}

#[test]
fn repl_preserves_bindings_across_lines_with_real_registry_host() {
    let mut fixture = Fixture::new(Mode::Inspect);
    let mut host = fixture.host(Some(&allow));
    let input = format!(
        "let result = {INVOKE}\nlet bytes = result.bytes\nprint(result.text)\nbytes\n:quit\n"
    );
    let mut output = Vec::new();
    scope_script::repl::run(std::io::Cursor::new(input), &mut output, &mut host).unwrap();
    let output = String::from_utf8(output).unwrap();
    assert!(
        output.contains(&format!("{:?}", Value::String(TEXT.into()))),
        "{output}"
    );
    assert!(
        output.contains(&format!("scope> => Integer({})", TEXT.len())),
        "{output}"
    );
    assert!(!output.contains("error"), "{output}");
    assert_eq!(host.results.len(), 1);
    drop(host);
    assert_eq!(fixture.calls.get(), 1);
}
