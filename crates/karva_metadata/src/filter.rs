use std::fmt;

use globset::{Glob, GlobMatcher};
use regex::Regex;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use thiserror::Error;

/// How the body of a predicate should be compared against the value it's
/// evaluated over (a test name or a tag name).
#[derive(Debug, Clone)]
pub enum Matcher {
    /// The value must equal the pattern exactly.
    Exact(String),
    /// The pattern must appear anywhere in the value.
    Substring(String),
    /// The value must match the compiled regular expression.
    Regex(Regex),
    /// The value must match the compiled glob pattern.
    Glob(GlobMatcher),
}

impl Matcher {
    fn matches(&self, value: &str) -> bool {
        match self {
            Self::Exact(pattern) => value == pattern,
            Self::Substring(pattern) => value.contains(pattern.as_str()),
            Self::Regex(regex) => regex.is_match(value),
            Self::Glob(glob) => glob.is_match(value),
        }
    }
}

/// A single predicate in the filter DSL, e.g. `test(~login)` or `tag(slow)`.
#[derive(Debug, Clone)]
pub enum Predicate {
    /// Evaluated against the fully qualified test name.
    Test(Matcher),
    /// Evaluated against each custom tag on the test; matches if any tag matches.
    Tag(Matcher),
}

/// The value a [`Filterset`] is evaluated against.
#[derive(Debug, Clone, Copy)]
pub struct EvalContext<'a> {
    pub test_name: &'a str,
    pub tags: &'a [&'a str],
}

#[derive(Debug, Clone)]
enum Expr {
    Predicate(Predicate),
    Not(Box<Self>),
    And(Box<Self>, Box<Self>),
    Or(Box<Self>, Box<Self>),
}

impl Expr {
    fn matches(&self, ctx: &EvalContext<'_>) -> bool {
        match self {
            Self::Predicate(Predicate::Test(matcher)) => matcher.matches(ctx.test_name),
            Self::Predicate(Predicate::Tag(matcher)) => {
                ctx.tags.iter().any(|tag| matcher.matches(tag))
            }
            Self::Not(inner) => !inner.matches(ctx),
            Self::And(lhs, rhs) => lhs.matches(ctx) && rhs.matches(ctx),
            Self::Or(lhs, rhs) => lhs.matches(ctx) || rhs.matches(ctx),
        }
    }
}

/// A parsed filterset expression that can be evaluated against a test.
#[derive(Debug, Clone)]
pub struct Filterset {
    expr: Expr,
}

impl Filterset {
    pub fn new(input: &str) -> Result<Self, FilterError> {
        let tokens = tokenize(input)?;
        let mut parser = Parser::new(&tokens, input);
        let expr = parser.parse_or()?;
        if parser.pos < parser.tokens.len() {
            return Err(FilterError::UnexpectedToken {
                token: parser.tokens[parser.pos].to_string(),
                expression: input.to_string(),
            });
        }
        Ok(Self { expr })
    }

    pub fn matches(&self, ctx: &EvalContext<'_>) -> bool {
        self.expr.matches(ctx)
    }
}

/// A filter expression that has been validated at construction time.
///
/// Wraps the raw string and a compiled [`Filterset`] so callers can
/// evaluate the filter without re-parsing or risking a panic. Equality
/// is defined on the raw string so structural comparisons (used by the
/// `Options` derive) remain meaningful.
#[derive(Debug, Clone)]
pub struct ValidatedFilter {
    raw: String,
    compiled: Filterset,
}

impl ValidatedFilter {
    pub fn new(raw: String) -> Result<Self, FilterError> {
        let compiled = Filterset::new(&raw)?;
        Ok(Self { raw, compiled })
    }

    pub fn as_str(&self) -> &str {
        &self.raw
    }

    pub fn matches(&self, ctx: &EvalContext<'_>) -> bool {
        self.compiled.matches(ctx)
    }

    pub fn filterset(&self) -> &Filterset {
        &self.compiled
    }
}

impl PartialEq for ValidatedFilter {
    fn eq(&self, other: &Self) -> bool {
        self.raw == other.raw
    }
}

