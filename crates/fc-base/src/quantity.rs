//! Units and quantities, mirroring the architecture of FreeCAD's
//! `Base::Quantity` / `Base::Unit`.
//!
//! # Model
//!
//! A [`Unit`] is a pure dimensional signature: one signed `i8` power per
//! base dimension — the seven SI base dimensions plus angle. A
//! [`Quantity`] pairs an `f64` value with such a unit. As in FreeCAD, the
//! unit records only the *dimension*; the value is stored in canonical
//! *internal* units:
//!
//! | dimension           | canonical | | dimension             | canonical |
//! |---------------------|-----------|-|-----------------------|-----------|
//! | length              | `mm`      | | amount of substance   | `mol`     |
//! | mass                | `kg`      | | luminous intensity    | `cd`      |
//! | time                | `s`       | | angle                 | `deg`     |
//! | electric current    | `A`       | |                       |           |
//! | temperature         | `K`       | |                       |           |
//!
//! Conversions from the named units of the conversion table (`in`, `ft`,
//! `mi`, `oz`, `lb`, `min`, `h`, `rad`, `C`, `F`, ...) are applied at
//! parse time, so two quantities with the same unit are always directly
//! comparable: `1 in` and `25.4 mm` are the same quantity. Use
//! [`Quantity::value_in`] to express a value in a named unit again.
//!
//! # Parsing
//!
//! [`Quantity::from_str`] accepts FreeCAD-style quantity strings:
//!
//! * a bare number — optionally signed, optionally in scientific
//!   notation (`"3"`, `"-2.5"`, `"1e3"`, `"1.2E-4"`) — is dimensionless;
//! * a number followed by a unit expression, with optional whitespace
//!   (`"3mm"`, `"25 mm"`, `"1.5 m"`, `"30°"`);
//! * unit tokens combined with `*`, `/` and integer `^` exponents; bare
//!   whitespace between tokens means multiplication, and `/` binds to the
//!   immediately following term: `"2 kg m / s^2"`, `"9.8 m/s^2"`,
//!   `"5 mm^3"`, `"1 s^-2"`.
//!
//! Unknown unit tokens are rejected with an error listing the accepted
//! set (case-sensitive; `um`, `µm` and `μm` are synonyms).
//!
//! # Affine temperature units
//!
//! `C` and `F` are *affine* (offset + scale), not multiplicative:
//! `K = C + 273.15` and `K = (F + 459.67) * 5/9`. They are therefore only
//! meaningful as a standalone level (`"100 C"`, `"-40 F"`) and are
//! rejected inside compound expressions (`"1 C^2"`, `"2 C K"`), where the
//! offset would be ill-defined — use `K` there.
//!
//! # Display
//!
//! [`Display`] is a canonical form, not a locale formatting system: the
//! value in canonical units, a space, then the unit built from the
//! canonical dimension symbols joined with `*` (numerator) and `/`
//! (denominator), e.g. `"25 mm"`, `"1 mm^2/s"`, `"1500 mm*kg/s^2"`.
//! Dimensionless quantities print as the bare number.
//!
//! # Examples
//!
//! ```
//! use std::str::FromStr;
//!
//! use fc_base::quantity::{Dimension, Quantity, Unit};
//!
//! let q = Quantity::from_str("25 mm").unwrap();
//! assert_eq!(q.value(), 25.0);
//! assert_eq!(q.unit(), Unit::meter());
//!
//! // Values are held in canonical units (mm, kg, s, deg, K, ...).
//! let g = Quantity::from_str("9.81 m/s^2").unwrap();
//! let t = Quantity::from_str("2 s").unwrap();
//! let d = g * t * t;
//! assert_eq!(d.unit().power(Dimension::Length), 1);
//! assert!((d.value() - 39240.0).abs() < 1e-9);
//!
//! // Parsing converts through the unit table ...
//! let inch = Quantity::from_str("1 in").unwrap();
//! assert_eq!(inch.value(), 25.4);
//! // ... and `value_in` converts back out.
//! assert!((inch.value_in("cm").unwrap() - 2.54).abs() < 1e-12);
//!
//! // Adding quantities of different dimensions is a checked error.
//! let err = Quantity::from_str("1 s").unwrap().try_add(inch).unwrap_err();
//! assert!(err.to_string().contains("differ"));
//! ```

