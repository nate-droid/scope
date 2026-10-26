//! A bounded, synchronous interpreter. Offsets are UTF-8 byte offsets.
//! Bindings and printed values persist between evaluations. Runtime failures keep
//! earlier successful statements and host effects; syntax failures execute nothing.
//! Named functions are top-level, non-recursive, and local to one eval (so body
//! offsets always refer to that eval's source). They snapshot visible values at
//! declaration; all captures and parameters are immutable. Calls share budgets,
//! but never write captured state back. Lists may remain heterogeneous when
//! reassigned: compatibility means the same outer runtime Value variant.
mod admission;
pub use admission::{SnapshotAdmission, admit_snapshot};

pub mod completion;
pub mod host;
mod parser;
pub mod presentation;
pub mod reduction;
pub mod repl;
pub mod runner;
pub mod semantic;
pub mod workbench;

pub use scope_ide::tool::ToolValue as Value;
use scope_ide::tool::operations::OperationError;
use std::collections::BTreeMap;
use std::fmt;

pub trait Host {
    /// Read-only discovery for completion; does not authorize or invoke operations.
    /// Hosts without a metadata catalog need not implement this hook.
    fn operation_names(&self) -> Vec<(String, String)> {
        Vec::new()
    }
    fn operations(&mut self) -> Result<Value, ErrorKind>;
    fn invoke(
        &mut self,
        provider: &str,
        action: &str,
        arguments: Value,
    ) -> Result<Value, ErrorKind>;
}
#[derive(Debug)]
pub enum ErrorKind {
    Operation(OperationError),
    Tool(scope_ide::tool::ToolError),
    Syntax(String),
    Runtime(String),
    LimitExceeded,
}
impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Operation(e) => e.fmt(f),
            Self::Tool(e) => e.fmt(f),
            Self::Syntax(s) => write!(f, "syntax error: {s}"),
            Self::Runtime(s) => write!(f, "runtime error: {s}"),
            Self::LimitExceeded => f.write_str("interpreter resource limit exceeded"),
        }
    }
}
impl std::error::Error for ErrorKind {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Operation(e) => Some(e),
            Self::Tool(e) => Some(e),
            _ => None,
        }
    }
}
impl From<OperationError> for ErrorKind {
    fn from(error: OperationError) -> Self {
        Self::Operation(error)
    }
}
#[derive(Debug)]
pub struct Error {
    pub offset: usize,
    pub kind: ErrorKind,
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at byte {}", self.kind, self.offset)
    }
}
impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.kind)
    }
}
fn runtime(message: &str) -> ErrorKind {
    ErrorKind::Runtime(message.into())
}
fn reserved(name: &str) -> bool {
    matches!(
        name,
        "scope"
            | "print"
            | "let"
            | "mut"
            | "for"
            | "in"
            | "fn"
            | "if"
            | "else"
            | "true"
            | "false"
            | "null"
    )
}

const MAX_WORK: usize = 1_000_000;
const MAX_RETAINED: usize = 1_000_000;
const MAX_STEPS: usize = 100_000;

