//! Explicit host-selected observations, never serialized native evidence or proposals.
use super::{ToolError, ToolResult, ToolValue};
use crate::{
    agent::{AgentSourceBinding, AgentSourceIdentity},
    source_edit::{SourceId, SourceSnapshot},
};
use scope_language::{SemanticOperationDescription, SemanticOperationResult};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VerificationClassification {
    Verified,
    Rejected,
    Inconclusive,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VerificationSubject {
    SourceSnapshot,
    HypotheticalCandidate,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VerificationCompleteness {
    SelectedFields,
    Unavailable,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerificationMetadata {
    pub operation: String,
    pub source: AgentSourceBinding,
    pub classification: VerificationClassification,
    pub subject: VerificationSubject,
    pub completeness: VerificationCompleteness,
    pub authority: String,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerificationObservation {
    pub metadata: VerificationMetadata,
    pub fields: ToolValue,
}

/// Only public labels and explicitly selected fields belong here. Provider
/// provenance and complete native payloads must not be copied into this selection.
pub struct VerificationSelection {
    pub classification: VerificationClassification,
    pub subject: VerificationSubject,
    pub completeness: VerificationCompleteness,
    pub authority: String,
    pub fields: ToolValue,
}

pub trait SemanticResultProjector {
    fn project(
        &mut self,
        operation: &SemanticOperationDescription,
        source: SourceSnapshot<'_>,
        arguments: &serde_json::Value,
        native: Option<&SemanticOperationResult>,
    ) -> VerificationSelection;
}
impl<F> SemanticResultProjector for F
where
    F: FnMut(
        &SemanticOperationDescription,
        SourceSnapshot<'_>,
        &serde_json::Value,
        Option<&SemanticOperationResult>,
    ) -> VerificationSelection,
{
    fn project(
        &mut self,
        operation: &SemanticOperationDescription,
        source: SourceSnapshot<'_>,
        arguments: &serde_json::Value,
        native: Option<&SemanticOperationResult>,
    ) -> VerificationSelection {
        self(operation, source, arguments, native)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn selection() -> VerificationSelection {
        VerificationSelection {
            classification: VerificationClassification::Verified,
            subject: VerificationSubject::SourceSnapshot,
            completeness: VerificationCompleteness::SelectedFields,
            authority: "public".into(),
            fields: ToolValue::Null,
        }
    }

    #[test]
    fn revision_identity_and_strict_caps() {
        let source = SourceSnapshot {
            id: SourceId::Document(crate::document::DocumentId::new(42)),
            version: u64::MAX,
            language: "fixture",
            text: "private",
        };
        let result =
            observation("language.fixture.check".into(), source, selection(), 4096).unwrap();
        let retained = result.verification.unwrap();
        assert_eq!(
            retained.metadata.source.identity,
            AgentSourceIdentity::Host("42".into())
        );
        assert_eq!(retained.metadata.source.revision, u64::MAX);
        let json = super::super::semantic::to_json(result.value);
        assert_eq!(
            json["observation"]["source"]["revision"],
            u64::MAX.to_string()
        );
        let exact = serde_json::to_vec(&json).unwrap().len();
        assert!(observation("language.fixture.check".into(), source, selection(), exact).is_ok());
        for cap in [0, 1, exact - 1] {
            let error =
                observation("language.fixture.check".into(), source, selection(), cap).unwrap_err();
            assert!(matches!(error, ToolError::Execution { .. }));
            assert!(!error.to_string().contains("private"));
        }
        for operation in ["", "unsafe\noperation"] {
            assert!(observation(operation.into(), source, selection(), 4096).is_err());
        }
        assert!(
            observation(
                "language.fixture.check".into(),
                SourceSnapshot {
                    language: "unsafe\nlanguage",
                    ..source
                },
                selection(),
                4096
            )
            .is_err()
        );
        let mut oversized = selection();
        oversized.fields = ToolValue::String("x".repeat(5000));
        assert!(observation("language.fixture.check".into(), source, oversized, 1).is_err());
    }

    #[test]
    fn oversized_or_malformed_selection_is_unavailable() {
        for authority in ["x".repeat(5000), "unsafe\nlabel".into(), "public".into()] {
            let authority_is_invalid = !safe_label(&authority);
            let result = observation(
                "language.fixture.check".into(),
                SourceSnapshot {
                    id: SourceId::Scratch(1),
                    version: 7,
                    language: "fixture",
                    text: "secret source",
                },
                VerificationSelection {
                    classification: VerificationClassification::Verified,
                    subject: VerificationSubject::HypotheticalCandidate,
                    completeness: VerificationCompleteness::SelectedFields,
                    authority,
                    fields: ToolValue::String("secret".repeat(1000)),
                },
                4096,
            )
            .unwrap();
            let observation = result.verification.unwrap();
            assert_eq!(
                observation.metadata.classification,
                VerificationClassification::Inconclusive
            );
            assert_eq!(
                observation.metadata.completeness,
                VerificationCompleteness::Unavailable
            );
            assert_eq!(observation.metadata.authority, "projection unavailable");
            assert_eq!(observation.fields, ToolValue::Null);
            let json = super::super::semantic::to_json(result.value);
            assert_eq!(
                json["reason"],
                if authority_is_invalid {
                    "projection_invalid_selection"
                } else {
                    "projection_too_large"
                }
            );
            assert!(json.to_string().len() < 4096);
            assert!(!json.to_string().contains("secret"));
        }
    }
}

fn safe_label(label: &str) -> bool {
    !label.is_empty() && label.len() <= 256 && !label.chars().any(char::is_control)
}

pub(super) fn observation(
    operation: String,
    source: SourceSnapshot<'_>,
    selection: VerificationSelection,
    cap: usize,
) -> Result<ToolResult, ToolError> {
    let failure = || ToolError::Execution {
        tool: "language.semantic".into(),
        message: "verification projection unavailable".into(),
    };
    if !safe_label(&operation) || !safe_label(source.language) {
        return Err(failure());
    }
    let identity = match source.id {
        SourceId::Scratch(id) => AgentSourceIdentity::ScratchBuffer(id.to_string()),
        SourceId::Document(id) => AgentSourceIdentity::Host(id.as_u64().to_string()),
    };
    let mut observation = VerificationObservation {
        metadata: VerificationMetadata {
            operation,
            source: AgentSourceBinding {
                identity,
                revision: source.version,
                language: source.language.into(),
            },
            classification: selection.classification,
            subject: selection.subject,
            completeness: selection.completeness,
            authority: selection.authority,
        },
        fields: selection.fields,
    };
    // Model-facing revisions are decimal strings; retained metadata stays u64.
    let metadata_json = |metadata: &VerificationMetadata| {
        let mut value = serde_json::to_value(metadata).expect("metadata is JSON");
        value["source"]["revision"] = metadata.source.revision.to_string().into();
        value
    };
    let envelope = |observation: &VerificationObservation, reason: Option<&str>| {
        let mut value = ToolValue::object([
            ("status", "verification_observation".into()),
            (
                "observation",
                super::semantic::from_json(metadata_json(&observation.metadata))
                    .expect("metadata is representable"),
            ),
            ("fields", observation.fields.clone()),
        ]);
        if let Some(reason) = reason
            && let ToolValue::Object(fields) = &mut value
        {
            fields.insert("reason".into(), reason.into());
        }
        value
    };
    let metadata_size = serde_json::to_vec(&metadata_json(&observation.metadata))
        .expect("metadata is JSON")
        .len();
    let value = envelope(&observation, None);
    let invalid = !safe_label(&observation.metadata.authority)
        || (observation.metadata.completeness == VerificationCompleteness::Unavailable
            && (observation.metadata.classification != VerificationClassification::Inconclusive
                || observation.fields != ToolValue::Null));
    let oversized = metadata_size > 2048
        || serde_json::to_vec(&super::semantic::to_json(value.clone()))
            .expect("tool JSON")
            .len()
            > cap.min(4096);
    let value = if invalid || oversized {
        observation.metadata.classification = VerificationClassification::Inconclusive;
        observation.metadata.completeness = VerificationCompleteness::Unavailable;
        observation.metadata.authority = "projection unavailable".into();

        observation.fields = ToolValue::Null;
        envelope(
            &observation,
            Some(if invalid {
                "projection_invalid_selection"
            } else {
                "projection_too_large"
            }),
        )
    } else {
        value
    };
    if serde_json::to_vec(&metadata_json(&observation.metadata))
        .expect("metadata JSON")
        .len()
        > 2048
        || serde_json::to_vec(&super::semantic::to_json(value.clone()))
            .expect("tool JSON")
            .len()
            > cap.min(4096)
    {
        return Err(failure());
    }
    let mut result = ToolResult::new(value);
    result.verification = Some(observation);
    Ok(result)
}
