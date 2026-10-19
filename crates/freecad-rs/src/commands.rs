//! Command implementations. Each mutating command loads, mutates, saves.
//! Exit codes: 0 ok, 1 runtime error, 2 usage (clap).

use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

use fc_app::{Document, DocumentObject, ObjectSummary, Property, RecomputeError};
use fc_base::{Placement, Rotation, Vector3};
use fc_io::{load_document, save_document};
use fc_part::features::{
    BoxFeature, ConeFeature, CylinderFeature, SphereFeature, TorusFeature, TransformFeature,
};
use fc_part::{mesh_of, shape_of, Mesh, TopoShape};

use crate::{AddShape, Command, ExportFormat};

pub fn dispatch(cmd: Command) -> Result<(), String> {
    match cmd {
        Command::New { file, label } => new(&file, &label),
        Command::Add { file, shape, name } => add(&file, shape, name.as_deref()),
        Command::Set {
            file,
            object,
            property,
            value,
        } => set(&file, &object, &property, &value),
        Command::Get {
            file,
            object,
            property,
        } => get(&file, &object, &property),
        Command::List { file, json } => list(&file, json),
        Command::Recompute { file, json } => recompute(&file, json),
        Command::Info { file } => info(&file),
        Command::Stats { file } => stats(&file),
        Command::Export { format } => match format {
            ExportFormat::Svg { file, output, view } => export(&file, &output, &view),
        },
        Command::Graph { file, output } => graph(&file, &output),
    }
}

/// Load-or-die helper: the file must exist, and load warnings (skipped
/// OCCT payloads, unknown properties, ...) are surfaced on stderr.
fn load(file: &str) -> Result<(Document, fc_io::LoadReport), String> {
    let path = Path::new(file);
    if !path.exists() {
        return Err(format!("file not found: '{file}'"));
    }
    let (doc, report) = load_document(path).map_err(|e| format!("cannot load '{file}': {e}"))?;
    for warning in &report.warnings {
        eprintln!("warning: {warning}");
    }
    Ok((doc, report))
}

fn save(file: &str, doc: &Document) -> Result<(), String> {
    save_document(doc, Path::new(file)).map_err(|e| format!("cannot save '{file}': {e}"))
}

// ---------------------------------------------------------------------------
// new
// ---------------------------------------------------------------------------

fn new(file: &str, label: &str) -> Result<(), String> {
    if Path::new(file).exists() {
        return Err(format!("refusing to overwrite existing file '{file}'"));
    }
    let mut doc = Document::new(label);
    doc.set_creator("freecad-rs");
    save(file, &doc)?;
    println!("created {file} — label \"{label}\" (0 objects)");
    Ok(())
}

// ---------------------------------------------------------------------------
// add
// ---------------------------------------------------------------------------

fn add(file: &str, shape: AddShape, name: Option<&str>) -> Result<(), String> {
    let (mut doc, _) = load(file)?;
    let object = build_feature(&doc, shape, name)?;
    let id = doc.add(object);
    let summary = doc
        .summary()
        .into_iter()
        .find(|s| s.id == id)
        .ok_or_else(|| "internal error: object vanished right after add".to_string())?;
    let final_name = summary.name.clone();
    let type_name = summary.type_name.clone();
    recompute_and_save(file, &mut doc)?;
    println!("added {final_name} ({type_name}) -> {file}");
    Ok(())
}

