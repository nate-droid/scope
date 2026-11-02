//! Opt-in host-only regression for the unpublished upstream Cell correction.
//! No direct Cell/Wasmtime dependency, language guest, or agent authority.

#[cfg(test)]
mod tests {
    use scope_execution::{
        Binding, Bindings, Cell, ExecutionError, ExecutionErrorKind, ExecutionPhase,
        ExecutionPolicy, Value, execute_with_policy,
    };
    use scope_types::{
        Capabilities, Connection, Endpoint, Graph, Pipeline, Port, Registry, Source,
        Transformation, TypeId,
    };
    use std::collections::BTreeMap;

    #[test]
    fn host_selects_transfer_policy_and_receives_typed_failure_evidence() {
        let ty = TypeId::new("Bytes", "1");
        let transform = Transformation {
            id: TypeId::new("finite-diagnostic", "1"),
            inputs: vec![],
            outputs: vec![Port::new("out", ty.clone())],
            requires: Capabilities::from(["evaluate".into()]),
        };
        let mut registry = Registry::default();
        registry.register(transform.clone()).unwrap();
        let pipeline = Pipeline {
            graph: Graph {
                inputs: BTreeMap::new(),
                nodes: BTreeMap::from([("diagnostic".into(), transform.clone())]),
                connections: Vec::<Connection>::new(),
            },
            output: Source::Output(Endpoint::new("diagnostic", "out")),
        };
        // Fixed two-page memory; output is a single port with a finite 64 KiB
        // byte list. No imports, growth, loops, or guest allocation are involved.
        let bindings = Bindings::from([(
            transform.id,
            Binding {
                export: "evaluate".into(),
                component: br#"(component
                  (core module $m
                    (memory (export "memory") 2 2)
                    (data (i32.const 0) "\20\00\00\00\01\00\00\00")
                    (data (i32.const 32)
                      "\80\00\00\00\03\00\00\00\88\00\00\00\05\00\00\00\90\00\00\00\01\00\00\00\00\01\00\00\00\00\01\00")
                    (data (i32.const 128) "out")
                    (data (i32.const 136) "Bytes")
                    (data (i32.const 144) "1")
                    (func (export "realloc") (param i32 i32 i32 i32) (result i32)
                      i32.const 100000)
                    (func (export "run") (param i32 i32) (result i32) i32.const 0))
                  (core instance $i (instantiate $m))
                  (type $ports (list (tuple string string string (list u8))))
                  (func (export "evaluate") (param "inputs" $ports) (result $ports)
                    (canon lift (core func $i "run")
                      (memory (core memory $i "memory"))
                      (realloc (core func $i "realloc")))))"#.to_vec(),
            },
        )]);
        let cell = Cell::new().unwrap(); // No installed guest capabilities.
        let grants = Capabilities::from(["evaluate".into()]);
        let base = ExecutionPolicy {
            fuel: Some(100_000),
            memory_bytes: Some(2 * 65_536),
            ..Default::default()
        };
        // A declaration does not grant execution, even with an approved binding.
        let denied = execute_with_policy(
            &cell,
            &pipeline,
            &bindings,
            BTreeMap::new(),
            &Capabilities::new(),
            &base,
        );
        assert!(!denied.errors.is_empty());
        assert!(denied.completed.is_empty());
        assert!(denied.artifacts.is_empty());

        for (transfer, logical, expected) in [
            (8, 8, ExecutionErrorKind::Transfer),
            (16 * 1024 * 1024, 8, ExecutionErrorKind::Output),
        ] {
            let policy = ExecutionPolicy {
                host_transfer_bytes: Some(transfer),
                output_bytes: Some(logical),
                ..base.clone()
            };
            let result = execute_with_policy(
                &cell,
                &pipeline,
                &bindings,
                BTreeMap::new(),
                &grants,
                &policy,
            );
            assert!(
                matches!(result.errors.as_slice(), [ExecutionError::Managed {
                node, phase: ExecutionPhase::Invoke, kind, message,
            }] if node == "diagnostic" && *kind == expected && !message.is_empty()),
                "{result:?}"
            );
            assert!(result.completed.is_empty());
            assert!(result.artifacts.is_empty());
            assert!(result.output.is_none());
        }
        // Scope creates fresh stores: earlier transfer failure does not poison
        // the Cell or the next execution, and only admitted artifacts publish.
        let policy = ExecutionPolicy {
            host_transfer_bytes: Some(16 * 1024 * 1024),
            output_bytes: Some(70_000),
            ..base
        };
        let result = execute_with_policy(
            &cell,
            &pipeline,
            &bindings,
            BTreeMap::new(),
            &grants,
            &policy,
        );
        assert!(result.errors.is_empty(), "{result:?}");
        assert_eq!(result.completed, ["diagnostic"]);
        assert_eq!(
            result.output,
            Some(Value {
                ty,
                bytes: vec![0; 65_536]
            })
        );
        assert_eq!(result.artifacts.len(), 1);
    }
}
