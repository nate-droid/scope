//! Synchronous, language-neutral execution. Cell alone supplies runtime authority.
use scope_types::{Capabilities, Diagnostic, Endpoint, Pipeline, Source, TypeId};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Value {
    pub ty: TypeId,
    pub bytes: Vec<u8>,
}

/// Component bytes and an export implementing the documented port-envelope ABI.
#[derive(Clone, Debug)]
pub struct Binding {
    pub component: Vec<u8>,
    pub export: String,
}
pub type Bindings = BTreeMap<TypeId, Binding>;
type Envelope = Vec<(String, String, String, Vec<u8>)>;

fn is_envelope(ty: &wasmtime::component::types::Type) -> bool {
    use wasmtime::component::types::Type;
    let Type::List(list) = ty else {
        return false;
    };
    let Type::Tuple(tuple) = list.ty() else {
        return false;
    };
    let fields: Vec<_> = tuple.types().collect();
    matches!(fields.as_slice(), [Type::String, Type::String, Type::String, Type::List(bytes)] if matches!(bytes.ty(), Type::U8))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExecutionError {
    Validation(Diagnostic),
    Binding {
        node: String,
        message: String,
    },
    Input {
        name: String,
        message: String,
    },
    Runtime {
        node: String,
        message: String,
    },
    Output {
        node: String,
        message: String,
    },
    Managed {
        node: String,
        phase: ExecutionPhase,
        kind: cell::ExecutionErrorKind,
        message: String,
    },
}

/// Stage at which a policy-aware execution failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecutionPhase {
    Compile,
    Link,
    Binding,
    Instantiate,
    Invoke,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExecutionResult {
    pub output: Option<Value>,
    pub artifacts: BTreeMap<Endpoint, Value>,
    pub completed: Vec<String>,
    pub errors: Vec<ExecutionError>,
}

fn resolve<'a>(
    source: &Source,
    inputs: &'a BTreeMap<String, Value>,
    artifacts: &'a BTreeMap<Endpoint, Value>,
) -> Option<&'a Value> {
    match source {
        Source::Input(name) => inputs.get(name),
        Source::Output(endpoint) => artifacts.get(endpoint),
    }
}

/// Validate the whole plan and all component links/exports before calling a guest.
/// Grants are host planning permissions, not authorization tokens. They neither
/// install capabilities nor override the policies in the supplied Cell.
/// Fail-fast execution retains artifacts from already completed nodes.
pub fn execute(
    cell: &cell::Cell,
    pipeline: &Pipeline,
    bindings: &Bindings,
    inputs: BTreeMap<String, Value>,
    granted: &Capabilities,
) -> ExecutionResult {
    execute_inner(cell, pipeline, bindings, inputs, granted, None)
}

/// Execute with Cell-managed limits. Budgets apply separately to each node's
/// instantiation and call, not cumulatively to the pipeline. Capabilities are
/// supplied exclusively by the caller's Cell; planning grants install nothing.
pub fn execute_with_policy(
    cell: &cell::Cell,
    pipeline: &Pipeline,
    bindings: &Bindings,
    inputs: BTreeMap<String, Value>,
    granted: &Capabilities,
    policy: &cell::ExecutionPolicy,
) -> ExecutionResult {
    execute_inner(cell, pipeline, bindings, inputs, granted, Some(policy))
}

