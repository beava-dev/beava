//! Recursive-descent parser for the canonical parenthesized expression grammar
//! emitted by `python/beava/_col.py::to_expr_string()`.
//!
//! # Grammar (locked — Phase 3 D-08; Phase 4 D-01)
//!
//! ```text
//! Expr      := OrExpr
//! OrExpr    := AndExpr ( 'or' AndExpr )*
//! AndExpr   := NotExpr ( 'and' NotExpr )*
//! NotExpr   := 'not' NotExpr | CmpExpr
//! CmpExpr   := AddExpr ( ('>'|'>='|'<'|'<='|'=='|'!=') AddExpr )?
//! AddExpr   := MulExpr ( ('+'|'-') MulExpr )*
//! MulExpr   := Atom ( ('*'|'/') Atom )*
//! Atom      := '(' Expr ')' | Call | Ident | Literal
//! Call      := Ident '(' ArgList ')'
//! ArgList   := Expr ( ',' Expr )* | ε
//! Literal   := Number | SingleQuotedString | 'true' | 'false' | 'null'
//! Ident     := [A-Za-z_][A-Za-z0-9_]* ( '.' [A-Za-z_][A-Za-z0-9_]* )?
//! ```
//!
//! # Post-parse AST normalization
//!
//! After parsing, two normalization passes run in order before `parse()` returns:
//!
//! **Pass A — cast bare-identifier normalization**: every `Call("cast", args)` whose
//! second argument parses as `Expr::Field { name }` is rewritten in-place to
//! `Expr::Literal(Literal::BareIdent(name))`. This lets `cast(amount, float)` flow
//! through the normal expression pipeline (identifiers parse as Fields) and
//! produce the `BareIdent` that the evaluator expects.
//!
//! **Pass B — null-equality rewrite (SDK-COL-04 compatibility)**: every
//! `BinOp("==", e, Literal::Null)` and `BinOp("==", Literal::Null, e)` is rewritten
//! recursively (bottom-up) to `Call("isnull", [e])`. Rationale: the Python SDK's
//! `.isnull()` emits `(x == null)`, but `CONTEXT.md §D-04` requires that
//! `BinOp("==")` in the evaluator stays strict-null (null == anything → Null).
//! Folding the rewrite here means `eval.rs` never needs to special-case `== null`,
//! and `.isnull()` always produces a deterministic `Bool`.
//!
//! **`!=` with null on either side IS also rewritten**: `(x != null)` /
//! `(null != x)` → `UnaryOp("not", Call("isnull", [x]))`. Without this
//! rewrite, the strict-null guard in `eval_binop` would cause `(x != null)`
//! to always evaluate to `Value::Null`, silently dropping rows where `x` is
//! present. The rewrite gives users natural "is not null" semantics.

use std::collections::BTreeSet;

/// Maximum nesting depth accepted by the parser.
///
/// Mirrors `crate::eval::MAX_EVAL_DEPTH = 512` so that any expression which
/// parses successfully is also safe to evaluate without the runtime depth
/// guard silently returning `Value::Null`. A predicate deeper than this
/// produces a `ParseError` at register time (surfaced as
/// `aggregation_invalid_where` in the registration response) instead of
/// silently filtering out every row.
pub const MAX_PARSE_DEPTH: usize = 512;

// ─── Public types ─────────────────────────────────────────────────────────────

/// Byte-offset span into the source string (`start..end`, exclusive end).
/// `col` for error reporting is `start + 1` (1-indexed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

/// Parse error with 1-indexed column number.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    /// 1-indexed byte offset of the offending character.
    pub col: usize,
    /// Human-readable reason; always prefixed `"col N: ..."`.
    pub reason: String,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.reason)
    }
}

/// Scalar literal variants.
#[derive(Debug, Clone, PartialEq)]
pub enum Literal {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    /// Bare identifier used as the type argument to `cast(x, float)`.
    /// Treated as a literal (not a field reference) by `referenced_fields`.
    BareIdent(String),
}

/// The expression AST produced by `parse()`.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    /// Column reference, e.g. `amount` or `Stream.x`.
    Field { name: String, span: Span },
    /// Scalar constant.
    Literal(Literal, Span),
    /// Binary operation, e.g. `(a > b)`, `(a and b)`.
    BinOp {
        op: String,
        left: Box<Expr>,
        right: Box<Expr>,
        span: Span,
    },
    /// Unary operation — currently only `not`.
    UnaryOp {
        op: String,
        operand: Box<Expr>,
        span: Span,
    },
    /// Function call, e.g. `cast(x, float)`, `isnull(x)`.
    Call {
        fn_name: String,
        args: Vec<Expr>,
        span: Span,
    },
}

impl Expr {
    /// Returns the byte-offset span of this node in the original source.
    pub fn span(&self) -> Span {
        match self {
            Expr::Field { span, .. }
            | Expr::Literal(_, span)
            | Expr::BinOp { span, .. }
            | Expr::UnaryOp { span, .. }
            | Expr::Call { span, .. } => span.clone(),
        }
    }

    /// Collects every `Expr::Field` name referenced anywhere in this subtree
    /// into a sorted set. Literal values (including `BareIdent`) are excluded.
    pub fn referenced_fields(&self) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        collect_fields(self, &mut out);
        out
    }
}

fn collect_fields(expr: &Expr, out: &mut BTreeSet<String>) {
    match expr {
        Expr::Field { name, .. } => {
            out.insert(name.clone());
        }
        Expr::Literal(..) => {}
        Expr::BinOp { left, right, .. } => {
            collect_fields(left, out);
            collect_fields(right, out);
        }
        Expr::UnaryOp { operand, .. } => {
            collect_fields(operand, out);
        }
        Expr::Call { args, .. } => {
            for arg in args {
                collect_fields(arg, out);
            }
        }
    }
}

// ─── Entry point ──────────────────────────────────────────────────────────────

/// Parse `source` into an `Expr` AST.
///
/// Returns `Err(ParseError)` on any syntax error. The error's `col` is
/// 1-indexed into `source`; the `reason` string is human-readable and always
/// begins with `"col N: "`.
///
/// Post-parse normalization passes are applied before returning:
/// - Pass A: cast's second-arg `Field` rewritten to `Literal::BareIdent`.
/// - Pass B: `(x == null)` / `(null == x)` rewritten to `Call("isnull", [x])`.
pub fn parse(source: &str) -> Result<Expr, ParseError> {
    let mut parser = Parser::new(source)?;
    let expr = parser.parse_expr()?;
    // Reject trailing tokens — check the lookahead buffer (the lexer pre-scans ahead,
    // so `pos` may already be at `source.len()` while a buffered token remains).
    if let Some(trailing) = parser.peek() {
        let col = trailing.span.start + 1;
        let snippet = if trailing.text.is_empty() {
            format!("{:?}", &source[trailing.span.start..trailing.span.end])
        } else {
            format!("{:?}", trailing.text)
        };
        return Err(ParseError {
            col,
            reason: format!("col {col}: unexpected token {snippet}"),
        });
    }
    // Pass A: cast second-arg bare-ident normalization.
    let expr = normalize_cast(expr);
    // Pass B: null-equality rewrite.
    let expr = rewrite_null_eq(expr);
    Ok(expr)
}

// ─── Tokenizer ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
enum TokenKind {
    LParen,
    RParen,
    Comma,
    // Operators
    Plus,
    Minus,
    Star,
    Slash,
    Gt,
    GtEq,
    Lt,
    LtEq,
    EqEq,
    BangEq,
    // Keywords / identifiers
    And,
    Or,
    Not,
    True,
    False,
    Null,
    Ident,
    // Literals
    IntLit,
    FloatLit,
    StrLit,
}

#[derive(Debug, Clone)]
struct Token {
    kind: TokenKind,
    span: Span,
    /// The parsed/unescaped string value for Ident and StrLit tokens.
    text: String,
    /// Parsed integer value (for IntLit).
    int_val: i64,
    /// Parsed float value (for FloatLit).
    float_val: f64,
}

