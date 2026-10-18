//! Bridge from validated operation output to reviewable document replacements.
//!
//! Proposal generation grants no apply or save authority. Native providers are
//! trusted in-process code: registry policy and this bridge do not sandbox their
//! ambient authority, allocations, runtime, or external side effects.

use super::ToolValue;
use super::operations::{
    OperationAuthorization, OperationCall, OperationError, OperationRegistry, OperationResult,
};
use crate::document::{DocumentSession, EditError, EditProposal, Revision};

/// Typed failures while generating a replacement proposal.
#[derive(Debug)]
pub enum OperationEditError {
    /// Registry validation, authorization, or provider execution failed.
    Operation(OperationError),
    /// The document could not accept a proposal at the expected revision.
    Edit(EditError),
    /// A successful, schema-valid operation returned something other than text.
    ExpectedReplacement,
}

impl std::fmt::Display for OperationEditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Operation(error) => error.fmt(f),
            Self::Edit(error) => error.fmt(f),
            Self::ExpectedReplacement => f.write_str("expected a string full replacement"),
        }
    }
}

impl std::error::Error for OperationEditError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Operation(error) => Some(error),
            Self::Edit(error) => Some(error),
            Self::ExpectedReplacement => None,
        }
    }
}

/// Immutable proposal with structured provenance: the original call and its result.
/// The proposal's display provenance is debug-formatted identity, not a wire format;
/// use `call` and `result` instead of parsing that string.
#[derive(Debug, Clone)]
pub struct OperationEdit {
    call: OperationCall,
    result: OperationResult,
    proposal: EditProposal,
}

impl OperationEdit {
    /// Original operation identity and arguments.
    pub fn call(&self) -> &OperationCall {
        &self.call
    }

    /// Validated output and the source document identity and revision.
    pub fn result(&self) -> &OperationResult {
        &self.result
    }

    /// Full replacement for the document's explicit review/authorization lifecycle.
    pub fn proposal(&self) -> &EditProposal {
        &self.proposal
    }
}

