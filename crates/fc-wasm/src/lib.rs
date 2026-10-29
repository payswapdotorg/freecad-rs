//! `fc-wasm` — the freecad-rs document engine in the browser.
//!
//! A thin, dependency-light boundary over the same crates the CLI
//! drives (`fc-app`/`fc-part`/`fc-sketcher`/`fc-io`): one in-memory
//! document session and a JSON command protocol, exposed through a C
//! ABI designed for `wasm32-unknown-unknown` with no JS-side glue
//! tooling (no wasm-bindgen; the host instantiates the module and
//! shuttles bytes):
//!
//! ```text
//! fc_alloc(len) -> ptr          allocate an input buffer
//! fc_free(ptr, len)             free an input/output buffer
//! fc_set_time(unix_secs)         wall clock for wasm32 (no SystemTime)
//! fc_command(ptr, len) -> u64   run one JSON command; the packed
//!                               return is (result_ptr << 32) | result_len
//! ```
//!
//! The protocol: one command object in, one response object out —
//! `{"op": "...", ...}` → `{"ok": true, "data": ...}` or
//! `{"ok": false, "error": "..."}`. Every mutating op recomputes, so
//! the caller always sees a consistent document.
//!
//! The crate builds and tests natively too (the command interpreter is
//! a plain `rlib`); only the C ABI entry points are wasm-oriented.

use std::cell::RefCell;

use fc_app::{Document, Property};
use fc_base::{Placement, Rotation, Vector3};
use fc_part::features::{mesh_of, shape_of};
use fc_part::{
    BooleanFeature, BooleanKind, BoxFeature, ConeFeature, CylinderFeature, Mesh, MirroringFeature,
    SphereFeature, TopoShape, TorusFeature, TransformFeature,
};
use fc_sketcher::{Constraint, Geometry, Point as SkPoint, PointPos, Sketch, SketchObject};
use serde_json::{json, Value};

thread_local! {
    static SESSION: RefCell<Option<Document>> = const { RefCell::new(None) };
}

/// Run `f` with the open document, or fail with the open-a-document
/// error message.
fn with_doc<R>(f: impl FnOnce(&mut Document) -> Result<R, String>) -> Result<R, String> {
    SESSION.with(|slot| {
        let mut slot = slot.borrow_mut();
        match slot.as_mut() {
            Some(doc) => f(doc),
            None => Err("no document open — send {\"op\":\"new\"} first".to_string()),
        }
    })
}

// ---------------------------------------------------------------------------
// The command interpreter
// ---------------------------------------------------------------------------

/// Execute one command object. Panics are caught by the C ABI layer;
/// here errors are values.
pub fn run_command(command: &Value) -> Value {
    let op = command
        .get("op")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let result = match op.as_str() {
        "version" => Ok(json!({
            "version": env!("CARGO_PKG_VERSION"),
            "engine": "freecad-rs",
        })),
        "new" => op_new(command),
        "load" => op_load(command),
        "save" => op_save(),
        "add" => op_add(command),
        "remove" => op_remove(command),
        "set" => op_set(command),
        "get" => op_get(command),
        "set_expr" => op_set_expr(command),
        "clear_expr" => op_clear_expr(command),
        "list" => op_list(),
        "properties" => op_properties(command),
        "recompute" => op_recompute(),
        "stats" => op_stats(command),
        "mesh" => op_mesh(command),
        "export_stl" => op_export_stl(command),
        "export_obj" => op_export_obj(command),
        "undo" => op_undo(),
        "redo" => op_redo(),
        "history" => op_history(),
        _ => Err(format!("unknown op '{op}'")),
    };
    match result {
        Ok(data) => json!({"ok": true, "data": data}),
        Err(error) => json!({"ok": false, "error": error}),
    }
}

/// The object rows behind the tree view.
fn rows(doc: &Document) -> Vec<Value> {
    doc.summary()
        .into_iter()
        .map(|s| {
            json!({
                "name": s.name,
                "type": s.type_name,
                "status": s.status.to_string(),
            })
        })
        .collect()
}

/// Recompute (touched rows first), mapping a failure to its message.
fn recompute(doc: &mut Document) -> Result<(), String> {
    doc.recompute_all().map(|_| ()).map_err(|e| e.to_string())
}

fn op_new(command: &Value) -> Result<Value, String> {
    let label = command
        .get("label")
        .and_then(Value::as_str)
        .unwrap_or("untitled")
        .to_string();
    let doc = Document::new(&label);
    let rows = rows(&doc);
    SESSION.with(|slot| *slot.borrow_mut() = Some(doc));
    Ok(json!({"label": label, "rows": rows}))
}

fn op_load(command: &Value) -> Result<Value, String> {
    let encoded = str_field(command, "fcstd_base64")?;
    let bytes = b64_decode(&encoded)?;
    let (doc, report) =
        fc_io::load_document_from_bytes(&bytes).map_err(|e| format!("load failed: {e}"))?;
    let loaded = report.objects + report.foreign;
    let rows = rows(&doc);
    SESSION.with(|slot| *slot.borrow_mut() = Some(doc));
    Ok(json!({
        "rows": rows,
        "objects": loaded,
        "foreign": report.foreign,
        "warnings": report.warnings.len(),
    }))
}

fn op_save() -> Result<Value, String> {
    with_doc(|doc| {
        let bytes = fc_io::save_document_to_vec(doc).map_err(|e| format!("save failed: {e}"))?;
        let size = bytes.len();
        let encoded = b64_encode(&bytes);
        Ok(json!({"fcstd_base64": encoded, "size": size}))
    })
}