struct Function {
    params: Vec<String>,
    body: Vec<parser::Statement>,
    captures: BTreeMap<String, Value>,
    size: usize,
}
#[derive(Default)]
pub struct Interpreter {
    bindings: BTreeMap<String, Value>,
    mutable: std::collections::BTreeSet<String>,
    functions: BTreeMap<String, std::rc::Rc<Function>>,
    active: std::collections::BTreeSet<String>,
    output: Vec<Value>,
    retained: usize,
    output_size: usize,
}
impl Interpreter {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn take_output(&mut self) -> Vec<Value> {
        self.retained -= self.output_size;
        self.output_size = 0;
        std::mem::take(&mut self.output)
    }
    pub fn eval(&mut self, source: &str, host: &mut impl Host) -> Result<Value, Error> {
        let statements = parser::parse(source)?;
        // Reject known rebinding before any host effects, including duplicates in this program.
        let names = self.bindings.keys().cloned().collect();
        check_bindings(&statements, &names)?;
        let mut budget = Budget {
            steps: MAX_STEPS,
            work: MAX_WORK,
        };
        let outcome = self.statements(&statements, host, &mut budget);
        for function in self.functions.values() {
            self.retained -= function.size;
        }
        self.functions.clear();
        outcome
    }
    fn statements(
        &mut self,
        statements: &[parser::Statement],
        host: &mut impl Host,
        budget: &mut Budget,
    ) -> Result<Value, Error> {
        let mut result = Value::Null;
        for statement in statements {
            match statement {
                parser::Statement::Assign { name, expr, offset } => {
                    let at = |kind| Error {
                        offset: *offset,
                        kind,
                    };
                    if !self.mutable.contains(name) {
                        return Err(at(runtime("assignment requires a mutable binding")));
                    }
                    let value = self.expression(expr, host, budget)?;
                    let old = &self.bindings[name];
                    if std::mem::discriminant(old) != std::mem::discriminant(&value) {
                        return Err(at(runtime(
                            "assignment requires the same runtime value kind",
                        )));
                    }
                    let old_size = value_size(old, 0).map_err(at)?;
                    let size = value_size(&value, 0).map_err(at)?;
                    self.retain(size.saturating_sub(old_size)).map_err(at)?;
                    self.retained -= old_size.saturating_sub(size);
                    self.bindings.insert(name.clone(), value);
                    result = Value::Null;
                }
                parser::Statement::Function {
                    name,
                    params,
                    body,
                    offset,
                } => {
                    let at = |kind| Error {
                        offset: *offset,
                        kind,
                    };
                    let mut size = 0;
                    for (key, value) in &self.bindings {
                        budget.charge(value).map_err(at)?;
                        size += key.len() + value_size(value, 0).map_err(at)?;
                    }
                    self.retain(size).map_err(at)?;
                    self.functions.insert(
                        name.clone(),
                        std::rc::Rc::new(Function {
                            params: params.clone(),
                            body: body.clone(),
                            captures: self.bindings.clone(),
                            size,
                        }),
                    );
                    result = Value::Null;
                }
                parser::Statement::For {
                    name,
                    collection,
                    body,
                    offset,
                } => {
                    let at = |kind| Error {
                        offset: *offset,
                        kind,
                    };
                    let value = self.expression(collection, host, budget)?;
                    let size = value_size(&value, 0).map_err(at)?;
                    let Value::Array(items) = value else {
                        return Err(at(runtime("for requires a list")));
                    };
                    self.retain(size).map_err(at)?;
                    let outcome = (|| {
                        for item in items {
                            budget.steps = budget
                                .steps
                                .checked_sub(1)
                                .ok_or_else(|| at(ErrorKind::LimitExceeded))?;
                            let previous = self.bindings.keys().cloned().collect();
                            let item_size = name.len() + value_size(&item, 0).map_err(at)?;
                            self.retain(item_size).map_err(at)?;
                            self.bindings.insert(name.clone(), item);
                            let outcome = self.statements(body, host, budget);
                            self.cleanup(&previous);
                            outcome?;
                        }
                        Ok::<_, Error>(())
                    })();
                    self.retained -= size;
                    outcome?;
                    result = Value::Null;
                }
                parser::Statement::Expression {
                    binding,
                    mutable,
                    expr,
                    offset,
                } => {
                    result = self.expression(expr, host, budget)?;
                    if let Some(name) = binding {
                        let size = value_size(&result, 0).map_err(|kind| Error {
                            offset: *offset,
                            kind,
                        })? + name.len();
                        self.retain(size).map_err(|kind| Error {
                            offset: *offset,
                            kind,
                        })?;
                        self.bindings.insert(name.clone(), result.clone());
                        if *mutable {
                            self.mutable.insert(name.clone());
                        }
                    }
                }
                parser::Statement::If { condition, yes, no } => {
                    let value = self.expression(condition, host, budget)?;
                    let Value::Boolean(selected) = value else {
                        return Err(Error {
                            offset: condition.offset,
                            kind: runtime("if condition must be boolean"),
                        });
                    };
                    let previous: std::collections::BTreeSet<_> =
                        self.bindings.keys().cloned().collect();
                    let outcome = self.statements(if selected { yes } else { no }, host, budget);
                    // Clean up locals even on failure; prints and host effects remain.
                    self.cleanup(&previous);
                    outcome?;
                    result = Value::Null;
                }
            }
        }
        Ok(result)
    }
    fn cleanup(&mut self, previous: &std::collections::BTreeSet<String>) {
        self.bindings.retain(|name, value| {
            if previous.contains(name) {
                true
            } else {
                self.retained -= value_size(value, 0).expect("bounded binding") + name.len();
                self.mutable.remove(name);
                false
            }
        });
    }
    fn retain(&mut self, size: usize) -> Result<(), ErrorKind> {
        if size > MAX_RETAINED.saturating_sub(self.retained) {
            return Err(ErrorKind::LimitExceeded);
        }
        self.retained += size;
        Ok(())
    }
    fn expression(
        &mut self,
        expr: &parser::Expr,
        host: &mut impl Host,
        budget: &mut Budget,
    ) -> Result<Value, Error> {
        budget.steps = budget.steps.checked_sub(1).ok_or(Error {
            offset: expr.offset,
            kind: ErrorKind::LimitExceeded,
        })?;
        // Keep named-call frames small enough for the supported call depth,
        // independently of the much larger general expression evaluator.
        if let parser::Kind::Call(name, args) = &expr.kind {
            if let Some(function) = self.functions.get(name).cloned() {
                return self.user_call(name, args, function, expr.offset, host, budget);
            }
        }
        self.expression_inner(expr, host, budget)
    }
    fn user_call(
        &mut self,
        name: &str,
        args: &[parser::Expr],
        function: std::rc::Rc<Function>,
        offset: usize,
        host: &mut impl Host,
        budget: &mut Budget,
    ) -> Result<Value, Error> {
        let at = |kind| Error { offset, kind };
        if args.len() != function.params.len() {
            return Err(at(runtime(&format!(
                "wrong number of arguments for function {name:?}: expected {}, got {}",
                function.params.len(),
                args.len()
            ))));
        }
        if self.active.contains(name) {
            return Err(at(runtime("recursive calls are not supported")));
        }
        if self.active.len() >= 64 {
            return Err(at(ErrorKind::LimitExceeded));
        }
        let values = args
            .iter()
            .map(|arg| self.expression(arg, host, budget))
            .collect::<Result<Vec<_>, _>>()?;
        let mut size = function.size;
        for (param, value) in function.params.iter().zip(&values) {
            size += param.len() + value_size(value, 0).map_err(at)?;
        }
        for value in function.captures.values() {
            budget.charge(value).map_err(at)?;
        }
        self.retain(size).map_err(at)?;
        let mut locals = function.captures.clone();
        for (param, value) in function.params.iter().zip(values) {
            locals.insert(param.clone(), value);
        }
        let saved = std::mem::replace(&mut self.bindings, locals);
        let mutable = std::mem::take(&mut self.mutable);
        self.active.insert(name.to_owned());
        let outcome = self.statements(&function.body, host, budget);
        self.active.remove(name);
        let empty = Default::default();
        self.cleanup(&empty);
        self.bindings = saved;
        self.mutable = mutable;
        let value = outcome?;
        budget.charge(&value).map_err(at)?;
        Ok(value)
    }
    fn expression_inner(
        &mut self,
        expr: &parser::Expr,
        host: &mut impl Host,
        budget: &mut Budget,
    ) -> Result<Value, Error> {
        let at = |kind| Error {
            offset: expr.offset,
            kind,
        };
        use parser::Kind;
        let value = match &expr.kind {
            Kind::Literal(value) => value.clone(),
            Kind::Name(name) => {
                let value = self
                    .bindings
                    .get(name)
                    .ok_or_else(|| at(runtime(&format!("unknown binding {name:?}"))))?;
                budget.charge(value).map_err(at)?;
                return Ok(value.clone());
            }
            Kind::Array(items) => Value::Array(
                items
                    .iter()
                    .map(|item| self.expression(item, host, budget))
                    .collect::<Result<_, _>>()?,
            ),
            Kind::Object(fields) => Value::Object(
                fields
                    .iter()
                    .map(|(key, item)| Ok((key.clone(), self.expression(item, host, budget)?)))
                    .collect::<Result<_, Error>>()?,
            ),
            Kind::Unary(op, item) => match (op.as_str(), self.expression(item, host, budget)?) {
                ("-", Value::Integer(n)) => Value::Integer(
                    n.checked_neg()
                        .ok_or_else(|| at(runtime("integer overflow")))?,
                ),
                ("!", Value::Boolean(b)) => Value::Boolean(!b),
                _ => return Err(at(runtime("invalid unary operand"))),
            },
            Kind::Binary(op, left, right) => {
                let left = self.expression(left, host, budget)?;
                match (op.as_str(), &left) {
                    ("&&", Value::Boolean(false)) => return Ok(Value::Boolean(false)),
                    ("||", Value::Boolean(true)) => return Ok(Value::Boolean(true)),
                    ("&&" | "||", Value::Boolean(_)) => {}
                    ("&&" | "||", _) => {
                        return Err(at(runtime("logical operands must be booleans")));
                    }
                    _ => {}
                }
                let right = self.expression(right, host, budget)?;
                binary(op, left, right).map_err(at)?
            }
            Kind::Index(base, index) => {
                let base = self.expression(base, host, budget)?;
                let index = self.expression(index, host, budget)?;
                match (base, index) {
                    (Value::Array(values), Value::Integer(index)) => {
                        let index = usize::try_from(index)
                            .map_err(|_| at(runtime("invalid array index")))?;
                        values
                            .into_iter()
                            .nth(index)
                            .ok_or_else(|| at(runtime("array index out of bounds")))?
                    }
                    (Value::Object(mut fields), Value::String(key)) => fields
                        .remove(&key)
                        .ok_or_else(|| at(runtime(&format!("missing object field {key:?}"))))?,
                    _ => {
                        return Err(at(runtime(
                            "index requires an array and integer or object and string",
                        )));
                    }
                }
            }
            Kind::Field(base, key) => match self.expression(base, host, budget)? {
                Value::Object(mut fields) => fields
                    .remove(key)
                    .ok_or_else(|| at(runtime(&format!("missing object field {key:?}"))))?,
                _ => return Err(at(runtime("field projection requires an object"))),
            },
            Kind::Call(name, args) => {
                if !matches!(
                    name.as_str(),
                    "print"
                        | "len"
                        | "contains"
                        | "keys"
                        | "join"
                        | "split"
                        | "parse_int"
                        | "starts_with"
                        | "ends_with"
                        | "trim"
                        | "lower"
                        | "upper"
                        | "replace"
                        | "append"
                        | "slice"
                        | "reverse"
                        | "get"
                        | "range"
                        | "abs"
                        | "min"
                        | "max"
                        | "format"
                        | "scope.operations"
                        | "scope.invoke"
                ) {
                    return Err(at(runtime("unknown function")));
                }

                let mut values = args
                    .iter()
                    .map(|e| self.expression(e, host, budget))
                    .collect::<Result<Vec<_>, _>>()?;
                if name == "replace" {
                    if let [Value::String(text), Value::String(from), Value::String(_)] =
                        values.as_slice()
                    {
                        let work = replace_work(text, from).map_err(at)?;
                        budget.work = budget
                            .work
                            .checked_sub(work)
                            .ok_or_else(|| at(ErrorKind::LimitExceeded))?;
                    }
                }
                match name.as_str() {
                    "print" => {
                        let value = values.pop().unwrap();
                        let size = value_size(&value, 0).map_err(at)?;
                        self.retain(size).map_err(at)?;
                        self.output_size += size;
                        self.output.push(value);
                        Value::Null
                    }
                    "scope.operations" => host.operations().map_err(at)?,
                    "scope.invoke" => {
                        let arguments = values.pop().unwrap();
                        let action = values.pop().unwrap();
                        let provider = values.pop().unwrap();
                        match (provider, action, arguments) {
                            (
                                Value::String(provider),
                                Value::String(action),
                                arguments @ Value::Object(_),
                            ) => host.invoke(&provider, &action, arguments).map_err(at)?,
                            _ => {
                                return Err(at(runtime(
                                    "invoke requires provider/action strings and an arguments object",
                                )));
                            }
                        }
                    }
                    "len" | "contains" | "keys" | "join" | "split" | "parse_int"
                    | "starts_with" | "ends_with" | "trim" | "lower" | "upper" | "replace"
                    | "append" | "slice" | "reverse" | "get" | "range" | "abs" | "min" | "max"
                    | "format" => builtin(name, &values, budget.work).map_err(at)?,
                    _ => unreachable!(),
                }
            }
        };
        budget.charge(&value).map_err(at)?;
        Ok(value)
    }
}
fn replace_work(text: &str, from: &str) -> Result<usize, ErrorKind> {
    // Cover both matching passes, even for a naive matcher and empty patterns.
    text.len()
        .checked_add(1)
        .and_then(|n| n.checked_mul(from.len().max(1)))
        .and_then(|n| n.checked_mul(2))
        .filter(|n| *n <= MAX_WORK)
        .ok_or(ErrorKind::LimitExceeded)
}

