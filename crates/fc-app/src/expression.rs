//! Expression engine, mirroring the numeric core of FreeCAD's
//! `App::PropertyExpressionEngine` (`src/App/Expression.cpp` + the
//! `Expression.y` grammar).
//!
//! FreeCAD binds any float property to a formula over other properties,
//! e.g. `Height = 2 * Base.Length`. This module distills that engine to
//! plain numbers: [`parse`] turns expression text into an [`Expression`]
//! AST, and [`Expression::eval`] evaluates it against an [`ExprEnv`]
//! that resolves property references.
//!
//! Semantics (matching FreeCAD unless noted):
//!
//! - **Degree trig**: `sin`, `cos`, `tan` take *degrees* — `sin(30)` is
//!   `0.5`. `asin`, `acos`, `atan`, `atan2` *return* degrees.
//! - **Unit folding** (design choice — units fold into the number at
//!   parse time, using FreeCAD's internal bases: millimetre and degree):
//!
//!   | written          | folded value           |
//!   |------------------|------------------------|
//!   | `5`              | 5                      |
//!   | `5mm`            | 5                      |
//!   | `5cm`            | 50                     |
//!   | `5m`             | 5000                   |
//!   | `5in` / `5"`     | 127                    |
//!   | `30deg`          | 30                     |
//!   | `30rad`          | 30 × 180/π ≈ 1718.8734 |
//!
//!   A radian suffix folds to its degree measure and trig re-converts to
//!   radians, so the conversions cancel: `sin(30rad)` is the sine of 30
//!   radians. The suffix must touch the number (`5mm` parses; `5 mm`
//!   is a parse error).
//! - **References**: `Object.Property`, `<<Object Label>>.Property`
//!   (labels may contain spaces), and bare `Property` / `.Property`
//!   (self-reference — `object: None`).
//! - **Constants**: `pi` and `e` fold into [`Expression::Number`] at
//!   parse time. A bare identifier that matches a constant
//!   case-insensitively but not exactly (`PI`, `E`) is rejected with
//!   [`ExprError::UnknownConstant`] to catch spelling mistakes.
//! - **Precedence** (low → high): `+ -` < `* / %` < `^` < unary `+ -` <
//!   atoms. `+ -` and `* / %` are left-associative; `^` is
//!   **right**-associative (`2 ^ 3 ^ 2` = 512 — note FreeCAD's yacc
//!   grammar declares `^` left-associative; we follow the classic math
//!   convention instead). Unary minus binds tighter than `^`, like
//!   FreeCAD: `-2 ^ 2` = 4.
//!
//! Deliberate simplifications: no quantity/dimensional analysis (units
//! fold away — `5mm * 5mm` is 25, not 25 mm²), no string, conditional or
//! comparison expressions, no multi-component paths (`a.b.c`), no
//! spreadsheet cells or aggregates, ASCII-only identifiers, `ln` and
//! `log` are both the natural logarithm (FreeCAD spells it `log`),
//! division/modulo by exact zero is [`ExprError::DivisionByZero`] where
//! FreeCAD would float to ±∞, and NaN-producing calls (`sqrt(-1)`)
//! return NaN. Parse error positions are byte offsets.

use std::collections::HashMap;
use std::fmt;

/// Unary operators (FreeCAD `OperatorExpression::NEG` / `POS`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    /// `-x`
    Neg,
    /// `+x`
    Pos,
}

/// Binary operators (FreeCAD `OperatorExpression::Operator`:
/// ADD/SUB/MUL/DIV/MOD/POW).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
}

/// Expression AST. Unit suffixes and the constants `pi`/`e` are folded
/// into [`Expression::Number`] at parse time (see the [module
/// docs](self)); references keep their textual shape.
#[derive(Debug, Clone, PartialEq)]
pub enum Expression {
    /// A literal number, already folded to internal units (mm/degrees).
    Number(f64),
    /// A property reference. `object` is `None` for self-references
    /// (bare `Property` / `.Property`); `Some(label)` for
    /// `Object.Property` / `<<Object Label>>.Property`.
    Ref {
        object: Option<String>,
        property: String,
    },
    /// Unary `+`/`-`.
    Unary { op: UnaryOp, expr: Box<Expression> },
    /// Binary arithmetic.
    Binary {
        op: BinaryOp,
        lhs: Box<Expression>,
        rhs: Box<Expression>,
    },
    /// Function call, e.g. `sin(x)`. The name is resolved at evaluation
    /// time (unknown names give [`ExprError::UnknownFunction`]);
    /// arity of known functions is validated at parse time.
    Call { name: String, args: Vec<Expression> },
}

/// Expression failure modes, shaped like [`crate::RecomputeError`].
#[derive(Debug, Clone, PartialEq)]
pub enum ExprError {
    /// Malformed input. `position` is the byte offset of the offending
    /// token; `message` names the problem (with token context).
    Parse { message: String, position: usize },
    /// A call names a function outside the supported table.
    UnknownFunction { name: String },
    /// A bare identifier looks like a misspelled constant (`PI`, `E`).
    UnknownConstant { name: String },
    /// `/` or `%` with an exact-zero divisor.
    DivisionByZero,
    /// The environment could not resolve a reference; carries the
    /// reference itself.
    UnresolvableReference {
        object: Option<String>,
        property: String,
    },
}

impl fmt::Display for ExprError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExprError::Parse { message, position } => {
                write!(f, "parse error at byte {position}: {message}")
            }
            ExprError::UnknownFunction { name } => write!(f, "unknown function: '{name}'"),
            ExprError::UnknownConstant { name } => write!(f, "unknown constant: '{name}'"),
            ExprError::DivisionByZero => write!(f, "division by zero"),
            ExprError::UnresolvableReference { object, property } => match object {
                Some(object) => write!(f, "cannot resolve '{object}.{property}'"),
                None => write!(
                    f,
                    "cannot resolve property '{property}' of the current object"
                ),
            },
        }
    }
}

impl std::error::Error for ExprError {}

/// Resolves property references during evaluation. `object` is `None`
/// for self-references (bare `Property` / `.Property`).
///
/// The document layer will implement this over the property graph;
/// tests use [`MapEnv`] or the closure adapter below.
///
/// ```
/// use fc_app::expression::{parse, MapEnv};
///
/// let mut env = MapEnv::new();
/// env.set("Base", "Length", 10.0).set_self("Width", 2.0);
/// let value = parse("2 * Base.Length + Width")
///     .expect("parses")
///     .eval(&env)
///     .expect("evaluates");
/// assert_eq!(value, 22.0);
/// ```
pub trait ExprEnv {
    /// Value of `object.property` (or of the current object's
    /// `property` when `object` is `None`).
    fn resolve(&self, object: Option<&str>, property: &str) -> Result<f64, ExprError>;
}

/// Closure adapter: any `Fn(Option<&str>, &str) -> Result<f64, ExprError>`
/// is an [`ExprEnv`], so environments can be written inline.
///
/// ```
/// use fc_app::expression::{parse, ExprError};
///
/// let env = |object: Option<&str>, property: &str| -> Result<f64, ExprError> {
///     match (object, property) {
///         (Some("Box"), "Length") => Ok(10.0),
///         _ => Err(ExprError::UnresolvableReference {
///             object: object.map(str::to_string),
///             property: property.to_string(),
///         }),
///     }
/// };
/// assert_eq!(parse("Box.Length * 2").unwrap().eval(&env).unwrap(), 20.0);
/// ```
impl<F> ExprEnv for F
where
    F: Fn(Option<&str>, &str) -> Result<f64, ExprError>,
{
    fn resolve(&self, object: Option<&str>, property: &str) -> Result<f64, ExprError> {
        self(object, property)
    }
}

