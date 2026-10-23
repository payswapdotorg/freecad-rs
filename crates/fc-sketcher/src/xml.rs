//! FreeCAD-style XML persistence for sketches.
//!
//! # Format
//!
//! A sketch serializes to a `<Sketch>` element containing `<Geometry .../>`
//! children followed by `<Constraint .../>` children. Attribute names
//! mirror FreeCAD's lowercase-ish schema; values are plain numbers.
//!
//! Geometry `type` ordinals (documented stable order — point first, then
//! the curve-free constructions):
//!
//! | type | variant        | attributes                                        |
//! |------|----------------|---------------------------------------------------|
//! | 0    | `Point`        | `x`, `y`                                          |
//! | 1    | `LineSegment`  | `startx`, `starty`, `endx`, `endy`                |
//! | 2    | `Circle`       | `centerx`, `centery`, `radius`                    |
//! | 3    | `ArcOfCircle`  | `centerx`, `centery`, `radius`, `startangle`, `endangle` |
//!
//! Constraint `type` ordinals follow the declaration order of
//! [`Constraint`](crate::constraint::Constraint):
//!
//! | type | variant         | attributes |
//! |------|-----------------|------------|
//! | 0    | `Coincident`    | `first`, `firstpos`, `second`, `secondpos` |
//! | 1    | `Horizontal`    | `first` |
//! | 2    | `Vertical`      | `first` |
//! | 3    | `Parallel`      | `first`, `second` |
//! | 4    | `Perpendicular` | `first`, `second` |
//! | 5    | `Distance`      | `first`, `firstpos`, `second`, `secondpos`, `value` |
//! | 6    | `DistanceX`     | `first`, `firstpos`, `second`, `secondpos`, `value` |
//! | 7    | `DistanceY`     | `first`, `firstpos`, `second`, `secondpos`, `value` |
//! | 8    | `Radius`        | `first`, `value` |
//! | 9    | `Diameter`      | `first`, `value` |
//! | 10   | `Equal`         | `first`, `second` |
//! | 11   | `Angle`         | `first`, `second`, `value` |
//! | 12   | `PointOnObject` | `first`, `firstpos`, `second` |
//! | 13   | `Tangent`       | `first`, `second` |
//! | 14   | `Symmetric`     | `first`, `firstpos`, `second`, `secondpos`, `third` (+ `thirdpos` only when symmetric about a point) |
//! | 15   | `Block`         | `first` |
//!
//! `first`/`second` are geometry indices ([`GeoId`] values, i.e. positions
//! in the geometry list); `firstpos`/`secondpos` are [`PointPos`] ordinals
//! (1 = Start, 2 = End, 3 = Center). Floats are written with `{:?}`
//! (Rust's shortest roundtrip-exact representation), so
//! `from_xml(to_xml(sk))` reproduces every bit of every coordinate.
//!
//! # Parser
//!
//! The parser is **hand-written** (zero new dependencies): a tiny scanner
//! for tags and `key="value"` attributes. It is strict where correctness
//! matters and lenient where cosmetics don't count:
//!
//! - accepted: leading/trailing whitespace, an optional `<?xml ...?>`
//!   declaration, any attributes on the root `<Sketch>` element;
//! - rejected (with [`SketchError::Xml`]): wrong root element, unknown or
//!   non-self-closing child elements, non-whitespace text content,
//!   missing/unknown attributes, non-finite or non-numeric values,
//!   unknown type ordinals, negative radii, content after `</Sketch>`;
//! - constraints are re-validated through `Sketch::add_constraint`, so
//!   bad references (`first="99"`) and unsupported combinations surface as
//!   their specific [`SketchError`] variants.

use crate::constraint::{Constraint, PointPos};
use crate::geometry::{GeoId, Geometry, Point, Sketch};
use crate::SketchError;

