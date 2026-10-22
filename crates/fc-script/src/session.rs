//! The script-facing document session: a [`Document`] plus a transcript
//! of what the script did to it.
//!
//! Every operation here mirrors the corresponding CLI verb's semantics
//! and honest error messages (same wording, minus the file-context the
//! CLI has): `add_*` mirrors `add`, `set` mirrors `set` (values parsed
//! against the property's existing type), `bind`/`unbind` mirror
//! `set-expr`/`clear-expr`, `recompute` mirrors `recompute` (failure
//! recorded, not raised), `volume`/`area`/`bbox` mirror `stats`, and
//! `save`/`export_*` mirror the persistence verbs. Errors are returned
//! as honest `String`s — the rhai layer turns them into runtime errors.
//!
//! The document lives behind `Arc<Mutex<..>>` because rhai requires
//! custom types to be `Clone + Send + Sync`: two script variables can
//! alias the same document (assignment shares the handle, like two
//! Python names for one `App.Document`).

use std::io::Write as _;
use std::path::Path;
use std::sync::{Arc, Mutex};

use fc_app::{Document, DocumentObject, Property};
use fc_base::{Placement, Rotation, Vector3};
use fc_io::mesh_export::{write_obj, write_stl_ascii, write_stl_binary};
use fc_io::{load_document, save_document};
use fc_part::features::{
    BooleanFeature, BooleanKind, BoxFeature, ConeFeature, CylinderFeature, SphereFeature,
    TorusFeature, TransformFeature,
};
use fc_part::{mesh_of, shape_of, Mesh, TopoShape};

/// Everything a script run leaves behind: the transcript (what was done,
/// in order — printed by the CLI's `script` verb), load warnings, and the
/// recompute failure (if any), which is what makes the process exit
/// non-zero — the CLI's own recompute-failure semantics.
#[derive(Default)]
pub struct ScriptLog {
    pub events: Vec<String>,
    pub warnings: Vec<String>,
    pub recompute_error: Option<String>,
}

/// A document under script control. One script may hold several (each
/// `new_document`/`open_document` starts a fresh one); they all share the
/// run's [`ScriptLog`].
#[derive(Clone)]
pub struct ScriptSession {
    doc: Arc<Mutex<Document>>,
    log: Arc<Mutex<ScriptLog>>,
}

impl ScriptSession {
    /// `new_document(label)` — a fresh, empty document.
    pub fn new(label: &str, log: Arc<Mutex<ScriptLog>>) -> Self {
        let mut doc = Document::new(label);
        doc.set_creator("freecad-rs");
        let session = Self {
            doc: Arc::new(Mutex::new(doc)),
            log,
        };
        session.record(format!("new document \"{label}\" (0 objects)"));
        session
    }

    /// `open_document(path)` — load a `.FCStd` file. Load warnings
    /// (skipped payloads, unknown properties) go to the run's warnings,
    /// exactly as the CLI surfaces them on stderr.
    pub fn open(path: &str, log: Arc<Mutex<ScriptLog>>) -> Result<Self, String> {
        let (mut doc, report) =
            load_document(Path::new(path)).map_err(|e| format!("cannot load '{path}': {e}"))?;
        {
            let mut log = log.lock().expect("script log poisoned");
            for warning in &report.warnings {
                log.warnings.push(warning.clone());
            }
        }
        // Like the CLI's load helper: a freshly loaded document carries no
        // undoable history — the journal belongs to the file's session,
        // not this script run.
        doc.clear_history();
        // Geometry is rebuilt, not restored: the writer persists
        // parameters/links/placements (a real FreeCAD restores solely
        // OCCT BRep), so every parametric feature reloads Touched —
        // one recompute here and the script sees the same shapes the
        // saver saw, the same experience FreeCAD's restored shapes give.
        doc.recompute_all()
            .map_err(|e| format!("recompute after open: {e}"))?;
        let count = doc.object_count();
        let session = Self {
            doc: Arc::new(Mutex::new(doc)),
            log,
        };
        session.record(format!("opened {path} ({count} objects)"));
        Ok(session)
    }