/// A simple [`ExprEnv`] for tests and small programs: named-object
/// references (`"Box", "Length"`) live in a `HashMap<(String, String),
/// f64>`, self-references in a per-property map. Misses give
/// [`ExprError::UnresolvableReference`] carrying the reference.
#[derive(Debug, Clone, Default)]
pub struct MapEnv {
    values: HashMap<(String, String), f64>,
    self_values: HashMap<String, f64>,
}

impl MapEnv {
    /// An empty environment (no objects, no self values).
    pub fn new() -> Self {
        Self::default()
    }

    /// Bind `object.property`. Chainable.
    pub fn set(&mut self, object: &str, property: &str, value: f64) -> &mut Self {
        self.values
            .insert((object.to_string(), property.to_string()), value);
        self
    }

    /// Bind a self-referenced `property`. Chainable.
    pub fn set_self(&mut self, property: &str, value: f64) -> &mut Self {
        self.self_values.insert(property.to_string(), value);
        self
    }
}

impl ExprEnv for MapEnv {
    fn resolve(&self, object: Option<&str>, property: &str) -> Result<f64, ExprError> {
        match object {
            Some(object) => self
                .values
                .get(&(object.to_string(), property.to_string()))
                .copied()
                .ok_or_else(|| ExprError::UnresolvableReference {
                    object: Some(object.to_string()),
                    property: property.to_string(),
                }),
            None => self.self_values.get(property).copied().ok_or_else(|| {
                ExprError::UnresolvableReference {
                    object: None,
                    property: property.to_string(),
                }
            }),
        }
    }
}

/// Known constants, mirroring FreeCAD's `CONSTANT` lexer tokens
/// (FreeCAD also has `True`/`False`/`None`).
const CONSTANTS: [(&str, f64); 2] = [("pi", std::f64::consts::PI), ("e", std::f64::consts::E)];

fn constant_value(name: &str) -> Option<f64> {
    CONSTANTS
        .iter()
        .find(|(known, _)| *known == name)
        .map(|(_, value)| *value)
}

/// Canonical spelling of the constant `name` case-insensitively matches
/// (but does not equal), e.g. `canonical_constant("PI") == Some("pi")`.
fn canonical_constant(name: &str) -> Option<&'static str> {
    CONSTANTS
        .iter()
        .find(|(known, _)| known.eq_ignore_ascii_case(name))
        .map(|(known, _)| *known)
}

/// Argument counts `(min, max)` for supported functions; `usize::MAX`
/// means unbounded. `None` = name unknown here (resolved, or rejected,
/// at evaluation time).
fn function_arity(name: &str) -> Option<(usize, usize)> {
    match name {
        "sin" | "cos" | "tan" | "asin" | "acos" | "atan" | "sqrt" | "abs" | "exp" | "ln"
        | "log" | "log10" | "floor" | "ceil" | "round" => Some((1, 1)),
        "atan2" | "pow" | "hypot" => Some((2, 2)),
        "min" | "max" => Some((2, usize::MAX)),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Tokenizer
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum TokenKind {
    /// Number with any unit suffix already folded to internal units.
    Number(f64),
    Ident(String),
    /// `<<Label>>` — content between the angle pairs.
    Label(String),
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Caret,
    LParen,
    RParen,
    Comma,
    Dot,
    Eof,
}

#[derive(Debug, Clone, PartialEq)]
struct Token {
    kind: TokenKind,
    /// Byte offset of the token's first character.
    pos: usize,
}

/// Fold a unit suffix onto `value`; returns the folded value and how
/// many bytes of `rest` the suffix consumed (0 if none). Multi-character
/// units are matched before single-character ones, so `5mm` is not read
/// as `5m` + `m`.
fn apply_unit_suffix(value: f64, rest: &str) -> (f64, usize) {
    const SUFFIXES: [(&str, f64); 7] = [
        ("deg", 1.0),
        ("rad", 180.0 / std::f64::consts::PI),
        ("mm", 1.0),
        ("cm", 10.0),
        ("in", 25.4),
        ("m", 1000.0),
        ("\"", 25.4),
    ];
    for (suffix, factor) in SUFFIXES {
        if rest.starts_with(suffix) {
            return (value * factor, suffix.len());
        }
    }
    (value, 0)
}

/// Scan a number starting at `start`: digits, optional `.digits`, then
/// optional `[eE][+-]?digits` (the exponent is only consumed when digits
/// actually follow, so `2e` leaves `e` to lex as an identifier).
/// Returns the value — with any adjacent unit suffix folded in — and the
/// byte offset just past the consumed text.
fn scan_number(input: &str, start: usize) -> Result<(f64, usize), ExprError> {
    let bytes = input.as_bytes();
    let mut end = start;
    while end < bytes.len() && bytes[end].is_ascii_digit() {
        end += 1;
    }
    if bytes.get(end) == Some(&b'.') {
        let mut after = end + 1;
        while after < bytes.len() && bytes[after].is_ascii_digit() {
            after += 1;
        }
        // Only take the dot when digits follow it; otherwise it belongs
        // to the next token (e.g. `Box.5`).
        if after > end + 1 {
            end = after;
        }
    }
    if matches!(bytes.get(end), Some(b'e') | Some(b'E')) {
        let mut after = end + 1;
        if matches!(bytes.get(after), Some(b'+') | Some(b'-')) {
            after += 1;
        }
        if after < bytes.len() && bytes[after].is_ascii_digit() {
            while after < bytes.len() && bytes[after].is_ascii_digit() {
                after += 1;
            }
            end = after;
        }
    }
    let text = &input[start..end];
    let value: f64 = text.parse().map_err(|_| ExprError::Parse {
        message: format!("malformed number '{text}'"),
        position: start,
    })?;
    let (value, suffix_len) = apply_unit_suffix(value, &input[end..]);
    Ok((value, end + suffix_len))
}

/// Scan `<<Label>>` starting at `start`. The label runs to the first
/// `>>` and must be non-empty.
fn scan_label(input: &str, start: usize) -> Result<(String, usize), ExprError> {
    let from = start + 2;
    let close = match input[from..].find(">>") {
        Some(offset) => from + offset,
        None => {
            return Err(ExprError::Parse {
                message: "unterminated object label (missing '>>')".to_string(),
                position: start,
            });
        }
    };
    let label = &input[from..close];
    if label.is_empty() {
        return Err(ExprError::Parse {
            message: "empty object label".to_string(),
            position: start,
        });
    }
    Ok((label.to_string(), close + 2))
}

fn tokenize(input: &str) -> Result<Vec<Token>, ExprError> {
    let bytes = input.as_bytes();
    let mut tokens: Vec<Token> = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let pos = i;
        let (kind, next) = match bytes[i] {
            b' ' | b'\t' | b'\r' | b'\n' => {
                i += 1;
                continue;
            }
            b'0'..=b'9' => {
                let (value, next) = scan_number(input, i)?;
                (TokenKind::Number(value), next)
            }
            b'<' if bytes.get(i + 1) == Some(&b'<') => {
                let (label, next) = scan_label(input, i)?;
                (TokenKind::Label(label), next)
            }
            b'A'..=b'Z' | b'a'..=b'z' | b'_' => {
                let mut j = i + 1;
                while j < bytes.len() && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'_') {
                    j += 1;
                }
                (TokenKind::Ident(input[i..j].to_string()), j)
            }
            b'+' => (TokenKind::Plus, i + 1),
            b'-' => (TokenKind::Minus, i + 1),
            b'*' => (TokenKind::Star, i + 1),
            b'/' => (TokenKind::Slash, i + 1),
            b'%' => (TokenKind::Percent, i + 1),
            b'^' => (TokenKind::Caret, i + 1),
            b'(' => (TokenKind::LParen, i + 1),
            b')' => (TokenKind::RParen, i + 1),
            b',' => (TokenKind::Comma, i + 1),
            b'.' => (TokenKind::Dot, i + 1),
            _ => {
                let ch = input[i..].chars().next().unwrap_or('?');
                return Err(ExprError::Parse {
                    message: format!("unexpected character '{ch}'"),
                    position: i,
                });
            }
        };
        tokens.push(Token { kind, pos });
        i = next;
    }
    tokens.push(Token {
        kind: TokenKind::Eof,
        pos: input.len(),
    });
    Ok(tokens)
}

