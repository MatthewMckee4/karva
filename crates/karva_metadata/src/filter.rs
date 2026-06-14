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
    use super::*;

    fn ctx<'a>(test_name: &'a str, tag_list: &'a [&'a str]) -> EvalContext<'a> {
        EvalContext {
            test_name,
            tags: tag_list,
        }
    }

    #[test]
    fn tag_default_exact() {
        let f = Filterset::new("tag(slow)").expect("parse");
        assert!(f.matches(&ctx("x", &["slow"])));
        assert!(!f.matches(&ctx("x", &["slowish"])));
        assert!(!f.matches(&ctx("x", &[])));
    }

    #[test]
    fn tag_exact_explicit() {
        let f = Filterset::new("tag(=slow)").expect("parse");
        assert!(f.matches(&ctx("x", &["slow"])));
        assert!(!f.matches(&ctx("x", &["slowish"])));
    }

    #[test]
    fn tag_substring() {
        let f = Filterset::new("tag(~slo)").expect("parse");
        assert!(f.matches(&ctx("x", &["slow"])));
        assert!(f.matches(&ctx("x", &["slowish"])));
        assert!(!f.matches(&ctx("x", &["fast"])));
    }

    #[test]
    fn tag_regex() {
        let f = Filterset::new("tag(/^slo/)").expect("parse");
        assert!(f.matches(&ctx("x", &["slow"])));
        assert!(f.matches(&ctx("x", &["slower"])));
        assert!(!f.matches(&ctx("x", &["not_slow"])));
    }

    #[test]
    fn tag_glob() {
        let f = Filterset::new("tag(#py3*)").expect("parse");
        assert!(f.matches(&ctx("x", &["py311"])));
        assert!(f.matches(&ctx("x", &["py312"])));
        assert!(!f.matches(&ctx("x", &["py2"])));
    }

    #[test]
    fn test_default_substring() {
        let f = Filterset::new("test(login)").expect("parse");
        assert!(f.matches(&ctx("mod::test_login", &[])));
        assert!(f.matches(&ctx("mod::test_login_flow", &[])));
        assert!(!f.matches(&ctx("mod::test_logout", &[])));
    }

    #[test]
    fn test_exact() {
        let f = Filterset::new("test(=mod::test_login)").expect("parse");
        assert!(f.matches(&ctx("mod::test_login", &[])));
        assert!(!f.matches(&ctx("mod::test_login_flow", &[])));
    }

    #[test]
    fn test_regex() {
        let f = Filterset::new("test(/^mod::test_login$/)").expect("parse");
        assert!(f.matches(&ctx("mod::test_login", &[])));
        assert!(!f.matches(&ctx("mod::test_login_flow", &[])));
    }

    #[test]
    fn test_regex_alternation() {
        let f = Filterset::new("test(/slow|fast/)").expect("parse");
        assert!(f.matches(&ctx("mod::test_slow", &[])));
        assert!(f.matches(&ctx("mod::test_fast", &[])));
        assert!(!f.matches(&ctx("mod::test_medium", &[])));
    }

    #[test]
    fn test_glob() {
        let f = Filterset::new("test(#*login*)").expect("parse");
        assert!(f.matches(&ctx("mod::test_login", &[])));
        assert!(f.matches(&ctx("mod::test_logout_and_login", &[])));
        assert!(!f.matches(&ctx("mod::test_logout", &[])));
    }

    #[test]
    fn quoted_string_matcher() {
        let f = Filterset::new("tag(=\"my tag\")").expect("parse");
        assert!(f.matches(&ctx("x", &["my tag"])));
        assert!(!f.matches(&ctx("x", &["my-tag"])));
    }

    #[test]
    fn and_both_required() {
        let f = Filterset::new("tag(slow) & tag(integration)").expect("parse");
        assert!(f.matches(&ctx("x", &["slow", "integration"])));
        assert!(!f.matches(&ctx("x", &["slow"])));
        assert!(!f.matches(&ctx("x", &["integration"])));
    }

    #[test]
    fn and_with_keyword_spelling() {
        let f = Filterset::new("tag(slow) and tag(integration)").expect("parse");
        assert!(f.matches(&ctx("x", &["slow", "integration"])));
        assert!(!f.matches(&ctx("x", &["slow"])));
    }

    #[test]
    fn or_either() {
        let f = Filterset::new("tag(slow) | tag(fast)").expect("parse");
        assert!(f.matches(&ctx("x", &["slow"])));
        assert!(f.matches(&ctx("x", &["fast"])));
        assert!(!f.matches(&ctx("x", &["medium"])));
    }

    #[test]
    fn or_with_keyword_spelling() {
        let f = Filterset::new("tag(slow) or tag(fast)").expect("parse");
        assert!(f.matches(&ctx("x", &["slow"])));
        assert!(f.matches(&ctx("x", &["fast"])));
    }

    #[test]
    fn not_inverts() {
        let f = Filterset::new("not tag(flaky)").expect("parse");
        assert!(f.matches(&ctx("x", &[])));
        assert!(f.matches(&ctx("x", &["slow"])));
        assert!(!f.matches(&ctx("x", &["flaky"])));
    }

    #[test]
    fn bang_inverts() {
        let f = Filterset::new("!tag(flaky)").expect("parse");
        assert!(f.matches(&ctx("x", &[])));
        assert!(!f.matches(&ctx("x", &["flaky"])));
    }

    #[test]
    fn minus_is_and_not() {
        let f = Filterset::new("tag(slow) - tag(flaky)").expect("parse");
        assert!(f.matches(&ctx("x", &["slow"])));
        assert!(!f.matches(&ctx("x", &["slow", "flaky"])));
        assert!(!f.matches(&ctx("x", &["flaky"])));
    }

    #[test]
    fn parens_override_precedence() {
        let f = Filterset::new("(tag(a) | tag(b)) & tag(c)").expect("parse");
        assert!(f.matches(&ctx("x", &["a", "c"])));
        assert!(f.matches(&ctx("x", &["b", "c"])));
        assert!(!f.matches(&ctx("x", &["a"])));
        assert!(!f.matches(&ctx("x", &["c"])));
    }

    #[test]
    fn precedence_and_binds_tighter_than_or() {
        let f = Filterset::new("tag(a) | tag(b) & tag(c)").expect("parse");
        assert!(f.matches(&ctx("x", &["a"])));
        assert!(f.matches(&ctx("x", &["b", "c"])));
        assert!(!f.matches(&ctx("x", &["b"])));
    }

    #[test]
    fn combined_test_and_tag() {
        let f = Filterset::new("test(login) & tag(slow)").expect("parse");
        assert!(f.matches(&ctx("mod::test_login", &["slow"])));
        assert!(!f.matches(&ctx("mod::test_login", &[])));
        assert!(!f.matches(&ctx("mod::test_logout", &["slow"])));
    }

    #[test]
    fn test_name_with_colons_bare() {
        let f = Filterset::new("test(=mod::sub::test_login)").expect("parse");
        assert!(f.matches(&ctx("mod::sub::test_login", &[])));
        assert!(!f.matches(&ctx("mod::sub::test_login_flow", &[])));
    }

    #[test]
    fn double_not() {
        let f = Filterset::new("not not tag(slow)").expect("parse");
        assert!(f.matches(&ctx("x", &["slow"])));
        assert!(!f.matches(&ctx("x", &["fast"])));
    }

    #[test]
    fn not_with_parens() {
        let f = Filterset::new("not (tag(a) & tag(b))").expect("parse");
        assert!(f.matches(&ctx("x", &["a"])));
        assert!(f.matches(&ctx("x", &["b"])));
        assert!(!f.matches(&ctx("x", &["a", "b"])));
    }

    #[test]
    fn filterset_set_or_across_flags() {
        let set = FiltersetSet::new(&["tag(slow)".to_string(), "tag(integration)".to_string()])
            .expect("parse");
        assert!(set.matches(&ctx("x", &["slow"])));
        assert!(set.matches(&ctx("x", &["integration"])));
        assert!(!set.matches(&ctx("x", &["fast"])));
    }

    #[test]
    fn filterset_set_empty_matches_all() {
        let set = FiltersetSet::new(&[]).expect("parse");
        assert!(set.is_empty());
        assert!(set.matches(&ctx("anything", &[])));
    }

    #[test]
    fn empty_expression_is_error() {
        assert!(matches!(
            Filterset::new(""),
            Err(FilterError::EmptyExpression { .. })
        ));
    }

    #[test]
    fn whitespace_only_is_error() {
        assert!(matches!(
            Filterset::new("   "),
            Err(FilterError::EmptyExpression { .. })
        ));
    }

    #[test]
    fn unknown_predicate_is_error() {
        assert!(matches!(
            Filterset::new("package(foo)"),
            Err(FilterError::UnknownPredicate { .. })
        ));
    }

    #[test]
    fn bare_ident_without_parens_is_error() {
        assert!(matches!(
            Filterset::new("slow"),
            Err(FilterError::UnknownPredicate { .. })
        ));
    }

    #[test]
    fn unclosed_paren_is_error() {
        assert!(matches!(
            Filterset::new("tag(slow"),
            Err(FilterError::UnclosedParenthesis { .. })
        ));
    }

    #[test]
    fn unclosed_regex_is_error() {
        assert!(matches!(
            Filterset::new("test(/slow"),
            Err(FilterError::UnclosedRegex { .. })
        ));
    }

    #[test]
    fn unclosed_string_is_error() {
        assert!(matches!(
            Filterset::new("tag(\"slow)"),
            Err(FilterError::UnclosedString { .. })
        ));
    }

    #[test]
    fn invalid_regex_is_error() {
        assert!(matches!(
            Filterset::new("test(/[invalid/)"),
            Err(FilterError::InvalidRegex { .. })
        ));
    }

    #[test]
    fn missing_matcher_body_is_error() {
        assert!(matches!(
            Filterset::new("tag()"),
            Err(FilterError::ExpectedMatcher { .. })
        ));
        assert!(matches!(
            Filterset::new("tag(=)"),
            Err(FilterError::ExpectedMatcher { .. })
        ));
    }

    #[test]
    fn predicate_without_parens_is_error() {
        assert!(matches!(
            Filterset::new("tag slow"),
            Err(FilterError::ExpectedPredicateOpenParen { .. })
        ));
    }

    #[test]
    fn trailing_and_is_error() {
        assert!(matches!(
            Filterset::new("tag(slow) &"),
            Err(FilterError::UnexpectedEndOfExpression { .. })
        ));
    }

    #[test]
    fn trailing_or_is_error() {
        assert!(matches!(
            Filterset::new("tag(slow) |"),
            Err(FilterError::UnexpectedEndOfExpression { .. })
        ));
    }

    #[test]
    fn leading_and_is_error() {
        assert!(matches!(
            Filterset::new("& tag(slow)"),
            Err(FilterError::UnexpectedToken { .. })
        ));
    }

    #[test]
    fn parametrized_test_name_via_regex() {
        let f = Filterset::new(r"test(/param=1/)").expect("parse");
        assert!(f.matches(&ctx("mod::test_add(param=1)", &[])));
        assert!(!f.matches(&ctx("mod::test_add(param=2)", &[])));
    }

    #[test]
    fn test_and_tag_keywords_not_reserved_inside_matchers() {
        let f = Filterset::new("tag(test)").expect("parse");
        assert!(f.matches(&ctx("x", &["test"])));
        let f = Filterset::new("test(tag)").expect("parse");
        assert!(f.matches(&ctx("mod::test_tag_something", &[])));
    }
}
