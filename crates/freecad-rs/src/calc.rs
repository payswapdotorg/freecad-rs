//! Quantity arithmetic calculator — `freecad-rs calc EXPR`.
//!
//! A small recursive-descent evaluator over [`fc_base::quantity::Quantity`]
//! (the unit-aware number type), mirroring the semantics of FreeCAD's
//! expression bar for pure arithmetic: dimension-checked `+`/`-`, unit
//! combining `*`/`/`, unary sign, parentheses, and scalar powers (`^2`).
//! Property references and functions are the expression engine's domain
//! (`set-expr`); this module is the unit-checked arithmetic surface.
//!
//! Grammar:
//!
//! ```text
//! expr   := term (('+' | '-') term)*
//! term   := factor (('*' | '/') factor)*
//! factor := ('+' | '-') factor | base ('^' base)?
//! base   := '(' expr ')' | QUANTITY
//! ```
//!
//! A `QUANTITY` is a number (with optional decimal point and `e`-exponent)
//! followed by an optional unit token: symbols joined by `*`, `/` or single
//! spaces, each with an optional `^integer` power, e.g. `25 mm`, `9.8 m/s^2`,
//! `2 kg m / s^2`, `30°`, or a bare `42`.

use std::fmt::Write as _;

use fc_base::quantity::Quantity;

/// The UTF-8 encoding of the degree sign `°` (used in unit tokens).
const DEGREE_SIGN: [u8; 2] = [0xC2, 0xB0];

/// One token of the calc grammar.
#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Qty(Quantity),
    Plus,
    Minus,
    Star,
    Slash,
    Caret,
    LParen,
    RParen,
}

/// Tokenize `input`; returns the tokens or a human-readable error.
fn lex(input: &str) -> Result<Vec<Tok>, String> {
    let b = input.as_bytes();
    let mut i = 0;
    let mut out = Vec::new();
    loop {
        while i < b.len() && (b[i] == b' ' || b[i] == b'\t') {
            i += 1;
        }
        if i >= b.len() {
            break;
        }
        match b[i] {
            b'+' => out.push(Tok::Plus),
            b'-' => out.push(Tok::Minus),
            b'*' => out.push(Tok::Star),
            b'/' => out.push(Tok::Slash),
            b'^' => out.push(Tok::Caret),
            b'(' => out.push(Tok::LParen),
            b')' => out.push(Tok::RParen),
            c if c.is_ascii_digit() || c == b'.' => {
                let (q, next) = scan_quantity(input, i)?;
                out.push(Tok::Qty(q));
                i = next;
                continue;
            }
            c => {
                return Err(format!(
                    "unexpected character {:?} at byte {}",
                    c as char, i
                ))
            }
        }
        i += 1;
    }
    Ok(out)
}

