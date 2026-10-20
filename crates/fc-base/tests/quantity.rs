//! Integration tests for `fc_base::quantity` — units, parsing, arithmetic,
//! conversion and display.
//!
//! Covers the WOF work-order checklist:
//! * unit power arithmetic (`m/s * s = m`, force = raw dims `mm*kg/s^2`)
//! * parse roundtrips for every accepted input form
//! * parse errors (`"abc"`, unknown units, missing number, empty, ...)
//! * add/sub dimension-mismatch errors, mul/div unit combination, `powi`
//! * conversions (`1 in = 25.4 mm`, `100 C = 373.15 K`, `32 F = 273.15 K`,
//!   `180 deg = pi rad`, `1 mi` in `km`)
//! * conversion dimension-mismatch errors
//! * representative display strings and same-dimension ordering

use std::cmp::Ordering;
use std::str::FromStr;

use fc_base::quantity::{Dimension, Quantity, QuantityError, Unit};

/// Tight tolerance for values whose arithmetic is almost exact.
const TOL: f64 = 1e-12;
/// Looser tolerance for multi-step conversions (affine temperatures, mi/yd).
const CONV_TOL: f64 = 1e-9;

/// All eight dimensions, in signature order.
const DIMS: [Dimension; 8] = [
    Dimension::Length,
    Dimension::Mass,
    Dimension::Time,
    Dimension::ElectricCurrent,
    Dimension::Temperature,
    Dimension::AmountOfSubstance,
    Dimension::LuminousIntensity,
    Dimension::Angle,
];

/// Parse `s`, panicking with the error message on failure.
fn q(s: &str) -> Quantity {
    Quantity::from_str(s).unwrap_or_else(|e| panic!("parse '{s}': {e}"))
}

/// Assert `a` and `b` agree within `tol`.
fn near(a: f64, b: f64, tol: f64, ctx: &str) {
    assert!((a - b).abs() <= tol, "{ctx}: {a} vs {b} (tol {tol})");
}

// ---------------------------------------------------------------- units ---

#[test]
fn unit_base_constructors() {
    let ctors: [(Unit, Dimension); 8] = [
        (Unit::meter(), Dimension::Length),
        (Unit::kilogram(), Dimension::Mass),
        (Unit::second(), Dimension::Time),
        (Unit::ampere(), Dimension::ElectricCurrent),
        (Unit::kelvin(), Dimension::Temperature),
        (Unit::mole(), Dimension::AmountOfSubstance),
        (Unit::candela(), Dimension::LuminousIntensity),
        (Unit::degree(), Dimension::Angle),
    ];
    for (unit, own) in ctors {
        assert_eq!(unit.power(own), 1, "power of {own:?}");
        assert!(!unit.is_dimensionless(), "{own:?} is dimensioned");
        assert_ne!(unit, Unit::dimensionless());
        for dim in DIMS {
            let expected = if dim == own { 1 } else { 0 };
            assert_eq!(unit.power(dim), expected, "{own:?} vs {dim:?}");
        }
    }
}

#[test]
fn unit_dimensionless_is_all_zero() {
    let d = Unit::dimensionless();
    assert!(d.is_dimensionless());
    for dim in DIMS {
        assert_eq!(d.power(dim), 0);
    }
    assert_eq!(d, Unit::default());
    // Equality treats zero powers as absent: explicit cancellation is 0.
    assert_eq!(Unit::meter().powi(0), Unit::dimensionless());
    let ms = Unit::meter() * Unit::second();
    let sm = Unit::second() * Unit::meter();
    assert_eq!(ms / sm, Unit::dimensionless());
    assert!((ms / sm).is_dimensionless());
}

#[test]
fn unit_mul_div_power_arithmetic() {
    // m/s * s = m
    let v = Unit::meter() / Unit::second() * Unit::second();
    assert_eq!(v, Unit::meter());
    // m * m = m^2
    let area = Unit::meter() * Unit::meter();
    assert_eq!(area.power(Dimension::Length), 2);
    // kg m / s^2 has the force dimension, shown as raw dims
    let force = Unit::kilogram() * Unit::meter() / (Unit::second() * Unit::second());
    assert_eq!(force.power(Dimension::Length), 1);
    assert_eq!(force.power(Dimension::Mass), 1);
    assert_eq!(force.power(Dimension::Time), -2);
    // division subtracts: m/m cancels
    assert!((Unit::meter() / Unit::meter()).is_dimensionless());
    assert_eq!(Unit::meter().powi(2), Unit::meter() * Unit::meter());
}

