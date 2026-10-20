//! Command implementations. Each mutating command loads, mutates, saves.
//! Exit codes: 0 ok, 1 runtime error, 2 usage (clap).

use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

use fc_app::{
    Document, DocumentObject, JournalChange, ObjectSummary, Property, RecomputeError,
    UNDO_STACK_CAP,
};
use fc_base::{Placement, Rotation, Vector3};
use fc_io::mesh_export::{write_obj, write_stl_ascii, write_stl_binary};
use fc_io::{load_document, save_document};
use fc_part::features::{
    BooleanFeature, BoxFeature, ConeFeature, CylinderFeature, SphereFeature, TorusFeature,
    TransformFeature,
};
use fc_part::{mesh_of, shape_of, BooleanKind, Mesh, TopoShape};

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
        Command::SetExpr {
            file,
            object,
            property,
            expression,
        } => set_expr(&file, &object, &property, &expression),
        Command::ClearExpr {
            file,
            object,
            property,
        } => clear_expr(&file, &object, &property),
        Command::Get {
            file,
            object,
            property,
        } => get(&file, &object, &property),
        Command::List { file, json } => list(&file, json),
        Command::Recompute { file, json } => recompute(&file, json),
        Command::Undo { file, steps } => undo(&file, steps),
        Command::Redo { file, steps } => redo(&file, steps),
        Command::History { file } => history(&file),
        Command::Info { file } => info(&file),
        Command::Stats { file } => stats(&file),
        Command::Export { format } => match format {
            ExportFormat::Svg { file, output, view } => export(&file, &output, &view),
            ExportFormat::Obj { file, output } => export_obj(&file, &output),
            ExportFormat::Stl {
                file,
                output,
                binary,
            } => export_stl(&file, &output, binary),
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
    let (mut doc, report) =
        load_document(path).map_err(|e| format!("cannot load '{file}': {e}"))?;
    for warning in &report.warnings {
        eprintln!("warning: {warning}");
    }
    // The loader inserts objects through the regular `Document` API,
    // which records them as transactions; a freshly loaded document
    // carries no undoable history — the first edit of the session is
    // step 1 (the `undo`/`redo` commands rehydrate the real stacks from
    // the session journal).
    doc.clear_history();
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
    // A brand-new document starts with a clean session journal — this
    // also clears any journal left over from a previous file of the
    // same name.
    Journal::empty().write(file)?;
    println!("created {file} — label \"{label}\" (0 objects)");
    Ok(())
}

// ---------------------------------------------------------------------------
// add
// ---------------------------------------------------------------------------

fn add(file: &str, shape: AddShape, name: Option<&str>) -> Result<(), String> {
    let (mut doc, _) = load(file)?;
    let mut journal = Journal::read(file)?;
    let object = build_feature(&doc, shape, name)?;
    let id = doc.add(object);
    let summary = doc
        .summary()
        .into_iter()
        .find(|s| s.id == id)
        .ok_or_else(|| "internal error: object vanished right after add".to_string())?;
    let final_name = summary.name.clone();
    let type_name = summary.type_name.clone();
    // Journal the added object's full state (type + properties +
    // expressions), so its addition can be undone AND redone across
    // processes: after an undo the object lives only in the journal.
    let added = JournalAdd::from_object(
        doc.get(id)
            .ok_or_else(|| "internal error: object vanished right after add".to_string())?,
    );
    let txn = JournalTxn {
        label: Some(format!("add {final_name}")),
        adds: vec![added],
        props: Vec::new(),
        labels: Vec::new(),
    };
    recompute_save_journal(file, &mut doc, &mut journal, txn)?;
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
        AddShape::Cut { base, tool } => boolean_feature(doc, name, BooleanKind::Cut, &base, &tool)?,
        AddShape::Fuse { base, tool } => {
            boolean_feature(doc, name, BooleanKind::Fuse, &base, &tool)?
        }
        AddShape::Common { base, tool } => {
            boolean_feature(doc, name, BooleanKind::Common, &base, &tool)?
        }
    })
}