fn describe(kind: &TokenKind) -> String {
    match kind {
        TokenKind::Number(value) => format!("number {value}"),
        TokenKind::Ident(name) => format!("'{name}'"),
        TokenKind::Label(_) => "object label".to_string(),
        TokenKind::Plus => "'+'".to_string(),
        TokenKind::Minus => "'-'".to_string(),
        TokenKind::Star => "'*'".to_string(),
        TokenKind::Slash => "'/'".to_string(),
        TokenKind::Percent => "'%'".to_string(),
        TokenKind::Caret => "'^'".to_string(),
        TokenKind::LParen => "'('".to_string(),
        TokenKind::RParen => "')'".to_string(),
        TokenKind::Comma => "','".to_string(),
        TokenKind::Dot => "'.'".to_string(),
        TokenKind::Eof => "end of input".to_string(),
    }
}

// ---------------------------------------------------------------------------
// Parser (Pratt)
// ---------------------------------------------------------------------------

/// Binding power of `+` / `-` (left-associative).
const BP_ADD: u8 = 10;
/// Binding power of `*` / `/` / `%` (left-associative).
const BP_MUL: u8 = 20;
/// Binding power of `^` (right-associative).
const BP_POW: u8 = 30;
/// Binding power of unary `+`/`-` — tighter than `^`, so `-2 ^ 2`
/// parses as `(-2) ^ 2` (FreeCAD's `NEG` precedence).
const BP_UNARY: u8 = 40;
/// Binding power of atoms (numbers, references, calls, parentheses).
const BP_ATOM: u8 = 50;

struct Parser<'t> {
    tokens: &'t [Token],
    pos: usize,
}

impl<'t> Parser<'t> {
    fn peek_kind(&self) -> Option<&TokenKind> {
        self.tokens.get(self.pos).map(|token| &token.kind)
    }

    /// Advance past the current token; never steps past the `Eof`
    /// sentinel, so indexing stays in bounds.
    fn advance(&mut self) {
        if !matches!(self.peek_kind(), None | Some(TokenKind::Eof)) {
            self.pos += 1;
        }
    }

    /// Parse error at the current token.
    fn error(&self, message: String) -> ExprError {
        let position = self
            .tokens
            .get(self.pos)
            .map(|token| token.pos)
            .unwrap_or_default();
        ExprError::Parse { message, position }
    }

    fn expect(&mut self, kind: TokenKind, message: &str) -> Result<(), ExprError> {
        if self.peek_kind() == Some(&kind) {
            self.advance();
            return Ok(());
        }
        let found = match self.peek_kind() {
            Some(found) => describe(found),
            None => "end of input".to_string(),
        };
        Err(self.error(format!("{message}, found {found}")))
    }

    fn expect_ident(&mut self, message: &str) -> Result<String, ExprError> {
        if let Some(TokenKind::Ident(name)) = self.peek_kind() {
            let name = name.clone();
            self.advance();
            return Ok(name);
        }
        let found = match self.peek_kind() {
            Some(found) => describe(found),
            None => "end of input".to_string(),
        };
        Err(self.error(format!("{message}, found {found}")))
    }