    /// Run `f` with the document, locking out other handles for the call.
    fn with_doc<R>(&self, f: impl FnOnce(&mut Document) -> R) -> R {
        let mut doc = self.doc.lock().expect("document mutex poisoned");
        f(&mut doc)
    }

    fn record(&self, event: impl Into<String>) {
        self.log
            .lock()
            .expect("script log poisoned")
            .events
            .push(event.into());
    }

    fn warn(&self, warning: impl Into<String>) {
        self.log
            .lock()
            .expect("script log poisoned")
            .warnings
            .push(warning.into());
    }

    // -------------------------------------------------------------------
    // add — the CLI `add` verb's surface, minus sketches (a sketch object
    // needs `fc-sketcher` and is empty until a fragment is imported; not
    // reachable from a script).
    // -------------------------------------------------------------------

    /// `Part::Box` — `doc.add_box("Base", 30, 20, 5)`. Returns the final
    /// (uniquified) object name.
    pub fn add_box(&mut self, name: &str, length: f64, width: f64, height: f64) -> String {
        let mut f = BoxFeature::new(name);
        f.container.set("Length", Property::Float(length));
        f.container.set("Width", Property::Float(width));
        f.container.set("Height", Property::Float(height));
        self.add(f, "Part::Box")
    }

    /// `Part::Sphere` — `doc.add_sphere("Ball", 4)`.
    pub fn add_sphere(&mut self, name: &str, radius: f64) -> String {
        let mut f = SphereFeature::new(name);
        f.container.set("Radius", Property::Float(radius));
        self.add(f, "Part::Sphere")
    }

    /// `Part::Cylinder` — `doc.add_cylinder("Stem", 1.5, 12)`.
    pub fn add_cylinder(&mut self, name: &str, radius: f64, height: f64) -> String {
        let mut f = CylinderFeature::new(name);
        f.container.set("Radius", Property::Float(radius));
        f.container.set("Height", Property::Float(height));
        self.add(f, "Part::Cylinder")
    }

    /// `Part::Cone` — `doc.add_cone("Cone", 2, 0, 10)`.
    pub fn add_cone(&mut self, name: &str, radius1: f64, radius2: f64, height: f64) -> String {
        let mut f = ConeFeature::new(name);
        f.container.set("Radius1", Property::Float(radius1));
        f.container.set("Radius2", Property::Float(radius2));
        f.container.set("Height", Property::Float(height));
        self.add(f, "Part::Cone")
    }

    /// `Part::Torus` — `doc.add_torus("Ring", 10, 2)`.
    pub fn add_torus(&mut self, name: &str, radius1: f64, radius2: f64) -> String {
        let mut f = TorusFeature::new(name);
        f.container.set("Radius1", Property::Float(radius1));
        f.container.set("Radius2", Property::Float(radius2));
        self.add(f, "Part::Torus")
    }

    /// `Part::Transform` — `doc.add_transform("Lift", "Ball")`. The source
    /// must exist (checked here, like the CLI's `add transform`); the
    /// placement and scale are set afterwards with `set`.
    pub fn add_transform(&mut self, name: &str, source: &str) -> Result<String, String> {
        if self.with_doc(|doc| doc.get_by_name(source).is_none()) {
            return Err(format!(
                "object '{source}' not found; available: {}",
                self.names_list()
            ));
        }
        let mut f = TransformFeature::new(name);
        f.container
            .set("Source", Property::Link(source.to_string()));
        Ok(self.add(f, "Part::Transform"))
    }

    /// `Part::Cut` — subtract `tool` from `base`.
    pub fn add_cut(&mut self, name: &str, base: &str, tool: &str) -> Result<String, String> {
        self.add_boolean(name, BooleanKind::Cut, base, tool)
    }

    /// `Part::Fuse` — union of `base` and `tool`.
    pub fn add_fuse(&mut self, name: &str, base: &str, tool: &str) -> Result<String, String> {
        self.add_boolean(name, BooleanKind::Fuse, base, tool)
    }

    /// `Part::Common` — intersection of `base` and `tool`.
    pub fn add_common(&mut self, name: &str, base: &str, tool: &str) -> Result<String, String> {
        self.add_boolean(name, BooleanKind::Common, base, tool)
    }