use std::cmp::Ordering;
use std::fmt;
use std::ops::{Div, Mul, Neg};
use std::str::FromStr;

/// Degrees per radian: one radian is `180 / pi` degrees (the canonical
/// angle unit is the degree).
const DEG_PER_RAD: f64 = 180.0 / std::f64::consts::PI;

/// Fahrenheit is affine: `K = F * 5/9 + offset`, i.e.
/// `K = (F + 459.67) * 5/9`.
const FAHRENHEIT_SCALE: f64 = 5.0 / 9.0;
const FAHRENHEIT_OFFSET_K: f64 = 459.67 * 5.0 / 9.0;

/// Largest magnitude a unit power may take (kept inside `i8` with room to
/// negate).
const MAX_POWER: i64 = 127;

/// One of the eight base dimensions a [`Unit`] tracks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Dimension {
    /// Length (canonical unit: millimetre).
    Length,
    /// Mass (canonical unit: kilogram).
    Mass,
    /// Time (canonical unit: second).
    Time,
    /// Electric current (canonical unit: ampere).
    ElectricCurrent,
    /// Thermodynamic temperature (canonical unit: kelvin).
    Temperature,
    /// Amount of substance (canonical unit: mole).
    AmountOfSubstance,
    /// Luminous intensity (canonical unit: candela).
    LuminousIntensity,
    /// Plane angle (canonical unit: degree).
    Angle,
}

impl Dimension {
    /// All dimensions, in signature/display order.
    const ALL: [Dimension; 8] = [
        Dimension::Length,
        Dimension::Mass,
        Dimension::Time,
        Dimension::ElectricCurrent,
        Dimension::Temperature,
        Dimension::AmountOfSubstance,
        Dimension::LuminousIntensity,
        Dimension::Angle,
    ];

    /// Position of this dimension inside a [`Unit`] signature.
    const fn index(self) -> usize {
        match self {
            Dimension::Length => 0,
            Dimension::Mass => 1,
            Dimension::Time => 2,
            Dimension::ElectricCurrent => 3,
            Dimension::Temperature => 4,
            Dimension::AmountOfSubstance => 5,
            Dimension::LuminousIntensity => 6,
            Dimension::Angle => 7,
        }
    }

    /// Canonical symbol used when displaying units.
    const fn symbol(self) -> &'static str {
        match self {
            Dimension::Length => "mm",
            Dimension::Mass => "kg",
            Dimension::Time => "s",
            Dimension::ElectricCurrent => "A",
            Dimension::Temperature => "K",
            Dimension::AmountOfSubstance => "mol",
            Dimension::LuminousIntensity => "cd",
            Dimension::Angle => "deg",
        }
    }
}

/// A dimensional signature: signed powers for the eight base dimensions.
///
/// `Unit` carries no scale — `mm` and `in` are the same unit (`Length^1`);
/// the scale lives in the conversion table applied when parsing (see the
/// [module docs](self)). Powers are saturated at ±127 on overflow.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Unit {
    powers: [i8; 8],
}

impl Unit {
    /// The dimensionless unit: all powers zero.
    pub const fn dimensionless() -> Unit {
        Unit { powers: [0; 8] }
    }

    /// Length to the first power (canonical unit: millimetre).
    pub const fn meter() -> Unit {
        Unit::from_power(Dimension::Length, 1)
    }

    /// Mass to the first power (canonical unit: kilogram).
    pub const fn kilogram() -> Unit {
        Unit::from_power(Dimension::Mass, 1)
    }

    /// Time to the first power (canonical unit: second).
    pub const fn second() -> Unit {
        Unit::from_power(Dimension::Time, 1)
    }

    /// Electric current to the first power (canonical unit: ampere).
    pub const fn ampere() -> Unit {
        Unit::from_power(Dimension::ElectricCurrent, 1)
    }

