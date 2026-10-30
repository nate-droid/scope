//! Retained observations only. Publication neither starts nor controls an agent.
//! Hosts clone the store into a provider and publish from lifecycle callbacks.
use super::*;
use crate::agent::AgentTurn;
use serde_json::Value;
use std::sync::{Arc, RwLock};

#[derive(Clone, Default)]
pub struct AgentTurnStore(Arc<RwLock<BTreeMap<u64, Snapshot>>>);

struct Snapshot {
    value: Value,
    terminal: bool,
}

/// A rejected publication leaves all retained observations unchanged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PublishError {
    Serialization(String),
    ContradictorySnapshot { id: u64 },
    Poisoned,
}
impl std::fmt::Display for PublishError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "agent turn publication rejected: {self:?}")
    }
}
impl std::error::Error for PublishError {}

impl AgentTurnStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Accept only append-only observations of a turn. Native tool outputs are
    /// discarded by AgentTurn's serializer before anything enters the store.
    pub fn publish(&self, turn: &AgentTurn) -> Result<(), PublishError> {
        let value = serde_json::to_value(turn)
            .map_err(|error| PublishError::Serialization(error.to_string()))?;
        let mut turns = self.0.write().map_err(|_| PublishError::Poisoned)?;
        if let Some(previous) = turns.get(&turn.id()) {
            if !extends(previous, &value) {
                return Err(PublishError::ContradictorySnapshot { id: turn.id() });
            }
        }
        turns.insert(
            turn.id(),
            Snapshot {
                value,
                terminal: turn.state().is_terminal(),
            },
        );
        Ok(())
    }

    /// Newest retained nonterminal turn by monotonically allocated turn ID.
    /// Returns an owned, sanitized snapshot; absence is JSON null in the tool API.
    pub fn active(&self) -> Result<Option<Value>, PublishError> {
        let turns = self.0.read().map_err(|_| PublishError::Poisoned)?;
        Ok(turns
            .values()
            .rev()
            .find(|s| !s.terminal)
            .map(|s| s.value.clone()))
    }

    pub fn get(&self, id: u64) -> Result<Option<Value>, PublishError> {
        Ok(self
            .0
            .read()
            .map_err(|_| PublishError::Poisoned)?
            .get(&id)
            .map(|s| s.value.clone()))
    }

    /// All retained sanitized turns in ascending ID order, including terminal turns.
    pub fn list(&self) -> Result<Vec<Value>, PublishError> {
        Ok(self
            .0
            .read()
            .map_err(|_| PublishError::Poisoned)?
            .values()
            .map(|s| s.value.clone())
            .collect())
    }
}

fn extends(previous: &Snapshot, next: &Value) -> bool {
    if previous.terminal {
        return previous.value == *next;
    }
    let mut old = previous.value.clone();
    let mut new = next.clone();
    let mut transitions_grew = false;
    for key in ["transitions", "evidence"] {
        let (Some(before), Some(after)) = (old[key].as_array(), new[key].as_array()) else {
            return false;
        };
        if !after.starts_with(before) {
            return false;
        }
        if key == "transitions" {
            transitions_grew = after.len() > before.len();
        }
        old.as_object_mut().unwrap().remove(key);
        new.as_object_mut().unwrap().remove(key);
    }
    // A state change without an appended transition is contradictory.
    if !transitions_grew && old["state"] != new["state"] {
        return false;
    }
    // Usage can advance independently of transitions (for example, accepted
    // conversation bytes). Every counter must retain its identity and never fall.
    let (Some(before), Some(after)) = (old["usage"].as_object(), new["usage"].as_object()) else {
        return false;
    };
    if before.len() != after.len()
        || !before.iter().all(|(key, value)| {
            match (value.as_u64(), after.get(key).and_then(Value::as_u64)) {
                (Some(before), Some(after)) => after >= before,
                _ => false,
            }
        })
    {
        return false;
    }
    old.as_object_mut().unwrap().remove("usage");
    new.as_object_mut().unwrap().remove("usage");
    old.as_object_mut().unwrap().remove("state");
    new.as_object_mut().unwrap().remove("state");
    old == new
}

/// Read-only view of a host-owned store. There is deliberately no publish tool.
pub struct AgentTurnToolProvider {
    store: AgentTurnStore,
    tools: Vec<ToolDefinition>,
}
impl AgentTurnToolProvider {
    pub fn new(store: AgentTurnStore) -> Self {
        Self {
            store,
            tools: vec![
                ToolDefinition::new(
                    "scope.agent.active",
                    "Read the newest retained nonterminal agent turn, or null.",
                ),
                ToolDefinition::new(
                    "scope.agent.list",
                    "Read all retained sanitized agent turns in ascending ID order.",
                ),
                ToolDefinition::new(
                    "scope.agent.get",
                    "Read a retained sanitized agent turn by ID.",
                )
                .input(ToolInputDefinition::required(
                    "id",
                    ToolInputKind::String,
                    "Turn ID as a decimal u64 string.",
                )),
            ],
        }
    }

