//! Narrow downstream projection of a host-installed semantic session.
use crate::{ErrorKind, Host, Value};
use scope_ide::tool::{AgentSession, ToolCall, ToolEffect};
use std::collections::BTreeMap;

/// Exposes host-selected semantic reads. Discovery conveys no authority.
/// The host must install `SemanticToolProvider` with its own target, selection,
/// document scope and source policy. No paths or revision tokens come from scripts.
pub struct SemanticHost<'a> {
    session: AgentSession<'a>,
    bindings: BTreeMap<String, Option<bool>>,
    expected_source: String,
    expected_version: String,
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
            bindings: BTreeMap::from([(action.into(), authorization)]),
            expected_source: expected_source.into(),
            expected_version: expected_version.to_string(),
            calls: 0,
        }
    }

    /// Select full semantic identities with independent invocation authorization.
    /// Duplicate bindings fail; unknown or non-read tools are not discoverable.
    /// Source tokens must belong to the installed providers' common target.
    pub fn with_bindings(
        session: AgentSession<'a>,
        bindings: impl IntoIterator<Item = (String, Option<bool>)>,
        expected_source: impl Into<String>,
        expected_version: u64,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let mut selected = BTreeMap::new();
        for (action, authorization) in bindings {
            if selected.insert(action.clone(), authorization).is_some() {
                return Err(format!("duplicate semantic binding: {action}").into());
            }
        }
        Ok(Self {
            session,
            bindings: selected,
            expected_source: expected_source.into(),
            expected_version: expected_version.to_string(),
            calls: 0,
        })
    }
}
impl Host for SemanticHost<'_> {
    fn operation_names(&self) -> Vec<(String, String)> {
        self.session
            .catalog()
            .tools
            .into_iter()
            .filter(|entry| {
                self.bindings.contains_key(&entry.definition.name)
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
                    self.bindings.contains_key(&entry.definition.name)
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
        if provider != "semantic"
            || !self.bindings.contains_key(action)
            || !action.starts_with("language.")
        {
            return Err(ErrorKind::Runtime("unavailable semantic operation".into()));
        }
        let catalog = self.session.catalog();
        let entry = catalog
            .tools
            .iter()
            .find(|entry| entry.definition.name == action)
            .ok_or_else(|| ErrorKind::Runtime("unavailable semantic operation".into()))?;
        if entry.definition.effect != ToolEffect::Read {
            return Err(ErrorKind::Runtime(
                "unsupported semantic operation effect: only Read is supported".into(),
            ));
        }
        if self.calls >= 64 {
            return Err(ErrorKind::LimitExceeded);
        }
        self.calls += 1;
        let call = ToolCall::new(action)
            .with_argument("expected_source", self.expected_source.clone().into())
            .with_argument("expected_version", self.expected_version.clone().into())
            .with_argument("arguments", arguments);
        let authorized = self.bindings.get(action) == Some(&Some(true));
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
