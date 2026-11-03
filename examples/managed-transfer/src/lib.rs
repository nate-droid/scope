//! Opt-in host-only regression for the published Cell managed-transfer release.
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
            transform.id.clone(),
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

        // Grants do not make a missing selected export executable.
        let mut missing_export_bindings = bindings.clone();
        missing_export_bindings
            .get_mut(&transform.id)
            .unwrap()
            .export = "missing".into();
        let denied = execute_with_policy(
            &cell,
            &pipeline,
            &missing_export_bindings,
            BTreeMap::new(),
            &grants,
            &base,
        );
        assert!(!denied.errors.is_empty(), "{denied:?}");
        assert!(denied.completed.is_empty());
        assert!(denied.artifacts.is_empty());
        assert!(denied.output.is_none());

        // This allowance admits the envelope and small payload, so subsequent
        // transfer failures depend on the large payload rather than metadata.
        let mut small_bindings = bindings.clone();
        let binding = small_bindings.get_mut(&transform.id).unwrap();
        let component = String::from_utf8(binding.component.clone()).unwrap();
        let small_component = component.replace(
            r#"\00\01\00\00\00\00\01\00""#,
            r#"\00\01\00\00\01\00\00\00""#,
        );
        assert_ne!(small_component, component);
        binding.component = small_component.into_bytes();
        let small_policy = ExecutionPolicy {
            host_transfer_bytes: Some(4096),
            output_bytes: Some(70_000),
            ..base.clone()
        };
        let small = execute_with_policy(
            &cell,
            &pipeline,
            &small_bindings,
            BTreeMap::new(),
            &grants,
            &small_policy,
        );
        assert!(small.errors.is_empty(), "{small:?}");
        assert_eq!(
            small.output,
            Some(Value {
                ty: ty.clone(),
                bytes: vec![0]
            })
        );
        assert_eq!(
            ExecutionPolicy::default().effective_host_transfer_bytes(),
            128 * 1024 * 1024
        );
        assert_eq!(
            ExecutionPolicy {
                host_transfer_bytes: Some(0),
                ..Default::default()
            }
            .effective_host_transfer_bytes(),
            0
        );

        // Keep the port-envelope ABI fixed, but lift a finite 64 KiB port name
        // before the byte list in this failure-only fixture. Zero-filled memory
        // is valid UTF-8; the normal fixture remains available for typed success.
        let mut string_bindings = small_bindings.clone();
        let binding = string_bindings.get_mut(&transform.id).unwrap();
        binding.component = String::from_utf8(binding.component.clone())
            .unwrap()
            .replace(r"\80\00\00\00\03\00\00\00", r"\00\01\00\00\00\00\01\00")
            .into_bytes();
        for (fixture, transfer, expected) in [
            (&bindings, 4096, ExecutionErrorKind::Transfer),
            (&bindings, 16 * 1024 * 1024, ExecutionErrorKind::Output),
            (&string_bindings, 4096, ExecutionErrorKind::Transfer),
            (
                &string_bindings,
                16 * 1024 * 1024,
                ExecutionErrorKind::Output,
            ),
        ] {
            let policy = ExecutionPolicy {
                host_transfer_bytes: Some(transfer),
                output_bytes: Some(8),
                ..base.clone()
            };
            let result =
                execute_with_policy(&cell, &pipeline, fixture, BTreeMap::new(), &grants, &policy);
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