impl Eq for ValidatedFilter {}

impl Serialize for ValidatedFilter {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.raw.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ValidatedFilter {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        Self::new(raw).map_err(serde::de::Error::custom)
    }
}

/// A set of filterset expressions combined with OR semantics (matches if any
/// filter matches). An empty set matches everything.
#[derive(Debug, Clone, Default)]
pub struct FiltersetSet {
    filters: Vec<Filterset>,
}

impl FiltersetSet {
    pub fn new(expressions: &[String]) -> Result<Self, FilterError> {
        let filters = expressions
            .iter()
            .map(|expr| Filterset::new(expr))
            .collect::<Result<_, _>>()?;
        Ok(Self { filters })
    }

    pub fn is_empty(&self) -> bool {
        self.filters.is_empty()
    }

    pub fn matches(&self, ctx: &EvalContext<'_>) -> bool {
        self.filters.is_empty() || self.filters.iter().any(|f| f.matches(ctx))
    }
}

#[derive(Debug, Error)]
pub enum FilterError {
    #[error("unexpected character `{character}` in filter expression `{expression}`")]
    UnexpectedCharacter { character: char, expression: String },
    #[error("empty filter expression `{expression}`")]
    EmptyExpression { expression: String },
    #[error("expected closing `)` in filter expression `{expression}`")]
    UnclosedParenthesis { expression: String },
    #[error("unterminated regex literal in filter expression `{expression}`")]
    UnclosedRegex { expression: String },
    #[error("unterminated quoted string in filter expression `{expression}`")]
    UnclosedString { expression: String },
    #[error("unexpected token `{token}` in filter expression `{expression}`")]
    UnexpectedToken { token: String, expression: String },
    #[error("unexpected end of filter expression `{expression}`")]
    UnexpectedEndOfExpression { expression: String },
    #[error("invalid regex `/{pattern}/` in filter expression `{expression}`: {error}")]
    InvalidRegex {
        pattern: String,
        error: regex::Error,
        expression: String,
    },
    #[error("invalid glob `#{pattern}` in filter expression `{expression}`: {error}")]
    InvalidGlob {
        pattern: String,
        error: globset::Error,
        expression: String,
    },
    #[error(
        "unknown predicate `{name}` in filter expression `{expression}` (expected `test` or `tag`)"
    )]
    UnknownPredicate { name: String, expression: String },
    #[error("expected `(` after predicate in filter expression `{expression}`")]
    ExpectedPredicateOpenParen { expression: String },
    #[error("expected a matcher body in filter expression `{expression}`")]
    ExpectedMatcher { expression: String },
}

#[derive(Debug, Clone, Copy)]
enum PredicateKind {
    Test,
    Tag,
}

#[derive(Debug, Eq, PartialEq)]
enum Token {
    Ident(String),
    String(String),
    Regex(String),
    Equals,
    Tilde,
    Hash,
    And,
    Or,
    Not,
    Minus,
    LParen,
    RParen,
}

impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ident(s) => write!(f, "{s}"),
            Self::String(s) => write!(f, "\"{s}\""),
            Self::Regex(s) => write!(f, "/{s}/"),
            Self::Equals => write!(f, "="),
            Self::Tilde => write!(f, "~"),
            Self::Hash => write!(f, "#"),
            Self::And => write!(f, "&"),
            Self::Or => write!(f, "|"),
            Self::Not => write!(f, "not"),
            Self::Minus => write!(f, "-"),
            Self::LParen => write!(f, "("),
            Self::RParen => write!(f, ")"),
        }
    }
}

