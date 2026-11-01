//! Explicit development-only native transport; no lifecycle or dispatch policy lives here.
use super::{DecisionMessage, DecisionModel};
pub use cell_capability_lmstudio::ReasoningMode;
use cell_capability_lmstudio::{Message, NativeChat, NativeResponse};
use serde_json::{Value, json};

/// Opt-in native adapter. The default `CellAgent` remains OpenAI-compatible.
///
/// Native input is a stateless transcript, not a provider-managed conversation.
/// Request acceptance is not independent verification of the effective reasoning mode.
pub struct LmStudioAgent {
    chat: NativeChat,
    invocations: u64,
    last_response: Option<NativeResponse>,
    last_error: Option<String>,
}

impl LmStudioAgent {
    pub fn new(model: &str, origin: &str, mode: ReasoningMode) -> Result<Self, String> {
        Ok(Self {
            chat: NativeChat::new(model, origin, mode).map_err(|e| e.to_string())?,
            invocations: 0,
            last_response: None,
            last_error: None,
        })
    }
}

impl DecisionModel for LmStudioAgent {
    fn provider_identity(&self) -> Option<String> {
        Some(format!("cell:lmstudio/chat@0.1.0:{}", self.chat.base_url()))
    }
    fn model_identity(&self) -> Option<String> {
        Some(self.chat.model().to_owned())
    }
    fn trace_settings(&self) -> Option<Value> {
        Some(json!({
            "backend":"lmstudio-native", "requested_reasoning":self.chat.mode(),
            "capability_support":"native request contract; model support requires backend acceptance",
            "backend_accepted":self.last_response.is_some(),
            "effective_mode":null,
            "observed_reasoning_items":self.last_response.as_ref().map(|r| r.reasoning.len()),
            "temperature":0.2,"history":"stateless JSON transcript with separate system_prompt",
            "invocations":self.invocations,"last_response":self.last_response,
            "last_error":self.last_error,
        }))
    }
    fn decide(&mut self, messages: &[DecisionMessage], max_tokens: u32) -> Result<String, String> {
        self.invocations = self.invocations.saturating_add(1);
        self.last_response = None;
        self.last_error = None;
        let messages: Vec<_> = messages
            .iter()
            .map(|m| Message {
                role: m.role.clone(),
                content: m.content.clone(),
            })
            .collect();
        match self.chat.complete(&messages, max_tokens) {
            Ok(response) => {
                let visible = response.visible.clone();
                self.last_response = Some(response);
                // Preserve empty/partial/prose verbatim for the existing strict parser.
                Ok(visible)
            }
            Err(error) => {
                self.last_error = Some(error.clone());
                Err(error)
            }
        }
    }
}

#[cfg(test)]
#[path = "agent_lmstudio_tests.rs"]
mod native_tests;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn configuration_does_not_claim_effective_mode_or_acceptance() {
        let model = LmStudioAgent::new("model", "http://127.0.0.1:1", ReasoningMode::On).unwrap();
        let settings = model.trace_settings().unwrap();
        assert_eq!(settings["requested_reasoning"], "on");
        assert_eq!(settings["backend_accepted"], false);
        assert!(settings["effective_mode"].is_null());
        assert_eq!(settings["invocations"], 0);
    }
    #[test]
    fn failed_invocation_is_counted_without_stale_metadata() {
        let mut model =
            LmStudioAgent::new("model", "http://127.0.0.1:1", ReasoningMode::Off).unwrap();
        assert!(model.decide(&[], 0).is_err());
        let settings = model.trace_settings().unwrap();
        assert_eq!(settings["invocations"], 1);
        assert_eq!(settings["backend_accepted"], false);
        assert!(settings["last_response"].is_null());
        assert!(settings["last_error"].is_string());
    }
}
