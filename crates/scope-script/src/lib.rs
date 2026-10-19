//! A bounded, synchronous expression interpreter. Offsets are UTF-8 byte offsets.
//! Bindings and printed values persist between evaluations. Runtime failures keep
//! earlier successful statements and host effects; syntax failures execute nothing.
pub mod host;
mod parser;
pub mod repl;

pub use scope_ide::tool::ToolValue as Value;
use scope_ide::tool::operations::OperationError;
use std::collections::BTreeMap;
use std::fmt;

pub trait Host {
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
    matches!(name, "scope" | "print" | "let" | "true" | "false" | "null")
}

const MAX_WORK: usize = 1_000_000;
const MAX_RETAINED: usize = 1_000_000;
const MAX_STEPS: usize = 100_000;

#[derive(Default)]
pub struct Interpreter {
    bindings: BTreeMap<String, Value>,
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
        let mut names = std::collections::BTreeSet::new();
        for statement in &statements {
            if let Some(name) = &statement.binding {
                if self.bindings.contains_key(name) || !names.insert(name) {
                    return Err(Error {
                        offset: statement.offset,
                        kind: runtime("binding already exists"),
                    });
                }
            }
        }
        let mut budget = Budget {
            steps: MAX_STEPS,
            work: MAX_WORK,
        };
        let mut result = Value::Null;
        for statement in statements {
            result = self.expression(&statement.expr, host, &mut budget)?;
            if let Some(name) = statement.binding {
                let size = value_size(&result, 0).map_err(|kind| Error {
                    offset: statement.offset,
                    kind,
                })? + name.len();
                self.retain(size).map_err(|kind| Error {
                    offset: statement.offset,
                    kind,
                })?;
                self.bindings.insert(name, result.clone());
            }
        }
        Ok(result)
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
        let at = |kind| Error {
            offset: expr.offset,
            kind,
        };
        budget.steps = budget
            .steps
            .checked_sub(1)
            .ok_or_else(|| at(ErrorKind::LimitExceeded))?;
        use parser::Kind;
        let value = match &expr.kind {
            Kind::Literal(value) => value.clone(),
            Kind::Name(name) => {
                let value = self
                    .bindings
                    .get(name)
                    .ok_or_else(|| at(runtime("unknown binding")))?;
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
                        .ok_or_else(|| at(runtime("missing object field")))?,
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
                    .ok_or_else(|| at(runtime("missing object field")))?,
                _ => return Err(at(runtime("field projection requires an object"))),
            },
            Kind::Call(name, args) => {
                let mut values = args
                    .iter()
                    .map(|e| self.expression(e, host, budget))
                    .collect::<Result<Vec<_>, _>>()?;
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
                    _ => unreachable!(),
                }
            }
        };
        budget.charge(&value).map_err(at)?;
        Ok(value)
    }
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
}
