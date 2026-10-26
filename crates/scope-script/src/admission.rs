//! Syntax-only admission for continuous scratch snapshots.
use crate::Error;
use crate::parser::{self, Expr, Kind, Statement};

/// Admission is independent of interpreter state and never evaluates source.
#[derive(Debug)]
pub enum SnapshotAdmission {
    /// Complete syntax with no print or host calls anywhere in the AST.
    CompletePure,
    /// Complete syntax containing a print or host call, even in unused code.
    CompleteExplicitRunRequired,
    /// A lexer or parser expectation reached EOF. Carries the original diagnostic.
    Incomplete(Error),
    /// Syntax or parser resource-limit failure that was not an EOF expectation.
    Invalid(Error),
}

/// Classify a whole Script snapshot without invoking a host or interpreter.
///
/// This is syntax admission, not semantic validation: unknown names, invalid
/// operand types, and other runtime failures can still occur in complete input.
/// All branches and function bodies are inspected conservatively; local bindings
/// and mutation are pure for this purpose. Empty/comment-only input is pure.
/// Diagnostic offsets are UTF-8 byte offsets into `source`.
pub fn admit_snapshot(source: &str) -> SnapshotAdmission {
    match parser::parse_snapshot(source) {
        (Ok(statements), _) if effectful(&statements) => {
            SnapshotAdmission::CompleteExplicitRunRequired
        }
        (Ok(_), _) => SnapshotAdmission::CompletePure,
        (Err(error), true) => SnapshotAdmission::Incomplete(error),
        (Err(error), false) => SnapshotAdmission::Invalid(error),
    }
}

fn effectful(statements: &[Statement]) -> bool {
    statements.iter().any(|statement| match statement {
        Statement::Expression { expr, .. } | Statement::Assign { expr, .. } => {
            expression_effectful(expr)
        }
        Statement::For {
            collection, body, ..
        } => expression_effectful(collection) || effectful(body),
        Statement::Function { body, .. } => effectful(body),
        Statement::If { condition, yes, no } => {
            expression_effectful(condition) || effectful(yes) || effectful(no)
        }
    })
}

fn expression_effectful(expr: &Expr) -> bool {
    match &expr.kind {
        Kind::Literal(_) | Kind::Name(_) => false,
        Kind::Array(items) => items.iter().any(expression_effectful),
        Kind::Object(fields) => fields.iter().any(|(_, value)| expression_effectful(value)),
        Kind::Unary(_, value) | Kind::Field(value, _) => expression_effectful(value),
        Kind::Binary(_, left, right) | Kind::Index(left, right) => {
            expression_effectful(left) || expression_effectful(right)
        }
        Kind::Call(name, args) => {
            name == "print" || name.starts_with("scope.") || args.iter().any(expression_effectful)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn complete_pure_is_syntax_only() {
        for source in [
            "",
            "# print(scope.operations())",
            "\"print(scope.invoke())\"",
            "let mut x = 1; x = x + 1; x",
            "len([1, 2])",
            "fn double(x) { x * 2 }; double(3)",
            "for x in range(0, 3) { x + 1 }",
            "if true { 1 } else { 2 }",
            "1 / 0",
            "unknown(unknown_binding)",
        ] {
            assert!(
                matches!(admit_snapshot(source), SnapshotAdmission::CompletePure),
                "{source}"
            );
        }
    }

    #[test]
    fn effects_are_conservative_and_recursive() {
        for source in [
            "print(1)",
            "scope.operations()",
            "scope.invoke(\"p\", \"a\", {})",
            "false && print(1)",
            "if false { print(1) } else { 0 }",
            "if true { 0 } else { print(1) }",
            "fn unused() { print(1) }",
            "for x in [] { scope.operations() }",
            "for x in scope.operations() { x }",
            "let x = {a: [print(1)]}",
            "len([scope.operations()])",
            "[1][print(0)]",
            "[print(1)][0]",
            "{a: print(1)}.a",
            "!print(true)",
            "let mut x = 0; x = print(1)",
        ] {
            assert!(
                matches!(
                    admit_snapshot(source),
                    SnapshotAdmission::CompleteExplicitRunRequired
                ),
                "{source}"
            );
        }
    }

    #[test]
    fn eof_expectations_are_incomplete() {
        for source in [
            "let",
            "let x =",
            "1 +",
            "foo.",
            "print(",
            "[1,",
            "{a:",
            "{",
            "if true {",
            "if true {} else",
            "for x in",
            "fn f(",
            "fn f() { 1",
            "\"unfinished",
            "\"escape\\",
            "len(\n[1,\n",
        ] {
            match admit_snapshot(source) {
                SnapshotAdmission::Incomplete(error) => {
                    assert!(error.offset <= source.len());
                    assert!(matches!(error.kind, crate::ErrorKind::Syntax(_)));
                }
                other => panic!("{source:?}: {other:?}"),
            }
        }
    }

    #[test]
    fn malformed_input_is_invalid_not_incomplete() {
        for source in [
            "[)",
            "let x = ;",
            "1 + ;",
            "{a: 1, a: 2}",
            "print()",
            "\"bad\\q",
            "\"line\n",
            "@(",
            "fn f() {}",
            "print(1); ]",
        ] {
            assert!(
                matches!(admit_snapshot(source), SnapshotAdmission::Invalid(_)),
                "{source:?}"
            );
        }
        assert!(matches!(
            admit_snapshot(&"[".repeat(65)),
            SnapshotAdmission::Invalid(_)
        ));
        assert!(matches!(
            admit_snapshot(&" ".repeat(65_537)),
            SnapshotAdmission::Invalid(_)
        ));
        match admit_snapshot("\"é\"; ]") {
            SnapshotAdmission::Invalid(error) => assert_eq!(error.offset, 6),
            other => panic!("{other:?}"),
        }
    }
}
