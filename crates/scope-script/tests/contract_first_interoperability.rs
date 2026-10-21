//! SI4: private inspection experiment, not a production compatibility policy.
use scope_ide::tool::{ToolEffect, operations::*};
use scope_script::{ErrorKind, Host, Interpreter, Value};
use std::{cell::Cell, rc::Rc};

mod producer {
    use scope_ide::{
        document::session::DocumentSnapshot,
        tool::{ToolArguments, ToolEffect, ToolValue, operations::*},
    };
    use std::{cell::Cell, rc::Rc};

    // Provider-owned fixture promise: the complete output value is uninterpreted
    // text and ordered uninterpreted labels, with no nominal/refinement obligations.
    pub const BOUNDARY: Result<(), &str> = Ok(());
    pub struct Provider(pub Vec<OperationDescriptor>, pub Rc<Cell<usize>>);
    impl Provider {
        pub fn new(calls: Rc<Cell<usize>>) -> Self {
            Self(
                vec![OperationDescriptor {
                    action_id: "publish".into(),
                    description: "Publish an uninterpreted transport record".into(),
                    input: OperationSchema::Object {
                        fields: [].into(),
                        required: [].into(),
                    },
                    output: OperationSchema::Object {
                        fields: [
                            ("text".into(), OperationSchema::String),
                            (
                                "labels".into(),
                                OperationSchema::Array(Box::new(OperationSchema::String)),
                            ),
                        ]
                        .into(),
                        required: ["text".into(), "labels".into()].into(),
                    },
                    effect: ToolEffect::Read,
                    execution: OperationExecution::Immediate,
                    authorization: ["read_source".into()].into(),
                }],
                calls,
            )
        }
    }
    impl OperationProvider for Provider {
        fn provider_id(&self) -> &str {
            "publisher"
        }
        fn descriptors(&self) -> &[OperationDescriptor] {
            &self.0
        }
        fn invoke(
            &self,
            _: &str,
            _: &DocumentSnapshot,
            _: &ToolArguments,
        ) -> Result<ToolValue, OperationFailure> {
            self.1.set(self.1.get() + 1);
            panic!("inspection must not dispatch producer")
        }
    }
}

mod consumer {
    use scope_ide::{
        document::session::DocumentSnapshot,
        tool::{ToolArguments, ToolEffect, ToolValue, operations::*},
    };
    use std::{cell::Cell, rc::Rc};