fn tokenize(input: &str) -> Result<Vec<Token>, FilterError> {
    let mut tokens = Vec::new();
    let mut chars = input.chars().peekable();

    while let Some(&ch) = chars.peek() {
        if ch.is_whitespace() {
            chars.next();
            continue;
        }

        match ch {
            '(' => {
                tokens.push(Token::LParen);
                chars.next();
            }
            ')' => {
                tokens.push(Token::RParen);
                chars.next();
            }
            '&' => {
                tokens.push(Token::And);
                chars.next();
            }
            '|' => {
                tokens.push(Token::Or);
                chars.next();
            }
            '!' => {
                tokens.push(Token::Not);
                chars.next();
            }
            '-' => {
                tokens.push(Token::Minus);
                chars.next();
            }
            '=' => {
                tokens.push(Token::Equals);
                chars.next();
            }
            '~' => {
                tokens.push(Token::Tilde);
                chars.next();
            }
            '#' => {
                tokens.push(Token::Hash);
                chars.next();
            }
            '/' => {
                chars.next();
                let body = consume_delimited(&mut chars, '/').ok_or_else(|| {
                    FilterError::UnclosedRegex {
                        expression: input.to_string(),
                    }
                })?;
                tokens.push(Token::Regex(body));
            }
            '"' => {
                chars.next();
                let body = consume_delimited(&mut chars, '"').ok_or_else(|| {
                    FilterError::UnclosedString {
                        expression: input.to_string(),
                    }
                })?;
                tokens.push(Token::String(body));
            }
            c if is_ident_char(c) => {
                let mut ident = String::new();
                while let Some(&c) = chars.peek() {
                    if is_ident_char(c) {
                        ident.push(c);
                        chars.next();
                    } else {
                        break;
                    }
                }
                match ident.as_str() {
                    "and" => tokens.push(Token::And),
                    "or" => tokens.push(Token::Or),
                    "not" => tokens.push(Token::Not),
                    _ => tokens.push(Token::Ident(ident)),
                }
            }
            _ => {
                return Err(FilterError::UnexpectedCharacter {
                    character: ch,
                    expression: input.to_string(),
                });
            }
        }
    }

    if tokens.is_empty() {
        return Err(FilterError::EmptyExpression {
            expression: input.to_string(),
        });
    }

    Ok(tokens)
}

/// Bare matcher bodies are allowed to contain glob and regex metacharacters
/// (`*`, `?`, `[`, `]`, `{`, `}`, `^`, `$`) so that expressions like
/// `tag(#py3*)` or `test(#[abc])` lex as a single identifier token. Removing
/// any of these would force users to quote the body.
fn is_ident_char(c: char) -> bool {
    c.is_alphanumeric()
        || matches!(
            c,
            '_' | '.' | ':' | '*' | '?' | '[' | ']' | '{' | '}' | '^' | '$'
        )
}

/// Consumes characters from `chars` up to and including the next occurrence
/// of `delim`, treating only `\<delim>` as an escape (other backslashes are
/// preserved literally so e.g. regex metacharacters like `\d` round-trip).
/// Returns the accumulated body, or `None` if the iterator is exhausted
/// before `delim` is found.
fn consume_delimited(
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
    delim: char,
) -> Option<String> {
    let mut body = String::new();
    loop {
        match chars.next() {
            Some('\\') => match chars.peek() {
                Some(&c) if c == delim => {
                    body.push(delim);
                    chars.next();
                }
                _ => body.push('\\'),
            },
            Some(c) if c == delim => return Some(body),
            Some(c) => body.push(c),
            None => return None,
        }
    }
}