    /// Thermodynamic temperature to the first power (canonical unit: kelvin).
    pub const fn kelvin() -> Unit {
        Unit::from_power(Dimension::Temperature, 1)
    }

    /// Amount of substance to the first power (canonical unit: mole).
    pub const fn mole() -> Unit {
        Unit::from_power(Dimension::AmountOfSubstance, 1)
    }

    /// Luminous intensity to the first power (canonical unit: candela).
    pub const fn candela() -> Unit {
        Unit::from_power(Dimension::LuminousIntensity, 1)
    }

    /// Plane angle to the first power (canonical unit: degree).
    pub const fn degree() -> Unit {
        Unit::from_power(Dimension::Angle, 1)
    }

    /// A unit with a single dimension raised to `power`.
    const fn from_power(dim: Dimension, power: i8) -> Unit {
        let mut powers = [0i8; 8];
        powers[dim.index()] = power;
        Unit { powers }
    }

    /// The power of `dim` in this signature (0 when absent).
    pub fn power(&self, dim: Dimension) -> i8 {
        self.powers[dim.index()]
    }

    /// True when every power is zero.
    pub fn is_dimensionless(&self) -> bool {
        self.powers == [0; 8]
    }

    /// Raise this unit to an integer power: every dimension power is
    /// multiplied by `exp` (saturated at ±127). `exp` may be negative.
    pub fn powi(self, exp: i32) -> Unit {
        Unit {
            powers: self
                .powers
                .map(|p| clamp_power(i64::from(p) * i64::from(exp))),
        }
    }
}

/// Clamp a computed power into the representable range (see `MAX_POWER`).
/// The clamp guarantees the final `as i8` cannot truncate.
fn clamp_power(p: i64) -> i8 {
    p.clamp(-MAX_POWER, MAX_POWER) as i8
}

impl Mul for Unit {
    type Output = Unit;

    /// Multiplication adds powers, dimension by dimension.
    fn mul(self, rhs: Unit) -> Unit {
        Unit {
            powers: std::array::from_fn(|i| {
                clamp_power(i64::from(self.powers[i]) + i64::from(rhs.powers[i]))
            }),
        }
    }
}

impl Div for Unit {
    type Output = Unit;

    /// Division subtracts powers, dimension by dimension.
    fn div(self, rhs: Unit) -> Unit {
        Unit {
            powers: std::array::from_fn(|i| {
                clamp_power(i64::from(self.powers[i]) - i64::from(rhs.powers[i]))
            }),
        }
    }
}

impl fmt::Display for Unit {
    /// Canonical form: positive-power dimensions joined with `*`, then `/`
    /// and the negative-power dimensions, e.g. `mm^2`, `mm/s`,
    /// `mm*kg/s^2`. The dimensionless unit prints as `1`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut pos: Vec<String> = Vec::new();
        let mut neg: Vec<String> = Vec::new();
        for dim in Dimension::ALL {
            let p = i32::from(self.powers[dim.index()]);
            if p > 0 {
                pos.push(dimension_term(dim, p));
            } else if p < 0 {
                neg.push(dimension_term(dim, p));
            }
        }
        if pos.is_empty() && neg.is_empty() {
            return write!(f, "1");
        }
        let num = if pos.is_empty() {
            "1".to_string()
        } else {
            pos.join("*")
        };
        if neg.is_empty() {
            write!(f, "{num}")
        } else {
            let den = neg.join("*");
            write!(f, "{num}/{den}")
        }
    }
}

/// Render one dimension's contribution to a unit string, e.g. `mm` or
/// `mm^3` (the exponent is always positive here; the sign is carried by
/// the `/` separator).
fn dimension_term(dim: Dimension, power: i32) -> String {
    if power.unsigned_abs() == 1 {
        dim.symbol().to_string()
    } else {
        format!("{}^{}", dim.symbol(), power.unsigned_abs())
    }
}