// ─── Parser ───────────────────────────────────────────────────────────────────

struct Parser<'src> {
    source: &'src str,
    pos: usize,
    /// One-token lookahead buffer.
    peeked: Option<Token>,
    /// Tracks how many unclosed `(` we are inside.
    /// Binary ops (add/mul/cmp/and/or) are only consumed when depth > 0,
    /// enforcing the SDK invariant that every binary op is parenthesized.
    paren_depth: usize,
    /// Current expression-nesting depth (incremented on every `parse_expr`
    /// entry, decremented on exit). Bounded by `MAX_PARSE_DEPTH` so that a
    /// passing parse is also safe for `eval::eval` (which caps at the same
    /// value, but silently returns Null past the cap — we want a loud
    /// register-time error instead).
    parse_depth: usize,
}

impl<'src> Parser<'src> {
    fn new(source: &'src str) -> Result<Self, ParseError> {
        let mut p = Parser {
            source,
            pos: 0,
            peeked: None,
            paren_depth: 0,
            parse_depth: 0,
        };
        // Pre-fill the lookahead so `peek()` is always valid.
        p.advance_lookahead()?;
        Ok(p)
    }

    // ── Whitespace ────────────────────────────────────────────────────────────

    fn skip_whitespace(&mut self) {
        while self.pos < self.source.len()
            && matches!(
                self.source.as_bytes()[self.pos],
                b' ' | b'\t' | b'\n' | b'\r'
            )
        {
            self.pos += 1;
        }
    }

    // ── Lexer ─────────────────────────────────────────────────────────────────