impl Sketch {
    /// Serializes the sketch to a FreeCAD-style `<Sketch>` XML fragment.
    ///
    /// Geometries are emitted first (in [`GeoId`] order), then constraints
    /// (in insertion order). See the [module documentation](self) for the
    /// exact schema.
    pub fn to_xml(&self) -> String {
        let mut out = String::from("<Sketch>\n");
        for g in self.geometry() {
            let mut attrs = Vec::new();
            match *g {
                Geometry::Point { x, y } => {
                    attrs.push("type=\"0\"".to_string());
                    attrs.push(format!("x=\"{x:?}\""));
                    attrs.push(format!("y=\"{y:?}\""));
                }
                Geometry::LineSegment { start, end } => {
                    attrs.push("type=\"1\"".to_string());
                    attrs.push(format!("startx=\"{:?}\"", start.x));
                    attrs.push(format!("starty=\"{:?}\"", start.y));
                    attrs.push(format!("endx=\"{:?}\"", end.x));
                    attrs.push(format!("endy=\"{:?}\"", end.y));
                }
                Geometry::Circle { center, radius } => {
                    attrs.push("type=\"2\"".to_string());
                    attrs.push(format!("centerx=\"{:?}\"", center.x));
                    attrs.push(format!("centery=\"{:?}\"", center.y));
                    attrs.push(format!("radius=\"{radius:?}\""));
                }
                Geometry::ArcOfCircle {
                    center,
                    radius,
                    start_angle,
                    end_angle,
                } => {
                    attrs.push("type=\"3\"".to_string());
                    attrs.push(format!("centerx=\"{:?}\"", center.x));
                    attrs.push(format!("centery=\"{:?}\"", center.y));
                    attrs.push(format!("radius=\"{radius:?}\""));
                    attrs.push(format!("startangle=\"{start_angle:?}\""));
                    attrs.push(format!("endangle=\"{end_angle:?}\""));
                }
            }
            out.push_str(&format!("  <Geometry {}/>\n", attrs.join(" ")));
        }
        for c in self.constraints() {
            let mut attrs = Vec::new();
            match *c {
                Constraint::Coincident { g1, pos1, g2, pos2 } => {
                    attrs.push("type=\"0\"".to_string());
                    attrs.push(format!("first=\"{}\"", g1.0));
                    attrs.push(format!("firstpos=\"{}\"", pos1.ordinal()));
                    attrs.push(format!("second=\"{}\"", g2.0));
                    attrs.push(format!("secondpos=\"{}\"", pos2.ordinal()));
                }
                Constraint::Horizontal { g } => {
                    attrs.push("type=\"1\"".to_string());
                    attrs.push(format!("first=\"{}\"", g.0));
                }
                Constraint::Vertical { g } => {
                    attrs.push("type=\"2\"".to_string());
                    attrs.push(format!("first=\"{}\"", g.0));
                }
                Constraint::Parallel { g1, g2 } => {
                    attrs.push("type=\"3\"".to_string());
                    attrs.push(format!("first=\"{}\"", g1.0));
                    attrs.push(format!("second=\"{}\"", g2.0));
                }
                Constraint::Perpendicular { g1, g2 } => {
                    attrs.push("type=\"4\"".to_string());
                    attrs.push(format!("first=\"{}\"", g1.0));
                    attrs.push(format!("second=\"{}\"", g2.0));
                }
                Constraint::Distance {
                    g1,
                    pos1,
                    g2,
                    pos2,
                    value,
                }
                | Constraint::DistanceX {
                    g1,
                    pos1,
                    g2,
                    pos2,
                    value,
                }
                | Constraint::DistanceY {
                    g1,
                    pos1,
                    g2,
                    pos2,
                    value,
                } => {
                    let t = match c {
                        Constraint::Distance { .. } => 5,
                        Constraint::DistanceX { .. } => 6,
                        _ => 7,
                    };
                    attrs.push(format!("type=\"{t}\""));
                    attrs.push(format!("first=\"{}\"", g1.0));
                    attrs.push(format!("firstpos=\"{}\"", pos1.ordinal()));
                    attrs.push(format!("second=\"{}\"", g2.0));
                    attrs.push(format!("secondpos=\"{}\"", pos2.ordinal()));
                    attrs.push(format!("value=\"{value:?}\""));
                }
                Constraint::Radius { g, value } => {
                    attrs.push("type=\"8\"".to_string());
                    attrs.push(format!("first=\"{}\"", g.0));
                    attrs.push(format!("value=\"{value:?}\""));
                }
                Constraint::Diameter { g, value } => {
                    attrs.push("type=\"9\"".to_string());
                    attrs.push(format!("first=\"{}\"", g.0));
                    attrs.push(format!("value=\"{value:?}\""));
                }
                Constraint::Equal { g1, g2 } => {
                    attrs.push("type=\"10\"".to_string());
                    attrs.push(format!("first=\"{}\"", g1.0));
                    attrs.push(format!("second=\"{}\"", g2.0));
                }
                Constraint::Angle { g1, g2, value } => {
                    attrs.push("type=\"11\"".to_string());
                    attrs.push(format!("first=\"{}\"", g1.0));
                    attrs.push(format!("second=\"{}\"", g2.0));
                    attrs.push(format!("value=\"{value:?}\""));
                }
                Constraint::PointOnObject { g1, pos1, g2 } => {
                    attrs.push("type=\"12\"".to_string());
                    attrs.push(format!("first=\"{}\"", g1.0));
                    attrs.push(format!("firstpos=\"{}\"", pos1.ordinal()));
                    attrs.push(format!("second=\"{}\"", g2.0));
                }
                Constraint::Tangent { g1, g2 } => {
                    attrs.push("type=\"13\"".to_string());
                    attrs.push(format!("first=\"{}\"", g1.0));
                    attrs.push(format!("second=\"{}\"", g2.0));
                }
                Constraint::Symmetric {
                    g1,
                    pos1,
                    g2,
                    pos2,
                    g3,
                    pos3,
                } => {
                    attrs.push("type=\"14\"".to_string());
                    attrs.push(format!("first=\"{}\"", g1.0));
                    attrs.push(format!("firstpos=\"{}\"", pos1.ordinal()));
                    attrs.push(format!("second=\"{}\"", g2.0));
                    attrs.push(format!("secondpos=\"{}\"", pos2.ordinal()));
                    attrs.push(format!("third=\"{}\"", g3.0));
                    if let Some(slot) = pos3 {
                        attrs.push(format!("thirdpos=\"{}\"", slot.ordinal()));
                    }
                }
                Constraint::Block { g } => {
                    attrs.push("type=\"15\"".to_string());
                    attrs.push(format!("first=\"{}\"", g.0));
                }
            }
            out.push_str(&format!("  <Constraint {}/>\n", attrs.join(" ")));
        }
        out.push_str("</Sketch>\n");
        out
    }
}