/// Everything that can go wrong with quantities.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QuantityError {
    /// The input string is not a well-formed quantity.
    InvalidQuantity {
        /// The offending input.
        input: String,
        /// What exactly is wrong with it.
        reason: String,
    },
    /// A unit token that is not in the conversion table.
    UnknownUnit {
        /// The token as written in the input.
        token: String,
    },
    /// An operation between quantities of different dimensions.
    DimensionMismatch {
        /// What the caller tried to do.
        operation: &'static str,
        /// The left-hand operand's unit.
        left: Unit,
        /// The right-hand operand's unit.
        right: Unit,
    },
}

impl fmt::Display for QuantityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            QuantityError::InvalidQuantity { input, reason } => {
                write!(f, "invalid quantity '{input}': {reason}")
            }
            QuantityError::UnknownUnit { token } => write!(
                f,
                "unknown unit token '{token}'; accepted (case-sensitive): {list}",
                token = token,
                list = accepted_units()
            ),
            QuantityError::DimensionMismatch {
                operation,
                left,
                right,
            } => write!(
                f,
                "cannot {operation}: dimensions differ ({left} vs {right})"
            ),
        }
    }
}

impl std::error::Error for QuantityError {}

/// A scalar value paired with a dimensional unit.
///
/// The value is expressed in the canonical unit of its dimension (mm, kg,
/// s, A, K, mol, cd, deg — see the [module docs](self)); parsing applies
/// the conversion table, so quantities of equal value and unit are
/// physically equal.
#[derive(Clone, Copy, Debug)]
pub struct Quantity {
    value: f64,
    unit: Unit,
}

impl Quantity {
    /// A quantity of `value` in `unit`.
    ///
    /// `value` must already be expressed in the canonical unit of
    /// `unit`'s dimension — use [`Quantity::from_str`] to convert named
    /// units.
    pub const fn new(value: f64, unit: Unit) -> Quantity {
        Quantity { value, unit }
    }

    /// A dimensionless quantity (a plain number).
    pub const fn dimensionless(value: f64) -> Quantity {
        Quantity {
            value,
            unit: Unit::dimensionless(),
        }
    }

    /// The value in canonical units.
    pub const fn value(&self) -> f64 {
        self.value
    }

    /// The dimensional signature of this quantity.
    pub const fn unit(&self) -> Unit {
        self.unit
    }

    /// Add two quantities. Requires the same dimension, otherwise a
    /// [`QuantityError::DimensionMismatch`] showing both units.
    ///
    /// (Spelled `try_add` rather than `add` because the failure is a
    /// recoverable `Result`, not a panic.)
    pub fn try_add(self, rhs: Quantity) -> Result<Quantity, QuantityError> {
        if self.unit != rhs.unit {
            return Err(mismatch("add quantities", self.unit, rhs.unit));
        }
        Ok(Quantity::new(self.value + rhs.value, self.unit))
    }

    /// Subtract `rhs` from `self`. Requires the same dimension, otherwise
    /// a [`QuantityError::DimensionMismatch`] showing both units.
    pub fn try_sub(self, rhs: Quantity) -> Result<Quantity, QuantityError> {
        if self.unit != rhs.unit {
            return Err(mismatch("subtract quantities", self.unit, rhs.unit));
        }
        Ok(Quantity::new(self.value - rhs.value, self.unit))
    }

    /// Raise to an integer power: the value is powered, every unit power
    /// is scaled by `exp` (saturated at ±127). Negative `exp` is allowed.
    pub fn powi(self, exp: i32) -> Quantity {
        Quantity {
            value: self.value.powi(exp),
            unit: self.unit.powi(exp),
        }
    }

    /// Check that `target` has the same dimension as this quantity and
    /// return its canonical representation.
    ///
    /// Because values are always stored in canonical units (see the
    /// [module docs](self)), the conversion itself happens at parse time;
    /// this method validates the target dimension — differing dimensions
    /// are a [`QuantityError::DimensionMismatch`] — and returns the
    /// canonical form. To express a value in a named unit (`km`, `rad`,
    /// `F`, ...), use [`Quantity::value_in`].
    pub fn convert_to(self, target: Unit) -> Result<Quantity, QuantityError> {
        if self.unit != target {
            return Err(mismatch("convert quantity", self.unit, target));
        }
        Ok(self)
    }

