use std::fmt;

/// A parsed tag filter expression that can be matched against a set of tag names.
#[derive(Debug, Clone)]
pub struct TagFilter {
    expr: Expr,
}

impl TagFilter {
    pub fn new(input: &str) -> Result<Self, TagFilterError> {
        let tokens = tokenize(input)?;
        let mut parser = Parser::new(&tokens);
        let expr = parser.parse_or()?;
        if parser.pos < parser.tokens.len() {
            return Err(TagFilterError {
                message: format!(
                    "unexpected token `{}` in tag expression `{input}`",
                    parser.tokens[parser.pos]
                ),
            });
        }
        Ok(Self { expr })
    }

    pub fn matches(&self, tag_names: &[&str]) -> bool {
        self.expr.matches(tag_names)
    }
}

/// A set of tag filters. Any filter must match for the set to match (OR semantics across `-t` flags).
#[derive(Debug, Clone, Default)]
pub struct TagFilterSet {
    filters: Vec<TagFilter>,
}

impl TagFilterSet {
    pub fn new(expressions: &[String]) -> Result<Self, TagFilterError> {
        let mut filters = Vec::with_capacity(expressions.len());
        for expr in expressions {
            filters.push(TagFilter::new(expr)?);
        }
        Ok(Self { filters })
    }

    pub fn is_empty(&self) -> bool {
        self.filters.is_empty()
    }

    pub fn matches(&self, tag_names: &[&str]) -> bool {
        self.filters.is_empty() || self.filters.iter().any(|f| f.matches(tag_names))
    }
}

/// Error that occurs when parsing a tag filter expression.
#[derive(Debug)]
pub struct TagFilterError {
    message: String,
}

impl fmt::Display for TagFilterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for TagFilterError {}

#[derive(Debug, Clone)]
enum Expr {
    Tag(String),
    Not(Box<Self>),
    And(Box<Self>, Box<Self>),
    Or(Box<Self>, Box<Self>),
}

impl Expr {
    fn matches(&self, tag_names: &[&str]) -> bool {
        match self {
            Self::Tag(name) => tag_names.contains(&name.as_str()),
            Self::Not(inner) => !inner.matches(tag_names),
            Self::And(lhs, rhs) => lhs.matches(tag_names) && rhs.matches(tag_names),
            Self::Or(lhs, rhs) => lhs.matches(tag_names) || rhs.matches(tag_names),
        }
    }
}

#[derive(Debug, PartialEq)]
enum Token {
    Ident(String),
    And,
    Or,
    Not,
    LParen,
    RParen,
}

impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ident(s) => write!(f, "{s}"),
            Self::And => write!(f, "and"),
            Self::Or => write!(f, "or"),
            Self::Not => write!(f, "not"),
            Self::LParen => write!(f, "("),
            Self::RParen => write!(f, ")"),
        }
    }
}