/// Scan a `QUANTITY` starting at byte `start` (a digit or `.`): the number,
/// then — separated by at most one space — an optional unit token, which
/// runs until an operator character (`+ - * / ^ ( )`) or end of input.
/// Returns the parsed quantity and the offset just past it.
fn scan_quantity(input: &str, start: usize) -> Result<(Quantity, usize), String> {
    let b = input.as_bytes();
    let mut i = start;
    // Number: digits [. digits] [eE [+-] digits] (exponent only when digits
    // follow, so `2e` leaves `e` to the unit scan as FreeCAD does).
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    if i < b.len() && b[i] == b'.' {
        i += 1;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        let mut j = i + 1;
        if j < b.len() && (b[j] == b'+' || b[j] == b'-') {
            j += 1;
        }
        if j < b.len() && b[j].is_ascii_digit() {
            i = j;
            while i < b.len() && b[i].is_ascii_digit() {
                i += 1;
            }
        }
    }
    let number_end = i;
    // Optional single space between number and unit.
    let mut unit_start = number_end;
    if unit_start < b.len() && b[unit_start] == b' ' {
        unit_start += 1;
    }
    let is_unit_start = unit_start < b.len()
        && (b[unit_start].is_ascii_alphabetic() || b[unit_start..].starts_with(&DEGREE_SIGN));
    if !is_unit_start {
        let num: &str = &input[start..number_end];
        let q = Quantity::parse(num).map_err(|e| e.to_string())?;
        return Ok((q, number_end));
    }
    // Unit token: symbols joined by `*`, `/` or single spaces; each symbol
    // is letters (or `°`) with an optional `^[+-]?digits` power.
    let mut i = unit_start;
    loop {
        while i < b.len() {
            if b[i..].starts_with(&DEGREE_SIGN) {
                i += DEGREE_SIGN.len();
            } else if b[i].is_ascii_alphabetic() {
                i += 1;
            } else {
                break;
            }
        }
        if i < b.len() && b[i] == b'^' {
            i += 1;
            if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
                i += 1;
            }
            while i < b.len() && b[i].is_ascii_digit() {
                i += 1;
            }
        }
        // A separator (`*` or `/`, optionally spaced) continues the token.
        let mut j = i;
        while j < b.len() && b[j] == b' ' {
            j += 1;
        }
        if j < b.len() && (b[j] == b'*' || b[j] == b'/') {
            let mut k = j + 1;
            while k < b.len() && b[k] == b' ' {
                k += 1;
            }
            let continues =
                k < b.len() && (b[k].is_ascii_alphabetic() || b[k..].starts_with(&DEGREE_SIGN));
            if continues {
                i = k;
                continue;
            }
        }
        break;
    }
    let text = input[start..i].trim_end();
    let q = Quantity::parse(text).map_err(|e| e.to_string())?;
    Ok((q, i))
}

/// Evaluate a pure-quantity arithmetic expression.
pub fn eval(input: &str) -> Result<Quantity, String> {
    let toks = lex(input)?;
    let mut p = Parser { toks, pos: 0 };
    if p.toks.is_empty() {
        return Err("empty expression".into());
    }
    let q = p.expr()?;
    if p.pos != p.toks.len() {
        return Err(format!(
            "unexpected trailing input after the expression (token #{})",
            p.pos
        ));
    }
    Ok(q)
}

struct Parser {
    toks: Vec<Tok>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }

    fn expr(&mut self) -> Result<Quantity, String> {
        let mut lhs = self.term()?;
        while let Some(t) = self.peek() {
            match t {
                Tok::Plus => {
                    self.pos += 1;
                    let rhs = self.term()?;
                    lhs = lhs.try_add(rhs).map_err(|e| e.to_string())?;
                }
                Tok::Minus => {
                    self.pos += 1;
                    let rhs = self.term()?;
                    lhs = lhs.try_sub(rhs).map_err(|e| e.to_string())?;
                }
                _ => break,
            }
        }
        Ok(lhs)
    }

    fn term(&mut self) -> Result<Quantity, String> {
        let mut lhs = self.factor()?;
        while let Some(t) = self.peek() {
            match t {
                Tok::Star => {
                    self.pos += 1;
                    let rhs = self.factor()?;
                    lhs = lhs * rhs;
                }
                Tok::Slash => {
                    self.pos += 1;
                    let rhs = self.factor()?;
                    if rhs.value() == 0.0 {
                        return Err("division by zero".into());
                    }
                    lhs = lhs / rhs;
                }
                _ => break,
            }
        }
        Ok(lhs)
    }

    fn factor(&mut self) -> Result<Quantity, String> {
        match self.peek() {
            Some(Tok::Plus) => {
                self.pos += 1;
                self.factor()
            }
            Some(Tok::Minus) => {
                self.pos += 1;
                let q = self.factor()?;
                Ok(-q)
            }
            _ => {
                let base = self.base()?;
                if matches!(self.peek(), Some(Tok::Caret)) {
                    self.pos += 1;
                    let exp = self.base()?;
                    let e = exp.value();
                    if !exp.unit().is_dimensionless() || e.fract() != 0.0 {
                        return Err("`^` needs an integer, dimensionless exponent".into());
                    }
                    // NaN was rejected by the `fract()` check above, so
                    // the range comparisons are total here.
                    let too_big =
                        matches!(e.partial_cmp(&127.0), Some(std::cmp::Ordering::Greater));
                    let too_small =
                        matches!(e.partial_cmp(&-127.0), Some(std::cmp::Ordering::Less));
                    if too_big || too_small {
                        return Err("exponent out of range".into());
                    }
                    return Ok(base.powi(e as i32));
                }
                Ok(base)
            }
        }
    }

    fn base(&mut self) -> Result<Quantity, String> {
        match self.peek() {
            Some(Tok::LParen) => {
                self.pos += 1;
                let q = self.expr()?;
                if !matches!(self.peek(), Some(Tok::RParen)) {
                    return Err("missing `)`".into());
                }
                self.pos += 1;
                Ok(q)
            }
            Some(Tok::Qty(q)) => {
                let q = *q;
                self.pos += 1;
                Ok(q)
            }
            other => Err(format!(
                "expected a quantity or `(`, found {}",
                describe(other)
            )),
        }
    }
}