    /// The value of this quantity expressed in the named unit `symbol`
    /// (e.g. `"ft"`, `"rad"`, `"F"`), applying the inverse of the parse
    /// conversion — including the affine offset for `C` and `F`.
    ///
    /// Only single named units are supported, and only for quantities
    /// that are a plain power-one of that dimension.
    ///
    /// ```rust
    /// # use std::str::FromStr;
    /// # use fc_base::quantity::Quantity;
    /// let boiling = Quantity::from_str("100 C").unwrap(); // stored as 373.15 K
    /// assert!((boiling.value_in("F").unwrap() - 212.0).abs() < 1e-9);
    /// ```
    pub fn value_in(self, symbol: &str) -> Result<f64, QuantityError> {
        let def = lookup_unit(symbol).ok_or_else(|| QuantityError::UnknownUnit {
            token: symbol.to_owned(),
        })?;
        let expected = Unit::from_power(def.dim, 1);
        if self.unit != expected {
            return Err(mismatch(
                "express quantity in a named unit",
                self.unit,
                expected,
            ));
        }
        if def.offset != 0.0 {
            Ok((self.value - def.offset) / def.factor)
        } else {
            Ok(self.value / def.factor)
        }
    }

    /// Parse a FreeCAD-style quantity string (see the
    /// [module docs](self) for the accepted grammar).
    pub fn parse(s: &str) -> Result<Quantity, QuantityError> {
        Quantity::from_str(s)
    }
}

impl FromStr for Quantity {
    type Err = QuantityError;

    fn from_str(s: &str) -> Result<Quantity, QuantityError> {
        let trimmed = s.trim();
        if trimmed.is_empty() {
            return Err(invalid(s, "empty input"));
        }
        let (num_text, rest) = scan_number(trimmed).map_err(|reason| invalid(s, reason))?;
        let value: f64 = num_text
            .trim_end_matches('.')
            .parse()
            .map_err(|_| invalid(s, "not a valid number"))?;
        let (value, unit) = parse_unit_expression(s, rest, value)?;
        Ok(Quantity { value, unit })
    }
}

impl Mul for Quantity {
    type Output = Quantity;

    /// Multiply values and combine units (powers add).
    fn mul(self, rhs: Quantity) -> Quantity {
        Quantity::new(self.value * rhs.value, self.unit * rhs.unit)
    }
}

impl Mul<f64> for Quantity {
    type Output = Quantity;

    /// Scale by a plain scalar.
    fn mul(self, rhs: f64) -> Quantity {
        Quantity::new(self.value * rhs, self.unit)
    }
}

impl Mul<Quantity> for f64 {
    type Output = Quantity;

    /// Scale a quantity by a plain scalar.
    fn mul(self, rhs: Quantity) -> Quantity {
        Quantity::new(self * rhs.value, rhs.unit)
    }
}

impl Div for Quantity {
    type Output = Quantity;

    /// Divide values and combine units (powers subtract).
    fn div(self, rhs: Quantity) -> Quantity {
        Quantity::new(self.value / rhs.value, self.unit / rhs.unit)
    }
}

impl Div<f64> for Quantity {
    type Output = Quantity;

    /// Scale by the inverse of a plain scalar.
    fn div(self, rhs: f64) -> Quantity {
        Quantity::new(self.value / rhs, self.unit)
    }
}

impl Neg for Quantity {
    type Output = Quantity;

    fn neg(self) -> Quantity {
        Quantity::new(-self.value, self.unit)
    }
}

impl PartialEq for Quantity {
    /// Exact equality of value and unit (no f64 tolerance, by design).
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value && self.unit == other.unit
    }
}

impl PartialOrd for Quantity {
    /// Ordering is only defined between quantities of the same dimension;
    /// comparing different dimensions yields `None` instead of a bogus
    /// answer.
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        if self.unit == other.unit {
            self.value.partial_cmp(&other.value)
        } else {
            None
        }
    }
}