/// Build a `Part::Cut`/`Part::Fuse`/`Part::Common` feature after checking
/// both operands exist.
#[allow(clippy::too_many_arguments)]
fn boolean_feature(
    doc: &Document,
    name: Option<&str>,
    kind: BooleanKind,
    base: &str,
    tool: &str,
) -> Result<Box<dyn DocumentObject>, String> {
    for (label, operand) in [("base", base), ("tool", tool)] {
        if doc.get_by_name(operand).is_none() {
            return Err(format!(
                "--{label} object '{operand}' not found in the document; available: {}",
                names_list(doc)
            ));
        }
    }
    let mut f = BooleanFeature::new(
        name.unwrap_or(kind.type_name().trim_start_matches("Part::")),
        kind,
    );
    f.container.set("Base", Property::Link(base.to_string()));
    f.container.set("Tool", Property::Link(tool.to_string()));
    Ok(Box::new(f))
}

// ---------------------------------------------------------------------------
// set / get
// ---------------------------------------------------------------------------

fn set(file: &str, object: &str, property: &str, value: &str) -> Result<(), String> {
    let (mut doc, _) = load(file)?;
    let mut journal = Journal::read(file)?;
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
    doc.set_property(object, property, parsed.clone())?;

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

    let txn = JournalTxn {
        label: Some(format!("set {object}.{property}")),
        adds: Vec::new(),
        props: vec![JournalProp {
            object: object.to_string(),
            key: property.to_string(),
            before: Some(existing),
            after: Some(parsed),
        }],
        labels: Vec::new(),
    };
    recompute_save_journal(file, &mut doc, &mut journal, txn)?;
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
    if let Some(expression) = obj.expressions().get(property) {
        println!("  (bound to expression: {expression})");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// expressions
// ---------------------------------------------------------------------------

fn set_expr(file: &str, object: &str, property: &str, expression: &str) -> Result<(), String> {
    let (mut doc, _) = load(file)?;
    if doc.get_by_name(object).is_none() {
        return Err(format!(
            "object '{object}' not found in '{file}'; available: {}",
            names_list(&doc)
        ));
    }
    doc.bind_expression(object, property, expression)?;
    recompute_and_save(file, &mut doc)?;
    let value = doc
        .get_by_name(object)
        .and_then(|obj| obj.properties().get(property))
        .map(|prop| prop.to_text())
        .unwrap_or_default();
    println!("bound {object}.{property} = {expression}");
    println!("  now {object}.{property} = {value}");
    Ok(())
}

fn clear_expr(file: &str, object: &str, property: &str) -> Result<(), String> {
    let (mut doc, _) = load(file)?;
    doc.clear_expression(object, property)?;
    recompute_and_save(file, &mut doc)?;
    println!("cleared expression on {object}.{property}");
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

/// Recompute, report, save, journal — the shared tail of `add` and `set`.
/// The document is saved before the journal entry is written, so the
/// journal never claims a step that was not persisted; a failed
/// recompute still saves the document (FreeCAD-style, failing object
/// marked `Error`), journals the committed step, and only then reports
/// the error — the journal always matches the file on disk.
fn recompute_save_journal(
    file: &str,
    doc: &mut Document,
    journal: &mut Journal,
    txn: JournalTxn,
) -> Result<(), String> {
    let report = doc.recompute_all();
    let summaries = doc.summary();
    if let Ok(ids) = &report {
        for id in ids {
            if let Some(s) = summaries.iter().find(|s| s.id == *id) {
                println!("  recomputed {} ({})", s.name, s.type_name);
            }
        }
    }
    save(file, doc)?;
    journal.commit(file, txn)?;
    match report {
        Ok(_) => Ok(()),
        Err(err) => Err(err.to_string()),
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

/// Collect the document's placed meshes (shared by the OBJ/STL exports).
fn placed_meshes(file: &str, deflection: f64) -> Result<Vec<(String, Mesh)>, String> {
    let (mut doc, _) = load(file)?;
    // The model must be current and consistent before we export it; no
    // output file is written when it is not.
    doc.recompute_all()
        .map_err(|e| format!("recompute failed: {e}"))?;
    let mut meshes = Vec::new();
    for s in doc.summary() {
        let Some(obj) = doc.get(s.id) else { continue };
        let Some(mut mesh) = mesh_of(obj, deflection) else {
            continue;
        };
        // Primitives keep their shape at the origin: their `Placement`
        // property is applied here, at export time. Part::Transform and
        // the booleans already bake geometry into the computed mesh —
        // re-applying would double the transform.
        if !matches!(
            obj.type_name(),
            "Part::Transform" | "Part::Cut" | "Part::Fuse" | "Part::Common"
        ) {
            if let Some(Property::Placement(p)) = obj.properties().get("Placement") {
                mesh.transform(&p.to_matrix());
            }
        }
        meshes.push((s.name.clone(), mesh));
    }
    Ok(meshes)
}

fn export_obj(file: &str, output: &str) -> Result<(), String> {
    let meshes = placed_meshes(file, 0.2)?;
    let triangles: usize = meshes.iter().map(|(_, m)| m.triangle_count()).sum();
    let out_file =
        std::fs::File::create(output).map_err(|e| format!("cannot write '{output}': {e}"))?;
    let mut out = std::io::BufWriter::new(out_file);
    // One named object group per document object, all vertices shared
    // within a group.
    for (name, mesh) in &meshes {
        writeln!(out, "o {}", name.replace(char::is_whitespace, "_"))
            .map_err(|e| format!("cannot write '{output}': {e}"))?;
        write_obj(&mesh.vertices, &mesh.triangles, &mut out)
            .map_err(|e| format!("cannot write '{output}': {e}"))?;
    }
    out.flush()
        .map_err(|e| format!("cannot write '{output}': {e}"))?;
    println!(
        "wrote {output} (OBJ, {} objects, {triangles} triangles)",
        meshes.len()
    );
    Ok(())
}

fn export_stl(file: &str, output: &str, binary: bool) -> Result<(), String> {
    let meshes = placed_meshes(file, 0.2)?;
    if meshes.len() > 1 {
        // STL carries no object names; a multi-solid file still exports
        // (FreeCAD concatenates too), but the user should know.
        eprintln!(
            "note: {n} objects in '{file}' — STL has no names; all geometry is merged into one solid",
            n = meshes.len()
        );
    }
    // One solid: STL (binary especially) allows exactly one header +
    // triangle table, so the meshes are merged (index-offset).
    let mut vertices: Vec<Vector3> = Vec::new();
    let mut triangles: Vec<[u32; 3]> = Vec::new();
    for (_, mesh) in &meshes {
        let offset = vertices.len() as u32;
        vertices.extend(mesh.vertices.iter().cloned());
        triangles.extend(
            mesh.triangles
                .iter()
                .map(|t| [t[0] + offset, t[1] + offset, t[2] + offset]),
        );
    }
    let out_file =
        std::fs::File::create(output).map_err(|e| format!("cannot write '{output}': {e}"))?;
    let mut out = std::io::BufWriter::new(out_file);
    let result = if binary {
        write_stl_binary("freecad-rs", &vertices, &triangles, &mut out)
    } else {
        write_stl_ascii("freecad-rs", &vertices, &triangles, &mut out)
    };
    result.map_err(|e| format!("cannot write '{output}': {e}"))?;
    out.flush()
        .map_err(|e| format!("cannot write '{output}': {e}"))?;
    let flavor = if binary { "binary" } else { "ASCII" };
    println!(
        "wrote {output} ({flavor} STL, {triangles} triangles)",
        triangles = triangles.len()
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
// undo / redo / history — the session journal
// ---------------------------------------------------------------------------

/// Undo the last N committed steps (default 1) through the real
/// `fc_app` transaction machinery, then save the document and the
/// session journal.
fn undo(file: &str, steps: usize) -> Result<(), String> {
    apply_steps(file, steps, false)
}

/// Redo the last N undone steps (default 1) — symmetric to `undo`.
fn redo(file: &str, steps: usize) -> Result<(), String> {
    apply_steps(file, steps, true)
}

/// Shared body of `undo`/`redo`: restore the session stacks from the
/// journal, run the requested number of steps through the real
/// `fc_app` machinery, recompute, save the document, then persist the
/// rewritten journal — in that order, so the journal always matches the
/// saved document even when a recompute fails (the document is still
/// saved, FreeCAD-style, with the failing object marked `Error`; the
/// error is reported after everything is safely on disk).
fn apply_steps(file: &str, steps: usize, redoing: bool) -> Result<(), String> {
    if steps == 0 {
        println!("nothing to do: 0 steps requested");
        return Ok(());
    }
    let (mut doc, _) = load(file)?;
    let mut journal = Journal::read(file)?;
    restore_stacks(&mut doc, &journal)?;
    let mut done = 0usize;
    for _ in 0..steps {
        let step = if redoing { doc.redo()? } else { doc.undo()? };
        let Some(info) = step else {
            break;
        };
        let label = info.label.as_deref().unwrap_or("-");
        let verb = if redoing { "redid" } else { "undid" };
        println!(
            "{verb} \"{label}\" ({} change{})",
            info.changes,
            plural(info.changes)
        );
        done += 1;
    }
    if done == 0 {
        println!("nothing to {}", if redoing { "redo" } else { "undo" });
        return Ok(());
    }
    if redoing {
        journal.redo_depth = journal.redo_depth.saturating_sub(done);
    } else {
        journal.redo_depth += done;
    }
    let report = doc.recompute_all();
    let summaries = doc.summary();
    if let Ok(ids) = &report {
        for id in ids {
            if let Some(s) = summaries.iter().find(|s| s.id == *id) {
                println!("  recomputed {} ({})", s.name, s.type_name);
            }
        }
    }
    save(file, &doc)?;
    journal.write(file)?;
    if redoing {
        println!("{done} redone, {} to undo", doc.undo_count());
    } else {
        println!("{done} undone, {} to redo", doc.redo_count());
    }
    match report {
        Ok(_) => Ok(()),
        Err(err) => Err(err.to_string()),
    }
}

/// The undo-stack labels and the redo count — `fc_app`'s history,
/// rehydrated from the session journal and read through the real
/// machinery, which validates the journal against the document.
fn history(file: &str) -> Result<(), String> {
    let (mut doc, _) = load(file)?;
    let journal = Journal::read(file)?;
    restore_stacks(&mut doc, &journal)?;
    let entries = doc.undo_history();
    let redo_count = doc.redo_count();
    if entries.is_empty() && redo_count == 0 {
        println!("no history for {file}");
        return Ok(());
    }
    println!("undo stack ({})", entries.len());
    for (position, entry) in entries.iter().enumerate() {
        let label = entry.label.as_deref().unwrap_or("-");
        println!(
            "  {}. {} ({} change{})",
            position + 1,
            label,
            entry.changes,
            plural(entry.changes)
        );
    }
    println!("redo available: {redo_count}");
    Ok(())
}

/// Sidecar session journal of a document (`demo.fcstd` ->
/// `demo.fcstd.undolog`): the persisted record of committed
/// transactions that the `undo`/`redo`/`history` commands replay through
/// the real `fc_app` transaction machinery.
fn journal_path(file: &str) -> String {
    format!("{file}.undolog")
}

/// One persisted transaction of the session journal.
struct JournalTxn {
    label: Option<String>,
    adds: Vec<JournalAdd>,
    props: Vec<JournalProp>,
    /// Document-label changes — no current command produces one, but
    /// the format (and the restore path) understands them.
    labels: Vec<(String, String)>,
}

/// A persisted "object was added" change: enough data (type +
/// properties + expression bindings) to rebuild the object should its
/// addition be undone and then redone.
struct JournalAdd {
    name: String,
    type_name: String,
    props: Vec<(String, Property)>,
    exprs: Vec<(String, String)>,
}

/// A persisted property change: `before`/`after` snapshots (`None` = the
/// property did not exist).
struct JournalProp {
    object: String,
    key: String,
    before: Option<Property>,
    after: Option<Property>,
}

impl JournalAdd {
    /// Full state of an object that was just added.
    fn from_object(object: &dyn DocumentObject) -> Self {
        Self {
            name: object.name().to_string(),
            type_name: object.type_name().to_string(),
            props: object
                .properties()
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
            exprs: object
                .expressions()
                .iter()
                .map(|(key, text)| (key.clone(), text.clone()))
                .collect(),
        }
    }
}

/// The session journal: committed transactions in commit order, of
/// which the last `redo_depth` have been undone.
struct Journal {
    redo_depth: usize,
    txns: Vec<JournalTxn>,
}

impl Journal {
    /// An empty journal — nothing undoable, nothing to redo.
    fn empty() -> Self {
        Self {
            redo_depth: 0,
            txns: Vec::new(),
        }
    }

    /// Read the journal of `file`; a missing journal is an empty one,
    /// anything unparsable is an error.
    fn read(file: &str) -> Result<Self, String> {
        let path = journal_path(file);
        match std::fs::read_to_string(&path) {
            Ok(text) => Self::parse(&text),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Self::empty()),
            Err(err) => Err(format!("cannot read journal '{path}': {err}")),
        }
    }

    /// Record a freshly committed transaction and persist the journal.
    /// Mirrors `fc_app`: a new step clears the redo region, and the
    /// journal keeps at most `UNDO_STACK_CAP` transactions (oldest
    /// dropped first).
    fn commit(&mut self, file: &str, txn: JournalTxn) -> Result<(), String> {
        self.redo_depth = 0;
        self.txns.push(txn);
        if self.txns.len() > UNDO_STACK_CAP {
            self.txns.remove(0);
        }
        self.write(file)
    }

    /// Serialize: `V1` header, `D <redo-depth>`, then one transaction
    /// block per committed step. All free-text fields are escaped
    /// ([`esc`]) so every field is a single blank-free token.
    fn write(&self, file: &str) -> Result<(), String> {
        let path = journal_path(file);
        let mut text = String::from("V1\n");
        text.push_str(&format!("D {}\n", self.redo_depth));
        for txn in &self.txns {
            match &txn.label {
                Some(label) => text.push_str(&format!("T {}\n", esc(label))),
                None => text.push_str("T -\n"),
            }
            for add in &txn.adds {
                text.push_str(&format!("A {} {}\n", esc(&add.name), esc(&add.type_name)));
                for (key, value) in &add.props {
                    text.push_str(&format!(
                        "K {} {} {}\n",
                        esc(key),
                        esc(value.type_name()),
                        esc(&value.to_text())
                    ));
                }
                for (key, expr) in &add.exprs {
                    text.push_str(&format!("X {} {}\n", esc(key), esc(expr)));
                }
            }
            for prop in &txn.props {
                let snapshot = |value: &Option<Property>| match value {
                    Some(value) => format!("{} {}", esc(value.type_name()), esc(&value.to_text())),
                    None => "-".to_string(),
                };
                text.push_str(&format!(
                    "P {} {} {} {}\n",
                    esc(&prop.object),
                    esc(&prop.key),
                    snapshot(&prop.before),
                    snapshot(&prop.after),
                ));
            }
            for (before, after) in &txn.labels {
                text.push_str(&format!("L {} {}\n", esc(before), esc(after)));
            }
        }
        std::fs::write(&path, text).map_err(|err| format!("cannot write journal '{path}': {err}"))
    }

    /// Parse the `V1` journal format (the inverse of [`Journal::write`]).
    fn parse(text: &str) -> Result<Self, String> {
        let mut redo_depth = 0usize;
        let mut txns: Vec<JournalTxn> = Vec::new();
        let mut txn: Option<JournalTxn> = None;
        let mut add: Option<JournalAdd> = None;
        let mut lines = text.lines().enumerate();
        match lines.next() {
            Some((_, "V1")) => {}
            Some((index, other)) => {
                return Err(format!(
                    "journal line {}: expected header 'V1', found '{other}'",
                    index + 1
                ))
            }
            None => return Err("journal: empty file".to_string()),
        }
        for (index, line) in lines {
            let number = index + 1;
            if line.is_empty() {
                return Err(format!("journal line {number}: blank line"));
            }
            let mut fields = line.split(' ');
            let kind = fields.next().unwrap_or_default();
            let rest: Vec<&str> = fields.collect();
            // K/X continue the open add block; anything else closes it.
            if kind != "K" && kind != "X" {
                if let Some(add) = add.take() {
                    let Some(open) = txn.as_mut() else {
                        return Err(format!(
                            "journal line {number}: add change outside a transaction"
                        ));
                    };
                    open.adds.push(add);
                }
            }
            match kind {
                "D" => {
                    let Some(&depth) = rest.first() else {
                        return Err(format!("journal line {number}: 'D' expects a redo depth"));
                    };
                    redo_depth = depth
                        .parse()
                        .map_err(|_| format!("journal line {number}: bad redo depth '{depth}'"))?;
                }
                "T" => {
                    if let Some(open) = txn.take() {
                        txns.push(open);
                    }
                    let Some(&label) = rest.first() else {
                        return Err(format!("journal line {number}: 'T' expects a label"));
                    };
                    txn = Some(JournalTxn {
                        label: journal_label(label, number)?,
                        adds: Vec::new(),
                        props: Vec::new(),
                        labels: Vec::new(),
                    });
                }
                "A" => {
                    if add.is_some() {
                        return Err(format!("journal line {number}: 'A' inside an add block"));
                    }
                    if rest.len() != 2 {
                        return Err(format!(
                            "journal line {number}: 'A' expects a name and a type"
                        ));
                    }
                    add = Some(JournalAdd {
                        name: unesc(rest[0], number)?,
                        type_name: unesc(rest[1], number)?,
                        props: Vec::new(),
                        exprs: Vec::new(),
                    });
                }
                "K" => {
                    let Some(block) = add.as_mut() else {
                        return Err(format!("journal line {number}: 'K' outside an add block"));
                    };
                    if rest.len() != 3 {
                        return Err(format!(
                            "journal line {number}: 'K' expects a key, a type and a value"
                        ));
                    }
                    let key = unesc(rest[0], number)?;
                    let type_name = unesc(rest[1], number)?;
                    let value_text = unesc(rest[2], number)?;
                    let value = Property::parse(&type_name, &value_text).ok_or_else(|| {
                        format!("journal line {number}: cannot parse '{value_text}' as {type_name}")
                    })?;
                    block.props.push((key, value));
                }
                "X" => {
                    let Some(block) = add.as_mut() else {
                        return Err(format!("journal line {number}: 'X' outside an add block"));
                    };
                    if rest.len() != 2 {
                        return Err(format!(
                            "journal line {number}: 'X' expects a key and an expression"
                        ));
                    }
                    block
                        .exprs
                        .push((unesc(rest[0], number)?, unesc(rest[1], number)?));
                }
                "P" => {
                    let Some(open) = txn.as_mut() else {
                        return Err(format!(
                            "journal line {number}: property change outside a transaction"
                        ));
                    };
                    if rest.len() < 4 {
                        return Err(format!(
                            "journal line {number}: 'P' expects an object, a key, before and after"
                        ));
                    }
                    let object = unesc(rest[0], number)?;
                    let key = unesc(rest[1], number)?;
                    let mut index = 2;
                    let before = parse_snapshot(&rest, &mut index, number)?;
                    let after = parse_snapshot(&rest, &mut index, number)?;
                    if index != rest.len() {
                        return Err(format!("journal line {number}: trailing tokens on 'P'"));
                    }
                    open.props.push(JournalProp {
                        object,
                        key,
                        before,
                        after,
                    });
                }
                "L" => {
                    let Some(open) = txn.as_mut() else {
                        return Err(format!(
                            "journal line {number}: label change outside a transaction"
                        ));
                    };
                    if rest.len() != 2 {
                        return Err(format!(
                            "journal line {number}: 'L' expects before and after"
                        ));
                    }
                    open.labels
                        .push((unesc(rest[0], number)?, unesc(rest[1], number)?));
                }
                other => {
                    return Err(format!(
                        "journal line {number}: unknown line kind '{other}'"
                    ))
                }
            }
        }
        if let Some(add) = add.take() {
            let Some(open) = txn.as_mut() else {
                return Err("journal: add change outside a transaction".to_string());
            };
            open.adds.push(add);
        }
        if let Some(open) = txn.take() {
            txns.push(open);
        }
        let journal = Self { redo_depth, txns };
        if journal.redo_depth > journal.txns.len() {
            return Err(format!(
                "journal: redo depth {} exceeds {} transaction(s)",
                journal.redo_depth,
                journal.txns.len()
            ));
        }
        Ok(journal)
    }
}

/// Rebuild the in-memory transaction stacks from the journal, exactly as
/// they were at the end of the last session: the committed-not-undone
/// transactions oldest first, then the undone ones in commit order —
/// `restore_redo_transaction` re-queues them so `redo` replays
/// chronologically, and so an undone addition registers its (absent)
/// object before any later property change of that object resolves. The
/// document has just been loaded, so it already reflects every committed
/// — and undone — transaction; nothing is applied here.
fn restore_stacks(doc: &mut Document, journal: &Journal) -> Result<(), String> {
    let split = journal.txns.len().saturating_sub(journal.redo_depth);
    for txn in &journal.txns[..split] {
        doc.restore_undo_transaction(txn.label.as_deref(), journal_changes(txn, false)?)?;
    }
    for txn in &journal.txns[split..] {
        doc.restore_redo_transaction(txn.label.as_deref(), journal_changes(txn, true)?)?;
    }
    Ok(())
}

/// A journaled transaction's changes as `fc_app` records. When the
/// transaction sits on the **redo** stack its added objects were undone
/// — they are rebuilt from their journaled state so a redo can re-add
/// them; on the undo stack they are still in the document.
fn journal_changes(txn: &JournalTxn, redoing: bool) -> Result<Vec<JournalChange>, String> {
    let mut changes = Vec::new();
    for added in &txn.adds {
        changes.push(if redoing {
            JournalChange::AddedAbsent {
                object: rebuild_object(added)?,
            }
        } else {
            JournalChange::Added {
                object: added.name.clone(),
            }
        });
    }
    for prop in &txn.props {
        changes.push(JournalChange::Property {
            object: prop.object.clone(),
            key: prop.key.clone(),
            before: prop.before.clone(),
            after: prop.after.clone(),
        });
    }
    for (before, after) in &txn.labels {
        changes.push(JournalChange::Label {
            before: before.clone(),
            after: after.clone(),
        });
    }
    Ok(changes)
}

/// Rebuild a journaled object from its recorded state: the same feature
/// constructors as `add`, then its properties and expression bindings.
fn rebuild_object(added: &JournalAdd) -> Result<Box<dyn DocumentObject>, String> {
    let mut object = match added.type_name.as_str() {
        "Part::Box" => Box::new(BoxFeature::new(&added.name)) as Box<dyn DocumentObject>,
        "Part::Sphere" => Box::new(SphereFeature::new(&added.name)),
        "Part::Cylinder" => Box::new(CylinderFeature::new(&added.name)),
        "Part::Cone" => Box::new(ConeFeature::new(&added.name)),
        "Part::Torus" => Box::new(TorusFeature::new(&added.name)),
        "Part::Transform" => Box::new(TransformFeature::new(&added.name)),
        "Part::Cut" | "Part::Fuse" | "Part::Common" => {
            let kind = BooleanKind::from_type_name(&added.type_name)
                .ok_or_else(|| format!("journal: unknown boolean type '{}'", added.type_name))?;
            Box::new(BooleanFeature::new(&added.name, kind))
        }
        other => {
            return Err(format!(
                "journal: cannot rebuild object '{name}' of type '{other}'",
                name = added.name
            ))
        }
    };
    for (key, value) in &added.props {
        object.properties_mut().insert(key.clone(), value.clone());
    }
    if let Some(expressions) = object.expressions_mut() {
        for (key, text) in &added.exprs {
            expressions.insert(key.clone(), text.clone());
        }
    }
    Ok(object)
}

/// `'-'` = no label; anything else is an escaped label string.
fn journal_label(field: &str, number: usize) -> Result<Option<String>, String> {
    if field == "-" {
        Ok(None)
    } else {
        unesc(field, number).map(Some)
    }
}

/// One `P`-line snapshot field: `-` (absent) or an escaped
/// type + value pair, parsed back into a [`Property`].
fn parse_snapshot(
    rest: &[&str],
    index: &mut usize,
    number: usize,
) -> Result<Option<Property>, String> {
    let token = rest
        .get(*index)
        .copied()
        .ok_or_else(|| format!("journal line {number}: truncated property snapshot on 'P'"))?;
    *index += 1;
    if token == "-" {
        return Ok(None);
    }
    let type_name = unesc(token, number)?;
    let value_token = rest
        .get(*index)
        .copied()
        .ok_or_else(|| format!("journal line {number}: truncated property value on 'P'"))?;
    *index += 1;
    let value_text = unesc(value_token, number)?;
    Property::parse(&type_name, &value_text)
        .map(Some)
        .ok_or_else(|| format!("journal line {number}: cannot parse '{value_text}' as {type_name}"))
}

/// Escape a journal field: backslash, blank, tab, newline, carriage
/// return and a bare `-` (reserved for "no value") never appear
/// verbatim, so escaped fields are single blank-free tokens.
fn esc(field: &str) -> String {
    let mut out = String::with_capacity(field.len());
    for c in field.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            ' ' => out.push_str("\\s"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '-' => out.push_str("\\-"),
            _ => out.push(c),
        }
    }
    out
}

/// Inverse of [`esc`].
fn unesc(field: &str, number: usize) -> Result<String, String> {
    let mut out = String::with_capacity(field.len());
    let mut chars = field.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('\\') => out.push('\\'),
            Some('s') => out.push(' '),
            Some('t') => out.push('\t'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('-') => out.push('-'),
            Some(other) => {
                return Err(format!("journal line {number}: invalid escape '\\{other}'"))
            }
            None => return Err(format!("journal line {number}: trailing escape '\\'")),
        }
    }
    Ok(out)
}

/// `""` for 1, `"s"` otherwise — the plural of "change"/"step".
fn plural(count: usize) -> &'static str {
    if count == 1 {
        ""
    } else {
        "s"
    }
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