    fn add_boolean(
        &mut self,
        name: &str,
        kind: BooleanKind,
        base: &str,
        tool: &str,
    ) -> Result<String, String> {
        for (label, operand) in [("base", base), ("tool", tool)] {
            if self.with_doc(|doc| doc.get_by_name(operand).is_none()) {
                return Err(format!(
                    "{label} object '{operand}' not found; available: {}",
                    self.names_list()
                ));
            }
        }
        let mut f = BooleanFeature::new(name, kind);
        f.container.set("Base", Property::Link(base.to_string()));
        f.container.set("Tool", Property::Link(tool.to_string()));
        Ok(self.add(f, kind.type_name()))
    }

    /// Add a fully-built feature (properties set before the document sees
    /// it, so the name uniquifier and the dependency graph observe a
    /// complete object — the CLI's `add` discipline) and record it.
    fn add(&mut self, object: impl DocumentObject + 'static, type_name: &str) -> String {
        let name = self.with_doc(|doc| {
            let id = doc.add(Box::new(object));
            doc.get(id)
                .map(|o| o.name().to_string())
                .unwrap_or_default()
        });
        self.record(format!("added {name} ({type_name})"));
        name
    }

    // -------------------------------------------------------------------
    // set / bind — property values and expression bindings
    // -------------------------------------------------------------------

    /// `doc.set(object, property, value_text)` — mirrors the CLI `set`
    /// verb exactly: the text is parsed against the property's **existing**
    /// type (`Property::parse`), so `"Ball"` into a Link property becomes a
    /// link, `1,2,3` into a Vector becomes a vector, and a mismatch is an
    /// honest error naming the expected form.
    pub fn set(&mut self, object: &str, property: &str, text: &str) -> Result<(), String> {
        let existing_type = self.with_doc(|doc| {
            let obj = doc.get_by_name(object).ok_or_else(|| {
                format!(
                    "object '{object}' not found; available: {}",
                    names_list(doc)
                )
            })?;
            let existing = obj.properties().get(property).ok_or_else(|| {
                format!(
                    "object '{object}' has no property '{property}'; properties: {}",
                    props_list(obj)
                )
            })?;
            Ok::<_, String>(existing.type_name().to_string())
        })?;
        let parsed = Property::parse(&existing_type, text).ok_or_else(|| {
            format!(
                "cannot parse '{text}' as {existing_type} for {object}.{property} (expected: {})",
                expected_form(&existing_type)
            )
        })?;
        self.with_doc(|doc| doc.set_property(object, property, parsed))?;
        self.record(format!("set {object}.{property} = {text}"));
        Ok(())
    }

    /// `doc.bind(object, property, expression)` — FreeCAD
    /// `setExpression`: the expression must parse and the property must
    /// exist and be a `Float`; the binding is evaluated at every recompute,
    /// dependencies-first (the `fc-app` expression engine owns the
    /// semantics, including cycle rejection and honest type-mismatch
    /// failures at recompute time).
    pub fn bind(&mut self, object: &str, property: &str, expression: &str) -> Result<(), String> {
        self.with_doc(|doc| doc.bind_expression(object, property, expression))?;
        self.record(format!("bound {object}.{property} = {expression}"));
        Ok(())
    }

    /// `doc.unbind(object, property)` — remove an expression binding; the
    /// property keeps its last evaluated value and becomes settable again.
    pub fn unbind(&mut self, object: &str, property: &str) -> Result<(), String> {
        self.with_doc(|doc| doc.clear_expression(object, property))?;
        self.record(format!("unbound {object}.{property}"));
        Ok(())
    }

    // -------------------------------------------------------------------
    // recompute — dependencies-first execution
    // -------------------------------------------------------------------

    /// Recompute every touched object in dependency order. `true` on
    /// success; on failure the failing object's error is recorded in the
    /// transcript and in the run's `recompute_error` (which turns into the
    /// CLI's exit code), and `false` is returned so the script can react.
    pub fn recompute(&mut self) -> bool {
        let report = self.with_doc(|doc| doc.recompute_all());
        match report {
            Ok(ids) => {
                for event in self.with_doc(|doc| recompute_events(doc, &ids)) {
                    self.record(event);
                }
                let total = self.with_doc(|doc| doc.object_count());
                self.record(format!("{} recomputed, {} total", ids.len(), total));
                true
            }
            Err(err) => {
                self.record(format!("recompute failed: {err}"));
                self.log
                    .lock()
                    .expect("script log poisoned")
                    .recompute_error = Some(err.to_string());
                false
            }
        }
    }