    /// Nominal identity only, not a schema, registry entry, or capability.
    pub fn turn_type() -> scope_meta::TypeId {
        scope_meta::TypeId::new("scope.agent.AgentTurn", "1")
    }
}
impl ToolProvider for AgentTurnToolProvider {
    fn tools(&self) -> &[ToolDefinition] {
        &self.tools
    }
    fn provider_name(&self) -> Option<&str> {
        Some("scope.agent")
    }
    fn output_description(&self, name: &str) -> Option<&str> {
        match name {
            "scope.agent.active" => {
                Some("Sanitized AgentTurn snapshot or null; no execution authority.")
            }
            "scope.agent.list" => {
                Some("Array of sanitized AgentTurn snapshots; no native tool outputs.")
            }
            "scope.agent.get" => {
                Some("Sanitized AgentTurn snapshot; unknown IDs are invalid arguments.")
            }
            _ => None,
        }
    }
    fn metadata(&self, name: &str) -> Option<ToolValue> {
        self.output_description(name)?;
        Some(ToolValue::object([
            ("type", meta_type_value(&Self::turn_type())),
            ("read_only", true.into()),
            ("sanitized", true.into()),
        ]))
    }
    fn invoke(&mut self, call: ToolCall) -> Result<ToolResult, ToolError> {
        let value = match call.name.as_str() {
            "scope.agent.active" => {
                reject_unexpected_arguments(&call, &[])?;
                self.store.active().map(|v| v.unwrap_or(Value::Null))
            }
            "scope.agent.list" => {
                reject_unexpected_arguments(&call, &[])?;
                self.store.list().map(Value::Array)
            }
            "scope.agent.get" => {
                reject_unexpected_arguments(&call, &["id"])?;
                let invalid = || ToolError::InvalidArgument {
                    tool: call.name.clone(),
                    argument: "id".into(),
                    message: "expected a retained turn ID as a decimal u64 string".into(),
                };
                let text = string_argument(&call, "id")?;
                if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
                    return Err(invalid());
                }
                let id = text.parse::<u64>().map_err(|_| invalid())?;
                let value = self
                    .store
                    .get(id)
                    .map_err(|e| ToolError::execution(&call.name, e.to_string()))?;
                return value
                    .ok_or_else(invalid)
                    .and_then(|v| tool_value(v, &call.name))
                    .map(ToolResult::new);
            }
            _ => return Err(ToolError::unknown_tool(call.name)),
        }
        .map_err(|e| ToolError::execution(&call.name, e.to_string()))?;
        Ok(ToolResult::new(tool_value(value, &call.name)?))
    }
}