/// Parses a `<Sketch>` XML fragment back into a [`Sketch`].
///
/// See the [module documentation](self) for the accepted grammar and the
/// strict/lenient split.
pub fn from_xml(input: &str) -> Result<Sketch, SketchError> {
    let mut p = Cursor::new(input);
    p.skip_ws();
    if p.consume_prefix("<?") {
        p.skip_until("?>")?;
        p.skip_ws();
    }
    p.expect(b'<')?;
    let (name, _root_attrs, self_closing) = p.parse_tag()?;
    if name != "Sketch" {
        return Err(SketchError::Xml(format!(
            "expected root element <Sketch>, found <{name}>"
        )));
    }
    let mut sk = Sketch::new();
    if !self_closing {
        loop {
            p.skip_ws();
            if p.consume_prefix("</") {
                let close = p.read_name()?;
                if close != "Sketch" {
                    return Err(SketchError::Xml(format!(
                        "expected </Sketch>, found </{close}>"
                    )));
                }
                p.skip_ws();
                p.expect(b'>')?;
                break;
            }
            if p.at_end() {
                return Err(SketchError::Xml(
                    "unexpected end of input: <Sketch> is not closed".to_string(),
                ));
            }
            p.expect(b'<')?;
            let (name, attrs, self_closing) = p.parse_tag()?;
            if !self_closing {
                return Err(SketchError::Xml(format!(
                    "element <{name}> must be an empty (self-closing) tag"
                )));
            }
            match name {
                "Geometry" => {
                    let g = geometry_from_attrs(&attrs)?;
                    sk.add_geometry(g);
                }
                "Constraint" => {
                    let c = constraint_from_attrs(&attrs)?;
                    sk.add_constraint(c)?;
                }
                other => {
                    return Err(SketchError::Xml(format!(
                        "unexpected element <{other}> inside <Sketch>"
                    )))
                }
            }
        }
    }
    p.skip_ws();
    if !p.at_end() {
        return Err(SketchError::Xml(
            "unexpected content after </Sketch>".to_string(),
        ));
    }
    Ok(sk)
}

fn geometry_from_attrs(attrs: &[(String, String)]) -> Result<Geometry, SketchError> {
    let t = parse_u32(attr(attrs, "type")?, "Geometry/@type")?;
    match t {
        0 => {
            require_only(attrs, &["type", "x", "y"], "Geometry")?;
            Ok(Geometry::Point {
                x: parse_f64(attr(attrs, "x")?, "Geometry/@x")?,
                y: parse_f64(attr(attrs, "y")?, "Geometry/@y")?,
            })
        }
        1 => {
            require_only(
                attrs,
                &["type", "startx", "starty", "endx", "endy"],
                "Geometry",
            )?;
            Ok(Geometry::LineSegment {
                start: Point::new(
                    parse_f64(attr(attrs, "startx")?, "Geometry/@startx")?,
                    parse_f64(attr(attrs, "starty")?, "Geometry/@starty")?,
                ),
                end: Point::new(
                    parse_f64(attr(attrs, "endx")?, "Geometry/@endx")?,
                    parse_f64(attr(attrs, "endy")?, "Geometry/@endy")?,
                ),
            })
        }
        2 => {
            require_only(attrs, &["type", "centerx", "centery", "radius"], "Geometry")?;
            let radius = parse_f64(attr(attrs, "radius")?, "Geometry/@radius")?;
            if radius < 0.0 {
                return Err(SketchError::Xml(format!("negative circle radius {radius}")));
            }
            Ok(Geometry::Circle {
                center: Point::new(
                    parse_f64(attr(attrs, "centerx")?, "Geometry/@centerx")?,
                    parse_f64(attr(attrs, "centery")?, "Geometry/@centery")?,
                ),
                radius,
            })
        }
        3 => {
            require_only(
                attrs,
                &[
                    "type",
                    "centerx",
                    "centery",
                    "radius",
                    "startangle",
                    "endangle",
                ],
                "Geometry",
            )?;
            let radius = parse_f64(attr(attrs, "radius")?, "Geometry/@radius")?;
            if radius < 0.0 {
                return Err(SketchError::Xml(format!("negative arc radius {radius}")));
            }
            Ok(Geometry::ArcOfCircle {
                center: Point::new(
                    parse_f64(attr(attrs, "centerx")?, "Geometry/@centerx")?,
                    parse_f64(attr(attrs, "centery")?, "Geometry/@centery")?,
                ),
                radius,
                start_angle: parse_f64(attr(attrs, "startangle")?, "Geometry/@startangle")?,
                end_angle: parse_f64(attr(attrs, "endangle")?, "Geometry/@endangle")?,
            })
        }
        other => Err(SketchError::Xml(format!(
            "unknown Geometry type {other} (expected 0..=3)"
        ))),
    }
}