    // -------------------------------------------------------------------
    // stats — the exact analytic numbers behind the CLI `stats` verb
    // -------------------------------------------------------------------

    /// Exact analytic volume of an object's computed geometry (a
    /// `Part::Transform`/boolean reports its mesh's volume, like `stats`).
    pub fn volume(&self, name: &str) -> Result<f64, String> {
        Ok(self.shape_of(name)?.volume())
    }

    /// Exact analytic surface area.
    pub fn area(&self, name: &str) -> Result<f64, String> {
        Ok(self.shape_of(name)?.surface_area())
    }

    /// Exact analytic bounding box (primitives: the canonical kernel
    /// position — a primitive's own `Placement` property is carried but
    /// not baked into its shape; a `Part::Transform`'s box reflects the
    /// transform).
    pub fn bbox(&self, name: &str) -> Result<fc_base::BoundBox3, String> {
        Ok(self.shape_of(name)?.bbox())
    }

    /// The computed `TopoShape` of a document object — the stats verbs'
    /// shared lookup (`shape_of`, falling back to the computed mesh), with
    /// honest errors for unknown objects and missing geometry.
    fn shape_of(&self, name: &str) -> Result<TopoShape, String> {
        self.with_doc(|doc| {
            let obj = doc.get_by_name(name).ok_or_else(|| {
                format!("object '{name}' not found; available: {}", names_list(doc))
            })?;
            if let Some(shape) = shape_of(obj) {
                return Ok(shape);
            }
            if let Some(mesh) = mesh_of(obj, fc_part::DEFAULT_DEFLECTION) {
                return Ok(TopoShape::Mesh(mesh));
            }
            Err(format!(
                "object '{name}' has no computed geometry (not a part feature, or not yet \
                 recomputed)"
            ))
        })
    }

    // -------------------------------------------------------------------
    // persistence + export
    // -------------------------------------------------------------------

    /// `doc.save(path)` — write the document as `.FCStd` (via `fc-io`).
    /// Unlike the CLI's mutating verbs, a script owns its persistence:
    /// only explicit `save` calls write anything.
    pub fn save(&mut self, path: &str) -> Result<(), String> {
        {
            let doc = self.doc.lock().expect("document mutex poisoned");
            save_document(&doc, Path::new(path))
                .map_err(|e| format!("cannot save '{path}': {e}"))?;
        }
        self.record(format!("saved {path}"));
        Ok(())
    }

    /// `doc.export_stl(path, binary)` — export the placed meshes as STL
    /// (one merged solid; the format carries no names). Recomputes first,
    /// like the CLI's export verbs.
    pub fn export_stl(&mut self, path: &str, binary: bool) -> Result<(), String> {
        self.recompute_for_export()?;
        let meshes = self.placed_meshes()?;
        if meshes.len() > 1 {
            self.warn(format!(
                "note: {} objects in the document — STL has no names; all geometry is \
                 merged into one solid",
                meshes.len()
            ));
        }
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
        let count = triangles.len();
        let file =
            std::fs::File::create(path).map_err(|e| format!("cannot write '{path}': {e}"))?;
        let mut out = std::io::BufWriter::new(file);
        let result = if binary {
            write_stl_binary("freecad-rs", &vertices, &triangles, &mut out)
        } else {
            write_stl_ascii("freecad-rs", &vertices, &triangles, &mut out)
        };
        result.map_err(|e| format!("cannot write '{path}': {e}"))?;
        out.flush()
            .map_err(|e| format!("cannot write '{path}': {e}"))?;
        let flavor = if binary { "binary" } else { "ASCII" };
        self.record(format!("wrote {path} ({flavor} STL, {count} triangles)"));
        Ok(())
    }

