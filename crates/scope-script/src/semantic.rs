//! Narrow downstream projection of a host-installed semantic session.
use crate::{ErrorKind, Host, Value};
use scope_ide::tool::{AgentSession, ToolCall, ToolEffect};

/// Exposes exactly one host-selected semantic read. Discovery conveys no authority.
/// The host must install `SemanticToolProvider` with its own target, selection,
/// document scope and source policy. No paths or revision tokens come from scripts.
pub struct SemanticHost<'a> {
    session: AgentSession<'a>,
    action: String,
    expected_source: String,
    expected_version: String,
    authorized: bool,
    calls: usize,
}
impl<'a> SemanticHost<'a> {
    /// Tokens must be captured from the same target used by the semantic provider.
    /// A missing authorization (`None`) denies calls, even after discovery.
    pub fn new(
        session: AgentSession<'a>,
        action: impl Into<String>,
        expected_source: impl Into<String>,
        expected_version: u64,
        authorization: Option<bool>,
    ) -> Self {
        Self {
            session,
            action: action.into(),
            expected_source: expected_source.into(),
            expected_version: expected_version.to_string(),
            authorized: authorization == Some(true),
            calls: 0,
        }
    }
}
impl Host for SemanticHost<'_> {
    fn operation_names(&self) -> Vec<(String, String)> {
        self.session
            .catalog()
            .tools
            .into_iter()
            .filter(|entry| {
                entry.definition.name == self.action
                    && entry.definition.effect == ToolEffect::Read
                    && entry.definition.name.starts_with("language.")
            })
            .map(|entry| ("semantic".into(), entry.definition.name))
            .collect()
    }
    fn operations(&mut self) -> Result<Value, ErrorKind> {
        Ok(Value::Array(
            self.session
                .catalog()
                .tools
                .into_iter()
                .filter(|entry| {
                    entry.definition.name == self.action
                        && entry.definition.effect == ToolEffect::Read
                        && entry.definition.name.starts_with("language.")
                })
                .map(|entry| {
                    Value::object([
                        ("provider", "semantic".into()),
                        ("action", entry.definition.name.into()),
                        ("description", entry.definition.description.into()),
                        ("metadata", entry.metadata.unwrap_or(Value::Null)),
                    ])
                })
                .collect(),
        ))
    }
    fn invoke(
        &mut self,
        provider: &str,
        action: &str,
        arguments: Value,
    ) -> Result<Value, ErrorKind> {
        let Value::Array(available) = self.operations()? else {
            unreachable!()
        };
        if provider != "semantic" || action != self.action || available.is_empty() {
            return Err(ErrorKind::Runtime("unavailable semantic operation".into()));
        }
        if self.calls >= 64 {
            return Err(ErrorKind::LimitExceeded);
        }
        self.calls += 1;
        let call = ToolCall::new(action)
            .with_argument("expected_source", self.expected_source.clone().into())
            .with_argument("expected_version", self.expected_version.clone().into())
            .with_argument("arguments", arguments);
        let authorized = self.authorized;
        let result = self
            .session
            .invoke_with_call_policy(call, |definition, _| {
                authorized && definition.effect == ToolEffect::Read
            })
            .map_err(ErrorKind::Tool)?;
        crate::value_size(&result.value, 0)?;
        Ok(result.value)
    }
}