impl OperationRegistry {
    /// Invoke with explicit call-aware policy, then propose the returned string as
    /// the entire replacement text. No parsing, review, authorization to apply, or
    /// disk write is performed by this bridge.
    ///
    /// `authorize` permits provider invocation only, not acceptance of its edit.
    /// Native providers retain their own authority and must be trusted; this API
    /// cannot prevent a provider from writing files or causing other side effects.
    pub fn propose_edit(
        &self,
        session: &mut DocumentSession,
        expected: &Revision,
        call: &OperationCall,
        authorize: impl FnOnce(OperationAuthorization<'_>) -> bool,
    ) -> Result<OperationEdit, OperationEditError> {
        let result = self
            .invoke(session, expected, call, authorize)
            .map_err(OperationEditError::Operation)?;
        let ToolValue::String(replacement) = &result.value else {
            return Err(OperationEditError::ExpectedReplacement);
        };
        let proposal = session
            .propose(
                expected,
                replacement.clone(),
                format!("{:?}", call.operation),
            )
            .map_err(OperationEditError::Edit)?;
        Ok(OperationEdit {
            call: call.clone(),
            result,
            proposal,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{DocumentId, EditDecision};
    use crate::test_support::TempWorkspace;
    use crate::tool::operations::{
        OperationDescriptor, OperationExecution, OperationFailure, OperationId, OperationLimits,
        OperationProvider, OperationSchema, OperationValidation,
    };
    use crate::tool::{ToolArguments, ToolEffect};
    use scope_language::{Diagnostic, Position, TextRange};
    use std::{cell::Cell, collections::BTreeMap, rc::Rc};

    struct Provider {
        descriptors: Vec<OperationDescriptor>,
        output: ToolValue,
        calls: Rc<Cell<usize>>,
    }

    impl OperationProvider for Provider {
        fn provider_id(&self) -> &str {
            "native"
        }
        fn descriptors(&self) -> &[OperationDescriptor] {
            &self.descriptors
        }
        fn invoke(
            &self,
            _: &str,
            snapshot: &crate::document::DocumentSnapshot,
            arguments: &ToolArguments,
        ) -> Result<ToolValue, OperationFailure> {
            assert_eq!(snapshot.text, "before\n");
            assert_eq!(arguments["target"], ToolValue::from("example.txt"));
            self.calls.set(self.calls.get() + 1);
            Ok(self.output.clone())
        }
    }

    fn registry(
        output: ToolValue,
        schema: OperationSchema,
    ) -> (OperationRegistry, Rc<Cell<usize>>) {
        let calls = Rc::new(Cell::new(0));
        let mut registry = OperationRegistry::new(OperationLimits::default());
        registry
            .register(Box::new(Provider {
                descriptors: vec![OperationDescriptor {
                    action_id: "replace".into(),
                    description: "Generate replacement text".into(),
                    input: OperationSchema::Object {
                        fields: BTreeMap::from([("target".into(), OperationSchema::String)]),
                        required: ["target".into()].into(),
                    },
                    output: schema,
                    effect: ToolEffect::Read,
                    execution: OperationExecution::Immediate,
                    authorization: ["generate".into()].into(),
                }],
                output,
                calls: calls.clone(),
            }))
            .unwrap();
        (registry, calls)
    }

    fn call() -> OperationCall {
        OperationCall {
            operation: OperationId {
                provider_id: "native".into(),
                action_id: "replace".into(),
            },
            arguments: BTreeMap::from([("target".into(), "example.txt".into())]),
        }
    }

    fn session(fixture: &TempWorkspace) -> DocumentSession {
        fixture.write_file("example.txt", "before\n");
        DocumentSession::open(fixture.workspace(), DocumentId::new(1), "example.txt").unwrap()
    }

    #[test]
    fn replacement_provenance_review_and_explicit_apply() {
        let fixture = TempWorkspace::new("operation-edit-lifecycle");
        let mut session = session(&fixture);
        let base = session.snapshot().unwrap();
        let (registry, calls) = registry("after 🦀\n".into(), OperationSchema::String);
        let call = call();
        let edit = registry
            .propose_edit(&mut session, &base.revision, &call, |context| {
                context.call == &call
                    && context.snapshot == &base
                    && context.descriptor.effect == ToolEffect::Read
                    && context.descriptor.authorization.contains("generate")
            })
            .unwrap();
        assert_eq!(calls.get(), 1);
        assert_eq!(edit.call(), &call);
        assert_eq!(edit.result().document_id, base.document_id);
        assert_eq!(edit.result().revision, base.revision);
        assert_eq!(edit.result().value, ToolValue::from("after 🦀\n"));
        assert_eq!(edit.proposal().base(), &base);
        assert_eq!(edit.proposal().candidate().text, "after 🦀\n");
        assert_eq!(
            edit.proposal().provenance(),
            format!("{:?}", call.operation)
        );
        assert_eq!(session.snapshot().unwrap(), base);
        assert_eq!(fixture.read_file("example.txt"), base.text);
        let diagnostic = Diagnostic::warning(
            TextRange::new(Position::new(0, 0), Position::new(0, 1)),
            "candidate warning",
        );
        let review = session
            .review(edit.proposal(), vec![diagnostic.clone()])
            .unwrap();
        assert_eq!(review.diagnostics(), &[diagnostic]);
        assert_eq!(review.proposal().candidate(), edit.proposal().candidate());
        assert!(review.diff().contains("-before\n+after 🦀\n"));
        let denied = session.authorize(&review, EditDecision::Deny).unwrap();
        assert!(matches!(
            session.apply(&review, &denied),
            Err(EditError::Denied)
        ));
        assert_eq!(session.snapshot().unwrap(), base);
        assert_eq!(fixture.read_file("example.txt"), base.text);
        let allowed = session.authorize(&review, EditDecision::Allow).unwrap();
        assert_eq!(fixture.read_file("example.txt"), base.text);
        let applied = session.apply(&review, &allowed).unwrap();
        assert_eq!(&applied, edit.proposal().candidate());
        assert_eq!(fixture.read_file("example.txt"), applied.text);
    }

    #[test]
    fn call_aware_denial_and_stale_revision_never_dispatch() {
        let fixture = TempWorkspace::new("operation-edit-denial");
        let mut session = session(&fixture);
        let base = session.snapshot().unwrap();
        let (registry, calls) = registry("after".into(), OperationSchema::String);
        assert!(matches!(
            registry.propose_edit(&mut session, &base.revision, &call(), |context| {
                assert_eq!(
                    context.call.arguments["target"],
                    ToolValue::from("example.txt")
                );
                false
            }),
            Err(OperationEditError::Operation(OperationError::Unauthorized))
        ));
        assert_eq!(session.snapshot().unwrap(), base);
        assert_eq!(fixture.read_file("example.txt"), base.text);
        fixture.write_file("example.txt", "external change\n");
        session.reload().unwrap();
        let current = session.snapshot().unwrap();
        assert!(matches!(
            registry.propose_edit(&mut session, &base.revision, &call(), |_| {
                panic!("stale call must not reach policy")
            }),
            Err(OperationEditError::Operation(OperationError::StaleRevision))
        ));
        assert_eq!(calls.get(), 0);
        assert_eq!(session.snapshot().unwrap(), current);
        assert_eq!(fixture.read_file("example.txt"), current.text);
    }

    #[test]
    fn nonstring_and_invalid_output_leave_source_unchanged() {
        let fixture = TempWorkspace::new("operation-edit-output");
        let mut session = session(&fixture);
        let base = session.snapshot().unwrap();
        let (valid, calls) = registry(ToolValue::Boolean(true), OperationSchema::Boolean);
        assert!(matches!(
            valid.propose_edit(&mut session, &base.revision, &call(), |_| true),
            Err(OperationEditError::ExpectedReplacement)
        ));
        assert_eq!(calls.get(), 1);
        let (invalid, calls) = registry(ToolValue::Boolean(true), OperationSchema::String);
        assert!(matches!(
            invalid.propose_edit(&mut session, &base.revision, &call(), |_| true),
            Err(OperationEditError::Operation(
                OperationError::InvalidOutput(OperationValidation::TypeMismatch)
            ))
        ));
        assert_eq!(calls.get(), 1);
        assert_eq!(session.snapshot().unwrap(), base);
        assert_eq!(fixture.read_file("example.txt"), base.text);
    }
}