// Size outputs before allocating; expression() charges the resulting value once.
fn builtin(name: &str, args: &[Value], available: usize) -> Result<Value, ErrorKind> {
    let bounded = |size: Option<usize>| {
        size.filter(|size| *size <= MAX_WORK && *size <= available)
            .ok_or(ErrorKind::LimitExceeded)
    };
    let success = |value: Value| Value::object([("ok", Value::Boolean(true)), ("value", value)]);
    let failure = |code: &str, message: &str| {
        Value::object([
            ("ok", Value::Boolean(false)),
            (
                "error",
                Value::object([("code", code.into()), ("message", message.into())]),
            ),
        ])
    };
    let checked_result = |value: Value| {
        bounded(Some(value_size(&value, 0)?))?;
        Ok(value)
    };
    match (name, args) {
        ("starts_with" | "ends_with", [Value::String(text), Value::String(part)]) => {
            bounded(Some(1))?;
            Ok(Value::Boolean(if name == "starts_with" {
                text.starts_with(part.as_str())
            } else {
                text.ends_with(part.as_str())
            }))
        }
        ("trim", [Value::String(text)]) => {
            let text = text.trim();
            bounded(text.len().checked_add(1))?;
            Ok(Value::String(text.to_owned()))
        }
        ("lower" | "upper", [Value::String(text)]) => {
            // Full-string casing preserves contextual Unicode rules (e.g. final sigma).
            // Contextual final sigma has the same byte length as ordinary sigma.
            let mut size = 1usize;
            for ch in text.chars() {
                let bytes = if name == "lower" {
                    ch.to_lowercase().map(char::len_utf8).sum::<usize>()
                } else {
                    ch.to_uppercase().map(char::len_utf8).sum::<usize>()
                };
                size = bounded(size.checked_add(bytes))?;
            }
            bounded(Some(size))?;
            let output = if name == "lower" {
                text.to_lowercase()
            } else {
                text.to_uppercase()
            };
            checked_result(Value::String(output))
        }
        ("replace", [Value::String(text), Value::String(from), Value::String(to)]) => {
            // Dispatch debits matching work; retain a preflight for direct callers.
            replace_work(text, from)?;
            let count = text.matches(from.as_str()).count();
            let length = count
                .checked_mul(from.len())
                .and_then(|removed| text.len().checked_sub(removed))
                .and_then(|n| {
                    count
                        .checked_mul(to.len())
                        .and_then(|added| n.checked_add(added))
                });
            bounded(length.and_then(|n| n.checked_add(1)))?;
            Ok(Value::String(text.replace(from.as_str(), to)))
        }
        ("append", [Value::Array(items), value]) => {
            let mut size = 1usize;
            for item in items.iter().chain(std::iter::once(value)) {
                size = bounded(size.checked_add(value_size(item, 1)?))?;
            }
            bounded(Some(size))?;
            let mut output = Vec::with_capacity(items.len() + 1);
            output.extend_from_slice(items);
            output.push(value.clone());
            Ok(Value::Array(output))
        }
        ("reverse", [value @ Value::Array(items)]) => {
            bounded(Some(value_size(value, 0)?))?;
            Ok(Value::Array(items.iter().rev().cloned().collect()))
        }
        (
            "slice",
            [
                Value::Array(items),
                Value::Integer(start),
                Value::Integer(end),
            ],
        ) => {
            let selected = usize::try_from(*start)
                .ok()
                .zip(usize::try_from(*end).ok())
                .and_then(|(start, end)| items.get(start..end));
            match selected {
                Some(selected) => {
                    let mut size = value_size(&success(Value::Array(vec![])), 0)?;
                    for item in selected {
                        size = bounded(size.checked_add(value_size(item, 2)?))?;
                    }
                    bounded(Some(size))?;
                    Ok(success(Value::Array(selected.to_vec())))
                }
                None => checked_result(failure(
                    "invalid_index",
                    "expected 0 <= start <= end <= array length",
                )),
            }
        }
        ("get", [collection, key]) => {
            let found = match (collection, key) {
                (Value::Array(items), Value::Integer(index)) => {
                    usize::try_from(*index).ok().and_then(|n| items.get(n))
                }
                (Value::Object(fields), Value::String(key)) => fields.get(key),
                _ => {
                    return Err(runtime(
                        "get requires an array and integer or object and string",
                    ));
                }
            };
            match found {
                Some(value) => {
                    bounded(
                        value_size(&success(Value::Null), 0)?
                            .checked_sub(1)
                            .and_then(|n| value_size(value, 1).ok().and_then(|s| n.checked_add(s))),
                    )?;
                    Ok(success(value.clone()))
                }
                None => checked_result(failure("missing_value", "requested value is missing")),
            }
        }
        ("range", [Value::Integer(start), Value::Integer(end)]) => {
            let length = usize::try_from((i128::from(*end) - i128::from(*start)).max(0))
                .map_err(|_| ErrorKind::LimitExceeded)?;
            bounded(length.checked_add(1))?;
            Ok(Value::Array((*start..*end).map(Value::Integer).collect()))
        }
        ("abs", [Value::Integer(value)]) => {
            bounded(Some(1))?;
            value
                .checked_abs()
                .map(Value::Integer)
                .ok_or_else(|| runtime("integer overflow"))
        }
        ("min" | "max", [Value::Integer(a), Value::Integer(b)]) => {
            bounded(Some(1))?;
            Ok(Value::Integer(if name == "min" {
                *a.min(b)
            } else {
                *a.max(b)
            }))
        }
        ("format", [value]) => {
            // Escaping, indentation and truncation annotations can expand input.
            // This conservative bound covers the default formatter's depth (8),
            // six-byte JSON escapes and per-node truncation markers.
            bounded(
                value_size(value, 0)?
                    .checked_mul(64)
                    .and_then(|n| n.checked_add(1)),
            )?;
            checked_result(Value::String(presentation::format_value(value)))
        }
        (
            "starts_with" | "ends_with" | "trim" | "lower" | "upper" | "replace" | "append"
            | "slice" | "reverse" | "get" | "range" | "abs" | "min" | "max" | "format",
            _,
        ) => {
            let signature = match name {
                "starts_with" | "ends_with" => "(string, string)",
                "trim" | "lower" | "upper" => "(string)",
                "replace" => "(string, string, string)",
                "append" => "(array, value)",
                "slice" => "(array, integer, integer)",
                "reverse" => "(array)",
                "get" => "(array, integer) or (object, string)",
                "range" | "min" | "max" => "(integer, integer)",
                "abs" => "(integer)",
                "format" => "(value)",
                _ => unreachable!(),
            };
            Err(runtime(&format!("{name} requires {signature}")))
        }
        ("parse_int", [Value::String(text)]) => Ok(match text.parse::<i64>() {
            Ok(value) => Value::object([
                ("ok", Value::Boolean(true)),
                ("value", Value::Integer(value)),
            ]),
            Err(_) => Value::object([
                ("ok", Value::Boolean(false)),
                (
                    "error",
                    Value::object([
                        ("code", "invalid_integer".into()),
                        ("message", "expected a signed 64-bit decimal integer".into()),
                    ]),
                ),
            ]),
        }),
        ("parse_int", _) => Err(runtime("parse_int requires a string")),
        ("len", [value]) => {
            let length = match value {
                Value::String(text) => text.chars().count(),
                Value::Array(items) => items.len(),
                Value::Object(fields) => fields.len(),
                _ => return Err(runtime("len requires a string, array, or object")),
            };
            Ok(Value::Integer(length as i64))
        }
        ("contains", [collection, item]) => {
            let found = match (collection, item) {
                (Value::String(text), Value::String(needle)) => text.contains(needle.as_str()),
                (Value::Array(items), item) => items.contains(item),
                (Value::Object(fields), Value::String(key)) => fields.contains_key(key),
                _ => {
                    return Err(runtime(
                        "contains requires a string and string, array and value, or object and string",
                    ));
                }
            };
            Ok(Value::Boolean(found))
        }
        ("keys", [Value::Object(fields)]) => {
            let mut size = 1usize;
            for key in fields.keys() {
                size = bounded(size.checked_add(1).and_then(|n| n.checked_add(key.len())))?;
            }
            bounded(Some(size))?;
            Ok(Value::Array(
                fields.keys().cloned().map(Value::String).collect(),
            ))
        }
        ("join", [Value::Array(items), Value::String(separator)]) => {
            let mut length = 0usize;
            for (index, item) in items.iter().enumerate() {
                let Value::String(text) = item else {
                    return Err(runtime("join requires string elements"));
                };
                length = length
                    .checked_add(text.len())
                    .and_then(|n| n.checked_add(if index == 0 { 0 } else { separator.len() }))
                    .ok_or(ErrorKind::LimitExceeded)?;
            }
            bounded(length.checked_add(1))?;
            let mut output = String::with_capacity(length);
            for (index, item) in items.iter().enumerate() {
                if index != 0 {
                    output.push_str(separator);
                }
                if let Value::String(text) = item {
                    output.push_str(text);
                }
            }
            Ok(Value::String(output))
        }
        ("split", [Value::String(text), Value::String(separator)]) => {
            let mut size = 1usize;
            for part in text.split(separator.as_str()) {
                size = bounded(size.checked_add(1).and_then(|n| n.checked_add(part.len())))?;
            }
            Ok(Value::Array(
                text.split(separator.as_str())
                    .map(|part| Value::String(part.to_owned()))
                    .collect(),
            ))
        }
        ("keys", _) => Err(runtime("keys requires an object")),
        ("join", _) => Err(runtime("join requires an array and string separator")),
        ("split", _) => Err(runtime("split requires a string and string separator")),
        _ => unreachable!("parser validates builtin names and arity"),
    }
}