    /// `doc.export_obj(path)` — export the placed meshes as Wavefront OBJ
    /// (one named `o` group per document object). Recomputes first.
    pub fn export_obj(&mut self, path: &str) -> Result<(), String> {
        self.recompute_for_export()?;
        let meshes = self.placed_meshes()?;
        let objects = meshes.len();
        let triangles: usize = meshes.iter().map(|(_, m)| m.triangle_count()).sum();
        let file =
            std::fs::File::create(path).map_err(|e| format!("cannot write '{path}': {e}"))?;
        let mut out = std::io::BufWriter::new(file);
        for (name, mesh) in &meshes {
            writeln!(out, "o {}", name.replace(char::is_whitespace, "_"))
                .map_err(|e| format!("cannot write '{path}': {e}"))?;
            write_obj(&mesh.vertices, &mesh.triangles, &mut out)
                .map_err(|e| format!("cannot write '{path}': {e}"))?;
        }
        out.flush()
            .map_err(|e| format!("cannot write '{path}': {e}"))?;
        self.record(format!(
            "wrote {path} (OBJ, {objects} objects, {triangles} triangles)"
        ));
        Ok(())
    }

    /// The CLI export verbs' gate: the model must be current and
    /// consistent before it is rendered; nothing is written otherwise.
    fn recompute_for_export(&mut self) -> Result<(), String> {
        self.with_doc(|doc| doc.recompute_all().map(|_| ()))
            .map_err(|e| format!("recompute failed: {e}"))
    }

    /// The document's placed meshes — the CLI's `placed_meshes` logic:
    /// primitives keep their shape at the origin and their `Placement` is
    /// applied here; `Part::Transform` and the booleans already bake their
    /// placement into the computed mesh (re-applying would double it).
    fn placed_meshes(&self) -> Result<Vec<(String, Mesh)>, String> {
        Ok(self.with_doc(|doc| {
            let mut meshes = Vec::new();
            for s in doc.summary() {
                let Some(obj) = doc.get(s.id) else {
                    continue;
                };
                let Some(mut mesh) = mesh_of(obj, fc_part::DEFAULT_DEFLECTION) else {
                    continue;
                };
                // FreeCAD rule: every object's own `Placement` positions
                // its local shape; only `Part::Transform` bakes its own
                // Placement into the computed mesh.
                if obj.type_name() != "Part::Transform" {
                    if let Some(Property::Placement(p)) = obj.properties().get("Placement") {
                        mesh.transform(&p.to_matrix());
                    }
                }
                meshes.push((s.name.clone(), mesh));
            }
            meshes
        }))
    }

    fn names_list(&self) -> String {
        self.with_doc(|doc| names_list(doc))
    }
}

/// Per-object transcript lines for a successful recompute (the CLI
/// recompute verb's output).
fn recompute_events(doc: &Document, ids: &[fc_app::ObjectId]) -> Vec<String> {
    let summaries = doc.summary();
    ids.iter()
        .filter_map(|id| {
            summaries
                .iter()
                .find(|s| s.id == *id)
                .map(|s| format!("  recomputed {} ({})", s.name, s.type_name))
        })
        .collect()
}

/// Object-name list for honest "not found" errors — the CLI's format.
fn names_list(doc: &Document) -> String {
    let names: Vec<String> = doc.summary().into_iter().map(|s| s.name).collect();
    if names.is_empty() {
        "document is empty".to_string()
    } else {
        names.join(", ")
    }
}

/// Property list for honest "has no property" errors — the CLI's format.
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

/// Expected value form per property type — the CLI `set` error's hint,
/// mirrored verbatim so script errors read like CLI errors.
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

/// A placement value built from script data (`placement(pos, axis, deg)`),
/// wrapping the kernel type so it can be assigned to a Placement property
/// through `set`.
#[derive(Clone)]
pub struct PlacementVal(pub Placement);

/// Build a placement from a position, a rotation axis and an angle in
/// degrees — the same construction the CLI's `add transform --rotate-*`
/// performs, with the same honest rejection of a degenerate axis.
pub fn placement_of(
    position: Vector3,
    axis: Vector3,
    angle_deg: f64,
) -> Result<PlacementVal, String> {
    let rotation = Rotation::from_axis_angle(axis, angle_deg.to_radians())
        .map_err(|_| "placement axis must be a non-zero vector".to_string())?;
    Ok(PlacementVal(Placement::new(position, rotation)))
}