#[test]
fn unit_powi_and_negative_powers() {
    assert_eq!(Unit::meter().powi(3).power(Dimension::Length), 3);
    assert_eq!(Unit::second().powi(-2).power(Dimension::Time), -2);
    assert!(Unit::meter().powi(0).is_dimensionless());
    assert_eq!(Unit::meter().powi(2).powi(-1), Unit::meter().powi(-2));
    assert_eq!(Unit::second().powi(-2).powi(-1), Unit::second().powi(2));
    assert_eq!(Unit::degree().powi(2).power(Dimension::Angle), 2);
    // negative exponent on a compound unit flips every sign
    let force = Unit::kilogram() * Unit::meter() / (Unit::second() * Unit::second());
    let inv = force.powi(-1);
    assert_eq!(inv.power(Dimension::Mass), -1);
    assert_eq!(inv.power(Dimension::Length), -1);
    assert_eq!(inv.power(Dimension::Time), 2);
}

#[test]
fn unit_display_canonical_forms() {
    assert_eq!(Unit::dimensionless().to_string(), "1");
    assert_eq!(Unit::meter().to_string(), "mm");
    assert_eq!(Unit::kilogram().to_string(), "kg");
    assert_eq!(Unit::second().to_string(), "s");
    assert_eq!(Unit::ampere().to_string(), "A");
    assert_eq!(Unit::kelvin().to_string(), "K");
    assert_eq!(Unit::mole().to_string(), "mol");
    assert_eq!(Unit::candela().to_string(), "cd");
    assert_eq!(Unit::degree().to_string(), "deg");
    assert_eq!((Unit::meter() * Unit::meter()).to_string(), "mm^2");
    // length^2 * time^-1 renders compactly (the spec's example)
    let area_per_time = Unit::meter() * Unit::meter() / Unit::second();
    assert_eq!(area_per_time.to_string(), "mm^2/s");
    assert_eq!((Unit::meter() / Unit::second()).to_string(), "mm/s");
    let force = Unit::kilogram() * Unit::meter() / (Unit::second() * Unit::second());
    assert_eq!(force.to_string(), "mm*kg/s^2");
    assert_eq!(Unit::degree().powi(2).to_string(), "deg^2");
    assert_eq!(Unit::second().powi(-1).to_string(), "1/s");
    assert_eq!((Unit::mole() * Unit::candela()).to_string(), "mol*cd");
}

#[test]
fn unit_power_accessor_across_dimensions() {
    let u = Unit::meter() * Unit::kilogram() * Unit::ampere() / (Unit::second() * Unit::kelvin());
    assert_eq!(u.power(Dimension::Length), 1);
    assert_eq!(u.power(Dimension::Mass), 1);
    assert_eq!(u.power(Dimension::Time), -1);
    assert_eq!(u.power(Dimension::ElectricCurrent), 1);
    assert_eq!(u.power(Dimension::Temperature), -1);
    assert_eq!(u.power(Dimension::AmountOfSubstance), 0);
    assert_eq!(u.power(Dimension::LuminousIntensity), 0);
    assert_eq!(u.power(Dimension::Angle), 0);
    assert_eq!(u.to_string(), "mm*kg*A/s*K");
}

#[test]
fn unit_hashable_as_signature() {
    use std::collections::HashMap;
    let mut map: HashMap<Unit, i32> = HashMap::new();
    map.insert(Unit::dimensionless(), 0);
    map.insert(Unit::meter(), 1);
    map.insert(Unit::meter() * Unit::meter(), 2);
    assert_eq!(map.get(&Unit::meter()), Some(&1));
    assert_eq!(map.get(&(Unit::meter() * Unit::meter())), Some(&2));
    assert_eq!(map.get(&Unit::second()), None);
}