fn execute_inner(
    cell: &cell::Cell,
    pipeline: &Pipeline,
    bindings: &Bindings,
    inputs: BTreeMap<String, Value>,
    granted: &Capabilities,
    policy: Option<&cell::ExecutionPolicy>,
) -> ExecutionResult {
    let mut result = ExecutionResult::default();
    let graph = &pipeline.graph;
    result.errors.extend(
        graph
            .validate(granted)
            .into_iter()
            .map(ExecutionError::Validation),
    );
    for (name, ty) in &graph.inputs {
        match inputs.get(name) {
            Some(value) if &value.ty == ty => {}
            Some(value) => result.errors.push(ExecutionError::Input {
                name: name.clone(),
                message: format!("expected {ty}, got {}", value.ty),
            }),
            None => result.errors.push(ExecutionError::Input {
                name: name.clone(),
                message: "missing graph input".into(),
            }),
        }
    }
    for name in inputs
        .keys()
        .filter(|name| !graph.inputs.contains_key(*name))
    {
        result.errors.push(ExecutionError::Input {
            name: name.clone(),
            message: "unknown graph input".into(),
        });
    }
    if graph.source_type(&pipeline.output).is_err() {
        result.errors.push(ExecutionError::Output {
            node: String::new(),
            message: "unknown selected pipeline output".into(),
        });
    }
    if !result.errors.is_empty() {
        return result;
    }

    let mut prepared = BTreeMap::new();
    for (name, node) in &graph.nodes {
        let Some(binding) = bindings.get(&node.id) else {
            result.errors.push(ExecutionError::Binding {
                node: name.clone(),
                message: format!("no implementation for {}", node.id),
            });
            continue;
        };
        // Link preflight has no guest execution. Instantiation is deferred because
        // component start functions can execute and must follow dependency order.
        let mut phase = ExecutionPhase::Compile;
        let preparation = (|| {
            let component = cell.component_from_bytes(&binding.component)?;
            phase = ExecutionPhase::Link;
            let pre = cell.linker().instantiate_pre(&component)?;
            phase = ExecutionPhase::Binding;
            let (_, export) = component
                .get_export(None, &binding.export)
                .ok_or_else(|| format!("missing export {}", binding.export))
                .map_err(std::io::Error::other)?;
            // Check the complete ABI without instantiation or running start functions.
            let component_type = component.component_type();
            let Some(wasmtime::component::types::ComponentItem::ComponentFunc(func)) =
                component_type
                    .get_export(cell.engine(), &binding.export)
                    .map(|export| export.ty)
            else {
                return Err(std::io::Error::other("export is not a component function").into());
            };
            if func.params().len() != 1
                || func.results().len() != 1
                || !func.params().all(|(_, ty)| is_envelope(&ty))
                || !func.results().all(|ty| is_envelope(&ty))
            {
                return Err(std::io::Error::other(
                    "export does not implement the port-envelope ABI",
                )
                .into());
            }
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>((component, pre, export))
        })();
        match preparation {
            Ok((component, pre, export)) => {
                prepared.insert(name.clone(), (component, pre, export));
            }
            Err(error) => result.errors.push(if policy.is_some() {
                ExecutionError::Managed {
                    node: name.clone(),
                    phase,
                    kind: cell::ExecutionErrorKind::Invalid,
                    message: format!("{error:#}"),
                }
            } else {
                ExecutionError::Binding {
                    node: name.clone(),
                    message: format!("{error:#}"),
                }
            }),
        }
    }
    if !result.errors.is_empty() {
        return result;
    }

    let mut remaining: BTreeSet<String> = graph.nodes.keys().cloned().collect();
    while !remaining.is_empty() {
        let name = remaining
            .iter()
            .find(|name| {
                graph
                    .connections
                    .iter()
                    .filter(|c| &c.to.node == *name)
                    .all(|c| resolve(&c.from, &inputs, &result.artifacts).is_some())
            })
            .cloned()
            .expect("validated acyclic graph has a ready node");
        remaining.remove(&name);
        let node = &graph.nodes[&name];
        let arguments: Envelope = node
            .inputs
            .iter()
            .map(|port| {
                let connection = graph
                    .connections
                    .iter()
                    .find(|c| c.to == Endpoint::new(&name, &port.name))
                    .unwrap();
                let value = resolve(&connection.from, &inputs, &result.artifacts).unwrap();
                (
                    port.name.clone(),
                    value.ty.name.clone(),
                    value.ty.version.clone(),
                    value.bytes.clone(),
                )
            })
            .collect();
        let (component, pre, export) = prepared.remove(&name).unwrap();
        let call = if let Some(policy) = policy {
            let mut store = cell.managed_store((), policy.clone());
            let managed = cell
                .instantiate_managed(&mut store, &component)
                .map_err(|error| (ExecutionPhase::Instantiate, error))
                .and_then(|instance| {
                    store
                        .call(
                            &instance,
                            &bindings[&node.id].export,
                            &[encode_envelope(arguments)],
                        )
                        .map_err(|error| (ExecutionPhase::Invoke, error))
                });
            match managed {
                Ok(values) => match decode_envelope(values) {
                    Some(outputs) => Ok(outputs),
                    None => {
                        result.errors.push(ExecutionError::Output {
                            node: name,
                            message: "invalid managed result envelope".into(),
                        });
                        return result;
                    }
                },
                Err((phase, error)) => {
                    result.errors.push(ExecutionError::Managed {
                        node: name,
                        phase,
                        kind: error.kind,
                        message: error.to_string(),
                    });
                    return result;
                }
            }
        } else {
            (|| {
                let mut store = cell.store(());
                let instance = pre.instantiate(&mut store)?;
                let function =
                    instance.get_typed_func::<(Envelope,), (Envelope,)>(&mut store, &export)?;
                let (outputs,) = function.call(&mut store, (arguments,))?;

                Ok::<_, Box<dyn std::error::Error + Send + Sync>>(outputs)
            })()
        };
        let outputs = match call {
            Ok(outputs) => outputs,
            Err(error) => {
                result.errors.push(ExecutionError::Runtime {
                    node: name,
                    message: format!("{error:#}"),
                });
                return result;
            }
        };
        let mut validated = BTreeMap::new();
        for (port, ty, version, bytes) in outputs {
            let value = Value {
                ty: TypeId::new(ty, version),
                bytes,
            };
            if !node
                .outputs
                .iter()
                .any(|p| p.name == port && p.ty == value.ty)
                || validated
                    .insert(Endpoint::new(&name, &port), value)
                    .is_some()
            {
                result.errors.push(ExecutionError::Output {
                    node: name.clone(),
                    message: format!("unexpected, duplicate, or incorrectly typed output {port}"),
                });
            }
        }
        for port in &node.outputs {
            if !validated.contains_key(&Endpoint::new(&name, &port.name)) {
                result.errors.push(ExecutionError::Output {
                    node: name.clone(),
                    message: format!("missing output {}", port.name),
                });
            }
        }
        if !result.errors.is_empty() {
            return result;
        }
        result.artifacts.extend(validated);
        result.completed.push(name);
    }
    result.output = resolve(&pipeline.output, &inputs, &result.artifacts).cloned();
    result
}