impl fmt::Display for Quantity {
    /// Canonical form `{value} {unit}` (e.g. `25 mm`); dimensionless
    /// quantities print as the bare number.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.unit.is_dimensionless() {
            write!(f, "{v}", v = self.value)
        } else {
            write!(f, "{v} {u}", v = self.value, u = self.unit)
        }
    }
}

/// Build a dimension-mismatch error.
fn mismatch(operation: &'static str, left: Unit, right: Unit) -> QuantityError {
    QuantityError::DimensionMismatch {
        operation,
        left,
        right,
    }
}

/// Build an invalid-quantity error.
fn invalid(input: &str, reason: impl Into<String>) -> QuantityError {
    QuantityError::InvalidQuantity {
        input: input.to_owned(),
        reason: reason.into(),
    }
}

/// One entry of the unit conversion table.
struct UnitDef {
    symbol: &'static str,
    dim: Dimension,
    /// How many canonical units one of this symbol is.
    factor: f64,
    /// Affine offset in canonical units (`C`, `F`); zero for linear units.
    offset: f64,
}

/// The conversion table: every accepted unit symbol with its factor to
/// the canonical unit of its dimension.
const UNIT_TABLE: &[UnitDef] = &[
    // length (canonical: mm)
    UnitDef {
        symbol: "nm",
        dim: Dimension::Length,
        factor: 1e-6,
        offset: 0.0,
    },
    UnitDef {
        symbol: "um",
        dim: Dimension::Length,
        factor: 1e-3,
        offset: 0.0,
    },
    UnitDef {
        symbol: "µm",
        dim: Dimension::Length,
        factor: 1e-3,
        offset: 0.0,
    },
    UnitDef {
        symbol: "μm",
        dim: Dimension::Length,
        factor: 1e-3,
        offset: 0.0,
    },
    UnitDef {
        symbol: "mm",
        dim: Dimension::Length,
        factor: 1.0,
        offset: 0.0,
    },
    UnitDef {
        symbol: "cm",
        dim: Dimension::Length,
        factor: 10.0,
        offset: 0.0,
    },
    UnitDef {
        symbol: "m",
        dim: Dimension::Length,
        factor: 1e3,
        offset: 0.0,
    },
    UnitDef {
        symbol: "km",
        dim: Dimension::Length,
        factor: 1e6,
        offset: 0.0,
    },
    UnitDef {
        symbol: "in",
        dim: Dimension::Length,
        factor: 25.4,
        offset: 0.0,
    },
    UnitDef {
        symbol: "ft",
        dim: Dimension::Length,
        factor: 304.8,
        offset: 0.0,
    },
    UnitDef {
        symbol: "yd",
        dim: Dimension::Length,
        factor: 914.4,
        offset: 0.0,
    },
    UnitDef {
        symbol: "mi",
        dim: Dimension::Length,
        factor: 1_609_344.0,
        offset: 0.0,
    },
    // mass (canonical: kg)
    UnitDef {
        symbol: "mg",
        dim: Dimension::Mass,
        factor: 1e-6,
        offset: 0.0,
    },
    UnitDef {
        symbol: "g",
        dim: Dimension::Mass,
        factor: 1e-3,
        offset: 0.0,
    },
    UnitDef {
        symbol: "kg",
        dim: Dimension::Mass,
        factor: 1.0,
        offset: 0.0,
    },
    UnitDef {
        symbol: "t",
        dim: Dimension::Mass,
        factor: 1e3,
        offset: 0.0,
    },
    UnitDef {
        symbol: "oz",
        dim: Dimension::Mass,
        factor: 0.028349523125,
        offset: 0.0,
    },
    UnitDef {
        symbol: "lb",
        dim: Dimension::Mass,
        factor: 0.45359237,
        offset: 0.0,
    },
    // time (canonical: s)
    UnitDef {
        symbol: "ms",
        dim: Dimension::Time,
        factor: 1e-3,
        offset: 0.0,
    },
    UnitDef {
        symbol: "s",
        dim: Dimension::Time,
        factor: 1.0,
        offset: 0.0,
    },
    UnitDef {
        symbol: "min",
        dim: Dimension::Time,
        factor: 60.0,
        offset: 0.0,
    },
    UnitDef {
        symbol: "h",
        dim: Dimension::Time,
        factor: 3600.0,
        offset: 0.0,
    },
    // angle (canonical: deg)
    UnitDef {
        symbol: "deg",
        dim: Dimension::Angle,
        factor: 1.0,
        offset: 0.0,
    },
    UnitDef {
        symbol: "°",
        dim: Dimension::Angle,
        factor: 1.0,
        offset: 0.0,
    },
    UnitDef {
        symbol: "rad",
        dim: Dimension::Angle,
        factor: DEG_PER_RAD,
        offset: 0.0,
    },
    // temperature (canonical: K; C and F are affine)
    UnitDef {
        symbol: "K",
        dim: Dimension::Temperature,
        factor: 1.0,
        offset: 0.0,
    },
    UnitDef {
        symbol: "C",
        dim: Dimension::Temperature,
        factor: 1.0,
        offset: 273.15,
    },
    UnitDef {
        symbol: "F",
        dim: Dimension::Temperature,
        factor: FAHRENHEIT_SCALE,
        offset: FAHRENHEIT_OFFSET_K,
    },
    // single-unit dimensions
    UnitDef {
        symbol: "A",
        dim: Dimension::ElectricCurrent,
        factor: 1.0,
        offset: 0.0,
    },
    UnitDef {
        symbol: "mol",
        dim: Dimension::AmountOfSubstance,
        factor: 1.0,
        offset: 0.0,
    },
    UnitDef {
        symbol: "cd",
        dim: Dimension::LuminousIntensity,
        factor: 1.0,
        offset: 0.0,
    },
];