// --------------------------------------------------------------- parsing ---

#[test]
fn parse_bare_numbers_are_dimensionless() {
    let cases = [
        ("3", 3.0),
        ("-2.5", -2.5),
        ("+7", 7.0),
        ("1e3", 1000.0),
        ("1.2E-4", 1.2e-4),
        ("-1e-3", -1e-3),
        (".5", 0.5),
        ("5.", 5.0),
        ("0", 0.0),
    ];
    for (s, expected) in cases {
        let parsed = q(s);
        assert!(
            parsed.unit().is_dimensionless(),
            "'{s}' should be dimensionless"
        );
        near(parsed.value(), expected, TOL, s);
        assert_eq!(parsed, Quantity::dimensionless(expected), "'{s}'");
    }
}

#[test]
fn parse_simple_units() {
    // factor-1.0 tokens are exact identities
    assert_eq!(q("3mm"), Quantity::new(3.0, Unit::meter()));
    assert_eq!(q("25 mm"), Quantity::new(25.0, Unit::meter()));
    assert_eq!(q("2 kg"), Quantity::new(2.0, Unit::kilogram()));
    assert_eq!(q("30 s"), Quantity::new(30.0, Unit::second()));
    assert_eq!(q("5 A"), Quantity::new(5.0, Unit::ampere()));
    assert_eq!(q("1 mol"), Quantity::new(1.0, Unit::mole()));
    assert_eq!(q("2 cd"), Quantity::new(2.0, Unit::candela()));
    assert_eq!(q("100 K"), Quantity::new(100.0, Unit::kelvin()));
    // scaled tokens (exact integer-valued factors stay exact)
    assert_eq!(q("1.5 m"), Quantity::new(1500.0, Unit::meter()));
    assert_eq!(q("12 cm"), Quantity::new(120.0, Unit::meter()));
    assert_eq!(q("3 km"), Quantity::new(3e6, Unit::meter()));
    assert_eq!(q("10 min"), Quantity::new(600.0, Unit::second()));
    assert_eq!(q("2.5 h"), Quantity::new(9000.0, Unit::second()));
    near(q("5 nm").value(), 5e-6, TOL, "5 nm");
    near(q("7 um").value(), 7e-3, TOL, "7 um");
    near(q("7 µm").value(), 7e-3, TOL, "7 µm");
}

#[test]
fn parse_mass_table() {
    assert_eq!(q("1 g").value(), 1e-3);
    assert_eq!(q("1 mg").value(), 1e-6);
    assert_eq!(q("1 t").value(), 1000.0);
    assert_eq!(q("1 oz").value(), 0.028349523125);
    assert_eq!(q("1 lb").value(), 0.45359237);
    assert_eq!(q("1 lb").unit(), Unit::kilogram());
    // 16 oz = 1 lb, both through the same table (exact, *16 is a shift)
    assert_eq!(q("16 oz"), q("1 lb"));
}

#[test]
fn parse_compound_expressions() {
    // "2 kg m / s^2": whitespace multiplies, '/' binds to the next term.
    let f = q("2 kg m / s^2");
    assert_eq!(f.value(), 2000.0);
    assert_eq!(f.unit().power(Dimension::Mass), 1);
    assert_eq!(f.unit().power(Dimension::Length), 1);
    assert_eq!(f.unit().power(Dimension::Time), -2);
    assert_eq!(f.to_string(), "2000 mm*kg/s^2");

    // The spec's gravity form.
    let g = q("9.8 m/s^2");
    near(g.value(), 9800.0, TOL, "9.8 m/s^2");
    assert_eq!(g.unit().to_string(), "mm/s^2");

    let volume = q("5 mm^3");
    assert_eq!(volume.value(), 5.0);
    assert_eq!(volume.unit().power(Dimension::Length), 3);
    assert_eq!(volume.unit().to_string(), "mm^3");

    // Explicit '*' and negative exponents.
    assert_eq!(q("1 kg*m/s^2").unit(), q("1 kg m / s^2").unit());
    near(q("1 kg*m/s^2").value(), 1000.0, TOL, "1 kg*m/s^2");
    assert_eq!(q("2 s^-2").unit().power(Dimension::Time), -2);

    // Areas: 3 m^2 is 3e6 mm^2.
    let a = q("3 m^2");
    assert_eq!(a.value(), 3e6);
    assert_eq!(a.unit().to_string(), "mm^2");
}