    pub struct Provider(pub Vec<OperationDescriptor>, pub Rc<Cell<usize>>);
    impl Provider {
        pub fn new(calls: Rc<Cell<usize>>) -> Self {
            let descriptors = ["accept", "integer_text", "opaque"]
                .into_iter()
                .map(|action| OperationDescriptor {
                    action_id: action.into(),
                    description: "Consume a transport record".into(),
                    input: OperationSchema::Object {
                        fields: [
                            (
                                "text".into(),
                                if action == "integer_text" {
                                    OperationSchema::Integer
                                } else {
                                    OperationSchema::String
                                },
                            ),
                            (
                                "labels".into(),
                                OperationSchema::Array(Box::new(OperationSchema::String)),
                            ),
                        ]
                        .into(),
                        required: ["text".into(), "labels".into()].into(),
                    },
                    output: OperationSchema::Null,
                    effect: ToolEffect::Write,
                    execution: OperationExecution::Blocking,
                    authorization: ["consume_record".into()].into(),
                })
                .collect();
            Self(descriptors, calls)
        }
        // Complete input promises for accept/integer_text: uninterpreted text
        // (or i64) and ordered uninterpreted labels; no additional obligations.
        // These are provider-owned test promises, not facts inferred from prose,
        // action names, matching shapes, examples, or registration.
        pub fn boundary(action: &str) -> Result<(), &'static str> {
            match action {
                "accept" | "integer_text" => Ok(()),
                _ => Err(
                    "consumer nominal/semantic obligations and complete input boundary unconfirmed",
                ),
            }
        }
    }
    impl OperationProvider for Provider {
        fn provider_id(&self) -> &str {
            "receiver"
        }
        fn descriptors(&self) -> &[OperationDescriptor] {
            &self.0
        }
        fn invoke(
            &self,
            _: &str,
            _: &DocumentSnapshot,
            _: &ToolArguments,
        ) -> Result<ToolValue, OperationFailure> {
            self.1.set(self.1.get() + 1);
            panic!("inspection must not dispatch consumer")
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Outcome {
    Exact,
    UnsupportedExact,
    Unknown(&'static str),
}

struct Inspection<'a> {
    producer: (OperationId, &'a OperationDescriptor),
    consumer: (OperationId, &'a OperationDescriptor),
    outcome: Outcome,
}

fn inspect<'a>(
    producer: (OperationId, &'a OperationDescriptor),
    consumer: (OperationId, &'a OperationDescriptor),
    output_boundary: Result<(), &'static str>,
    input_boundary: Result<(), &'static str>,
) -> Inspection<'a> {
    let outcome = match output_boundary.and(input_boundary) {
        Err(reason) => Outcome::Unknown(reason),
        Ok(()) if producer.1.output == consumer.1.input => Outcome::Exact,
        Ok(()) => Outcome::UnsupportedExact,
    };
    Inspection {
        producer,
        consumer,
        outcome,
    }
}

// A metadata-only test host exposes a derived report through existing discovery.
// It never routes scope.invoke to either provider and contains no schema codec.
struct InspectionHost<'a>(Inspection<'a>);
impl Host for InspectionHost<'_> {
    fn operations(&mut self) -> Result<Value, ErrorKind> {
        let endpoint = |(id, descriptor): &(OperationId, &OperationDescriptor), role: &str| {
            Value::object([
                ("provider", id.provider_id.clone().into()),
                ("action", id.action_id.clone().into()),
                (
                    "schema_provenance",
                    format!("OperationRegistry::descriptors/{role}").into(),
                ),
                ("effect", format!("{:?}", descriptor.effect).into()),
                ("execution", format!("{:?}", descriptor.execution).into()),
                (
                    "authorization",
                    Value::Array(
                        descriptor
                            .authorization
                            .iter()
                            .cloned()
                            .map(Value::String)
                            .collect(),
                    ),
                ),
            ])
        };
        Ok(Value::object([
            ("producer", endpoint(&self.0.producer, "output")),
            ("consumer", endpoint(&self.0.consumer, "input")),
            ("outcome", format!("{:?}", self.0.outcome).into()),
            ("authorized", Value::Boolean(false)),
            ("contract_scope", "complete producer output value to complete consumer argument object; fixture-owned boundary attestations".into()),
            (
                "document_revision",
                "not inspected; required separately at invocation".into(),
            ),
        ]))
    }
    fn invoke(&mut self, _: &str, _: &str, _: Value) -> Result<Value, ErrorKind> {
        Err(OperationError::Unauthorized.into())
    }
}

#[test]
fn registered_contracts_are_inspectable_by_script_without_execution_or_authority() {
    let producer_calls = Rc::new(Cell::new(0));
    let consumer_calls = Rc::new(Cell::new(0));
    let mut registry = OperationRegistry::new(OperationLimits::default());
    registry
        .register(Box::new(producer::Provider::new(producer_calls.clone())))
        .unwrap();
    registry
        .register(Box::new(consumer::Provider::new(consumer_calls.clone())))
        .unwrap();
    assert_eq!(registry.descriptors().count(), 4);

    for (action, expected) in [
        ("accept", Outcome::Exact),
        ("integer_text", Outcome::UnsupportedExact),
        (
            "opaque",
            Outcome::Unknown(
                "consumer nominal/semantic obligations and complete input boundary unconfirmed",
            ),
        ),
    ] {
        let output = registry
            .descriptors()
            .find(|(id, _)| id.provider_id == "publisher" && id.action_id == "publish")
            .unwrap();
        let input = registry
            .descriptors()
            .find(|(id, _)| id.provider_id == "receiver" && id.action_id == action)
            .unwrap();
        let inspection = inspect(
            output,
            input,
            producer::BOUNDARY,
            consumer::Provider::boundary(action),
        );
        assert_eq!(inspection.outcome, expected);
        assert_eq!(inspection.producer.1.effect, ToolEffect::Read);
        assert_eq!(inspection.consumer.1.effect, ToolEffect::Write);
        assert_eq!(
            inspection.consumer.1.execution,
            OperationExecution::Blocking
        );
        assert!(
            inspection
                .consumer
                .1
                .authorization
                .contains("consume_record")
        );
        let mut host = InspectionHost(inspection);
        let report = Interpreter::new()
            .eval("scope.operations()", &mut host)
            .unwrap();
        assert_eq!(report, host.operations().unwrap());
        let mut interpreter = Interpreter::new();
        interpreter.eval(
                    "let contract = scope.operations(); if contract.authorized == false { print(contract) } else { scope.invoke(\"receiver\", \"accept\", {}) }",
                    &mut host,
                ).unwrap();
        assert_eq!(interpreter.take_output(), [report.clone()]);
        assert_eq!(interpreter.eval("contract", &mut host).unwrap(), report);
        let Value::Object(report) = report else {
            panic!("report must be an object")
        };
        assert_eq!(report["outcome"], Value::String(format!("{expected:?}")));
        assert_eq!(report["authorized"], Value::Boolean(false));
        for (provider, action) in [("publisher", "publish"), ("receiver", action)] {
            let error = Interpreter::new()
                .eval(
                    &format!("scope.invoke(\"{provider}\", \"{action}\", {{}})"),
                    &mut host,
                )
                .unwrap_err();
            assert!(matches!(
                error.kind,
                ErrorKind::Operation(OperationError::Unauthorized)
            ));
        }
        assert_eq!(producer_calls.get(), 0);
        assert_eq!(consumer_calls.get(), 0);
    }
}

#[test]
fn missing_output_boundary_is_unknown_even_for_equal_registered_schemas() {
    let calls = Rc::new(Cell::new(0));
    let mut registry = OperationRegistry::new(OperationLimits::default());
    registry
        .register(Box::new(producer::Provider::new(calls.clone())))
        .unwrap();
    registry
        .register(Box::new(consumer::Provider::new(calls.clone())))
        .unwrap();
    let output = registry
        .descriptors()
        .find(|(id, _)| id.provider_id == "publisher")
        .unwrap();
    let input = registry
        .descriptors()
        .find(|(id, _)| id.action_id == "accept")
        .unwrap();
    assert_eq!(output.1.output, input.1.input);
    assert_eq!(
        inspect(
            output,
            input,
            Err("output value versus result envelope boundary unknown"),
            Ok(())
        )
        .outcome,
        Outcome::Unknown("output value versus result envelope boundary unknown")
    );
    assert_eq!(calls.get(), 0);
}
