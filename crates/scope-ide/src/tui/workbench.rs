//! Keyboard consumer of the headless workbench; never reads App buffers.
use std::collections::BTreeMap;

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::{
    layout::Rect,
    widgets::{Block, Borders, Paragraph, Wrap},
};
use scope_language::Diagnostic;

use super::{Pane, TuiContext, TuiContextMut};
use crate::{
    document::{DocumentSnapshot, EditDecision},
    tool::{
        ToolValue,
        operations::{OperationAuthorization, OperationCall, OperationResult},
    },
    workbench::{
        OperationApproval, OperationPurpose, OperationRequest, VerificationOutcome,
        VerificationReport, Workbench,
    },
};

type Policy = Box<dyn for<'a> FnMut(OperationAuthorization<'a>) -> bool>;
enum Interpreter {
    Outcome(Box<dyn FnMut(&OperationResult) -> VerificationOutcome>),
    Report(Box<dyn FnMut(&OperationResult) -> VerificationReport>),
}
type Diagnostics = Box<dyn FnMut(&DocumentSnapshot) -> Vec<Diagnostic>>;

/// Owns a capability-backed controller and explicit host callbacks. There is no
/// default policy or verification interpretation. Diagnostics must describe the
/// exact candidate passed to the callback; an empty result is not proof of safety.
///
/// F1 refresh, F2 edit arguments, F3/F4 select operation, F5 cycle purpose,
/// F6 queue, F7 approve invocation, F8 cancel, F9 review, F10 accept/F11 reject
/// (then Enter confirms). F12 abandons editing/confirmation. Page keys scroll.
/// Arguments use a bounded JSON subset: objects, arrays, strings (including
/// `\uXXXX` escapes), signed integers, booleans and null; no floats.
/// The standard TUI reserves q/Esc/Tab; enter q as `\u0071` in its editor.
pub struct WorkbenchPane {
    controller: Workbench,
    policy: Policy,
    interpret: Interpreter,
    diagnostics: Diagnostics,
    snapshot: Option<DocumentSnapshot>,
    last_result: Option<OperationResult>,
    selected: usize,
    purpose: OperationPurpose,
    arguments: String,
    editing: bool,
    confirmation: Option<EditDecision>,
    error: Option<String>,
}