#[test]
fn parse_angle_synonyms() {
    assert_eq!(q("30°"), Quantity::new(30.0, Unit::degree()));
    assert_eq!(q("90 deg"), Quantity::new(90.0, Unit::degree()));
    assert_eq!(q("45 °"), Quantity::new(45.0, Unit::degree()));
    // pi radians = 180 degrees (spec: approx to 1e-12)
    let pi_rad = q("3.141592653589793 rad");
    near(pi_rad.value(), 180.0, 1e-12, "pi rad in deg");
    let straight = q("180 deg");
    assert!((straight.value() - pi_rad.value()).abs() <= 1e-12);
    assert_eq!(straight.unit(), pi_rad.unit());
}

#[test]
fn parse_whitespace_tolerance() {
    assert_eq!(q("25   mm"), q("25 mm"));
    assert_eq!(q("  25 mm  "), q("25 mm"));
    assert_eq!(q("\t9.8 m/s^2\t"), q("9.8 m/s^2"));
    assert_eq!(q("2  kg   m  /  s^2"), q("2 kg m / s^2"));
    // whitespace around '^' tolerated as well
    assert_eq!(q("5 mm ^ 3"), q("5 mm^3"));
    assert_eq!(q("1 m ^2"), q("1 m^2"));
}

#[test]
fn parse_glued_number_and_unit() {
    assert_eq!(q("25mm"), q("25 mm"));
    assert_eq!(q("1.5m"), q("1.5 m"));
    assert_eq!(q("1e-3kg"), q("1e-3 kg"));
    assert_eq!(q("-1e-3 kg"), Quantity::new(-1e-3, Unit::kilogram()));
    assert_eq!(q("5µm"), q("5 um"));
    assert_eq!(q("5μm"), q("5 um"));
}

#[test]
fn parse_temperature_levels() {
    near(q("100 C").value(), 373.15, 1e-12, "100 C");
    near(q("0 C").value(), 273.15, 1e-12, "0 C");
    assert_eq!(q("-273.15 C").value(), 0.0);
    near(q("32 F").value(), 273.15, 1e-12, "32 F");
    near(q("212 F").value(), 373.15, 1e-12, "212 F");
    near(q("-40 F").value(), 233.15, 1e-12, "-40 F");
    assert_eq!(q("0 K").value(), 0.0);
    assert_eq!(q("373.15 K").value(), 373.15);
    assert_eq!(q("100 C").unit(), Unit::kelvin());
    // C and K agree on differences (both stored in K)
    let dt = q("100 C").try_sub(q("0 C")).unwrap();
    assert_eq!(dt.value(), 100.0);
    assert_eq!(dt.unit(), Unit::kelvin());
}

#[test]
fn parse_affine_units_rejected_in_compounds() {
    for s in [
        "1 C^2",
        "2 C K",
        "5 K / C",
        "2 C C",
        "3 / C",
        "5 F s",
        "100 F^1 kg",
        "1 C^0",
        "1 K / F",
    ] {
        let err = Quantity::from_str(s).unwrap_err();
        assert!(
            matches!(err, QuantityError::InvalidQuantity { .. }),
            "'{s}' should be rejected, got: {err}"
        );
        assert!(err.to_string().contains("affine"), "'{s}': {err}");
    }
}

#[test]
fn parse_unknown_units() {
    let err = Quantity::from_str("5 furlongs").unwrap_err();
    assert!(matches!(err, QuantityError::UnknownUnit { .. }));
    let msg = err.to_string();
    assert!(msg.contains("furlongs"), "{msg}");
    assert!(msg.contains("mm"), "accepted set should be listed: {msg}");
    assert!(msg.contains("case-sensitive"), "{msg}");
    for s in ["5 mmm", "3 kgm", "5 KG", "3 M", "5e mm"] {
        assert!(
            matches!(
                Quantity::from_str(s),
                Err(QuantityError::UnknownUnit { .. })
            ),
            "'{s}' should be an unknown unit"
        );
    }
}

