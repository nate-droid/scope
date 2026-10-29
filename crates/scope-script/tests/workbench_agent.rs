#![cfg(feature = "agent")]
use scope_ide::{
    agent::{DecisionMessage, DecisionModel},
    app::App,
    cli::{ReadOnlyCommand, SemanticAgentBinding, workspace_command::WorkspaceCommandOutput},
    document::Document,
    language::InteractiveSession,
    scratch::ScratchBuffers,
    text::{Position, TextEdit, TextRange},
    workspace::Workspace,
};
use scope_script::{workbench::WorkbenchRunner, workbench_agent::editing_config};
use serde_json::{Value, json};
use std::{collections::VecDeque, io::Write};

struct Model {
    replies: VecDeque<Result<String, String>>,
    conversations: Vec<Vec<DecisionMessage>>,
}
impl Model {
    fn new(decisions: Vec<Value>) -> Self {
        Self {
            replies: decisions
                .into_iter()
                .map(|value| Ok(value.to_string()))
                .collect(),
            conversations: vec![],
        }
    }
}
impl DecisionModel for Model {
    fn decide(&mut self, messages: &[DecisionMessage], _: u32) -> Result<String, String> {
        self.conversations.push(messages.to_vec());
        self.replies.pop_front().expect("unexpected backend call")
    }
}
struct Host {
    runner: WorkbenchRunner,
    model: Model,
}
impl ReadOnlyCommand for Host {
    fn semantic_agent(&mut self) -> Option<SemanticAgentBinding<'_>> {
        let mut binding = self.runner.semantic_agent()?;
        binding.model = Some(&mut self.model);
        Some(binding)
    }
    fn invoke(
        &mut self,
        _: &Document,
        _: TextRange,
        _: &str,
        _: &mut dyn Write,
    ) -> Result<(), Box<dyn std::error::Error>> {
        panic!("no App fallback");
    }
}
fn tool(id: &str, arguments: Value) -> Value {
    json!({"type":"tool", "name":format!("language.scope-script.{id}"), "arguments":arguments})
}
fn host(decisions: Vec<Value>) -> Host {
    Host {
        runner: WorkbenchRunner::default().with_agent_reads(),
        model: Model::new(decisions),
    }
}
fn app(root: &tempfile::TempDir) -> App {
    App::new(Workspace::read_only(root.path().to_str().unwrap()).unwrap()).unwrap()
}
fn submit(
    output: &mut WorkspaceCommandOutput,
    app: &mut App,
    host: &mut Host,
    sources: &mut ScratchBuffers,
    line: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    output.submit(
        line,
        app,
        &mut InteractiveSession::new(),
        Some(host),
        sources,
    )
}

#[test]
fn workspace_ask_uses_selected_unsaved_script_with_only_read_operations() {
    let root = tempfile::tempdir().unwrap();
    let mut app = app(&root);
    let mut sources = ScratchBuffers::default();
    let bound = sources.create("text", "not the target");
    sources.bind_semantic_target(Some(bound)).unwrap();
    let selected = sources.create("scope-script", "print(\"雪😀\"); missing");
    let mut host = host(vec![
        tool("inspect", json!({})),
        json!({"type":"complete", "content":"done"}),
    ]);
    let mut output = WorkspaceCommandOutput::default();
    submit(
        &mut output,
        &mut app,
        &mut host,
        &mut sources,
        ":scope document-ask explain",
    )
    .unwrap();
    assert_eq!(host.model.conversations.len(), 2);
    let context = host.model.conversations[1]
        .iter()
        .map(|message| message.content.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(context.contains("雪😀"));
    assert!(!context.contains("not the target"));
    assert!(!context.contains("language.scope-script.replace"));
    assert_eq!(sources.selected().unwrap().id(), selected);
    assert_eq!(sources.semantic_target().unwrap().unwrap().id(), bound);
    assert_eq!(sources.selected().unwrap().version(), 0);
    assert!(
        host.runner.records.is_empty(),
        "agent cannot execute Script"
    );
    assert!(app.open_document().is_none());
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
}

#[test]
fn workspace_proposal_requires_edit_grant_and_stale_source_cannot_be_applied() {
    let root = tempfile::tempdir().unwrap();
    let mut app = app(&root);
    let mut sources = ScratchBuffers::default();
    let id = sources.create("scope-script", "print(1)\n");
    let mut host = host(vec![tool("replace", json!({"candidate":"print(2)\n"}))]);
    let mut denied = WorkspaceCommandOutput::default();
    assert!(
        submit(
            &mut denied,
            &mut app,
            &mut host,
            &mut sources,
            ":scope document-propose change"
        )
        .unwrap_err()
        .to_string()
        .contains("editing contract required")
    );
    assert!(host.model.conversations.is_empty());
    let mut output = WorkspaceCommandOutput::with_editing(editing_config()).unwrap();
    submit(
        &mut output,
        &mut app,
        &mut host,
        &mut sources,
        ":scope document-propose change",
    )
    .unwrap();
    assert_eq!(sources.selected().unwrap().text(), "print(1)\n");
    assert!(String::from_utf8_lossy(output.bytes()).contains("source proposal 1 ready"));
    sources
        .selected_mut()
        .unwrap()
        .apply_edit(TextEdit::insert(Position::new(0, 0), "# user edit\n"))
        .unwrap();
    submit(
        &mut output,
        &mut app,
        &mut host,
        &mut sources,
        ":scope source-accept 1",
    )
    .unwrap();
    assert_eq!(sources.get(id).unwrap().text(), "# user edit\nprint(1)\n");
    assert!(String::from_utf8_lossy(output.bytes()).contains("error:"));
    assert!(host.runner.records.is_empty());
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
}

#[test]
fn workspace_missing_wrong_and_closed_selected_source_never_call_model() {
    let root = tempfile::tempdir().unwrap();
    let mut app = app(&root);
    let mut sources = ScratchBuffers::default();
    let mut host = host(vec![]);
    let mut output = WorkspaceCommandOutput::default();
    assert!(
        submit(
            &mut output,
            &mut app,
            &mut host,
            &mut sources,
            ":scope document-ask explain"
        )
        .is_err()
    );
    sources.create("text", "wrong language");
    assert!(
        submit(
            &mut output,
            &mut app,
            &mut host,
            &mut sources,
            ":scope document-ask explain"
        )
        .is_err()
    );
    sources.create("scope-script", "print(1)");
    sources.discard_selected().unwrap();
    assert!(
        submit(
            &mut output,
            &mut app,
            &mut host,
            &mut sources,
            ":scope document-ask explain"
        )
        .is_err()
    );
    assert!(host.model.conversations.is_empty());
}

#[test]
fn workspace_backend_failure_does_not_mutate_execute_or_register_proposal() {
    let root = tempfile::tempdir().unwrap();
    let mut app = app(&root);
    let mut sources = ScratchBuffers::default();
    sources.create("scope-script", "print(1)\n");
    let mut host = host(vec![]);
    host.model
        .replies
        .push_back(Err("mock backend unavailable".into()));
    let mut output = WorkspaceCommandOutput::with_editing(editing_config()).unwrap();
    assert!(
        submit(
            &mut output,
            &mut app,
            &mut host,
            &mut sources,
            ":scope document-propose change"
        )
        .is_err()
    );
    assert_eq!(sources.selected().unwrap().text(), "print(1)\n");
    assert_eq!(sources.selected().unwrap().version(), 0);
    assert!(output.source_edit_controller().list().next().is_none());
    assert!(host.runner.records.is_empty());
}