fn tool_value(value: Value, tool: &str) -> Result<ToolValue, ToolError> {
    Ok(match value {
        Value::Null => ToolValue::Null,
        Value::Bool(v) => v.into(),
        Value::String(v) => v.into(),
        Value::Number(v) => {
            if let Some(n) = v.as_i64() {
                n.into()
            } else if let Some(n) = v.as_u64() {
                n.to_string().into()
            } else {
                return Err(ToolError::execution(tool, "unsupported snapshot number"));
            }
        }
        Value::Array(v) => ToolValue::Array(
            v.into_iter()
                .map(|v| tool_value(v, tool))
                .collect::<Result<_, _>>()?,
        ),
        Value::Object(v) => ToolValue::Object(
            v.into_iter()
                .map(|(k, v)| Ok((k, tool_value(v, tool)?)))
                .collect::<Result<_, ToolError>>()?,
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{
        AgentCancellationToken, AgentLimits, DecisionMessage, DecisionModel, run_session_observed,
    };
    struct Model;
    impl DecisionModel for Model {
        fn model_identity(&self) -> Option<String> {
            Some("offline-model".into())
        }
        fn trace_settings(&self) -> Option<Value> {
            Some(serde_json::json!({"credential":"private-credential"}))
        }
        fn decide(&mut self, _: &[DecisionMessage], _: u32) -> Result<String, String> {
            Ok(r#"{"type":"complete","content":"private-response"}"#.into())
        }
    }
    #[test]
    fn retained_session_discovery_and_regression() {
        let store = AgentTurnStore::new();
        let mut reader = AgentSession::builder()
            .provider(AgentTurnToolProvider::new(store.clone()))
            .build()
            .unwrap();
        for name in ["scope.agent.active", "scope.agent.list", "scope.agent.get"] {
            let catalog = reader.catalog();
            let entry = catalog.lookup(name).unwrap();
            assert_eq!(entry.definition.effect, ToolEffect::Read);
            assert!(entry.metadata.is_some());
        }
        let mut runner = AgentSession::builder().build().unwrap();
        let mut snapshots = Vec::new();
        let run = run_session_observed(
            &mut Model,
            &mut runner,
            "private-request",
            AgentLimits::default(),
            |_, _| true,
            None,
            &AgentCancellationToken::default(),
            &mut |turn| {
                store.publish(turn).unwrap();
                snapshots.push(turn.clone());
                reader.invoke(ToolCall::new("scope.agent.list")).unwrap();
            },
        )
        .unwrap();
        let expected = serde_json::to_value(&run.turn).unwrap();
        assert_eq!(store.get(run.turn.id()).unwrap(), Some(expected.clone()));
        assert_eq!(store.active().unwrap(), None);
        assert_eq!(
            reader
                .invoke(
                    ToolCall::new("scope.agent.get")
                        .with_argument("id", run.turn.id().to_string().into())
                )
                .unwrap()
                .value,
            tool_value(expected, "test").unwrap()
        );
        assert!(matches!(
            store.publish(&snapshots[0]),
            Err(PublishError::ContradictorySnapshot { .. })
        ));
        store.publish(&run.turn).unwrap();
        for id in ["0", "unknown", "18446744073709551616"] {
            assert!(matches!(
                reader.invoke(ToolCall::new("scope.agent.get").with_argument("id", id.into())),
                Err(ToolError::InvalidArgument { .. })
            ));
        }
        let serialized = serde_json::to_string(&store.list().unwrap()).unwrap();
        for secret in [
            "private-credential",
            "private-request",
            "private-response",
            "output",
        ] {
            assert!(!serialized.contains(secret));
        }
    }
    #[test]
    fn publishes_every_live_callback_and_imports_terminal_snapshot() {
        struct Script(usize);
        impl DecisionModel for Script {
            fn decide(&mut self, _: &[DecisionMessage], _: u32) -> Result<String, String> {
                self.0 += 1;
                Ok(match self.0 {
                    1 => "not a decision",
                    2 => r#"{"type":"tool","name":"read","arguments":{}}"#,
                    _ => r#"{"type":"complete","content":"private-response"}"#,
                }
                .into())
            }
        }
        struct Read(Vec<ToolDefinition>);
        impl ToolProvider for Read {
            fn tools(&self) -> &[ToolDefinition] {
                &self.0
            }
            fn invoke(&mut self, _: ToolCall) -> Result<ToolResult, ToolError> {
                Ok(ToolResult::new("private-native-output".into()))
            }
        }
        let store = AgentTurnStore::new();
        let mut session = AgentSession::builder()
            .provider(Read(vec![ToolDefinition::new("read", "Read fixture")]))
            .build()
            .unwrap();
        let mut snapshots = Vec::new();
        let run = run_session_observed(
            &mut Script(0),
            &mut session,
            "private-request",
            AgentLimits::default(),
            |_, _| true,
            None,
            &AgentCancellationToken::default(),
            &mut |turn| {
                store.publish(turn).unwrap();
                let value = serde_json::to_value(turn).unwrap();
                assert_eq!(store.get(turn.id()).unwrap(), Some(value.clone()));
                snapshots.push(value);
            },
        )
        .unwrap();
        assert!(
            snapshots
                .windows(2)
                .any(|pair| pair[0]["transitions"] == pair[1]["transitions"]
                    && pair[0]["evidence"] != pair[1]["evidence"])
        );
        assert!(
            snapshots
                .windows(2)
                .any(|pair| pair[0]["transitions"] == pair[1]["transitions"]
                    && pair[0]["usage"] != pair[1]["usage"])
        );
        let serialized = serde_json::to_string(&run.turn).unwrap();
        assert!(!serialized.contains("private-native-output"));
        let imported: AgentTurn = serde_json::from_str(&serialized).unwrap();
        assert!(imported.state().is_terminal());
        let imported_store = AgentTurnStore::new();
        imported_store.publish(&imported).unwrap();
        assert_eq!(imported_store.list().unwrap(), store.list().unwrap());
        assert_eq!(imported_store.active().unwrap(), None);
        let mut invalid = serde_json::to_value(&imported).unwrap();
        invalid["usage"]["decisions"] = serde_json::json!(0);
        assert!(serde_json::from_value::<AgentTurn>(invalid).is_err());
    }

    #[test]
    fn rejects_shorter_or_contradictory_histories_and_terminal_changes() {
        let value = serde_json::json!({"id":1,"state":"running","transitions":[1,2],"evidence":[1],"usage":{"decisions":1,"tool_calls":0,"malformed":0,"conversation_bytes":10}});
        let previous = Snapshot {
            value: value.clone(),
            terminal: false,
        };
        for key in ["transitions", "evidence"] {
            let mut shorter = value.clone();
            shorter[key] = serde_json::json!([]);
            assert!(!extends(&previous, &shorter));
        }
        let mut changed = value.clone();
        changed["evidence"] = serde_json::json!([2]);
        assert!(!extends(&previous, &changed));
        let mut advanced = value.clone();
        advanced["usage"]["conversation_bytes"] = serde_json::json!(11);
        assert!(extends(&previous, &advanced));
        for key in ["decisions", "tool_calls", "malformed", "conversation_bytes"] {
            let mut higher = value.clone();
            higher["usage"][key] = serde_json::json!(20);
            assert!(extends(&previous, &higher));
            assert!(!extends(
                &Snapshot {
                    value: higher,
                    terminal: false
                },
                &value
            ));
        }
        for key in ["id", "model_identity", "source_binding", "limits"] {
            let mut changed = value.clone();
            changed[key] = serde_json::json!("changed");
            assert!(!extends(&previous, &changed));
        }
        let terminal = Snapshot {
            value,
            terminal: true,
        };
        assert!(!extends(&terminal, &changed));
    }
}