#[test]
fn parse_malformed_inputs() {
    for s in [
        "abc", "mm m", "", "   ", "5 5 mm", "5 mm 3", "5 mm *", "5 * * mm", "5 ^2", "5 mm^",
        "5 mm^x", "--5 mm", "5 mm^999", "5 mm..2", "12 \"",
    ] {
        let err = Quantity::from_str(s).unwrap_err();
        assert!(
            matches!(err, QuantityError::InvalidQuantity { .. }),
            "'{s}' should be rejected, got: {err}"
        );
    }
    // and the documented reasons show up
    assert!(Quantity::from_str("mm m")
        .unwrap_err()
        .to_string()
        .contains("missing numeric value"));
    assert!(Quantity::from_str("5 mm^999")
        .unwrap_err()
        .to_string()
        .contains("exponent too large"));
    assert!(Quantity::from_str("5 mm *")
        .unwrap_err()
        .to_string()
        .contains("dangling"));
}

#[test]
fn parse_display_roundtrip() {
    let inputs = [
        "3",
        "1e3",
        "1.2E-4",
        "25 mm",
        "1.5 m",
        "2 kg",
        "30°",
        "90 deg",
        "9.8 m/s^2",
        "2 kg m / s^2",
        "5 mm^3",
        "-1e-3 kg",
        "100 C",
        "32 F",
    ];
    for s in inputs {
        let parsed = q(s);
        let text = parsed.to_string();
        let again = Quantity::from_str(&text)
            .unwrap_or_else(|e| panic!("reparse '{text}' (from '{s}'): {e}"));
        assert_eq!(again, parsed, "'{s}' -> '{text}' should roundtrip");
    }
}

#[test]
fn display_canonical_strings() {
    assert_eq!(q("25 mm").to_string(), "25 mm");
    assert_eq!(q("1.5 m").to_string(), "1500 mm");
    assert_eq!(q("30°").to_string(), "30 deg");
    assert_eq!(q("9.8 m/s^2").to_string(), "9800 mm/s^2");
    assert_eq!(q("2 kg m / s^2").to_string(), "2000 mm*kg/s^2");
    assert_eq!(q("3").to_string(), "3");
    assert_eq!(q("-1e-3 kg").to_string(), "-0.001 kg");
    assert_eq!(q("1 in").to_string(), "25.4 mm");
    assert_eq!(q("45 min").to_string(), "2700 s");
    assert_eq!(q("100 C").to_string(), "373.15 K");
    assert_eq!(Quantity::dimensionless(2.5).to_string(), "2.5");
}

// ------------------------------------------------------------ arithmetic ---

#[test]
fn try_add_same_dimension() {
    let sum = Quantity::new(25.0, Unit::meter())
        .try_add(Quantity::new(1000.0, Unit::meter()))
        .unwrap();
    assert_eq!(sum, Quantity::new(1025.0, Unit::meter()));
    assert_eq!(q("1 m").try_add(q("25 mm")).unwrap().value(), 1025.0);
    // angles add in canonical degrees
    let total = q("90 deg").try_add(q("1.5707963267948966 rad")).unwrap();
    assert_eq!(total.unit(), Unit::degree());
    near(total.value(), 180.0, 1e-12, "90 deg + pi/2 rad");
    // dimensionless quantities add like numbers
    assert_eq!(
        q("2").try_add(q("3.5")).unwrap(),
        Quantity::dimensionless(5.5)
    );
    assert!(q("1 mm").try_add(q("1 mm")).is_ok());
}

#[test]
fn try_add_dimension_mismatch() {
    let err = q("1 mm").try_add(q("1 s")).unwrap_err();
    assert_eq!(
        err,
        QuantityError::DimensionMismatch {
            operation: "add quantities",
            left: Unit::meter(),
            right: Unit::second(),
        }
    );
    let msg = err.to_string();
    assert!(msg.contains("differ"), "{msg}");
    assert!(msg.contains("mm"), "{msg}");
    assert!(msg.contains("s"), "{msg}");
    // dimensionless + dimensioned is also a mismatch
    assert!(q("2").try_add(q("1 mm")).is_err());
}