impl WorkbenchPane {
    /// Outcome-only convenience constructor. The controller records an explicit
    /// lack of check-specific coverage. Performs initial snapshot I/O; failures
    /// remain visible and can be retried with F1.
    pub fn new(
        controller: Workbench,
        policy: impl for<'a> FnMut(OperationAuthorization<'a>) -> bool + 'static,
        interpret: impl FnMut(&OperationResult) -> VerificationOutcome + 'static,
        diagnostics: impl FnMut(&DocumentSnapshot) -> Vec<Diagnostic> + 'static,
    ) -> Self {
        Self::initialize(
            controller,
            Box::new(policy),
            Interpreter::Outcome(Box::new(interpret)),
            Box::new(diagnostics),
        )
    }

    /// Uses a named verification report with check-specific coverage limitations.
    /// Like `new`, performs initial snapshot I/O; failures are displayed in the pane.
    pub fn with_verification_report(
        controller: Workbench,
        policy: impl for<'a> FnMut(OperationAuthorization<'a>) -> bool + 'static,
        interpret: impl FnMut(&OperationResult) -> VerificationReport + 'static,
        diagnostics: impl FnMut(&DocumentSnapshot) -> Vec<Diagnostic> + 'static,
    ) -> Self {
        Self::initialize(
            controller,
            Box::new(policy),
            Interpreter::Report(Box::new(interpret)),
            Box::new(diagnostics),
        )
    }

    fn initialize(
        controller: Workbench,
        policy: Policy,
        interpret: Interpreter,
        diagnostics: Diagnostics,
    ) -> Self {
        let mut pane = Self {
            controller,
            policy,
            interpret,
            diagnostics,
            snapshot: None,
            last_result: None,
            selected: 0,
            purpose: OperationPurpose::Inspect,
            arguments: "{}".into(),
            editing: false,
            confirmation: None,
            error: None,
        };
        pane.refresh();
        pane
    }

    pub fn controller(&self) -> &Workbench {
        &self.controller
    }

    fn refresh(&mut self) -> bool {
        // Drop the old display before I/O, including on failure.
        self.snapshot = None;
        match self.controller.snapshot() {
            Ok(snapshot) => {
                if self
                    .last_result
                    .as_ref()
                    .is_some_and(|result| result.revision != snapshot.revision)
                {
                    self.last_result = None;
                }
                self.snapshot = Some(snapshot);
                true
            }
            Err(error) => {
                self.last_result = None;
                self.error = Some(error.to_string());
                false
            }
        }
    }

    /// Input-only transition, also usable by hosts with their own event routing.
    pub fn handle_input(&mut self, key: KeyEvent) -> bool {
        if key.kind != KeyEventKind::Press {
            return false;
        }
        if key.code == KeyCode::F(12) {
            self.editing = false;
            self.confirmation = None;
            return true;
        }
        if let Some(decision) = self.confirmation {
            if key.code == KeyCode::Enter {
                self.confirmation = None;
                if !self.refresh() {
                    return true;
                }
                let result = self.controller.decide_edit(decision);
                if let Err(error) = result {
                    self.error = Some(error.to_string());
                }
                self.refresh();
            }
            return true;
        }
        if self.editing {
            match key.code {
                KeyCode::Enter => self.editing = false,
                KeyCode::Backspace => {
                    self.arguments.pop();
                }
                KeyCode::Char(c)
                    if !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                        && self.arguments.len() < 65_536 =>
                {
                    self.arguments.push(c)
                }
                _ => {}
            }
            return true;
        }
        let KeyCode::F(action @ 1..=11) = key.code else {
            return false;
        };
        self.error = None;
        match action {
            1 => {
                self.refresh();
            }
            2 => self.editing = true,
            3 | 4 => {
                let count = self.controller.operations().count();
                if count > 0 {
                    self.selected = if action == 3 {
                        (self.selected + count - 1) % count
                    } else {
                        (self.selected + 1) % count
                    };
                    self.arguments = "{}".into();
                }
            }
            5 => {
                self.purpose = match self.purpose {
                    OperationPurpose::Inspect => OperationPurpose::ProposeEdit,
                    OperationPurpose::ProposeEdit => OperationPurpose::Verify,
                    OperationPurpose::Verify => OperationPurpose::Inspect,
                }
            }
            6 => {
                let result = parse_arguments(&self.arguments);
                match result {
                    Err(error) => self.error = Some(error),
                    Ok(arguments) => {
                        if self.refresh() {
                            let operation = self
                                .controller
                                .operations()
                                .nth(self.selected)
                                .map(|(id, _)| id);
                            if let Some(operation) = operation {
                                let request = OperationRequest {
                                    call: OperationCall {
                                        operation,
                                        arguments,
                                    },
                                    revision: self.snapshot.as_ref().unwrap().revision.clone(),
                                    purpose: self.purpose,
                                };
                                if let Err(error) = self.controller.request(request) {
                                    self.error = Some(error.to_string());
                                }
                            } else {
                                self.error = Some("No operation selected".into());
                            }
                        }
                    }
                }
            }
            7 => {
                // Explicit key approval still cannot bypass the host's call-aware policy.
                self.last_result = None;
                let result = match &mut self.interpret {
                    Interpreter::Outcome(interpret) => self.controller.approve(
                        OperationApproval::Allow,
                        &mut self.policy,
                        interpret,
                    ),
                    Interpreter::Report(interpret) => self.controller.approve_with_verification(
                        OperationApproval::Allow,
                        &mut self.policy,
                        interpret,
                    ),
                };
                match result {
                    Ok(result) => self.last_result = Some(result),
                    Err(error) => self.error = Some(error.to_string()),
                }
                self.refresh();
            }
            8 => {
                if let Err(error) = self.controller.cancel() {
                    self.error = Some(error.to_string());
                }
            }
            9 => {
                if self.refresh() {
                    if let Some(proposal) = self.controller.proposal() {
                        let diagnostics = (self.diagnostics)(proposal.proposal().candidate());
                        if let Err(error) = self.controller.review_proposal(diagnostics) {
                            self.error = Some(error.to_string());
                        }
                    } else {
                        self.error = Some("No proposal to review".into());
                    }
                }
            }
            10 | 11 => {
                if self.controller.review().is_some() {
                    self.confirmation = Some(if action == 10 {
                        EditDecision::Allow
                    } else {
                        EditDecision::Deny
                    });
                } else {
                    self.error = Some("Review with F9 before deciding".into());
                }
            }
            _ => unreachable!(),
        }
        true
    }

    fn display(&self) -> String {
        let mut text = String::from(
            "F1 refresh | F2 arguments | F3/F4 operation | F5 purpose\nF6 queue | F7 approve | F8 cancel | F9 review | F10 accept/F11 reject\nEnter finish/confirm | F12 abandon | PgUp/PgDn scroll\n",
        );
        if let Some(decision) = self.confirmation {
            text.push_str(&format!(
                "CONFIRM {decision:?}: Enter confirms exact reviewed candidate; F12 abandons\n"
            ));
        }
        if let Some(error) = &self.error {
            text.push_str(&format!("Error: {error}\n"));
        }
        match &self.snapshot {
            Some(snapshot) => text.push_str(&format!(
                "Document {} {} revision {:?}\n{}\n",
                snapshot.document_id, snapshot.path, snapshot.revision, snapshot.text
            )),
            None => text.push_str("Snapshot unavailable: F1 refresh (no App buffer used)\n"),
        }
        text.push_str("Provider catalog (metadata grants no authority):\n");
        for (index, (id, descriptor)) in self.controller.operations().enumerate() {
            text.push_str(&format!("{} {}/{}: {}\n  effect {:?}; execution {:?}; permissions {:?}\n  input {:?}\n  output {:?}\n",
                if index == self.selected { ">" } else { " " }, id.provider_id, id.action_id,
                descriptor.description, descriptor.effect, descriptor.execution, descriptor.authorization,
                descriptor.input, descriptor.output));
        }
        text.push_str(&format!(
            "Purpose {:?}; arguments{}: {}\nRequest {:?}: {:?}\nProposal {:?}\nVerification {:?}\n",
            self.purpose,
            if self.editing { " [editing]" } else { "" },
            self.arguments,
            self.controller.request_status(),
            self.controller.pending(),
            self.controller.proposal_status(),
            self.controller.verification_status()
        ));
        // Hide retained review/evidence when the source cannot be validated.
        if self.snapshot.is_some() {
            if let Some(result) = &self.last_result {
                text.push_str(&format!(
                    "Last operation result: document {} revision {:?}\nContents: {:?}\n",
                    result.document_id, result.revision, result.value
                ));
            }
            if let Some(edit) = self.controller.proposal() {
                let proposal = edit.proposal();
                text.push_str(&format!(
                    "Proposal base {:?}; candidate {:?}; origin {}\n",
                    proposal.base().revision,
                    proposal.candidate().revision,
                    proposal.provenance()
                ));
                if let Some(review) = self.controller.review() {
                    text.push_str(review.diff());
                    text.push_str(&format!(
                        "Candidate diagnostics: {:?}\n",
                        review.diagnostics()
                    ));
                } else {
                    text.push_str("Unreviewed full replacement:\n--- base\n");
                    text.push_str(&proposal.base().text);
                    text.push_str("\n+++ candidate\n");
                    text.push_str(&proposal.candidate().text);
                    text.push_str("\nDiagnostics not computed; F9 review\n");
                }
            }
            if let Some(evidence) = self.controller.verification() {
                text.push_str(&format!(
                    "Verification check: {}\nOutcome: {:?}; evidence revision {:?}\nLimitations:\n",
                    evidence.check_name, evidence.outcome, evidence.result.revision
                ));
                if evidence.limitations.is_empty() {
                    text.push_str(
                        "  None supplied; this does not imply exhaustive verification.\n",
                    );
                } else {
                    for limitation in &evidence.limitations {
                        text.push_str(&format!("  - {limitation}\n"));
                    }
                }
                text.push_str(&format!("Evidence contents: {:?}\n", evidence.result.value));
            }
        }
        text.push_str("Evidence covers only its accepted revision, not a future candidate; passing is not save authority.\nTrusted native providers may block or have side effects; cancellation only removes queued work.\nChecked saves are best-effort conflict detection, not atomic compare-and-swap.\nMessages:\n");
        for message in self.controller.messages() {
            text.push_str(&format!("{:?}: {}\n", message.kind, message.content));
        }
        text
    }
}