/// Case-sensitive lookup of a unit symbol.
fn lookup_unit(token: &str) -> Option<&'static UnitDef> {
    UNIT_TABLE.iter().find(|d| d.symbol == token)
}

/// Comma-separated list of every accepted unit symbol, for error
/// messages.
fn accepted_units() -> String {
    UNIT_TABLE
        .iter()
        .map(|d| d.symbol)
        .collect::<Vec<_>>()
        .join(", ")
}

/// Characters a unit token may consist of (ASCII letters plus the degree
/// sign and the two micro signs).
fn is_unit_char(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '°' || c == 'µ' || c == 'μ'
}

/// Scan the leading number off `s` (already trimmed): optional sign,
/// digits with optional fraction, optional `[eE][+-]?digits` — the
/// exponent is only consumed when digits actually follow, so `"1e3"`
/// scans as 1000 while `"5e"` leaves the `e` to lex as a unit token.
/// Returns the numeric text and the remainder.
fn scan_number(s: &str) -> Result<(&str, &str), String> {
    let bytes = s.as_bytes();
    let mut i = 0usize;
    if bytes.first() == Some(&b'+') || bytes.first() == Some(&b'-') {
        i = 1;
    }
    let mut int_digits = 0usize;
    while bytes.get(i).is_some_and(|b| b.is_ascii_digit()) {
        i += 1;
        int_digits += 1;
    }
    let mut frac_digits = 0usize;
    if bytes.get(i) == Some(&b'.') {
        i += 1;
        while bytes.get(i).is_some_and(|b| b.is_ascii_digit()) {
            i += 1;
            frac_digits += 1;
        }
    }
    if int_digits + frac_digits == 0 {
        return Err("missing numeric value".to_owned());
    }
    if matches!(bytes.get(i), Some(b'e') | Some(b'E')) {
        let mut j = i + 1;
        if matches!(bytes.get(j), Some(b'+') | Some(b'-')) {
            j += 1;
        }
        if bytes.get(j).is_some_and(|b| b.is_ascii_digit()) {
            while bytes.get(j).is_some_and(|b| b.is_ascii_digit()) {
                j += 1;
            }
            i = j;
        }
    }
    Ok((&s[..i], &s[i..]))
}

