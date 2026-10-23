//! Typed property values, mirroring FreeCAD's `App::Property*` family.

use fc_base::{Placement, Rotation, Vector3};

/// A property value. Variant names map to FreeCAD property types via
/// [`Property::type_name`].
#[derive(Debug, Clone, PartialEq)]
pub enum Property {
    Bool(bool),
    Float(f64),
    Integer(i64),
    String(String),
    Vector(Vector3),
    Placement(Placement),
    /// Link to another object, by its unique internal name.
    Link(String),
    LinkList(Vec<String>),
    /// FreeCAD `App::PropertyLinkSub`: a link to another object plus a
    /// list of subelement names (faces/edges of the target's shape,
    /// e.g. `("Box", ["Face3"])` — `Part::Mirroring`'s `MirrorPlane`
    /// and `PartDesign` profiles carry it). The target name plus the
    /// subelement names, in file order.
    LinkSub(String, Vec<String>),
}

impl Property {
    /// FreeCAD-compatible type name, e.g. `App::PropertyFloat`.
    pub fn type_name(&self) -> &'static str {
        match self {
            Property::Bool(_) => "App::PropertyBool",
            Property::Float(_) => "App::PropertyFloat",
            Property::Integer(_) => "App::PropertyInteger",
            Property::String(_) => "App::PropertyString",
            Property::Vector(_) => "App::PropertyVector",
            Property::Placement(_) => "App::PropertyPlacement",
            Property::Link(_) => "App::PropertyLink",
            Property::LinkList(_) => "App::PropertyLinkList",
            Property::LinkSub(_, _) => "App::PropertyLinkSub",
        }
    }

    /// Human/CLI text form, exactly re-parsed by [`Property::parse`]:
    ///
    /// | variant     | text form                                  |
    /// |-------------|--------------------------------------------|
    /// | `Bool`      | `true` / `false`                           |
    /// | `Float`     | shortest round-trip form (`2.5`)          |
    /// | `Integer`   | `7`                                        |
    /// | `String`    | raw, unchanged                             |
    /// | `Vector`    | `x,y,z`                                    |
    /// | `Placement` | `pos=(x,y,z);axis=(x,y,z);angle=deg`      |
    /// | `Link`      | the target name, verbatim                  |
    /// | `LinkList`  | target names, comma-joined verbatim       |
    /// | `LinkSub`   | `name` / `name[sub,sub]`, verbatim        |
    ///
    /// Placement angles are degrees rounded to at most 4 decimals; axis
    /// components to at most 12 (see `format_axis` below for why), so that
    /// `to_text(parse(t)) == t` for every text `t` this method emits.
    pub fn to_text(&self) -> String {
        match self {
            Property::Bool(value) => value.to_string(),
            Property::Float(value) => format!("{value}"),
            Property::Integer(value) => format!("{value}"),
            Property::String(value) => value.clone(),
            Property::Vector(v) => format!("{},{},{}", v.x, v.y, v.z),
            Property::Placement(p) => {
                let (axis, degrees) = axis_angle_of(&p.rotation);
                let degrees = format_degrees(degrees);
                // A rotation whose angle quantizes to a full turn ("360")
                // or to zero ("0") is indistinguishable from the identity
                // at this format's 4-decimal angle resolution; emit the
                // canonical identity form so `to_text(parse(t)) == t`
                // holds for every text `t` this method emits.
                let (axis, degrees) = if degrees == "360" || degrees == "0" {
                    (Vector3::Z, "0".to_string())
                } else {
                    (axis, degrees)
                };
                format!(
                    "pos=({},{},{});axis=({},{},{});angle={}",
                    p.position.x,
                    p.position.y,
                    p.position.z,
                    format_axis(axis.x),
                    format_axis(axis.y),
                    format_axis(axis.z),
                    degrees
                )
            }
            Property::Link(name) => name.clone(),
            Property::LinkList(names) => names.join(","),
            Property::LinkSub(name, subs) => {
                if subs.is_empty() {
                    name.clone()
                } else {
                    format!("{name}[{}]", subs.join(","))
                }
            }
        }
    }

    /// Parse a CLI value (from `freecad-rs set`) for the property with the
    /// given FreeCAD type name. Returns `None` on unknown type or malformed
    /// text. Whitespace around scalars is tolerated; link names are taken
    /// verbatim so `to_text` output always re-parses identically.
    pub fn parse(type_name: &str, text: &str) -> Option<Property> {
        match type_name {
            "App::PropertyBool" => match text.trim().to_ascii_lowercase().as_str() {
                "true" => Some(Property::Bool(true)),
                "false" => Some(Property::Bool(false)),
                _ => None,
            },
            "App::PropertyFloat" => text.trim().parse::<f64>().ok().map(Property::Float),
            "App::PropertyInteger" => text.trim().parse::<i64>().ok().map(Property::Integer),
            "App::PropertyString" => Some(Property::String(text.to_string())),
            "App::PropertyVector" => parse_vector(text).map(Property::Vector),
            "App::PropertyPlacement" => parse_placement(text).map(Property::Placement),
            "App::PropertyLink" => Some(Property::Link(text.to_string())),
            "App::PropertyLinkSub" => {
                // `name` or `name[sub,sub]` — the exact inverse of the
                // `LinkSub` text form. Names and subelements are taken
                // verbatim (FreeCAD internal names cannot contain `[`,
                // `]` or `,`... they are `[A-Za-z0-9_]`; subelement
                // names like `Face3`/`Edge7` cannot either).
                let trimmed = text.trim();
                if let Some(open) = trimmed.find('[') {
                    if !trimmed.ends_with(']') || open + 1 >= trimmed.len() - 1 {
                        return None;
                    }
                    let inner = &trimmed[open + 1..trimmed.len() - 1];
                    let mut subs = Vec::new();
                    for part in inner.split(',') {
                        let sub = part.trim();
                        if sub.is_empty() {
                            return None;
                        }
                        subs.push(sub.to_string());
                    }
                    Some(Property::LinkSub(trimmed[..open].trim().to_string(), subs))
                } else {
                    Some(Property::LinkSub(trimmed.to_string(), Vec::new()))
                }
            }
            "App::PropertyLinkList" => {
                // Tolerant on the way in, invertible on the way out:
                // entries are trimmed; an empty entry inside the list is
                // malformed, but a single trailing empty entry is kept so
                // that `to_text` of a list ending in an empty name (e.g.
                // `"a,"`) re-parses to the same value. The empty string
                // itself denotes the empty list.
                let trimmed = text.trim();
                if trimmed.is_empty() {
                    return Some(Property::LinkList(Vec::new()));
                }
                let parts: Vec<&str> = trimmed.split(',').collect();
                let last = parts.len() - 1;
                let mut names = Vec::with_capacity(parts.len());
                for (i, part) in parts.iter().enumerate() {
                    let name = part.trim();
                    if name.is_empty() && i != last {
                        return None;
                    }
                    names.push(name.to_string());
                }
                Some(Property::LinkList(names))
            }
            _ => None,
        }
    }
}