fn encode_envelope(values: Envelope) -> wasmtime::component::Val {
    use wasmtime::component::Val;
    Val::List(
        values
            .into_iter()
            .map(|(port, name, version, bytes)| {
                Val::Tuple(vec![
                    Val::String(port),
                    Val::String(name),
                    Val::String(version),
                    Val::List(bytes.into_iter().map(Val::U8).collect()),
                ])
            })
            .collect(),
    )
}

fn decode_envelope(mut results: Vec<wasmtime::component::Val>) -> Option<Envelope> {
    use wasmtime::component::Val;
    if results.len() != 1 {
        return None;
    }
    let Val::List(values) = results.pop()? else {
        return None;
    };
    values
        .into_iter()
        .map(|value| {
            let Val::Tuple(fields) = value else {
                return None;
            };
            let mut fields = fields.into_iter();
            let Val::String(port) = fields.next()? else {
                return None;
            };
            let Val::String(name) = fields.next()? else {
                return None;
            };
            let Val::String(version) = fields.next()? else {
                return None;
            };
            let Val::List(bytes) = fields.next()? else {
                return None;
            };
            if fields.next().is_some() {
                return None;
            }
            let bytes = bytes
                .into_iter()
                .map(|v| if let Val::U8(b) = v { Some(b) } else { None })
                .collect::<Option<Vec<_>>>()?;
            Some((port, name, version, bytes))
        })
        .collect()
}