fn constraint_from_attrs(attrs: &[(String, String)]) -> Result<Constraint, SketchError> {
    let t = parse_u32(attr(attrs, "type")?, "Constraint/@type")?;
    let first = || parse_geo(attr(attrs, "first")?, "Constraint/@first");
    let second = || parse_geo(attr(attrs, "second")?, "Constraint/@second");
    let firstpos = || parse_pos(attr(attrs, "firstpos")?, "Constraint/@firstpos");
    let secondpos = || parse_pos(attr(attrs, "secondpos")?, "Constraint/@secondpos");
    let value = || parse_f64(attr(attrs, "value")?, "Constraint/@value");
    match t {
        0 => {
            require_only(
                attrs,
                &["type", "first", "firstpos", "second", "secondpos"],
                "Constraint",
            )?;
            Ok(Constraint::Coincident {
                g1: first()?,
                pos1: firstpos()?,
                g2: second()?,
                pos2: secondpos()?,
            })
        }
        1 => {
            require_only(attrs, &["type", "first"], "Constraint")?;
            Ok(Constraint::Horizontal { g: first()? })
        }
        2 => {
            require_only(attrs, &["type", "first"], "Constraint")?;
            Ok(Constraint::Vertical { g: first()? })
        }
        3 => {
            require_only(attrs, &["type", "first", "second"], "Constraint")?;
            Ok(Constraint::Parallel {
                g1: first()?,
                g2: second()?,
            })
        }
        4 => {
            require_only(attrs, &["type", "first", "second"], "Constraint")?;
            Ok(Constraint::Perpendicular {
                g1: first()?,
                g2: second()?,
            })
        }
        5..=7 => {
            require_only(
                attrs,
                &["type", "first", "firstpos", "second", "secondpos", "value"],
                "Constraint",
            )?;
            let g1 = first()?;
            let pos1 = firstpos()?;
            let g2 = second()?;
            let pos2 = secondpos()?;
            let v = value()?;
            Ok(match t {
                5 => Constraint::Distance {
                    g1,
                    pos1,
                    g2,
                    pos2,
                    value: v,
                },
                6 => Constraint::DistanceX {
                    g1,
                    pos1,
                    g2,
                    pos2,
                    value: v,
                },
                _ => Constraint::DistanceY {
                    g1,
                    pos1,
                    g2,
                    pos2,
                    value: v,
                },
            })
        }
        8 => {
            require_only(attrs, &["type", "first", "value"], "Constraint")?;
            Ok(Constraint::Radius {
                g: first()?,
                value: value()?,
            })
        }
        9 => {
            require_only(attrs, &["type", "first", "value"], "Constraint")?;
            Ok(Constraint::Diameter {
                g: first()?,
                value: value()?,
            })
        }
        10 => {
            require_only(attrs, &["type", "first", "second"], "Constraint")?;
            Ok(Constraint::Equal {
                g1: first()?,
                g2: second()?,
            })
        }
        11 => {
            require_only(attrs, &["type", "first", "second", "value"], "Constraint")?;
            Ok(Constraint::Angle {
                g1: first()?,
                g2: second()?,
                value: value()?,
            })
        }
        12 => {
            require_only(
                attrs,
                &["type", "first", "firstpos", "second"],
                "Constraint",
            )?;
            Ok(Constraint::PointOnObject {
                g1: first()?,
                pos1: firstpos()?,
                g2: second()?,
            })
        }
        13 => {
            require_only(attrs, &["type", "first", "second"], "Constraint")?;
            Ok(Constraint::Tangent {
                g1: first()?,
                g2: second()?,
            })
        }
        14 => {
            // `thirdpos` is present only in the about-a-point form.
            let thirdpos = attrs
                .iter()
                .find(|(k, _)| k == "thirdpos")
                .map(|(_, v)| v.as_str());
            if thirdpos.is_some() {
                require_only(
                    attrs,
                    &[
                        "type",
                        "first",
                        "firstpos",
                        "second",
                        "secondpos",
                        "third",
                        "thirdpos",
                    ],
                    "Constraint",
                )?;
            } else {
                require_only(
                    attrs,
                    &["type", "first", "firstpos", "second", "secondpos", "third"],
                    "Constraint",
                )?;
            }
            let pos3 = match thirdpos {
                Some(v) => Some(parse_pos(v, "Constraint/@thirdpos")?),
                None => None,
            };
            Ok(Constraint::Symmetric {
                g1: first()?,
                pos1: firstpos()?,
                g2: second()?,
                pos2: secondpos()?,
                g3: parse_geo(attr(attrs, "third")?, "Constraint/@third")?,
                pos3,
            })
        }
        15 => {
            require_only(attrs, &["type", "first"], "Constraint")?;
            Ok(Constraint::Block { g: first()? })
        }
        other => Err(SketchError::Xml(format!(
            "unknown Constraint type {other} (expected 0..=15)"
        ))),
    }
}

/// Looks up a required attribute.
fn attr<'a>(attrs: &'a [(String, String)], key: &str) -> Result<&'a str, SketchError> {
    attrs
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
        .ok_or_else(|| SketchError::Xml(format!("missing attribute '{key}'")))
}

/// Rejects attributes outside the expected set.
fn require_only(
    attrs: &[(String, String)],
    allowed: &[&str],
    element: &str,
) -> Result<(), SketchError> {
    for (k, _) in attrs {
        if !allowed.contains(&k.as_str()) {
            return Err(SketchError::Xml(format!(
                "unknown attribute '{k}' on <{element}>"
            )));
        }
    }
    Ok(())
}

fn parse_f64(v: &str, what: &str) -> Result<f64, SketchError> {
    let x: f64 = v
        .parse()
        .map_err(|_| SketchError::Xml(format!("cannot parse '{v}' as a number in {what}")))?;
    if x.is_finite() {
        Ok(x)
    } else {
        Err(SketchError::Xml(format!(
            "non-finite value '{v}' in {what}"
        )))
    }
}

fn parse_u32(v: &str, what: &str) -> Result<u32, SketchError> {
    v.parse()
        .map_err(|_| SketchError::Xml(format!("cannot parse '{v}' as an integer in {what}")))
}

fn parse_geo(v: &str, what: &str) -> Result<GeoId, SketchError> {
    Ok(GeoId(parse_u32(v, what)?))
}

fn parse_pos(v: &str, what: &str) -> Result<PointPos, SketchError> {
    let o = parse_u32(v, what)?;
    PointPos::from_ordinal(o)
        .ok_or_else(|| SketchError::Xml(format!("invalid point position '{o}' in {what}")))
}

/// A parsed tag: its name, attributes, and whether it was self-closing.
type Tag<'a> = (&'a str, Vec<(String, String)>, bool);

