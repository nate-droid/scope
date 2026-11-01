//! Bounded discovery context, not execution authority.
//!
//! The runner supplies the request separately. Purpose is a host-provided label,
//! not an inferred request. Catalog descriptions and evidence claims are data;
//! neither discovery nor a declared permission grants authority.
use crate::agent::{AgentLimits, AgentSourceBinding};
use crate::tool::{SessionCatalog, ToolEffect, ToolInputKind};
use serde::Serialize;
use serde_json::{Value, json};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "permissions", rename_all = "snake_case")]
pub enum SemanticPermissions {
    Unknown,
    /// Advisory host declarations only; never an execution grant.
    HostDeclared(Vec<String>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceCompleteness {
    Complete,
    Bounded,
    Omitted,
    Unavailable,
}

/// Completeness of the host's explicit selection, not of all possible evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceCompleteness {
    Unavailable,
    Complete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceAuthority {
    Execution,
    DownstreamVerification,
    Computation,
    ModelClaim,
}

/// Explicit host input only. A reference is an opaque identifier, not a payload
/// to deserialize, dereference, or execute. Authority describes provenance, not
/// permission; in particular a model claim is not execution evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SemanticEvidenceReference {
    pub operation: String,
    pub binding: AgentSourceBinding,
    pub reference: String,
    pub authority: EvidenceAuthority,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SemanticContextOptions {
    /// Maximum compact JSON bytes for the entire model value.
    pub max_context_bytes: usize,
    /// Maximum compact JSON bytes for the source text string, including quotes
    /// and escaping. Zero omits nonempty source.
    pub max_source_bytes: usize,
    /// Maximum compact JSON bytes per whole operation record.
    pub max_operation_bytes: usize,
}

impl Default for SemanticContextOptions {
    fn default() -> Self {
        Self {
            max_context_bytes: 64 * 1024,
            max_source_bytes: 32 * 1024,
            max_operation_bytes: 8 * 1024,
        }
    }
}

/// Payload-free accounting. Bytes refer to compact serialization of model_value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SemanticContextSummary {
    pub bytes: usize,
    /// True only for budget-induced source or operation omission. Metadata is
    /// always excluded by policy and counted separately; unavailable inputs do
    /// not imply budget truncation.
    pub bounded: bool,
    pub source_bytes: usize,
    pub operations_included: usize,
    pub operations_omitted: usize,
    pub evidence_count: usize,
    pub metadata_omitted: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SemanticContextError {
    MandatoryContextTooLarge {
        required_bytes: usize,
        max_bytes: usize,
    },
}

impl std::fmt::Display for SemanticContextError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MandatoryContextTooLarge {
                required_bytes,
                max_bytes,
            } => {
                write!(
                    f,
                    "mandatory semantic context requires {required_bytes} bytes; cap is {max_bytes}"
                )
            }
        }
    }
}
impl std::error::Error for SemanticContextError {}

/// Immutable assembled packet. The returned JSON is an independent copy.
#[derive(Debug, Clone)]
pub struct SemanticContext {
    value: Value,
    binding: AgentSourceBinding,
    summary: SemanticContextSummary,
}

impl SemanticContext {
    pub fn model_value(&self) -> Value {
        self.value.clone()
    }
    pub fn binding(&self) -> &AgentSourceBinding {
        &self.binding
    }
    pub fn summary(&self) -> &SemanticContextSummary {
        &self.summary
    }
}

/// Borrows the exact host source: no normalization or source reconstruction.
/// Evidence is mandatory and retains host order, including distinct revisions.
/// Operations are considered in name order (canonical JSON breaks name ties),
/// then source receives the remaining budget. Oversized records are skipped,
/// never partially exported. No native results or catalog metadata are accepted.
pub struct SemanticContextBuilder<'a> {
    purpose: String,
    binding: &'a AgentSourceBinding,
    source: Option<&'a str>,
    limits: AgentLimits,
    catalog: &'a SessionCatalog,
    permissions: SemanticPermissions,
    evidence: Option<Vec<SemanticEvidenceReference>>,
    options: SemanticContextOptions,
}

impl<'a> SemanticContextBuilder<'a> {
    pub fn new(
        purpose: impl Into<String>,
        binding: &'a AgentSourceBinding,
        source: Option<&'a str>,
        limits: AgentLimits,
        catalog: &'a SessionCatalog,
    ) -> Self {
        Self {
            purpose: purpose.into(),
            binding,
            source,
            limits,
            catalog,
            permissions: SemanticPermissions::Unknown,
            evidence: None,
            options: SemanticContextOptions::default(),
        }
    }