#[test]
fn try_sub_and_differences() {
    let d = q("1 m").try_sub(q("25 mm")).unwrap();
    assert_eq!(d.value(), 975.0);
    assert_eq!(d.unit(), Unit::meter());
    assert_eq!(q("5 s").try_sub(q("1.5 s")).unwrap(), q("3.5 s"));
    let err = q("1 kg").try_sub(q("1 mm")).unwrap_err();
    assert_eq!(
        err,
        QuantityError::DimensionMismatch {
            operation: "subtract quantities",
            left: Unit::kilogram(),
            right: Unit::meter(),
        }
    );
}

#[test]
fn mul_div_combine_units() {
    let dist = q("5 mm") * q("2 s");
    assert_eq!(dist.value(), 10.0);
    assert_eq!(dist.unit().power(Dimension::Length), 1);
    assert_eq!(dist.unit().power(Dimension::Time), 1);
    assert_eq!(dist.unit().to_string(), "mm*s");

    // F = m * a
    let f = q("2 kg") * q("3 m/s^2");
    assert_eq!(f.value(), 6000.0);
    assert_eq!(f.unit().to_string(), "mm*kg/s^2");

    // quotient cancels dimensions: 1 m / 25.4 mm is a pure number
    let ratio = q("1 m") / q("25.4 mm");
    assert!(ratio.unit().is_dimensionless());
    near(ratio.value(), 1000.0 / 25.4, TOL, "1 m per inch");

    // division combines: (mm/s) / s = mm/s^2
    let acc = q("10 mm/s") / q("2 s");
    assert_eq!(acc.unit().to_string(), "mm/s^2");
    assert_eq!(acc.value(), 5.0);
}

#[test]
fn scalar_mul_div_neg() {
    let base = Quantity::new(2.5, Unit::meter());
    assert_eq!(base * 4.0, Quantity::new(10.0, Unit::meter()));
    assert_eq!(3.0 * base, Quantity::new(7.5, Unit::meter()));
    assert_eq!(base / 5.0, Quantity::new(0.5, Unit::meter()));
    let t = Quantity::new(2.0, Unit::second());
    assert_eq!(-t, Quantity::new(-2.0, Unit::second()));
    assert_eq!(-(-t), t);
    assert_eq!((-t).unit(), Unit::second());
    // scalar ops agree with parsing
    assert_eq!(q("25 mm") * 2.0, q("50 mm"));
    assert_eq!(2.0 * q("25 mm"), q("50 mm"));
    assert_eq!(q("1 h") / 3.0, q("20 min"));
}

#[test]
fn powi_quantities() {
    let cube = q("2 mm").powi(3);
    assert_eq!(cube.value(), 8.0);
    assert_eq!(cube.unit().power(Dimension::Length), 3);
    assert_eq!(cube.to_string(), "8 mm^3");
    // negative powers allowed
    let inv = q("4 s").powi(-1);
    assert_eq!(inv.value(), 0.25);
    assert_eq!(inv.unit().power(Dimension::Time), -1);
    assert_eq!(inv.to_string(), "0.25 1/s");
    // zeroth power is the dimensionless one
    assert_eq!(q("5 mm").powi(0), Quantity::dimensionless(1.0));
    // negative base, odd power
    assert_eq!(q("-2 mm").powi(3).value(), -8.0);
    // compound unit powers scale per dimension
    let a = q("3 m/s").powi(2);
    assert_eq!(a.unit().power(Dimension::Length), 2);
    assert_eq!(a.unit().power(Dimension::Time), -2);
    assert_eq!(a.value(), 9e6);
}

// ------------------------------------------------------------ comparison ---