    /// Pratt loop: parse a prefix expression, then fold infix operators
    /// while their binding power exceeds `min_bp`. Left-associative
    /// operators recurse with `lbp + 1`, `^` with `lbp - 1`.
    fn parse_expr(&mut self, min_bp: u8) -> Result<Expression, ExprError> {
        let mut lhs = self.parse_prefix()?;
        loop {
            let (op, lbp, right_assoc) = match self.peek_kind() {
                Some(TokenKind::Plus) => (BinaryOp::Add, BP_ADD, false),
                Some(TokenKind::Minus) => (BinaryOp::Sub, BP_ADD, false),
                Some(TokenKind::Star) => (BinaryOp::Mul, BP_MUL, false),
                Some(TokenKind::Slash) => (BinaryOp::Div, BP_MUL, false),
                Some(TokenKind::Percent) => (BinaryOp::Mod, BP_MUL, false),
                Some(TokenKind::Caret) => (BinaryOp::Pow, BP_POW, true),
                _ => break,
            };
            if lbp <= min_bp {
                break;
            }
            self.advance();
            let rhs_bp = if right_assoc { lbp - 1 } else { lbp + 1 };
            let rhs = self.parse_expr(rhs_bp)?;
            lhs = Expression::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    fn parse_prefix(&mut self) -> Result<Expression, ExprError> {
        let pos = self
            .tokens
            .get(self.pos)
            .map(|token| token.pos)
            .unwrap_or_default();
        let kind = self.peek_kind().cloned().unwrap_or(TokenKind::Eof);
        match kind {
            TokenKind::Number(value) => {
                self.advance();
                Ok(Expression::Number(value))
            }
            TokenKind::Ident(name) => {
                self.advance();
                self.parse_name_tail(name, pos)
            }
            TokenKind::Label(label) => {
                self.advance();
                self.expect(TokenKind::Dot, "expected '.' after object label")?;
                let property = self.expect_ident("expected a property name after '.'")?;
                Ok(Expression::Ref {
                    object: Some(label),
                    property,
                })
            }
            TokenKind::Minus => {
                self.advance();
                let operand = self.parse_expr(BP_UNARY)?;
                Ok(Expression::Unary {
                    op: UnaryOp::Neg,
                    expr: Box::new(operand),
                })
            }
            TokenKind::Plus => {
                self.advance();
                let operand = self.parse_expr(BP_UNARY)?;
                Ok(Expression::Unary {
                    op: UnaryOp::Pos,
                    expr: Box::new(operand),
                })
            }
            TokenKind::LParen => {
                self.advance();
                let inner = self.parse_expr(0)?;
                self.expect(TokenKind::RParen, "expected ')'")?;
                Ok(inner)
            }
            TokenKind::Dot => {
                self.advance();
                let property = self.expect_ident("expected a property name after '.'")?;
                Ok(Expression::Ref {
                    object: None,
                    property,
                })
            }
            other => Err(self.error(format!("unexpected {}", describe(&other)))),
        }
    }

    /// After an identifier: `(` starts a call, `.` an object-qualified
    /// reference, anything else a constant or self-reference.
    fn parse_name_tail(&mut self, name: String, pos: usize) -> Result<Expression, ExprError> {
        match self.peek_kind() {
            Some(TokenKind::LParen) => {
                self.advance();
                let args = self.parse_args()?;
                self.check_arity(&name, args.len(), pos)?;
                Ok(Expression::Call { name, args })
            }
            Some(TokenKind::Dot) => {
                self.advance();
                let property = self.expect_ident("expected a property name after '.'")?;
                Ok(Expression::Ref {
                    object: Some(name),
                    property,
                })
            }
            _ => match constant_value(&name) {
                Some(value) => Ok(Expression::Number(value)),
                None if canonical_constant(&name).is_some() => {
                    Err(ExprError::UnknownConstant { name })
                }
                None => Ok(Expression::Ref {
                    object: None,
                    property: name,
                }),
            },
        }
    }

    /// Comma-separated arguments up to `)`; `name()` gives no arguments.
    fn parse_args(&mut self) -> Result<Vec<Expression>, ExprError> {
        if matches!(self.peek_kind(), Some(TokenKind::RParen)) {
            self.advance();
            return Ok(Vec::new());
        }
        let mut args = Vec::new();
        loop {
            args.push(self.parse_expr(0)?);
            match self.peek_kind() {
                Some(TokenKind::Comma) => self.advance(),
                Some(TokenKind::RParen) => {
                    self.advance();
                    return Ok(args);
                }
                _ => {
                    let found = match self.peek_kind() {
                        Some(found) => describe(found),
                        None => "end of input".to_string(),
                    };
                    return Err(self.error(format!(
                        "expected ',' or ')' in argument list, found {found}"
                    )));
                }
            }
        }
    }

    /// Arity of *known* functions is validated at parse time so errors
    /// carry the call site; unknown names are left to evaluation.
    fn check_arity(&self, name: &str, count: usize, pos: usize) -> Result<(), ExprError> {
        let Some((min, max)) = function_arity(name) else {
            return Ok(());
        };
        if count < min || count > max {
            let expected = if min == max {
                format!("{min} argument{}", if min == 1 { "" } else { "s" })
            } else {
                format!("at least {min} arguments")
            };
            return Err(ExprError::Parse {
                message: format!("'{name}' expects {expected}, got {count}"),
                position: pos,
            });
        }
        Ok(())
    }
}

/// Parse FreeCAD-style expression text into an [`Expression`] AST.
///
/// ```
/// use fc_app::expression::parse;
///
/// // Units fold to internal bases at parse time (mm, degrees).
/// assert_eq!(parse("5cm").unwrap(), parse("50").unwrap());
///
/// // References keep their shape; `<<...>>` labels may contain spaces.
/// let expr = parse("2 * Base.Length + <<My Label>>.Width").unwrap();
/// assert_eq!(
///     expr.variables(),
///     vec![
///         (Some("Base".to_string()), "Length".to_string()),
///         (Some("My Label".to_string()), "Width".to_string()),
///     ]
/// );
/// ```
pub fn parse(input: &str) -> Result<Expression, ExprError> {
    let tokens = tokenize(input)?;
    let mut parser = Parser {
        tokens: &tokens,
        pos: 0,
    };
    let expression = parser.parse_expr(0)?;
    match parser.peek_kind() {
        Some(TokenKind::Eof) | None => Ok(expression),
        Some(kind) => {
            let found = describe(kind);
            Err(parser.error(format!("unexpected {found} after expression")))
        }
    }
}

// ---------------------------------------------------------------------------
// Evaluation
// ---------------------------------------------------------------------------

impl Expression {
    /// Evaluate to a number, resolving references through `env`.
    ///
    /// Trig follows FreeCAD's degree mode: `sin(30)` is `0.5`, while
    /// `asin(0.5)` is `30`. Division or modulo by an exact zero is
    /// [`ExprError::DivisionByZero`]; domain misses (`sqrt(-1)`)
    /// evaluate to NaN, like FreeCAD.
    pub fn eval(&self, env: &dyn ExprEnv) -> Result<f64, ExprError> {
        match self {
            Expression::Number(value) => Ok(*value),
            Expression::Ref { object, property } => env.resolve(object.as_deref(), property),
            Expression::Unary { op, expr } => {
                let value = expr.eval(env)?;
                Ok(match op {
                    UnaryOp::Neg => -value,
                    UnaryOp::Pos => value,
                })
            }
            Expression::Binary { op, lhs, rhs } => {
                let l = lhs.eval(env)?;
                let r = rhs.eval(env)?;
                Ok(match op {
                    BinaryOp::Add => l + r,
                    BinaryOp::Sub => l - r,
                    BinaryOp::Mul => l * r,
                    BinaryOp::Div => {
                        if r == 0.0 {
                            return Err(ExprError::DivisionByZero);
                        }
                        l / r
                    }
                    BinaryOp::Mod => {
                        if r == 0.0 {
                            return Err(ExprError::DivisionByZero);
                        }
                        l % r
                    }
                    BinaryOp::Pow => l.powf(r),
                })
            }
            Expression::Call { name, args } => {
                let mut values = Vec::with_capacity(args.len());
                for arg in args {
                    values.push(arg.eval(env)?);
                }
                eval_call(name, &values)
            }
        }
    }
}

/// Apply a supported function to evaluated arguments. Slice patterns
/// keep arity handling panic-free; a known name with the wrong arity
/// (only constructible by hand — the parser validates arity) reports
/// [`ExprError::UnknownFunction`].
fn eval_call(name: &str, values: &[f64]) -> Result<f64, ExprError> {
    match (name, values) {
        ("sin", [x]) => Ok(x.to_radians().sin()),
        ("cos", [x]) => Ok(x.to_radians().cos()),
        ("tan", [x]) => Ok(x.to_radians().tan()),
        ("asin", [x]) => Ok(x.asin().to_degrees()),
        ("acos", [x]) => Ok(x.acos().to_degrees()),
        ("atan", [x]) => Ok(x.atan().to_degrees()),
        ("atan2", [y, x]) => Ok(y.atan2(*x).to_degrees()),
        ("sqrt", [x]) => Ok(x.sqrt()),
        ("abs", [x]) => Ok(x.abs()),
        ("exp", [x]) => Ok(x.exp()),
        ("ln", [x]) | ("log", [x]) => Ok(x.ln()),
        ("log10", [x]) => Ok(x.log10()),
        ("pow", [a, b]) => Ok(a.powf(*b)),
        ("floor", [x]) => Ok(x.floor()),
        ("ceil", [x]) => Ok(x.ceil()),
        ("round", [x]) => Ok(x.round()),
        ("hypot", [a, b]) => Ok(a.hypot(*b)),
        ("min", _) if values.len() >= 2 => Ok(values.iter().copied().fold(f64::INFINITY, f64::min)),
        ("max", _) if values.len() >= 2 => {
            Ok(values.iter().copied().fold(f64::NEG_INFINITY, f64::max))
        }
        _ => Err(ExprError::UnknownFunction {
            name: name.to_string(),
        }),
    }
}

// ---------------------------------------------------------------------------
// Introspection / printing
// ---------------------------------------------------------------------------

impl Expression {
    /// Referenced `(object, property)` pairs in source order, deduped
    /// (first occurrence wins). Constants contribute nothing (they fold
    /// into [`Expression::Number`]). The document layer will use this
    /// to wire expressions into the dependency graph.
    pub fn variables(&self) -> Vec<(Option<String>, String)> {
        let mut variables = Vec::new();
        self.collect_variables(&mut variables);
        variables
    }

    fn collect_variables(&self, variables: &mut Vec<(Option<String>, String)>) {
        match self {
            Expression::Ref { object, property } => {
                let reference = (object.clone(), property.clone());
                if !variables.contains(&reference) {
                    variables.push(reference);
                }
            }
            Expression::Unary { expr, .. } => expr.collect_variables(variables),
            Expression::Binary { lhs, rhs, .. } => {
                lhs.collect_variables(variables);
                rhs.collect_variables(variables);
            }
            Expression::Call { args, .. } => {
                for arg in args {
                    arg.collect_variables(variables);
                }
            }
            Expression::Number(_) => {}
        }
    }

    /// Canonical text form: single spaces around binary operators,
    /// `", "` between call arguments, labels quoted with `<<>>` when
    /// they are not plain identifiers, and minimal parentheses.
    ///
    /// Round-trip guarantee: for every AST produced by [`parse`],
    /// `parse(expr.to_text())` is the same AST, and `to_text` is
    /// idempotent. Spacing (and unit folds) are normalized, e.g.
    /// `2+3` → `2 + 3` and `5cm` → `50`.
    pub fn to_text(&self) -> String {
        match self {
            Expression::Number(value) => format!("{value}"),
            Expression::Ref {
                object: None,
                property,
            } => property.clone(),
            Expression::Ref {
                object: Some(object),
                property,
            } => {
                if is_identifier(object) {
                    format!("{object}.{property}")
                } else {
                    format!("<<{object}>>.{property}")
                }
            }
            Expression::Unary { op, expr } => {
                let sign = match op {
                    UnaryOp::Neg => "-",
                    UnaryOp::Pos => "+",
                };
                match **expr {
                    Expression::Number(_) | Expression::Ref { .. } | Expression::Call { .. } => {
                        format!("{sign}{}", expr.to_text())
                    }
                    _ => format!("{sign}({})", expr.to_text()),
                }
            }
            Expression::Binary { op, lhs, rhs } => {
                let bp = binary_bp(*op);
                let right_assoc = matches!(op, BinaryOp::Pow);
                let lhs_text = if precedence(lhs) < bp || (right_assoc && precedence(lhs) <= bp) {
                    format!("({})", lhs.to_text())
                } else {
                    lhs.to_text()
                };
                let rhs_text = if right_assoc {
                    // `a ^ b ^ c` means `a ^ (b ^ c)`, so an equally
                    // binding right child needs no parentheses while a
                    // weaker one does.
                    if precedence(rhs) < bp {
                        format!("({})", rhs.to_text())
                    } else {
                        rhs.to_text()
                    }
                } else if precedence(rhs) <= bp {
                    format!("({})", rhs.to_text())
                } else {
                    rhs.to_text()
                };
                format!("{lhs_text} {} {rhs_text}", op_symbol(*op))
            }
            Expression::Call { name, args } => {
                let args: Vec<String> = args.iter().map(Expression::to_text).collect();
                format!("{name}({})", args.join(", "))
            }
        }
    }
}

/// Highest operator binding power inside `expr` (see the `BP_*`
/// constants); used by [`Expression::to_text`] to decide parentheses.
fn precedence(expr: &Expression) -> u8 {
    match expr {
        Expression::Number(_) | Expression::Ref { .. } | Expression::Call { .. } => BP_ATOM,
        Expression::Unary { .. } => BP_UNARY,
        Expression::Binary { op, .. } => binary_bp(*op),
    }
}

fn binary_bp(op: BinaryOp) -> u8 {
    match op {
        BinaryOp::Add | BinaryOp::Sub => BP_ADD,
        BinaryOp::Mul | BinaryOp::Div | BinaryOp::Mod => BP_MUL,
        BinaryOp::Pow => BP_POW,
    }
}

fn op_symbol(op: BinaryOp) -> &'static str {
    match op {
        BinaryOp::Add => "+",
        BinaryOp::Sub => "-",
        BinaryOp::Mul => "*",
        BinaryOp::Div => "/",
        BinaryOp::Mod => "%",
        BinaryOp::Pow => "^",
    }
}

fn is_identifier(text: &str) -> bool {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) if first.is_ascii_alphabetic() || first == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Test environment: `Box.Length = 10`, `Box.Width = 2`,
    /// `<<My Label>>.Radius = 7`, self `Height = 4`.
    fn test_env() -> MapEnv {
        let mut env = MapEnv::new();
        env.set("Box", "Length", 10.0)
            .set("Box", "Width", 2.0)
            .set("My Label", "Radius", 7.0)
            .set_self("Height", 4.0);
        env
    }

