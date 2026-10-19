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
    Eof,
}
#[derive(Debug)]
pub(crate) struct Expr {
    pub offset: usize,
    depth: usize,
    pub kind: Kind,
}
#[derive(Debug)]
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
pub(crate) struct Statement {
    pub binding: Option<String>,
    pub expr: Expr,
    pub offset: usize,
}
fn syntax(offset: usize, message: impl Into<String>) -> Error {
    Error {
        offset,
        kind: ErrorKind::Syntax(message.into()),
    }
}

pub(crate) fn parse(source: &str) -> Result<Vec<Statement>, Error> {
    if source.len() > MAX_SOURCE {
        return Err(syntax(0, "source size limit exceeded"));
    }
    let mut tokens = Vec::new();
    let mut chars = source.char_indices().peekable();
    let mut nesting = 0usize;
    while let Some((offset, c)) = chars.next() {
        let token = match c {
            ' ' | '\t' | '\r' => continue,
            '\n' if nesting != 0 => continue,
            '\n' | ';' => Token::End,
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
                            let (_, escaped) = chars
                                .next()
                                .ok_or_else(|| syntax(at, "unfinished escape"))?;
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
    };
    let mut statements = Vec::new();
    while parser.token() != &Token::Eof {
        if parser.eat(&Token::End) {
            continue;
        }
        let offset = parser.offset();
        let binding = if parser.eat(&Token::Name("let".into())) {
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
        statements.push(Statement {
            binding,
            expr,
            offset,
        });
        if parser.token() != &Token::Eof {
            parser.expect(Token::End)?;
        }
    }
    Ok(statements)
}
struct Parser {
    tokens: Vec<(usize, Token)>,
    pos: usize,
    recursion: usize,
}
impl Parser {
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
            Err(syntax(self.offset(), format!("expected {token:?}")))
        }
    }
    fn name(&mut self) -> Result<String, Error> {
        if let Token::Name(name) = self.token().clone() {
            self.pos += 1;
            Ok(name)
        } else {
            Err(syntax(self.offset(), "expected name"))
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
                            _ => return Err(syntax(at, "expected object key")),
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
            } else if self.eat(&Token::Symbol('[')) {
                let index = self.expr(0)?;
                self.expect(Token::Symbol(']'))?;
                left = self.node(
                    offset,
                    left.depth.max(index.depth) + 1,
                    Kind::Index(Box::new(left), Box::new(index)),
                )?;
            } else if self.eat(&Token::Symbol('(')) {
                let name = match &left.kind {
                    Kind::Name(n) if n == "print" => n.clone(),
                    Kind::Field(base, n)
                        if matches!(&base.kind, Kind::Name(s) if s == "scope")
                            && matches!(n.as_str(), "operations" | "invoke") =>
                    {
                        format!("scope.{n}")
                    }
                    _ => return Err(syntax(offset, "only builtin calls are supported")),
                };
                let args = self.arguments(')')?;
                let expected = match name.as_str() {
                    "print" => 1,
                    "scope.operations" => 0,
                    _ => 3,
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