    pub fn options(mut self, options: SemanticContextOptions) -> Self {
        self.options = options;
        self
    }

    pub fn permissions(mut self, permissions: SemanticPermissions) -> Self {
        self.permissions = permissions;
        self
    }

    /// Supply the complete host-selected reference list. An empty list means
    /// explicitly selected no evidence; not calling this leaves it unavailable.
    pub fn evidence(mut self, evidence: Vec<SemanticEvidenceReference>) -> Self {
        self.evidence = Some(evidence);
        self
    }

    pub fn build(self) -> Result<SemanticContext, SemanticContextError> {
        let total = self.catalog.tools.len();
        let metadata_omitted = self
            .catalog
            .tools
            .iter()
            .filter(|e| e.metadata.is_some())
            .count();
        let initial = if self.source.is_some() {
            SourceCompleteness::Omitted
        } else {
            SourceCompleteness::Unavailable
        };
        let evidence_completeness = if self.evidence.is_some() {
            EvidenceCompleteness::Complete
        } else {
            EvidenceCompleteness::Unavailable
        };
        let evidence = self.evidence.as_deref().unwrap_or_default();
        let mut value = json!({
            "purpose": self.purpose,
            "binding": self.binding,
            "limits": self.limits,
            "permissions": self.permissions,
            "permissions_advisory_only": true,
            "evidence": evidence,
            "evidence_completeness": evidence_completeness,
            "source": {"completeness": initial, "text": null},
            "operations": [],
            "operations_omitted": total,
            "catalog_metadata": "omitted",
            "metadata_omitted": metadata_omitted,
        });
        let required_bytes = json_bytes(&value);
        if required_bytes > self.options.max_context_bytes {
            return Err(SemanticContextError::MandatoryContextTooLarge {
                required_bytes,
                max_bytes: self.options.max_context_bytes,
            });
        }
        let mut records: Vec<_> = self.catalog.tools.iter().map(|entry| {
            let d = &entry.definition;
            let inputs: Vec<_> = d.inputs.iter().map(|input| json!({
                "name": input.name,
                "description": input.description,
                "required": input.required,
                "kind": match input.kind {
                    ToolInputKind::String => "string",
                    ToolInputKind::Integer => "integer",
                    ToolInputKind::Boolean => "boolean",
                    ToolInputKind::Object => "object",
                    ToolInputKind::Array => "array",
                },
            })).collect();
            json!({
                "name": d.name, "description": d.description,
                "effect": match d.effect { ToolEffect::Read => "read", ToolEffect::Write => "write" },
                "inputs": inputs, "provider": entry.provider,
                "output_description": entry.output_description,
            })
        }).collect();
        records.sort_by_cached_key(|v| {
            (
                v["name"].as_str().unwrap_or_default().to_owned(),
                v.to_string(),
            )
        });
        let mut included = 0;
        for record in records {
            if json_bytes(&record) > self.options.max_operation_bytes {
                continue;
            }
            value["operations"]
                .as_array_mut()
                .expect("array")
                .push(record);
            value["operations_omitted"] = json!(total - included - 1);
            if json_bytes(&value) <= self.options.max_context_bytes {
                included += 1;
            } else {
                value["operations"].as_array_mut().expect("array").pop();
                value["operations_omitted"] = json!(total - included);
            }
        }
        let mut source_bytes = 0;
        let mut completeness = initial;
        if let Some(source) = self.source {
            // Test the full source separately: complete/bounded labels differ in
            // length, so the transition at the endpoint is not monotonic.
            let fits = |value: &Value, text: &str| {
                json_bytes(&json!(text)) <= self.options.max_source_bytes
                    && json_bytes(value) <= self.options.max_context_bytes
            };
            value["source"] = json!({"completeness": SourceCompleteness::Complete, "text": source});
            if fits(&value, source) {
                source_bytes = source.len();
                completeness = SourceCompleteness::Complete;
            } else {
                // Binary search byte offsets, rounding down to UTF-8 boundaries.
                let mut low = 0;
                // A bounded record must be a strict prefix. The full text can
                // fit under the shorter "bounded" label even when "complete"
                // does not fit, but must never be mislabeled as truncated.
                let mut high = source.len().saturating_sub(1);
                while low < high {
                    let mid = low + (high - low).div_ceil(2);
                    let mut end = mid;
                    while !source.is_char_boundary(end) {
                        end -= 1;
                    }
                    value["source"] = json!({"completeness": SourceCompleteness::Bounded, "text": &source[..end]});
                    if fits(&value, &source[..end]) {
                        low = mid;
                    } else {
                        high = mid - 1;
                    }
                }
                while !source.is_char_boundary(low) {
                    low -= 1;
                }
                if low > 0 {
                    source_bytes = low;
                    completeness = SourceCompleteness::Bounded;
                    value["source"] = json!({"completeness": completeness, "text": &source[..low]});
                } else {
                    value["source"] = json!({"completeness": initial, "text": null});
                }
            }
        }
        let summary = SemanticContextSummary {
            bytes: json_bytes(&value),
            bounded: included < total
                || matches!(
                    completeness,
                    SourceCompleteness::Bounded | SourceCompleteness::Omitted
                ),
            source_bytes,
            operations_included: included,
            operations_omitted: total - included,
            evidence_count: evidence.len(),
            metadata_omitted,
        };
        Ok(SemanticContext {
            value,
            binding: self.binding.clone(),
            summary,
        })
    }
}