    fn eval_in(text: &str, env: &dyn ExprEnv) -> f64 {
        parse(text)
            .expect("expression should parse")
            .eval(env)
            .expect("expression should evaluate")
    }

    fn assert_close(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() < 1e-9,
            "actual {actual} != expected {expected}"
        );
    }

    // -- precedence & associativity --------------------------------------

    #[test]
    fn precedence_task_example() {
        // The distillation's canonical example: 2 + 3 * 4^2 = 50.
        assert_eq!(eval_in("2 + 3 * 4 ^ 2", &test_env()), 50.0);
        // Whitespace does not matter.
        assert_eq!(eval_in(" 2+3*4^2 ", &test_env()), 50.0);
    }

    #[test]
    fn precedence_levels() {
        let env = test_env();
        assert_eq!(eval_in("2 + 3 * 4", &env), 14.0);
        assert_eq!(eval_in("(2 + 3) * 4", &env), 20.0);
        assert_eq!(eval_in("2 * 3 + 4", &env), 10.0);
        assert_eq!(eval_in("2 * (3 + 4)", &env), 14.0);
        assert_eq!(eval_in("2 ^ (1 + 1)", &env), 4.0);
        assert_eq!(eval_in("2 * 3 ^ 2", &env), 18.0);
    }

    #[test]
    fn left_associativity() {
        let env = test_env();
        assert_eq!(eval_in("10 - 4 - 3", &env), 3.0);
        assert_eq!(eval_in("10 - (4 - 3)", &env), 9.0);
        assert_eq!(eval_in("100 / 10 / 5", &env), 2.0);
        assert_eq!(eval_in("100 / (10 / 5)", &env), 50.0);
    }

    #[test]
    fn power_is_right_associative() {
        let env = test_env();
        // 2 ^ (3 ^ 2) = 2 ^ 9 = 512 (task spec; FreeCAD's yacc grammar
        // declares ^ left-associative instead).
        assert_eq!(eval_in("2 ^ 3 ^ 2", &env), 512.0);
        assert_eq!(eval_in("(2 ^ 3) ^ 2", &env), 64.0);
        assert_eq!(eval_in("2 ^ 3", &env), 8.0);
    }

    #[test]
    fn unary_operators() {
        let env = test_env();
        // Unary minus binds tighter than ^ (FreeCAD NEG precedence):
        // (-2) ^ 2 = 4.
        assert_eq!(eval_in("-2 ^ 2", &env), 4.0);
        assert_eq!(eval_in("2 ^ -2", &env), 0.25);
        assert_eq!(eval_in("--5", &env), 5.0);
        assert_eq!(eval_in("-(2 + 3)", &env), -5.0);
        assert_eq!(eval_in("+5", &env), 5.0);
        assert_eq!(eval_in("1 + +2", &env), 3.0);
        assert_eq!(eval_in("-2 * 3", &env), -6.0);
        assert_eq!(eval_in("-sin(90)", &env), -1.0);
        assert_eq!(eval_in("2 ^ -3 ^ 2", &env), 512.0); // (-3) ^ 2 = 9
    }

    #[test]
    fn modulo_follows_fmod_sign_rules() {
        let env = test_env();
        assert_eq!(eval_in("7 % 4", &env), 3.0);
        // fmod: the result carries the dividend's sign.
        assert_eq!(eval_in("-7 % 4", &env), -3.0);
        assert_eq!(eval_in("7 % -4", &env), 3.0);
        assert_eq!(eval_in("2 * 3 % 4", &env), 2.0);
    }

    // -- numbers, constants, units --------------------------------------

    #[test]
    fn number_literals() {
        let env = test_env();
        assert_eq!(eval_in("42", &env), 42.0);
        assert_eq!(eval_in("2.5", &env), 2.5);
        assert_eq!(eval_in("0.5", &env), 0.5);
        assert_eq!(eval_in("1e3", &env), 1000.0);
        assert_eq!(eval_in("1E3", &env), 1000.0);
        assert_eq!(eval_in("2.5e-2", &env), 0.025);
        assert_eq!(eval_in("1e+2", &env), 100.0);
    }

    #[test]
    fn constants_fold_at_parse_time() {
        let env = test_env();
        assert_close(eval_in("pi", &env), std::f64::consts::PI);
        assert_close(eval_in("e", &env), std::f64::consts::E);
        assert_close(eval_in("2 * pi", &env), 2.0 * std::f64::consts::PI);
        assert_close(
            eval_in("e ^ 2", &env),
            std::f64::consts::E * std::f64::consts::E,
        );
        assert_eq!(
            parse("pi").unwrap(),
            Expression::Number(std::f64::consts::PI)
        );
    }

    #[test]
    fn unit_folding() {
        let env = test_env();
        assert_eq!(eval_in("5", &env), 5.0);
        assert_eq!(eval_in("5mm", &env), 5.0);
        assert_eq!(eval_in("5cm", &env), 50.0);
        assert_eq!(eval_in("5m", &env), 5000.0);
        assert_eq!(eval_in("5in", &env), 127.0);
        assert_eq!(eval_in("5\"", &env), 127.0);
        assert_eq!(eval_in("30deg", &env), 30.0);
        assert_close(eval_in("30rad", &env), 30.0_f64.to_degrees());
        // Folded units are plain numbers: parse("5mm") == parse("5").
        assert_eq!(parse("5mm").unwrap(), parse("5").unwrap());
        assert_eq!(parse("5cm").unwrap(), parse("50").unwrap());
        assert_eq!(parse("1e1cm").unwrap(), parse("100").unwrap());
    }

    #[test]
    fn units_in_arithmetic() {
        let env = test_env();
        assert_eq!(eval_in("2 * 5cm", &env), 100.0);
        assert_eq!(eval_in("10mm + 2cm", &env), 30.0);
        assert_eq!(eval_in("3.5cm", &env), 35.0);
        assert_close(eval_in("1in + 25.4mm", &env), 50.8);
        // No dimensional analysis: 5mm * 5mm is just 25.
        assert_eq!(eval_in("5mm * 5mm", &env), 25.0);
    }

    #[test]
    fn unit_suffix_must_touch_the_number() {
        // FreeCAD's lexer accepts `5 mm`; this distillation requires
        // adjacency, so the spaced form is a parse error.
        assert!(matches!(parse("5 mm"), Err(ExprError::Parse { .. })));
    }

    // -- functions --------------------------------------------------------

    #[test]
    fn trig_is_degree_mode() {
        let env = test_env();
        // The task-mandated check: sin(30) = 0.5 (+/- 1e-12).
        assert!((eval_in("sin(30)", &env) - 0.5).abs() < 1e-12);
        assert!((eval_in("cos(60)", &env) - 0.5).abs() < 1e-12);
        assert!((eval_in("tan(45)", &env) - 1.0).abs() < 1e-12);
        assert!((eval_in("sin(90)", &env) - 1.0).abs() < 1e-12);
        assert!((eval_in("cos(0)", &env) - 1.0).abs() < 1e-12);
        assert!((eval_in("cos(90)", &env)).abs() < 1e-12);
        assert_close(eval_in("tan(60)", &env), 3.0_f64.sqrt());
        assert_close(eval_in("2 * sin(30)", &env), 1.0);
    }

    #[test]
    fn inverse_trig_returns_degrees() {
        let env = test_env();
        assert_close(eval_in("asin(0.5)", &env), 30.0);
        assert_close(eval_in("acos(0.5)", &env), 60.0);
        assert_close(eval_in("atan(1)", &env), 45.0);
        assert_close(eval_in("atan2(1, 1)", &env), 45.0);
        assert_close(eval_in("atan2(1, sqrt(3))", &env), 30.0);
        assert_close(eval_in("atan2(-1, -1)", &env), -135.0);
    }

    #[test]
    fn radian_arguments_cancel_the_degree_fold() {
        let env = test_env();
        // 30rad folds to its degree measure; trig converts back to
        // radians, so this is sin of 30 radians.
        let value = eval_in("sin(30rad)", &env);
        assert!((value - 30.0_f64.sin()).abs() < 1e-9, "got {value}");
        // Degree mode still applies to bare numbers.
        assert_close(
            eval_in("sin(30rad)", &env) - eval_in("sin(30)", &env),
            30.0_f64.sin() - 0.5,
        );
    }

    #[test]
    fn degree_mode_documented_gotcha() {
        // `pi` is dimensionless, so `sin(pi / 6)` is sin of ~0.5236
        // DEGREES, not of 30 degrees — FreeCAD behaves the same way.
        let env = test_env();
        let expected = (std::f64::consts::PI / 6.0).to_radians().sin();
        assert_close(eval_in("sin(pi / 6)", &env), expected);
    }

    #[test]
    fn math_functions() {
        let env = test_env();
        assert_eq!(eval_in("sqrt(16)", &env), 4.0);
        assert_eq!(eval_in("abs(-3.5)", &env), 3.5);
        assert_eq!(eval_in("abs(3.5)", &env), 3.5);
        assert_eq!(eval_in("exp(0)", &env), 1.0);
        assert_close(eval_in("exp(1)", &env), std::f64::consts::E);
        assert_close(eval_in("ln(e)", &env), 1.0);
        assert_eq!(eval_in("ln(1)", &env), 0.0);
        // `log` is FreeCAD's spelling of the natural log.
        assert_close(eval_in("log(e)", &env), 1.0);
        assert_eq!(eval_in("log10(100)", &env), 2.0);
        assert_eq!(eval_in("pow(2, 10)", &env), 1024.0);
        assert_eq!(eval_in("pow(2, -2)", &env), 0.25);
        assert_eq!(eval_in("floor(2.7)", &env), 2.0);
        assert_eq!(eval_in("floor(-2.7)", &env), -3.0);
        assert_eq!(eval_in("ceil(2.1)", &env), 3.0);
        assert_eq!(eval_in("ceil(-2.1)", &env), -2.0);
        assert_eq!(eval_in("hypot(3, 4)", &env), 5.0);
        assert_eq!(eval_in("sqrt(pow(3, 2) + pow(4, 2))", &env), 5.0);
    }

    #[test]
    fn round_half_away_from_zero_like_freecad() {
        let env = test_env();
        assert_eq!(eval_in("round(2.5)", &env), 3.0);
        assert_eq!(eval_in("round(2.4)", &env), 2.0);
        assert_eq!(eval_in("round(-2.5)", &env), -3.0);
        assert_eq!(eval_in("round(2.6)", &env), 3.0);
    }

    #[test]
    fn min_max_are_n_ary() {
        let env = test_env();
        assert_eq!(eval_in("min(3, 2)", &env), 2.0);
        assert_eq!(eval_in("max(3, 2)", &env), 3.0);
        assert_eq!(eval_in("min(1, 2, 3)", &env), 1.0);
        assert_eq!(eval_in("max(4, 2, 3, 1)", &env), 4.0);
        assert_eq!(eval_in("min(2, 2)", &env), 2.0);
    }

    // -- references ------------------------------------------------------

    #[test]
    fn references_resolve_through_the_env() {
        let env = test_env();
        assert_eq!(eval_in("Box.Length", &env), 10.0);
        assert_eq!(eval_in("Box.Length * 2", &env), 20.0);
        assert_eq!(eval_in("Box.Length / Box.Width", &env), 5.0);
        // Bare identifier and leading-dot form are self-references.
        assert_eq!(eval_in("Height", &env), 4.0);
        assert_eq!(eval_in(".Height", &env), 4.0);
        assert_eq!(eval_in("Height ^ 2", &env), 16.0);
        assert_eq!(eval_in("Box.Length + Height", &env), 14.0);
        assert_eq!(eval_in("pow(Box.Length, 2)", &env), 100.0);
    }

    #[test]
    fn labels_with_spaces_reference_properties() {
        let env = test_env();
        assert_eq!(eval_in("<<My Label>>.Radius", &env), 7.0);
        assert_eq!(eval_in("<<My Label>>.Radius * Box.Width", &env), 14.0);
        assert_close(
            eval_in("2 * pi * <<My Label>>.Radius", &env),
            2.0 * std::f64::consts::PI * 7.0,
        );
        assert_eq!(
            parse("<<My Label>>.Radius").unwrap(),
            Expression::Ref {
                object: Some("My Label".to_string()),
                property: "Radius".to_string(),
            }
        );
    }

    #[test]
    fn closure_env_adapter() {
        let env = |object: Option<&str>, property: &str| -> Result<f64, ExprError> {
            match (object, property) {
                (Some("Box"), "Length") => Ok(10.0),
                (None, "Height") => Ok(4.0),
                _ => Err(ExprError::UnresolvableReference {
                    object: object.map(str::to_string),
                    property: property.to_string(),
                }),
            }
        };
        assert_eq!(eval_in("Box.Length + Height", &env), 14.0);
        assert_eq!(eval_in("3 * Height", &env), 12.0);
    }

    #[test]
    fn whitespace_is_flexible() {
        let env = test_env();
        assert_eq!(eval_in("  2\n+\t3  ", &env), 5.0);
        assert_eq!(eval_in("( 1 + 2 ) * 3", &env), 9.0);
        assert_close(eval_in("sin( 30 )", &env), 0.5);
    }

    // -- variables() -------------------------------------------------------

    #[test]
    fn variables_collect_source_order_deduped() {
        assert_eq!(
            parse("2 + 3").unwrap().variables(),
            Vec::<(Option<String>, String)>::new()
        );
        assert_eq!(
            parse("Box.Length + Height").unwrap().variables(),
            vec![
                (Some("Box".to_string()), "Length".to_string()),
                (None, "Height".to_string()),
            ]
        );
        // Duplicates collapse to the first occurrence.
        assert_eq!(
            parse("Box.Length + Box.Length * Box.Width")
                .unwrap()
                .variables(),
            vec![
                (Some("Box".to_string()), "Length".to_string()),
                (Some("Box".to_string()), "Width".to_string()),
            ]
        );
        // Call arguments come before the operator's right side.
        assert_eq!(
            parse("min(A.X, B.Y) + A.X * C.Z").unwrap().variables(),
            vec![
                (Some("A".to_string()), "X".to_string()),
                (Some("B".to_string()), "Y".to_string()),
                (Some("C".to_string()), "Z".to_string()),
            ]
        );
        // Constants fold into numbers and reference nothing.
        assert_eq!(
            parse("sin(pi) + e").unwrap().variables(),
            Vec::<(Option<String>, String)>::new()
        );
        // Self-reference via the leading-dot form.
        assert_eq!(
            parse("atan2(Box.Length, .Height)").unwrap().variables(),
            vec![
                (Some("Box".to_string()), "Length".to_string()),
                (None, "Height".to_string()),
            ]
        );
    }

    // -- to_text() ---------------------------------------------------------

    #[test]
    fn to_text_normalizes() {
        let cases = [
            ("2+3*4^2", "2 + 3 * 4 ^ 2"),
            ("  2   *  ( 3+4 ) ", "2 * (3 + 4)"),
            ("2^3^2", "2 ^ 3 ^ 2"),
            ("(2^3)^2", "(2 ^ 3) ^ 2"),
            ("2 ^ (3 * 4)", "2 ^ (3 * 4)"),
            ("(2 * 3) ^ 2", "(2 * 3) ^ 2"),
            ("-2^2", "-2 ^ 2"),
            ("2 ^ -2", "2 ^ -2"),
            ("--5", "-(-5)"),
            ("-(2+3)", "-(2 + 3)"),
            ("+5", "+5"),
            ("10-2-3", "10 - 2 - 3"),
            ("10-(2-3)", "10 - (2 - 3)"),
            ("5cm", "50"),
            ("30deg", "30"),
            (".Height", "Height"),
            ("Height", "Height"),
            ("<<My Label>>.Radius", "<<My Label>>.Radius"),
            ("atan2(1,1)", "atan2(1, 1)"),
            ("min(3 , 2)", "min(3, 2)"),
            ("-sin(30)", "-sin(30)"),
            ("2 * pi", "2 * 3.141592653589793"),
        ];
        for (input, expected) in cases {
            assert_eq!(parse(input).unwrap().to_text(), expected, "input: {input}");
        }
    }

    #[test]
    fn to_text_round_trips_and_is_idempotent() {
        let inputs = [
            "2 + 3 * 4 ^ 2",
            "2 ^ 3 ^ 2",
            "(2 ^ 3) ^ 2",
            "2 ^ (3 * 4)",
            "-2 ^ 2",
            "2 ^ -2",
            "--5",
            "-(2 + 3)",
            "10 - (2 - 3)",
            "100 / (10 / 5)",
            "2 * (3 + 4)",
            "5cm",
            "5\"",
            "30rad",
            "1e3 + 2.5e-2",
            ".Height",
            "Box.Length",
            "<<My Label>>.Radius",
            "min(A.X, .Height) * 2",
            "atan2(sin(30), cos(60))",
            "-(sin(30) + cos(60))",
            "pow(2 ^ 3, 2)",
            "7 % 4",
            "pi * 2 ^ 2",
        ];
        for input in inputs {
            let first = parse(input).expect(input);
            let text = first.to_text();
            // parse(to_text(expr)) == expr ...
            assert_eq!(parse(&text), Ok(first.clone()), "round trip of {input}");
            // ... and to_text is stable (idempotent).
            let second = parse(&text).expect(&text);
            assert_eq!(second.to_text(), text, "idempotence for {input}");
        }
    }

    #[test]
    fn to_text_quotes_only_non_identifier_labels() {
        assert_eq!(parse("Box.Length").unwrap().to_text(), "Box.Length");
        // A label that is a valid identifier round-trips unquoted; the
        // AST is identical either way.
        assert_eq!(
            parse("<<Box>>.Length").unwrap(),
            parse("Box.Length").unwrap()
        );
        assert_eq!(parse("<<Box>>.Length").unwrap().to_text(), "Box.Length");
    }

    // -- errors --------------------------------------------------------------

    #[test]
    fn parse_errors() {
        let cases = [
            "",
            "2 +",
            "(2",
            "2 & 3",
            "Box.",
            "Box.5",
            "Box.Length.Sub",
            "sin(1, 2)",
            "sin()",
            "atan2(1)",
            "min(1)",
            "2 3",
            "<<abc",
            "<<>>.P",
            "5 mm",
            "2(3)",
            ".5",
            "foo(1,)",
            "1 , 2",
            "2 * * 3",
            "sin 30",
        ];
        for input in cases {
            assert!(
                matches!(parse(input), Err(ExprError::Parse { .. })),
                "expected parse error for {input:?}"
            );
        }
    }

    #[test]
    fn parse_errors_carry_positions() {
        // "2 +" — error at end of input (byte 3).
        match parse("2 +") {
            Err(ExprError::Parse { position, .. }) => assert_eq!(position, 3),
            other => panic!("expected parse error, got {other:?}"),
        }
        // "2 & 3" — unexpected character at byte 2.
        match parse("2 & 3") {
            Err(ExprError::Parse { position, message }) => {
                assert_eq!(position, 2);
                assert!(message.contains('&'), "message: {message}");
            }
            other => panic!("expected parse error, got {other:?}"),
        }
        // "sin(1, 2)" — arity error positioned at the function name.
        match parse("sin(1, 2)") {
            Err(ExprError::Parse { position, message }) => {
                assert_eq!(position, 0);
                assert!(message.contains("expects 1 argument"), "message: {message}");
            }
            other => panic!("expected parse error, got {other:?}"),
        }
    }

    #[test]
    fn unknown_function_is_an_eval_error() {
        let env = test_env();
        assert_eq!(
            parse("foo(1)").unwrap().eval(&env),
            Err(ExprError::UnknownFunction {
                name: "foo".to_string()
            })
        );
        assert_eq!(
            parse("1 + bar(2)").unwrap().eval(&env),
            Err(ExprError::UnknownFunction {
                name: "bar".to_string()
            })
        );
        // Function names are only recognized in call position; a bare
        // `sin` is an ordinary self-reference.
        assert_eq!(
            parse("sin").unwrap(),
            Expression::Ref {
                object: None,
                property: "sin".to_string()
            }
        );
    }

    #[test]
    fn unknown_constant_rejects_misspellings() {
        for input in ["PI", "Pi", "E", "pI"] {
            assert_eq!(
                parse(input),
                Err(ExprError::UnknownConstant {
                    name: input.to_string()
                }),
                "input: {input}"
            );
        }
        // Exact spellings still fold.
        assert!(matches!(parse("pi"), Ok(Expression::Number(_))));
        assert!(matches!(parse("e"), Ok(Expression::Number(_))));
        // Unrelated identifiers stay references.
        assert!(matches!(parse("epsilon"), Ok(Expression::Ref { .. })));
    }

    #[test]
    fn division_by_zero() {
        let env = test_env();
        for input in ["1 / 0", "5 % 0", "1 / (2 - 2)", "1 + 1 / 0"] {
            assert_eq!(
                parse(input).unwrap().eval(&env),
                Err(ExprError::DivisionByZero),
                "input: {input}"
            );
        }
        // -0.0 counts as zero too.
        assert_eq!(
            parse("1 / -0").unwrap().eval(&env),
            Err(ExprError::DivisionByZero)
        );
    }

    #[test]
    fn unresolvable_reference_carries_the_reference() {
        let empty = MapEnv::new();
        assert_eq!(
            parse("Box.Length").unwrap().eval(&empty),
            Err(ExprError::UnresolvableReference {
                object: Some("Box".to_string()),
                property: "Length".to_string(),
            })
        );
        // Self-references fail with object: None.
        assert_eq!(
            parse("Height").unwrap().eval(&empty),
            Err(ExprError::UnresolvableReference {
                object: None,
                property: "Height".to_string(),
            })
        );
        // Partial environments fail on the missing binding only.
        let mut partial = MapEnv::new();
        partial.set("Box", "Length", 1.0);
        assert_eq!(
            parse("Box.Length + Box.Width").unwrap().eval(&partial),
            Err(ExprError::UnresolvableReference {
                object: Some("Box".to_string()),
                property: "Width".to_string(),
            })
        );
    }

    #[test]
    fn error_display() {
        assert_eq!(
            ExprError::Parse {
                message: "unexpected character '&'".to_string(),
                position: 7
            }
            .to_string(),
            "parse error at byte 7: unexpected character '&'"
        );
        assert_eq!(
            ExprError::UnknownFunction {
                name: "foo".to_string()
            }
            .to_string(),
            "unknown function: 'foo'"
        );
        assert_eq!(
            ExprError::UnknownConstant {
                name: "PI".to_string()
            }
            .to_string(),
            "unknown constant: 'PI'"
        );
        assert_eq!(ExprError::DivisionByZero.to_string(), "division by zero");
        assert_eq!(
            ExprError::UnresolvableReference {
                object: Some("Box".to_string()),
                property: "Length".to_string(),
            }
            .to_string(),
            "cannot resolve 'Box.Length'"
        );
        assert_eq!(
            ExprError::UnresolvableReference {
                object: None,
                property: "Height".to_string(),
            }
            .to_string(),
            "cannot resolve property 'Height' of the current object"
        );
    }

    #[test]
    fn domain_misses_evaluate_to_nan_like_freecad() {
        let env = test_env();
        assert!(parse("sqrt(-1)").unwrap().eval(&env).unwrap().is_nan());
        assert!(parse("asin(2)").unwrap().eval(&env).unwrap().is_nan());
    }

    // -- environment -------------------------------------------------------

    #[test]
    fn map_env_set_and_chain() {
        let mut env = MapEnv::new();
        assert!(parse("Box.Length").unwrap().eval(&env).is_err());
        env.set("Box", "Length", 3.0).set("Box", "Width", 4.0);
        assert_eq!(
            parse("Box.Length * Box.Width").unwrap().eval(&env),
            Ok(12.0)
        );
        env.set_self("Height", 5.0);
        assert_eq!(parse("Height").unwrap().eval(&env), Ok(5.0));
        // Later bindings overwrite earlier ones.
        env.set("Box", "Length", 10.0);
        assert_eq!(parse("Box.Length").unwrap().eval(&env), Ok(10.0));
    }
}