/// Build the feature an `add` command describes, with parameters set
/// before the document sees it (the CLI's rule: the name uniquifier
/// and the dependency graph observe a fully-formed object).
fn build_feature(
    doc: &Document,
    kind: &str,
    name: Option<&str>,
    p: &Value,
) -> Result<Box<dyn fc_app::DocumentObject>, String> {
    fn f32_field(p: &Value, key: &str) -> Result<f64, String> {
        p.get(key)
            .and_then(Value::as_f64)
            .ok_or_else(|| format!("parameter '{key}' missing or not a number"))
    }
    fn vec3(p: &Value, key: &str) -> Result<[f64; 3], String> {
        let v = p
            .get(key)
            .and_then(Value::as_array)
            .ok_or_else(|| format!("parameter '{key}' expects [x, y, z]"))?;
        if v.len() != 3 {
            return Err(format!("parameter '{key}' expects [x, y, z]"));
        }
        Ok([
            v[0].as_f64().ok_or("x not a number")?,
            v[1].as_f64().ok_or("y not a number")?,
            v[2].as_f64().ok_or("z not a number")?,
        ])
    }
    fn link(doc: &Document, p: &Value, key: &str) -> Result<String, String> {
        let name = str_field(p, key)?;
        if doc.get_by_name(&name).is_none() {
            return Err(format!(
                "object '{name}' not found; available: {}",
                doc.summary()
                    .into_iter()
                    .map(|s| s.name)
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        Ok(name)
    }
    Ok(match kind {
        "box" => {
            let mut f = BoxFeature::new(name.unwrap_or("Box"));
            f.container
                .set("Length", Property::Float(f32_field(p, "length")?));
            f.container
                .set("Width", Property::Float(f32_field(p, "width")?));
            f.container
                .set("Height", Property::Float(f32_field(p, "height")?));
            Box::new(f)
        }
        "sphere" => {
            let mut f = SphereFeature::new(name.unwrap_or("Sphere"));
            f.container
                .set("Radius", Property::Float(f32_field(p, "radius")?));
            Box::new(f)
        }
        "cylinder" => {
            let mut f = CylinderFeature::new(name.unwrap_or("Cylinder"));
            f.container
                .set("Radius", Property::Float(f32_field(p, "radius")?));
            f.container
                .set("Height", Property::Float(f32_field(p, "height")?));
            Box::new(f)
        }
        "cone" => {
            let mut f = ConeFeature::new(name.unwrap_or("Cone"));
            f.container
                .set("Radius1", Property::Float(f32_field(p, "radius1")?));
            f.container
                .set("Radius2", Property::Float(f32_field(p, "radius2")?));
            f.container
                .set("Height", Property::Float(f32_field(p, "height")?));
            Box::new(f)
        }
        "torus" => {
            let mut f = TorusFeature::new(name.unwrap_or("Torus"));
            f.container
                .set("Radius1", Property::Float(f32_field(p, "radius1")?));
            f.container
                .set("Radius2", Property::Float(f32_field(p, "radius2")?));
            Box::new(f)
        }
        "cut" | "fuse" | "common" => {
            let boolean_kind = match kind {
                "cut" => BooleanKind::Cut,
                "fuse" => BooleanKind::Fuse,
                _ => BooleanKind::Common,
            };
            let base = link(doc, p, "base")?;
            let tool = link(doc, p, "tool")?;
            let mut f = BooleanFeature::new(
                name.unwrap_or_else(|| boolean_kind.type_name().trim_start_matches("Part::")),
                boolean_kind,
            );
            f.container.set("Base", Property::Link(base));
            f.container.set("Tool", Property::Link(tool));
            Box::new(f)
        }
        "transform" => {
            let source = link(doc, p, "source")?;
            let translate = p
                .get("translate")
                .and_then(Value::as_array)
                .map(|v| {
                    Ok::<_, String>([
                        v.first().and_then(Value::as_f64).unwrap_or(0.0),
                        v.get(1).and_then(Value::as_f64).unwrap_or(0.0),
                        v.get(2).and_then(Value::as_f64).unwrap_or(0.0),
                    ])
                })
                .transpose()?
                .unwrap_or([0.0, 0.0, 0.0]);
            let rotate_axis = p
                .get("rotate_axis")
                .and_then(Value::as_array)
                .map(|v| {
                    Ok::<_, String>([
                        v.first().and_then(Value::as_f64).unwrap_or(0.0),
                        v.get(1).and_then(Value::as_f64).unwrap_or(0.0),
                        v.get(2).and_then(Value::as_f64).unwrap_or(1.0),
                    ])
                })
                .transpose()?
                .unwrap_or([0.0, 0.0, 1.0]);
            let rotate_deg = p.get("rotate_deg").and_then(Value::as_f64).unwrap_or(0.0);
            let scale = p.get("scale").and_then(Value::as_f64).unwrap_or(1.0);
            let rotation = Rotation::from_axis_angle(
                Vector3::new(rotate_axis[0], rotate_axis[1], rotate_axis[2]),
                rotate_deg.to_radians(),
            )
            .map_err(|_| "rotate_axis must be a non-zero vector".to_string())?;
            let mut f = TransformFeature::new(name.unwrap_or("Transform"));
            f.container.set("Source", Property::Link(source));
            f.container.set(
                "Placement",
                Property::Placement(Placement::new(
                    Vector3::new(translate[0], translate[1], translate[2]),
                    rotation,
                )),
            );
            f.container.set("Scale", Property::Float(scale));
            Box::new(f)
        }
        "mirroring" => {
            let source = link(doc, p, "source")?;
            let base = vec3(p, "base")?;
            let normal = vec3(p, "normal")?;
            let mirror_plane = match p.get("mirror_plane") {
                Some(Value::String(_)) => Some(link(doc, p, "mirror_plane")?),
                _ => None,
            };
            let mut f = MirroringFeature::new(name.unwrap_or("Mirrored"));
            f.container.set("Source", Property::Link(source));
            f.container.set(
                "Base",
                Property::Vector(Vector3::new(base[0], base[1], base[2])),
            );
            f.container.set(
                "Normal",
                Property::Vector(Vector3::new(normal[0], normal[1], normal[2])),
            );
            if let Some(plane) = mirror_plane {
                f.container
                    .set("MirrorPlane", Property::LinkSub(plane, Vec::new()));
            }
            Box::new(f)
        }
        "sketch" => {
            let mut sketch = Sketch::new();
            for g in p
                .get("geometry")
                .and_then(Value::as_array)
                .ok_or("sketch expects geometry: [{kind: ...}]")?
            {
                add_sketch_geometry(&mut sketch, g)?;
            }
            Box::new(SketchObject::with_sketch(name.unwrap_or("Sketch"), sketch))
        }
        "pad" | "pocket" | "revolution" => {
            let profile = link(doc, p, "profile")?;
            let mut f = fc_part::partdesign::PadFeature::new(name.unwrap_or("Pad"));
            let feature: Box<dyn fc_app::DocumentObject> = match kind {
                "pad" => {
                    f.container
                        .set("Profile", Property::LinkSub(profile, Vec::new()));
                    f.container.set(
                        "Length",
                        Property::Float(p.get("length").and_then(Value::as_f64).unwrap_or(10.0)),
                    );
                    Box::new(f)
                }
                "pocket" => {
                    let base = link(doc, p, "base")?;
                    let mut f = fc_part::partdesign::PocketFeature::new(name.unwrap_or("Pocket"));
                    f.container
                        .set("Profile", Property::LinkSub(profile, Vec::new()));
                    f.container.set("Base", Property::Link(base));
                    f.container.set(
                        "Length",
                        Property::Float(p.get("length").and_then(Value::as_f64).unwrap_or(5.0)),
                    );
                    Box::new(f)
                }
                _ => {
                    let mut f =
                        fc_part::partdesign::RevolutionFeature::new(name.unwrap_or("Revolution"));
                    f.container
                        .set("Profile", Property::LinkSub(profile, Vec::new()));
                    f.container.set(
                        "Angle",
                        Property::Float(p.get("angle").and_then(Value::as_f64).unwrap_or(360.0)),
                    );
                    f.container
                        .set("Axis", Property::Vector(Vector3::new(0.0, 0.0, 1.0)));
                    Box::new(f)
                }
            };
            feature
        }
        "pattern-linear" | "pattern-polar" => {
            let originals = link(doc, p, "originals")?;
            let base = link(doc, p, "base")?;
            // Accept both JSON integers and whole floats (the studio's
            // number inputs serialize floats).
            let occurrences = p
                .get("occurrences")
                .and_then(|v| v.as_i64().or_else(|| v.as_f64().map(|f| f as i64)))
                .unwrap_or(2);
            let feature: Box<dyn fc_app::DocumentObject> = if kind == "pattern-linear" {
                let mut f =
                    fc_part::partdesign::LinearPatternFeature::new(name.unwrap_or("LinearPattern"));
                f.container
                    .set("Originals", Property::LinkSub(originals, Vec::new()));
                f.container.set("Base", Property::Link(base));
                let direction = match p.get("direction").and_then(Value::as_str) {
                    Some("x") | None => Vector3::new(1.0, 0.0, 0.0),
                    Some("y") => Vector3::new(0.0, 1.0, 0.0),
                    Some("z") => Vector3::Z,
                    Some(other) => {
                        return Err(format!(
                            "parameter 'direction' expects x, y or z (got '{other}')"
                        ))
                    }
                };
                f.container.set("Direction", Property::Vector(direction));
                f.container.set(
                    "Length",
                    Property::Float(p.get("length").and_then(Value::as_f64).unwrap_or(20.0)),
                );
                f.container
                    .set("Occurrences", Property::Integer(occurrences));
                if p.get("reversed").and_then(Value::as_bool) == Some(true) {
                    f.container.set("Reversed", Property::Bool(true));
                }
                Box::new(f)
            } else {
                let mut f =
                    fc_part::partdesign::PolarPatternFeature::new(name.unwrap_or("PolarPattern"));
                f.container
                    .set("Originals", Property::LinkSub(originals, Vec::new()));
                f.container.set("Base", Property::Link(base));
                let axis = match p.get("axis").and_then(Value::as_str) {
                    Some("y") => Vector3::new(0.0, 1.0, 0.0),
                    Some("z") | None => Vector3::Z,
                    Some(other) => {
                        return Err(format!("parameter 'axis' expects y or z (got '{other}')"))
                    }
                };
                f.container.set("Axis", Property::Vector(axis));
                f.container.set(
                    "Angle",
                    Property::Float(p.get("angle").and_then(Value::as_f64).unwrap_or(360.0)),
                );
                f.container
                    .set("Occurrences", Property::Integer(occurrences));
                if p.get("reversed").and_then(Value::as_bool) == Some(true) {
                    f.container.set("Reversed", Property::Bool(true));
                }
                Box::new(f)
            };
            feature
        }
        _ => {
            return Err(format!(
                "unknown kind '{kind}' (box, sphere, cylinder, cone, torus, cut, fuse, \
                 common, transform, mirroring, sketch, pad, pocket, revolution, \
                 pattern-linear, pattern-polar)"
            ))
        }
    })
}

/// One sketch geometry entry (the composite `rectangle` adds the
/// canonical 4-line + 8-constraint recipe).
fn add_sketch_geometry(sketch: &mut Sketch, g: &Value) -> Result<(), String> {
    let kind = g
        .get("kind")
        .and_then(Value::as_str)
        .ok_or("geometry entry expects a kind")?;
    fn num(g: &Value, key: &str) -> Result<f64, String> {
        g.get(key)
            .and_then(Value::as_f64)
            .ok_or_else(|| format!("geometry '{key}' missing or not a number"))
    }
    match kind {
        "rectangle" => {
            let (x, y) = (num(g, "x")?, num(g, "y")?);
            let (w, h) = (num(g, "w")?, num(g, "h")?);
            let ids = [
                sketch.add_geometry(Geometry::LineSegment {
                    start: SkPoint::new(x, y),
                    end: SkPoint::new(x + w, y),
                }),
                sketch.add_geometry(Geometry::LineSegment {
                    start: SkPoint::new(x + w, y),
                    end: SkPoint::new(x + w, y + h),
                }),
                sketch.add_geometry(Geometry::LineSegment {
                    start: SkPoint::new(x + w, y + h),
                    end: SkPoint::new(x, y + h),
                }),
                sketch.add_geometry(Geometry::LineSegment {
                    start: SkPoint::new(x, y + h),
                    end: SkPoint::new(x, y),
                }),
            ];
            for i in 0..4 {
                sketch
                    .add_constraint(Constraint::Coincident {
                        g1: ids[i],
                        pos1: PointPos::End,
                        g2: ids[(i + 1) % 4],
                        pos2: PointPos::Start,
                    })
                    .map_err(|e| format!("constraint: {e:?}"))?;
            }
            sketch
                .add_constraint(Constraint::Horizontal { g: ids[0] })
                .map_err(|e| format!("constraint: {e:?}"))?;
            sketch
                .add_constraint(Constraint::Horizontal { g: ids[2] })
                .map_err(|e| format!("constraint: {e:?}"))?;
            sketch
                .add_constraint(Constraint::Vertical { g: ids[1] })
                .map_err(|e| format!("constraint: {e:?}"))?;
            sketch
                .add_constraint(Constraint::Vertical { g: ids[3] })
                .map_err(|e| format!("constraint: {e:?}"))?;
            sketch
                .add_constraint(Constraint::Distance {
                    g1: ids[0],
                    pos1: PointPos::Start,
                    g2: ids[0],
                    pos2: PointPos::End,
                    value: w.abs(),
                })
                .map_err(|e| format!("constraint: {e:?}"))?;
            sketch
                .add_constraint(Constraint::Distance {
                    g1: ids[1],
                    pos1: PointPos::Start,
                    g2: ids[1],
                    pos2: PointPos::End,
                    value: h.abs(),
                })
                .map_err(|e| format!("constraint: {e:?}"))?;
            Ok(())
        }
        "circle" => {
            sketch.add_geometry(Geometry::Circle {
                center: SkPoint::new(num(g, "cx")?, num(g, "cy")?),
                radius: num(g, "r")?,
            });
            Ok(())
        }
        "line" => {
            sketch.add_geometry(Geometry::LineSegment {
                start: SkPoint::new(num(g, "x1")?, num(g, "y1")?),
                end: SkPoint::new(num(g, "x2")?, num(g, "y2")?),
            });
            Ok(())
        }
        "point" => {
            sketch.add_geometry(Geometry::Point {
                x: num(g, "x")?,
                y: num(g, "y")?,
            });
            Ok(())
        }
        _ => Err(format!(
            "unknown sketch geometry kind '{kind}' (rectangle, circle, line, point)"
        )),
    }
}

fn op_add(command: &Value) -> Result<Value, String> {
    let kind = str_field(command, "kind")?;
    let name = command
        .get("name")
        .and_then(Value::as_str)
        .map(String::from);
    // Parameters under `params`, or flat on the command itself — both
    // forms accepted (the flat form mirrors the CLI's flags).
    let params = match command.get("params") {
        Some(p) if p.is_object() => p.clone(),
        _ => command.clone(),
    };
    with_doc(|doc| {
        let object = build_feature(doc, &kind, name.as_deref(), &params)?;
        let id = doc.add(object);
        recompute(doc)?;
        let summary = doc
            .summary()
            .into_iter()
            .find(|s| s.id == id)
            .ok_or("internal error: object vanished after add")?;
        Ok(json!({
            "name": summary.name,
            "type": summary.type_name,
            "status": summary.status.to_string(),
            "rows": rows(doc),
        }))
    })
}

fn op_remove(command: &Value) -> Result<Value, String> {
    let object = str_field(command, "object")?;
    with_doc(|doc| {
        doc.remove(&object)?;
        recompute(doc)?;
        Ok(json!({"rows": rows(doc)}))
    })
}

fn op_set(command: &Value) -> Result<Value, String> {
    let object = str_field(command, "object")?;
    let property = str_field(command, "property")?;
    let value = str_field(command, "value")?;
    with_doc(|doc| {
        let existing = {
            let obj = doc
                .get_by_name(&object)
                .ok_or_else(|| format!("object '{object}' not found"))?;
            obj.properties()
                .get(&property)
                .ok_or_else(|| format!("object '{object}' has no property '{property}'"))?
                .clone()
        };
        let parsed = Property::parse(existing.type_name(), &value).ok_or_else(|| {
            format!(
                "cannot parse '{value}' as {} for {object}.{property}",
                existing.type_name()
            )
        })?;
        doc.set_property(&object, &property, parsed)?;
        recompute(doc)?;
        // The object itself plus everything downstream (the CLI's rule).
        let mut touched = vec![object.clone()];
        touched.extend(doc.dependents_of(&object));
        Ok(json!({
            "rows": rows(doc),
            "touched": touched,
        }))
    })
}

fn op_get(command: &Value) -> Result<Value, String> {
    let object = str_field(command, "object")?;
    let property = str_field(command, "property")?;
    with_doc(|doc| {
        let obj = doc
            .get_by_name(&object)
            .ok_or_else(|| format!("object '{object}' not found"))?;
        let prop = obj
            .properties()
            .get(&property)
            .ok_or_else(|| format!("object '{object}' has no property '{property}'"))?;
        let mut data = json!({"value": prop.to_text()});
        if let Some(expression) = obj.expressions().get(&property) {
            data["expression"] = json!(expression);
        }
        Ok(data)
    })
}

fn op_set_expr(command: &Value) -> Result<Value, String> {
    let object = str_field(command, "object")?;
    let property = str_field(command, "property")?;
    let expression = str_field(command, "expression")?;
    with_doc(|doc| {
        doc.bind_expression(&object, &property, &expression)?;
        recompute(doc)?;
        let value = doc
            .get_by_name(&object)
            .and_then(|obj| obj.properties().get(&property))
            .map(|prop| prop.to_text())
            .unwrap_or_default();
        Ok(json!({"value": value, "rows": rows(doc)}))
    })
}

fn op_clear_expr(command: &Value) -> Result<Value, String> {
    let object = str_field(command, "object")?;
    let property = str_field(command, "property")?;
    with_doc(|doc| {
        doc.clear_expression(&object, &property)?;
        recompute(doc)?;
        Ok(json!({"rows": rows(doc)}))
    })
}

fn op_list() -> Result<Value, String> {
    with_doc(|doc| Ok(json!({"rows": rows(doc)})))
}

fn op_properties(command: &Value) -> Result<Value, String> {
    let object = str_field(command, "object")?;
    with_doc(|doc| {
        let obj = doc
            .get_by_name(&object)
            .ok_or_else(|| format!("object '{object}' not found"))?;
        let mut props = serde_json::Map::new();
        for (key, value) in obj.properties() {
            props.insert(key.clone(), json!(value.to_text()));
        }
        let mut expressions = serde_json::Map::new();
        for (key, expression) in obj.expressions() {
            expressions.insert(key.clone(), json!(expression));
        }
        Ok(json!({
            "name": obj.name(),
            "type": obj.type_name(),
            "status": obj.status().to_string(),
            "props": props,
            "expressions": expressions,
        }))
    })
}

fn op_recompute() -> Result<Value, String> {
    with_doc(|doc| {
        recompute(doc)?;
        Ok(json!({"rows": rows(doc)}))
    })
}

/// The placed mesh of one object (placement applied — the FreeCAD
/// rule; `Part::Transform` bakes its own).
fn placed_mesh(doc: &Document, object: &str, deflection: f64) -> Result<Mesh, String> {
    let obj = doc
        .get_by_name(object)
        .ok_or_else(|| format!("object '{object}' not found"))?;
    let mut mesh = mesh_of(obj, deflection)
        .ok_or_else(|| format!("object '{object}' has no computed geometry yet"))?;
    if obj.type_name() != "Part::Transform" {
        if let Some(Property::Placement(p)) = obj.properties().get("Placement") {
            mesh.transform(&p.to_matrix());
        }
    }
    Ok(mesh)
}

fn op_mesh(command: &Value) -> Result<Value, String> {
    let object = str_field(command, "object")?;
    let deflection = command
        .get("deflection")
        .and_then(Value::as_f64)
        .unwrap_or(0.2);
    with_doc(|doc| {
        recompute(doc)?;
        let mesh = placed_mesh(doc, &object, deflection)?;
        let mut positions: Vec<f64> = Vec::with_capacity(mesh.vertices.len() * 3);
        for v in &mesh.vertices {
            positions.extend_from_slice(&[v.x, v.y, v.z]);
        }
        let indices: Vec<u32> = mesh
            .triangles
            .iter()
            .flat_map(|t| t.iter().copied())
            .collect();
        Ok(json!({
            "name": object,
            "vertices": mesh.vertices.len(),
            "triangles": mesh.triangle_count(),
            "positions": positions,
            "indices": indices,
        }))
    })
}

fn op_stats(command: &Value) -> Result<Value, String> {
    let object = command.get("object").and_then(Value::as_str);
    with_doc(|doc| {
        recompute(doc)?;
        let mut stats = Vec::new();
        let mut total = 0.0;
        for s in doc.summary() {
            if let Some(want) = object {
                if s.name != want {
                    continue;
                }
            }
            let Some(obj) = doc.get_by_name(&s.name) else {
                continue;
            };
            let mesh = mesh_of(obj, 0.2);
            if let Some(shape) = shape_of(obj) {
                let triangles = mesh.as_ref().map(|m| m.triangle_count()).unwrap_or(0);
                let volume = shape.volume();
                total += volume;
                stats.push(json!({
                    "name": s.name,
                    "kind": shape.kind_name(),
                    "volume": volume,
                    "area": shape.surface_area(),
                    "triangles": triangles,
                }));
            } else if let Some(mesh) = mesh {
                let triangles = mesh.triangle_count();
                let shape = TopoShape::Mesh(mesh);
                let volume = shape.volume();
                total += volume;
                stats.push(json!({
                    "name": s.name,
                    "kind": shape.kind_name(),
                    "volume": volume,
                    "area": shape.surface_area(),
                    "triangles": triangles,
                }));
            } else {
                stats.push(json!({"name": s.name, "kind": null}));
            }
        }
        if stats.is_empty() {
            // An empty document is a legal state: the whole-document
            // query reports zero objects and zero volume — it is not
            // an error (the studio refreshes into one right after
            // `new`, and after removing or undoing back to nothing).
            // Only a *named* object that cannot be found is an error.
            if let Some(want) = object {
                return Err(format!("object '{want}' not found or has no shape"));
            }
            return Ok(json!({"objects": [], "total_volume": 0.0}));
        }
        Ok(json!({"objects": stats, "total_volume": total}))
    })
}

fn op_export_stl(command: &Value) -> Result<Value, String> {
    let object = str_field(command, "object")?;
    let binary = command
        .get("binary")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    with_doc(|doc| {
        recompute(doc)?;
        let mesh = placed_mesh(doc, &object, 0.2)?;
        let mut bytes: Vec<u8> = Vec::new();
        if binary {
            fc_io::write_stl_binary("freecad-rs", &mesh.vertices, &mesh.triangles, &mut bytes)
                .map_err(|e| format!("stl export failed: {e}"))?;
        } else {
            fc_io::write_stl_ascii("freecad-rs", &mesh.vertices, &mesh.triangles, &mut bytes)
                .map_err(|e| format!("stl export failed: {e}"))?;
        }
        let size = bytes.len();
        Ok(json!({"stl_base64": b64_encode(&bytes), "size": size}))
    })
}

fn op_export_obj(command: &Value) -> Result<Value, String> {
    let object = str_field(command, "object")?;
    with_doc(|doc| {
        recompute(doc)?;
        let mesh = placed_mesh(doc, &object, 0.2)?;
        let mut bytes: Vec<u8> = Vec::new();
        fc_io::write_obj(&mesh.vertices, &mesh.triangles, &mut bytes)
            .map_err(|e| format!("obj export failed: {e}"))?;
        let size = bytes.len();
        Ok(json!({"obj_text": String::from_utf8_lossy(&bytes), "size": size}))
    })
}

fn op_undo() -> Result<Value, String> {
    with_doc(|doc| {
        let applied = doc.undo()?.is_some();
        recompute(doc)?;
        Ok(json!({"applied": applied, "rows": rows(doc),
                  "undo_depth": doc.undo_count(), "redo_depth": doc.redo_count()}))
    })
}

fn op_redo() -> Result<Value, String> {
    with_doc(|doc| {
        let applied = doc.redo()?.is_some();
        recompute(doc)?;
        Ok(json!({"applied": applied, "rows": rows(doc),
                  "undo_depth": doc.undo_count(), "redo_depth": doc.redo_count()}))
    })
}

fn op_history() -> Result<Value, String> {
    with_doc(|doc| {
        let undo: Vec<String> = doc
            .undo_history()
            .into_iter()
            .map(|e| {
                e.label
                    .unwrap_or_else(|| format!("transaction #{}", e.id.0))
            })
            .collect();
        let redo: Vec<String> = doc
            .redo_history()
            .into_iter()
            .map(|e| {
                e.label
                    .unwrap_or_else(|| format!("transaction #{}", e.id.0))
            })
            .collect();
        Ok(json!({"undo": undo, "redo": redo,
                  "undo_depth": doc.undo_count(), "redo_depth": doc.redo_count()}))
    })
}

fn str_field(value: &Value, key: &str) -> Result<String, String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(String::from)
        .ok_or_else(|| format!("field '{key}' expects a string"))
}

// ---------------------------------------------------------------------------
// Base64 (standard alphabet, with padding) — no dependency needed
// ---------------------------------------------------------------------------

const B64_ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn b64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(B64_ALPHABET[(n >> 18) as usize & 63] as char);
        out.push(B64_ALPHABET[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            B64_ALPHABET[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            B64_ALPHABET[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

fn b64_decode(text: &str) -> Result<Vec<u8>, String> {
    let mut values = Vec::with_capacity(text.len());
    for byte in text.bytes() {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => continue,
            b'\n' | b'\r' | b' ' => continue,
            _ => return Err(format!("invalid base64 byte 0x{byte:02x}")),
        };
        values.push(value);
    }
    let mut out = Vec::with_capacity(values.len() * 3 / 4);
    for chunk in values.chunks(4) {
        let mut n: u32 = 0;
        for (i, v) in chunk.iter().enumerate() {
            n |= u32::from(*v) << (18 - 6 * i);
        }
        out.push((n >> 16) as u8);
        if chunk.len() > 2 {
            out.push((n >> 8) as u8);
        }
        if chunk.len() > 3 {
            out.push(n as u8);
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// The C ABI (the browser boundary)
// ---------------------------------------------------------------------------

/// Allocate `len` bytes for a command buffer. The host writes the
/// command JSON here, calls [`fc_command`], then frees the buffer with
/// [`fc_free`].
#[no_mangle]
pub extern "C" fn fc_alloc(len: usize) -> *mut u8 {
    let mut buffer = Vec::with_capacity(len);
    let ptr = buffer.as_mut_ptr();
    std::mem::forget(buffer);
    ptr
}

/// Free a buffer from [`fc_alloc`] or a command result from
/// [`fc_command`] (`capacity == len` in both — the allocation contract).
///
/// # Safety
/// `ptr`/`len` must describe a live allocation made by this module.
#[no_mangle]
pub unsafe extern "C" fn fc_free(ptr: *mut u8, len: usize) {
    if !ptr.is_null() && len > 0 {
        drop(Vec::from_raw_parts(ptr, len, len));
    }
}

/// Set the wall clock (Unix seconds) — the wasm32 target has no
/// `SystemTime`; the embedding calls this before creating documents
/// (and may call it periodically). Native targets ignore it.
#[no_mangle]
pub extern "C" fn fc_set_time(unix_secs: i64) {
    fc_app::set_embedded_wallclock_secs(unix_secs);
}

/// Run one JSON command. The return packs `(result_ptr << 32) |
/// result_len`; the host reads the response bytes from linear memory,
/// then frees them with [`fc_free`].
///
/// # Safety
/// `ptr`/`len` must describe a valid, UTF-8 command buffer.
#[no_mangle]
pub unsafe extern "C" fn fc_command(ptr: *const u8, len: usize) -> u64 {
    let result = std::panic::catch_unwind(|| {
        let bytes = std::slice::from_raw_parts(ptr, len);
        let command: Value = match serde_json::from_slice(bytes) {
            Ok(command) => command,
            Err(error) => {
                return json!({"ok": false, "error": format!("bad command JSON: {error}")})
            }
        };
        run_command(&command)
    })
    .unwrap_or_else(|_| json!({"ok": false, "error": "engine panic"}));
    let out = serde_json::to_vec(&result).unwrap_or_else(|_| b"{}".to_vec());
    let out_len = out.len();
    let raw = Box::into_raw(out.into_boxed_slice());
    let out_ptr = raw as *mut u8 as u64;
    (out_ptr << 32) | (out_len as u64)
}

// ---------------------------------------------------------------------------
// Tests — the whole command surface, natively (and in CI)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(command: Value) -> Value {
        let response = run_command(&command);
        assert!(
            response["ok"].as_bool().unwrap(),
            "command failed: {response}"
        );
        response["data"].clone()
    }

    fn err(command: Value) -> String {
        let response = run_command(&command);
        assert!(
            !response["ok"].as_bool().unwrap(),
            "command unexpectedly ok: {response}"
        );
        response["error"].as_str().unwrap().to_string()
    }

    fn fresh_doc() {
        ok(json!({"op": "new", "label": "test"}));
    }

    #[test]
    fn version_and_new() {
        let data = ok(json!({"op": "version"}));
        assert_eq!(data["engine"], "freecad-rs");
        fresh_doc();
        let data = ok(json!({"op": "list"}));
        assert_eq!(data["rows"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn command_requires_document() {
        let error = err(json!({"op": "list"}));
        assert!(error.contains("no document"), "{error}");
    }

    #[test]
    fn box_set_stats_workflow() {
        fresh_doc();
        let data = ok(json!({"op": "add", "kind": "box",
                             "params": {"length": 10, "width": 8, "height": 6}}));
        assert_eq!(data["type"], "Part::Box");
        assert_eq!(data["name"], "Box");
        let data = ok(json!({"op": "set", "object": "Box", "property": "Length", "value": "20"}));
        assert!(data["touched"].as_array().unwrap().contains(&json!("Box")));
        let data = ok(json!({"op": "stats", "object": "Box"}));
        assert_eq!(data["objects"][0]["kind"], "Box");
        assert!((data["objects"][0]["volume"].as_f64().unwrap() - 960.0).abs() < 1e-9);
    }

    #[test]
    fn stats_on_an_empty_document_is_zero_not_an_error() {
        // The studio refreshes into an empty document right after
        // `new` (and after removing or undoing back to nothing) — the
        // whole-document query must report zero, not fail.
        fresh_doc();
        let data = ok(json!({"op": "stats"}));
        assert_eq!(data["objects"].as_array().unwrap().len(), 0);
        assert_eq!(data["total_volume"].as_f64().unwrap(), 0.0);
        // A named object that does not exist is still an honest error.
        let error = err(json!({"op": "stats", "object": "Ghost"}));
        assert!(error.contains("Ghost"), "{error}");
    }

    #[test]
    fn boolean_and_mesh_workflow() {
        fresh_doc();
        ok(
            json!({"op": "add", "kind": "box", "params": {"length": 10, "width": 10, "height": 10}}),
        );
        ok(json!({"op": "add", "kind": "box", "name": "Tool",
                  "params": {"length": 5, "width": 5, "height": 10}}));
        let data =
            ok(json!({"op": "add", "kind": "cut", "params": {"base": "Box", "tool": "Tool"}}));
        assert_eq!(data["name"], "Cut");
        let data = ok(json!({"op": "stats", "object": "Cut"}));
        assert!((data["objects"][0]["volume"].as_f64().unwrap() - 750.0).abs() < 1e-9);
        let data = ok(json!({"op": "mesh", "object": "Cut"}));
        assert!(data["triangles"].as_u64().unwrap() > 0);
        assert_eq!(data["positions"].as_array().unwrap().len() % 3, 0);
    }

    #[test]
    fn sketch_pad_revolution_workflow() {
        fresh_doc();
        let data = ok(json!({"op": "add", "kind": "sketch", "name": "Profile",
        "params": {"geometry": [
            {"kind": "rectangle", "x": -10, "y": -6, "w": 20, "h": 12},
            {"kind": "circle", "cx": 0, "cy": 0, "r": 3},
        ]}}));
        assert_eq!(data["type"], "Sketcher::SketchObject");
        let data =
            ok(json!({"op": "add", "kind": "pad", "params": {"profile": "Profile", "length": 6}}));
        assert_eq!(data["type"], "PartDesign::Pad");
        let data = ok(json!({"op": "stats", "object": "Pad"}));
        // 20x12x6 box with a r=3 hole punched through: 1440 - pi*9*6
        let expect = 1440.0 - std::f64::consts::PI * 9.0 * 6.0;
        assert!(
            (data["objects"][0]["volume"].as_f64().unwrap() - expect).abs() < 1.0,
            "pad volume {} vs expected {expect}",
            data["objects"][0]["volume"]
        );
        let error = err(json!({"op": "add", "kind": "pad"}));
        assert!(error.contains("profile"), "{error}");
    }

    #[test]
    fn expressions_workflow() {
        fresh_doc();
        ok(
            json!({"op": "add", "kind": "box", "params": {"length": 10, "width": 10, "height": 10}}),
        );
        let data = ok(
            json!({"op": "set_expr", "object": "Box", "property": "Height",
                             "expression": "Length * 2"}),
        );
        assert_eq!(data["value"], "20");
        let data = ok(json!({"op": "get", "object": "Box", "property": "Height"}));
        assert_eq!(data["value"], "20");
        assert_eq!(data["expression"], "Length * 2");
        ok(json!({"op": "clear_expr", "object": "Box", "property": "Height"}));
        let data = ok(json!({"op": "get", "object": "Box", "property": "Height"}));
        assert_eq!(data["value"], "20");
    }

    #[test]
    fn undo_redo_workflow() {
        fresh_doc();
        ok(json!({"op": "add", "kind": "sphere", "params": {"radius": 2}}));
        let data = ok(json!({"op": "undo"}));
        assert!(data["applied"].as_bool().unwrap());
        let data = ok(json!({"op": "list"}));
        assert_eq!(data["rows"].as_array().unwrap().len(), 0);
        let data = ok(json!({"op": "redo"}));
        assert!(data["applied"].as_bool().unwrap());
        let data = ok(json!({"op": "list"}));
        assert_eq!(data["rows"].as_array().unwrap().len(), 1);
        let data = ok(json!({"op": "history"}));
        assert!(!data["undo"].as_array().unwrap().is_empty());
    }

    #[test]
    fn save_load_round_trip() {
        fresh_doc();
        ok(json!({"op": "add", "kind": "cylinder", "params": {"radius": 3, "height": 7}}));
        let saved = ok(json!({"op": "save"}));
        let encoded = saved["fcstd_base64"].as_str().unwrap().to_string();
        assert!(saved["size"].as_u64().unwrap() > 200);
        ok(json!({"op": "new"}));
        let data = ok(json!({"op": "list"}));
        assert_eq!(data["rows"].as_array().unwrap().len(), 0);
        let data = ok(json!({"op": "load", "fcstd_base64": encoded}));
        assert_eq!(data["objects"], 1);
        let data = ok(json!({"op": "stats", "object": "Cylinder"}));
        let expect = std::f64::consts::PI * 9.0 * 7.0;
        assert!((data["objects"][0]["volume"].as_f64().unwrap() - expect).abs() < 0.2);
    }

    #[test]
    fn exports_work() {
        fresh_doc();
        ok(json!({"op": "add", "kind": "box", "params": {"length": 4, "width": 4, "height": 4}}));
        let data = ok(json!({"op": "export_stl", "object": "Box"}));
        assert!(data["stl_base64"].as_str().unwrap().len() > 100);
        assert!(data["size"].as_u64().unwrap() > 300);
        let data = ok(json!({"op": "export_obj", "object": "Box"}));
        assert!(data["obj_text"]
            .as_str()
            .unwrap()
            .starts_with("# freecad-rs export"));
    }

    #[test]
    fn properties_listing_and_base64() {
        fresh_doc();
        ok(json!({"op": "add", "kind": "box", "params": {"length": 1, "width": 1, "height": 1}}));
        let data = ok(json!({"op": "properties", "object": "Box"}));
        assert_eq!(data["props"]["Length"], "1");
        // base64 self-check
        let sample = b"freecad-rs round trip";
        assert_eq!(b64_decode(&b64_encode(sample)).unwrap(), sample);
        assert_eq!(b64_encode(b"fun"), "ZnVu");
    }

    #[test]
    fn unknown_ops_report_errors() {
        fresh_doc();
        let error = err(json!({"op": "warp"}));
        assert!(error.contains("unknown op"), "{error}");
        let error = err(json!({"op": "add", "kind": "holodeck"}));
        assert!(error.contains("unknown kind"), "{error}");
    }

    // -----------------------------------------------------------------
    // Feature patterns (the mechanical-workflow milestone)
    // -----------------------------------------------------------------

    /// The stats volume of `object`.
    fn volume_of(object: &str) -> f64 {
        ok(json!({"op": "stats", "object": object}))["objects"][0]["volume"]
            .as_f64()
            .unwrap()
    }

    /// Pad the given circle profile in a scratch document and read the
    /// volume — the engine's own measurement of the tessellated tool.
    fn circle_tool_volume(cx: f64, cy: f64, r: f64) -> f64 {
        ok(json!({"op": "new", "label": "tool"}));
        ok(json!({"op": "add", "kind": "sketch", "geometry": [
            {"kind": "circle", "cx": cx, "cy": cy, "r": r}]}));
        ok(json!({"op": "add", "kind": "pad", "params": {"profile": "Sketch", "length": 10}}));
        volume_of("Pad")
    }

    #[test]
    fn patterns_bracket_plate_end_to_end() {
        // Tool volumes, measured through the engine itself.
        let v_corner = circle_tool_volume(-50.0, 30.0, 3.0);
        let v_bolt = circle_tool_volume(40.0, 0.0, 2.0);

        // The bracket plate: centered 120x80 plate.
        ok(json!({"op": "new", "label": "Bracket"}));
        ok(json!({"op": "add", "kind": "sketch", "geometry": [
            {"kind": "rectangle", "x": -60, "y": -40, "w": 120, "h": 80}]}));
        ok(json!({"op": "add", "kind": "pad", "name": "Plate",
                  "params": {"profile": "Sketch", "length": 10}}));
        assert!((volume_of("Plate") - 96000.0).abs() < 1e-6);

        // One corner hole, pocketed through.
        ok(json!({"op": "add", "kind": "sketch", "geometry": [
            {"kind": "circle", "cx": -50, "cy": 30, "r": 3}]}));
        ok(json!({"op": "add", "kind": "pocket", "name": "Hole",
                  "params": {"profile": "Sketch001", "base": "Plate", "length": 10}}));
        assert!((volume_of("Hole") - (96000.0 - v_corner)).abs() < 1e-4);

        // The Y row (reversed: the copy lands at y = -30).
        ok(
            json!({"op": "add", "kind": "pattern-linear", "name": "HolesY",
                  "params": {"originals": "Hole", "base": "Plate", "direction": "y",
                             "length": 60, "occurrences": 2, "reversed": true}}),
        );
        let data = ok(json!({"op": "list"}));
        assert!(data["rows"].to_string().contains("LinearPattern"), "{data}");
        assert!((volume_of("HolesY") - (96000.0 - 2.0 * v_corner)).abs() < 1e-4);

        // The nested X grid: 2x2 from one pocket.
        ok(
            json!({"op": "add", "kind": "pattern-linear", "name": "HolesGrid",
                  "params": {"originals": "HolesY", "base": "Plate", "direction": "x",
                             "length": 100, "occurrences": 2}}),
        );
        assert!((volume_of("HolesGrid") - (96000.0 - 4.0 * v_corner)).abs() < 1e-4);

        // The bolt circle: one hole at (40, 0), patterned around Z.
        ok(json!({"op": "add", "kind": "sketch", "geometry": [
            {"kind": "circle", "cx": 40, "cy": 0, "r": 2}]}));
        ok(json!({"op": "add", "kind": "pocket", "name": "BoltHole",
                  "params": {"profile": "Sketch002", "base": "HolesGrid", "length": 10}}));
        ok(
            json!({"op": "add", "kind": "pattern-polar", "name": "BoltCircle",
                  "params": {"originals": "BoltHole", "base": "HolesGrid", "axis": "z",
                             "angle": 360, "occurrences": 6}}),
        );
        let data = ok(json!({"op": "list"}));
        assert!(data["rows"].to_string().contains("PolarPattern"), "{data}");
        assert!(
            (volume_of("BoltCircle") - (96000.0 - 4.0 * v_corner - 6.0 * v_bolt)).abs() < 1e-3,
            "bracket volume {} vs expected",
            volume_of("BoltCircle")
        );

        // The save/load roundtrip keeps the patterns and their
        // geometry.
        let data = ok(json!({"op": "save"}));
        let encoded = data["fcstd_base64"].as_str().unwrap().to_string();
        ok(json!({"op": "load", "fcstd_base64": encoded}));
        assert!(
            (volume_of("BoltCircle") - (96000.0 - 4.0 * v_corner - 6.0 * v_bolt)).abs() < 1e-3,
            "reloaded bracket volume"
        );
    }

    #[test]
    fn pattern_params_are_validated() {
        fresh_doc();
        ok(
            json!({"op": "add", "kind": "box", "params": {"length": 30, "width": 30, "height": 10}}),
        );
        ok(json!({"op": "add", "kind": "sketch", "geometry": [
            {"kind": "rectangle", "x": -2, "y": -2, "w": 4, "h": 4}]}));
        ok(json!({"op": "add", "kind": "pocket", "name": "Hole",
                  "params": {"profile": "Sketch", "base": "Box", "length": 10}}));

        // Occurrences below 2 is not a pattern.
        let error = err(json!({"op": "add", "kind": "pattern-linear",
            "params": {"originals": "Hole", "base": "Box", "direction": "x",
                       "length": 10, "occurrences": 1}}));
        assert!(error.contains("Occurrences must be >= 2"), "{error}");

        // Unknown direction is named.
        let error = err(json!({"op": "add", "kind": "pattern-linear",
            "params": {"originals": "Hole", "base": "Box", "direction": "w",
                       "length": 10, "occurrences": 2}}));
        assert!(error.contains("direction"), "{error}");

        // Unknown originals is named.
        let error = err(json!({"op": "add", "kind": "pattern-polar",
            "params": {"originals": "Ghost", "base": "Box", "angle": 360,
                       "occurrences": 3}}));
        assert!(error.contains("Ghost"), "{error}");
    }
}
