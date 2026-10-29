use crate::{Error, ErrorKind, Value};
use std::collections::BTreeMap;

pub(crate) const MAX_DEPTH: usize = 64;
const MAX_SOURCE: usize = 65_536;

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Name(String),
    Int(String),
    Str(String),
    Symbol(char),
    Op(String),
    End,
    Newline,
    Eof,
}
#[derive(Debug, Clone)]
pub(crate) struct Expr {
    pub offset: usize,
    depth: usize,
    pub kind: Kind,
}
#[derive(Debug, Clone)]
pub(crate) enum Kind {
    Literal(Value),
    Name(String),
    Array(Vec<Expr>),
    Object(Vec<(String, Expr)>),
    Unary(String, Box<Expr>),
    Binary(String, Box<Expr>, Box<Expr>),
    Index(Box<Expr>, Box<Expr>),
    Field(Box<Expr>, String),
    Call(String, Vec<Expr>),
}
#[derive(Debug, Clone)]
pub(crate) enum Statement {
    For {
        name: String,
        collection: Expr,
        body: Vec<Statement>,
        offset: usize,
    },
    Function {
        name: String,
        params: Vec<String>,
        body: Vec<Statement>,
        offset: usize,
    },
    Assign {
        name: String,
        expr: Expr,
        offset: usize,
    },
    Expression {
        binding: Option<String>,
        mutable: bool,
        expr: Expr,
        offset: usize,
    },
    If {
        condition: Expr,
        yes: Vec<Statement>,
        no: Vec<Statement>,
    },
}
fn syntax(offset: usize, message: impl Into<String>) -> Error {
    Error {
        offset,
        kind: ErrorKind::Syntax(message.into()),
    }
}

pub(crate) fn parse(source: &str) -> Result<Vec<Statement>, Error> {
    parse_snapshot(source).0
}

/// The flag records an actual lexer/parser expectation at EOF, not delimiter balance.
pub(crate) fn parse_snapshot(source: &str) -> (Result<Vec<Statement>, Error>, bool) {
    let mut incomplete = false;
    let result = parse_inner(source, &mut incomplete);
    (result, incomplete)
}