/// Scan a `^` exponent (the caret is already consumed): optional sign,
/// then digits, then the remainder. Bounded to ±127.
fn scan_exponent(s: &str) -> Result<(i32, &str), String> {
    let mut rest = s;
    let mut negative = false;
    if rest.starts_with('+') {
        rest = &rest[1..];
    } else if rest.starts_with('-') {
        negative = true;
        rest = &rest[1..];
    }
    let digits_len = rest.bytes().take_while(|b| b.is_ascii_digit()).count();
    if digits_len == 0 {
        return Err("missing exponent after '^'".to_owned());
    }
    let magnitude: i64 = rest[..digits_len]
        .parse()
        .map_err(|_| "exponent too large (limit is 127)".to_owned())?;
    let signed = if negative { -magnitude } else { magnitude };
    if !(-MAX_POWER..=MAX_POWER).contains(&signed) {
        return Err("exponent too large (limit is 127)".to_owned());
    }
    // Bounded to ±127 by the check above.
    Ok((signed as i32, &rest[digits_len..]))
}

/// Parse the unit expression following the number: tokens (whitespace
/// between tokens means multiplication) combined with `*`, `/` and
/// integer `^` exponents. Scales `value` into canonical units and builds
/// the dimensional signature. `input` is the original full string, kept
/// for error messages.
fn parse_unit_expression(
    input: &str,
    s: &str,
    mut value: f64,
) -> Result<(f64, Unit), QuantityError> {
    let mut unit = Unit::dimensionless();
    let mut affine: Option<&'static UnitDef> = None;
    let mut pending_div = false;
    let mut expect_token = false;
    let mut last_op = ' ';
    let mut rest = s;

    loop {
        rest = rest.trim_start();
        if rest.is_empty() {
            if expect_token {
                return Err(invalid(
                    input,
                    format!("dangling '{last_op}': a unit token was expected after the operator"),
                ));
            }
            break;
        }
        let c = rest.chars().next().expect("non-empty by check above");
        match c {
            '*' | '/' => {
                if expect_token {
                    return Err(invalid(
                        input,
                        format!("consecutive operators '{last_op}' and '{c}'"),
                    ));
                }
                last_op = c;
                pending_div = c == '/';
                expect_token = true;
                rest = &rest[1..];
            }
            _ if is_unit_char(c) => {
                if let Some(seen) = affine {
                    return Err(invalid(
                        input,
                        format!(
                            "affine unit '{sym}' can only be used on its own, e.g. \"100 {sym}\"; use K in compound expressions",
                            sym = seen.symbol
                        ),
                    ));
                }
                let token_len = rest.find(|ch| !is_unit_char(ch)).unwrap_or(rest.len());
                let token = &rest[..token_len];
                rest = &rest[token_len..];
                let def = lookup_unit(token).ok_or_else(|| QuantityError::UnknownUnit {
                    token: token.to_owned(),
                })?;
                // Optional `^integer` exponent (whitespace tolerated).
                let mut exp: i32 = 1;
                let after_token = rest.trim_start();
                if let Some(after_caret) = after_token.strip_prefix('^') {
                    let (e, remainder) = scan_exponent(after_caret.trim_start())
                        .map_err(|reason| invalid(input, reason))?;
                    exp = e;
                    rest = remainder;
                } else {
                    rest = after_token;
                }
                let net = if pending_div { -exp } else { exp };
                if def.offset != 0.0 {
                    if net != 1 {
                        return Err(invalid(
                            input,
                            format!("affine unit '{sym}' requires exponent +1", sym = def.symbol),
                        ));
                    }
                    value *= def.factor;
                    affine = Some(def);
                } else {
                    value *= def.factor.powi(net);
                }
                let idx = def.dim.index();
                let combined = i64::from(unit.powers[idx]) + i64::from(net);
                if !(-MAX_POWER..=MAX_POWER).contains(&combined) {
                    return Err(invalid(
                        input,
                        "unit power overflow (limit is 127)".to_owned(),
                    ));
                }
                unit.powers[idx] = combined as i8;
                pending_div = false;
                expect_token = false;
            }
            _ => {
                return Err(invalid(input, format!("unexpected character '{c}'")));
            }
        }
    }

    // A lone affine token converts the parsed *level*: value = v*scale + offset.
    if let Some(def) = affine {
        value += def.offset;
    }
    Ok((value, unit))
}