impl Pane for WorkbenchPane {
    fn id(&self) -> &'static str {
        "scope.workbench"
    }
    fn render(&self, frame: &mut ratatui::Frame<'_>, area: Rect, ctx: &TuiContext<'_>) {
        frame.render_widget(
            Paragraph::new(self.display())
                .wrap(Wrap { trim: false })
                .scroll((ctx.scroll(), 0))
                .block(Block::default().borders(Borders::ALL).title("Workbench")),
            area,
        );
    }
    fn handle_key(&mut self, key: KeyEvent, _ctx: &mut TuiContextMut<'_>) -> bool {
        self.handle_input(key)
    }
}

// Small bounded JSON-subset parser; schema and authorization validation remain
// exclusively in the registry. Duplicate keys and trailing data are rejected.
fn parse_arguments(input: &str) -> Result<BTreeMap<String, ToolValue>, String> {
    struct Parser<'a> {
        chars: std::iter::Peekable<std::str::Chars<'a>>,
        nodes: usize,
    }
    impl Parser<'_> {
        fn space(&mut self) {
            while self.chars.peek().is_some_and(|c| c.is_whitespace()) {
                self.chars.next();
            }
        }
        fn expect(&mut self, c: char) -> Result<(), String> {
            self.space();
            if self.chars.next() == Some(c) {
                Ok(())
            } else {
                Err(format!("Expected {c}"))
            }
        }
        fn string(&mut self) -> Result<String, String> {
            self.expect('"')?;
            let mut result = String::new();
            loop {
                match self.chars.next().ok_or("Unterminated string")? {
                    '"' => return Ok(result),
                    '\\' => result.push(match self.chars.next().ok_or("Incomplete escape")? {
                        '"' => '"',
                        '\\' => '\\',
                        '/' => '/',
                        'n' => '\n',
                        'r' => '\r',
                        't' => '\t',
                        'b' => '\u{8}',
                        'f' => '\u{c}',
                        'u' => {
                            let mut code = 0;
                            for _ in 0..4 {
                                code = code * 16
                                    + self
                                        .chars
                                        .next()
                                        .and_then(|c| c.to_digit(16))
                                        .ok_or("Invalid Unicode escape")?;
                            }
                            char::from_u32(code)
                                .ok_or("Unicode escape must be a scalar (no surrogate pairs)")?
                        }
                        _ => return Err("Invalid escape".into()),
                    }),
                    c if c.is_control() => return Err("Escape control characters".into()),
                    c => result.push(c),
                }
            }
        }
        fn value(&mut self, depth: usize) -> Result<ToolValue, String> {
            self.nodes += 1;
            if depth > 32 || self.nodes > 4096 {
                return Err("Argument complexity limit exceeded".into());
            }
            self.space();
            match self.chars.peek().copied().ok_or("Expected value")? {
                '"' => Ok(ToolValue::String(self.string()?)),
                '{' => {
                    self.chars.next();
                    self.space();
                    let mut fields = BTreeMap::new();
                    if self.chars.peek() == Some(&'}') {
                        self.chars.next();
                        return Ok(ToolValue::Object(fields));
                    }
                    loop {
                        let key = self.string()?;
                        self.expect(':')?;
                        if fields.insert(key, self.value(depth + 1)?).is_some() {
                            return Err("Duplicate key".into());
                        }
                        self.space();
                        match self.chars.next() {
                            Some('}') => break,
                            Some(',') => {}
                            _ => return Err("Expected comma or }".into()),
                        }
                    }
                    Ok(ToolValue::Object(fields))
                }
                '[' => {
                    self.chars.next();
                    self.space();
                    let mut values = Vec::new();
                    if self.chars.peek() == Some(&']') {
                        self.chars.next();
                        return Ok(ToolValue::Array(values));
                    }
                    loop {
                        values.push(self.value(depth + 1)?);
                        self.space();
                        match self.chars.next() {
                            Some(']') => break,
                            Some(',') => {}
                            _ => return Err("Expected comma or ]".into()),
                        }
                    }
                    Ok(ToolValue::Array(values))
                }
                _ => {
                    let mut token = String::new();
                    while self
                        .chars
                        .peek()
                        .is_some_and(|c| !c.is_whitespace() && !matches!(c, ',' | '}' | ']'))
                    {
                        token.push(self.chars.next().unwrap());
                    }
                    match token.as_str() {
                        "null" => Ok(ToolValue::Null),
                        "true" => Ok(ToolValue::Boolean(true)),
                        "false" => Ok(ToolValue::Boolean(false)),
                        _ => {
                            let digits = token.strip_prefix('-').unwrap_or(&token);
                            if digits.is_empty()
                                || !digits.bytes().all(|c| c.is_ascii_digit())
                                || (digits.len() > 1 && digits.starts_with('0'))
                            {
                                return Err("Expected null, boolean or signed integer".into());
                            }
                            token
                                .parse()
                                .map(ToolValue::Integer)
                                .map_err(|_| "Integer out of range".into())
                        }
                    }
                }
            }
        }
    }
    if input.len() > 65_536 {
        return Err("Arguments exceed 64 KiB".into());
    }
    let mut parser = Parser {
        chars: input.chars().peekable(),
        nodes: 0,
    };
    let value = parser.value(0)?;
    parser.space();
    if parser.chars.next().is_some() {
        return Err("Trailing argument data".into());
    }
    match value {
        ToolValue::Object(fields) => Ok(fields),
        _ => Err("Arguments must be an object".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        app::App,
        document::{DocumentId, DocumentSession},
        test_support::TempWorkspace,
        tool::{
            ToolArguments, ToolEffect,
            operations::{
                OperationDescriptor, OperationExecution, OperationFailure, OperationLimits,
                OperationProvider, OperationRegistry, OperationSchema,
            },
        },
        workbench::{ProposalStatus, RequestStatus, VerificationStatus},
    };
    use ratatui::{Terminal, backend::TestBackend};
    use std::{cell::Cell, rc::Rc};

    struct Provider {
        descriptors: Vec<OperationDescriptor>,
        calls: Rc<Cell<usize>>,
    }
    impl OperationProvider for Provider {
        fn provider_id(&self) -> &str {
            "test"
        }
        fn descriptors(&self) -> &[OperationDescriptor] {
            &self.descriptors
        }
        fn invoke(
            &self,
            _: &str,
            _: &DocumentSnapshot,
            _: &ToolArguments,
        ) -> Result<ToolValue, OperationFailure> {
            self.calls.set(self.calls.get() + 1);
            Ok(ToolValue::String("replacement".into()))
        }
    }
    fn setup(allow: bool) -> (TempWorkspace, WorkbenchPane, Rc<Cell<usize>>) {
        let fixture = TempWorkspace::new("tui_workbench");
        fixture.write_file("source.txt", "original");
        let session =
            DocumentSession::open(fixture.workspace(), DocumentId::new(1), "source.txt").unwrap();
        let calls = Rc::new(Cell::new(0));
        let mut registry = OperationRegistry::new(OperationLimits::default());
        registry
            .register(Box::new(Provider {
                calls: calls.clone(),
                descriptors: vec![OperationDescriptor {
                    action_id: "replace".into(),
                    description: "Replacement".into(),
                    input: OperationSchema::Object {
                        fields: BTreeMap::new(),
                        required: Default::default(),
                    },
                    output: OperationSchema::String,
                    effect: ToolEffect::Write,
                    execution: OperationExecution::Blocking,
                    authorization: ["host".into()].into(),
                }],
            }))
            .unwrap();
        let pane = WorkbenchPane::new(
            Workbench::new(session, registry),
            move |ctx| {
                allow
                    && ctx.call.arguments.is_empty()
                    && ctx.descriptor.authorization.contains("host")
                    && ctx.snapshot.text == "original"
            },
            |_| VerificationOutcome::Inconclusive,
            |_| vec![],
        );
        (fixture, pane, calls)
    }
    fn press(pane: &mut WorkbenchPane, code: KeyCode) {
        assert!(pane.handle_input(KeyEvent::new(code, KeyModifiers::NONE)));
    }
    fn f(pane: &mut WorkbenchPane, n: u8) {
        press(pane, KeyCode::F(n));
    }
    fn render(pane: &WorkbenchPane, app: &App) -> String {
        let mut terminal = Terminal::new(TestBackend::new(120, 60)).unwrap();
        terminal
            .draw(|frame| pane.render(frame, frame.area(), &TuiContext::new(app)))
            .unwrap();
        terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect()
    }
    #[test]
    fn queue_approval_cancel_and_policy_are_separate() {
        let (_fixture, mut pane, calls) = setup(false);
        f(&mut pane, 6);
        assert_eq!(calls.get(), 0);
        assert_eq!(
            pane.controller.request_status(),
            RequestStatus::AwaitingApproval
        );
        f(&mut pane, 8);
        assert_eq!(pane.controller.request_status(), RequestStatus::Cancelled);
        f(&mut pane, 6);
        f(&mut pane, 7);
        assert_eq!(calls.get(), 0);
        assert_eq!(pane.controller.request_status(), RequestStatus::Denied);
    }
    #[test]
    fn proposal_requires_review_and_confirmed_decision() {
        let (fixture, mut pane, calls) = setup(true);
        f(&mut pane, 5);
        f(&mut pane, 6);
        f(&mut pane, 7);
        assert_eq!(calls.get(), 1);
        assert_eq!(
            std::fs::read_to_string(fixture.root().join("source.txt")).unwrap(),
            "original"
        );
        f(&mut pane, 10);
        assert!(pane.confirmation.is_none());
        f(&mut pane, 9);
        assert!(
            pane.controller
                .review()
                .unwrap()
                .diff()
                .contains("+replacement")
        );
        f(&mut pane, 10);
        f(&mut pane, 12);
        assert_eq!(pane.controller.proposal_status(), ProposalStatus::Reviewed);
        f(&mut pane, 11);
        press(&mut pane, KeyCode::Enter);
        assert_eq!(pane.controller.proposal_status(), ProposalStatus::Rejected);
        f(&mut pane, 6);
        f(&mut pane, 7);
        f(&mut pane, 9);
        f(&mut pane, 10);
        press(&mut pane, KeyCode::Enter);
        assert_eq!(pane.controller.proposal_status(), ProposalStatus::Applied);
        assert_eq!(
            std::fs::read_to_string(fixture.root().join("source.txt")).unwrap(),
            "replacement"
        );
    }
    #[test]
    fn render_is_io_free_and_failed_refresh_clears_snapshot() {
        let (fixture, mut pane, calls) = setup(true);
        let app = App::new(fixture.workspace()).unwrap();
        assert!(render(&pane, &app).contains("original"));
        assert!(pane.snapshot.is_some());
        f(&mut pane, 1);
        let display = render(&pane, &app);
        for expected in [
            "revision",
            "Write",
            "Blocking",
            "permissions",
            "input",
            "output",
            "original",
        ] {
            assert!(display.contains(expected), "{expected}");
        }
        std::fs::remove_file(fixture.root().join("source.txt")).unwrap();
        assert!(render(&pane, &app).contains("original"));
        f(&mut pane, 1);
        assert!(pane.snapshot.is_none());
        assert!(!render(&pane, &app).contains("original"));
        assert_eq!(calls.get(), 0);
    }
    #[test]
    fn stale_queue_and_verification_are_not_success() {
        let (fixture, mut pane, calls) = setup(true);
        f(&mut pane, 5);
        f(&mut pane, 5);
        f(&mut pane, 6);
        fixture.write_file("source.txt", "changed");
        f(&mut pane, 7);
        assert_eq!(calls.get(), 0);
        assert_eq!(
            pane.controller.verification_status(),
            VerificationStatus::Stale
        );
        let (_fixture, mut pane, calls) = setup(true);
        f(&mut pane, 5);
        f(&mut pane, 5);
        f(&mut pane, 6);
        f(&mut pane, 7);
        assert_eq!(calls.get(), 1);
        assert_eq!(
            pane.controller.verification_status(),
            VerificationStatus::Inconclusive
        );
        assert!(pane.display().contains("not save authority"));
    }
    #[test]
    fn named_reports_render_check_specific_limitations() {
        for outcome in [
            VerificationOutcome::Passed,
            VerificationOutcome::Skipped,
            VerificationOutcome::Unavailable,
        ] {
            let (fixture, original, calls) = setup(true);
            let mut pane = WorkbenchPane::with_verification_report(
                original.controller,
                |ctx| {
                    ctx.call.arguments.is_empty() && ctx.descriptor.authorization.contains("host")
                },
                move |_| VerificationReport {
                    check_name: "Integration checks".into(),
                    outcome,
                    limitations: vec!["Database prerequisite absent".into()],
                },
                |_| vec![],
            );
            assert!(pane.snapshot.is_some());
            assert_eq!(calls.get(), 0);
            f(&mut pane, 5);
            f(&mut pane, 5);
            f(&mut pane, 6);
            f(&mut pane, 7);
            let app = App::new(fixture.workspace()).unwrap();
            let display = render(&pane, &app);
            assert!(display.contains("Verification check: Integration checks"));
            assert!(display.contains("Limitations:"));
            assert!(display.contains("Database prerequisite absent"));
            assert_eq!(pane.controller.verification().unwrap().outcome, outcome);
            assert_eq!(calls.get(), 1);
        }
    }

    #[test]
    fn inspect_result_is_visible_and_cleared_on_failure_or_revision_change() {
        let (fixture, mut pane, _) = setup(true);
        let app = App::new(fixture.workspace()).unwrap();
        f(&mut pane, 6);
        f(&mut pane, 7);
        let display = render(&pane, &app);
        assert!(display.contains("Last operation result: document 1 revision"));
        assert!(display.contains("Contents: String(\"replacement\")"));
        f(&mut pane, 7); // No request: do not retain the previous success.
        assert!(pane.last_result.is_none());
        f(&mut pane, 6);
        f(&mut pane, 7);
        fixture.write_file("source.txt", "changed");
        f(&mut pane, 1);
        assert!(pane.last_result.is_none());
        assert!(!render(&pane, &app).contains("replacement"));
    }

    #[test]
    fn constructor_failure_and_stale_retry_remain_visible() {
        let (fixture, original, calls) = setup(true);
        std::fs::remove_file(fixture.root().join("source.txt")).unwrap();
        let mut pane = WorkbenchPane::new(
            original.controller,
            |_| false,
            |_| VerificationOutcome::Inconclusive,
            |_| vec![],
        );
        assert!(pane.snapshot.is_none());
        assert!(pane.error.is_some());
        assert!(pane.display().contains("Snapshot unavailable"));
        assert_eq!(calls.get(), 0);
        fixture.write_file("source.txt", "restored");
        f(&mut pane, 1);
        // Refresh does not accept externally replaced source: the controller
        // retains authority over reload and revision conflict handling.
        assert!(pane.snapshot.is_none());
        assert!(pane.error.is_some());
        assert!(!pane.display().contains("restored"));
    }

    #[test]
    fn outcome_convenience_and_empty_report_do_not_imply_full_coverage() {
        let (_fixture, mut pane, _) = setup(true);
        f(&mut pane, 5);
        f(&mut pane, 5);
        f(&mut pane, 6);
        f(&mut pane, 7);
        assert!(pane.display().contains("Verification check: test/replace"));
        assert!(
            pane.display()
                .contains("Outcome-only interpreter supplied no check-specific")
        );
        let mut pane = WorkbenchPane::with_verification_report(
            pane.controller,
            |_| true,
            |_| VerificationReport {
                check_name: "Syntax".into(),
                outcome: VerificationOutcome::Passed,
                limitations: vec![],
            },
            |_| vec![],
        );
        f(&mut pane, 5);
        f(&mut pane, 5);
        f(&mut pane, 6);
        f(&mut pane, 7);
        assert!(
            pane.display()
                .contains("None supplied; this does not imply exhaustive verification")
        );
    }

    #[test]
    fn typed_editor_and_bounded_parser() {
        let (_fixture, mut pane, _) = setup(true);
        f(&mut pane, 2);
        press(&mut pane, KeyCode::Backspace);
        press(&mut pane, KeyCode::Backspace);
        for c in r#"{"items":[null,true,-42,{"name":"\u0071\n"}]}"#.chars() {
            press(&mut pane, KeyCode::Char(c));
        }
        press(&mut pane, KeyCode::Enter);
        let parsed = parse_arguments(&pane.arguments).unwrap();
        assert!(matches!(parsed["items"], ToolValue::Array(_)));
        for invalid in [
            "[]",
            "{} trailing",
            r#"{"a":1,"a":2}"#,
            r#"{"x":1.2}"#,
            r#"{"x":01}"#,
        ] {
            assert!(parse_arguments(invalid).is_err());
        }
        assert!(
            parse_arguments(&format!("{{\"x\":{}0{}}}", "[".repeat(40), "]".repeat(40))).is_err()
        );
    }
}
