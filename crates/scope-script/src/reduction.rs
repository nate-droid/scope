//! Experimental, whole-snapshot reduction. Not valid for persistent interpreter sessions.
use crate::{
    Value,
    parser::{self, Expr, Kind, Statement},
    runner::Snapshot,
};
use std::ops::Range;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Replacement {
    pub range: Range<usize>,
    pub text: String,
}

#[derive(Debug)]
pub struct Candidate {
    pub original: Snapshot,
    pub text: String,
    pub edits: Vec<Replacement>,
    pub rule: &'static str,
    pub checked: &'static [&'static str],
}

#[derive(Debug, PartialEq, Eq)]
pub enum Refusal {
    InvalidSource,
    Unsupported,
}

// AST offsets alone do not delimit expressions. Only accept the exact decimal
// spelling at that offset; parenthesized literals must not leave dangling syntax.
fn decimal(source: &str, expr: &Expr) -> Option<Range<usize>> {
    if !matches!(expr.kind, Kind::Literal(Value::Integer(_))) {
        return None;
    }
    let start = expr.offset;
    let len = source[start..]
        .bytes()
        .take_while(u8::is_ascii_digit)
        .count();
    (len > 0).then_some(start..start + len)
}

/// Propose one rule: a single mutable decimal-integer binding, one or more
/// consecutive same-binding decimal assignments, then a final binding read.
/// Requires fresh execution; no execution, host calls, or source mutation occurs.
pub fn reduce(snapshot: &Snapshot) -> Result<Candidate, Refusal> {
    let source = &snapshot.text;
    let statements = parser::parse(source).map_err(|_| Refusal::InvalidSource)?;
    if source.len() > 4096 || !(3..=64).contains(&statements.len()) {
        return Err(Refusal::Unsupported);
    }
    let Statement::Expression {
        binding: Some(name),
        mutable: true,
        expr,
        ..
    } = &statements[0]
    else {
        return Err(Refusal::Unsupported);
    };
    let initial = decimal(source, expr).ok_or(Refusal::Unsupported)?;
    let Some(Statement::Expression {
        binding: None,
        expr: last,
        ..
    }) = statements.last()
    else {
        return Err(Refusal::Unsupported);
    };
    if !matches!(&last.kind, Kind::Name(read) if read == name) {
        return Err(Refusal::Unsupported);
    }
    let mut assignments = Vec::new();
    for statement in &statements[1..statements.len() - 1] {
        let Statement::Assign {
            name: assigned,
            expr,
            offset,
        } = statement
        else {
            return Err(Refusal::Unsupported);
        };
        if assigned != name {
            return Err(Refusal::Unsupported);
        }
        let rhs = decimal(source, expr).ok_or(Refusal::Unsupported)?;
        // Only consume an immediately adjacent separator: scanning through trivia
        // could mistake a semicolon in a comment for the assignment terminator.
        // Keep all offsets in the original snapshot for replacement provenance.
        let end = rhs.end + usize::from(source.as_bytes().get(rhs.end) == Some(&b';'));
        assignments.push((*offset..end, rhs));
    }
    let final_rhs = &assignments.last().unwrap().1;
    let mut edits = vec![Replacement {
        range: initial,
        text: source[final_rhs.clone()].to_owned(),
    }];
    edits.extend(assignments.into_iter().map(|(range, _)| Replacement {
        range,
        text: String::new(),
    }));
    let mut text = source.clone();
    for edit in edits.iter().rev() {
        text.replace_range(edit.range.clone(), &edit.text);
    }
    // Existing parser is the independent syntax guard for incomplete end spans
    // (e.g. parentheses). Never publish malformed byte-preserving replacements.
    let reduced = parser::parse(&text).map_err(|_| Refusal::Unsupported)?;
    if reduced.len() != 2 {
        return Err(Refusal::Unsupported);
    }
    Ok(Candidate {
        original: snapshot.clone(),
        text,
        edits,
        rule: "consecutive decimal-integer assignment folding",
        checked: &[
            "whole fresh program: one mutable binding, consecutive assignments, final read",
            "all initializers are decimal integer literals of the same runtime kind",
            "no intermediate reads, calls, branches, loops, functions, or output",
            "at most 4096 source bytes and 64 statements: below runtime budgets",
            "candidate reparses as two statements; unaffected bytes retained",
        ],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Interpreter, host::NoHost};
    use scope_language::{RevisionId, SourceId};

    fn snapshot(text: &str) -> Snapshot {
        Snapshot {
            identity: SourceId::fresh(),
            revision: RevisionId::from_version(3),
            text: text.into(),
        }
    }
    fn observe(text: &str) -> (Value, Vec<Value>) {
        let mut interpreter = Interpreter::new();
        let value = interpreter.eval(text, &mut NoHost).unwrap();
        (value, interpreter.take_output())
    }
    #[test]
    fn generated_chains_match_independent_execution() {
        for a in [0, 6, 99, i64::MAX] {
            for b in [0, 8, 12, i64::MAX] {
                let original = snapshot(&format!("let mut n = {a}; n = {b}; n = 12; n"));
                let candidate = reduce(&original).unwrap();
                assert_eq!(observe(&original.text), observe(&candidate.text));
                assert!(reduce(&snapshot(&candidate.text)).is_err());
                assert_eq!(candidate.original.text, original.text);
            }
        }
    }
    #[test]
    fn ranges_and_trivia() {
        let original = snapshot("# λ\nlet mut n = 6; # n = 999\nn = 12; n");
        let candidate = reduce(&original).unwrap();
        assert_eq!(candidate.text, "# λ\nlet mut n = 12; # n = 999\n n");
        assert_eq!(&original.text[candidate.edits[0].range.clone()], "6");
        assert_eq!(&original.text[candidate.edits[1].range.clone()], "n = 12;");
        assert_eq!(observe(&original.text), observe(&candidate.text));
    }
    #[test]
    fn assignment_separators_preserve_trivia_and_provenance() {
        for (source, expected, removed) in [
            ("let mut n = 1;n = 22;n", "let mut n = 22;n", "n = 22;"),
            (
                "let mut n = 1; n = 22;; n",
                "let mut n = 22; ; n",
                "n = 22;",
            ),
            (
                "let mut n = 1; n = 22 \t; n",
                "let mut n = 22;  \t; n",
                "n = 22",
            ),
            (
                "let mut n = 1; n = 22 # λ; comment\nn",
                "let mut n = 22;  # λ; comment\nn",
                "n = 22",
            ),
            (
                "let mut n = 1; n = 22; # λ; comment\nn",
                "let mut n = 22;  # λ; comment\nn",
                "n = 22;",
            ),
            (
                "let mut n = 1; n = 22\r\nn",
                "let mut n = 22; \r\nn",
                "n = 22",
            ),
        ] {
            let original = snapshot(source);
            let candidate = reduce(&original).unwrap();
            assert_eq!(candidate.text, expected, "{source}");
            assert_eq!(&source[candidate.edits[1].range.clone()], removed);
            assert_eq!(candidate.original.identity, original.identity);
            assert_eq!(candidate.original.revision, original.revision);
            assert_eq!(candidate.original.text, original.text);
            let mut replay = source.to_owned();
            for edit in candidate.edits.iter().rev() {
                replay.replace_range(edit.range.clone(), &edit.text);
            }
            assert_eq!(replay, candidate.text);
            assert_eq!(observe(source), observe(&candidate.text));
        }
    }

    #[test]
    fn counterexamples_are_observable() {
        assert_eq!(
            observe("let mut n = 6; let previous = n; n = 8; previous + n").0,
            Value::Integer(14)
        );
        assert_eq!(
            observe("let mut n = 8; let previous = n; previous + n").0,
            Value::Integer(16)
        );
        assert_eq!(
            observe("let mut n = 6; print(n); n = 8; print(n)").1,
            vec![Value::Integer(6), Value::Integer(8)]
        );
        assert_eq!(
            observe("let mut n = 6; fn old() { n }; n = 8; old()").0,
            Value::Integer(6)
        );
        for source in [
            "let mut n = 6; n = 1 / 0; n = 8; n",
            "let mut n = 6; n = true; n = 8; n",
            "let mut n = 6; if true { let n = 8 }; n",
        ] {
            let mut interpreter = Interpreter::new();
            assert!(interpreter.eval(source, &mut NoHost).is_err());
            assert!(reduce(&snapshot(source)).is_err());
        }
    }

    #[test]
    fn misleading_and_uncertain_programs_refused() {
        for text in [
            "let mut n = 6; let previous = n; n = 8; previous + n",
            "let mut n = 6; print(n); n = 8; print(n)",
            "let mut n = 6; n = 1 / 0; n = 8; n",
            "let mut n = 6; if true { n = 8 }; n",
            "let mut n = 6; if true { let n = 8 }; n",
            "let mut n = 6; for x in [1] { n = 8 }; n",
            "let mut n = 6; fn old() { n }; n = 8; old()",
            "let mut n = 6; n = scope.operations(); n = 8; n",
            "let mut n = 6; n = true; n = 8; n",
            "let mut n = (6); n = (8); n",
            "let mut n = 6; n = 8; n * 7",
            "let mut n = 6; n = 8",
            "1 / 0; 42",
            "1; 42",
        ] {
            assert!(reduce(&snapshot(text)).is_err(), "{text}");
        }
    }
}