fn parse_inner(source: &str, incomplete: &mut bool) -> Result<Vec<Statement>, Error> {
    if source.len() > MAX_SOURCE {
        return Err(syntax(0, "source size limit exceeded"));
    }
    let mut tokens = Vec::new();
    let mut chars = source.char_indices().peekable();
    let mut nesting = 0usize;
    while let Some((offset, c)) = chars.next() {
        let token = match c {
            ' ' | '\t' | '\r' => continue,
            '#' => {
                while chars.peek().is_some_and(|(_, c)| *c != '\n') {
                    chars.next();
                }
                continue;
            }
            '\n' => Token::Newline,
            ';' => Token::End,
            'a'..='z' | 'A'..='Z' | '_' => {
                let mut name = String::from(c);
                while chars
                    .peek()
                    .is_some_and(|(_, c)| c.is_ascii_alphanumeric() || *c == '_')
                {
                    name.push(chars.next().unwrap().1);
                }
                Token::Name(name)
            }
            '0'..='9' => {
                let mut n = String::from(c);
                while chars.peek().is_some_and(|(_, c)| c.is_ascii_digit()) {
                    n.push(chars.next().unwrap().1);
                }
                Token::Int(n)
            }
            '"' => {
                let mut text = String::new();
                let mut closed = false;
                while let Some((at, c)) = chars.next() {
                    match c {
                        '"' => {
                            closed = true;
                            break;
                        }
                        '\\' => {
                            let (_, escaped) = chars.next().ok_or_else(|| {
                                *incomplete = true;
                                syntax(at, "unfinished escape")
                            })?;
                            text.push(match escaped {
                                'n' => '\n',
                                'r' => '\r',
                                't' => '\t',
                                '"' => '"',
                                '\\' => '\\',
                                _ => return Err(syntax(at, "unsupported string escape")),
                            });
                        }
                        c if c.is_control() => {
                            return Err(syntax(at, "control character in string"));
                        }
                        c => text.push(c),
                    }
                }
                if !closed {
                    *incomplete = true;
                    return Err(syntax(offset, "unterminated string"));
                }
                Token::Str(text)
            }
            '(' | '[' | '{' => {
                nesting += 1;
                if nesting > MAX_DEPTH {
                    return Err(syntax(offset, "nesting limit exceeded"));
                }
                Token::Symbol(c)
            }
            ')' | ']' | '}' => {
                nesting = nesting.saturating_sub(1);
                Token::Symbol(c)
            }
            ',' | ':' | '.' => Token::Symbol(c),
            '+' | '-' | '*' | '/' | '%' | '=' | '!' | '<' | '>' | '&' | '|' => {
                let mut op = String::from(c);
                if chars.peek().is_some_and(|(_, next)| {
                    (*next == '=' && matches!(c, '=' | '!' | '<' | '>'))
                        || (*next == c && matches!(c, '&' | '|'))
                }) {
                    op.push(chars.next().unwrap().1);
                }
                Token::Op(op)
            }
            _ => return Err(syntax(offset, "unexpected character")),
        };
        tokens.push((offset, token));
    }
    tokens.push((source.len(), Token::Eof));
    let mut parser = Parser {
        tokens,
        pos: 0,
        recursion: 0,
        incomplete,
    };
    parser.statements(false, 0)
}
impl Parser<'_> {
    fn separators(&mut self) {
        while self.eat(&Token::End) || self.eat(&Token::Newline) {}
    }
    fn statements(&mut self, block: bool, depth: usize) -> Result<Vec<Statement>, Error> {
        if depth > MAX_DEPTH {
            return Err(syntax(self.offset(), "statement nesting limit exceeded"));
        }
        let parser = self;
        let mut statements = Vec::new();
        parser.separators();
        while parser.token() != &Token::Eof && !(block && parser.token() == &Token::Symbol('}')) {
            let offset = parser.offset();
            if parser.eat(&Token::Name("for".into())) {
                let name = parser.name()?;
                if crate::reserved(&name) {
                    return Err(syntax(offset, "reserved loop binding"));
                }
                parser.expect(Token::Name("in".into()))?;
                let collection = parser.expr(0)?;
                parser.expect(Token::Symbol('{'))?;
                let body = parser.statements(true, depth + 1)?;
                parser.expect(Token::Symbol('}'))?;
                statements.push(Statement::For {
                    name,
                    collection,
                    body,
                    offset,
                });
            } else if parser.eat(&Token::Name("fn".into())) {
                if block {
                    return Err(syntax(offset, "functions must be declared at top level"));
                }
                let name = parser.name()?;
                if crate::reserved(&name)
                    || matches!(
                        name.as_str(),
                        "len"
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
                    )
                {
                    return Err(syntax(offset, "reserved function name"));
                }
                parser.expect(Token::Symbol('('))?;
                let mut params = Vec::new();
                if !parser.eat(&Token::Symbol(')')) {
                    loop {
                        let param = parser.name()?;
                        if crate::reserved(&param) {
                            return Err(syntax(offset, "reserved parameter name"));
                        }
                        params.push(param);
                        if parser.eat(&Token::Symbol(')')) {
                            break;
                        }
                        parser.expect(Token::Symbol(','))?;
                    }
                }
                parser.expect(Token::Symbol('{'))?;
                let body = parser.statements(true, depth + 1)?;
                parser.expect(Token::Symbol('}'))?;
                if !matches!(
                    body.last(),
                    Some(Statement::Expression { binding: None, .. })
                ) {
                    return Err(syntax(offset, "function requires a final expression"));
                }
                statements.push(Statement::Function {
                    name,
                    params,
                    body,
                    offset,
                });
            } else if matches!(parser.token(), Token::Name(_))
                && parser
                    .tokens
                    .get(parser.pos + 1)
                    .is_some_and(|(_, t)| t == &Token::Op("=".into()))
            {
                let name = parser.name()?;
                parser.expect(Token::Op("=".into()))?;
                let expr = parser.expr(0)?;
                statements.push(Statement::Assign { name, expr, offset });
            } else if parser.eat(&Token::Name("if".into())) {
                let condition = parser.expr(0)?;
                parser.expect(Token::Symbol('{'))?;
                let yes = parser.statements(true, depth + 1)?;
                parser.expect(Token::Symbol('}'))?;
                let saved = parser.pos;
                while parser.eat(&Token::Newline) {}
                let no = if parser.eat(&Token::Name("else".into())) {
                    parser.expect(Token::Symbol('{'))?;
                    let body = parser.statements(true, depth + 1)?;
                    parser.expect(Token::Symbol('}'))?;
                    body
                } else {
                    parser.pos = saved;
                    Vec::new()
                };
                statements.push(Statement::If { condition, yes, no });
            } else {
                let offset = parser.offset();
                let mut mutable = false;
                let binding = if parser.eat(&Token::Name("let".into())) {
                    mutable = parser.eat(&Token::Name("mut".into()));
                    let name = parser.name()?;
                    if crate::reserved(&name) {
                        return Err(syntax(offset, "reserved binding name"));
                    }
                    parser.expect(Token::Op("=".into()))?;
                    Some(name)
                } else {
                    None
                };
                let expr = parser.expr(0)?;
                statements.push(Statement::Expression {
                    binding,
                    mutable,
                    expr,
                    offset,
                });
            }
            if parser.token() != &Token::Eof
                && !(block && parser.token() == &Token::Symbol('}'))
                && !parser.eat(&Token::Newline)
            {
                parser.expect(Token::End)?;
            }
            parser.separators();
        }
        Ok(statements)
    }
}
struct Parser<'a> {
    tokens: Vec<(usize, Token)>,
    pos: usize,
    recursion: usize,
    incomplete: &'a mut bool,
}
impl Parser<'_> {
    fn expected(&mut self, message: impl Into<String>) -> Error {
        *self.incomplete |= self.token() == &Token::Eof;
        syntax(self.offset(), message)
    }
    fn token(&self) -> &Token {
        &self.tokens[self.pos].1
    }
    fn offset(&self) -> usize {
        self.tokens[self.pos].0
    }
    fn eat(&mut self, token: &Token) -> bool {
        if self.token() == token {
            self.pos += 1;
            true
        } else {
            false
        }
    }
    fn expect(&mut self, token: Token) -> Result<(), Error> {
        if self.eat(&token) {
            Ok(())
        } else {
            Err(self.expected(format!("expected {token:?}")))
        }
    }
    fn name(&mut self) -> Result<String, Error> {
        if let Token::Name(name) = self.token().clone() {
            self.pos += 1;
            Ok(name)
        } else {
            Err(self.expected("expected name"))
        }
    }
    fn node(&self, offset: usize, depth: usize, kind: Kind) -> Result<Expr, Error> {
        if depth > MAX_DEPTH {
            Err(syntax(offset, "expression depth limit exceeded"))
        } else {
            Ok(Expr {
                offset,
                depth,
                kind,
            })
        }
    }
    fn expr(&mut self, min: u8) -> Result<Expr, Error> {
        self.recursion += 1;
        if self.recursion > MAX_DEPTH {
            return Err(syntax(self.offset(), "expression nesting limit exceeded"));
        }
        let result = self.expression(min);
        self.recursion -= 1;
        result
    }
    fn expression(&mut self, min: u8) -> Result<Expr, Error> {
        let offset = self.offset();
        let token = self.token().clone();
        // Expressions cannot contain statements. Preserve multiline literal/call
        // syntax by removing only newlines within expression delimiters.
        if matches!(token, Token::Symbol('(' | '[' | '{')) {
            self.group_newlines();
        }
        if token == Token::Eof {
            return Err(self.expected("expected expression"));
        }
        self.pos += 1;
        let mut left = match token {
            Token::Int(n) => self.node(
                offset,
                1,
                Kind::Literal(Value::Integer(
                    n.parse()
                        .map_err(|_| syntax(offset, "integer out of range"))?,
                )),
            )?,
            Token::Str(s) => self.node(offset, 1, Kind::Literal(Value::String(s)))?,
            Token::Name(n) => self.node(
                offset,
                1,
                match n.as_str() {
                    "true" => Kind::Literal(Value::Boolean(true)),
                    "false" => Kind::Literal(Value::Boolean(false)),
                    "null" => Kind::Literal(Value::Null),
                    _ => Kind::Name(n),
                },
            )?,
            Token::Op(op) if op == "-" || op == "!" => {
                // Permit the minimum i64 literal without overflowing its positive magnitude.
                if op == "-" && self.token() == &Token::Int("9223372036854775808".into()) {
                    self.pos += 1;
                    self.node(offset, 1, Kind::Literal(Value::Integer(i64::MIN)))?
                } else {
                    let value = self.expr(7)?;
                    self.node(offset, value.depth + 1, Kind::Unary(op, Box::new(value)))?
                }
            }
            Token::Symbol('(') => {
                let e = self.expr(0)?;
                self.expect(Token::Symbol(')'))?;
                e
            }
            Token::Symbol('[') => {
                let items = self.arguments(']')?;
                let depth = items.iter().map(|e| e.depth).max().unwrap_or(0) + 1;
                self.node(offset, depth, Kind::Array(items))?
            }
            Token::Symbol('{') => {
                let mut fields = Vec::new();
                let mut keys = BTreeMap::new();
                if !self.eat(&Token::Symbol('}')) {
                    loop {
                        let at = self.offset();
                        let key = match self.token().clone() {
                            Token::Name(s) | Token::Str(s) => {
                                self.pos += 1;
                                s
                            }
                            _ => return Err(self.expected("expected object key")),
                        };
                        if keys.insert(key.clone(), ()).is_some() {
                            return Err(syntax(at, "duplicate object key"));
                        }
                        self.expect(Token::Symbol(':'))?;
                        fields.push((key, self.expr(0)?));
                        if self.eat(&Token::Symbol('}')) {
                            break;
                        }
                        self.expect(Token::Symbol(','))?;
                        if self.eat(&Token::Symbol('}')) {
                            break;
                        }
                    }
                }
                let depth = fields.iter().map(|(_, e)| e.depth).max().unwrap_or(0) + 1;
                self.node(offset, depth, Kind::Object(fields))?
            }
            _ => return Err(syntax(offset, "expected expression")),
        };
        loop {
            if self.eat(&Token::Symbol('.')) {
                let name = self.name()?;
                left = self.node(offset, left.depth + 1, Kind::Field(Box::new(left), name))?;
            } else if self.token() == &Token::Symbol('[') {
                self.group_newlines();
                self.pos += 1;
                let index = self.expr(0)?;
                self.expect(Token::Symbol(']'))?;
                left = self.node(
                    offset,
                    left.depth.max(index.depth) + 1,
                    Kind::Index(Box::new(left), Box::new(index)),
                )?;
            } else if self.token() == &Token::Symbol('(') {
                self.group_newlines();
                self.pos += 1;
                let name = match &left.kind {
                    Kind::Name(n) => n.clone(),
                    Kind::Field(base, n)
                        if matches!(&base.kind, Kind::Name(s) if s == "scope")
                            && matches!(n.as_str(), "operations" | "invoke") =>
                    {
                        format!("scope.{n}")
                    }
                    _ => return Err(syntax(offset, "only named calls are supported")),
                };
                let args = self.arguments(')')?;
                let expected = match name.as_str() {
                    "print" | "len" | "keys" | "parse_int" | "trim" | "lower" | "upper"
                    | "reverse" | "abs" | "format" => 1,
                    "contains" | "join" | "split" | "starts_with" | "ends_with" | "append"
                    | "get" | "range" | "min" | "max" => 2,
                    "replace" | "slice" => 3,
                    "scope.operations" => 0,
                    "scope.invoke" => 3,
                    _ => args.len(),
                };
                if args.len() != expected {
                    return Err(syntax(offset, "wrong number of arguments"));
                }
                let depth = args.iter().map(|e| e.depth).max().unwrap_or(0) + 1;
                left = self.node(offset, depth, Kind::Call(name, args))?;
            } else {
                let Token::Op(op) = self.token() else { break };
                let precedence = match op.as_str() {
                    "||" => 1,
                    "&&" => 2,
                    "==" | "!=" => 3,
                    "<" | "<=" | ">" | ">=" => 4,
                    "+" | "-" => 5,
                    "*" | "/" | "%" => 6,
                    _ => break,
                };
                if precedence < min {
                    break;
                }
                let op = op.clone();
                self.pos += 1;
                let right = self.expr(precedence + 1)?;
                left = self.node(
                    offset,
                    left.depth.max(right.depth) + 1,
                    Kind::Binary(op, Box::new(left), Box::new(right)),
                )?;
            }
        }
        Ok(left)
    }
    fn group_newlines(&mut self) {
        let mut depth = 0usize;
        let mut end = self.pos;
        for (_, token) in &self.tokens[self.pos..] {
            match token {
                Token::Symbol('(' | '[' | '{') => depth += 1,
                Token::Symbol(')' | ']' | '}') => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
            end += 1;
        }
        let mut index = 0;
        self.tokens.retain(|(_, token)| {
            let keep = index < self.pos || index >= end || *token != Token::Newline;
            index += 1;
            keep
        });
    }
    fn arguments(&mut self, end: char) -> Result<Vec<Expr>, Error> {
        let mut args = Vec::new();
        if self.eat(&Token::Symbol(end)) {
            return Ok(args);
        }
        loop {
            args.push(self.expr(0)?);
            if self.eat(&Token::Symbol(end)) {
                break;
            }
            self.expect(Token::Symbol(','))?;
            if self.eat(&Token::Symbol(end)) {
                break;
            }
        }
        Ok(args)
    }
}
