mod support;

use cell::{CancellationToken, Cell, ExecutionErrorKind, ExecutionPolicy};
use scope_execution::{ExecutionError, ExecutionPhase, ExecutionResult, execute_with_policy};
use scope_types::{Capabilities, Diagnostic, Endpoint};
use std::time::{Duration, Instant};
use support::*;

fn policy() -> ExecutionPolicy {
    ExecutionPolicy {
        fuel: Some(1_000_000),
        ..ExecutionPolicy::default()
    }
}

fn assert_empty(result: &ExecutionResult) {
    assert!(result.completed.is_empty(), "{result:?}");
    assert!(result.artifacts.is_empty(), "{result:?}");
    assert!(result.output.is_none(), "{result:?}");
}

fn assert_managed(
    result: &ExecutionResult,
    expected_node: &str,
    expected_phase: ExecutionPhase,
    expected_kind: ExecutionErrorKind,
) {
    assert_eq!(result.errors.len(), 1, "{result:?}");
    match &result.errors[0] {
        ExecutionError::Managed {
            node,
            phase,
            kind,
            message,
        } => {
            assert_eq!(node, expected_node);
            assert_eq!(phase, &expected_phase);
            assert_eq!(*kind, expected_kind);
            assert!(!message.is_empty());
        }
        other => panic!("expected managed error, got {other:?}"),
    }
    assert!(result.output.is_none());
}

#[test]
fn normal_fuel_executes_real_guests_and_publishes_typed_artifacts() {
    let result = execute_with_policy(
        &Cell::new().unwrap(),
        &pipeline(),
        &bindings(),
        inputs("hello wasm world"),
        &Capabilities::new(),
        &policy(),
    );
    assert!(result.errors.is_empty(), "{result:?}");
    assert_eq!(result.completed, ["z-uppercase", "a-count"]);
    assert_eq!(result.artifacts.len(), 2);
    let intermediate = &result.artifacts[&Endpoint::new("z-uppercase", "out")];
    assert_eq!(intermediate.ty, ty("Text"));
    assert_eq!(intermediate.bytes, b"HELLO WASM WORLD");
    let output = result.output.as_ref().unwrap();
    assert_eq!(output.ty, ty("Number"));
    assert_eq!(output.bytes, 3_u64.to_le_bytes());
    assert_eq!(output, &result.artifacts[&Endpoint::new("a-count", "out")]);
}

#[test]
fn fuel_bounds_real_loops_during_instantiation_and_invocation() {
    let cell = Cell::new().unwrap();
    for (guest, phase) in [
        (Guest::StartLoop, ExecutionPhase::Instantiate),
        (Guest::Loop, ExecutionPhase::Invoke),
    ] {
        let mut bound = bindings();
        bound.insert(ty("Uppercase"), binding(guest));
        let result = execute_with_policy(
            &cell,
            &pipeline(),
            &bound,
            inputs("hello"),
            &Capabilities::new(),
            &policy(),
        );
        assert_managed(&result, "z-uppercase", phase, ExecutionErrorKind::Fuel);
        assert_empty(&result);
    }
}

#[test]
fn payload_limits_apply_to_the_recursive_port_envelope() {
    let cell = Cell::new().unwrap();
    // "hello": list + tuple + "in" + "Text" + "1" + byte-list + five u8s = 18.
    // The uppercase output uses "out", so its recursive cost is 19.
    for (input_bytes, output_bytes, kind) in [
        (Some(17), None, ExecutionErrorKind::Input),
        (Some(18), Some(18), ExecutionErrorKind::Output),
    ] {
        let limits = ExecutionPolicy {
            input_bytes,
            output_bytes,
            ..policy()
        };
        let result = execute_with_policy(
            &cell,
            &pipeline(),
            &bindings(),
            inputs("hello"),
            &Capabilities::new(),
            &limits,
        );
        assert_managed(&result, "z-uppercase", ExecutionPhase::Invoke, kind);
        assert_empty(&result);
    }
    let limits = ExecutionPolicy {
        input_bytes: Some(19),
        output_bytes: Some(26),
        ..policy()
    };
    let result = execute_with_policy(
        &cell,
        &pipeline(),
        &bindings(),
        inputs("hello"),
        &Capabilities::new(),
        &limits,
    );
    assert!(result.errors.is_empty(), "{result:?}");
    assert_eq!(result.completed, ["z-uppercase", "a-count"]);
    assert_eq!(result.output.unwrap().bytes, 1_u64.to_le_bytes());
}

#[test]
fn memory_ceiling_applies_at_instantiation_and_guest_growth() {
    let cell = Cell::new().unwrap();
    for (guest, memory_bytes, phase) in [
        (Guest::Uppercase, 65_536, ExecutionPhase::Instantiate),
        (Guest::GrowMemory, 2 * 65_536, ExecutionPhase::Invoke),
    ] {
        let mut bound = bindings();
        bound.insert(ty("Uppercase"), binding(guest));
        let limits = ExecutionPolicy {
            memory_bytes: Some(memory_bytes),
            ..policy()
        };
        let result = execute_with_policy(
            &cell,
            &pipeline(),
            &bound,
            inputs("hello"),
            &Capabilities::new(),
            &limits,
        );
        assert_managed(&result, "z-uppercase", phase, ExecutionErrorKind::Memory);
        assert_empty(&result);
    }
}