    /// Scan the next token from `self.pos`, storing it into `self.peeked`.
    /// Called after consuming a token (via `next()`) to refill the lookahead.
    fn advance_lookahead(&mut self) -> Result<(), ParseError> {
        self.skip_whitespace();
        if self.pos >= self.source.len() {
            self.peeked = None;
            return Ok(());
        }
        let start = self.pos;
        let b = self.source.as_bytes()[self.pos];

        // Single-char punctuation
        match b {
            b'(' => {
                self.pos += 1;
                self.peeked = Some(self.make_token(TokenKind::LParen, start, "", 0, 0.0));
                return Ok(());
            }
            b')' => {
                self.pos += 1;
                self.peeked = Some(self.make_token(TokenKind::RParen, start, "", 0, 0.0));
                return Ok(());
            }
            b',' => {
                self.pos += 1;
                self.peeked = Some(self.make_token(TokenKind::Comma, start, "", 0, 0.0));
                return Ok(());
            }
            b'+' => {
                self.pos += 1;
                self.peeked = Some(self.make_token(TokenKind::Plus, start, "", 0, 0.0));
                return Ok(());
            }
            b'-' => {
                self.pos += 1;
                self.peeked = Some(self.make_token(TokenKind::Minus, start, "", 0, 0.0));
                return Ok(());
            }
            b'*' => {
                self.pos += 1;
                self.peeked = Some(self.make_token(TokenKind::Star, start, "", 0, 0.0));
                return Ok(());
            }
            b'/' => {
                self.pos += 1;
                self.peeked = Some(self.make_token(TokenKind::Slash, start, "", 0, 0.0));
                return Ok(());
            }
            b'>' => {
                if self.pos + 1 < self.source.len() && self.source.as_bytes()[self.pos + 1] == b'='
                {
                    self.pos += 2;
                    self.peeked = Some(self.make_token(TokenKind::GtEq, start, "", 0, 0.0));
                } else {
                    self.pos += 1;
                    self.peeked = Some(self.make_token(TokenKind::Gt, start, "", 0, 0.0));
                }
                return Ok(());
            }
            b'<' => {
                if self.pos + 1 < self.source.len() && self.source.as_bytes()[self.pos + 1] == b'='
                {
                    self.pos += 2;
                    self.peeked = Some(self.make_token(TokenKind::LtEq, start, "", 0, 0.0));
                } else {
                    self.pos += 1;
                    self.peeked = Some(self.make_token(TokenKind::Lt, start, "", 0, 0.0));
                }
                return Ok(());
            }
            b'=' => {
                if self.pos + 1 < self.source.len() && self.source.as_bytes()[self.pos + 1] == b'='
                {
                    self.pos += 2;
                    self.peeked = Some(self.make_token(TokenKind::EqEq, start, "", 0, 0.0));
                } else {
                    let col = start + 1;
                    return Err(ParseError {
                        col,
                        reason: format!(
                            "col {col}: unexpected character '='; use '==' for equality"
                        ),
                    });
                }
                return Ok(());
            }
            b'!' => {
                if self.pos + 1 < self.source.len() && self.source.as_bytes()[self.pos + 1] == b'='
                {
                    self.pos += 2;
                    self.peeked = Some(self.make_token(TokenKind::BangEq, start, "", 0, 0.0));
                } else {
                    let col = start + 1;
                    return Err(ParseError {
                        col,
                        reason: format!("col {col}: unexpected character '!'"),
                    });
                }
                return Ok(());
            }
            b'\'' => {
                // Single-quoted string with \\ and \' unescaping.
                self.pos += 1; // skip opening '
                let mut s = String::new();
                loop {
                    if self.pos >= self.source.len() {
                        let col = self.pos + 1;
                        return Err(ParseError {
                            col,
                            reason: format!("col {col}: unterminated string literal"),
                        });
                    }
                    let c = self.source.as_bytes()[self.pos];
                    if c == b'\'' {
                        self.pos += 1; // skip closing '
                        break;
                    } else if c == b'\\' {
                        self.pos += 1;
                        if self.pos >= self.source.len() {
                            let col = self.pos + 1;
                            return Err(ParseError {
                                col,
                                reason: format!("col {col}: unterminated string literal"),
                            });
                        }
                        let esc = self.source.as_bytes()[self.pos];
                        match esc {
                            b'\\' => s.push('\\'),
                            b'\'' => s.push('\''),
                            _ => {
                                s.push('\\');
                                s.push(esc as char);
                            }
                        }
                        self.pos += 1;
                    } else {
                        s.push(c as char);
                        self.pos += 1;
                    }
                }
                self.peeked = Some(self.make_token(TokenKind::StrLit, start, &s, 0, 0.0));
                return Ok(());
            }
            _ => {}
        }

        // Number (digits, optional leading minus handled at Atom level via Minus token)
        if b.is_ascii_digit() {
            return self.lex_number(start);
        }

        // Identifier or keyword
        if b.is_ascii_alphabetic() || b == b'_' {
            let name_start = self.pos;
            while self.pos < self.source.len() {
                let c = self.source.as_bytes()[self.pos];
                if c.is_ascii_alphanumeric() || c == b'_' {
                    self.pos += 1;
                } else if c == b'.' {
                    // Allow one level of dot qualification: a.b
                    // Only extend if next char is alpha/underscore (avoids consuming trailing dot)
                    if self.pos + 1 < self.source.len() {
                        let nc = self.source.as_bytes()[self.pos + 1];
                        if nc.is_ascii_alphabetic() || nc == b'_' {
                            self.pos += 1; // consume '.'
                                           // consume rest of second segment
                            while self.pos < self.source.len() {
                                let c2 = self.source.as_bytes()[self.pos];
                                if c2.is_ascii_alphanumeric() || c2 == b'_' {
                                    self.pos += 1;
                                } else {
                                    break;
                                }
                            }
                        } else {
                            break;
                        }
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            }
            let name = &self.source[name_start..self.pos];
            let kind = match name {
                "and" => TokenKind::And,
                "or" => TokenKind::Or,
                "not" => TokenKind::Not,
                "true" => TokenKind::True,
                "false" => TokenKind::False,
                "null" => TokenKind::Null,
                _ => TokenKind::Ident,
            };
            self.peeked = Some(self.make_token(kind, start, name, 0, 0.0));
            return Ok(());
        }

        // Unknown character
        let ch = b as char;
        let col = start + 1;
        Err(ParseError {
            col,
            reason: format!("col {col}: unknown character '{ch}'"),
        })
    }

    fn lex_number(&mut self, start: usize) -> Result<(), ParseError> {
        let num_start = self.pos;
        while self.pos < self.source.len() && self.source.as_bytes()[self.pos].is_ascii_digit() {
            self.pos += 1;
        }
        // Check for decimal point
        let is_float = self.pos < self.source.len()
            && self.source.as_bytes()[self.pos] == b'.'
            && self.pos + 1 < self.source.len()
            && self.source.as_bytes()[self.pos + 1].is_ascii_digit();
        if is_float {
            self.pos += 1; // consume '.'
            while self.pos < self.source.len() && self.source.as_bytes()[self.pos].is_ascii_digit()
            {
                self.pos += 1;
            }
            // Optional exponent: e/E [+-] digits
            if self.pos < self.source.len()
                && (self.source.as_bytes()[self.pos] == b'e'
                    || self.source.as_bytes()[self.pos] == b'E')
            {
                self.pos += 1;
                if self.pos < self.source.len()
                    && (self.source.as_bytes()[self.pos] == b'+'
                        || self.source.as_bytes()[self.pos] == b'-')
                {
                    self.pos += 1;
                }
                while self.pos < self.source.len()
                    && self.source.as_bytes()[self.pos].is_ascii_digit()
                {
                    self.pos += 1;
                }
            }
            let text = &self.source[num_start..self.pos];
            let val: f64 = text.parse().map_err(|_| {
                let col = start + 1;
                ParseError {
                    col,
                    reason: format!("col {col}: invalid float literal {text:?}"),
                }
            })?;
            self.peeked = Some(self.make_token(TokenKind::FloatLit, start, text, 0, val));
        } else {
            let text = &self.source[num_start..self.pos];
            let val: i64 = text.parse().map_err(|_| {
                let col = start + 1;
                ParseError {
                    col,
                    reason: format!("col {col}: invalid integer literal {text:?}"),
                }
            })?;
            self.peeked = Some(self.make_token(TokenKind::IntLit, start, text, val, 0.0));
        }
        Ok(())
    }

    fn make_token(
        &self,
        kind: TokenKind,
        start: usize,
        text: &str,
        int_val: i64,
        float_val: f64,
    ) -> Token {
        Token {
            kind,
            span: Span {
                start,
                end: self.pos,
            },
            text: text.to_string(),
            int_val,
            float_val,
        }
    }

    // ── Token stream helpers ──────────────────────────────────────────────────

    fn peek(&self) -> Option<&Token> {
        self.peeked.as_ref()
    }

    /// Consume and return the current lookahead; advance to next token.
    fn next(&mut self) -> Result<Token, ParseError> {
        let tok = self.peeked.take().ok_or_else(|| {
            let col = self.pos + 1;
            ParseError {
                col,
                reason: format!("col {col}: unexpected end of input"),
            }
        })?;
        self.advance_lookahead()?;
        Ok(tok)
    }

    fn expect(&mut self, kind: TokenKind, msg: &str) -> Result<Token, ParseError> {
        match self.peek() {
            Some(t) if t.kind == kind => self.next(),
            Some(t) => {
                let col = t.span.start + 1;
                Err(ParseError {
                    col,
                    reason: format!("col {col}: {msg}"),
                })
            }
            None => {
                let col = self.pos + 1;
                Err(ParseError {
                    col,
                    reason: format!("col {col}: {msg}"),
                })
            }
        }
    }

    // ── Grammar non-terminals ─────────────────────────────────────────────────

    fn parse_expr(&mut self) -> Result<Expr, ParseError> {
        // Bound expression nesting at the same depth eval is willing to walk.
        // Without this, a 600-deep `&` chain parses cleanly, then `eval` (cap
        // `MAX_EVAL_DEPTH = 512`) silently returns Null at runtime — every
        // event gets filtered out with no diagnostic. Fail loudly here so
        // register surfaces `aggregation_invalid_where` with a "depth" reason
        // instead.
        self.parse_depth += 1;
        if self.parse_depth > MAX_PARSE_DEPTH {
            let col = self
                .peek()
                .map(|t| t.span.start + 1)
                .unwrap_or(self.pos + 1);
            return Err(ParseError {
                col,
                reason: format!(
                    "col {col}: predicate nesting depth exceeds {} (got {})",
                    MAX_PARSE_DEPTH, self.parse_depth
                ),
            });
        }
        let result = self.parse_or();
        self.parse_depth -= 1;
        result
    }

    fn parse_or(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_and()?;
        while self.paren_depth > 0 {
            match self.peek() {
                Some(tok) if tok.kind == TokenKind::Or => {}
                _ => break,
            }
            self.next()?; // consume 'or'
            let right = self.parse_and()?;
            let span = Span {
                start: left.span().start,
                end: right.span().end,
            };
            left = Expr::BinOp {
                op: "or".to_string(),
                left: Box::new(left),
                right: Box::new(right),
                span,
            };
        }
        Ok(left)
    }

    fn parse_and(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_not()?;
        while self.paren_depth > 0 {
            match self.peek() {
                Some(tok) if tok.kind == TokenKind::And => {}
                _ => break,
            }
            self.next()?; // consume 'and'
            let right = self.parse_not()?;
            let span = Span {
                start: left.span().start,
                end: right.span().end,
            };
            left = Expr::BinOp {
                op: "and".to_string(),
                left: Box::new(left),
                right: Box::new(right),
                span,
            };
        }
        Ok(left)
    }

    fn parse_not(&mut self) -> Result<Expr, ParseError> {
        // Note: 'not' only appears parenthesized in the SDK canonical form: "(not x)"
        // The grammar allows it as a prefix here; the parentheses are consumed by parse_atom.
        self.parse_cmp()
    }

    fn parse_cmp(&mut self) -> Result<Expr, ParseError> {
        let left = self.parse_add()?;
        // Only consume comparison operators when inside parentheses.
        if self.paren_depth == 0 {
            return Ok(left);
        }
        let op_str = match self.peek().map(|t| &t.kind) {
            Some(TokenKind::Gt) => ">",
            Some(TokenKind::GtEq) => ">=",
            Some(TokenKind::Lt) => "<",
            Some(TokenKind::LtEq) => "<=",
            Some(TokenKind::EqEq) => "==",
            Some(TokenKind::BangEq) => "!=",
            _ => return Ok(left),
        }
        .to_string();
        self.next()?; // consume operator
        let right = self.parse_add()?;
        let span = Span {
            start: left.span().start,
            end: right.span().end,
        };
        Ok(Expr::BinOp {
            op: op_str,
            left: Box::new(left),
            right: Box::new(right),
            span,
        })
    }

    fn parse_add(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_mul()?;
        while self.paren_depth > 0 {
            let op_str = match self.peek().map(|t| &t.kind) {
                Some(TokenKind::Plus) => "+",
                Some(TokenKind::Minus) => "-",
                _ => break,
            }
            .to_string();
            self.next()?;
            let right = self.parse_mul()?;
            let span = Span {
                start: left.span().start,
                end: right.span().end,
            };
            left = Expr::BinOp {
                op: op_str,
                left: Box::new(left),
                right: Box::new(right),
                span,
            };
        }
        Ok(left)
    }

    fn parse_mul(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_atom()?;
        while self.paren_depth > 0 {
            let op_str = match self.peek().map(|t| &t.kind) {
                Some(TokenKind::Star) => "*",
                Some(TokenKind::Slash) => "/",
                _ => break,
            }
            .to_string();
            self.next()?;
            let right = self.parse_atom()?;
            let span = Span {
                start: left.span().start,
                end: right.span().end,
            };
            left = Expr::BinOp {
                op: op_str,
                left: Box::new(left),
                right: Box::new(right),
                span,
            };
        }
        Ok(left)
    }

    fn parse_atom(&mut self) -> Result<Expr, ParseError> {
        let tok = match self.peek() {
            None => {
                let col = self.pos + 1;
                return Err(ParseError {
                    col,
                    reason: format!("col {col}: expected expression"),
                });
            }
            Some(t) => t,
        };

        match tok.kind.clone() {
            TokenKind::LParen => {
                let lp = self.next()?; // consume '('
                let open_start = lp.span.start;
                self.paren_depth += 1;

                // Peek: if next is 'not', parse as UnaryOp.
                if self
                    .peek()
                    .map(|t| t.kind == TokenKind::Not)
                    .unwrap_or(false)
                {
                    self.next()?; // consume 'not'
                    let operand = self.parse_expr()?;
                    self.paren_depth -= 1;
                    let rp = self.expect(TokenKind::RParen, "expected ')'")?;
                    return Ok(Expr::UnaryOp {
                        op: "not".to_string(),
                        operand: Box::new(operand),
                        span: Span {
                            start: open_start,
                            end: rp.span.end,
                        },
                    });
                }

                // General parenthesized expression.
                let inner = self.parse_expr()?;
                self.paren_depth -= 1;
                let rp = self.expect(TokenKind::RParen, "expected ')'")?;
                let outer_span = Span {
                    start: open_start,
                    end: rp.span.end,
                };
                // Propagate inner node but replace span to cover the parens.
                let spanned = match inner {
                    Expr::Field { name, .. } => Expr::Field {
                        name,
                        span: outer_span,
                    },
                    Expr::Literal(lit, _) => Expr::Literal(lit, outer_span),
                    Expr::BinOp {
                        op, left, right, ..
                    } => Expr::BinOp {
                        op,
                        left,
                        right,
                        span: outer_span,
                    },
                    Expr::UnaryOp { op, operand, .. } => Expr::UnaryOp {
                        op,
                        operand,
                        span: outer_span,
                    },
                    Expr::Call { fn_name, args, .. } => Expr::Call {
                        fn_name,
                        args,
                        span: outer_span,
                    },
                };
                Ok(spanned)
            }

            TokenKind::True => {
                let t = self.next()?;
                Ok(Expr::Literal(Literal::Bool(true), t.span))
            }
            TokenKind::False => {
                let t = self.next()?;
                Ok(Expr::Literal(Literal::Bool(false), t.span))
            }
            TokenKind::Null => {
                let t = self.next()?;
                Ok(Expr::Literal(Literal::Null, t.span))
            }

            TokenKind::And | TokenKind::Or | TokenKind::Not => {
                let t = self.next()?;
                let col = t.span.start + 1;
                Err(ParseError {
                    col,
                    reason: format!("col {col}: unexpected keyword {:?} in expression", t.text),
                })
            }

            TokenKind::Ident => {
                let t = self.next()?;
                let tok_text = t.text.clone();
                let tok_span = t.span.clone();
                // Check if followed by '(' → Call.
                if self
                    .peek()
                    .map(|p| p.kind == TokenKind::LParen)
                    .unwrap_or(false)
                {
                    self.next()?; // consume '('
                    let args = self.parse_arglist()?;
                    let rp = self.expect(TokenKind::RParen, "expected ',' or ')'")?;
                    Ok(Expr::Call {
                        fn_name: tok_text,
                        args,
                        span: Span {
                            start: tok_span.start,
                            end: rp.span.end,
                        },
                    })
                } else {
                    Ok(Expr::Field {
                        name: tok_text,
                        span: tok_span,
                    })
                }
            }

            TokenKind::IntLit => {
                let t = self.next()?;
                Ok(Expr::Literal(Literal::Int(t.int_val), t.span))
            }
            TokenKind::FloatLit => {
                let t = self.next()?;
                Ok(Expr::Literal(Literal::Float(t.float_val), t.span))
            }
            TokenKind::StrLit => {
                let t = self.next()?;
                Ok(Expr::Literal(Literal::Str(t.text.clone()), t.span))
            }

            // Negative literal: leading `-` followed immediately by a number atom.
            TokenKind::Minus => {
                let minus_tok = self.next()?; // consume '-'
                match self.peek().map(|t| t.kind.clone()) {
                    Some(TokenKind::IntLit) => {
                        let num = self.next()?;
                        let span = Span {
                            start: minus_tok.span.start,
                            end: num.span.end,
                        };
                        Ok(Expr::Literal(
                            Literal::Int(num.int_val.wrapping_neg()),
                            span,
                        ))
                    }
                    Some(TokenKind::FloatLit) => {
                        let num = self.next()?;
                        let span = Span {
                            start: minus_tok.span.start,
                            end: num.span.end,
                        };
                        Ok(Expr::Literal(Literal::Float(-num.float_val), span))
                    }
                    _ => {
                        let col = minus_tok.span.start + 1;
                        Err(ParseError {
                            col,
                            reason: format!("col {col}: '-' must be followed by a number literal"),
                        })
                    }
                }
            }

            _ => {
                let t = self.peek().unwrap();
                let col = t.span.start + 1;
                let snippet = t.text.clone();
                Err(ParseError {
                    col,
                    reason: format!("col {col}: unexpected token {snippet:?} in expression"),
                })
            }
        }
    }

    fn parse_arglist(&mut self) -> Result<Vec<Expr>, ParseError> {
        // Empty arglist: immediately ')'
        if self
            .peek()
            .map(|t| t.kind == TokenKind::RParen)
            .unwrap_or(false)
        {
            return Ok(vec![]);
        }
        let mut args = vec![self.parse_expr()?];
        while self
            .peek()
            .map(|t| t.kind == TokenKind::Comma)
            .unwrap_or(false)
        {
            self.next()?; // consume ','
            args.push(self.parse_expr()?);
        }
        Ok(args)
    }
}

// ─── Post-parse normalization ─────────────────────────────────────────────────

/// Pass A: for `cast(x, Field { name })`, rewrite second arg to `Literal::BareIdent(name)`.
/// Recurses into all nodes.
fn normalize_cast(expr: Expr) -> Expr {
    match expr {
        Expr::Call {
            fn_name,
            args,
            span,
        } => {
            // Recurse first (bottom-up).
            let mut args: Vec<Expr> = args.into_iter().map(normalize_cast).collect();
            if fn_name == "cast" && args.len() == 2 {
                if let Expr::Field {
                    name,
                    span: field_span,
                } = &args[1]
                {
                    let bare = Expr::Literal(Literal::BareIdent(name.clone()), field_span.clone());
                    args[1] = bare;
                }
            }
            Expr::Call {
                fn_name,
                args,
                span,
            }
        }
        Expr::BinOp {
            op,
            left,
            right,
            span,
        } => Expr::BinOp {
            op,
            left: Box::new(normalize_cast(*left)),
            right: Box::new(normalize_cast(*right)),
            span,
        },
        Expr::UnaryOp { op, operand, span } => Expr::UnaryOp {
            op,
            operand: Box::new(normalize_cast(*operand)),
            span,
        },
        leaf => leaf,
    }
}

/// Pass B: rewrite null-equality / null-inequality at parse time.
///
/// - `BinOp("==", e, Literal::Null)` / `BinOp("==", Literal::Null, e)`
///   → `Call("isnull", [e])`
/// - `BinOp("!=", e, Literal::Null)` / `BinOp("!=", Literal::Null, e)`
///   → `UnaryOp("not", Call("isnull", [e]))`  (i.e. `not isnull(e)`)
///
/// Both rewrites are symmetric (commutative). Applied bottom-up (recurse into
/// children first). Without the `!=` rewrite, `(x != null)` would always
/// evaluate to `Value::Null` because the strict-null propagation guard in
/// `eval_binop` fires before the `!=` branch — silently dropping rows.
fn rewrite_null_eq(expr: Expr) -> Expr {
    match expr {
        Expr::BinOp {
            op,
            left,
            right,
            span,
        } => {
            let lhs = rewrite_null_eq(*left);
            let rhs = rewrite_null_eq(*right);
            if op == "==" {
                match (&lhs, &rhs) {
                    (_, Expr::Literal(Literal::Null, _)) => {
                        return Expr::Call {
                            fn_name: "isnull".to_string(),
                            args: vec![lhs],
                            span,
                        };
                    }
                    (Expr::Literal(Literal::Null, _), _) => {
                        return Expr::Call {
                            fn_name: "isnull".to_string(),
                            args: vec![rhs],
                            span,
                        };
                    }
                    _ => {}
                }
            }
            if op == "!=" {
                // (x != null) → (not isnull(x)); symmetric for (null != x).
                match (&lhs, &rhs) {
                    (_, Expr::Literal(Literal::Null, _)) => {
                        let isnull = Expr::Call {
                            fn_name: "isnull".to_string(),
                            args: vec![lhs],
                            span: span.clone(),
                        };
                        return Expr::UnaryOp {
                            op: "not".to_string(),
                            operand: Box::new(isnull),
                            span,
                        };
                    }
                    (Expr::Literal(Literal::Null, _), _) => {
                        let isnull = Expr::Call {
                            fn_name: "isnull".to_string(),
                            args: vec![rhs],
                            span: span.clone(),
                        };
                        return Expr::UnaryOp {
                            op: "not".to_string(),
                            operand: Box::new(isnull),
                            span,
                        };
                    }
                    _ => {}
                }
            }
            Expr::BinOp {
                op,
                left: Box::new(lhs),
                right: Box::new(rhs),
                span,
            }
        }
        Expr::UnaryOp { op, operand, span } => Expr::UnaryOp {
            op,
            operand: Box::new(rewrite_null_eq(*operand)),
            span,
        },
        Expr::Call {
            fn_name,
            args,
            span,
        } => Expr::Call {
            fn_name,
            args: args.into_iter().map(rewrite_null_eq).collect(),
            span,
        },
        leaf => leaf,
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Helpers (some used only in green-phase tests; allow dead_code for red commit) ──
    // Per-fn allows below carry a brief `// reason:` so each is grep-discoverable
    // in isolation; the shared rationale is "AST-construction scaffolding for
    // future red-phase tests" per the doc-comment above.

    // reason: AST-construction scaffolding (see header comment above).
    #[allow(dead_code)]
    fn field(name: &str, start: usize, end: usize) -> Expr {
        Expr::Field {
            name: name.to_string(),
            span: Span { start, end },
        }
    }

    // reason: AST-construction scaffolding (see header comment above).
    #[allow(dead_code)]
    fn lit_int(n: i64, start: usize, end: usize) -> Expr {
        Expr::Literal(Literal::Int(n), Span { start, end })
    }

    // reason: AST-construction scaffolding (see header comment above).
    #[allow(dead_code)]
    fn lit_float(f: f64, start: usize, end: usize) -> Expr {
        Expr::Literal(Literal::Float(f), Span { start, end })
    }

    // reason: AST-construction scaffolding (see header comment above).
    #[allow(dead_code)]
    fn lit_null(start: usize, end: usize) -> Expr {
        Expr::Literal(Literal::Null, Span { start, end })
    }

    // reason: AST-construction scaffolding (see header comment above).
    #[allow(dead_code)]
    fn lit_bool(b: bool, start: usize, end: usize) -> Expr {
        Expr::Literal(Literal::Bool(b), Span { start, end })
    }

    // reason: AST-construction scaffolding (see header comment above).
    #[allow(dead_code)]
    fn lit_str(s: &str, start: usize, end: usize) -> Expr {
        Expr::Literal(Literal::Str(s.to_string()), Span { start, end })
    }

    // reason: AST-construction scaffolding (see header comment above).
    #[allow(dead_code)]
    fn binop(op: &str, left: Expr, right: Expr, start: usize, end: usize) -> Expr {
        Expr::BinOp {
            op: op.to_string(),
            left: Box::new(left),
            right: Box::new(right),
            span: Span { start, end },
        }
    }

    // reason: AST-construction scaffolding (see header comment above).
    #[allow(dead_code)]
    fn unaryop(op: &str, operand: Expr, start: usize, end: usize) -> Expr {
        Expr::UnaryOp {
            op: op.to_string(),
            operand: Box::new(operand),
            span: Span { start, end },
        }
    }

    // reason: AST-construction scaffolding (see header comment above).
    #[allow(dead_code)]
    fn call(fn_name: &str, args: Vec<Expr>, start: usize, end: usize) -> Expr {
        Expr::Call {
            fn_name: fn_name.to_string(),
            args,
            span: Span { start, end },
        }
    }

    // ── Test 1: bare field ────────────────────────────────────────────────────

    #[test]
    fn parse_bare_field() {
        let expr = parse("amount").expect("should parse bare field");
        assert!(
            matches!(&expr, Expr::Field { name, span } if name == "amount" && span.start == 0 && span.end == 6),
            "got {expr:?}"
        );
    }

    // ── Test 2: qualified field ───────────────────────────────────────────────

    #[test]
    fn parse_qualified_field() {
        let expr = parse("Stream.x").expect("should parse qualified field");
        assert!(
            matches!(&expr, Expr::Field { name, .. } if name == "Stream.x"),
            "got {expr:?}"
        );
    }

    // ── Test 3: null literal ──────────────────────────────────────────────────

    #[test]
    fn parse_null_literal() {
        let expr = parse("null").expect("should parse null literal");
        assert!(
            matches!(&expr, Expr::Literal(Literal::Null, _)),
            "got {expr:?}"
        );
    }

    // ── Test 4: bool literals ─────────────────────────────────────────────────

    #[test]
    fn parse_bool_literals() {
        let t = parse("true").expect("should parse true");
        assert!(
            matches!(&t, Expr::Literal(Literal::Bool(true), _)),
            "got {t:?}"
        );
        let f = parse("false").expect("should parse false");
        assert!(
            matches!(&f, Expr::Literal(Literal::Bool(false), _)),
            "got {f:?}"
        );
    }

    // ── Test 5: integer literals (positive + negative) ────────────────────────

    #[test]
    fn parse_integer_literal() {
        // Positive integer
        let pos = parse("42").expect("should parse 42");
        assert!(
            matches!(&pos, Expr::Literal(Literal::Int(42), _)),
            "got {pos:?}"
        );
        // Negative literal — the Python SDK emits `repr(-7)` which is `-7`
        let neg = parse("-7").expect("should parse -7");
        assert!(
            matches!(&neg, Expr::Literal(Literal::Int(-7), _)),
            "got {neg:?}"
        );
        // Parenthesized subtraction (also accepted)
        let sub = parse("(0 - 7)").expect("should parse (0 - 7)");
        assert!(
            matches!(&sub, Expr::BinOp { op, .. } if op == "-"),
            "got {sub:?}"
        );
    }

    // ── Test 6: float literals (positive + negative) ──────────────────────────

    #[test]
    fn parse_float_literal() {
        // Use 2.5 (exact in binary float; not an approximation of a named constant)
        let pos = parse("2.5").expect("should parse 2.5");
        assert!(
            matches!(&pos, Expr::Literal(Literal::Float(f), _) if *f == 2.5_f64),
            "got {pos:?}"
        );
        let neg = parse("-0.5").expect("should parse -0.5");
        assert!(
            matches!(&neg, Expr::Literal(Literal::Float(f), _) if *f == -0.5_f64),
            "got {neg:?}"
        );
    }

    // ── Test 7: string literals with escapes ──────────────────────────────────

    #[test]
    fn parse_string_literal_with_escapes() {
        // Plain string
        let plain = parse("'hello world'").expect("should parse plain string");
        assert!(
            matches!(&plain, Expr::Literal(Literal::Str(s), _) if s == "hello world"),
            "got {plain:?}"
        );
        // Escaped apostrophe: `'it\'s'` (10 bytes: ' i t \ ' s ')
        let apos = parse(r"'it\'s'").expect("should parse escaped apostrophe");
        assert!(
            matches!(&apos, Expr::Literal(Literal::Str(s), _) if s == "it's"),
            "got {apos:?}"
        );
        // Escaped backslash: `'a\\b'` → "a\b" (one backslash)
        let bs = parse(r"'a\\b'").expect("should parse escaped backslash");
        assert!(
            matches!(&bs, Expr::Literal(Literal::Str(s), _) if s == r"a\b"),
            "got {bs:?}"
        );
    }

    // ── Test 8: binary comparison ─────────────────────────────────────────────

    #[test]
    fn parse_binary_comparison() {
        let expr = parse("(amount > 100)").expect("should parse binary comparison");
        match &expr {
            Expr::BinOp {
                op,
                left,
                right,
                span,
            } => {
                assert_eq!(op, ">");
                assert!(matches!(left.as_ref(), Expr::Field { name, .. } if name == "amount"));
                assert!(matches!(
                    right.as_ref(),
                    Expr::Literal(Literal::Int(100), _)
                ));
                assert_eq!(span.start, 0);
                assert_eq!(span.end, 14);
            }
            _ => panic!("expected BinOp, got {expr:?}"),
        }
    }

    // ── Test 9: binary arithmetic ─────────────────────────────────────────────

    #[test]
    fn parse_binary_arithmetic() {
        let expr = parse("(a + b)").expect("should parse binary arithmetic");
        assert!(
            matches!(&expr, Expr::BinOp { op, left, right, .. }
                if op == "+" &&
                   matches!(left.as_ref(), Expr::Field { name, .. } if name == "a") &&
                   matches!(right.as_ref(), Expr::Field { name, .. } if name == "b")),
            "got {expr:?}"
        );
    }

    // ── Test 10: nested and/or ────────────────────────────────────────────────

    #[test]
    fn parse_nested_and_or() {
        let expr = parse("((a > 0) and (b < 5))").expect("should parse nested and/or");
        match &expr {
            Expr::BinOp {
                op, left, right, ..
            } => {
                assert_eq!(op, "and");
                assert!(matches!(left.as_ref(), Expr::BinOp { op, .. } if op == ">"));
                assert!(matches!(right.as_ref(), Expr::BinOp { op, .. } if op == "<"));
            }
            _ => panic!("expected BinOp('and'), got {expr:?}"),
        }
    }

    // ── Test 11: unary not ────────────────────────────────────────────────────

    #[test]
    fn parse_unary_not() {
        let expr = parse("(not flag)").expect("should parse unary not");
        match &expr {
            Expr::UnaryOp { op, operand, .. } => {
                assert_eq!(op, "not");
                assert!(matches!(operand.as_ref(), Expr::Field { name, .. } if name == "flag"));
            }
            _ => panic!("expected UnaryOp('not'), got {expr:?}"),
        }
    }

    // ── Test 12: call cast ────────────────────────────────────────────────────

    #[test]
    fn parse_call_cast() {
        // cast(amount, float) → after Pass A normalization, second arg is BareIdent
        let expr = parse("cast(amount, float)").expect("should parse cast call");
        match &expr {
            Expr::Call { fn_name, args, .. } => {
                assert_eq!(fn_name, "cast");
                assert_eq!(args.len(), 2);
                assert!(matches!(&args[0], Expr::Field { name, .. } if name == "amount"));
                assert!(
                    matches!(&args[1], Expr::Literal(Literal::BareIdent(n), _) if n == "float"),
                    "expected BareIdent('float'), got {:?}",
                    &args[1]
                );
            }
            _ => panic!("expected Call('cast'), got {expr:?}"),
        }
    }

    // ── Test 13: call isnull ──────────────────────────────────────────────────

    #[test]
    fn parse_call_isnull() {
        let expr = parse("isnull(amount)").expect("should parse isnull call");
        match &expr {
            Expr::Call { fn_name, args, .. } => {
                assert_eq!(fn_name, "isnull");
                assert_eq!(args.len(), 1);
                assert!(matches!(&args[0], Expr::Field { name, .. } if name == "amount"));
            }
            _ => panic!("expected Call('isnull'), got {expr:?}"),
        }
    }

    // ── Test 14: empty arglist ────────────────────────────────────────────────

    #[test]
    fn parse_empty_arglist() {
        // Grammar allows empty arglists; semantics (unknown builtins) are 04-03's concern.
        let expr = parse("noop()").expect("should parse empty arglist");
        match &expr {
            Expr::Call { fn_name, args, .. } => {
                assert_eq!(fn_name, "noop");
                assert!(args.is_empty(), "expected no args, got {args:?}");
            }
            _ => panic!("expected Call('noop'), got {expr:?}"),
        }
    }

    // ── Test 15: rejects empty input ──────────────────────────────────────────

    #[test]
    fn parse_rejects_empty_input() {
        let err = parse("").expect_err("empty input should fail");
        assert_eq!(err.col, 1, "error col should be 1 for empty input");
        let reason_lc = err.reason.to_lowercase();
        assert!(
            reason_lc.contains("expected") || reason_lc.contains("empty"),
            "reason should mention 'expected' or 'empty', got: {:?}",
            err.reason
        );
    }

    // ── Test 16: rejects trailing tokens ─────────────────────────────────────

    #[test]
    fn parse_rejects_trailing_tokens() {
        // "a b" — 'b' starts at byte 2 (0-indexed) → col 3 (1-indexed)
        let err = parse("a b").expect_err("trailing token should fail");
        assert_eq!(err.col, 3, "col should point at 'b' (byte 2 + 1)");
        let reason_lc = err.reason.to_lowercase();
        assert!(
            reason_lc.contains("unexpected") || reason_lc.contains("trailing"),
            "reason should mention unexpected/trailing, got: {:?}",
            err.reason
        );
    }

    // ── Test 17: rejects unclosed paren ───────────────────────────────────────

    #[test]
    fn parse_rejects_unclosed_paren() {
        let err = parse("(amount > 100").expect_err("unclosed paren should fail");
        // col should be at or past the end of input; reason must mention ')'
        assert!(
            err.reason.contains("')'") || err.reason.contains(")"),
            "reason should mention ')', got: {:?}",
            err.reason
        );
    }

    // ── Test 18: rejects bare binop ───────────────────────────────────────────

    #[test]
    fn parse_rejects_bare_binop() {
        // "a + b" — SDK always parenthesizes; bare binary ops are rejected.
        // '+' starts at byte 2 → col 3.
        let err = parse("a + b").expect_err("bare binary op should fail");
        // The '+' at byte 2 produces trailing content after parsing 'a'
        assert!(
            err.col >= 3,
            "error col should be ≥ 3 (at the operator), got {}",
            err.col
        );
    }

    // ── Test 19: rejects unknown trailing char ────────────────────────────────

    #[test]
    fn parse_rejects_unknown_trailing_char() {
        // "a $ b" — '$' is at byte 2 → col 3
        let err = parse("a $ b").expect_err("unknown char should fail");
        assert_eq!(err.col, 3, "col should point at '$'");
        assert!(
            err.reason.contains('$'),
            "reason should mention '$', got: {:?}",
            err.reason
        );
    }

    // ── Test 20: referenced_fields collects all fields ────────────────────────

    #[test]
    fn referenced_fields_collects_all() {
        let expr =
            parse("((amount > 100) and (isnull(merchant_id)))").expect("should parse compound");
        let fields = expr.referenced_fields();
        let expected: BTreeSet<String> = ["amount", "merchant_id"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(
            fields, expected,
            "expected {{amount, merchant_id}}, got {fields:?}"
        );
    }

    // ── Test 21: span points inside outer expr ────────────────────────────────

    #[test]
    fn span_points_inside_outer_expr() {
        // "((a > 0) and (b < 5))"
        //  0123456789012345678901
        //  ^       ^    ^    ^  ^
        //  0       8    13   17 21
        // Inner left BinOp "(a > 0)" spans bytes 1..8
        // Outer BinOp spans bytes 0..21
        let expr = parse("((a > 0) and (b < 5))").expect("should parse");
        match &expr {
            Expr::BinOp { span, left, .. } => {
                assert_eq!(span.start, 0, "outer span.start");
                assert_eq!(span.end, 21, "outer span.end");
                match left.as_ref() {
                    Expr::BinOp {
                        span: inner_span, ..
                    } => {
                        assert_eq!(inner_span.start, 1, "inner left span.start");
                        assert_eq!(inner_span.end, 8, "inner left span.end");
                    }
                    _ => panic!("expected inner BinOp for left, got {left:?}"),
                }
            }
            _ => panic!("expected outer BinOp, got {expr:?}"),
        }
    }

    // ── Test 22: (x == null) → isnull(x) (right-side null) ───────────────────

    #[test]
    fn parse_equal_null_rewrites_to_isnull_call_right() {
        let expr = parse("(x == null)").expect("should parse (x == null)");
        assert!(
            matches!(&expr, Expr::Call { fn_name, .. } if fn_name == "isnull"),
            "expected Call('isnull', ...) after rewrite, got {expr:?}"
        );
        match &expr {
            Expr::Call { fn_name, args, .. } => {
                assert_eq!(fn_name, "isnull");
                assert_eq!(args.len(), 1);
                assert!(
                    matches!(&args[0], Expr::Field { name, .. } if name == "x"),
                    "expected Field('x'), got {:?}",
                    &args[0]
                );
            }
            _ => unreachable!(),
        }
    }

    // ── Test 23: (null == x) → isnull(x) (left-side null, commutative) ───────

    #[test]
    fn parse_equal_null_rewrites_to_isnull_call_left() {
        let expr = parse("(null == x)").expect("should parse (null == x)");
        assert!(
            matches!(&expr, Expr::Call { fn_name, .. } if fn_name == "isnull"),
            "expected Call('isnull', ...) after commutative rewrite, got {expr:?}"
        );
        match &expr {
            Expr::Call { fn_name, args, .. } => {
                assert_eq!(fn_name, "isnull");
                assert_eq!(args.len(), 1);
                assert!(
                    matches!(&args[0], Expr::Field { name, .. } if name == "x"),
                    "expected Field('x'), got {:?}",
                    &args[0]
                );
            }
            _ => unreachable!(),
        }
    }

    // ── Test 24: dotted field path preserved through rewrite ──────────────────

    #[test]
    fn parse_equal_null_rewrite_preserves_field_path() {
        let expr = parse("(Stream.field == null)").expect("should parse dotted field with null");
        assert!(
            matches!(&expr, Expr::Call { fn_name, .. } if fn_name == "isnull"),
            "expected isnull call, got {expr:?}"
        );
        match &expr {
            Expr::Call { args, .. } => {
                assert!(
                    matches!(&args[0], Expr::Field { name, .. } if name == "Stream.field"),
                    "expected Field('Stream.field'), got {:?}",
                    &args[0]
                );
            }
            _ => unreachable!(),
        }
    }

    // ── Test 25: sub-expression preserved through rewrite ─────────────────────

    #[test]
    fn parse_equal_null_rewrite_with_nested_expr() {
        // ((amount + 1) == null) → isnull(BinOp("+", Field("amount"), Int(1)))
        let expr = parse("((amount + 1) == null)").expect("should parse nested expr with null");
        assert!(
            matches!(&expr, Expr::Call { fn_name, .. } if fn_name == "isnull"),
            "expected isnull call, got {expr:?}"
        );
        match &expr {
            Expr::Call { args, .. } => {
                assert!(
                    matches!(&args[0], Expr::BinOp { op, .. } if op == "+"),
                    "expected BinOp('+') as isnull arg, got {:?}",
                    &args[0]
                );
            }
            _ => unreachable!(),
        }
    }

    // ── Test 26: rewrite applies recursively inside and/or ────────────────────

    #[test]
    fn parse_equal_null_inside_and_or_rewrites() {
        // ((amount == null) and (merchant_id == 'X'))
        // → BinOp("and", Call("isnull", [Field("amount")]), BinOp("==", Field("merchant_id"), Str("X")))
        let expr = parse("((amount == null) and (merchant_id == 'X'))")
            .expect("should parse compound with null");
        match &expr {
            Expr::BinOp {
                op, left, right, ..
            } => {
                assert_eq!(op, "and");
                assert!(
                    matches!(left.as_ref(), Expr::Call { fn_name, .. } if fn_name == "isnull"),
                    "left should be isnull call, got {left:?}"
                );
                assert!(
                    matches!(right.as_ref(), Expr::BinOp { op, .. } if op == "=="),
                    "right should remain BinOp('=='), got {right:?}"
                );
            }
            _ => panic!("expected BinOp('and'), got {expr:?}"),
        }
    }

    // ── Test 27: (x != null) IS rewritten → (not isnull(x)) ─────────────────
    //
    // Pass B now rewrites both `==` and `!=` with null. Without this rewrite,
    // `(x != null)` always evaluates to Null (strict-null propagation in
    // eval_binop fires before the != branch), silently dropping rows.
    // The rewrite gives users natural "is not null" semantics.

    #[test]
    fn parse_not_equal_null_rewrites_to_not_isnull() {
        // (x != null) → UnaryOp("not", Call("isnull", [Field("x")]))
        let expr = parse("(x != null)").expect("should parse (x != null)");
        assert!(
            matches!(&expr, Expr::UnaryOp { op, .. } if op == "not"),
            "expected UnaryOp('not', ...) after != null rewrite, got {expr:?}"
        );
        match &expr {
            Expr::UnaryOp { op, operand, .. } => {
                assert_eq!(op, "not");
                assert!(
                    matches!(operand.as_ref(), Expr::Call { fn_name, .. } if fn_name == "isnull"),
                    "inner node must be isnull call, got {operand:?}"
                );
                match operand.as_ref() {
                    Expr::Call { args, .. } => {
                        assert_eq!(args.len(), 1);
                        assert!(
                            matches!(&args[0], Expr::Field { name, .. } if name == "x"),
                            "isnull arg must be Field('x'), got {:?}",
                            &args[0]
                        );
                    }
                    _ => unreachable!(),
                }
            }
            _ => unreachable!(),
        }
    }

    // ── Test 27b: (null != x) IS rewritten → (not isnull(x)) (commutative) ───

    #[test]
    fn parse_null_not_equal_rewrites_to_not_isnull_commutative() {
        // (null != x) → UnaryOp("not", Call("isnull", [Field("x")]))
        let expr = parse("(null != x)").expect("should parse (null != x)");
        assert!(
            matches!(&expr, Expr::UnaryOp { op, .. } if op == "not"),
            "expected UnaryOp('not', ...) for commutative != null rewrite, got {expr:?}"
        );
        match &expr {
            Expr::UnaryOp { operand, .. } => {
                assert!(
                    matches!(operand.as_ref(), Expr::Call { fn_name, .. } if fn_name == "isnull"),
                    "inner node must be isnull call, got {operand:?}"
                );
                match operand.as_ref() {
                    Expr::Call { args, .. } => {
                        assert!(
                            matches!(&args[0], Expr::Field { name, .. } if name == "x"),
                            "isnull arg must be Field('x'), got {:?}",
                            &args[0]
                        );
                    }
                    _ => unreachable!(),
                }
            }
            _ => unreachable!(),
        }
    }

    // ── Test 28: (null == null) → isnull(null) ────────────────────────────────

    #[test]
    fn parse_equal_null_literal_both_sides_rewrites_to_isnull_of_null() {
        // Degenerate: both sides null → isnull(null)
        let expr = parse("(null == null)").expect("should parse (null == null)");
        assert!(
            matches!(&expr, Expr::Call { fn_name, .. } if fn_name == "isnull"),
            "expected isnull call, got {expr:?}"
        );
        match &expr {
            Expr::Call { args, .. } => {
                assert_eq!(args.len(), 1);
                assert!(
                    matches!(&args[0], Expr::Literal(Literal::Null, _)),
                    "expected Literal::Null inside isnull, got {:?}",
                    &args[0]
                );
            }
            _ => unreachable!(),
        }
    }

    // ── Test 29: referenced_fields after null-equality rewrite ────────────────

    #[test]
    fn parse_equal_null_referenced_fields() {
        let expr = parse("(amount == null)").expect("should parse (amount == null)");
        let fields = expr.referenced_fields();
        let expected: BTreeSet<String> = ["amount"].iter().map(|s| s.to_string()).collect();
        assert_eq!(
            fields, expected,
            "rewrite must not drop field references; expected {{amount}}, got {fields:?}"
        );
    }

    // ── Test 30: proptest — SDK strings always parse ──────────────────────────

    use proptest::prelude::*;

    /// Mirror of Python's `_col.py` AST for proptest generation.
    #[derive(Debug, Clone)]
    enum SdkExpr {
        Field(String),
        LitNull,
        LitBool(bool),
        LitInt(i64),
        LitFloat(f64),
        LitStr(String),
        BinOp(String, Box<SdkExpr>, Box<SdkExpr>),
        UnaryNot(Box<SdkExpr>),
        CallIsnull(Box<SdkExpr>),
        CallCast(Box<SdkExpr>, String),
    }

    impl SdkExpr {
        fn to_expr_string(&self) -> String {
            match self {
                SdkExpr::Field(name) => name.clone(),
                SdkExpr::LitNull => "null".to_string(),
                SdkExpr::LitBool(b) => if *b { "true" } else { "false" }.to_string(),
                SdkExpr::LitInt(n) => n.to_string(),
                SdkExpr::LitFloat(f) => {
                    // Mimic Python repr() for floats — always includes decimal point
                    let s = format!("{f}");
                    if s.contains('.') || s.contains('e') {
                        s
                    } else {
                        format!("{s}.0")
                    }
                }
                SdkExpr::LitStr(s) => {
                    let escaped = s.replace('\\', "\\\\").replace('\'', "\\'");
                    format!("'{escaped}'")
                }
                SdkExpr::BinOp(op, l, r) => {
                    format!("({} {op} {})", l.to_expr_string(), r.to_expr_string())
                }
                SdkExpr::UnaryNot(e) => format!("(not {})", e.to_expr_string()),
                SdkExpr::CallIsnull(e) => format!("isnull({})", e.to_expr_string()),
                SdkExpr::CallCast(e, ty) => {
                    format!("cast({}, {ty})", e.to_expr_string())
                }
            }
        }
    }

    fn arb_field() -> impl Strategy<Value = SdkExpr> {
        prop_oneof![
            Just(SdkExpr::Field("a".to_string())),
            Just(SdkExpr::Field("b".to_string())),
            Just(SdkExpr::Field("amount".to_string())),
            Just(SdkExpr::Field("Stream.x".to_string())),
        ]
    }

    fn arb_literal() -> impl Strategy<Value = SdkExpr> {
        prop_oneof![
            Just(SdkExpr::LitNull),
            any::<bool>().prop_map(SdkExpr::LitBool),
            // Restrict to i32 range to avoid repr differences with very large i64
            any::<i32>().prop_map(|n| SdkExpr::LitInt(n as i64)),
            // Use well-behaved floats (no NaN/Inf)
            (-1000.0f64..1000.0f64).prop_map(SdkExpr::LitFloat),
            // Strings with printable ASCII only (no control chars that complicate escaping)
            "[a-zA-Z0-9 _/]*".prop_map(SdkExpr::LitStr),
        ]
    }

    fn arb_sdk_expr(depth: u32) -> impl Strategy<Value = SdkExpr> {
        let leaf = prop_oneof![arb_field(), arb_literal()];
        leaf.prop_recursive(depth, 64, 4, move |inner| {
            let bin_ops = vec![
                "+", "-", "*", "/", ">", ">=", "<", "<=", "==", "!=", "and", "or",
            ];
            prop_oneof![
                // BinOp: pick a random op
                (0usize..bin_ops.len(), inner.clone(), inner.clone()).prop_map(
                    move |(idx, l, r)| {
                        SdkExpr::BinOp(bin_ops[idx].to_string(), Box::new(l), Box::new(r))
                    }
                ),
                // UnaryNot
                inner.clone().prop_map(|e| SdkExpr::UnaryNot(Box::new(e))),
                // isnull call
                inner.clone().prop_map(|e| SdkExpr::CallIsnull(Box::new(e))),
                // cast call — type arg is one of the known cast types
                (
                    inner.clone(),
                    prop_oneof![Just("int"), Just("float"), Just("str"), Just("bool"),]
                )
                    .prop_map(|(e, ty)| SdkExpr::CallCast(Box::new(e), ty.to_string())),
            ]
        })
    }

    proptest! {
        #[test]
        fn proptest_sdk_strings_parse(sdk in arb_sdk_expr(4)) {
            let s = sdk.to_expr_string();
            let result = parse(&s);
            prop_assert!(
                result.is_ok(),
                "SDK-generated string failed to parse: {:?}\nError: {:?}",
                s,
                result.err()
            );
        }
    }

    // ── Test 31: parser rejects depth > MAX_PARSE_DEPTH ──────────────────────
    //
    // A 600-deep left-associative AND chain — same shape the Python SDK emits
    // for `predicate = (predicate & x)` in a loop — must be rejected at parse
    // time. Before this guard the parse succeeded and `eval::eval` silently
    // returned Null for every event, dropping all rows with no diagnostic.

    #[test]
    fn test_parser_rejects_over_max_depth() {
        // Build "(((...(a and a) and a)...) and a)" with 600 ANDs.
        // Each AND adds one paren level → 600 nested parse_expr calls.
        //
        // Run on a fat-stack worker thread. Even though the depth guard
        // bails out at frame `MAX_PARSE_DEPTH + 1`, walking that many
        // parse_expr → parse_or → … → parse_atom frames in debug mode
        // still chews through the cargo-test default ~2 MiB stack before
        // it gets there. Production main-thread stacks (8 MiB) hold this
        // comfortably.
        let handle = std::thread::Builder::new()
            .stack_size(16 * 1024 * 1024)
            .spawn(|| {
                const N: usize = 600;
                let mut s = String::from("a");
                for _ in 0..N {
                    s = format!("({s} and a)");
                }
                parse(&s).map_err(|e| e.reason).map(|_| ())
            })
            .expect("spawn fat-stack worker");
        let result = handle.join().expect("worker did not panic");
        let reason = result.expect_err("600-deep AND chain must be rejected");
        assert!(
            reason.to_lowercase().contains("depth"),
            "error reason must mention 'depth'; got: {reason:?}"
        );
        assert!(
            reason.contains(&MAX_PARSE_DEPTH.to_string()),
            "error reason should cite the limit ({}); got: {:?}",
            MAX_PARSE_DEPTH,
            reason
        );
    }

    // ── Test 32: parser accepts depth at MAX_PARSE_DEPTH ─────────────────────
    //
    // A chain whose top-level `parse_expr` depth is exactly `MAX_PARSE_DEPTH`
    // must parse cleanly. This pins down that the bound is an inclusive max,
    // not an off-by-one over-rejection that breaks legitimately-deep predicates.

    #[test]
    fn test_parser_accepts_at_max_depth() {
        // Top-level parse_expr is depth 1. Each `(...)` paren adds one more
        // parse_expr call. To land on depth == MAX_PARSE_DEPTH, wrap with
        // (MAX_PARSE_DEPTH - 1) parens-with-AND-children.
        //
        // Run on a fat-stack worker thread: cargo-test threads default to
        // ~2 MiB, but the recursive parser + recursive `Drop` on the AST
        // both need O(depth) frames. Production server threads are 8 MiB
        // (default Rust main / tokio worker) so a 512-deep predicate fits
        // at runtime; the test just needs a larger stack to reproduce that.
        let handle = std::thread::Builder::new()
            .stack_size(16 * 1024 * 1024)
            .spawn(|| {
                let n = MAX_PARSE_DEPTH - 1;
                let mut s = String::from("a");
                for _ in 0..n {
                    s = format!("({s} and a)");
                }
                parse(&s).map(|_| ()).map_err(|e| e.reason)
            })
            .expect("spawn fat-stack worker");
        let result = handle.join().expect("worker did not panic");
        assert!(
            result.is_ok(),
            "depth == MAX_PARSE_DEPTH ({}) must parse; got: {:?}",
            MAX_PARSE_DEPTH,
            result.err()
        );
    }
}