/// Build the feature a subcommand describes, with its parameters set
/// **before** the document sees it (so the name uniquifier and the
/// dependency graph observe a fully-formed object).
fn build_feature(
    doc: &Document,
    shape: AddShape,
    name: Option<&str>,
) -> Result<Box<dyn DocumentObject>, String> {
    Ok(match shape {
        AddShape::Box {
            length,
            width,
            height,
        } => {
            let mut f = BoxFeature::new(name.unwrap_or("Box"));
            f.container.set("Length", Property::Float(length));
            f.container.set("Width", Property::Float(width));
            f.container.set("Height", Property::Float(height));
            Box::new(f)
        }
        AddShape::Sphere { radius } => {
            let mut f = SphereFeature::new(name.unwrap_or("Sphere"));
            f.container.set("Radius", Property::Float(radius));
            Box::new(f)
        }
        AddShape::Cylinder { radius, height } => {
            let mut f = CylinderFeature::new(name.unwrap_or("Cylinder"));
            f.container.set("Radius", Property::Float(radius));
            f.container.set("Height", Property::Float(height));
            Box::new(f)
        }
        AddShape::Cone {
            radius1,
            radius2,
            height,
        } => {
            let mut f = ConeFeature::new(name.unwrap_or("Cone"));
            f.container.set("Radius1", Property::Float(radius1));
            f.container.set("Radius2", Property::Float(radius2));
            f.container.set("Height", Property::Float(height));
            Box::new(f)
        }
        AddShape::Torus { radius1, radius2 } => {
            let mut f = TorusFeature::new(name.unwrap_or("Torus"));
            f.container.set("Radius1", Property::Float(radius1));
            f.container.set("Radius2", Property::Float(radius2));
            Box::new(f)
        }
        AddShape::Transform {
            source,
            translate,
            rotate_axis,
            rotate_deg,
            scale,
        } => {
            if doc.get_by_name(&source).is_none() {
                return Err(format!(
                    "--source object '{source}' not found in the document; available: {}",
                    names_list(doc)
                ));
            }
            if translate.len() != 3 {
                return Err(format!(
                    "--translate expects three values 'x,y,z', got {}",
                    translate.len()
                ));
            }
            if rotate_axis.len() != 3 {
                return Err(format!(
                    "--rotate-axis expects three values 'x,y,z', got {}",
                    rotate_axis.len()
                ));
            }
            let rotation = Rotation::from_axis_angle(
                Vector3::new(rotate_axis[0], rotate_axis[1], rotate_axis[2]),
                rotate_deg.to_radians(),
            )
            .map_err(|_| "--rotate-axis must be a non-zero vector 'x,y,z'".to_string())?;
            let mut f = TransformFeature::new(name.unwrap_or("Transform"));
            f.container.set("Source", Property::Link(source.clone()));
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
    })
}

// ---------------------------------------------------------------------------
// set / get
// ---------------------------------------------------------------------------

fn set(file: &str, object: &str, property: &str, value: &str) -> Result<(), String> {
    let (mut doc, _) = load(file)?;
    let existing = {
        let obj = doc.get_by_name(object).ok_or_else(|| {
            format!(
                "object '{object}' not found in '{file}'; available: {}",
                names_list(&doc)
            )
        })?;
        obj.properties()
            .get(property)
            .ok_or_else(|| {
                format!(
                    "object '{object}' has no property '{property}'; properties: {}",
                    props_list(obj)
                )
            })?
            .clone()
    };
    let parsed = Property::parse(existing.type_name(), value).ok_or_else(|| {
        format!(
            "cannot parse '{value}' as {} for {object}.{property} (expected: {})",
            existing.type_name(),
            expected_form(existing.type_name())
        )
    })?;
    doc.set_property(object, property, parsed)?;

    // Report the parametric cascade: the object itself plus everything
    // downstream of it, in creation order.
    let order: Vec<String> = doc.summary().into_iter().map(|s| s.name).collect();
    let mut touched: Vec<String> = vec![object.to_string()];
    touched.extend(doc.dependents_of(object));
    touched.sort_by_key(|n| order.iter().position(|o| o == n).unwrap_or(usize::MAX));
    touched.dedup();
    for name in &touched {
        println!("  touched {name}");
    }

    recompute_and_save(file, &mut doc)?;
    println!("set {object}.{property} = {value}");
    Ok(())
}

fn get(file: &str, object: &str, property: &str) -> Result<(), String> {
    let (doc, _) = load(file)?;
    let obj = doc.get_by_name(object).ok_or_else(|| {
        format!(
            "object '{object}' not found in '{file}'; available: {}",
            names_list(&doc)
        )
    })?;
    let prop = obj.properties().get(property).ok_or_else(|| {
        format!(
            "object '{object}' has no property '{property}'; properties: {}",
            props_list(obj)
        )
    })?;
    println!("{}", prop.to_text());
    Ok(())
}

// ---------------------------------------------------------------------------
// list / recompute
// ---------------------------------------------------------------------------

fn list(file: &str, json: bool) -> Result<(), String> {
    let (doc, _) = load(file)?;
    let rows = doc.summary();
    if json {
        println!("{}", summaries_json(rows.iter()));
        return Ok(());
    }
    let name_w = rows
        .iter()
        .map(|r| r.name.len())
        .max()
        .unwrap_or(0)
        .max("NAME".len());
    let type_w = rows
        .iter()
        .map(|r| r.type_name.len())
        .max()
        .unwrap_or(0)
        .max("TYPE".len());
    println!("{:<name_w$} {:<type_w$} STATUS", "NAME", "TYPE");
    for r in &rows {
        println!("{:<name_w$} {:<type_w$} {}", r.name, r.type_name, r.status);
    }
    Ok(())
}

fn recompute(file: &str, json: bool) -> Result<(), String> {
    let (mut doc, _) = load(file)?;
    let report = doc.recompute_all();
    let summaries = doc.summary();
    let total = doc.object_count();
    match report {
        Ok(ids) => {
            let recomputed: Vec<&ObjectSummary> = ids
                .iter()
                .filter_map(|id| summaries.iter().find(|s| s.id == *id))
                .collect();
            if json {
                println!("{}", summaries_json(recomputed.iter().copied()));
            } else {
                for s in &recomputed {
                    println!("  recomputed {} ({})", s.name, s.type_name);
                }
            }
            println!("{} recomputed, {} total", recomputed.len(), total);
            save(file, &doc)
        }
        Err(err) => Err(save_after_failure(file, &doc, &err)),
    }
}

/// Recompute, report, save — the shared tail of `add` and `set`.
///
/// On recompute failure the document is still saved (with the failing
/// object marked `Error`, FreeCAD-style) and the error is returned, so the
/// process exits 1 having lost no work.
fn recompute_and_save(file: &str, doc: &mut Document) -> Result<(), String> {
    let report = doc.recompute_all();
    let summaries = doc.summary();
    match report {
        Ok(ids) => {
            for id in ids {
                if let Some(s) = summaries.iter().find(|s| s.id == id) {
                    println!("  recomputed {} ({})", s.name, s.type_name);
                }
            }
            save(file, doc)
        }
        Err(err) => Err(save_after_failure(file, doc, &err)),
    }
}

/// Best-effort save after a failed recompute; combines the errors when
/// even the save fails.
fn save_after_failure(file: &str, doc: &Document, err: &RecomputeError) -> String {
    match save(file, doc) {
        Ok(()) => err.to_string(),
        Err(save_err) => {
            format!("recompute failed: {err}; saving the document also failed: {save_err}")
        }
    }
}

// ---------------------------------------------------------------------------
// info / stats
// ---------------------------------------------------------------------------

fn info(file: &str) -> Result<(), String> {
    let (doc, _) = load(file)?;
    println!("label:   {}", doc.label());
    println!("creator: {}", doc.creator());
    println!("created: {}", doc.created());
    println!("objects: {}", doc.object_count());
    let rows = doc.summary();
    let mut histogram: BTreeMap<&str, usize> = BTreeMap::new();
    for s in &rows {
        *histogram.entry(s.type_name.as_str()).or_insert(0) += 1;
    }
    for (type_name, count) in &histogram {
        println!("  {type_name} x {count}");
    }
    Ok(())
}

fn stats(file: &str) -> Result<(), String> {
    let (doc, _) = load(file)?;
    let mut total_volume = 0.0;
    for s in doc.summary() {
        let Some(obj) = doc.get(s.id) else { continue };
        let mesh = mesh_of(obj, 0.2);
        if let Some(shape) = shape_of(obj) {
            let volume = shape.volume();
            let area = shape.surface_area();
            let triangles = mesh.as_ref().map(|m| m.triangle_count()).unwrap_or(0);
            println!(
                "  {} ({}): volume {volume:.3} mm³, area {area:.3} mm², {triangles} triangles",
                s.name,
                shape.kind_name()
            );
            total_volume += volume;
        } else if let Some(mesh) = mesh {
            // Features whose computed geometry is a mesh (Part::Transform).
            let triangles = mesh.triangle_count();
            let shape = TopoShape::Mesh(mesh);
            let volume = shape.volume();
            println!(
                "  {} ({}): volume {volume:.3} mm³, area {:.3} mm², {triangles} triangles",
                s.name,
                shape.kind_name(),
                shape.surface_area()
            );
            total_volume += volume;
        } else {
            println!("  {}: no shape yet", s.name);
        }
    }
    println!(
        "total: {} objects, volume {:.3} mm³",
        doc.object_count(),
        total_volume
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// export svg / graph
// ---------------------------------------------------------------------------

fn export(file: &str, output: &str, view: &str) -> Result<(), String> {
    let (mut doc, _) = load(file)?;
    let view = crate::svg::View::parse(view)?;
    // The model must be current and consistent before we render it; no
    // output file is written when it is not.
    doc.recompute_all()
        .map_err(|e| format!("recompute failed: {e}"))?;

    let mut meshes: Vec<(String, Mesh)> = Vec::new();
    let mut triangles = 0usize;
    for s in doc.summary() {
        let Some(obj) = doc.get(s.id) else { continue };
        let Some(mut mesh) = mesh_of(obj, 0.2) else {
            continue;
        };
        // Primitives keep their shape at the origin: their `Placement`
        // property is applied here, at render time. Part::Transform
        // features already bake their Placement (and Scale) into the
        // computed mesh — re-applying would double the transform.
        if obj.type_name() != "Part::Transform" {
            if let Some(Property::Placement(p)) = obj.properties().get("Placement") {
                mesh.transform(&p.to_matrix());
            }
        }
        triangles += mesh.triangle_count();
        meshes.push((s.name.clone(), mesh));
    }

    let out_file =
        std::fs::File::create(output).map_err(|e| format!("cannot write '{output}': {e}"))?;
    let mut out = std::io::BufWriter::new(out_file);
    crate::svg::render_svg(&meshes, view, &mut out)
        .map_err(|e| format!("cannot write '{output}': {e}"))?;
    out.flush()
        .map_err(|e| format!("cannot write '{output}': {e}"))?;
    println!(
        "wrote {output} (view={}, {} objects, {triangles} triangles)",
        view.label(),
        meshes.len()
    );
    Ok(())
}

fn graph(file: &str, output: &str) -> Result<(), String> {
    let (doc, _) = load(file)?;
    let dot = doc.dependencies_dot();
    let edges = dot.lines().filter(|l| l.contains("->")).count();
    std::fs::write(output, &dot).map_err(|e| format!("cannot write '{output}': {e}"))?;
    println!(
        "wrote {output} ({} objects, {edges} edges)",
        doc.object_count()
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

/// Comma-separated object names for error messages (or "document is
/// empty").
fn names_list(doc: &Document) -> String {
    let names: Vec<String> = doc.summary().into_iter().map(|s| s.name).collect();
    if names.is_empty() {
        "document is empty".to_string()
    } else {
        names.join(", ")
    }
}

/// An object's properties as `Name (Type), ...` for error messages.
fn props_list(obj: &dyn DocumentObject) -> String {
    if obj.properties().is_empty() {
        "none".to_string()
    } else {
        obj.properties()
            .iter()
            .map(|(k, v)| format!("{k} ({})", v.type_name()))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// Human description of a property type's CLI text form, mirroring
/// `fc_app::Property::to_text`.
fn expected_form(type_name: &str) -> &'static str {
    match type_name {
        "App::PropertyBool" => "true or false",
        "App::PropertyFloat" => "a number, e.g. 2.5",
        "App::PropertyInteger" => "an integer, e.g. 7",
        "App::PropertyString" => "any text",
        "App::PropertyVector" => "x,y,z — e.g. 1,2,3",
        "App::PropertyPlacement" => {
            "pos=(x,y,z);axis=(x,y,z);angle=deg — e.g. pos=(0,0,12);axis=(0,0,1);angle=25"
        }
        "App::PropertyLink" => "an object name",
        "App::PropertyLinkList" => "object names, comma-separated",
        _ => "a value in this type's text form (see docs/cli.md)",
    }
}

/// Escape a string for inclusion in a JSON string literal. We hand-roll
/// the tiny amount of JSON we emit; there is no serde dependency.
fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// `[{"name":...,"type":...,"status":...}, ...]` for the given summaries.
fn summaries_json<'a>(items: impl IntoIterator<Item = &'a ObjectSummary>) -> String {
    let mut out = String::from("[");
    for (i, s) in items.into_iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&format!(
            "{{\"name\":\"{}\",\"type\":\"{}\",\"status\":\"{}\"}}",
            json_escape(&s.name),
            json_escape(&s.type_name),
            json_escape(&s.status.to_string())
        ));
    }
    out.push(']');
    out
}