#[test]
fn expired_deadline_and_sticky_cancellation_prevent_guest_execution() {
    let cell = Cell::new().unwrap();
    let token = CancellationToken::default();
    token.cancel();
    assert!(token.is_cancelled());
    for (limits, kind) in [
        (
            ExecutionPolicy {
                deadline: Some(Instant::now() - Duration::from_secs(1)),
                ..policy()
            },
            ExecutionErrorKind::Deadline,
        ),
        (
            ExecutionPolicy {
                cancellation: Some(token),
                ..policy()
            },
            ExecutionErrorKind::Cancelled,
        ),
    ] {
        let mut bound = bindings();
        bound.insert(ty("Uppercase"), binding(Guest::StartTrap));
        for _ in 0..2 {
            let result = execute_with_policy(
                &cell,
                &pipeline(),
                &bound,
                inputs("hello"),
                &Capabilities::new(),
                &limits,
            );
            assert_managed(&result, "z-uppercase", ExecutionPhase::Instantiate, kind);
            assert_empty(&result);
        }
    }
}

#[test]
fn second_node_loop_retains_only_completed_first_node_artifacts() {
    let mut bound = bindings();
    bound.insert(ty("WordCount"), binding(Guest::Loop));
    let result = execute_with_policy(
        &Cell::new().unwrap(),
        &pipeline(),
        &bound,
        inputs("hello world"),
        &Capabilities::new(),
        &policy(),
    );
    assert_managed(
        &result,
        "a-count",
        ExecutionPhase::Invoke,
        ExecutionErrorKind::Fuel,
    );
    assert_eq!(result.completed, ["z-uppercase"]);
    assert_eq!(result.artifacts.len(), 1);
    let artifact = &result.artifacts[&Endpoint::new("z-uppercase", "out")];
    assert_eq!(artifact.ty, ty("Text"));
    assert_eq!(artifact.bytes, b"HELLO WORLD");
}

#[test]
fn policy_preserves_binding_preflight_before_any_guest_starts() {
    let cell = Cell::new().unwrap();
    for missing in [true, false] {
        let mut bound = bindings();
        bound.insert(ty("Uppercase"), binding(Guest::StartTrap));
        if missing {
            bound.remove(&ty("WordCount"));
        } else {
            bound.get_mut(&ty("WordCount")).unwrap().export = "absent".into();
        }
        let result = execute_with_policy(
            &cell,
            &pipeline(),
            &bound,
            inputs("hello"),
            &Capabilities::new(),
            &policy(),
        );
        if missing {
            assert_eq!(result.errors.len(), 1, "{result:?}");
            assert!(
                matches!(&result.errors[0], ExecutionError::Binding { node, .. }
                if node == "a-count"),
                "{result:?}"
            );
        } else {
            assert_managed(
                &result,
                "a-count",
                ExecutionPhase::Binding,
                ExecutionErrorKind::Invalid,
            );
        }
        assert_empty(&result);
    }
}

#[test]
fn live_loops_are_interrupted_by_deadline_and_cancellation() {
    let cell = Cell::new().unwrap();
    for (guest, phase) in [
        (Guest::StartLoop, ExecutionPhase::Instantiate),
        (Guest::Loop, ExecutionPhase::Invoke),
    ] {
        let mut bound = bindings();
        bound.insert(ty("Uppercase"), binding(guest));
        // Warm the component cache before starting the clock so compilation does
        // not consume the time reserved for entering the guest loop.
        for binding in bound.values() {
            cell.component_from_bytes(&binding.component).unwrap();
        }
        for cancel in [false, true] {
            let token = CancellationToken::default();
            let limits = ExecutionPolicy {
                // Finite fallback if epoch interruption regresses; deliberately
                // much larger than the work possible before the timed interrupt.
                fuel: Some(10_000_000_000),
                deadline: (!cancel).then(|| Instant::now() + Duration::from_millis(250)),
                cancellation: cancel.then(|| token.clone()),
                ..ExecutionPolicy::default()
            };
            let result = std::thread::scope(|scope| {
                if cancel {
                    scope.spawn(|| {
                        std::thread::sleep(Duration::from_millis(250));
                        token.cancel();
                    });
                }
                execute_with_policy(
                    &cell,
                    &pipeline(),
                    &bound,
                    inputs("hello"),
                    &Capabilities::new(),
                    &limits,
                )
            });
            assert_managed(
                &result,
                "z-uppercase",
                phase,
                if cancel {
                    ExecutionErrorKind::Cancelled
                } else {
                    ExecutionErrorKind::Deadline
                },
            );
            assert_empty(&result);
        }
    }
}