fn check_bindings(
    statements: &[parser::Statement],
    outer: &std::collections::BTreeSet<String>,
) -> Result<(), Error> {
    // Reserve every binding in this lexical scope before checking children, so
    // shadowing does not depend on the order of outer declarations.
    let mut names = outer.clone();
    for statement in statements {
        let declaration = match statement {
            parser::Statement::Expression {
                binding: Some(name),
                offset,
                ..
            }
            | parser::Statement::Function { name, offset, .. } => Some((name, offset)),
            _ => None,
        };
        if let Some((name, offset)) = declaration {
            if !names.insert(name.clone()) {
                return Err(Error {
                    offset: *offset,
                    kind: runtime("binding already exists"),
                });
            }
        }
    }
    for statement in statements {
        match statement {
            parser::Statement::If { yes, no, .. } => {
                check_bindings(yes, &names)?;
                check_bindings(no, &names)?;
            }
            parser::Statement::For {
                name, body, offset, ..
            } => {
                let mut locals = names.clone();
                if !locals.insert(name.clone()) {
                    return Err(Error {
                        offset: *offset,
                        kind: runtime("binding already exists"),
                    });
                }
                check_bindings(body, &locals)?;
            }
            parser::Statement::Function {
                params,
                body,
                offset,
                ..
            } => {
                let mut locals = names.clone();
                for param in params {
                    if !locals.insert(param.clone()) {
                        return Err(Error {
                            offset: *offset,
                            kind: runtime("parameter already exists"),
                        });
                    }
                }
                check_bindings(body, &locals)?;
            }
            _ => {}
        }
    }
    Ok(())
}
struct Budget {
    steps: usize,
    work: usize,
}
impl Budget {
    fn charge(&mut self, value: &Value) -> Result<(), ErrorKind> {
        let size = value_size(value, 0)?;
        self.work = self
            .work
            .checked_sub(size)
            .ok_or(ErrorKind::LimitExceeded)?;
        Ok(())
    }
}
// Count both nodes and string bytes, and stop scanning oversized host results early.
fn value_size(value: &Value, depth: usize) -> Result<usize, ErrorKind> {
    if depth > parser::MAX_DEPTH {
        return Err(ErrorKind::LimitExceeded);
    }
    let mut size = 1usize;
    let mut add = |amount: usize| -> Result<(), ErrorKind> {
        size = size
            .checked_add(amount)
            .filter(|n| *n <= MAX_WORK)
            .ok_or(ErrorKind::LimitExceeded)?;
        Ok(())
    };
    match value {
        Value::String(s) => add(s.len())?,
        Value::Array(items) => {
            for item in items {
                add(value_size(item, depth + 1)?)?;
            }
        }
        Value::Object(fields) => {
            for (key, item) in fields {
                add(key.len())?;
                add(value_size(item, depth + 1)?)?;
            }
        }
        _ => {}
    }
    Ok(size)
}
fn binary(op: &str, left: Value, right: Value) -> Result<Value, ErrorKind> {
    if op == "==" {
        return Ok(Value::Boolean(left == right));
    }
    if op == "!=" {
        return Ok(Value::Boolean(left != right));
    }
    match (left, right) {
        (Value::Integer(a), Value::Integer(b)) => {
            let n = match op {
                "+" => a.checked_add(b),
                "-" => a.checked_sub(b),
                "*" => a.checked_mul(b),
                "/" => a.checked_div(b),
                "%" => a.checked_rem(b),
                "<" => return Ok(Value::Boolean(a < b)),
                "<=" => return Ok(Value::Boolean(a <= b)),
                ">" => return Ok(Value::Boolean(a > b)),
                ">=" => return Ok(Value::Boolean(a >= b)),
                _ => return Err(runtime("invalid integer operator")),
            };
            n.map(Value::Integer)
                .ok_or_else(|| runtime("integer overflow or division by zero"))
        }
        (Value::Boolean(a), Value::Boolean(b)) if op == "&&" || op == "||" => {
            Ok(Value::Boolean(if op == "&&" { a && b } else { a || b }))
        }
        (Value::String(a), Value::String(b)) => match op {
            "<" => Ok(Value::Boolean(a < b)),
            "<=" => Ok(Value::Boolean(a <= b)),
            ">" => Ok(Value::Boolean(a > b)),
            ">=" => Ok(Value::Boolean(a >= b)),
            _ => Err(runtime("invalid string operator")),
        },
        _ => Err(runtime("incompatible operands")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Default)]
    struct TestHost {
        calls: usize,
    }
    impl Host for TestHost {
        fn operations(&mut self) -> Result<Value, ErrorKind> {
            self.calls += 1;
            Ok(Value::Array(vec![Value::String("test".into())]))
        }
        fn invoke(&mut self, _: &str, _: &str, arguments: Value) -> Result<Value, ErrorKind> {
            self.calls += 1;
            Ok(arguments)
        }
    }
    #[test]
    fn s13_helpers() {
        let mut host = TestHost::default();
        for (source, expected) in [
            (r#"starts_with("é🦀", "é")"#, "true"),
            (r#"ends_with("", "")"#, "true"),
            (r#"trim("　 é \n")"#, r#""é""#),
            (r#"lower("ΟΣİ")"#, r#""οσi̇""#),
            (r#"lower("ΟΣ")"#, r#""ος""#),
            (r#"upper("straße")"#, r#""STRASSE""#),
            (r#"replace("é🦀", "", "-")"#, r#""-é-🦀-""#),
            (r#"replace("aaa", "aa", "b")"#, r#""ba""#),
            ("append([1], {a: null})", "[1, {a: null}]"),
            ("reverse([1, true, null])", "[null, true, 1]"),
            ("reverse([])", "[]"),
            ("slice([1, true], 1, 2)", "{ok: true, value: [true]}"),
            ("slice([], 0, 0)", "{ok: true, value: []}"),
            ("slice([], -1, 0).error.code", r#""invalid_index""#),
            ("slice([1], 1, 0).ok", "false"),
            ("get([null], 0)", "{ok: true, value: null}"),
            (r#"get({a: false}, "a")"#, "{ok: true, value: false}"),
            ("get([], -1).error.code", r#""missing_value""#),
            ("get([], 0).ok", "false"),
            (r#"get({}, "a").ok"#, "false"),
            ("range(-2, 1)", "[-2, -1, 0]"),
            ("range(2, 1)", "[]"),
            ("abs(-7)", "7"),
            ("min(-1, 2)", "-1"),
            ("max(-1, 2)", "2"),
            ("format([1, true])", r#""[1, true]""#),
        ] {
            let expected = Interpreter::new().eval(expected, &mut host).unwrap();
            for _ in 0..2 {
                assert_eq!(
                    Interpreter::new().eval(source, &mut host).unwrap(),
                    expected,
                    "{source}"
                );
            }
        }
        for source in [
            "starts_with(1, 2)",
            "trim([])",
            "lower(null)",
            "upper(1)",
            "replace(1, 2, 3)",
            "append({}, 1)",
            "slice([], true, 0)",
            "reverse(1)",
            "get([], true)",
            "range(0, true)",
            "abs(true)",
            "min(1, false)",
            "max(null, 0)",
            "abs(-9223372036854775807 - 1)",
        ] {
            assert!(
                matches!(
                    Interpreter::new().eval(source, &mut host).unwrap_err().kind,
                    ErrorKind::Runtime(_)
                ),
                "{source}"
            );
        }
        assert_eq!(host.calls, 0);
    }

    #[test]
    fn s13_signatures_and_reserved_names() {
        let mut host = TestHost::default();
        for (name, args) in [
            ("starts_with", "1, 2"),
            ("ends_with", "1, 2"),
            ("trim", "1"),
            ("lower", "1"),
            ("upper", "1"),
            ("replace", "1, 2, 3"),
            ("append", "1, 2"),
            ("slice", "1, 2, 3"),
            ("reverse", "1"),
            ("get", "1, 2"),
            ("range", "1, true"),
            ("abs", "true"),
            ("min", "1, true"),
            ("max", "1, true"),
            ("format", "1"),
        ] {
            for source in [
                format!("{name}()"),
                format!("{name}({args}, null)"),
                format!("fn {name}() {{ null }}"),
            ] {
                let error = Interpreter::new().eval(&source, &mut host).unwrap_err();
                assert!(
                    matches!(error.kind, ErrorKind::Syntax(_)),
                    "{source}: {error}"
                );
            }
            let source = format!("{name}({args})");
            let result = Interpreter::new().eval(&source, &mut host);
            if name == "format" {
                assert!(result.is_ok());
            } else {
                let error = result.unwrap_err();
                assert!(
                    matches!(&error.kind, ErrorKind::Runtime(message)
                    if message.contains(name) && message.contains("requires")),
                    "{source}: {error}"
                );
            }
        }
        assert_eq!(host.calls, 0);
    }

    #[test]
    fn s13_replace_debits_matching_work() {
        let mut interpreter = Interpreter::new();
        let mut host = TestHost::default();
        // Each call returns an empty string, but its matching passes are not free.
        let source = format!(
            "let text = \"{}\"; for n in range(0, 10) {{ replace(text, text, \"\") }}",
            "a".repeat(250)
        );
        assert!(matches!(
            interpreter.eval(&source, &mut host).unwrap_err().kind,
            ErrorKind::LimitExceeded
        ));
    }

    #[test]
    fn s13_depth_and_formatter_bounds() {
        let mut nested = Value::Null;
        for _ in 0..parser::MAX_DEPTH {
            nested = Value::Array(vec![nested]);
        }
        assert!(value_size(&nested, 0).is_ok());
        for (name, args) in [
            ("append", vec![Value::Array(vec![]), nested.clone()]),
            (
                "slice",
                vec![
                    Value::Array(vec![nested.clone()]),
                    Value::Integer(0),
                    Value::Integer(1),
                ],
            ),
            ("get", vec![Value::Array(vec![nested]), Value::Integer(0)]),
        ] {
            assert!(
                matches!(
                    builtin(name, &args, MAX_WORK),
                    Err(ErrorKind::LimitExceeded)
                ),
                "{name}"
            );
        }
        for value in [
            Value::Integer(i64::MIN),
            "\0".repeat(201).into(),
            Value::Array(vec![Value::Array(vec![Value::Null; 21]); 21]),
            Value::object([("\0", Value::String("\0".repeat(201)))]),
        ] {
            let bound = value_size(&value, 0).unwrap() * 64 + 1;
            let output = builtin("format", &[value], bound).unwrap();
            assert!(value_size(&output, 0).unwrap() <= bound);
        }
    }

    #[test]
    fn s13_budgets() {
        for (name, args) in [
            (
                "range",
                vec![Value::Integer(i64::MIN), Value::Integer(i64::MAX)],
            ),
            (
                "replace",
                vec!["a".repeat(2000).into(), "a".repeat(2000).into(), "".into()],
            ),
            ("append", vec![Value::Array(vec![]), Value::Null]),
            (
                "slice",
                vec![Value::Array(vec![]), Value::Integer(0), Value::Integer(0)],
            ),
            ("get", vec![Value::Array(vec![]), Value::Integer(-1)]),
            ("reverse", vec![Value::Array(vec![])]),
            ("upper", vec!["ß".into()]),
            ("format", vec![Value::Null]),
        ] {
            assert!(
                matches!(builtin(name, &args, 0), Err(ErrorKind::LimitExceeded)),
                "{name}"
            );
        }
        let args = [Value::Array(vec![Value::Integer(1)]), Value::Boolean(true)];
        assert!(matches!(
            builtin("append", &args, 2),
            Err(ErrorKind::LimitExceeded)
        ));
        assert_eq!(
            builtin("append", &args, 3).unwrap(),
            Value::Array(vec![Value::Integer(1), Value::Boolean(true)])
        );
        assert!(matches!(
            builtin(
                "replace",
                &["a".repeat(2000).into(), "a".repeat(2000).into(), "".into()],
                MAX_WORK
            ),
            Err(ErrorKind::LimitExceeded)
        ));
    }

    #[test]
    fn pure_helpers() {
        let mut i = Interpreter::new();
        let mut h = TestHost::default();
        for (source, expected) in [
            (r#"len("é🦀é")"#, "4"),
            ("len([])", "0"),
            ("len({b: 1, a: 2})", "2"),
            (r#"contains("café", "fé")"#, "true"),
            (r#"contains("", "")"#, "true"),
            ("contains([{a: [1, null]}], {a: [1, null]})", "true"),
            ("contains([1], true)", "false"),
            (r#"contains({a: null}, "a")"#, "true"),
            (r#"keys({z: 0, a: 1, "é": 2})"#, r#"["a", "z", "é"]"#),
            (r#"join(["é", "🦀", ""], "|")"#, r#""é|🦀|""#),
            (r#"join([], "x")"#, r#""""#),
            (r#"split("é🦀", "")"#, r#"["", "é", "🦀", ""]"#),
            (r#"split("a,,", ",")"#, r#"["a", "", ""]"#),
            (r#"split("", ",")"#, r#"[""]"#),
            (r#"split("aaa", "aa")"#, r#"["", "a"]"#),
        ] {
            // Use the parser for expected values to avoid presentation dependencies.
            let expected = Interpreter::new().eval(expected, &mut h).unwrap();
            assert_eq!(i.eval(source, &mut h).unwrap(), expected, "{source}");
        }
        assert_eq!(h.calls, 0);
        assert_eq!(i.eval("let len = 7; let contains = 8; let keys = 9; let join = 10; let split = 11; len + split", &mut h).unwrap(), Value::Integer(18));
        assert_eq!(i.eval("len([len])", &mut h).unwrap(), Value::Integer(1));
    }

    #[test]
    fn helper_errors_and_named_diagnostics() {
        for source in [
            "len(1)",
            "contains(1, 2)",
            r#"contains("a", 1)"#,
            "contains({}, 1)",
            "keys([])",
            "join([1], \"\")",
            "join([], 1)",
            "split(1, \"\")",
            "split(\"a\", [])",
        ] {
            assert!(
                matches!(
                    Interpreter::new()
                        .eval(source, &mut TestHost::default())
                        .unwrap_err()
                        .kind,
                    ErrorKind::Runtime(_)
                ),
                "{source}"
            );
        }
        for name in ["len", "contains", "keys", "join", "split"] {
            for args in ["", "1, 2, 3"] {
                assert!(matches!(
                    Interpreter::new()
                        .eval(
                            &format!("print(1); {name}({args})"),
                            &mut TestHost::default()
                        )
                        .unwrap_err()
                        .kind,
                    ErrorKind::Syntax(_)
                ));
            }
        }
        for (source, name) in [
            ("absent", "absent"),
            ("{}.missing", "missing"),
            (r#"{}["missing"]"#, "missing"),
        ] {
            let error = Interpreter::new()
                .eval(source, &mut TestHost::default())
                .unwrap_err();
            assert_eq!(error.offset, 0);
            assert!(error.to_string().contains(name));
        }
    }

    #[test]
    fn helper_budgets() {
        let mut i = Interpreter::new();
        let mut h = TestHost::default();
        i.bindings
            .insert("large".into(), Value::String("x".repeat(MAX_WORK)));
        assert!(matches!(
            i.eval("len(large)", &mut h).unwrap_err().kind,
            ErrorKind::LimitExceeded
        ));
        i.bindings.insert(
            "parts".into(),
            Value::Array(vec![Value::String(String::new()); 2000]),
        );
        i.bindings
            .insert("separator".into(), Value::String("x".repeat(1000)));
        assert!(matches!(
            i.eval("join(parts, separator)", &mut h).unwrap_err().kind,
            ErrorKind::LimitExceeded
        ));
        for (source, work) in [
            (r#"join(["a", "b"], "-")"#, 10),
            (r#"split("ab", "")"#, 10),
            ("keys({a: 1})", 5),
            ("len([])", 1),
        ] {
            let statements = parser::parse(source).unwrap();
            let mut budget = Budget {
                steps: MAX_STEPS,
                work,
            };
            assert!(
                matches!(
                    i.statements(&statements, &mut h, &mut budget)
                        .unwrap_err()
                        .kind,
                    ErrorKind::LimitExceeded
                ),
                "{source}"
            );
        }
        assert_eq!(
            builtin(
                "join",
                &[Value::Array(vec![]), Value::String(String::new())],
                1
            )
            .unwrap(),
            Value::String(String::new())
        );
    }

    #[test]
    fn line_comments_preserve_separators_and_strings() {
        let mut i = Interpreter::new();
        let mut h = TestHost::default();
        assert_eq!(
            i.eval(
                "# first\nlet a = [1, # item\n2]\n# middle\nlen(a) # eof",
                &mut h
            )
            .unwrap(),
            Value::Integer(2)
        );
        assert_eq!(
            i.eval(
                "if true { # branch\nprint(\"# literal\") # end\n} # between\nelse { print(0) }",
                &mut h
            )
            .unwrap(),
            Value::Null
        );
        assert_eq!(i.take_output(), [Value::String("# literal".into())]);
    }

    #[test]
    fn bindings_output_and_projection() {
        let mut i = Interpreter::new();
        let mut h = TestHost::default();
        assert_eq!(i.eval("let x = scope.invoke(\"p\", \"a\", {items: [4, 7]})\nprint(x.items[1]); x[\"items\"][0] + 2 * 3", &mut h).unwrap(), Value::Integer(10));
        assert_eq!(i.take_output(), vec![Value::Integer(7)]);
        assert!(i.take_output().is_empty());
        assert_eq!(i.eval("x.items[0]", &mut h).unwrap(), Value::Integer(4));
        assert!(i.eval("let x = 3", &mut h).is_err());
        assert!(i.eval("let scope = 3", &mut h).is_err());
    }
    #[test]
    fn syntax_is_checked_before_effects() {
        let mut i = Interpreter::new();
        let mut h = TestHost::default();
        assert!(i.eval("scope.operations(); let x = [", &mut h).is_err());
        assert_eq!(h.calls, 0);
        assert!(i.eval("print(1); let x = 1; let x = 2", &mut h).is_err());
        assert!(i.take_output().is_empty());
    }
    #[test]
    fn arithmetic_and_short_circuit() {
        let mut i = Interpreter::new();
        let mut h = TestHost::default();
        assert_eq!(
            i.eval("!false && 2 + 3 * 4 == 14", &mut h).unwrap(),
            Value::Boolean(true)
        );
        assert_eq!(
            i.eval("false && scope.operations()", &mut h).unwrap(),
            Value::Boolean(false)
        );
        assert_eq!(h.calls, 0);
        for source in [
            "9223372036854775807 + 1",
            "1 / 0",
            "-(-9223372036854775808)",
            "[1][-1]",
            "true + 1",
        ] {
            assert!(i.eval(source, &mut h).is_err(), "{source}");
        }
    }
    #[test]
    fn conditional_scopes_offsets_and_persistence() {
        let mut i = Interpreter::new();
        let mut h = TestHost::default();
        i.eval("let baseline = 42", &mut h).unwrap();
        i.eval("if baseline > 0 {\n let local = {items: [true, null]}\n if local.items[0] { print(local) } else { scope.operations() }\n}\nprint(baseline)", &mut h).unwrap();
        assert_eq!(h.calls, 0);
        assert_eq!(i.take_output().len(), 2);
        assert!(i.eval("local", &mut h).is_err());
        for source in [
            "if 1 { print(1) }",
            "if null {}",
            "if [] {}",
            "if {} {}",
            "if true { let baseline = 2 }",
            "if false { let x = 1; let x = 2 }",
            "scope.operations(); if false { print([) }",
        ] {
            assert!(i.eval(source, &mut h).is_err(), "{source}");
        }
        assert_eq!(h.calls, 0);
        let source = "if true { let temporary = 1; print(temporary); missing.field }";
        let error = i.eval(source, &mut h).unwrap_err();
        assert_eq!(error.offset, source.find("missing").unwrap());
        assert_eq!(i.take_output(), vec![Value::Integer(1)]);
        assert!(i.eval("temporary", &mut h).is_err());
        i.eval(
            "if false { let same = 1 } else { let same = 2; print(same) }",
            &mut h,
        )
        .unwrap();
        assert_eq!(i.take_output(), vec![Value::Integer(2)]);
        assert_eq!(i.eval("baseline", &mut h).unwrap(), Value::Integer(42));
        assert!(
            i.eval(
                &format!("{}{}", "if true {".repeat(65), "}".repeat(65)),
                &mut h
            )
            .is_err()
        );
        // All selected expressions share the existing work budget.
        i.eval(&format!("let large = \"{}\"", "x".repeat(60_000)), &mut h)
            .unwrap();
        let source = format!("if true {{ {} }}", "large;".repeat(30));
        assert!(matches!(
            i.eval(&source, &mut h).unwrap_err().kind,
            ErrorKind::LimitExceeded
        ));
        // A skipped branch consumes no expression work or host calls.
        i.eval(&source.replacen("true", "false", 1), &mut h)
            .unwrap();
    }
    #[test]
    fn conditional_step_budget_is_shared() {
        let mut i = Interpreter::new();
        let mut h = TestHost::default();
        let statements = parser::parse("if true { if true { 1 } }").unwrap();
        let mut budget = Budget {
            steps: 2,
            work: MAX_WORK,
        };
        assert!(matches!(
            i.statements(&statements, &mut h, &mut budget)
                .unwrap_err()
                .kind,
            ErrorKind::LimitExceeded
        ));
    }
    #[test]
    fn resource_limits() {
        let mut i = Interpreter::new();
        let mut h = TestHost::default();
        assert!(i.eval(&"[".repeat(100), &mut h).is_err());
        assert!(i.eval(&"1 + ".repeat(1000), &mut h).is_err());
        assert!(i.eval(&" ".repeat(65_537), &mut h).is_err());
        // Repeated immutable copies cannot grow without bound across eval calls.
        i.eval(&format!("let big = \"{}\"", "x".repeat(60_000)), &mut h)
            .unwrap();
        let mut limited = false;
        for n in 0..30 {
            if i.eval(&format!("let copy{n} = big"), &mut h).is_err() {
                limited = true;
                break;
            }
        }
        assert!(limited);
    }
    #[test]
    fn loops_mutation_and_fresh_locals() {
        let mut i = Interpreter::new();
        let mut h = TestHost::default();
        assert_eq!(i.eval("let mut total = 0; for item in [1,2,3] { let local = item; total = total + local }; total", &mut h).unwrap(), Value::Integer(6));
        assert!(i.eval("item", &mut h).is_err());
        assert!(i.eval("local", &mut h).is_err());
        i.eval(
            "let mut mixed = [1, true]; mixed = [null, {}]; mixed = []",
            &mut h,
        )
        .unwrap();
        assert!(i.eval("total = false", &mut h).is_err());
        assert_eq!(i.eval("total", &mut h).unwrap(), Value::Integer(6));
        assert!(i.eval("let fixed = 1; fixed = 2", &mut h).is_err());
        for source in ["for x in 1 {}", "for x in {} {}", "for x in null {}"] {
            assert!(i.eval(source, &mut h).is_err());
        }
        let retained = i.retained;
        let source = "for x in [1,2] { let temp = x; print(x); missing }";
        let error = i.eval(source, &mut h).unwrap_err();
        assert_eq!(error.offset, source.find("missing").unwrap());
        assert_eq!(i.take_output(), vec![Value::Integer(1)]);
        assert_eq!(i.retained, retained);
        assert!(i.eval("temp", &mut h).is_err());
    }

    #[test]
    fn functions_capture_isolation_and_eval_lifetime() {
        for _ in 0..3 {
            let mut i = Interpreter::new();
            let mut h = TestHost::default();
            assert_eq!(
                i.eval(
                    "let mut x = 3; fn add(y) { let local = y + x; local }; x = 9; add(4)",
                    &mut h
                )
                .unwrap(),
                Value::Integer(7)
            );
            assert_eq!(i.retained, 2);
            assert!(i.eval("add(4)", &mut h).is_err());
            assert_eq!(i.eval("x", &mut h).unwrap(), Value::Integer(9));
            assert!(i.eval("fn bad() { x = 7; x }; bad()", &mut h).is_err());
            assert_eq!(i.eval("x", &mut h).unwrap(), Value::Integer(9));
            assert_eq!(i.eval("fn sum(items) { let mut acc = 0; for n in items { acc = acc + n }; acc }; sum([2,3])", &mut h).unwrap(), Value::Integer(5));
            assert!(i.functions.is_empty());
            assert!(i.active.is_empty());
            assert_eq!(i.retained, 2);
        }
    }

    #[test]
    fn s11_preflight_and_call_diagnostics() {
        let mut i = Interpreter::new();
        let mut h = TestHost::default();
        for source in [
            "scope.operations(); for x in [] { let x = 1 }",
            "scope.operations(); fn f(a,a) { a }",
            "scope.operations(); fn f(a) { let a = 1; a }",
            "scope.operations(); fn f() { 1 }; let f = 2",
            "scope.operations(); for x in [] {}; let x = 1",
            "scope.operations(); if false { fn f() { 1 } }",
            "scope.operations(); fn f() {}",
        ] {
            assert!(i.eval(source, &mut h).is_err(), "{source}");
            assert_eq!(h.calls, 0);
        }
        for (source, message) in [
            ("fn f(a) { a }; f()", "wrong number"),
            ("fn f() { f() }; f()", "recursive"),
            ("fn f() { g() }; fn g() { f() }; f()", "recursive"),
            ("missing()", "unknown function"),
            ("fn f(a) { a + 1 }; f(true)", "incompatible"),
        ] {
            let error = i.eval(source, &mut h).unwrap_err();
            assert!(error.to_string().contains(message), "{error}");
            assert!(i.functions.is_empty());
            assert!(i.active.is_empty());
            assert_eq!(i.retained, 0);
        }
        let source = "fn f() { unknown }; f()";
        assert_eq!(
            i.eval(source, &mut h).unwrap_err().offset,
            source.find("unknown").unwrap()
        );
    }

    #[test]
    fn s11_shared_budgets_and_retention() {
        let mut i = Interpreter::new();
        let mut h = TestHost::default();
        let statements = parser::parse("for x in [1,2,3] {}").unwrap();
        let mut budget = Budget {
            steps: 6,
            work: MAX_WORK,
        };
        assert!(matches!(
            i.statements(&statements, &mut h, &mut budget)
                .unwrap_err()
                .kind,
            ErrorKind::LimitExceeded
        ));
        assert_eq!(i.retained, 0);
        i.eval(&format!("let large = \"{}\"", "x".repeat(60_000)), &mut h)
            .unwrap();
        assert!(matches!(
            i.eval(
                "fn f() { large }; for n in [1,2,3,4,5,6,7,8,9,10] { f() }",
                &mut h
            )
            .unwrap_err()
            .kind,
            ErrorKind::LimitExceeded
        ));
        assert_eq!(i.retained, 60_006);
        let source = (0..20)
            .map(|n| format!("fn f{n}() {{ 0 }};"))
            .collect::<String>();
        assert!(matches!(
            i.eval(&source, &mut h).unwrap_err().kind,
            ErrorKind::LimitExceeded
        ));
        assert_eq!(i.retained, 60_006);
        assert!(i.functions.is_empty());
    }
    #[test]
    fn s11_capability_denial_propagates_and_cleans_up() {
        struct DeniedHost {
            attempts: usize,
        }
        impl Host for DeniedHost {
            fn operations(&mut self) -> Result<Value, ErrorKind> {
                Ok(Value::Array(vec![]))
            }
            fn invoke(&mut self, _: &str, _: &str, _: Value) -> Result<Value, ErrorKind> {
                self.attempts += 1;
                Err(ErrorKind::Operation(OperationError::Unauthorized))
            }
        }
        for source in [
            "for item in [1,2] { let local = item; scope.invoke(\"p\", \"a\", {}) }",
            "fn denied() { let local = 1; scope.invoke(\"p\", \"a\", {}) }; denied()",
            "fn denied() { scope.invoke(\"p\", \"a\", {}) }; for item in [1,2] { denied() }",
        ] {
            let mut i = Interpreter::new();
            let mut h = DeniedHost { attempts: 0 };
            let error = i.eval(source, &mut h).unwrap_err();
            assert!(matches!(
                error.kind,
                ErrorKind::Operation(OperationError::Unauthorized)
            ));
            assert_eq!(error.offset, source.find("scope.invoke").unwrap());
            assert_eq!(h.attempts, 1);
            assert!(i.bindings.is_empty());
            assert!(i.mutable.is_empty());
            assert!(i.functions.is_empty());
            assert!(i.active.is_empty());
            assert_eq!(i.retained, 0);
        }
    }

    #[test]
    fn s11_large_host_lists_are_bounded() {
        struct ListHost {
            length: usize,
        }
        impl Host for ListHost {
            fn operations(&mut self) -> Result<Value, ErrorKind> {
                Ok(Value::Array(vec![Value::Null; self.length]))
            }
            fn invoke(&mut self, _: &str, _: &str, _: Value) -> Result<Value, ErrorKind> {
                unreachable!()
            }
        }
        for length in [100_001, MAX_WORK] {
            let mut i = Interpreter::new();
            let mut h = ListHost { length };
            let error = i
                .eval("for item in scope.operations() {}", &mut h)
                .unwrap_err();
            assert!(matches!(error.kind, ErrorKind::LimitExceeded));
            assert!(i.bindings.is_empty());
            assert_eq!(i.retained, 0);
        }
        // A host collection fits by itself, but must coexist with persistent bindings.
        let mut i = Interpreter::new();
        let mut host = TestHost::default();
        i.eval(
            &format!("let saved = \"{}\"", "x".repeat(60_000)),
            &mut host,
        )
        .unwrap();
        let retained = i.retained;
        let mut h = ListHost { length: 950_000 };
        assert!(matches!(
            i.eval("for item in scope.operations() {}", &mut h)
                .unwrap_err()
                .kind,
            ErrorKind::LimitExceeded
        ));
        assert_eq!(i.retained, retained);
    }

    #[test]
    fn s11_nonrecursive_call_depth_boundary() {
        for depth in [64, 65] {
            let mut source = String::new();
            for n in 0..depth {
                if n + 1 == depth {
                    source.push_str(&format!("fn f{n}() {{ 42 }};"));
                } else {
                    source.push_str(&format!("fn f{n}() {{ f{}() }};", n + 1));
                }
            }
            source.push_str("f0()");
            let mut i = Interpreter::new();
            let mut h = TestHost::default();
            let result = i.eval(&source, &mut h);
            if depth == 64 {
                assert_eq!(result.unwrap(), Value::Integer(42));
            } else {
                let error = result.unwrap_err();
                assert!(matches!(error.kind, ErrorKind::LimitExceeded));
                assert_eq!(error.offset, source.find("f64()").unwrap());
            }
            assert!(i.bindings.is_empty());
            assert!(i.active.is_empty());
            assert!(i.functions.is_empty());
            assert_eq!(i.retained, 0);
        }
    }

    #[test]
    fn s11_user_arity_reports_name_counts_and_skips_arguments() {
        let mut i = Interpreter::new();
        let mut h = TestHost::default();
        let source = "fn identity(value) { value }; identity(scope.operations(), 2)";
        let error = i.eval(source, &mut h).unwrap_err();
        assert_eq!(error.offset, source.find("identity(scope").unwrap());
        let message = error.to_string();
        assert!(message.contains("identity"));
        assert!(message.contains("expected 1, got 2"));
        assert_eq!(h.calls, 0);
        assert_eq!(i.retained, 0);
    }
}