fn describe(tok: Option<&Tok>) -> &'static str {
    match tok {
        Some(Tok::Plus) => "`+`",
        Some(Tok::Minus) => "`-`",
        Some(Tok::Star) => "`*`",
        Some(Tok::Slash) => "`/`",
        Some(Tok::Caret) => "`^`",
        Some(Tok::LParen) => "`(`",
        Some(Tok::RParen) => "`)`",
        Some(Tok::Qty(_)) => "a quantity",
        None => "end of input",
    }
}

/// Render the evaluation result for the CLI: the value in canonical units
/// plus, where useful, common alternate readings.
pub fn format_result(q: &Quantity) -> String {
    let mut s = String::new();
    let _ = write!(s, "{q}");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dimension_checked_addition() {
        let q = eval("25 mm + 3 cm").unwrap();
        assert!((q.value() - 55.0).abs() < 1e-12, "got {q}");
    }

    #[test]
    fn mismatched_addition_is_an_error() {
        assert!(eval("1 m + 1 kg").is_err());
    }

    #[test]
    fn compound_units_and_precedence() {
        // 9.8 m/s^2 * 2 kg: canonical storage is mm·kg/s^2, so
        // 9800 mm/s^2 * 2 kg = 19600 mm·kg/s^2.
        let q = eval("9.8 m/s^2 * 2 kg").unwrap();
        assert!((q.value() - 19_600.0).abs() < 1e-6, "got {q}");
    }

    #[test]
    fn division_combines_units() {
        // 10 m / 5 s = 2000 mm/s in canonical units.
        let q = eval("10 m / 5 s").unwrap();
        assert!((q.value() - 2000.0).abs() < 1e-9, "got {q}");
    }

    #[test]
    fn unary_minus_and_parens() {
        let q = eval("-(2 mm + 3 mm)").unwrap();
        assert!((q.value() + 5.0).abs() < 1e-12, "got {q}");
    }

    #[test]
    fn power_operator() {
        let q = eval("2 mm^3 ^ 2").unwrap();
        // (2 mm^3)^2 = 4 mm^6 — value 4, unit mm^6.
        assert!((q.value() - 4.0).abs() < 1e-12, "got {q}");
        assert_eq!(q.unit().power(fc_base::quantity::Dimension::Length), 6);
    }

    #[test]
    fn bare_numbers_are_dimensionless() {
        let q = eval("2 - 3 * 4").unwrap();
        assert!((q.value() + 10.0).abs() < 1e-12, "got {q}");
    }

    #[test]
    fn scientific_notation_with_unit() {
        // Canonical mass unit is kg: 1e-3 kg stays 0.001 kg.
        let q = eval("1e-3 kg").unwrap();
        assert!((q.value() - 1e-3).abs() < 1e-12, "got {q}");
    }

    #[test]
    fn degree_symbol() {
        let q = eval("30° + 30°").unwrap();
        assert!((q.value() - 60.0).abs() < 1e-9, "got {q}");
    }

    #[test]
    fn errors_are_actionable() {
        assert!(eval("").is_err());
        assert!(eval("1 m +").is_err());
        assert!(eval("(1 m").is_err());
        assert!(eval("1 m & 2").is_err());
        assert!(eval("1 / 0").is_err());
    }

    #[test]
    fn trailing_input_rejected() {
        assert!(eval("1 m 2").is_err());
    }
}
