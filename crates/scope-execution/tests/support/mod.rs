use scope_execution::{Binding, Bindings, Value};
use scope_types::{Connection, Endpoint, Graph, Pipeline, Port, Source, Transformation, TypeId};
use std::collections::BTreeMap;

pub fn ty(name: &str) -> TypeId {
    TypeId::new(name, "1")
}

#[derive(Clone, Copy)]
pub enum Guest {
    Uppercase,
    WordCount,
    WrongType,
    WrongVersion,
    WrongPort,
    MissingOutput,
    DuplicateOutput,
    Trap,
    StartTrap,
    UninstalledImport,
}

/// Real components: all payload processing happens in core Wasm, never in a host callback.
pub fn component(guest: Guest) -> Vec<u8> {
    let importing = matches!(guest, Guest::UninstalledImport);
    let (component_import, core_import, instance) = if importing {
        (
            "(import \"uninstalled\" (func $host)) (core func $host-lowered (canon lower (func $host))) (core instance $host-instance (export \"call\" (func $host-lowered)))",
            "(import \"host\" \"call\" (func $call))",
            "(core instance $i (instantiate $m (with \"host\" (instance $host-instance))))",
        )
    } else {
        ("", "", "(core instance $i (instantiate $m))")
    };
    let body = match guest {
        Guest::Uppercase => {
            r#"
          (block $done (loop $loop
            (br_if $done (i32.ge_u (local.get $index) (local.get $len)))
            (local.set $byte (i32.load8_u (i32.add (local.get $payload) (local.get $index))))
            (if (i32.and (i32.ge_u (local.get $byte) (i32.const 97)) (i32.le_u (local.get $byte) (i32.const 122)))
              (then (i32.store8 (i32.add (local.get $payload) (local.get $index)) (i32.sub (local.get $byte) (i32.const 32)))))
            (local.set $index (i32.add (local.get $index) (i32.const 1))) (br $loop)))
        "#
        }
        Guest::WordCount => {
            r#"
          (block $done (loop $loop
            (br_if $done (i32.ge_u (local.get $index) (local.get $len)))
            (local.set $byte (i32.load8_u (i32.add (local.get $payload) (local.get $index))))
            (local.set $space (i32.or (i32.eq (local.get $byte) (i32.const 32))
              (i32.and (i32.ge_u (local.get $byte) (i32.const 9)) (i32.le_u (local.get $byte) (i32.const 13)))))
            (if (i32.and (i32.eqz (local.get $space)) (i32.eqz (local.get $inside)))
              (then (local.set $count (i64.add (local.get $count) (i64.const 1)))))
            (local.set $inside (i32.eqz (local.get $space)))
            (local.set $index (i32.add (local.get $index) (i32.const 1))) (br $loop)))
          (i64.store (i32.const 320) (local.get $count))
          (local.set $payload (i32.const 320)) (local.set $len (i32.const 8))
        "#
        }
        Guest::Trap => "unreachable",
        Guest::UninstalledImport => "(call $call)",
        _ => "",
    };
    let name = if matches!(guest, Guest::WordCount | Guest::WrongType) {
        "Number"
    } else {
        "Text"
    };
    let version = if matches!(guest, Guest::WrongVersion) {
        "2"
    } else {
        "1"
    };
    let port = if matches!(guest, Guest::WrongPort) {
        "oops"
    } else {
        "out"
    };
    let length = match guest {
        Guest::MissingOutput => 0,
        Guest::DuplicateOutput => 2,
        _ => 1,
    };
    let start = if matches!(guest, Guest::StartTrap) {
        "(func $start unreachable) (start $start)"
    } else {
        ""
    };
    let wat = format!(
        r#"(component
      {component_import}
      (core module $m
        {core_import}
        {start}
        (memory (export "memory") 2)
        (global $heap (mut i32) (i32.const 4096))
        (data (i32.const 256) "{port}")
        (data (i32.const 272) "{name}")
        (data (i32.const 288) "{version}")
        (func (export "realloc") (param i32 i32 i32 i32) (result i32) (local $ptr i32)
          (local.set $ptr (i32.and (i32.add (global.get $heap) (i32.const 7)) (i32.const -8)))
          (global.set $heap (i32.add (local.get $ptr) (local.get 3))) (local.get $ptr))
        (func (export "run") (param $args i32) (param $argc i32) (result i32)
          (local $payload i32) (local $len i32) (local $index i32) (local $byte i32)
          (local $space i32) (local $inside i32) (local $count i64)
          (local.set $payload (i32.load offset=24 (local.get $args)))
          (local.set $len (i32.load offset=28 (local.get $args)))
          {body}
          (i32.store (i32.const 128) (i32.const 256))
          (i32.store (i32.const 132) (i32.const {port_len}))
          (i32.store (i32.const 136) (i32.const 272))
          (i32.store (i32.const 140) (i32.const {name_len}))
          (i32.store (i32.const 144) (i32.const 288))
          (i32.store (i32.const 148) (i32.const 1))
          (i32.store (i32.const 152) (local.get $payload))
          (i32.store (i32.const 156) (local.get $len))
          (memory.copy (i32.const 160) (i32.const 128) (i32.const 32))
          (i32.store (i32.const 64) (i32.const 128))
          (i32.store (i32.const 68) (i32.const {length}))
          (i32.const 64)))
      {instance}
      (type $value (tuple string string string (list u8)))
      (type $values (list $value))
      (func (export "transform") (param "inputs" $values) (result $values)
        (canon lift (core func $i "run") (memory (core memory $i "memory")) (realloc (core func $i "realloc")))))"#,
        port_len = port.len(),
        name_len = name.len()
    );
    wat::parse_str(wat).expect("fixture must be valid component WAT")
}

pub fn pipeline() -> Pipeline {
    let transform = |name: &str, output: &str| Transformation {
        id: ty(name),
        inputs: vec![Port::new("in", ty("Text"))],
        outputs: vec![Port::new("out", ty(output))],
        requires: Default::default(),
    };
    Pipeline {
        graph: Graph {
            inputs: BTreeMap::from([("input".into(), ty("Text"))]),
            nodes: BTreeMap::from([
                ("z-uppercase".into(), transform("Uppercase", "Text")),
                ("a-count".into(), transform("WordCount", "Number")),
            ]),
            connections: vec![
                Connection {
                    from: Source::Input("input".into()),
                    to: Endpoint::new("z-uppercase", "in"),
                },
                Connection {
                    from: Source::Output(Endpoint::new("z-uppercase", "out")),
                    to: Endpoint::new("a-count", "in"),
                },
            ],
        },
        output: Source::Output(Endpoint::new("a-count", "out")),
    }
}

pub fn bindings() -> Bindings {
    BTreeMap::from([
        (ty("Uppercase"), binding(Guest::Uppercase)),
        (ty("WordCount"), binding(Guest::WordCount)),
    ])
}

pub fn binding(guest: Guest) -> Binding {
    Binding {
        component: component(guest),
        export: "transform".into(),
    }
}

pub fn inputs(text: &str) -> BTreeMap<String, Value> {
    BTreeMap::from([(
        "input".into(),
        Value {
            ty: ty("Text"),
            bytes: text.as_bytes().to_vec(),
        },
    )])
}
