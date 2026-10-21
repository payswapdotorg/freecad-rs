//! Script value types and coercions.
//!
//! [`Vec3`] is the script's vector constructor type (`vec3(x, y, z)`),
//! used for placements and vector properties. The coercion helpers turn
//! rhai `Dynamic` values into honest kernel inputs: script numbers may be
//! `INT` or `FLOAT` (so `add_box("B", 30, 20, 5)` works like the
//! all-float form), and anything else is an honest "expected a number"
//! error — never a panic.

use rhai::Dynamic;

use fc_base::Vector3;

use crate::session::PlacementVal;

/// A 3D vector for script use (`vec3(x, y, z)`); wraps the kernel's
/// [`Vector3`].
#[derive(Clone)]
pub struct Vec3(pub Vector3);

impl Vec3 {
    pub fn x(&self) -> f64 {
        self.0.x
    }
    pub fn y(&self) -> f64 {
        self.0.y
    }
    pub fn z(&self) -> f64 {
        self.0.z
    }
}

impl std::fmt::Display for Vec3 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "({}, {}, {})", self.0.x, self.0.y, self.0.z)
    }
}

/// Coerce a rhai value into an `f64`: `INT` and `FLOAT` both work
/// (mirroring how a shell argument can be `30` or `30.0`); anything else
/// is an honest error naming the actual type.
pub fn coerce_f64(value: &Dynamic) -> Result<f64, String> {
    if let Ok(i) = value.as_int() {
        return Ok(i as f64);
    }
    if let Ok(f) = value.as_float() {
        return Ok(f);
    }
    Err(format!("expected a number, got {}", rhai_type_name(value)))
}

/// Render a rhai value into the CLI's property text form, from which the
/// `set` path parses it against the property's existing type (`1,2,3`
/// into a Vector, `"Ball"` into a Link, ...). Structured values render
/// through their kernel text forms; anything else is an honest error.
pub fn property_text(value: &Dynamic) -> Result<String, String> {
    if let Ok(b) = value.as_bool() {
        return Ok(b.to_string());
    }
    if let Ok(i) = value.as_int() {
        return Ok(i.to_string());
    }
    if let Ok(f) = value.as_float() {
        return Ok(format!("{f}"));
    }
    if let Ok(s) = value.clone().into_string() {
        return Ok(s);
    }
    if let Some(v) = value.clone().try_cast::<Vec3>() {
        return Ok(format!("{},{},{}", v.0.x, v.0.y, v.0.z));
    }
    if let Some(p) = value.clone().try_cast::<PlacementVal>() {
        // The CLI's Placement text form (Property::to_text), so the
        // parse-by-existing-type path handles it like any other value.
        return Ok(fc_app::Property::Placement(p.0).to_text());
    }
    Err(format!(
        "cannot set a property from {} (use a number, bool, string, vec3 or placement)",
        rhai_type_name(value)
    ))
}

/// Human-readable rhai type name for honest error messages.
fn rhai_type_name(value: &Dynamic) -> String {
    let name = value.type_name();
    if name.contains("Vec3") {
        return "a vec3".to_string();
    }
    if name.contains("PlacementVal") {
        return "a placement".to_string();
    }
    name.to_string()
}