#[test]
fn ordering_same_dimension_only() {
    let mm25 = q("25 mm");
    let m1 = q("1 m");
    assert!(mm25 < m1);
    assert!(m1 > mm25);
    assert_eq!(mm25.partial_cmp(&m1), Some(Ordering::Less));
    assert_eq!(m1.partial_cmp(&q("1000 mm")), Some(Ordering::Equal));
    // different dimensions are incomparable, never silently ordered
    let one_mm = q("1 mm");
    let one_s = q("1 s");
    assert_eq!(one_mm.partial_cmp(&one_s), None);
    assert!(one_mm != one_s);
    // operator comparisons must not claim an order across dimensions
    assert!(!one_mm.lt(&one_s));
    assert!(!one_mm.gt(&one_s));
    // equal values across unit spellings (parse converts to canonical)
    assert_eq!(q("1 in"), q("25.4 mm"));
    assert_eq!(q("1 h"), q("3600 s"));
    assert!(q("1 in") <= q("25.4 mm"));
}

// ----------------------------------------------------------- conversions ---

#[test]
fn convert_to_checks_dimension() {
    let length = q("25.4 mm");
    let same = length.convert_to(Unit::meter()).unwrap();
    assert_eq!(same, length);
    assert_eq!(q("1 in").convert_to(Unit::meter()).unwrap(), q("25.4 mm"));
    assert!(Quantity::dimensionless(1.0)
        .convert_to(Unit::dimensionless())
        .is_ok());
    let err = q("1 m").convert_to(Unit::second()).unwrap_err();
    assert_eq!(
        err,
        QuantityError::DimensionMismatch {
            operation: "convert quantity",
            left: Unit::meter(),
            right: Unit::second(),
        }
    );
    let msg = err.to_string();
    assert!(msg.contains("mm") && msg.contains("s"), "{msg}");
    assert!(matches!(
        q("1 m").convert_to(Unit::dimensionless()),
        Err(QuantityError::DimensionMismatch { .. })
    ));
}

#[test]
fn value_in_length_units() {
    let inch = q("1 in");
    // 1 in = 25.4 mm, exact (spec case)
    assert_eq!(inch.value_in("mm").unwrap(), 25.4);
    assert_eq!(inch.value_in("in").unwrap(), 1.0);
    assert_eq!(q("25.4 mm").value_in("in").unwrap(), 1.0);
    near(
        q("1 m").value_in("ft").unwrap(),
        1000.0 / 304.8,
        TOL,
        "1 m in ft",
    );
    // 1 mi expressed in km (spec case)
    near(
        q("1 mi").value_in("km").unwrap(),
        1.609344,
        CONV_TOL,
        "1 mi in km",
    );
    near(
        q("1 mi").value_in("m").unwrap(),
        1609.344,
        CONV_TOL,
        "1 mi in m",
    );
    near(
        q("1 mi").value_in("yd").unwrap(),
        1760.0,
        CONV_TOL,
        "1 mi in yd",
    );
    near(q("1 ft").value_in("in").unwrap(), 12.0, TOL, "1 ft in in");
    assert_eq!(q("1 h").value_in("min").unwrap(), 60.0);
}

#[test]
fn value_in_time_and_angle() {
    assert_eq!(q("1 h").value_in("s").unwrap(), 3600.0);
    assert_eq!(q("90 min").value_in("h").unwrap(), 1.5);
    assert_eq!(q("5 min").value_in("s").unwrap(), 300.0);
    // 180 deg = pi rad, approx to 1e-12 (spec case)
    near(
        q("180 deg").value_in("rad").unwrap(),
        std::f64::consts::PI,
        1e-12,
        "180 deg in rad",
    );
    near(
        q("3.141592653589793 rad").value_in("deg").unwrap(),
        180.0,
        1e-12,
        "pi rad in deg",
    );
    near(
        q("1 rad").value_in("deg").unwrap(),
        180.0 / std::f64::consts::PI,
        1e-12,
        "1 rad in deg",
    );
    assert_eq!(q("30°").value_in("°").unwrap(), 30.0);
    near(
        q("30°").value_in("rad").unwrap(),
        std::f64::consts::PI / 6.0,
        1e-12,
        "30 deg in rad",
    );
}