#[test]
fn compile_and_link_failures_are_classified_before_guest_start() {
    let cell = Cell::new().unwrap();
    for (component, phase) in [
        (b"not a component".to_vec(), ExecutionPhase::Compile),
        (component(Guest::UninstalledImport), ExecutionPhase::Link),
    ] {
        let mut bound = bindings();
        bound.insert(ty("Uppercase"), binding(Guest::StartTrap));
        bound.get_mut(&ty("WordCount")).unwrap().component = component;
        let result = execute_with_policy(
            &cell,
            &pipeline(),
            &bound,
            inputs("hello"),
            &Capabilities::new(),
            &policy(),
        );
        assert_managed(&result, "a-count", phase, ExecutionErrorKind::Invalid);
        assert_empty(&result);
    }
}

#[test]
fn guest_traps_are_classified_at_instantiation_and_invocation() {
    let cell = Cell::new().unwrap();
    for (guest, phase) in [
        (Guest::StartTrap, ExecutionPhase::Instantiate),
        (Guest::Trap, ExecutionPhase::Invoke),
    ] {
        let mut bound = bindings();
        bound.insert(ty("Uppercase"), binding(guest));
        let result = execute_with_policy(
            &cell,
            &pipeline(),
            &bound,
            inputs("hello"),
            &Capabilities::new(),
            &policy(),
        );
        assert_managed(&result, "z-uppercase", phase, ExecutionErrorKind::Trap);
        assert_empty(&result);
    }
}

#[test]
fn policy_preflight_requires_the_exact_port_envelope_abi() {
    let cell = Cell::new().unwrap();
    let no_arguments = r#"(component
        (core module $m (func (export "run")))
        (core instance $i (instantiate $m))
        (func (export "transform") (canon lift (core func $i "run"))))"#;
    // Same flattened calling convention as the envelope, but the payload has
    // u16 elements rather than u8. Structural ABI validation must reject it.
    let wrong_payload = r#"(component
        (core module $m
            (memory (export "memory") 1)
            (func (export "run") (param i32 i32) (result i32) i32.const 0)
            (func (export "realloc") (param i32 i32 i32 i32) (result i32) i32.const 0))
        (core instance $i (instantiate $m))
        (type $values (list (tuple string string string (list u16))))
        (func (export "transform") (param "inputs" $values) (result $values)
            (canon lift (core func $i "run")
                (memory (core memory $i "memory"))
                (realloc (core func $i "realloc")))))"#;
    for wat in [no_arguments, wrong_payload] {
        let bytes = wat::parse_str(wat).unwrap();
        // Ensure the fixture itself compiles: the failure must be Binding, not Compile.
        cell.component_from_bytes(&bytes).unwrap();
        let mut bound = bindings();
        bound.insert(ty("Uppercase"), binding(Guest::StartTrap));
        bound.get_mut(&ty("WordCount")).unwrap().component = bytes;
        let result = execute_with_policy(
            &cell,
            &pipeline(),
            &bound,
            inputs("hello"),
            &Capabilities::new(),
            &policy(),
        );
        assert_managed(
            &result,
            "a-count",
            ExecutionPhase::Binding,
            ExecutionErrorKind::Invalid,
        );
        assert_empty(&result);
    }
}

#[test]
fn policy_preserves_nominal_graph_and_input_validation() {
    let cell = Cell::new().unwrap();
    let mut bound = bindings();
    bound.insert(ty("Uppercase"), binding(Guest::StartTrap));
    let mut plan = pipeline();
    plan.graph.nodes.get_mut("a-count").unwrap().inputs[0].ty = ty("Number");
    let result = execute_with_policy(
        &cell,
        &plan,
        &bound,
        inputs("hello"),
        &Capabilities::new(),
        &policy(),
    );
    assert!(
        result.errors.iter().any(|error| matches!(
            error,
            ExecutionError::Validation(Diagnostic::TypeMismatch { .. })
        )),
        "{result:?}"
    );
    assert!(
        result
            .errors
            .iter()
            .all(|error| matches!(error, ExecutionError::Validation(_)))
    );
    assert_empty(&result);

    let mut supplied = inputs("hello");
    supplied.get_mut("input").unwrap().ty = ty("Number");
    let result = execute_with_policy(
        &cell,
        &pipeline(),
        &bound,
        supplied,
        &Capabilities::new(),
        &policy(),
    );
    assert_eq!(result.errors.len(), 1, "{result:?}");
    assert!(matches!(&result.errors[0], ExecutionError::Input { name, .. } if name == "input"));
    assert_empty(&result);
}

#[test]
fn policy_does_not_publish_nominally_invalid_guest_outputs() {
    let cell = Cell::new().unwrap();
    for guest in [
        Guest::WrongType,
        Guest::WrongVersion,
        Guest::WrongPort,
        Guest::MissingOutput,
        Guest::DuplicateOutput,
    ] {
        let mut bound = bindings();
        bound.insert(ty("Uppercase"), binding(guest));
        let result = execute_with_policy(
            &cell,
            &pipeline(),
            &bound,
            inputs("hello"),
            &Capabilities::new(),
            &policy(),
        );
        assert!(!result.errors.is_empty());
        assert!(
            result.errors.iter().all(|error| matches!(error,
            ExecutionError::Output { node, .. } if node == "z-uppercase")),
            "{result:?}"
        );
        assert_empty(&result);
    }
}