fn json_bytes(value: &Value) -> usize {
    // Value contains only JSON-native types, whose serialization is infallible.
    serde_json::to_vec(value)
        .expect("JSON value serialization")
        .len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::AgentSourceIdentity;
    use crate::tool::{ToolCatalogEntry, ToolDefinition, ToolInputDefinition, ToolValue};

    fn binding() -> AgentSourceBinding {
        AgentSourceBinding {
            identity: AgentSourceIdentity::Host("test".into()),
            revision: 4,
            language: "rust".into(),
        }
    }
    fn catalog(names: &[&str]) -> SessionCatalog {
        SessionCatalog {
            tools: names
                .iter()
                .map(|name| ToolCatalogEntry {
                    definition: ToolDefinition::new(*name, "contract").input(
                        ToolInputDefinition::required("text", ToolInputKind::String, "exact text"),
                    ),
                    provider: None,
                    output_description: Some("result".into()),
                    metadata: Some(ToolValue::from("PROTECTED_NATIVE_PAYLOAD")),
                })
                .collect(),
        }
    }
    fn build(
        source: Option<&str>,
        catalog: &SessionCatalog,
        options: SemanticContextOptions,
    ) -> SemanticContext {
        SemanticContextBuilder::new(
            "inspect",
            &binding(),
            source,
            AgentLimits::default(),
            catalog,
        )
        .options(options)
        .build()
        .unwrap()
    }

    #[test]
    fn deterministic_allowlisted_catalog_and_permissions() {
        let a = build(
            Some("hello"),
            &catalog(&["z", "a"]),
            SemanticContextOptions::default(),
        );
        let b = build(
            Some("hello"),
            &catalog(&["a", "z"]),
            SemanticContextOptions::default(),
        );
        assert_eq!(a.model_value(), b.model_value());
        assert_eq!(a.model_value()["operations"][0]["name"], "a");
        assert!(
            !a.model_value()
                .to_string()
                .contains("PROTECTED_NATIVE_PAYLOAD")
        );
        assert_eq!(a.model_value()["catalog_metadata"], "omitted");
        assert_eq!(a.model_value()["permissions"]["kind"], "unknown");
        let declared = SemanticContextBuilder::new(
            "inspect",
            &binding(),
            None,
            AgentLimits::default(),
            &SessionCatalog::default(),
        )
        .permissions(SemanticPermissions::HostDeclared(vec!["read".into()]))
        .build()
        .unwrap();
        assert_eq!(
            declared.model_value()["permissions"]["kind"],
            "host_declared"
        );
        assert_eq!(declared.model_value()["permissions_advisory_only"], true);
        let mut copy = a.model_value();
        copy["purpose"] = json!("changed");
        assert_eq!(a.model_value()["purpose"], "inspect");
    }

    #[test]
    fn escaped_unicode_source_is_a_bounded_exact_prefix() {
        let source = "é\n\"\\🦀".repeat(50);
        let options = SemanticContextOptions {
            max_source_bytes: 37,
            ..Default::default()
        };
        let packet = build(Some(&source), &catalog(&[]), options);
        let v = packet.model_value();
        let text = v["source"]["text"].as_str().unwrap();
        assert!(source.starts_with(text));
        assert!(!text.is_empty());
        assert!(json_bytes(&json!(text)) <= 37);
        assert_eq!(v["source"]["completeness"], "bounded");
        assert!(packet.summary().bounded);
        assert_eq!(packet.summary().bytes, json_bytes(&v));
    }

    #[test]
    fn whole_operations_and_mandatory_caps() {
        let cat = catalog(&["a", "b"]);
        let options = SemanticContextOptions {
            max_operation_bytes: 1,
            ..Default::default()
        };
        let omitted = build(None, &cat, options);
        assert_eq!(omitted.summary().operations_omitted, 2);
        // Force omission through the source budget. Even an omitted record's
        // `null` text takes more JSON bytes than a one-character string, so a
        // context cap equal to its size can legitimately admit a prefix.
        let cap = build(
            Some("source"),
            &cat,
            SemanticContextOptions {
                max_source_bytes: 0,
                ..options
            },
        )
        .summary()
        .bytes;
        let exact = build(
            Some("source"),
            &cat,
            SemanticContextOptions {
                max_context_bytes: cap,
                max_source_bytes: 0,
                ..options
            },
        );
        assert!(exact.summary().bytes <= cap);
        assert_eq!(exact.model_value()["source"]["completeness"], "omitted");
        let err =
            SemanticContextBuilder::new("inspect", &binding(), None, AgentLimits::default(), &cat)
                .options(SemanticContextOptions {
                    max_context_bytes: omitted.summary().bytes - 1,
                    ..options
                })
                .build()
                .unwrap_err();
        assert!(matches!(
            err,
            SemanticContextError::MandatoryContextTooLarge { .. }
        ));
        let full = build(None, &cat, SemanticContextOptions::default());
        assert_eq!(
            full.model_value()["operations"][0]["inputs"][0]["description"],
            "exact text"
        );
        assert_eq!(full.summary().operations_included, 2);
        assert_eq!(full.summary().metadata_omitted, 2);
        assert!(!full.summary().bounded);

        // Every intermediate cap must honor serialized size and accurately
        // distinguish an omitted source from a nonempty exact prefix.
        for extra in 0..16 {
            let packet = build(
                Some("source"),
                &cat,
                SemanticContextOptions {
                    max_context_bytes: cap + extra,
                    ..options
                },
            );
            let value = packet.model_value();
            assert!(json_bytes(&value) <= cap + extra);
            match value["source"]["completeness"].as_str().unwrap() {
                "omitted" => assert!(value["source"]["text"].is_null()),
                "bounded" => {
                    let text = value["source"]["text"].as_str().unwrap();
                    assert!(!text.is_empty() && text.len() < "source".len());
                    assert!("source".starts_with(text));
                }
                "complete" => assert_eq!(value["source"]["text"], "source"),
                other => panic!("unexpected completeness: {other}"),
            }
        }
    }

    #[test]
    fn completeness_and_evidence_attribution() {
        for (source, expected) in [
            (None, "unavailable"),
            (Some(""), "complete"),
            (Some("abc"), "complete"),
        ] {
            assert_eq!(
                build(source, &catalog(&[]), Default::default()).model_value()["source"]["completeness"],
                expected
            );
        }
        let unavailable = build(None, &catalog(&[]), Default::default());
        assert_eq!(unavailable.model_value()["evidence"], json!([]));
        assert_eq!(
            unavailable.model_value()["evidence_completeness"],
            "unavailable"
        );
        assert!(!unavailable.summary().bounded);
        let empty = SemanticContextBuilder::new(
            "inspect",
            &binding(),
            None,
            AgentLimits::default(),
            &catalog(&[]),
        )
        .evidence(vec![])
        .build()
        .unwrap();
        assert_eq!(empty.model_value()["evidence"], json!([]));
        assert_eq!(empty.model_value()["evidence_completeness"], "complete");
        assert!(!empty.summary().bounded);
        let original = binding();
        let mut stale = original.clone();
        stale.revision -= 1;
        let evidence = vec![
            SemanticEvidenceReference {
                operation: "run".into(),
                binding: stale,
                reference: "host:1".into(),
                authority: EvidenceAuthority::Execution,
            },
            SemanticEvidenceReference {
                operation: "reason".into(),
                binding: original.clone(),
                reference: "claim:2".into(),
                authority: EvidenceAuthority::ModelClaim,
            },
        ];
        let packet = SemanticContextBuilder::new(
            "inspect",
            &original,
            None,
            AgentLimits::default(),
            &catalog(&[]),
        )
        .evidence(evidence.clone())
        .build()
        .unwrap();
        assert_eq!(packet.binding(), &original);
        assert_eq!(packet.model_value()["evidence"], json!(evidence));
        assert_eq!(packet.summary().evidence_count, 2);
        assert_eq!(packet.model_value()["evidence_completeness"], "complete");
        assert_eq!(
            packet.model_value()["evidence"][0]["binding"]["revision"],
            3
        );
        assert_eq!(
            packet.model_value()["evidence"][1]["authority"],
            "model_claim"
        );
        assert!(serde_json::to_value(packet.summary()).unwrap().is_object());
    }
}