#[test]
fn value_in_temperature_affine() {
    // 100 C = 373.15 K (spec case)
    let boiling = q("100 C");
    near(boiling.value(), 373.15, 1e-12, "100 C value");
    near(boiling.value_in("K").unwrap(), 373.15, 1e-12, "100 C in K");
    near(boiling.value_in("C").unwrap(), 100.0, 1e-12, "100 C in C");
    near(
        boiling.value_in("F").unwrap(),
        212.0,
        CONV_TOL,
        "100 C in F",
    );
    // 32 F = 273.15 K (spec case)
    let freezing = q("32 F");
    near(freezing.value_in("K").unwrap(), 273.15, 1e-12, "32 F in K");
    near(freezing.value_in("C").unwrap(), 0.0, CONV_TOL, "32 F in C");
    near(freezing.value_in("F").unwrap(), 32.0, 1e-12, "32 F in F");
    // the classic -40 crossover
    near(
        q("-40 F").value_in("C").unwrap(),
        -40.0,
        CONV_TOL,
        "-40 F in C",
    );
    near(
        q("-40 C").value_in("F").unwrap(),
        -40.0,
        CONV_TOL,
        "-40 C in F",
    );
    near(
        q("0 K").value_in("F").unwrap(),
        -459.67,
        CONV_TOL,
        "0 K in F",
    );
    near(
        q("0 K").value_in("C").unwrap(),
        -273.15,
        CONV_TOL,
        "0 K in C",
    );
}

#[test]
fn value_in_errors() {
    let err = q("1 m").value_in("kg").unwrap_err();
    assert_eq!(
        err,
        QuantityError::DimensionMismatch {
            operation: "express quantity in a named unit",
            left: Unit::meter(),
            right: Unit::kilogram(),
        }
    );
    let msg = err.to_string();
    assert!(msg.contains("mm") && msg.contains("kg"), "{msg}");
    assert!(matches!(
        q("1 m").value_in("furlongs"),
        Err(QuantityError::UnknownUnit { .. })
    ));
    assert!(q("1 m")
        .value_in("furlongs")
        .unwrap_err()
        .to_string()
        .contains("furlongs"));
    // compound quantities cannot be expressed in a single named unit
    assert!(q("9.8 m/s^2").value_in("m").is_err());
    // nor can powered dimensions (m^2 is not a plain length)
    assert!(q("1 m^2").value_in("m").is_err());
    assert!(q("1 m^2").value_in("m^2").is_err());
}

// ---------------------------------------------------------------- errors ---

#[test]
fn error_display_messages() {
    let msg = Quantity::from_str("5 furlongs").unwrap_err().to_string();
    assert!(msg.starts_with("unknown unit token 'furlongs'"), "{msg}");
    assert!(msg.contains("accepted (case-sensitive): nm"), "{msg}");
    assert!(msg.contains("cd"), "{msg}");

    let msg = Quantity::from_str("").unwrap_err().to_string();
    assert_eq!(msg, "invalid quantity '': empty input");

    let msg = Quantity::from_str("mm m").unwrap_err().to_string();
    assert!(msg.contains("missing numeric value"), "{msg}");

    let msg = q("1 s").try_add(q("1 mm")).unwrap_err().to_string();
    assert!(
        msg.starts_with("cannot add quantities: dimensions differ"),
        "{msg}"
    );
    assert!(msg.ends_with("(s vs mm)"), "{msg}");
}

#[test]
fn quantity_construction_and_getters() {
    let area = Quantity::new(3.5, Unit::meter().powi(2));
    assert_eq!(area.value(), 3.5);
    assert_eq!(area.unit(), Unit::meter().powi(2));
    assert_eq!(area.unit().power(Dimension::Length), 2);
    let n = Quantity::dimensionless(-1.25);
    assert_eq!(n.value(), -1.25);
    assert!(n.unit().is_dimensionless());
    // Quantity is Copy
    let copy = area;
    assert_eq!(copy, area);
    // Quantity::parse mirrors FromStr
    assert_eq!(
        Quantity::parse("2 s").unwrap(),
        Quantity::from_str("2 s").unwrap()
    );
}