impl Default for Property {
    fn default() -> Self {
        Property::Bool(false)
    }
}

/// Parse `x,y,z` (whitespace around scalars tolerated).
fn parse_vector(text: &str) -> Option<Vector3> {
    let mut parts = text.trim().split(',');
    let x = parts.next()?.trim().parse().ok()?;
    let y = parts.next()?.trim().parse().ok()?;
    let z = parts.next()?.trim().parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some(Vector3::new(x, y, z))
}

/// Parse `(x,y,z)`.
fn parse_paren_vector(text: &str) -> Option<Vector3> {
    parse_vector(text.trim().strip_prefix('(')?.strip_suffix(')')?)
}

/// Parse `pos=(x,y,z);axis=(x,y,z);angle=deg` — the exact inverse of the
/// [`Property::Placement`] text form. Segments may appear in any order.
fn parse_placement(text: &str) -> Option<Placement> {
    let mut position: Option<Vector3> = None;
    let mut axis: Option<Vector3> = None;
    let mut angle_deg: Option<f64> = None;
    for segment in text.split(';') {
        let segment = segment.trim();
        if segment.is_empty() {
            continue;
        }
        let (key, value) = segment.split_once('=')?;
        match key.trim() {
            "pos" if position.is_none() => position = Some(parse_paren_vector(value)?),
            "axis" if axis.is_none() => axis = Some(parse_paren_vector(value)?),
            "angle" if angle_deg.is_none() => {
                let degrees: f64 = value.trim().parse().ok()?;
                angle_deg = Some(degrees);
            }
            // Duplicate or unknown key: malformed.
            _ => return None,
        }
    }
    let rotation = Rotation::from_axis_angle(axis?, angle_deg?.to_radians()).ok()?;
    Some(Placement::new(position?, rotation))
}

/// Axis-angle view `(axis, degrees)` of a unit quaternion, FreeCAD
/// `Base::Rotation::getValue(axis, angle)` style: the angle covers the
/// full turn `[0, 360)`. (Near-)identity rotations canonicalize to
/// `((0, 0, 1), 0)`.
fn axis_angle_of(rotation: &Rotation) -> (Vector3, f64) {
    let vector_len =
        (rotation.x * rotation.x + rotation.y * rotation.y + rotation.z * rotation.z).sqrt();
    if vector_len < 1e-12 {
        return (Vector3::Z, 0.0);
    }
    let angle = 2.0 * vector_len.atan2(rotation.w);
    let axis = Vector3::new(
        rotation.x / vector_len,
        rotation.y / vector_len,
        rotation.z / vector_len,
    );
    (axis, angle.to_degrees())
}

/// Degrees with at most 4 decimals and trailing zeros trimmed
/// (`90`, `45.25`, `33.3333`).
fn format_degrees(degrees: f64) -> String {
    quantize_text(degrees, 4)
}

/// Axis component with at most 12 decimals and trailing zeros trimmed.
///
/// Unlike positions (which re-parse exactly, so they keep full `f64`
/// precision), the axis travels through `normalize -> sin -> normalize`
/// on parse, which perturbs it by a few ulps (~1e-15). A 12-decimal grid
/// (half-step 5e-13, ~800x that drift) makes the printed value a grid
/// point that re-quantizes to itself, guaranteeing the text round-trip —
/// while still pinning the direction ~8 decimals finer than the angle.
fn format_axis(component: f64) -> String {
    quantize_text(component, 12)
}

/// `value` rounded to at most `decimals` decimals with trailing zeros
/// (and a trailing `.`) trimmed.
fn quantize_text(value: f64, decimals: usize) -> String {
    let mut text = format!("{value:.decimals$}");
    if text.contains('.') {
        while text.ends_with('0') {
            text.pop();
        }
        if text.ends_with('.') {
            text.pop();
        }
    }
    text
}