fn tokenize(input: &str) -> Result<Vec<Token>, TagFilterError> {
    let mut tokens = Vec::new();
    let mut chars = input.chars().peekable();

    while let Some(&ch) = chars.peek() {
        if ch.is_whitespace() {
            chars.next();
            continue;
        }

        if ch == '(' {
            tokens.push(Token::LParen);
            chars.next();
            continue;
        }

        if ch == ')' {
            tokens.push(Token::RParen);
            chars.next();
            continue;
        }

        if ch.is_alphanumeric() || ch == '_' {
            let mut ident = String::new();
            while let Some(&c) = chars.peek() {
                if c.is_alphanumeric() || c == '_' {
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
            continue;
        }

        return Err(TagFilterError {
            message: format!("unexpected character `{ch}` in tag expression `{input}`"),
        });
    }

    if tokens.is_empty() {
        return Err(TagFilterError {
            message: format!("empty tag expression `{input}`"),
        });
    }

    Ok(tokens)
}

struct Parser<'a> {
    tokens: &'a [Token],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(tokens: &'a [Token]) -> Self {
        Self { tokens, pos: 0 }
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn advance(&mut self) {
        self.pos += 1;
    }

    fn parse_or(&mut self) -> Result<Expr, TagFilterError> {
        let mut left = self.parse_and()?;
        while self.peek() == Some(&Token::Or) {
            self.advance();
            let right = self.parse_and()?;
            left = Expr::Or(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_and(&mut self) -> Result<Expr, TagFilterError> {
        let mut left = self.parse_not()?;
        while self.peek() == Some(&Token::And) {
            self.advance();
            let right = self.parse_not()?;
            left = Expr::And(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_not(&mut self) -> Result<Expr, TagFilterError> {
        if self.peek() == Some(&Token::Not) {
            self.advance();
            let inner = self.parse_not()?;
            return Ok(Expr::Not(Box::new(inner)));
        }
        self.parse_atom()
    }

    fn parse_atom(&mut self) -> Result<Expr, TagFilterError> {
        match self.peek() {
            Some(Token::LParen) => {
                self.advance();
                let expr = self.parse_or()?;
                if self.peek() != Some(&Token::RParen) {
                    return Err(TagFilterError {
                        message: "expected closing `)` in tag expression".to_string(),
                    });
                }
                self.advance();
                Ok(expr)
            }
            Some(Token::Ident(_)) => {
                if let Token::Ident(name) = &self.tokens[self.pos] {
                    let name = name.clone();
                    self.advance();
                    Ok(Expr::Tag(name))
                } else {
                    Err(TagFilterError {
                        message: "unexpected parse error".to_string(),
                    })
                }
            }
            Some(token) => Err(TagFilterError {
                message: format!("unexpected token `{token}` in tag expression"),
            }),
            None => Err(TagFilterError {
                message: "unexpected end of tag expression".to_string(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Single tag ──────────────────────────────────────────────────────

    #[test]
    fn single_tag_present() {
        let f = TagFilter::new("slow").expect("parse");
        assert!(f.matches(&["slow"]));
    }

    #[test]
    fn single_tag_present_among_others() {
        let f = TagFilter::new("slow").expect("parse");
        assert!(f.matches(&["slow", "integration"]));
    }

    #[test]
    fn single_tag_absent() {
        let f = TagFilter::new("slow").expect("parse");
        assert!(!f.matches(&["fast"]));
    }

    #[test]
    fn single_tag_empty_set() {
        let f = TagFilter::new("slow").expect("parse");
        assert!(!f.matches(&[]));
    }

    // ── `not` ───────────────────────────────────────────────────────────

    #[test]
    fn not_present_tag() {
        let f = TagFilter::new("not slow").expect("parse");
        assert!(!f.matches(&["slow"]));
    }

    #[test]
    fn not_absent_tag() {
        let f = TagFilter::new("not slow").expect("parse");
        assert!(f.matches(&["fast"]));
    }

    #[test]
    fn not_empty_set() {
        let f = TagFilter::new("not slow").expect("parse");
        assert!(f.matches(&[]));
    }

    #[test]
    fn double_not() {
        let f = TagFilter::new("not not slow").expect("parse");
        assert!(f.matches(&["slow"]));
        assert!(!f.matches(&["fast"]));
    }

    #[test]
    fn triple_not() {
        let f = TagFilter::new("not not not slow").expect("parse");
        assert!(!f.matches(&["slow"]));
        assert!(f.matches(&["fast"]));
    }

    // ── `and` ───────────────────────────────────────────────────────────

    #[test]
    fn and_both_present() {
        let f = TagFilter::new("slow and integration").expect("parse");
        assert!(f.matches(&["slow", "integration"]));
    }

    #[test]
    fn and_only_left_present() {
        let f = TagFilter::new("slow and integration").expect("parse");
        assert!(!f.matches(&["slow"]));
    }

    #[test]
    fn and_only_right_present() {
        let f = TagFilter::new("slow and integration").expect("parse");
        assert!(!f.matches(&["integration"]));
    }

    #[test]
    fn and_neither_present() {
        let f = TagFilter::new("slow and integration").expect("parse");
        assert!(!f.matches(&[]));
    }

    #[test]
    fn chained_and() {
        let f = TagFilter::new("a and b and c").expect("parse");
        assert!(f.matches(&["a", "b", "c"]));
        assert!(!f.matches(&["a", "b"]));
        assert!(!f.matches(&["a", "c"]));
        assert!(!f.matches(&["b", "c"]));
    }

    // ── `or` ────────────────────────────────────────────────────────────

    #[test]
    fn or_left_present() {
        let f = TagFilter::new("slow or integration").expect("parse");
        assert!(f.matches(&["slow"]));
    }

    #[test]
    fn or_right_present() {
        let f = TagFilter::new("slow or integration").expect("parse");
        assert!(f.matches(&["integration"]));
    }

    #[test]
    fn or_both_present() {
        let f = TagFilter::new("slow or integration").expect("parse");
        assert!(f.matches(&["slow", "integration"]));
    }

    #[test]
    fn or_neither_present() {
        let f = TagFilter::new("slow or integration").expect("parse");
        assert!(!f.matches(&["fast"]));
        assert!(!f.matches(&[]));
    }

    #[test]
    fn chained_or() {
        let f = TagFilter::new("a or b or c").expect("parse");
        assert!(f.matches(&["a"]));
        assert!(f.matches(&["b"]));
        assert!(f.matches(&["c"]));
        assert!(!f.matches(&["d"]));
    }

    // ── Precedence: `and` binds tighter than `or` ──────────────────────

    #[test]
    fn precedence_and_binds_tighter_than_or() {
        // "a or b and c" → "a or (b and c)"
        let f = TagFilter::new("a or b and c").expect("parse");
        assert!(f.matches(&["a"]));
        assert!(f.matches(&["b", "c"]));
        assert!(!f.matches(&["b"]));
        assert!(!f.matches(&["c"]));
    }

    #[test]
    fn precedence_reverse_order() {
        // "a and b or c" → "(a and b) or c"
        let f = TagFilter::new("a and b or c").expect("parse");
        assert!(f.matches(&["a", "b"]));
        assert!(f.matches(&["c"]));
        assert!(!f.matches(&["a"]));
        assert!(!f.matches(&["b"]));
    }

    // ── Parentheses override precedence ─────────────────────────────────

    #[test]
    fn parens_override_precedence() {
        // "(a or b) and c" — parens force or to evaluate first
        let f = TagFilter::new("(a or b) and c").expect("parse");
        assert!(f.matches(&["a", "c"]));
        assert!(f.matches(&["b", "c"]));
        assert!(!f.matches(&["a"]));
        assert!(!f.matches(&["c"]));
    }

    #[test]
    fn parens_around_and() {
        // "a or (b and c)" — explicit version of default precedence
        let f = TagFilter::new("a or (b and c)").expect("parse");
        assert!(f.matches(&["a"]));
        assert!(f.matches(&["b", "c"]));
        assert!(!f.matches(&["b"]));
    }

    #[test]
    fn nested_parens() {
        let f = TagFilter::new("((a or b) and (c or d))").expect("parse");
        assert!(f.matches(&["a", "c"]));
        assert!(f.matches(&["b", "d"]));
        assert!(f.matches(&["a", "d"]));
        assert!(!f.matches(&["a"]));
        assert!(!f.matches(&["c"]));
    }

    // ── Combined `not` with `and`/`or` ──────────────────────────────────

    #[test]
    fn and_not() {
        let f = TagFilter::new("slow and not integration").expect("parse");
        assert!(f.matches(&["slow"]));
        assert!(f.matches(&["slow", "fast"]));
        assert!(!f.matches(&["slow", "integration"]));
        assert!(!f.matches(&["integration"]));
        assert!(!f.matches(&[]));
    }

    #[test]
    fn or_not() {
        let f = TagFilter::new("slow or not integration").expect("parse");
        assert!(f.matches(&["slow"]));
        assert!(f.matches(&["slow", "integration"]));
        assert!(f.matches(&["fast"]));
        assert!(f.matches(&[]));
        assert!(!f.matches(&["integration"]));
    }

    #[test]
    fn not_with_parens() {
        // "not (a and b)" → true unless both a AND b are present
        let f = TagFilter::new("not (a and b)").expect("parse");
        assert!(f.matches(&["a"]));
        assert!(f.matches(&["b"]));
        assert!(f.matches(&[]));
        assert!(!f.matches(&["a", "b"]));
    }

    #[test]
    fn not_or_in_parens() {
        // "not (a or b)" → true only when neither a nor b
        let f = TagFilter::new("not (a or b)").expect("parse");
        assert!(f.matches(&["c"]));
        assert!(f.matches(&[]));
        assert!(!f.matches(&["a"]));
        assert!(!f.matches(&["b"]));
        assert!(!f.matches(&["a", "b"]));
    }

    // ── Identifiers ─────────────────────────────────────────────────────

    #[test]
    fn underscores_in_tag_names() {
        let f = TagFilter::new("my_tag").expect("parse");
        assert!(f.matches(&["my_tag"]));
        assert!(!f.matches(&["my"]));
    }

    #[test]
    fn numeric_in_tag_names() {
        let f = TagFilter::new("py312").expect("parse");
        assert!(f.matches(&["py312"]));
        assert!(!f.matches(&["py311"]));
    }

    #[test]
    fn tag_starting_with_underscore() {
        let f = TagFilter::new("_internal").expect("parse");
        assert!(f.matches(&["_internal"]));
    }

    // ── Whitespace handling ─────────────────────────────────────────────

    #[test]
    fn extra_whitespace() {
        let f = TagFilter::new("  slow   and   integration  ").expect("parse");
        assert!(f.matches(&["slow", "integration"]));
        assert!(!f.matches(&["slow"]));
    }

    #[test]
    fn no_whitespace_around_parens() {
        let f = TagFilter::new("(slow)and(integration)").expect("parse");
        assert!(f.matches(&["slow", "integration"]));
        assert!(!f.matches(&["slow"]));
    }

    // ── TagFilterSet (OR semantics across multiple -t flags) ────────────

    #[test]
    fn filter_set_or_semantics() {
        let set =
            TagFilterSet::new(&["slow".to_string(), "integration".to_string()]).expect("parse");
        assert!(set.matches(&["slow"]));
        assert!(set.matches(&["integration"]));
        assert!(set.matches(&["slow", "integration"]));
        assert!(!set.matches(&["fast"]));
        assert!(!set.matches(&[]));
    }

    #[test]
    fn filter_set_single_filter() {
        let set = TagFilterSet::new(&["slow".to_string()]).expect("parse");
        assert!(set.matches(&["slow"]));
        assert!(!set.matches(&["fast"]));
    }

    #[test]
    fn filter_set_empty_always_matches() {
        let set = TagFilterSet::new(&[]).expect("parse");
        assert!(set.is_empty());
        assert!(set.matches(&[]));
        assert!(set.matches(&["anything"]));
    }

    #[test]
    fn filter_set_complex_expressions() {
        // -t "slow and not flaky" -t "integration"
        let set = TagFilterSet::new(&["slow and not flaky".to_string(), "integration".to_string()])
            .expect("parse");
        // Matches first filter
        assert!(set.matches(&["slow"]));
        // Matches second filter
        assert!(set.matches(&["integration"]));
        // slow+flaky fails first filter, but no second either
        assert!(!set.matches(&["slow", "flaky"]));
        // slow+flaky+integration: first filter fails, second matches
        assert!(set.matches(&["slow", "flaky", "integration"]));
    }

    // ── Parse errors ────────────────────────────────────────────────────

    #[test]
    fn empty_expression_is_error() {
        assert!(TagFilter::new("").is_err());
    }

    #[test]
    fn whitespace_only_is_error() {
        assert!(TagFilter::new("   ").is_err());
    }

    #[test]
    fn invalid_character_is_error() {
        assert!(TagFilter::new("slow!").is_err());
        assert!(TagFilter::new("a & b").is_err());
        assert!(TagFilter::new("a | b").is_err());
    }

    #[test]
    fn unclosed_paren_is_error() {
        assert!(TagFilter::new("(slow").is_err());
    }

    #[test]
    fn extra_closing_paren_is_error() {
        assert!(TagFilter::new("slow)").is_err());
    }

    #[test]
    fn trailing_and_is_error() {
        assert!(TagFilter::new("slow and").is_err());
    }

    #[test]
    fn trailing_or_is_error() {
        assert!(TagFilter::new("slow or").is_err());
    }

    #[test]
    fn trailing_not_is_error() {
        assert!(TagFilter::new("not").is_err());
    }

    #[test]
    fn leading_and_is_error() {
        assert!(TagFilter::new("and slow").is_err());
    }

    #[test]
    fn leading_or_is_error() {
        assert!(TagFilter::new("or slow").is_err());
    }

    #[test]
    fn double_operator_is_error() {
        assert!(TagFilter::new("slow and and fast").is_err());
        assert!(TagFilter::new("slow or or fast").is_err());
    }

    #[test]
    fn empty_parens_is_error() {
        assert!(TagFilter::new("()").is_err());
    }

    #[test]
    fn filter_set_rejects_invalid_expression() {
        assert!(TagFilterSet::new(&["slow".to_string(), "and".to_string()]).is_err());
    }
}