/// A minimal cursor over the XML text. All delimiters the scanner looks
/// for are ASCII, so byte-level scanning is boundary-safe on `&str`.
struct Cursor<'a> {
    s: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn new(s: &'a str) -> Self {
        Cursor {
            s: s.as_bytes(),
            pos: 0,
        }
    }

    fn at_end(&self) -> bool {
        self.pos >= self.s.len()
    }

    fn peek(&self) -> Option<u8> {
        self.s.get(self.pos).copied()
    }

    fn skip_ws(&mut self) {
        while let Some(b) = self.peek() {
            if b.is_ascii_whitespace() {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    fn consume_prefix(&mut self, prefix: &str) -> bool {
        if self.s[self.pos..].starts_with(prefix.as_bytes()) {
            self.pos += prefix.len();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, c: u8) -> Result<(), SketchError> {
        if self.peek() == Some(c) {
            self.pos += 1;
            Ok(())
        } else {
            Err(SketchError::Xml(format!(
                "expected '{}' at byte {}",
                c as char, self.pos
            )))
        }
    }

    /// Skips forward past the next literal `marker`.
    fn skip_until(&mut self, marker: &str) -> Result<(), SketchError> {
        let hay = &self.s[self.pos..];
        let needle = marker.as_bytes();
        let found = hay
            .windows(needle.len())
            .position(|w| w == needle)
            .ok_or_else(|| {
                SketchError::Xml(format!("missing closing '{marker}' in declaration"))
            })?;
        self.pos += found + needle.len();
        Ok(())
    }

    /// Reads a name (stops at whitespace, `/`, `>`, `=`, `"`).
    fn read_name(&mut self) -> Result<&'a str, SketchError> {
        let start = self.pos;
        while let Some(b) = self.peek() {
            if b.is_ascii_whitespace() || matches!(b, b'/' | b'>' | b'=' | b'"') {
                break;
            }
            self.pos += 1;
        }
        if self.pos == start {
            return Err(SketchError::Xml(format!("expected a name at byte {start}")));
        }
        std::str::from_utf8(&self.s[start..self.pos])
            .map_err(|_| SketchError::Xml("invalid UTF-8".to_string()))
    }

    /// Parses a tag starting after `<`: `(name, attrs, self_closing)`.
    fn parse_tag(&mut self) -> Result<Tag<'a>, SketchError> {
        let name = self.read_name()?;
        let mut attrs = Vec::new();
        loop {
            self.skip_ws();
            match self.peek() {
                None => {
                    return Err(SketchError::Xml(
                        "unexpected end of input inside a tag".to_string(),
                    ))
                }
                Some(b'/') => {
                    self.pos += 1;
                    self.expect(b'>')?;
                    return Ok((name, attrs, true));
                }
                Some(b'>') => {
                    self.pos += 1;
                    return Ok((name, attrs, false));
                }
                Some(_) => {
                    let aname = self.read_name()?.to_string();
                    self.skip_ws();
                    self.expect(b'=')?;
                    self.skip_ws();
                    self.expect(b'"')?;
                    let vstart = self.pos;
                    while let Some(b) = self.peek() {
                        if b == b'"' {
                            break;
                        }
                        self.pos += 1;
                    }
                    if self.at_end() {
                        return Err(SketchError::Xml("unterminated attribute value".to_string()));
                    }
                    let value = std::str::from_utf8(&self.s[vstart..self.pos])
                        .map_err(|_| SketchError::Xml("invalid UTF-8".to_string()))?
                        .to_string();
                    self.expect(b'"')?;
                    attrs.push((aname, value));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::PointId;

    /// A sketch using every geometry kind and every constraint variant.
    fn kitchen_sink() -> Sketch {
        let mut sk = Sketch::new();
        let p = sk.add_geometry(Geometry::Point { x: 0.5, y: -0.5 }); // 0
        let l1 = sk.add_geometry(Geometry::LineSegment {
            start: Point::new(0.0, 0.0),
            end: Point::new(4.0, 3.0),
        }); // 1
        let l2 = sk.add_geometry(Geometry::LineSegment {
            start: Point::new(-1.0, 2.0),
            end: Point::new(2.0, -2.0),
        }); // 2
        let c = sk.add_geometry(Geometry::Circle {
            center: Point::new(1.0, 1.0),
            radius: 2.5,
        }); // 3
        let arc = sk.add_geometry(Geometry::ArcOfCircle {
            center: Point::new(-2.0, -2.0),
            radius: 1.25,
            start_angle: 0.1,
            end_angle: 2.3,
        }); // 4

        sk.add_constraint(Constraint::Coincident {
            g1: p,
            pos1: PointPos::Start,
            g2: l1,
            pos2: PointPos::Start,
        })
        .unwrap(); // 0
        sk.add_constraint(Constraint::Horizontal { g: l1 }).unwrap(); // 1
        sk.add_constraint(Constraint::Vertical { g: l2 }).unwrap(); // 2
        sk.add_constraint(Constraint::Parallel { g1: l1, g2: l2 })
            .unwrap(); // 3
        sk.add_constraint(Constraint::Perpendicular { g1: l1, g2: l2 })
            .unwrap(); // 4
        sk.add_constraint(Constraint::Distance {
            g1: p,
            pos1: PointPos::Start,
            g2: l1,
            pos2: PointPos::End,
            value: 3.5,
        })
        .unwrap(); // 5
        sk.add_constraint(Constraint::DistanceX {
            g1: p,
            pos1: PointPos::Start,
            g2: l2,
            pos2: PointPos::End,
            value: -1.5,
        })
        .unwrap(); // 6
        sk.add_constraint(Constraint::DistanceY {
            g1: p,
            pos1: PointPos::Start,
            g2: l2,
            pos2: PointPos::End,
            value: 0.25,
        })
        .unwrap(); // 7
        sk.add_constraint(Constraint::Radius { g: c, value: 2.5 })
            .unwrap(); // 8
        sk.add_constraint(Constraint::Diameter { g: arc, value: 2.5 })
            .unwrap(); // 9
        sk.add_constraint(Constraint::Equal { g1: c, g2: arc })
            .unwrap(); // 10
        sk.add_constraint(Constraint::Angle {
            g1: l1,
            g2: l2,
            value: 1.25,
        })
        .unwrap(); // 11
        sk.add_constraint(Constraint::PointOnObject {
            g1: p,
            pos1: PointPos::Start,
            g2: c,
        })
        .unwrap(); // 12
        sk
    }

    #[test]
    fn xml_roundtrip_all_geometry_and_constraint_kinds() {
        let sk = kitchen_sink();
        let xml = sk.to_xml();
        let back = from_xml(&xml).unwrap();
        assert_eq!(back.geometry(), sk.geometry());
        assert_eq!(back.constraints(), sk.constraints());
        assert_eq!(back, sk);
        // Ids are positions, so a second roundtrip is byte-stable.
        assert_eq!(back.to_xml(), xml);
    }

    #[test]
    fn xml_emits_documented_schema() {
        let mut sk = Sketch::new();
        sk.add_geometry(Geometry::LineSegment {
            start: Point::new(0.0, 0.0),
            end: Point::new(4.0, 3.0),
        });
        sk.add_geometry(Geometry::Point { x: 1.5, y: -2.5 });
        sk.add_geometry(Geometry::Circle {
            center: Point::new(1.0, 1.0),
            radius: 2.5,
        });
        sk.add_geometry(Geometry::ArcOfCircle {
            center: Point::new(0.0, 0.0),
            radius: 1.0,
            start_angle: 0.0,
            end_angle: 1.5,
        });
        sk.add_constraint(Constraint::Horizontal { g: GeoId(0) })
            .unwrap();
        sk.add_constraint(Constraint::Coincident {
            g1: GeoId(1),
            pos1: PointPos::Start,
            g2: GeoId(0),
            pos2: PointPos::End,
        })
        .unwrap();
        sk.add_constraint(Constraint::Radius {
            g: GeoId(2),
            value: 2.5,
        })
        .unwrap();
        let expected = "<Sketch>\n\
          \x20 <Geometry type=\"1\" startx=\"0.0\" starty=\"0.0\" endx=\"4.0\" endy=\"3.0\"/>\n\
          \x20 <Geometry type=\"0\" x=\"1.5\" y=\"-2.5\"/>\n\
          \x20 <Geometry type=\"2\" centerx=\"1.0\" centery=\"1.0\" radius=\"2.5\"/>\n\
          \x20 <Geometry type=\"3\" centerx=\"0.0\" centery=\"0.0\" radius=\"1.0\" startangle=\"0.0\" endangle=\"1.5\"/>\n\
          \x20 <Constraint type=\"1\" first=\"0\"/>\n\
          \x20 <Constraint type=\"0\" first=\"1\" firstpos=\"1\" second=\"0\" secondpos=\"2\"/>\n\
          \x20 <Constraint type=\"8\" first=\"2\" value=\"2.5\"/>\n\
          </Sketch>\n";
        assert_eq!(sk.to_xml(), expected);
    }

    #[test]
    fn xml_roundtrip_is_bit_exact_for_tricky_floats() {
        let mut sk = Sketch::new();
        let tricky = [0.1, -0.0, 1e-300, 1e300, 1.0 / 3.0, -1234.56789012345e-7];
        for (i, v) in tricky.iter().enumerate() {
            sk.add_geometry(Geometry::Point {
                x: *v,
                y: tricky[(i + 1) % tricky.len()],
            });
        }
        let back = from_xml(&sk.to_xml()).unwrap();
        assert_eq!(back.geometry(), sk.geometry());
    }

    #[test]
    fn xml_accepts_declaration_whitespace_and_root_attributes() {
        let xml = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
                   <Sketch name=\"demo\" version=\"1\">\n  \
                   <Geometry type=\"0\" x=\"1.0\" y=\"2.0\"/>\n\
                   </Sketch>\n";
        let sk = from_xml(xml).unwrap();
        assert_eq!(sk.geometry().len(), 1);
        assert_eq!(
            sk.point(PointId::new(GeoId(0), PointPos::Start)),
            Some(Point::new(1.0, 2.0))
        );
    }

    #[test]
    fn xml_accepts_empty_and_self_closing_sketch() {
        assert_eq!(from_xml("<Sketch>\n</Sketch>\n").unwrap(), Sketch::new());
        assert_eq!(from_xml("  <Sketch/>  ").unwrap(), Sketch::new());
        // An entirely empty string is *not* a sketch: no root element.
        assert!(matches!(from_xml(""), Err(SketchError::Xml(_))));
        assert!(matches!(from_xml("   "), Err(SketchError::Xml(_))));
    }

    #[test]
    fn xml_rejects_wrong_root_and_structure() {
        assert!(matches!(from_xml("hello"), Err(SketchError::Xml(_))));
        assert!(matches!(from_xml("<Foo/>"), Err(SketchError::Xml(_))));
        // Sanity: this well-formed input is accepted.
        assert!(from_xml("<Sketch><Geometry type=\"0\" x=\"0.0\" y=\"0.0\"/></Sketch>").is_ok());
        // Unclosed sketch.
        assert!(matches!(
            from_xml("<Sketch><Geometry type=\"0\" x=\"0.0\" y=\"0.0\"/>"),
            Err(SketchError::Xml(_))
        ));
        // Non-self-closing child element.
        assert!(matches!(
            from_xml("<Sketch><Geometry type=\"0\" x=\"0.0\" y=\"0.0\"></Geometry></Sketch>"),
            Err(SketchError::Xml(_))
        ));
        // Unknown child element.
        assert!(matches!(
            from_xml("<Sketch><Bogus/></Sketch>"),
            Err(SketchError::Xml(_))
        ));
        // Text content inside the sketch.
        assert!(matches!(
            from_xml("<Sketch>some text</Sketch>"),
            Err(SketchError::Xml(_))
        ));
        // Trailing junk.
        assert!(matches!(
            from_xml("<Sketch/><extra/>"),
            Err(SketchError::Xml(_))
        ));
        // Bad close tag.
        assert!(matches!(
            from_xml("<Sketch></Other>"),
            Err(SketchError::Xml(_))
        ));
    }

    #[test]
    fn xml_rejects_bad_attributes_and_values() {
        // Unknown geometry type ordinal.
        assert!(matches!(
            from_xml("<Sketch><Geometry type=\"9\" x=\"0.0\" y=\"0.0\"/></Sketch>"),
            Err(SketchError::Xml(_))
        ));
        // Unknown constraint type ordinal.
        assert!(matches!(
            from_xml(
                "<Sketch><Geometry type=\"1\" startx=\"0.0\" starty=\"0.0\" endx=\"1.0\" endy=\"1.0\"/>\
                 <Constraint type=\"13\" first=\"0\"/></Sketch>"
            ),
            Err(SketchError::Xml(_))
        ));
        // Missing attribute.
        assert!(matches!(
            from_xml("<Sketch><Geometry type=\"1\" startx=\"0.0\" starty=\"0.0\" endx=\"1.0\"/></Sketch>"),
            Err(SketchError::Xml(_))
        ));
        // Unknown attribute.
        assert!(matches!(
            from_xml("<Sketch><Geometry type=\"0\" x=\"0.0\" y=\"0.0\" z=\"0.0\"/></Sketch>"),
            Err(SketchError::Xml(_))
        ));
        // Non-numeric value.
        assert!(matches!(
            from_xml("<Sketch><Geometry type=\"0\" x=\"abc\" y=\"0.0\"/></Sketch>"),
            Err(SketchError::Xml(_))
        ));
        // Non-finite value.
        assert!(matches!(
            from_xml("<Sketch><Geometry type=\"0\" x=\"NaN\" y=\"0.0\"/></Sketch>"),
            Err(SketchError::Xml(_))
        ));
        assert!(matches!(
            from_xml("<Sketch><Geometry type=\"0\" x=\"inf\" y=\"0.0\"/></Sketch>"),
            Err(SketchError::Xml(_))
        ));
        // Negative radius on load.
        assert!(matches!(
            from_xml("<Sketch><Geometry type=\"2\" centerx=\"0.0\" centery=\"0.0\" radius=\"-1.0\"/></Sketch>"),
            Err(SketchError::Xml(_))
        ));
        // Bad point position ordinal.
        assert!(matches!(
            from_xml(
                "<Sketch><Geometry type=\"1\" startx=\"0.0\" starty=\"0.0\" endx=\"1.0\" endy=\"1.0\"/>\
                 <Constraint type=\"1\" first=\"0\" firstpos=\"0\"/></Sketch>"
            ),
            Err(SketchError::Xml(_))
        ));
        // Missing constraint attribute.
        assert!(matches!(
            from_xml(
                "<Sketch><Geometry type=\"2\" centerx=\"0.0\" centery=\"0.0\" radius=\"1.0\"/>\
                 <Constraint type=\"8\" first=\"0\"/></Sketch>"
            ),
            Err(SketchError::Xml(_))
        ));
    }

    #[test]
    fn xml_rejects_bad_references_via_validation() {
        // Constraint refers to a geometry index that does not exist.
        let err = from_xml(
            "<Sketch><Geometry type=\"1\" startx=\"0.0\" starty=\"0.0\" endx=\"1.0\" endy=\"1.0\"/>\
             <Constraint type=\"1\" first=\"7\"/></Sketch>",
        )
        .unwrap_err();
        assert_eq!(err, SketchError::UnknownGeometry(GeoId(7)));

        // Radius constraint on a line: unsupported combination.
        let err = from_xml(
            "<Sketch><Geometry type=\"1\" startx=\"0.0\" starty=\"0.0\" endx=\"1.0\" endy=\"1.0\"/>\
             <Constraint type=\"8\" first=\"0\" value=\"2.0\"/></Sketch>",
        )
        .unwrap_err();
        assert!(matches!(err, SketchError::UnsupportedConstraint(_)));

        // Negative Distance value: unsatisfiable.
        let err = from_xml(
            "<Sketch><Geometry type=\"1\" startx=\"0.0\" starty=\"0.0\" endx=\"1.0\" endy=\"1.0\"/>\
             <Constraint type=\"5\" first=\"0\" firstpos=\"1\" second=\"0\" secondpos=\"2\" value=\"-2.0\"/></Sketch>",
        )
        .unwrap_err();
        assert!(matches!(err, SketchError::InvalidValue { .. }));

        // Constraint appearing before the geometry it references.
        let err = from_xml(
            "<Sketch><Constraint type=\"1\" first=\"0\"/>\
             <Geometry type=\"1\" startx=\"0.0\" starty=\"0.0\" endx=\"1.0\" endy=\"1.0\"/></Sketch>",
        )
        .unwrap_err();
        assert_eq!(err, SketchError::UnknownGeometry(GeoId(0)));
    }

    #[test]
    fn xml_roundtrip_through_solve_scenario() {
        // Serialize a *solved* sketch and check the solution survives.
        let mut sk = Sketch::new();
        let l1 = sk.add_geometry(Geometry::LineSegment {
            start: Point::new(-0.05, 3.05),
            end: Point::new(4.05, 2.9),
        });
        let l2 = sk.add_geometry(Geometry::LineSegment {
            start: Point::new(4.05, 2.9),
            end: Point::new(3.95, -0.1),
        });
        let l3 = sk.add_geometry(Geometry::LineSegment {
            start: Point::new(3.95, -0.1),
            end: Point::new(0.1, 0.05),
        });
        let l4 = sk.add_geometry(Geometry::LineSegment {
            start: Point::new(0.1, 0.05),
            end: Point::new(-0.05, 3.05),
        });
        for (g1, p1, g2, p2) in [
            (l1, PointPos::End, l2, PointPos::Start),
            (l2, PointPos::End, l3, PointPos::Start),
            (l3, PointPos::End, l4, PointPos::Start),
            (l4, PointPos::End, l1, PointPos::Start),
        ] {
            sk.add_constraint(Constraint::Coincident {
                g1,
                pos1: p1,
                g2,
                pos2: p2,
            })
            .unwrap();
        }
        for g in [l1, l3] {
            sk.add_constraint(Constraint::Horizontal { g }).unwrap();
        }
        for g in [l2, l4] {
            sk.add_constraint(Constraint::Vertical { g }).unwrap();
        }
        sk.add_constraint(Constraint::DistanceX {
            g1: l1,
            pos1: PointPos::Start,
            g2: l1,
            pos2: PointPos::End,
            value: 4.0,
        })
        .unwrap();
        sk.add_constraint(Constraint::DistanceY {
            g1: l4,
            pos1: PointPos::Start,
            g2: l4,
            pos2: PointPos::End,
            value: 3.0,
        })
        .unwrap();
        assert!(matches!(
            sk.solve().unwrap(),
            crate::SolveResult::Solved { .. }
        ));

        let back = from_xml(&sk.to_xml()).unwrap();
        assert_eq!(back, sk);
        // And the reloaded sketch is still solved-equivalent: re-solving is
        // a zero-iteration no-op.
        let mut back = back;
        match back.solve().unwrap() {
            crate::SolveResult::Solved { iterations, .. } => assert_eq!(iterations, 0),
            other => panic!("expected Solved, got {other:?}"),
        }
    }
}

#[cfg(test)]
mod tangent_symmetric_block_tests {
    use super::*;
    use crate::geometry::{Geometry, Point};

    /// The three FreeCAD-native kinds round-trip through the fragment
    /// format bit-for-bit, including Symmetric's third slot (about-line
    /// without `thirdpos`, about-point with it).
    #[test]
    fn tangent_symmetric_block_round_trip_bit_exact() {
        let mut sk = Sketch::new();
        let l = sk.add_geometry(Geometry::LineSegment {
            start: Point::new(0.0, 0.0),
            end: Point::new(10.0, 0.0),
        }); // 0
        let c = sk.add_geometry(Geometry::Circle {
            center: Point::new(5.0, 4.0),
            radius: 4.0,
        }); // 1
        let p1 = sk.add_geometry(Geometry::Point { x: 0.0, y: 20.0 }); // 2
        let p2 = sk.add_geometry(Geometry::Point { x: 0.0, y: -20.0 }); // 3
        let ps = sk.add_geometry(Geometry::Point { x: 15.0, y: 30.0 }); // 4
        sk.add_constraint(Constraint::Tangent { g1: l, g2: c })
            .unwrap();
        sk.add_constraint(Constraint::Symmetric {
            g1: p1,
            pos1: PointPos::Start,
            g2: p2,
            pos2: PointPos::End,
            g3: l,
            pos3: None,
        })
        .unwrap();
        sk.add_constraint(Constraint::Symmetric {
            g1: p1,
            pos1: PointPos::Center,
            g2: p2,
            pos2: PointPos::Center,
            g3: ps,
            pos3: Some(PointPos::Center),
        })
        .unwrap();
        sk.add_constraint(Constraint::Block { g: ps }).unwrap();

        let xml = sk.to_xml();
        let back = from_xml(&xml).unwrap();
        assert_eq!(back.to_xml(), xml, "fragment round-trips bit-exactly");
        assert_eq!(back.constraints().len(), 4);
        assert!(matches!(
            back.constraints()[0],
            Constraint::Tangent { g1, g2 } if g1.0 == 0 && g2.0 == 1
        ));
        assert!(matches!(
            back.constraints()[1],
            Constraint::Symmetric { pos3: None, g3, .. } if g3.0 == 0
        ));
        assert!(matches!(
            back.constraints()[2],
            Constraint::Symmetric {
                pos3: Some(PointPos::Center),
                g3,
                ..
            } if g3.0 == 4
        ));
        assert!(matches!(
            back.constraints()[3],
            Constraint::Block { g } if g.0 == 4
        ));

        // The emitted fragment carries the documented shape: type 13/14/15,
        // with `thirdpos` present only in the about-point Symmetric.
        assert!(xml.contains(r#"<Constraint type="13" first="0" second="1"/>"#));
        assert!(xml.contains(
            r#"<Constraint type="14" first="2" firstpos="1" second="3" secondpos="2" third="0"/>"#
        ));
        assert!(xml.contains(r#"<Constraint type="14" first="2" firstpos="3" second="3" secondpos="3" third="4" thirdpos="3"/>"#));
        assert!(xml.contains(r#"<Constraint type="15" first="4"/>"#));
    }

    /// The strict parser rejects unknown attribute shapes on the new
    /// kinds (the honest-error contract of the fragment grammar).
    #[test]
    fn new_kinds_reject_unknown_attributes() {
        assert!(from_xml(
            "<Sketch>\n  <Geometry type=\"0\" x=\"0\" y=\"0\"/>\n  \
             <Constraint type=\"13\" first=\"0\" second=\"0\" value=\"1\"/>\n</Sketch>\n"
        )
        .is_err());
        assert!(from_xml(
            "<Sketch>\n  <Geometry type=\"0\" x=\"0\" y=\"0\"/>\n  \
             <Constraint type=\"14\" first=\"0\" firstpos=\"1\" second=\"0\" secondpos=\"1\"/>\n</Sketch>\n"
        )
        .is_err());
        assert!(from_xml(
            "<Sketch>\n  <Geometry type=\"0\" x=\"0\" y=\"0\"/>\n  \
             <Constraint type=\"15\" first=\"0\" second=\"0\"/>\n</Sketch>\n"
        )
        .is_err());
    }
}