struct Parser<'a> {
    tokens: &'a [Token],
    input: &'a str,
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(tokens: &'a [Token], input: &'a str) -> Self {
        Self {
            tokens,
            input,
            pos: 0,
        }
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn advance(&mut self) {
        self.pos += 1;
    }

    fn expr_str(&self) -> String {
        self.input.to_string()
    }

    fn parse_or(&mut self) -> Result<Expr, FilterError> {
        let mut left = self.parse_and()?;
        while self.peek() == Some(&Token::Or) {
            self.advance();
            let right = self.parse_and()?;
            left = Expr::Or(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_and(&mut self) -> Result<Expr, FilterError> {
        let mut left = self.parse_unary()?;
        loop {
            match self.peek() {
                Some(Token::And) => {
                    self.advance();
                    let right = self.parse_unary()?;
                    left = Expr::And(Box::new(left), Box::new(right));
                }
                Some(Token::Minus) => {
                    self.advance();
                    let right = self.parse_unary()?;
                    left = Expr::And(Box::new(left), Box::new(Expr::Not(Box::new(right))));
                }
                _ => break,
            }
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Expr, FilterError> {
        if self.peek() == Some(&Token::Not) {
            self.advance();
            let inner = self.parse_unary()?;
            return Ok(Expr::Not(Box::new(inner)));
        }
        self.parse_atom()
    }

    fn parse_atom(&mut self) -> Result<Expr, FilterError> {
        match self.peek() {
            Some(Token::LParen) => {
                self.advance();
                let expr = self.parse_or()?;
                if self.peek() != Some(&Token::RParen) {
                    return Err(FilterError::UnclosedParenthesis {
                        expression: self.expr_str(),
                    });
                }
                self.advance();
                Ok(expr)
            }
            Some(Token::Ident(name)) => {
                let name = name.clone();
                let kind = match name.as_str() {
                    "test" => PredicateKind::Test,
                    "tag" => PredicateKind::Tag,
                    _ => {
                        return Err(FilterError::UnknownPredicate {
                            name,
                            expression: self.expr_str(),
                        });
                    }
                };
                self.advance();
                if self.peek() != Some(&Token::LParen) {
                    return Err(FilterError::ExpectedPredicateOpenParen {
                        expression: self.expr_str(),
                    });
                }
                self.advance();
                let matcher = self.parse_matcher(kind)?;
                if self.peek() != Some(&Token::RParen) {
                    return Err(FilterError::UnclosedParenthesis {
                        expression: self.expr_str(),
                    });
                }
                self.advance();
                let predicate = match kind {
                    PredicateKind::Test => Predicate::Test(matcher),
                    PredicateKind::Tag => Predicate::Tag(matcher),
                };
                Ok(Expr::Predicate(predicate))
            }
            Some(token) => Err(FilterError::UnexpectedToken {
                token: token.to_string(),
                expression: self.expr_str(),
            }),
            None => Err(FilterError::UnexpectedEndOfExpression {
                expression: self.expr_str(),
            }),
        }
    }

    fn parse_matcher(&mut self, kind: PredicateKind) -> Result<Matcher, FilterError> {
        match self.peek() {
            Some(Token::Regex(pattern)) => {
                let pattern = pattern.clone();
                self.advance();
                match Regex::new(&pattern) {
                    Ok(regex) => Ok(Matcher::Regex(regex)),
                    Err(error) => Err(FilterError::InvalidRegex {
                        pattern,
                        error,
                        expression: self.expr_str(),
                    }),
                }
            }
            Some(Token::Equals) => {
                self.advance();
                let body = self.parse_matcher_body()?;
                Ok(Matcher::Exact(body))
            }
            Some(Token::Tilde) => {
                self.advance();
                let body = self.parse_matcher_body()?;
                Ok(Matcher::Substring(body))
            }
            Some(Token::Hash) => {
                self.advance();
                let body = self.parse_matcher_body()?;
                match Glob::new(&body) {
                    Ok(glob) => Ok(Matcher::Glob(glob.compile_matcher())),
                    Err(error) => Err(FilterError::InvalidGlob {
                        pattern: body,
                        error,
                        expression: self.expr_str(),
                    }),
                }
            }
            Some(Token::Ident(_) | Token::String(_)) => {
                let body = self.parse_matcher_body()?;
                Ok(match kind {
                    PredicateKind::Test => Matcher::Substring(body),
                    PredicateKind::Tag => Matcher::Exact(body),
                })
            }
            _ => Err(FilterError::ExpectedMatcher {
                expression: self.expr_str(),
            }),
        }
    }

    fn parse_matcher_body(&mut self) -> Result<String, FilterError> {
        match self.peek() {
            Some(Token::Ident(name)) => {
                let name = name.clone();
                self.advance();
                Ok(name)
            }
            Some(Token::String(s)) => {
                let s = s.clone();
                self.advance();
                Ok(s)
            }
            _ => Err(FilterError::ExpectedMatcher {
                expression: self.expr_str(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;

    use insta::assert_snapshot;

    use super::*;

    struct FilterCase<'a> {
        name: &'a str,
        expression: &'a str,
        contexts: &'a [EvalContext<'a>],
    }

    fn ctx<'a>(test_name: &'a str, tag_list: &'a [&'a str]) -> EvalContext<'a> {
        EvalContext {
            test_name,
            tags: tag_list,
        }
    }

    #[test]
    fn evaluates_matcher_predicates() {
        assert_snapshot!(
            render_filter_evaluations(&[
                FilterCase {
                    name: "tag default exact",
                    expression: "tag(slow)",
                    contexts: &[ctx("x", &["slow"]), ctx("x", &["slowish"]), ctx("x", &[])],
                },
                FilterCase {
                    name: "tag explicit exact",
                    expression: "tag(=slow)",
                    contexts: &[ctx("x", &["slow"]), ctx("x", &["slowish"])],
                },
                FilterCase {
                    name: "tag substring",
                    expression: "tag(~slo)",
                    contexts: &[ctx("x", &["slow"]), ctx("x", &["slowish"]), ctx("x", &["fast"])],
                },
                FilterCase {
                    name: "tag regex",
                    expression: "tag(/^slo/)",
                    contexts: &[ctx("x", &["slow"]), ctx("x", &["slower"]), ctx("x", &["not_slow"])],
                },
                FilterCase {
                    name: "tag glob",
                    expression: "tag(#py3*)",
                    contexts: &[ctx("x", &["py311"]), ctx("x", &["py312"]), ctx("x", &["py2"])],
                },
                FilterCase {
                    name: "quoted tag",
                    expression: "tag(=\"my tag\")",
                    contexts: &[ctx("x", &["my tag"]), ctx("x", &["my-tag"])],
                },
                FilterCase {
                    name: "test default substring",
                    expression: "test(login)",
                    contexts: &[
                        ctx("mod::test_login", &[]),
                        ctx("mod::test_login_flow", &[]),
                        ctx("mod::test_logout", &[]),
                    ],
                },
                FilterCase {
                    name: "test exact",
                    expression: "test(=mod::test_login)",
                    contexts: &[ctx("mod::test_login", &[]), ctx("mod::test_login_flow", &[])],
                },
                FilterCase {
                    name: "test regex",
                    expression: "test(/^mod::test_login$/)",
                    contexts: &[ctx("mod::test_login", &[]), ctx("mod::test_login_flow", &[])],
                },
                FilterCase {
                    name: "test regex alternation",
                    expression: "test(/slow|fast/)",
                    contexts: &[
                        ctx("mod::test_slow", &[]),
                        ctx("mod::test_fast", &[]),
                        ctx("mod::test_medium", &[]),
                    ],
                },
                FilterCase {
                    name: "test glob",
                    expression: "test(#*login*)",
                    contexts: &[
                        ctx("mod::test_login", &[]),
                        ctx("mod::test_logout_and_login", &[]),
                        ctx("mod::test_logout", &[]),
                    ],
                },
                FilterCase {
                    name: "test name with colons",
                    expression: "test(=mod::sub::test_login)",
                    contexts: &[
                        ctx("mod::sub::test_login", &[]),
                        ctx("mod::sub::test_login_flow", &[]),
                    ],
                },
                FilterCase {
                    name: "parametrized test regex",
                    expression: r"test(/param=1/)",
                    contexts: &[
                        ctx("mod::test_add(param=1)", &[]),
                        ctx("mod::test_add(param=2)", &[]),
                    ],
                },
                FilterCase {
                    name: "keyword tag matcher",
                    expression: "tag(test)",
                    contexts: &[ctx("x", &["test"]), ctx("x", &["slow"])],
                },
                FilterCase {
                    name: "keyword test matcher",
                    expression: "test(tag)",
                    contexts: &[ctx("mod::test_tag_something", &[]), ctx("mod::test_something", &[])],
                },
            ]),
            @r#"
tag default exact: tag(slow)
  x tags=[slow] => match
  x tags=[slowish] => miss
  x tags=[] => miss

tag explicit exact: tag(=slow)
  x tags=[slow] => match
  x tags=[slowish] => miss

tag substring: tag(~slo)
  x tags=[slow] => match
  x tags=[slowish] => match
  x tags=[fast] => miss

tag regex: tag(/^slo/)
  x tags=[slow] => match
  x tags=[slower] => match
  x tags=[not_slow] => miss

tag glob: tag(#py3*)
  x tags=[py311] => match
  x tags=[py312] => match
  x tags=[py2] => miss

quoted tag: tag(="my tag")
  x tags=[my tag] => match
  x tags=[my-tag] => miss

test default substring: test(login)
  mod::test_login tags=[] => match
  mod::test_login_flow tags=[] => match
  mod::test_logout tags=[] => miss

test exact: test(=mod::test_login)
  mod::test_login tags=[] => match
  mod::test_login_flow tags=[] => miss

test regex: test(/^mod::test_login$/)
  mod::test_login tags=[] => match
  mod::test_login_flow tags=[] => miss

test regex alternation: test(/slow|fast/)
  mod::test_slow tags=[] => match
  mod::test_fast tags=[] => match
  mod::test_medium tags=[] => miss

test glob: test(#*login*)
  mod::test_login tags=[] => match
  mod::test_logout_and_login tags=[] => match
  mod::test_logout tags=[] => miss

test name with colons: test(=mod::sub::test_login)
  mod::sub::test_login tags=[] => match
  mod::sub::test_login_flow tags=[] => miss

parametrized test regex: test(/param=1/)
  mod::test_add(param=1) tags=[] => match
  mod::test_add(param=2) tags=[] => miss

keyword tag matcher: tag(test)
  x tags=[test] => match
  x tags=[slow] => miss

keyword test matcher: test(tag)
  mod::test_tag_something tags=[] => match
  mod::test_something tags=[] => miss
"#
        );
    }

    #[test]
    fn evaluates_boolean_expressions() {
        assert_snapshot!(
            render_filter_evaluations(&[
                FilterCase {
                    name: "and requires both",
                    expression: "tag(slow) & tag(integration)",
                    contexts: &[
                        ctx("x", &["slow", "integration"]),
                        ctx("x", &["slow"]),
                        ctx("x", &["integration"]),
                    ],
                },
                FilterCase {
                    name: "and keyword",
                    expression: "tag(slow) and tag(integration)",
                    contexts: &[ctx("x", &["slow", "integration"]), ctx("x", &["slow"])],
                },
                FilterCase {
                    name: "or matches either",
                    expression: "tag(slow) | tag(fast)",
                    contexts: &[ctx("x", &["slow"]), ctx("x", &["fast"]), ctx("x", &["medium"])],
                },
                FilterCase {
                    name: "or keyword",
                    expression: "tag(slow) or tag(fast)",
                    contexts: &[ctx("x", &["slow"]), ctx("x", &["fast"])],
                },
                FilterCase {
                    name: "not keyword",
                    expression: "not tag(flaky)",
                    contexts: &[ctx("x", &[]), ctx("x", &["slow"]), ctx("x", &["flaky"])],
                },
                FilterCase {
                    name: "bang not",
                    expression: "!tag(flaky)",
                    contexts: &[ctx("x", &[]), ctx("x", &["flaky"])],
                },
                FilterCase {
                    name: "minus excludes",
                    expression: "tag(slow) - tag(flaky)",
                    contexts: &[
                        ctx("x", &["slow"]),
                        ctx("x", &["slow", "flaky"]),
                        ctx("x", &["flaky"]),
                    ],
                },
                FilterCase {
                    name: "parens override precedence",
                    expression: "(tag(a) | tag(b)) & tag(c)",
                    contexts: &[
                        ctx("x", &["a", "c"]),
                        ctx("x", &["b", "c"]),
                        ctx("x", &["a"]),
                        ctx("x", &["c"]),
                    ],
                },
                FilterCase {
                    name: "and binds tighter than or",
                    expression: "tag(a) | tag(b) & tag(c)",
                    contexts: &[ctx("x", &["a"]), ctx("x", &["b", "c"]), ctx("x", &["b"])],
                },
                FilterCase {
                    name: "combined test and tag",
                    expression: "test(login) & tag(slow)",
                    contexts: &[
                        ctx("mod::test_login", &["slow"]),
                        ctx("mod::test_login", &[]),
                        ctx("mod::test_logout", &["slow"]),
                    ],
                },
                FilterCase {
                    name: "double not",
                    expression: "not not tag(slow)",
                    contexts: &[ctx("x", &["slow"]), ctx("x", &["fast"])],
                },
                FilterCase {
                    name: "not with parens",
                    expression: "not (tag(a) & tag(b))",
                    contexts: &[ctx("x", &["a"]), ctx("x", &["b"]), ctx("x", &["a", "b"])],
                },
            ]),
            @r#"
and requires both: tag(slow) & tag(integration)
  x tags=[slow, integration] => match
  x tags=[slow] => miss
  x tags=[integration] => miss

and keyword: tag(slow) and tag(integration)
  x tags=[slow, integration] => match
  x tags=[slow] => miss

or matches either: tag(slow) | tag(fast)
  x tags=[slow] => match
  x tags=[fast] => match
  x tags=[medium] => miss

or keyword: tag(slow) or tag(fast)
  x tags=[slow] => match
  x tags=[fast] => match

not keyword: not tag(flaky)
  x tags=[] => match
  x tags=[slow] => match
  x tags=[flaky] => miss

bang not: !tag(flaky)
  x tags=[] => match
  x tags=[flaky] => miss

minus excludes: tag(slow) - tag(flaky)
  x tags=[slow] => match
  x tags=[slow, flaky] => miss
  x tags=[flaky] => miss

parens override precedence: (tag(a) | tag(b)) & tag(c)
  x tags=[a, c] => match
  x tags=[b, c] => match
  x tags=[a] => miss
  x tags=[c] => miss

and binds tighter than or: tag(a) | tag(b) & tag(c)
  x tags=[a] => match
  x tags=[b, c] => match
  x tags=[b] => miss

combined test and tag: test(login) & tag(slow)
  mod::test_login tags=[slow] => match
  mod::test_login tags=[] => miss
  mod::test_logout tags=[slow] => miss

double not: not not tag(slow)
  x tags=[slow] => match
  x tags=[fast] => miss

not with parens: not (tag(a) & tag(b))
  x tags=[a] => match
  x tags=[b] => match
  x tags=[a, b] => miss
"#
        );
    }

    #[test]
    fn filterset_set_matches_any_expression() {
        let set = FiltersetSet::new(&["tag(slow)".to_string(), "tag(integration)".to_string()])
            .expect("parse");
        assert_snapshot!(
            render_filterset_set_evaluation(
                &set,
                &[ctx("x", &["slow"]), ctx("x", &["integration"]), ctx("x", &["fast"])]
            ),
            @r#"
x tags=[slow] => match
x tags=[integration] => match
x tags=[fast] => miss
"#
        );
    }

    #[test]
    fn filterset_set_empty_matches_all() {
        let set = FiltersetSet::new(&[]).expect("parse");
        assert!(set.is_empty());
        assert_snapshot!(
            render_filterset_set_evaluation(&set, &[ctx("anything", &[])]),
            @"anything tags=[] => match"
        );
    }

    #[test]
    fn reports_parse_errors() {
        assert_snapshot!(
            render_parse_errors(&[
                "",
                "   ",
                "package(foo)",
                "slow",
                "tag(slow",
                "test(/slow",
                "tag(\"slow)",
                "test(/[invalid/)",
                "tag()",
                "tag(=)",
                "tag slow",
                "tag(slow) &",
                "tag(slow) |",
                "& tag(slow)",
                "tag(@)",
                "tag(#[)",
            ]),
            @r#"
"": EmptyExpression: empty filter expression ``
"   ": EmptyExpression: empty filter expression `   `
"package(foo)": UnknownPredicate: unknown predicate `package` in filter expression `package(foo)` (expected `test` or `tag`)
"slow": UnknownPredicate: unknown predicate `slow` in filter expression `slow` (expected `test` or `tag`)
"tag(slow": UnclosedParenthesis: expected closing `)` in filter expression `tag(slow`
"test(/slow": UnclosedRegex: unterminated regex literal in filter expression `test(/slow`
"tag(\"slow)": UnclosedString: unterminated quoted string in filter expression `tag("slow)`
"test(/[invalid/)": InvalidRegex: invalid regex `/[invalid/` in filter expression `test(/[invalid/)`: regex parse error:
    [invalid
    ^
error: unclosed character class
"tag()": ExpectedMatcher: expected a matcher body in filter expression `tag()`
"tag(=)": ExpectedMatcher: expected a matcher body in filter expression `tag(=)`
"tag slow": ExpectedPredicateOpenParen: expected `(` after predicate in filter expression `tag slow`
"tag(slow) &": UnexpectedEndOfExpression: unexpected end of filter expression `tag(slow) &`
"tag(slow) |": UnexpectedEndOfExpression: unexpected end of filter expression `tag(slow) |`
"& tag(slow)": UnexpectedToken: unexpected token `&` in filter expression `& tag(slow)`
"tag(@)": UnexpectedCharacter: unexpected character `@` in filter expression `tag(@)`
"tag(#[)": InvalidGlob: invalid glob `#[` in filter expression `tag(#[)`: error parsing glob '[': unclosed character class; missing ']'
"#
        );
    }

    fn render_filter_evaluations(cases: &[FilterCase<'_>]) -> String {
        let mut report = String::new();

        for case in cases {
            let filter = Filterset::new(case.expression).expect("parse filter expression");
            report.push_str(case.name);
            report.push_str(": ");
            report.push_str(case.expression);
            report.push('\n');

            for context in case.contexts {
                push_context_result(&mut report, "  ", context, filter.matches(context));
                report.push('\n');
            }

            report.push('\n');
        }

        report.trim_end().to_string()
    }

    fn render_filterset_set_evaluation(set: &FiltersetSet, contexts: &[EvalContext<'_>]) -> String {
        let mut report = String::new();

        for context in contexts {
            push_context_result(&mut report, "", context, set.matches(context));
            report.push('\n');
        }

        report.trim_end().to_string()
    }

    fn push_context_result(
        report: &mut String,
        indent: &str,
        context: &EvalContext<'_>,
        matches: bool,
    ) {
        if !report.is_empty() && !report.ends_with('\n') {
            report.push('\n');
        }
        report.push_str(indent);
        report.push_str(context.test_name);
        report.push_str(" tags=");
        report.push_str(&format_tags(context.tags));
        report.push_str(" => ");
        report.push_str(if matches { "match" } else { "miss" });
    }

    fn format_tags(tags: &[&str]) -> String {
        if tags.is_empty() {
            return "[]".to_string();
        }

        format!("[{}]", tags.join(", "))
    }

    fn render_parse_errors(inputs: &[&str]) -> String {
        let mut report = String::new();

        for input in inputs {
            let error = Filterset::new(input).expect_err("filter expression should not parse");
            writeln!(
                &mut report,
                "{input:?}: {}: {error}",
                filter_error_kind(&error)
            )
            .expect("write parse error report");
        }

        report.trim_end().to_string()
    }

    fn filter_error_kind(error: &FilterError) -> &'static str {
        match error {
            FilterError::UnexpectedCharacter { .. } => "UnexpectedCharacter",
            FilterError::EmptyExpression { .. } => "EmptyExpression",
            FilterError::UnclosedParenthesis { .. } => "UnclosedParenthesis",
            FilterError::UnclosedRegex { .. } => "UnclosedRegex",
            FilterError::UnclosedString { .. } => "UnclosedString",
            FilterError::UnexpectedToken { .. } => "UnexpectedToken",
            FilterError::UnexpectedEndOfExpression { .. } => "UnexpectedEndOfExpression",
            FilterError::InvalidRegex { .. } => "InvalidRegex",
            FilterError::InvalidGlob { .. } => "InvalidGlob",
            FilterError::UnknownPredicate { .. } => "UnknownPredicate",
            FilterError::ExpectedPredicateOpenParen { .. } => "ExpectedPredicateOpenParen",
            FilterError::ExpectedMatcher { .. } => "ExpectedMatcher",
        }
    }
}
