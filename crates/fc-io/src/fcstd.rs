//! `.FCStd` reader/writer. See the crate docs for the format.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::Path;

use fc_app::{Document, DocumentObject, ObjectStatus, Property, PropertyContainer};
use fc_base::{Placement, Rotation, Vector3};
use fc_part::brep::{self, BrpFormat};
use fc_part::features::{
    BooleanFeature, BooleanKind, BoxFeature, ConeFeature, CylinderFeature, MeshFeature,
    MirroringFeature, MultiFuseFeature, SphereFeature, TorusFeature, TransformFeature,
};
use fc_part::partdesign::{PadFeature, PocketFeature, RevolutionFeature};
use fc_part::{TopoShape, DEFAULT_DEFLECTION};
use fc_sketcher::constraint::Constraint;
use fc_sketcher::geometry::Geometry;
use fc_sketcher::SketchObject;
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

#[derive(Debug)]
pub enum FcstdError {
    Io(std::io::Error),
    Zip(String),
    Xml(String),
    Unsupported(String),
}

impl std::fmt::Display for FcstdError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FcstdError::Io(e) => write!(f, "i/o error: {e}"),
            FcstdError::Zip(e) => write!(f, "zip container error: {e}"),
            FcstdError::Xml(e) => write!(f, "Document.xml error: {e}"),
            FcstdError::Unsupported(e) => write!(f, "unsupported: {e}"),
        }
    }
}

impl std::error::Error for FcstdError {}

impl From<std::io::Error> for FcstdError {
    fn from(e: std::io::Error) -> Self {
        FcstdError::Io(e)
    }
}

/// What happened while loading (never fatal by itself; warnings are
/// reported, objects are still created when possible).
#[derive(Debug, Default, Clone)]
pub struct LoadReport {
    pub warnings: Vec<String>,
    /// Objects successfully constructed.
    pub objects: usize,
    /// Objects skipped entirely (unknown type, unreadable).
    pub skipped: usize,
    /// Per-property skips (WOM reader breadth): properties whose value
    /// could not be read at all — unknown value element or malformed
    /// payload — as `Object.Property` paths. The owning object still
    /// loads with its other parameters; the reason is always in
    /// [`LoadReport::warnings`] too. A property here is never a silent
    /// drop.
    pub skipped_properties: Vec<String>,
    /// Properties that arrived quantity-typed (`App::PropertyAngle` /
    /// `App::PropertyLength` / `App::PropertyDistance`) and were mapped
    /// to plain floats, as `Object.Property`, plus ` (unit X)` when the
    /// file carried a unit attribute. Bookkeeping, not a loss: the
    /// float value is kept.
    pub quantity_properties: Vec<String>,
}

/// Property types whose file form is a plain float with an optional
/// unit attribute (FreeCAD `App::PropertyQuantity` family, as far as
/// this loader's contract reaches). Mapped to [`Property::Float`] and
/// recorded in [`LoadReport::quantity_properties`].
const QUANTITY_TYPES: [&str; 3] = [
    "App::PropertyAngle",
    "App::PropertyLength",
    "App::PropertyDistance",
];

// ---------------------------------------------------------------------------
// Saving
// ---------------------------------------------------------------------------

/// Save `doc` as `.FCStd`. Writes `Document.xml` first, then one FCBREP
/// side file per part feature (`Shape001.brp`, `Shape002.brp`, ...).
/// Deflate-compressed entries, like FreeCAD.
pub fn save_document(doc: &Document, path: &Path) -> Result<(), FcstdError> {
    let file = std::fs::File::create(path)?;
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    let mut side_files: Vec<(String, Vec<u8>)> = Vec::new();
    let xml = build_document_xml(doc, &mut side_files)?;

    zip.start_file("Document.xml", options).map_err(zip_err)?;
    zip.write_all(xml.as_bytes())?;
    for (name, bytes) in &side_files {
        zip.start_file(name.as_str(), options).map_err(zip_err)?;
        zip.write_all(bytes)?;
    }
    zip.finish().map_err(zip_err)?;
    Ok(())
}

fn zip_err(e: zip::result::ZipError) -> FcstdError {
    FcstdError::Zip(e.to_string())
}

/// Render `Document.xml` (FreeCAD's exact skeleton, one-space indentation)
/// and collect the FCBREP side files to store next to it.
fn build_document_xml(
    doc: &Document,
    side_files: &mut Vec<(String, Vec<u8>)>,
) -> Result<String, FcstdError> {
    let mut xml = String::new();
    xml.push_str(&format!(
        "<Document SchemaVersion=\"4\" ProgramVersion=\"freecad-rs {}\" FileVersion=\"1\">\n",
        env!("CARGO_PKG_VERSION")
    ));

    // Document-level properties (Label / Creator / CreationDate).
    xml.push_str(" <Properties Count=\"3\">\n");
    push_property(
        &mut xml,
        2,
        "Label",
        &Property::String(doc.label().to_string()),
        "App::Document",
    );
    push_property(
        &mut xml,
        2,
        "Creator",
        &Property::String(doc.creator().to_string()),
        "App::Document",
    );
    push_property(
        &mut xml,
        2,
        "CreationDate",
        &Property::String(doc.created().to_string()),
        "App::Document",
    );
    xml.push_str(" </Properties>\n");

    let objects = doc.summary();

    // Object table.
    xml.push_str(&format!(
        " <Objects Count=\"{}\" Dependencies=\"1\">\n",
        objects.len()
    ));
    for o in &objects {
        let Some(obj) = doc.get(o.id) else { continue };
        xml.push_str(&format!(
            "  <Object type=\"{}\" name=\"{}\" id=\"{}\"",
            escape_attr(&o.type_name),
            escape_attr(&o.name),
            o.id.0
        ));
        // A parametric feature whose geometry is not in the file
        // (everything but imported meshes and the last-good shapes of
        // errored features) saves as Touched, not Valid: the status is
        // the file's promise about its geometry, and a reader without
        // OCCT — a real FreeCAD restoring our file — must recompute
        // before the shapes exist. Our own reader agrees.
        let persisted = obj.as_any().downcast_ref::<MeshFeature>().is_some()
            || matches!(obj.status(), ObjectStatus::Error(_));
        match obj.status() {
            ObjectStatus::Touched => xml.push_str(" status=\"Touched\""),
            ObjectStatus::Valid if persisted || !expects_geometry(&o.type_name) => {
                xml.push_str(" status=\"Valid\"")
            }
            ObjectStatus::Valid => xml.push_str(" status=\"Touched\""),
            ObjectStatus::Error(ref reason) => {
                xml.push_str(&format!(
                    " status=\"Error\" Error=\"{}\"",
                    escape_attr(reason)
                ));
            }
        }
        xml.push_str("/>\n");
    }
    xml.push_str(" </Objects>\n");

    // Per-object property data.
    xml.push_str(&format!(" <ObjectData Count=\"{}\">\n", objects.len()));
    for o in &objects {
        let obj = doc.get(o.id).ok_or_else(|| {
            FcstdError::Xml(format!(
                "internal: object '{}' missing from document",
                o.name
            ))
        })?;
        let props = obj.properties();
        // Sketch content goes out as the FreeCAD-native property pair
        // (`Geometry` + `Constraints`) — the exact forms a real FreeCAD
        // writes and restores, so the whole sketch — geometry and
        // constraints — survives the trip into the real program. Our
        // old `<Sketch>` object-data fragment is retired from the
        // writer; the reader still accepts it (files from v0.1.0).
        let sketch_props = obj.as_any().downcast_ref::<SketchObject>().map(|so| {
            (
                sketch_geometrylist_xml(&so.sketch),
                sketch_constraintlist_xml(&so.sketch),
            )
        });
        // Shape payloads go to exactly the objects FreeCAD's own files
        // give them: imported data (`Part::Feature` meshes) and the
        // **last-good geometry of an errored feature** (FreeCAD keeps
        // the old shape in the file when a recompute fails — loading
        // shows it, flagged). Every healthy parametric feature rebuilds
        // from its parameters on recompute — ours in this kernel,
        // FreeCAD's own engine in a real FreeCAD, which restores only
        // OCCT BRep and would reject our FCBREP bytes as
        // "shape is invalid".
        let last_good = matches!(obj.status(), ObjectStatus::Error(_));
        let shape = if last_good || obj.as_any().downcast_ref::<MeshFeature>().is_some() {
            computed_shape(obj)
        } else {
            None
        };
        let count =
            props.len() + usize::from(shape.is_some()) + usize::from(sketch_props.is_some()) * 2;

        xml.push_str(&format!("  <Object name=\"{}\">\n", escape_attr(&o.name)));
        xml.push_str(&format!("   <Properties Count=\"{}\">\n", count));
        for (key, value) in props {
            push_property(&mut xml, 4, key, value, &o.type_name);
        }
        // The sketch's own content: one `Part::PropertyGeometryList`
        // and one `Sketcher::PropertyConstraintList`, exactly where
        // FreeCAD's writer puts them (after the plain properties).
        if let Some((geometry_xml, constraints_xml)) = sketch_props {
            xml.push_str("    <Property name=\"Geometry\" type=\"Part::PropertyGeometryList\">\n");
            xml.push_str(&geometry_xml);
            xml.push_str("    </Property>\n");
            xml.push_str(
                "    <Property name=\"Constraints\" type=\"Sketcher::PropertyConstraintList\">\n",
            );
            xml.push_str(&constraints_xml);
            xml.push_str("    </Property>\n");
        }
        // Computed geometry goes last, as one referenced FCBREP side file.
        if let Some(shape) = shape {
            let file = format!("Shape{:03}.brp", side_files.len() + 1);
            xml.push_str("    <Property name=\"Shape\" type=\"Part::PropertyPartShape\">\n");
            xml.push_str(&format!("     <Part file=\"{}\"/>\n", escape_attr(&file)));
            xml.push_str("    </Property>\n");
            let mut bytes = Vec::new();
            brep::write_shape(&mut bytes, &shape)?;
            side_files.push((file, bytes));
        }
        xml.push_str("   </Properties>\n");
        // Expression bindings (FreeCAD persists them as children of the
        // <Object> element, after the properties).
        for (path, expression) in obj.expressions() {
            xml.push_str(&format!(
                "   <Expression path=\"{}\" expression=\"{}\"/>\n",
                escape_attr(path),
                escape_attr(expression)
            ));
        }
        xml.push_str("  </Object>\n");
    }
    xml.push_str(" </ObjectData>\n");
    xml.push_str("</Document>\n");
    Ok(xml)
}

/// Append one `<Property>` (with its value element) at `indent` spaces.
fn push_property(xml: &mut String, indent: usize, name: &str, value: &Property, owner_type: &str) {
    let ind = " ".repeat(indent);
    let (default_ty, body) = property_xml(value);
    // FreeCAD's reader finds a property by name and then restores it
    // through its registered C++ type — a `type` attribute that
    // disagrees is silently skipped. The Part primitives' dimensions
    // are `App::PropertyLength` in FreeCAD 1.0 (verified against its
    // own files), so that is what our files must say.
    let ty = if default_ty == "App::PropertyFloat" && is_primitive_dimension(owner_type, name) {
        "App::PropertyLength"
    } else {
        default_ty
    };
    xml.push_str(&format!(
        "{ind}<Property name=\"{}\" type=\"{ty}\">\n",
        escape_attr(name)
    ));
    xml.push_str(&format!("{ind} {body}\n"));
    xml.push_str(&format!("{ind}</Property>\n"));
}

/// The dimension properties FreeCAD types `App::PropertyLength`
/// (checked against FreeCAD 1.0's own serialization of the Part
/// primitives; `PartDesign` lengths stay `App::PropertyFloat`).
fn is_primitive_dimension(owner_type: &str, name: &str) -> bool {
    match owner_type {
        "Part::Box" => matches!(name, "Length" | "Width" | "Height"),
        "Part::Sphere" => name == "Radius",
        "Part::Cylinder" => matches!(name, "Radius" | "Height"),
        "Part::Cone" => matches!(name, "Radius1" | "Radius2" | "Height"),
        "Part::Torus" => matches!(name, "Radius1" | "Radius2"),
        _ => false,
    }
}

/// The FreeCAD value element for a property, per the crate's element table.
fn property_xml(value: &Property) -> (&'static str, String) {
    match value {
        Property::Bool(v) => (
            "App::PropertyBool",
            format!("<Bool value=\"{v}\"/>"),
        ),
        // Rust's `{}` for f64 prints the shortest round-trip form, which is
        // what FreeCAD's full-precision writer aims for too.
        Property::Float(v) => (
            "App::PropertyFloat",
            format!("<Float value=\"{v}\"/>"),
        ),
        Property::Integer(v) => (
            "App::PropertyInteger",
            format!("<Integer value=\"{v}\"/>"),
        ),
        Property::String(v) => (
            "App::PropertyString",
            format!("<String value=\"{}\"/>", escape_attr(v)),
        ),
        Property::Vector(v) => (
            "App::PropertyVector",
            format!(
                "<PropertyVector valueX=\"{}\" valueY=\"{}\" valueZ=\"{}\"/>",
                v.x, v.y, v.z
            ),
        ),
        // A/Ox/Oy/Oz are optional in FreeCAD's format; we omit them.
        Property::Placement(p) => (
            "App::PropertyPlacement",
            format!(
                "<PropertyPlacement Px=\"{}\" Py=\"{}\" Pz=\"{}\" Q0=\"{}\" Q1=\"{}\" Q2=\"{}\" Q3=\"{}\"/>",
                p.position.x,
                p.position.y,
                p.position.z,
                p.rotation.x,
                p.rotation.y,
                p.rotation.z,
                p.rotation.w
            ),
        ),
        Property::Link(v) => (
            "App::PropertyLink",
            format!("<Link value=\"{}\"/>", escape_attr(v)),
        ),
        Property::LinkList(v) => {
            // FreeCAD's `PropertyLinkList::Restore` reads a `count`
            // attribute and then exactly that many nested `<Link>`
            // elements (App/PropertyLinks.cpp) — the attribute form it
            // never accepts. Mirror FreeCAD's own serialization.
            let mut body = format!("<LinkList count=\"{}\">", v.len());
            for name in v {
                body.push_str(&format!("<Link value=\"{}\"/>", escape_attr(name)));
            }
            body.push_str("</LinkList>");
            ("App::PropertyLinkList", body)
        }
    }
}

/// A sketch's geometry as FreeCAD 1.0's `Part::PropertyGeometryList`
/// value (`<GeometryList>`): one `<Geometry type="Part::Geom…"
/// migrated="1">` block per element, each carrying its kind's own child
/// element with the parameters — the byte shapes of
/// `GeomPoint::Save`/`GeomLineSegment::Save`/`GeomCircle::Save`/
/// `GeomArcOfCircle::Save` in the FreeCAD source. Everything is
/// XY-plane (`Z="0"`, `Normal="0 0 1"`, `AngleXU="0"`), exactly the
/// subset this kernel's 2D sketcher models.
fn sketch_geometrylist_xml(sketch: &fc_sketcher::Sketch) -> String {
    let mut xml = format!(
        "     <GeometryList count=\"{}\">\n",
        sketch.geometry().len()
    );
    for g in sketch.geometry() {
        match *g {
            Geometry::Point { x, y } => {
                xml.push_str("      <Geometry type=\"Part::GeomPoint\" migrated=\"1\">\n");
                xml.push_str("       <GeoExtensions count=\"0\">\n       </GeoExtensions>\n");
                xml.push_str(&format!(
                    "       <GeomPoint X=\"{x:?}\" Y=\"{y:?}\" Z=\"0.0\"/>\n"
                ));
                xml.push_str("      </Geometry>\n");
            }
            Geometry::LineSegment { start, end } => {
                xml.push_str("      <Geometry type=\"Part::GeomLineSegment\" migrated=\"1\">\n");
                xml.push_str("       <GeoExtensions count=\"0\">\n       </GeoExtensions>\n");
                xml.push_str(&format!(
                    "       <LineSegment StartX=\"{:?}\" StartY=\"{:?}\" StartZ=\"0.0\" \
                     EndX=\"{:?}\" EndY=\"{:?}\" EndZ=\"0.0\"/>\n",
                    start.x, start.y, end.x, end.y
                ));
                xml.push_str("      </Geometry>\n");
            }
            Geometry::Circle { center, radius } => {
                xml.push_str("      <Geometry type=\"Part::GeomCircle\" migrated=\"1\">\n");
                xml.push_str("       <GeoExtensions count=\"0\">\n       </GeoExtensions>\n");
                xml.push_str(&format!(
                    "       <Circle CenterX=\"{:?}\" CenterY=\"{:?}\" CenterZ=\"0.0\" \
                     NormalX=\"0.0\" NormalY=\"0.0\" NormalZ=\"1.0\" AngleXU=\"0.0\" \
                     Radius=\"{radius:?}\"/>\n",
                    center.x, center.y
                ));
                xml.push_str("      </Geometry>\n");
            }
            Geometry::ArcOfCircle {
                center,
                radius,
                start_angle,
                end_angle,
            } => {
                xml.push_str("      <Geometry type=\"Part::GeomArcOfCircle\" migrated=\"1\">\n");
                xml.push_str("       <GeoExtensions count=\"0\">\n       </GeoExtensions>\n");
                xml.push_str(&format!(
                    "       <ArcOfCircle CenterX=\"{:?}\" CenterY=\"{:?}\" CenterZ=\"0.0\" \
                     NormalX=\"0.0\" NormalY=\"0.0\" NormalZ=\"1.0\" AngleXU=\"0.0\" \
                     Radius=\"{radius:?}\" StartAngle=\"{start_angle:?}\" \
                     EndAngle=\"{end_angle:?}\"/>\n",
                    center.x, center.y
                ));
                xml.push_str("      </Geometry>\n");
            }
        }
    }
    xml.push_str("     </GeometryList>\n");
    xml
}

/// A sketch's constraints as FreeCAD 1.0's
/// `Sketcher::PropertyConstraintList` value (`<ConstraintList>`): one
/// `<Constrain …/>` element per constraint — the missing final "t" is
/// FreeCAD's own spelling (`Constraint::Save` in the FreeCAD source) —
/// carrying every attribute FreeCAD's writer emits, so FreeCAD's
/// `Constraint::Restore` finds its required set (`Name`, `Type`,
/// `Value`, the `First`/`Second`/`Third` triple) plus the presentation
/// flags. `Type` uses FreeCAD's `ConstraintType` ordinals
/// (Coincident=1 … Diameter=18), NOT our fragment dialect's. `Value`
/// carries radians for `Angle` and millimetres for the datum kinds,
/// matching FreeCAD's internal storage.
fn sketch_constraintlist_xml(sketch: &fc_sketcher::Sketch) -> String {
    let mut xml = format!(
        "     <ConstraintList count=\"{}\">\n",
        sketch.constraints().len()
    );
    for c in sketch.constraints() {
        // FreeCAD's element references: a used slot is (GeoId, pos
        // ordinal — shared numbering: start=1, end=2, mid=3), an unused
        // one is GeoUndef (-2000) with pos 0.
        let (ty, first, firstpos, second, secondpos, value) = match *c {
            Constraint::Coincident { g1, pos1, g2, pos2 } => (
                1,
                g1.0 as i64,
                pos1.ordinal(),
                g2.0 as i64,
                pos2.ordinal(),
                None,
            ),
            Constraint::Horizontal { g } => (2, g.0 as i64, 0, -2000, 0, None),
            Constraint::Vertical { g } => (3, g.0 as i64, 0, -2000, 0, None),
            Constraint::Parallel { g1, g2 } => (4, g1.0 as i64, 0, g2.0 as i64, 0, None),
            Constraint::Perpendicular { g1, g2 } => (10, g1.0 as i64, 0, g2.0 as i64, 0, None),
            Constraint::Distance {
                g1,
                pos1,
                g2,
                pos2,
                value,
            } => (
                6,
                g1.0 as i64,
                pos1.ordinal(),
                g2.0 as i64,
                pos2.ordinal(),
                Some(value),
            ),
            Constraint::DistanceX {
                g1,
                pos1,
                g2,
                pos2,
                value,
            } => (
                7,
                g1.0 as i64,
                pos1.ordinal(),
                g2.0 as i64,
                pos2.ordinal(),
                Some(value),
            ),
            Constraint::DistanceY {
                g1,
                pos1,
                g2,
                pos2,
                value,
            } => (
                8,
                g1.0 as i64,
                pos1.ordinal(),
                g2.0 as i64,
                pos2.ordinal(),
                Some(value),
            ),
            Constraint::Radius { g, value } => (11, g.0 as i64, 0, -2000, 0, Some(value)),
            Constraint::Diameter { g, value } => (18, g.0 as i64, 0, -2000, 0, Some(value)),
            Constraint::Equal { g1, g2 } => (12, g1.0 as i64, 0, g2.0 as i64, 0, None),
            Constraint::Angle { g1, g2, value } => (9, g1.0 as i64, 0, g2.0 as i64, 0, Some(value)),
            Constraint::PointOnObject { g1, pos1, g2 } => {
                (13, g1.0 as i64, pos1.ordinal(), g2.0 as i64, 0, None)
            }
        };
        let value = value.unwrap_or(0.0);
        xml.push_str(&format!(
            "      <Constrain Name=\"\" MetaData=\"\" Type=\"{ty}\" Orientation=\"0\" \
             Value=\"{value:?}\" LabelDistance=\"0.0\" LabelPosition=\"0.0\" \
             IsDriving=\"1\" IsInVirtualSpace=\"0\" IsVisible=\"1\" IsActive=\"1\" \
             First=\"{first}\" FirstPos=\"{firstpos}\" Second=\"{second}\" \
             SecondPos=\"{secondpos}\" Third=\"-2000\" ThirdPos=\"0\"/>\n",
        ));
    }
    xml.push_str("     </ConstraintList>\n");
    xml
}

/// Computed geometry of a part feature, if any: primitives carry their
/// analytic `TopoShape`, transform/boolean/PartDesign features their
/// result mesh. Objects without geometry (or features without a
/// result) return `None`.
fn computed_shape(obj: &dyn DocumentObject) -> Option<TopoShape> {
    let any = obj.as_any();
    if let Some(f) = any.downcast_ref::<BoxFeature>() {
        f.shape.clone()
    } else if let Some(f) = any.downcast_ref::<SphereFeature>() {
        f.shape.clone()
    } else if let Some(f) = any.downcast_ref::<CylinderFeature>() {
        f.shape.clone()
    } else if let Some(f) = any.downcast_ref::<ConeFeature>() {
        f.shape.clone()
    } else if let Some(f) = any.downcast_ref::<TorusFeature>() {
        f.shape.clone()
    } else if let Some(f) = any.downcast_ref::<TransformFeature>() {
        f.result.clone().map(TopoShape::Mesh)
    } else if let Some(f) = any.downcast_ref::<BooleanFeature>() {
        f.result.clone().map(TopoShape::Mesh)
    } else if let Some(f) = any.downcast_ref::<MultiFuseFeature>() {
        f.result.clone().map(TopoShape::Mesh)
    } else if let Some(f) = any.downcast_ref::<MirroringFeature>() {
        f.result.clone().map(TopoShape::Mesh)
    } else if let Some(f) = any.downcast_ref::<PadFeature>() {
        f.result.clone().map(TopoShape::Mesh)
    } else if let Some(f) = any.downcast_ref::<PocketFeature>() {
        f.result.clone().map(TopoShape::Mesh)
    } else if let Some(f) = any.downcast_ref::<RevolutionFeature>() {
        f.result.clone().map(TopoShape::Mesh)
    } else if let Some(f) = any.downcast_ref::<MeshFeature>() {
        f.shape.clone()
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Loading
// ---------------------------------------------------------------------------

/// Load a `.FCStd`. Accepts our own files and real FreeCAD files (see
/// crate docs for fidelity limits). Objects are added in file order (the
/// file's ids are cosmetic — links resolve by name) and each gets its
/// persisted properties, status and geometry where possible.
pub fn load_document(path: &Path) -> Result<(Document, LoadReport), FcstdError> {
    load_document_with_hook(path, None)
}

/// Per-object mutation hook for [`load_document_with_hook`]: receives the
/// object's file name and the fully-built object (properties, status and
/// sketch content already restored) just before it is added to the
/// document.
pub type ObjectHook<'a> = dyn FnMut(&str, &mut dyn DocumentObject) + 'a;

/// Load a `.FCStd`, letting `mutate` visit every fully-built object with
/// mutable access right before it enters the document.
///
/// This is the load-time escape hatch for content operations the document
/// API cannot express once an object is inside a `Document` (which
/// deliberately exposes no mutable object access): e.g. the CLI's
/// `sketch-import` swaps a `Sketcher::SketchObject`'s content via
/// `replace_sketch` (which touches the object) and marks the objects that
/// link to it as `Touched` so they re-execute on the next recompute. The
/// hook runs while objects are still outside the document, so touch
/// cascades and journal records are the caller's responsibility.
pub fn load_document_with_hook(
    path: &Path,
    mutate: Option<&mut ObjectHook<'_>>,
) -> Result<(Document, LoadReport), FcstdError> {
    let file = std::fs::File::open(path)?;
    let mut zip = zip::ZipArchive::new(file).map_err(zip_err)?;

    let xml_bytes = match zip.by_name("Document.xml") {
        Ok(mut entry) => {
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes)?;
            bytes
        }
        Err(zip::result::ZipError::FileNotFound) => {
            return Err(FcstdError::Xml(
                "no Document.xml entry in archive (not a .FCStd file?)".to_string(),
            ));
        }
        Err(e) => return Err(zip_err(e)),
    };

    let mut report = LoadReport::default();
    let mut doc = Document::new("untitled");
    parse_document(&xml_bytes, &mut zip, &mut doc, &mut report, mutate)?;

    Ok((doc, report))
}

/// Does this FreeCAD type carry geometry its own parameters (or the
/// features linking into it) rebuild on recompute? The parametric
/// feature families of this kernel; containers and data carriers are
/// excluded — their geometry, when present, is file-restored only.
fn expects_geometry(type_name: &str) -> bool {
    matches!(
        type_name,
        "Part::Box"
            | "Part::Sphere"
            | "Part::Cylinder"
            | "Part::Cone"
            | "Part::Torus"
            | "Part::Cut"
            | "Part::Fuse"
            | "Part::Common"
            | "Part::MultiFuse"
            | "Part::Mirroring"
            | "Part::Transform"
            | "PartDesign::Pad"
            | "PartDesign::Pocket"
            | "PartDesign::Revolution"
    )
}

/// One `<Part>` payload reference found while parsing.
struct ShapeRef {
    /// Side-file name inside the zip archive.
    file: Option<String>,
    /// Inline payload bytes (FreeCAD's `brep="1"` mode).
    inline: Vec<u8>,
}

/// A parsed property value element.
enum PropValue {
    Value(Property),
    Shape(ShapeRef),
    /// A sketch translated from FreeCAD 1.0's `Geometry` property
    /// (`Part::PropertyGeometryList`) into our `<Sketch>` fragment
    /// form — applied via `store_sketch_fragment`. `geo_remap` is the
    /// geometry-index translation: entry `i` is the fragment's index
    /// for the FreeCAD file's geometry `i`, or `None` when that
    /// geometry was skipped (off-plane, construction, unmodeled
    /// kind) — later `Constraints` references route through it.
    Sketch {
        fragment: String,
        geo_remap: Vec<Option<u32>>,
    },
    /// The raw inner XML of FreeCAD 1.0's `Constraints` property
    /// (`Sketcher::PropertyConstraintList`, `<ConstraintList …>`), held
    /// until the object is fully parsed — the geometry (and its remap)
    /// is known then, whatever the file's property order.
    SketchConstraints(String),
}

/// The pull-parser state we need between helper calls.
type XmlReader<'x> = Reader<&'x [u8]>;

/// Read the next event as owned data (so the shared buffer can be reused).
fn next_event(reader: &mut XmlReader<'_>, buf: &mut Vec<u8>) -> Result<Event<'static>, FcstdError> {
    reader
        .read_event_into(buf)
        .map(|ev| ev.into_owned())
        .map_err(|e| FcstdError::Xml(format!("XML parse error: {e}")))
}

/// All attributes of an element, unescaped, as an owned map.
fn attrs_of(e: &BytesStart<'_>) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    for a in e.attributes().flatten() {
        if let Ok(v) = a.unescape_value() {
            map.insert(
                String::from_utf8_lossy(a.key.as_ref()).into_owned(),
                v.into_owned(),
            );
        }
    }
    map
}

fn attr_f64(map: &BTreeMap<String, String>, key: &str) -> Option<f64> {
    map.get(key)?.trim().parse::<f64>().ok()
}

/// FreeCAD's bool spellings (`true`/`false`, `1`/`0`).
fn parse_bool(s: &str) -> Option<bool> {
    match s.trim() {
        "true" | "1" | "True" | "TRUE" => Some(true),
        "false" | "0" | "False" | "FALSE" => Some(false),
        _ => None,
    }
}

/// `Touched="1"` / `Invalid="1"` (FreeCAD) and `status="Touched|Valid|Error"`
/// (ours) both map onto [`ObjectStatus`].
fn parse_status(map: &BTreeMap<String, String>) -> ObjectStatus {
    if let Some(s) = map.get("status") {
        return match s.as_str() {
            "Valid" => ObjectStatus::Valid,
            "Error" => ObjectStatus::Error(
                map.get("Error")
                    .cloned()
                    .unwrap_or_else(|| "error (from file)".to_string()),
            ),
            // Unknown status words degrade to "needs recompute", the safe default.
            _ => ObjectStatus::Touched,
        };
    }
    if parse_bool(map.get("Invalid").map_or("", String::as_str)).unwrap_or(false) {
        return ObjectStatus::Error("invalid (from file)".to_string());
    }
    if parse_bool(map.get("Touched").map_or("", String::as_str)).unwrap_or(false) {
        ObjectStatus::Touched
    } else {
        // No status information at all: FreeCAD only omits Touched/Invalid
        // for objects that are up to date.
        ObjectStatus::Valid
    }
}

/// Consume the rest of a just-opened element (depth-counted).
fn skip_element(reader: &mut XmlReader<'_>, buf: &mut Vec<u8>) -> Result<(), FcstdError> {
    let mut depth = 1usize;
    loop {
        match next_event(reader, buf)? {
            Event::Start(_) => depth += 1,
            Event::End(_) => {
                depth -= 1;
                if depth == 0 {
                    return Ok(());
                }
            }
            Event::Eof => {
                return Err(FcstdError::Xml(
                    "unexpected end of Document.xml".to_string(),
                ));
            }
            _ => {}
        }
    }
}

/// Parse the whole `Document.xml` into `doc`/`report`, reading shape
/// payloads from `zip`.
fn parse_document(
    xml: &[u8],
    zip: &mut zip::ZipArchive<std::fs::File>,
    doc: &mut Document,
    report: &mut LoadReport,
    mut mutate: Option<&mut ObjectHook<'_>>,
) -> Result<(), FcstdError> {
    let mut reader = Reader::from_reader(xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();

    // Locate the <Document> root (skipping declaration / DOCTYPE / comments).
    loop {
        match next_event(&mut reader, &mut buf)? {
            Event::Start(e) => {
                if e.name().as_ref() != b"Document" {
                    return Err(FcstdError::Xml(format!(
                        "expected <Document> root, found <{}>",
                        String::from_utf8_lossy(e.name().as_ref())
                    )));
                }
                break;
            }
            Event::Eof => {
                return Err(FcstdError::Xml("no <Document> root element".to_string()));
            }
            _ => continue,
        }
    }

    // Objects built from the <Objects> table, in FILE order. They are
    // completed (properties, status, geometry) before being added to the
    // document, which then assigns fresh ids in insertion order.
    //
    // ID handling: the file's `id` attributes are deliberately ignored.
    // Links in the format are BY NAME (see the `<Link value="...">`
    // element), so real FreeCAD files with id gaps (objects were deleted)
    // load fine — `doc.add` gives our ids 1..N in insertion order instead.
    // The only observable difference is the cosmetic `id` attribute
    // renumbered on the next save.
    let mut built: Vec<(String, Option<Box<dyn DocumentObject>>)> = Vec::new();

    loop {
        match next_event(&mut reader, &mut buf)? {
            Event::Start(e) => match e.name().as_ref() {
                b"Properties" => {
                    parse_document_properties(&mut reader, &mut buf, xml, doc, report)?
                }
                b"Objects" => parse_objects_section(&mut reader, &mut buf, &mut built, report)?,
                b"ObjectData" => {
                    parse_object_data_section(&mut reader, &mut buf, xml, &mut built, zip, report)?
                }
                _ => skip_element(&mut reader, &mut buf)?,
            },
            // Self-closing sections: nothing to read inside them.
            Event::Empty(_) => {}
            Event::End(e) if e.name().as_ref() == b"Document" => break,
            Event::Eof => {
                return Err(FcstdError::Xml(
                    "unterminated <Document> element".to_string(),
                ));
            }
            _ => {}
        }
    }

    // Add the finished objects in file order; the document owns the ids from
    // here on. `add` may suffix names for uniqueness (Box, Box001, ...) —
    // only possible with duplicate names in the file, which we report.
    for (file_name, object) in built {
        if let Some(mut object) = object {
            // The caller's hook sees the finished object (properties,
            // status, sketch content) before the document takes ownership.
            if let Some(mutate) = mutate.as_deref_mut() {
                mutate(&file_name, object.as_mut());
            }
            // A parametric feature whose geometry did not come from the
            // file (no Shape payload — our writer omits it for everything
            // except imported `Part::Feature` meshes) loads as Touched,
            // so the first recompute rebuilds it — the state FreeCAD
            // leaves a touched feature in. Data carriers keep their
            // restored status: the file's word is final for imported
            // geometry.
            if object.as_any().downcast_ref::<MeshFeature>().is_none()
                && computed_shape(object.as_ref()).is_none()
                && expects_geometry(object.type_name())
                // A persisted Error is the file's honest verdict (the
                // recompute that failed and its message); it survives
                // the restore — only a clean Valid flips to Touched
                // for the geometry rebuild.
                && matches!(object.status(), ObjectStatus::Valid)
            {
                object.set_status(ObjectStatus::Touched);
            }
            let id = doc.add(object);
            let final_name = doc.get(id).map(DocumentObject::name).unwrap_or_default();
            if final_name != file_name {
                report.warnings.push(format!(
                    "object '{file_name}' renamed to '{final_name}' (duplicate name in file)"
                ));
            }
        }
    }

    // Links are by name; verify they resolve now that everything is loaded.
    for summary in doc.summary() {
        if let Some(obj) = doc.get_by_name(&summary.name) {
            for (key, value) in obj.properties() {
                let targets: Vec<String> = match value {
                    Property::Link(t) => vec![t.clone()],
                    Property::LinkList(ts) => ts.clone(),
                    _ => continue,
                };
                for t in targets {
                    if doc.get_by_name(&t).is_none() {
                        report.warnings.push(format!(
                            "object '{}': property '{key}' links to '{t}', which was not loaded \
                             (unknown or skipped object)",
                            summary.name
                        ));
                    }
                }
            }
        }
    }

    Ok(())
}

/// Document-level `<Properties>`: we persist Label/Creator/CreationDate;
/// other metadata (Uid, Company, ...) is not modeled and ignored.
fn parse_document_properties(
    reader: &mut XmlReader<'_>,
    buf: &mut Vec<u8>,
    xml: &[u8],
    doc: &mut Document,
    report: &mut LoadReport,
) -> Result<(), FcstdError> {
    loop {
        match next_event(reader, buf)? {
            Event::Start(e) if e.name().as_ref() == b"Property" => {
                let map = attrs_of(&e);
                let name = map.get("name").cloned().unwrap_or_default();
                let value =
                    parse_property_value(reader, buf, xml, "document", &name, &map, report)?;
                match (name.as_str(), value) {
                    ("Label", Some(PropValue::Value(Property::String(s)))) => doc.set_label(&s),
                    ("Creator", Some(PropValue::Value(Property::String(s)))) => doc.set_creator(&s),
                    ("CreationDate", Some(PropValue::Value(Property::String(s)))) => {
                        doc.set_created(s)
                    }
                    (n @ ("Label" | "Creator" | "CreationDate"), Some(_)) => {
                        report.warnings.push(format!(
                            "document property '{n}': not a string value — skipped"
                        ));
                    }
                    _ => {}
                }
            }
            Event::End(e) if e.name().as_ref() == b"Properties" => return Ok(()),
            Event::Eof => {
                return Err(FcstdError::Xml(
                    "unterminated <Properties> element".to_string(),
                ));
            }
            _ => continue,
        }
    }
}

/// The `<Objects>` table: type/name/id triplets, plus status dialects.
/// Builds the objects immediately (they get their data later).
fn parse_objects_section(
    reader: &mut XmlReader<'_>,
    buf: &mut Vec<u8>,
    built: &mut Vec<(String, Option<Box<dyn DocumentObject>>)>,
    report: &mut LoadReport,
) -> Result<(), FcstdError> {
    loop {
        match next_event(reader, buf)? {
            Event::Start(e) | Event::Empty(e) if e.name().as_ref() == b"Object" => {
                let map = attrs_of(&e);
                let type_id = map.get("type").cloned().unwrap_or_default();
                let name = map.get("name").cloned().unwrap_or_default();
                // `id` is read but intentionally unused (see parse_document).
                let _file_id = map.get("id");
                if type_id.is_empty() || name.is_empty() {
                    report.skipped += 1;
                    report.warnings.push(format!(
                        "<Object> entry without type or name (id {}) — skipped",
                        map.get("id").map_or("?", String::as_str)
                    ));
                    continue;
                }
                let status = parse_status(&map);
                let object = match build_object(&type_id, &name) {
                    Some(mut object) => {
                        object.set_status(status);
                        report.objects += 1;
                        Some(object)
                    }
                    // Unknown `Part::` family member (a `Part::Feature`
                    // derivative we do not model): load it as a
                    // parameter carrier so its readable properties come
                    // along — the geometry payload, if any, is skipped
                    // with the usual warnings. The source type id is
                    // kept in a `TypeId` string property; the object
                    // lists as `Part::Feature` (the family root).
                    None if type_id.starts_with("Part::") => {
                        report.warnings.push(format!(
                            "object '{name}': unknown Part:: family type '{type_id}' — loaded as \
                             'Part::Feature' parameter container (TypeId kept, geometry skipped)"
                        ));
                        let mut object = LoadContainer::new("Part::Feature", &name);
                        object
                            .container
                            .set("TypeId", Property::String(type_id.clone()));
                        object.set_status(status);
                        report.objects += 1;
                        Some(Box::new(object) as Box<dyn DocumentObject>)
                    }
                    None => {
                        report.skipped += 1;
                        report.warnings.push(format!(
                            "object '{name}': unknown type '{type_id}' — skipped"
                        ));
                        None
                    }
                };
                built.push((name, object));
            }
            Event::End(e) if e.name().as_ref() == b"Objects" => return Ok(()),
            Event::Eof => {
                return Err(FcstdError::Xml(
                    "unterminated <Objects> element".to_string(),
                ));
            }
            _ => continue,
        }
    }
}

/// The `<ObjectData>` section: feeds each `<Object>`'s `<Properties>` into
/// the object built from the `<Objects>` table (matched by name).
fn parse_object_data_section(
    reader: &mut XmlReader<'_>,
    buf: &mut Vec<u8>,
    xml: &[u8],
    built: &mut Vec<(String, Option<Box<dyn DocumentObject>>)>,
    zip: &mut zip::ZipArchive<std::fs::File>,
    report: &mut LoadReport,
) -> Result<(), FcstdError> {
    loop {
        match next_event(reader, buf)? {
            Event::Start(e) if e.name().as_ref() == b"Object" => {
                let map = attrs_of(&e);
                let name = map.get("name").cloned().unwrap_or_default();
                if name.is_empty() {
                    report
                        .warnings
                        .push("<ObjectData> entry without a name — skipped".to_string());
                    skip_element(reader, buf)?;
                    continue;
                }
                match built.iter().position(|(n, _)| *n == name) {
                    Some(i) => {
                        let owner = built[i].0.clone();
                        if let Some(object) = built[i].1.as_mut() {
                            parse_object_properties(reader, buf, xml, object, &owner, zip, report)?;
                        }
                        // An object that failed to build was already reported
                        // (with its skip) in the <Objects> section.
                    }
                    None => {
                        report.warnings.push(format!(
                            "object '{name}': <ObjectData> entry has no matching <Objects> entry — ignored"
                        ));
                        skip_element(reader, buf)?;
                    }
                }
            }
            Event::Empty(_) => {}
            Event::End(e) if e.name().as_ref() == b"ObjectData" => return Ok(()),
            Event::Eof => {
                return Err(FcstdError::Xml(
                    "unterminated <ObjectData> element".to_string(),
                ));
            }
            _ => continue,
        }
    }
}

/// Consume one `<Object>`'s properties (called after its Start event;
/// returns at its End event). `xml` is the whole Document.xml source —
/// needed to extract `<Sketch>` fragments byte-exactly (they are object
/// data, not `<Property>` elements).
fn parse_object_properties(
    reader: &mut XmlReader<'_>,
    buf: &mut Vec<u8>,
    xml: &[u8],
    object: &mut Box<dyn DocumentObject>,
    owner: &str,
    zip: &mut zip::ZipArchive<std::fs::File>,
    report: &mut LoadReport,
) -> Result<(), FcstdError> {
    // FreeCAD 1.0's `Constraints` payload, held until the object is
    // fully parsed (the geometry — and its GeoId remap — may arrive
    // after the constraints, whatever the file's property order).
    let mut constraint_inner: Option<String> = None;
    // The geometry-index remap from a `Geometry` property; `None`
    // while no geometry property has been seen (the legacy `<Sketch>`
    // fragment path needs no remap — its indices are already ours).
    let mut geo_remap: Option<Vec<Option<u32>>> = None;
    loop {
        match next_event(reader, buf)? {
            Event::Start(e) if e.name().as_ref() == b"Property" => {
                let map = attrs_of(&e);
                let name = map.get("name").cloned().unwrap_or_default();
                let value = parse_property_value(reader, buf, xml, owner, &name, &map, report)?;
                match (name.is_empty(), value) {
                    (false, Some(PropValue::Value(v))) => {
                        object.properties_mut().insert(name, v);
                    }
                    (
                        false,
                        Some(PropValue::Sketch {
                            fragment,
                            geo_remap: remap,
                        }),
                    ) => {
                        // FreeCAD 1.0's `Geometry` property, already
                        // translated into our `<Sketch>` fragment form
                        // (with its GeoId remap).
                        store_sketch_fragment(object, &fragment, owner, report);
                        geo_remap = Some(remap);
                    }
                    (false, Some(PropValue::SketchConstraints(inner))) => {
                        constraint_inner = Some(inner);
                    }
                    (false, Some(PropValue::Shape(shape_ref))) => {
                        match load_shape_payload(zip, &shape_ref, owner, report)? {
                            Some(shape) => attach_shape(object, shape, owner, report),
                            // Geometry not restorable: keep the parameters
                            // and stay Touched so recompute regenerates
                            // what it can.
                            None => object.set_status(ObjectStatus::Touched),
                        }
                    }
                    (true, _) => {
                        report.skipped_properties.push(format!("{owner}.<unnamed>"));
                        report
                            .warnings
                            .push(format!("{owner}: property without a name — skipped"));
                    }
                    (false, None) => {}
                }
            }
            Event::Empty(e) if e.name().as_ref() == b"Property" => {
                let name = attrs_of(&e).get("name").cloned().unwrap_or_default();
                if name.is_empty() {
                    report.skipped_properties.push(format!("{owner}.<unnamed>"));
                } else {
                    report.skipped_properties.push(format!("{owner}.{name}"));
                }
                report
                    .warnings
                    .push(format!("{owner}: property '{name}' has no value — skipped"));
            }
            Event::Empty(e) if e.name().as_ref() == b"Expression" => {
                let map = attrs_of(&e);
                let path = map.get("path").cloned().unwrap_or_default();
                let expression = map.get("expression").cloned().unwrap_or_default();
                if path.is_empty() || expression.is_empty() {
                    report.warnings.push(format!(
                        "{owner}: <Expression> without path or text — skipped"
                    ));
                } else if let Some(expressions) = object.expressions_mut() {
                    expressions.insert(path, expression);
                }
            }
            // The sketch content of a `Sketcher::SketchObject`: a
            // `<Sketch>` fragment that is object data, not a `<Property>`
            // (our writer emits it as a child of `<Object>`, next to
            // `<Properties>`; the sketch parser is whitespace-tolerant).
            Event::Start(e) if e.name().as_ref() == b"Sketch" => {
                let inner = raw_inner_xml(xml, reader, buf, b"Sketch")?;
                store_sketch_fragment(object, &format!("<Sketch>{inner}</Sketch>"), owner, report);
            }
            Event::Empty(e) if e.name().as_ref() == b"Sketch" => {
                // A self-closing `<Sketch/>` is an empty sketch.
                store_sketch_fragment(object, "<Sketch/>", owner, report);
            }
            Event::End(e) if e.name().as_ref() == b"Object" => {
                apply_freecad_constraints(
                    object,
                    constraint_inner.as_deref(),
                    geo_remap.as_deref(),
                    owner,
                    report,
                );
                return Ok(());
            }
            Event::Eof => {
                return Err(FcstdError::Xml("unterminated <Object> element".to_string()));
            }
            // The enclosing <Properties> start/end and stray text.
            _ => continue,
        }
    }
}

/// Translate FreeCAD 1.0's `Part::PropertyGeometryList` payload (the
/// raw inner XML of a `<GeometryList>` element) into our `<Sketch>`
/// fragment form, so the existing parser + `SketchObject` restore path
/// applies it.
///
/// FreeCAD's per-geometry blocks:
///
/// ```xml
/// <Geometry type="Part::GeomLineSegment" id="0" migrated="1">
///     <GeoExtensions count="…">…</GeoExtensions>   (ignored)
///     <LineSegment StartX="…" StartY="…" StartZ="…"
///                  EndX="…" EndY="…" EndZ="…"/>
///     <Construction value="0"/>
/// </Geometry>
/// ```
///
/// `Circle` and `ArcOfCircle` carry `CenterX/Y/Z`, `NormalX/Y/Z`,
/// `Radius` (and `StartAngle`/`EndAngle` for arcs, CCW about the
/// normal). Only XY-plane geometry (`z ≈ 0`, normal `≈ +Z`) maps into
/// this kernel's 2D sketcher; construction geometry and other curve
/// types are skipped with per-element warnings — the honest-policy
/// record of what did not survive the trip.
fn freecad_geometrylist_to_fragment(
    inner: &str,
    owner: &str,
    report: &mut LoadReport,
) -> (String, Vec<Option<u32>>) {
    let mut fragment = String::from("<Sketch>\n");
    // geometry-index translation: FreeCAD's `i` → our index, `None` if skipped
    let mut remap: Vec<Option<u32>> = Vec::new();
    let mut i = 0usize;
    let mut index = 0usize;
    while let Some(start) = inner[i..].find("<Geometry ") {
        let start = i + start;
        let Some(len) = inner[start..].find("</Geometry>") else {
            break;
        };
        let block = &inner[start..start + len];
        i = start + len;

        let kind = attr_of_str(block, "type").unwrap_or_default();
        // The fragment index this geometry takes when it maps — computed
        // before any push (remap entries pair with their geometry).
        let ours = fragment.matches("<Geometry ").count() as u32;
        if block.contains("<Construction value=\"1\"/>") {
            report.warnings.push(format!(
                "{owner}: geometry #{index} ({kind}) is construction — skipped"
            ));
            remap.push(None);
            index += 1;
            continue;
        }
        match kind {
            "Part::GeomPoint" => match (
                attr_of_f64(block, "X"),
                attr_of_f64(block, "Y"),
                attr_of_f64(block, "Z"),
            ) {
                (Some(x), Some(y), Some(z)) if z.abs() < 1e-9 => {
                    fragment.push_str(&format!(
                        "  <Geometry type=\"0\" x=\"{x:?}\" y=\"{y:?}\"/>\n"
                    ));
                    remap.push(Some(ours));
                }
                _ => report.warnings.push(format!(
                    "{owner}: geometry #{index} (point) off the XY plane or malformed — skipped"
                )),
            },
            "Part::GeomLineSegment" => {
                match (
                    attr_of_f64(block, "StartX"),
                    attr_of_f64(block, "StartY"),
                    attr_of_f64(block, "StartZ"),
                    attr_of_f64(block, "EndX"),
                    attr_of_f64(block, "EndY"),
                    attr_of_f64(block, "EndZ"),
                ) {
                    (Some(sx), Some(sy), Some(sz), Some(ex), Some(ey), Some(ez))
                        if sz.abs() < 1e-9 && ez.abs() < 1e-9 =>
                    {
                        fragment.push_str(&format!(
                            "  <Geometry type=\"1\" startx=\"{sx:?}\" starty=\"{sy:?}\" endx=\"{ex:?}\" endy=\"{ey:?}\"/>\n"
                        ));
                        remap.push(Some(ours));
                    }
                    _ => report.warnings.push(format!(
                        "{owner}: geometry #{index} (line) off the XY plane or malformed — skipped"
                    )),
                }
            }
            "Part::GeomCircle" | "Part::GeomArcOfCircle" => {
                let center_ok = matches!(
                    (
                        attr_of_f64(block, "CenterX"),
                        attr_of_f64(block, "CenterY"),
                        attr_of_f64(block, "CenterZ")
                    ),
                    (Some(_), Some(_), Some(cz)) if cz.abs() < 1e-9
                );
                let normal_ok = matches!(
                    (
                        attr_of_f64(block, "NormalX"),
                        attr_of_f64(block, "NormalY"),
                        attr_of_f64(block, "NormalZ")
                    ),
                    (Some(nx), Some(ny), Some(nz))
                        if nx.abs() < 1e-9 && ny.abs() < 1e-9 && (nz - 1.0).abs() < 1e-9
                );
                match (center_ok && normal_ok, attr_of_f64(block, "Radius"), kind) {
                    (true, Some(radius), "Part::GeomCircle") => {
                        let (cx, cy) = (
                            attr_of_f64(block, "CenterX").unwrap(),
                            attr_of_f64(block, "CenterY").unwrap(),
                        );
                        fragment.push_str(&format!(
                            "  <Geometry type=\"2\" centerx=\"{cx:?}\" centery=\"{cy:?}\" radius=\"{radius:?}\"/>\n"
                        ));
                        remap.push(Some(ours));
                    }
                    (true, Some(radius), _) => match (
                        attr_of_f64(block, "StartAngle"),
                        attr_of_f64(block, "EndAngle"),
                    ) {
                        (Some(start_angle), Some(end_angle)) => {
                            let (cx, cy) = (
                                attr_of_f64(block, "CenterX").unwrap(),
                                attr_of_f64(block, "CenterY").unwrap(),
                            );
                            fragment.push_str(&format!(
                                "  <Geometry type=\"3\" centerx=\"{cx:?}\" centery=\"{cy:?}\" radius=\"{radius:?}\" startangle=\"{start_angle:?}\" endangle=\"{end_angle:?}\"/>\n"
                            ));
                            remap.push(Some(ours));
                        }
                        _ => report.warnings.push(format!(
                            "{owner}: geometry #{index} (arc) malformed angles — skipped"
                        )),
                    },
                    _ => report.warnings.push(format!(
                        "{owner}: geometry #{index} ({kind}) off the XY plane or malformed — skipped"
                    )),
                }
            }
            other => {
                report.warnings.push(format!(
                    "{owner}: geometry #{index} type '{other}' not modeled — skipped"
                ));
            }
        }
        index += 1;
    }
    fragment.push_str("</Sketch>");
    (fragment, remap)
}

/// `key="value"` extraction on a raw XML block (first match, exact case).
fn attr_of_str<'a>(block: &'a str, key: &str) -> Option<&'a str> {
    let needle = format!("{key}=\"");
    let start = block.find(&needle)? + needle.len();
    let end = block[start..].find('"')? + start;
    Some(&block[start..end])
}

/// Apply FreeCAD 1.0's `Constraints` property payload (raw inner XML
/// of a `<ConstraintList>` element) to the object once the whole object
/// is parsed — the geometry (and its GeoId remap) is known then. Only a
/// `Sketcher::SketchObject` accepts constraints; anything else gets one
/// honest warning.
fn apply_freecad_constraints(
    object: &mut Box<dyn DocumentObject>,
    inner: Option<&str>,
    geo_remap: Option<&[Option<u32>]>,
    owner: &str,
    report: &mut LoadReport,
) {
    let Some(inner) = inner else { return };
    let lines = freecad_constraintlist_to_fragment_lines(inner, geo_remap, owner, report);
    if lines.is_empty() {
        return;
    }
    // The constraint lines are our fragment dialect; splicing them into
    // the object's already-restored geometry (its own `to_xml()`) makes
    // one complete sketch that the existing strict parser can check as
    // a whole — every constraint reference is resolved against the real
    // geometry list, exactly as `from_xml` does for our own files.
    if let Some(sketch_object) = object.as_any_mut().downcast_mut::<SketchObject>() {
        let body = sketch_object.sketch.to_xml();
        let Some(body) = body.strip_suffix("</Sketch>\n") else {
            report.warnings.push(format!(
                "{owner}: internal: sketch fragment without closing tag — constraints skipped"
            ));
            return;
        };
        let combined = format!("{body}{lines}</Sketch>\n");
        match fc_sketcher::xml::from_xml(&combined) {
            Ok(sk) => sketch_object.restore_sketch(sk),
            Err(err) => {
                report.warnings.push(format!(
                    "{owner}: malformed Constraints property: {err} — constraints skipped"
                ));
            }
        }
    } else {
        report.warnings.push(format!(
            "{owner}: Constraints property on a non-sketch object — ignored"
        ));
    }
}

/// Translate FreeCAD 1.0's `Constraints` payload (raw inner XML of a
/// `<ConstraintList>` element) into our `<Sketch>`-fragment `<Constraint
/// …/>` lines. FreeCAD's `<Constrain …/>` elements (the missing final
/// "t" is FreeCAD's own spelling) carry the FreeCAD `ConstraintType`
/// ordinals; the mappable subset translates onto this kernel's 13
/// kinds, everything else is skipped with a warning naming the FreeCAD
/// kind — the honest-policy record.
///
/// FreeCAD type → fragment type (both directions mirrored in
/// [`sketch_constraintlist_xml`]):
///
/// | FreeCAD (ordinal) | name           | ours (ordinal) |
/// |------------------|----------------|----------------|
/// | 1  | Coincident    | 0 (Coincident) |
/// | 2  | Horizontal    | 1 (Horizontal, edge form only) |
/// | 3  | Vertical      | 2 (Vertical, edge form only) |
/// | 4  | Parallel      | 3 (Parallel) |
/// | 6  | Distance      | 5 (Distance, two-point form) |
/// | 7  | DistanceX     | 6 (DistanceX) |
/// | 8  | DistanceY      | 7 (DistanceY) |
/// | 9  | Angle         | 11 (Angle, edge form, radians) |
/// | 10 | Perpendicular | 4 (Perpendicular, edge form) |
/// | 11 | Radius        | 8 (Radius) |
/// | 12 | Equal         | 10 (Equal) |
/// | 13 | PointOnObject | 12 (PointOnObject) |
/// | 18 | Diameter      | 9 (Diameter) |
///
/// Skipped with warnings: `Tangent` (5), `Symmetric` (14),
/// `InternalAlignment` (15), `SnellsLaw` (16), `Block` (17), `Weight`
/// (19), `Group` (20), `Text` (21) — no counterpart in this kernel's
/// solver vocabulary; axis/root-point/external references (`GeoId < 0`)
/// — no such geometries here; reference constraints (`IsDriving="0"`)
/// — measurements, not equations; the extended forms FreeCAD can express
/// but this solver cannot (two-point Horizontal/Vertical, point-to-line
/// Distance, angle-via-point) — skipped instead of silently
/// re-interpreted. GeoIds route through the geometry remap: skipped
/// geometry shifts every later index.
fn freecad_constraintlist_to_fragment_lines(
    inner: &str,
    geo_remap: Option<&[Option<u32>]>,
    owner: &str,
    report: &mut LoadReport,
) -> String {
    /// FreeCAD's own `ConstraintType` names, for honest warnings.
    fn freecad_kind(ty: i64) -> &'static str {
        match ty {
            0 => "None",
            1 => "Coincident",
            2 => "Horizontal",
            3 => "Vertical",
            4 => "Parallel",
            5 => "Tangent",
            6 => "Distance",
            7 => "DistanceX",
            8 => "DistanceY",
            9 => "Angle",
            10 => "Perpendicular",
            11 => "Radius",
            12 => "Equal",
            13 => "PointOnObject",
            14 => "Symmetric",
            15 => "InternalAlignment",
            16 => "SnellsLaw",
            17 => "Block",
            18 => "Diameter",
            19 => "Weight",
            20 => "Group",
            21 => "Text",
            _ => "unknown",
        }
    }

    let mut lines = String::new();
    let mut i = 0usize;
    let mut index = 0usize;
    while let Some(start) = inner[i..].find("<Constrain ") {
        let start = i + start;
        let Some(len) = inner[start..].find("/>") else {
            break;
        };
        let block = &inner[start..start + len + 2];
        i = start + len + 2;

        let skip = |report: &mut LoadReport, why: &str| {
            report.warnings.push(format!(
                "{owner}: constraint #{index} ({}): {why} — skipped",
                attr_of_str(block, "Type").unwrap_or("?")
            ));
        };

        // Reference constraints (`IsDriving="0"`) are measurements, not
        // equations — this solver has no reference vocabulary.
        if attr_of_str(block, "IsDriving") == Some("0") {
            skip(report, "reference (IsDriving=0)");
            index += 1;
            continue;
        }
        let Some(ty) = attr_of_str(block, "Type").and_then(|v| v.trim().parse::<i64>().ok()) else {
            skip(report, "missing or non-numeric Type");
            index += 1;
            continue;
        };
        // Our fragment ordinal for the FreeCAD kind.
        let frag_ty: u32 = match ty {
            1 => 0,
            2 => 1,
            3 => 2,
            4 => 3,
            10 => 4,
            6 => 5,
            7 => 6,
            8 => 7,
            11 => 8,
            18 => 9,
            12 => 10,
            9 => 11,
            13 => 12,
            other => {
                report.warnings.push(format!(
                    "{owner}: constraint #{index}: FreeCAD kind {} ({other}) has no counterpart in this kernel — skipped",
                    freecad_kind(other)
                ));
                index += 1;
                continue;
            }
        };
        let first = attr_of_str(block, "First").and_then(|v| v.trim().parse::<i64>().ok());
        let first_pos = attr_of_str(block, "FirstPos")
            .and_then(|v| v.trim().parse::<i64>().ok())
            .unwrap_or(0);
        let second = attr_of_str(block, "Second").and_then(|v| v.trim().parse::<i64>().ok());
        let second_pos = attr_of_str(block, "SecondPos")
            .and_then(|v| v.trim().parse::<i64>().ok())
            .unwrap_or(0);
        let third = attr_of_str(block, "Third")
            .and_then(|v| v.trim().parse::<i64>().ok())
            .unwrap_or(-2000);
        let value = attr_of_f64(block, "Value");

        // Form validation: which slots each of our kinds can express.
        // - edge-form kinds want pos 0 on both references;
        // - the two-point kinds want pos 1..=3 on both;
        // - PointOnObject wants a point first, an edge second.
        let form_ok = match frag_ty {
            0 | 5 | 6 | 7 => (1..=3).contains(&first_pos) && (1..=3).contains(&second_pos),
            1 | 2 | 3 | 4 | 10 | 11 => first_pos == 0 && second_pos == 0,
            8 | 9 => first_pos == 0,
            12 => (1..=3).contains(&first_pos) && second_pos == 0,
            _ => unreachable!("fragment ordinals cover our 13 kinds"),
        };
        if !form_ok {
            skip(
                report,
                "a form this kernel does not model (point slots on an edge-kind, or vice versa)",
            );
            index += 1;
            continue;
        }
        // A third reference means a form we do not carry (e.g. a
        // symmetry point) — only FreeCAD's GeoUndef (-2000) is expected.
        if third >= 0 {
            skip(report, "carries a third element reference");
            index += 1;
            continue;
        }

        // Map GeoIds through the geometry remap. Negative ids are the
        // axes / root point / external geometry — no such things here.
        let map_geo = |orig: i64| -> Option<u32> {
            if orig < 0 {
                return None;
            }
            match geo_remap {
                Some(m) => m.get(orig as usize).copied().flatten(),
                None => Some(orig as u32),
            }
        };
        let needs_second = !matches!(frag_ty, 1 | 2 | 8 | 9);
        let Some(first) = first.filter(|v| *v >= 0).and_then(map_geo) else {
            skip(
                report,
                "references geometry that is an axis, root point, external, or was skipped",
            );
            index += 1;
            continue;
        };
        let second_v = if needs_second {
            let Some(second) = second.filter(|v| *v >= 0).and_then(map_geo) else {
                skip(
                    report,
                    "references geometry that is an axis, root point, external, or was skipped",
                );
                index += 1;
                continue;
            };
            second
        } else {
            0
        };

        // Valued kinds require their datum.
        let valued = matches!(frag_ty, 5..=9 | 11);
        let Some(value) = (if valued { value } else { Some(0.0) }) else {
            skip(report, "missing Value for a datum constraint");
            index += 1;
            continue;
        };

        let line = match frag_ty {
            0 => format!(
                "  <Constraint type=\"0\" first=\"{first}\" firstpos=\"{first_pos}\" \
                 second=\"{second_v}\" secondpos=\"{second_pos}\"/>\n"
            ),
            1 => format!("  <Constraint type=\"1\" first=\"{first}\"/>\n"),
            2 => format!("  <Constraint type=\"2\" first=\"{first}\"/>\n"),
            3 => format!("  <Constraint type=\"3\" first=\"{first}\" second=\"{second_v}\"/>\n"),
            4 => format!("  <Constraint type=\"4\" first=\"{first}\" second=\"{second_v}\"/>\n"),
            5..=7 => {
                let t = frag_ty;
                format!(
                    "  <Constraint type=\"{t}\" first=\"{first}\" firstpos=\"{first_pos}\" \
                     second=\"{second_v}\" secondpos=\"{second_pos}\" value=\"{value:?}\"/>\n"
                )
            }
            8 => format!("  <Constraint type=\"8\" first=\"{first}\" value=\"{value:?}\"/>\n"),
            9 => format!("  <Constraint type=\"9\" first=\"{first}\" value=\"{value:?}\"/>\n"),
            10 => format!("  <Constraint type=\"10\" first=\"{first}\" second=\"{second_v}\"/>\n"),
            11 => format!(
                "  <Constraint type=\"11\" first=\"{first}\" second=\"{second_v}\" \
                 value=\"{value:?}\"/>\n"
            ),
            12 => format!(
                "  <Constraint type=\"12\" first=\"{first}\" firstpos=\"{first_pos}\" \
                 second=\"{second_v}\"/>\n"
            ),
            _ => unreachable!("frag_ty validated above"),
        };
        lines.push_str(&line);
        index += 1;
    }
    lines
}

fn attr_of_f64(block: &str, key: &str) -> Option<f64> {
    attr_of_str(block, key).and_then(|v| v.trim().parse::<f64>().ok())
}

/// Called right after a `<name>` Start event: consumes events up to the
/// matching `</name>` and returns the **raw source bytes** between the
/// two tags, so the fragment reaches the sketch parser exactly as it was
/// written (no event re-serialization, no attribute churn).
fn raw_inner_xml(
    xml: &[u8],
    reader: &mut XmlReader<'_>,
    buf: &mut Vec<u8>,
    name: &[u8],
) -> Result<String, FcstdError> {
    let start = reader.buffer_position() as usize;
    let mut depth = 1usize;
    loop {
        match next_event(reader, buf)? {
            Event::Start(e) if e.name().as_ref() == name => depth += 1,
            Event::End(e) if e.name().as_ref() == name => {
                depth -= 1;
                if depth == 0 {
                    // buffer_position() is past the closing tag; the tag's
                    // own '<' is the LAST '<' in the range, so cutting there
                    // returns exactly the bytes between the two tags.
                    let end = reader.buffer_position() as usize;
                    let range = &xml[start..end];
                    let tag_start = range.iter().rposition(|&b| b == b'<').ok_or_else(|| {
                        FcstdError::Xml("malformed element: closing tag not found".to_string())
                    })?;
                    return Ok(String::from_utf8_lossy(&range[..tag_start]).into_owned());
                }
            }
            Event::Eof => {
                return Err(FcstdError::Xml(
                    "unexpected end of Document.xml".to_string(),
                ));
            }
            _ => {}
        }
    }
}

/// Parse a `<Sketch>` fragment and store it into `object` when that
/// object is a `Sketcher::SketchObject`, via `restore_sketch` — the
/// loader's non-touching path; the file's persisted status is the truth
/// (a solved-and-saved sketch reloads as `Valid`, like every other
/// object kind; FreeCAD stores solved geometry and does not re-solve on
/// open). Anything else is a recoverable defect reported through the
/// warnings channel: a malformed fragment, or a `<Sketch>` element on a
/// non-sketch object. The loader reports + continues, mirroring how it
/// treats unreadable shape payloads.
fn store_sketch_fragment(
    object: &mut Box<dyn DocumentObject>,
    fragment: &str,
    owner: &str,
    report: &mut LoadReport,
) {
    match fc_sketcher::xml::from_xml(fragment) {
        Ok(sketch) => {
            let any = object.as_any_mut();
            if let Some(sketch_object) = any.downcast_mut::<SketchObject>() {
                // The loader's restore path: no touch — the file's
                // persisted status is the truth (a solved-and-saved
                // sketch reloads as Valid, like every other object kind).
                sketch_object.restore_sketch(sketch);
            } else {
                report.warnings.push(format!(
                    "{owner}: <Sketch> element on a non-sketch object — ignored"
                ));
            }
        }
        Err(err) => {
            report.warnings.push(format!(
                "{owner}: malformed <Sketch> fragment: {err} — sketch content skipped"
            ));
        }
    }
}

/// Parse one `<Property>`'s value element. Called after the Property Start
/// event; consumes through the Property End event and returns the value
/// (or `None` after recording a warning).
fn parse_property_value(
    reader: &mut XmlReader<'_>,
    buf: &mut Vec<u8>,
    xml: &[u8],
    owner: &str,
    prop: &str,
    prop_attrs: &BTreeMap<String, String>,
    report: &mut LoadReport,
) -> Result<Option<PropValue>, FcstdError> {
    let prop_type = prop_attrs.get("type").map_or("", String::as_str);
    loop {
        match next_event(reader, buf)? {
            // Self-closing value element, the normal case.
            Event::Empty(e) => {
                let value = parse_value_element(&e, None, &[], owner, prop, prop_type, report)?;
                return finish_property(reader, buf, value);
            }
            // Value element with content (e.g. FreeCAD's inline
            // `<Part brep="1">CASCADE Topology ...` payloads, or the
            // child-entry forms of the list properties). Direct child
            // elements are collected (name + attributes) so the value
            // parser can accept element forms (see `parse_value_element`).
            Event::Start(e) => {
                let element_name = e.name().as_ref().to_vec();
                // FreeCAD 1.0's sketch geometry: `Part::PropertyGeometryList`
                // serializes as a `<GeometryList>` element whose
                // `<Geometry type="Part::Geom…">` children each carry one
                // curve element two levels deep — the generic child
                // collector only sees names+attributes one level down, so
                // the raw XML is captured and translated instead.
                if element_name == b"GeometryList" && prop == "Geometry" {
                    let inner = raw_inner_xml(xml, reader, buf, b"GeometryList")?;
                    let (fragment, geo_remap) =
                        freecad_geometrylist_to_fragment(&inner, owner, report);
                    return finish_property(
                        reader,
                        buf,
                        Some(PropValue::Sketch {
                            fragment,
                            geo_remap,
                        }),
                    );
                }
                // FreeCAD 1.0's `Constraints` property
                // (`Sketcher::PropertyConstraintList`, on
                // `Sketcher::SketchObject`): the raw payload is held —
                // constraint references are resolved after the object's
                // geometry is known, whatever the property order.
                if element_name == b"ConstraintList" {
                    let inner = raw_inner_xml(xml, reader, buf, b"ConstraintList")?;
                    return finish_property(reader, buf, Some(PropValue::SketchConstraints(inner)));
                }
                if element_name == b"GeometryList" {
                    // `ExternalGeo` (the sketch's datum axes): the generic
                    // path would warn per-element; one honest note.
                    let _ = raw_inner_xml(xml, reader, buf, b"GeometryList")?;
                    report.warnings.push(format!(
                        "{owner}: property '{prop}': external geometry not modeled — skipped"
                    ));
                    report.skipped_properties.push(format!("{owner}.{prop}"));
                    return finish_property(reader, buf, None);
                }
                let mut text = Vec::new();
                let mut children: Vec<(String, BTreeMap<String, String>)> = Vec::new();
                loop {
                    match next_event(reader, buf)? {
                        Event::Text(t) => text.extend_from_slice(&t.into_inner()),
                        Event::Start(e) | Event::Empty(e) => {
                            children.push((
                                String::from_utf8_lossy(e.name().as_ref()).into_owned(),
                                attrs_of(&e),
                            ));
                        }
                        Event::End(end) if end.name().as_ref() == element_name.as_slice() => break,
                        Event::Eof => {
                            return Err(FcstdError::Xml(format!(
                                "unterminated <{}> value element",
                                String::from_utf8_lossy(&element_name)
                            )));
                        }
                        _ => continue,
                    }
                }
                let value = parse_value_element(
                    &e,
                    Some(&text),
                    &children,
                    owner,
                    prop,
                    prop_type,
                    report,
                )?;
                return finish_property(reader, buf, value);
            }
            Event::End(e) if e.name().as_ref() == b"Property" => {
                report.skipped_properties.push(format!("{owner}.{prop}"));
                report
                    .warnings
                    .push(format!("{owner}: property '{prop}' has no value — skipped"));
                return Ok(None);
            }
            Event::Eof => {
                return Err(FcstdError::Xml(
                    "unterminated <Property> element".to_string(),
                ));
            }
            _ => continue,
        }
    }
}

/// After the value element, drain up to the closing `</Property>`.
fn finish_property(
    reader: &mut XmlReader<'_>,
    buf: &mut Vec<u8>,
    value: Option<PropValue>,
) -> Result<Option<PropValue>, FcstdError> {
    loop {
        match next_event(reader, buf)? {
            Event::End(e) if e.name().as_ref() == b"Property" => return Ok(value),
            Event::Eof => {
                return Err(FcstdError::Xml(
                    "unterminated <Property> element".to_string(),
                ));
            }
            _ => continue,
        }
    }
}

/// Parse one value element (Bool/Integer/Float/String/PropertyVector/
/// PropertyPlacement/Link/LinkList/Part/StringList/FloatList/IntegerList/
/// PropertyColor/PropertyMatrix) by its element name, per the crate's
/// element table. `text` is the element's content when it was not
/// self-closing; `children` its direct child elements (name + attributes).
///
/// Type-sensitive dispatch: `prop_type` refines the element — quantity
/// types (`App::PropertyAngle`/`Length`/`Distance`) arrive as a Float
/// with an optional unit and are mapped to plain floats (recorded in the
/// report), and unknown value elements are recorded per-property in
/// `LoadReport::skipped_properties` (never fatal).
fn parse_value_element(
    e: &BytesStart<'_>,
    text: Option<&[u8]>,
    children: &[(String, BTreeMap<String, String>)],
    owner: &str,
    prop: &str,
    prop_type: &str,
    report: &mut LoadReport,
) -> Result<Option<PropValue>, FcstdError> {
    macro_rules! skip_warn {
        ($($arg:tt)*) => {{
            report.skipped_properties.push(format!("{owner}.{prop}"));
            report.warnings.push(format!($($arg)*));
            return Ok(None);
        }};
    }

    let element = String::from_utf8_lossy(e.name().as_ref()).into_owned();
    let map = attrs_of(e);
    match element.as_str() {
        "Bool" => match parse_bool(map.get("value").map_or("", String::as_str)) {
            Some(v) => Ok(Some(PropValue::Value(Property::Bool(v)))),
            None => skip_warn!(
                "{owner}: property '{prop}': malformed Bool value '{}' — skipped",
                map.get("value").map_or("?", String::as_str)
            ),
        },
        "Integer" => match map.get("value").and_then(|v| v.trim().parse::<i64>().ok()) {
            Some(v) => Ok(Some(PropValue::Value(Property::Integer(v)))),
            None => skip_warn!(
                "{owner}: property '{prop}': malformed Integer value '{}' — skipped",
                map.get("value").map_or("?", String::as_str)
            ),
        },
        // Quantity family (`App::PropertyAngle` / `PropertyLength` /
        // `PropertyDistance`): a float value with an optional unit
        // string attribute. Mapped to a plain float (the property
        // vocabulary has no quantity-carrying float); the fact it was
        // quantity-typed is recorded in the report, which is not a loss.
        "Float" if QUANTITY_TYPES.contains(&prop_type) => {
            match map.get("value").and_then(|v| v.trim().parse::<f64>().ok()) {
                Some(v) => {
                    let unit = map
                        .get("unit")
                        .or_else(|| prop_attrs_quant_unit(children))
                        .map_or("", String::as_str)
                        .trim()
                        .to_string();
                    report.quantity_properties.push(if unit.is_empty() {
                        format!("{owner}.{prop}")
                    } else {
                        format!("{owner}.{prop} (unit {unit})")
                    });
                    Ok(Some(PropValue::Value(Property::Float(v))))
                }
                None => skip_warn!(
                    "{owner}: property '{prop}': malformed {prop_type} value '{}' — skipped",
                    map.get("value").map_or("?", String::as_str)
                ),
            }
        }
        "Float" => match map.get("value").and_then(|v| v.trim().parse::<f64>().ok()) {
            Some(v) => Ok(Some(PropValue::Value(Property::Float(v)))),
            None => skip_warn!(
                "{owner}: property '{prop}': malformed Float value '{}' — skipped",
                map.get("value").map_or("?", String::as_str)
            ),
        },
        "String" => Ok(Some(PropValue::Value(Property::String(
            map.get("value").cloned().unwrap_or_default(),
        )))),
        // Attribute form (valueX/valueY/valueZ on the element itself) or
        // element form (the first child carrying the same attributes).
        "PropertyVector" => {
            let coords = vector_of_attrs(&map).or_else(|| {
                children
                    .iter()
                    .find(|(_, m)| vector_of_attrs(m).is_some())
                    .and_then(|(_, m)| vector_of_attrs(m))
            });
            match coords {
                Some((x, y, z)) => {
                    Ok(Some(PropValue::Value(Property::Vector(Vector3::new(x, y, z)))))
                }
                None => skip_warn!(
                    "{owner}: property '{prop}': malformed PropertyVector — skipped"
                ),
            }
        }
        "PropertyPlacement" => match parse_placement(&map) {
            Some(p) => Ok(Some(PropValue::Value(Property::Placement(p)))),
            None => {
                report.skipped_properties.push(format!("{owner}.{prop}"));
                report.warnings.push(format!(
                    "{owner}: property '{prop}': malformed PropertyPlacement — skipped"
                ));
                Ok(None)
            }
        },
        // FreeCAD writes single-element link lists as <Link .../>.
        "Link" => {
            let value = map.get("value").cloned().unwrap_or_default();
            let property = if prop_type == "App::PropertyLinkList" {
                Property::LinkList(vec![value])
            } else {
                Property::Link(value)
            };
            Ok(Some(PropValue::Value(property)))
        }
        // `App::PropertyLinkSub` — an object link plus a list of
        // subelement names (faces/edges). FreeCAD writes it for
        // `PartDesign` profiles (`<LinkSub value="Sketch" count="0"/>`)
        // and every "link to a feature of an object" property. The
        // subelements carry no geometry semantics in this kernel, so
        // the link is read as a plain `Link` and subelement names are
        // recorded as a warning when present.
        "LinkSub" => {
            let value = map.get("value").cloned().unwrap_or_default();
            let count = map
                .get("count")
                .and_then(|c| c.trim().parse::<usize>().ok())
                .unwrap_or(0);
            if count > 0 {
                report.warnings.push(format!(
                    "{owner}: property '{prop}': LinkSub subelements dropped ({count})"
                ));
            }
            Ok(Some(PropValue::Value(Property::Link(value))))
        }
        "LinkList" => {
            // FreeCAD's form: `<LinkList count="N"><Link value="…"/>…
            // </LinkList>` (nested child elements); ours (legacy) is a
            // `value="a,b"` attribute. Read both, children first.
            let mut list: Vec<String> = children
                .iter()
                .filter(|(name, _)| name == "Link")
                .filter_map(|(_, attrs)| attrs.get("value").cloned())
                .filter(|v| !v.is_empty())
                .collect();
            if list.is_empty() {
                list = map
                    .get("value")
                    .map(|v| {
                        v.split(',')
                            .map(str::trim)
                            .filter(|s| !s.is_empty())
                            .map(String::from)
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
            }
            Ok(Some(PropValue::Value(Property::LinkList(list))))
        }
        // Packed-uint color (the work order form: `value="16711680"` is
        // 0xFF0000). We have no color type: stored as the documented
        // string "#RRGGBB" (uppercase hex; bits above the low 24 — an
        // alpha channel in a 32-bit packed form — are not representable
        // and dropped).
        "PropertyColor" | "Color" => {
            match map.get("value").and_then(|v| v.trim().parse::<u64>().ok()) {
                Some(v) => Ok(Some(PropValue::Value(Property::String(format!(
                    "#{:06X}",
                    v & 0xFF_FF_FF
                ))))),
                None => skip_warn!(
                    "{owner}: property '{prop}': malformed packed color '{}' — skipped",
                    map.get("value").map_or("?", String::as_str)
                ),
            }
        }
        // String list. Accepted forms: a `value` attribute or inner text
        // payload split on whitespace (the work order form), plus
        // repeated `<String value="..."/>` child entries. Canonical
        // storage: entries joined with single spaces, as a plain string
        // property (entries cannot contain whitespace by construction).
        "StringList" => {
            let mut entries = list_payload_strings(&map, text);
            for (name, attrs) in children {
                if name == "String" {
                    if let Some(v) = attrs.get("value") {
                        entries.push(v.clone());
                    }
                }
            }
            Ok(Some(PropValue::Value(Property::String(entries.join(" ")))))
        }
        // Numeric lists. Accepted forms: `value` attribute or inner text
        // payload split on whitespace, plus `<Float|Integer value=".."/>`
        // child entries. Canonical storage: single-space-joined numbers
        // as a plain string property. One malformed entry skips the whole
        // property (with a per-property record), never the load.
        "FloatList" => {
            let mut items = list_payload_strings(&map, text);
            for (name, attrs) in children {
                if name == "Float" {
                    if let Some(v) = attrs.get("value") {
                        items.push(v.clone());
                    }
                }
            }
            let mut values = Vec::with_capacity(items.len());
            for item in &items {
                match item.trim().parse::<f64>() {
                    Ok(v) => values.push(v),
                    Err(_) => skip_warn!(
                        "{owner}: property '{prop}': malformed FloatList entry '{item}' — skipped"
                    ),
                }
            }
            Ok(Some(PropValue::Value(Property::String(
                numbers_text(&values),
            ))))
        }
        "IntegerList" => {
            let mut items = list_payload_strings(&map, text);
            for (name, attrs) in children {
                if name == "Integer" {
                    if let Some(v) = attrs.get("value") {
                        items.push(v.clone());
                    }
                }
            }
            let mut values = Vec::with_capacity(items.len());
            for item in &items {
                match item.trim().parse::<i64>() {
                    Ok(v) => values.push(v),
                    Err(_) => skip_warn!(
                        "{owner}: property '{prop}': malformed IntegerList entry '{item}' — skipped"
                    ),
                }
            }
            Ok(Some(PropValue::Value(Property::String(
                numbers_text(&values),
            ))))
        }
        // 16-float matrix, attributes a11..a44 (row-major) on the element
        // itself or on its first child carrying them. Canonical storage:
        // the 16 floats row-major, single-space joined, as a plain string
        // property. All 16 are required; a missing one skips the property.
        "PropertyMatrix" | "Matrix" => {
            let source = if MATRIX_KEYS.iter().all(|k| map.contains_key(*k)) {
                Some(&map)
            } else {
                children
                    .iter()
                    .find(|(_, m)| MATRIX_KEYS.iter().all(|k| m.contains_key(*k)))
                    .map(|(_, m)| m)
            };
            let Some(m) = source else {
                skip_warn!("{owner}: property '{prop}': malformed PropertyMatrix — skipped");
            };
            let mut values = Vec::with_capacity(16);
            for key in MATRIX_KEYS {
                match attr_f64(m, key) {
                    Some(v) => values.push(v),
                    None => skip_warn!(
                        "{owner}: property '{prop}': malformed PropertyMatrix (bad '{key}') — skipped"
                    ),
                }
            }
            Ok(Some(PropValue::Value(Property::String(
                numbers_text(&values),
            ))))
        }
        "Part" => Ok(Some(PropValue::Shape(ShapeRef {
            file: map.get("file").filter(|f| !f.is_empty()).cloned(),
            inline: text.unwrap_or_default().to_vec(),
        }))),
        _ => skip_warn!(
            "{owner}: property '{prop}': unsupported value element <{element}> (type '{prop_type}') — skipped"
        ),
    }
}

/// `(x, y, z)` from `valueX`/`valueY`/`valueZ` attributes, all required.
fn vector_of_attrs(map: &BTreeMap<String, String>) -> Option<(f64, f64, f64)> {
    match (
        attr_f64(map, "valueX"),
        attr_f64(map, "valueY"),
        attr_f64(map, "valueZ"),
    ) {
        (Some(x), Some(y), Some(z)) => Some((x, y, z)),
        _ => None,
    }
}

/// Row-major attribute keys of a `<PropertyMatrix a11..a44/>`.
const MATRIX_KEYS: [&str; 16] = [
    "a11", "a12", "a13", "a14", "a21", "a22", "a23", "a24", "a31", "a32", "a33", "a34", "a41",
    "a42", "a43", "a44",
];

/// The payload of a list property: its `value` attribute, or — when the
/// element carried content — its inner text. Whitespace-separated.
fn list_payload_strings(map: &BTreeMap<String, String>, text: Option<&[u8]>) -> Vec<String> {
    let from_attr = map.get("value").map_or("", String::as_str);
    if !from_attr.trim().is_empty() {
        return from_attr.split_whitespace().map(String::from).collect();
    }
    match text {
        Some(t) => String::from_utf8_lossy(t)
            .split_whitespace()
            .map(String::from)
            .collect(),
        None => Vec::new(),
    }
}

/// Canonical text of a numeric list: shortest round-trip forms,
/// single-space joined (the documented storage for list properties).
fn numbers_text<T: std::fmt::Display>(values: &[T]) -> String {
    values
        .iter()
        .map(|v| format!("{v}"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// The unit attribute is documented on the value element; accept it on a
/// child element too (defensive breadth for files that wrap the float).
fn prop_attrs_quant_unit(children: &[(String, BTreeMap<String, String>)]) -> Option<&String> {
    children
        .iter()
        .find(|(_, m)| m.contains_key("unit"))
        .and_then(|(_, m)| m.get("unit"))
}

/// `<PropertyPlacement>`: position + quaternion (Q0..Q3, scalar last).
/// FreeCAD also writes the equivalent A/Ox/Oy/Oz axis-angle form; use it
/// only when the quaternion is absent.
fn parse_placement(map: &BTreeMap<String, String>) -> Option<Placement> {
    let position = Vector3::new(
        attr_f64(map, "Px").unwrap_or(0.0),
        attr_f64(map, "Py").unwrap_or(0.0),
        attr_f64(map, "Pz").unwrap_or(0.0),
    );
    let rotation = if ["Q0", "Q1", "Q2", "Q3"]
        .iter()
        .any(|k| map.contains_key(*k))
    {
        normalized_rotation(Rotation {
            x: attr_f64(map, "Q0").unwrap_or(0.0),
            y: attr_f64(map, "Q1").unwrap_or(0.0),
            z: attr_f64(map, "Q2").unwrap_or(0.0),
            w: attr_f64(map, "Q3").unwrap_or(1.0),
        })
    } else if let (Some(a), Some(ox), Some(oy), Some(oz)) = (
        attr_f64(map, "A"),
        attr_f64(map, "Ox"),
        attr_f64(map, "Oy"),
        attr_f64(map, "Oz"),
    ) {
        // A is radians in FreeCAD's format.
        Rotation::from_axis_angle(Vector3::new(ox, oy, oz), a).ok()?
    } else {
        Rotation::IDENTITY
    };
    Some(Placement { position, rotation })
}

/// Re-normalize a quaternion read from a file (FreeCAD normalizes on
/// restore; near-unit inputs are unchanged).
fn normalized_rotation(r: Rotation) -> Rotation {
    let len = (r.x * r.x + r.y * r.y + r.z * r.z + r.w * r.w).sqrt();
    if len > 0.0 && (len - 1.0).abs() > 1e-12 {
        Rotation {
            x: r.x / len,
            y: r.y / len,
            z: r.z / len,
            w: r.w / len,
        }
    } else {
        r
    }
}

/// Resolve a `<Part>` reference into a shape: read the side file from the
/// zip (or the inline payload), probe its format and parse ours.
fn load_shape_payload(
    zip: &mut zip::ZipArchive<std::fs::File>,
    shape_ref: &ShapeRef,
    owner: &str,
    report: &mut LoadReport,
) -> Result<Option<TopoShape>, FcstdError> {
    let bytes: Vec<u8> = if let Some(file) = &shape_ref.file {
        match zip.by_name(file) {
            Ok(mut entry) => {
                let mut bytes = Vec::new();
                entry.read_to_end(&mut bytes)?;
                bytes
            }
            Err(zip::result::ZipError::FileNotFound) => {
                report.warnings.push(format!(
                    "{owner}: shape file '{file}' missing from archive — geometry skipped (parameters kept)"
                ));
                return Ok(None);
            }
            Err(e) => return Err(zip_err(e)),
        }
    } else if !shape_ref.inline.is_empty() {
        shape_ref.inline.clone()
    } else {
        // An empty <Part/> is FreeCAD's marker for "no computed shape".
        // Returning None leaves the object Touched (uniform with the other
        // no-geometry paths), so recompute can regenerate it.
        return Ok(None);
    };

    match brep::probe_format(&bytes) {
        BrpFormat::FcBrep => match brep::read_shape_bytes(&bytes) {
            Ok(shape) => Ok(Some(shape)),
            Err(e) => {
                report.warnings.push(format!(
                    "{owner}: malformed FCBREP payload: {e} — geometry skipped (parameters kept)"
                ));
                Ok(None)
            }
        },
        BrpFormat::OcctBrep => {
            report.warnings.push(format!(
                "{owner}: OCCT BRep shape not supported — geometry skipped (parameters kept)"
            ));
            Ok(None)
        }
        BrpFormat::Unknown => {
            report.warnings.push(format!(
                "{owner}: unknown shape payload format — geometry skipped (parameters kept)"
            ));
            Ok(None)
        }
    }
}

/// Attach a parsed shape to the concrete feature owning it.
fn attach_shape(
    object: &mut Box<dyn DocumentObject>,
    shape: TopoShape,
    owner: &str,
    report: &mut LoadReport,
) {
    let any = object.as_any_mut();
    if let Some(f) = any.downcast_mut::<BoxFeature>() {
        f.shape = Some(shape);
    } else if let Some(f) = any.downcast_mut::<SphereFeature>() {
        f.shape = Some(shape);
    } else if let Some(f) = any.downcast_mut::<CylinderFeature>() {
        f.shape = Some(shape);
    } else if let Some(f) = any.downcast_mut::<ConeFeature>() {
        f.shape = Some(shape);
    } else if let Some(f) = any.downcast_mut::<MeshFeature>() {
        f.shape = Some(shape);
    } else if let Some(f) = any.downcast_mut::<TorusFeature>() {
        f.shape = Some(shape);
    } else if let Some(f) = any.downcast_mut::<TransformFeature>() {
        // Transform features persist their computed mesh.
        f.result = match shape {
            TopoShape::Mesh(mesh) => Some(mesh),
            other => Some(other.tessellate(DEFAULT_DEFLECTION)),
        };
    } else if let Some(f) = any.downcast_mut::<BooleanFeature>() {
        // Boolean features persist their computed mesh.
        f.result = match shape {
            TopoShape::Mesh(mesh) => Some(mesh),
            other => Some(other.tessellate(DEFAULT_DEFLECTION)),
        };
    } else if let Some(f) = any.downcast_mut::<MultiFuseFeature>() {
        // Multi-fuse features persist their computed mesh.
        f.result = match shape {
            TopoShape::Mesh(mesh) => Some(mesh),
            other => Some(other.tessellate(DEFAULT_DEFLECTION)),
        };
    } else if let Some(f) = any.downcast_mut::<MirroringFeature>() {
        // Mirror features persist their computed mesh.
        f.result = match shape {
            TopoShape::Mesh(mesh) => Some(mesh),
            other => Some(other.tessellate(DEFAULT_DEFLECTION)),
        };
    } else if let Some(f) = any.downcast_mut::<PadFeature>() {
        // PartDesign features persist their computed mesh.
        f.result = match shape {
            TopoShape::Mesh(mesh) => Some(mesh),
            other => Some(other.tessellate(DEFAULT_DEFLECTION)),
        };
    } else if let Some(f) = any.downcast_mut::<PocketFeature>() {
        f.result = match shape {
            TopoShape::Mesh(mesh) => Some(mesh),
            other => Some(other.tessellate(DEFAULT_DEFLECTION)),
        };
    } else if let Some(f) = any.downcast_mut::<RevolutionFeature>() {
        f.result = match shape {
            TopoShape::Mesh(mesh) => Some(mesh),
            other => Some(other.tessellate(DEFAULT_DEFLECTION)),
        };
    } else {
        report.warnings.push(format!(
            "{owner}: object type carries no shape field — geometry skipped"
        ));
    }
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// Escape a string for an XML attribute body.
pub(crate) fn escape_attr(s: &str) -> String {
    quick_xml::escape::escape(s).into_owned()
}

/// Map a FreeCAD type id (e.g. `Part::Box`) to a fresh object instance.
/// Returns `None` for unknown types — except that unknown `Part::`
/// family members are handled by the caller as parameter containers
/// (see `parse_objects_section`).
pub(crate) fn build_object(type_id: &str, name: &str) -> Option<Box<dyn fc_app::DocumentObject>> {
    match type_id {
        "Part::Box" => Some(Box::new(BoxFeature::new(name))),
        "Part::Sphere" => Some(Box::new(SphereFeature::new(name))),
        "Part::Cylinder" => Some(Box::new(CylinderFeature::new(name))),
        "Part::Cone" => Some(Box::new(ConeFeature::new(name))),
        "Part::Torus" => Some(Box::new(TorusFeature::new(name))),
        "Part::Transform" => Some(Box::new(TransformFeature::new(name))),
        "Part::Cut" | "Part::Fuse" | "Part::Common" => BooleanKind::from_type_name(type_id)
            .map(|kind| Box::new(BooleanFeature::new(name, kind)) as _),
        "Part::MultiFuse" => Some(Box::new(MultiFuseFeature::new(name))),
        "Part::Mirroring" => Some(Box::new(MirroringFeature::new(name))),
        "PartDesign::Pad" => Some(Box::new(PadFeature::new(name))),
        "PartDesign::Pocket" => Some(Box::new(PocketFeature::new(name))),
        "PartDesign::Revolution" => Some(Box::new(RevolutionFeature::new(name))),
        "Sketcher::SketchObject" => Some(Box::new(SketchObject::new(name))),
        // Container / link / origin types: no geometry semantics of our
        // own — loaded as plain property containers (see `LoadContainer`)
        // that list and re-save under their own type id. Their `Group` /
        // `Tip` / `LinkedObject` / `Role` / `Placement`/ parameters ride
        // in as ordinary properties; no attachment or grouping semantics
        // are derived from them (documented non-goal).
        "App::Part" => Some(Box::new(LoadContainer::new("App::Part", name))),
        "PartDesign::Body" => Some(Box::new(LoadContainer::new("PartDesign::Body", name))),
        "App::DocumentGroup" => Some(Box::new(LoadContainer::new("App::DocumentGroup", name))),
        "App::Link" => Some(Box::new(LoadContainer::new("App::Link", name))),
        "App::Origin" => Some(Box::new(LoadContainer::new("App::Origin", name))),
        "App::Plane" => Some(Box::new(LoadContainer::new("App::Plane", name))),
        "App::Line" => Some(Box::new(LoadContainer::new("App::Line", name))),
        "Part::Feature" => Some(Box::new(MeshFeature::new(name))),
        _ => None,
    }
}

/// Loader-side generic object: a named property container for FreeCAD
/// object types that carry no geometry semantics we model — the
/// container/link/origin family (`App::Part`, `PartDesign::Body`,
/// `App::DocumentGroup`, `App::Link`, `App::Origin`, `App::Plane`,
/// `App::Line`), the `Part::Feature` family root, and (via the caller)
/// unknown `Part::`-family members with their source type id kept in a
/// `TypeId` property.
///
/// It exists only on the load path: real FreeCAD files name far more
/// object types than this kernel builds, and the work order's policy is
/// to keep their readable parameters instead of dropping the objects.
/// `execute` is a no-op (these objects carry no executable semantics in
/// this kernel), so they survive recompute untouched and save back as
/// their container type.
pub(crate) struct LoadContainer {
    pub container: PropertyContainer,
    /// The static type id this object lists and re-saves as.
    kind: &'static str,
    expressions: BTreeMap<String, String>,
}

impl LoadContainer {
    fn new(kind: &'static str, name: &str) -> Self {
        Self {
            container: PropertyContainer::new(name),
            kind,
            expressions: BTreeMap::new(),
        }
    }
}

impl DocumentObject for LoadContainer {
    fn type_name(&self) -> &'static str {
        self.kind
    }
    fn name(&self) -> &str {
        self.container.name()
    }
    fn set_name(&mut self, name: &str) {
        self.container.set_name(name)
    }
    fn properties(&self) -> &BTreeMap<String, Property> {
        &self.container.properties
    }
    fn properties_mut(&mut self) -> &mut BTreeMap<String, Property> {
        &mut self.container.properties
    }
    fn expressions(&self) -> &BTreeMap<String, String> {
        &self.expressions
    }
    fn expressions_mut(&mut self) -> Option<&mut BTreeMap<String, String>> {
        Some(&mut self.expressions)
    }
    fn execute(&mut self, _doc: &Document) -> Result<(), String> {
        // Parameter containers have nothing to execute.
        Ok(())
    }
    fn status(&self) -> &ObjectStatus {
        self.container.status()
    }
    fn set_status(&mut self, status: ObjectStatus) {
        self.container.set_status(status)
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

// ---------------------------------------------------------------------------
// Tests: Sketcher::SketchObject persistence
// ---------------------------------------------------------------------------

#[cfg(test)]
mod sketch_persistence_tests {
    use super::*;
    use fc_part::features::TransformFeature;
    use fc_sketcher::constraint::Constraint;
    use fc_sketcher::geometry::{Geometry, Point};
    use fc_sketcher::xml::from_xml;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn temp_path(tag: &str) -> PathBuf {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        std::env::temp_dir().join(format!(
            "fcrs-woi-fcstd-{}-{}.fcstd",
            tag,
            COUNTER.fetch_add(1, Ordering::SeqCst)
        ))
    }

    /// Two lines sharing a corner, the corner pinned horizontal: 8
    /// parameters, 2 (coincident) + 1 (horizontal) = 3 constraint
    /// dimensions -> DOF 5.
    fn corner_sketch() -> fc_sketcher::Sketch {
        let mut sk = fc_sketcher::Sketch::new();
        let a = sk.add_geometry(Geometry::LineSegment {
            start: Point::new(0.0, 0.0),
            end: Point::new(40.5, 0.25),
        });
        let b = sk.add_geometry(Geometry::LineSegment {
            start: Point::new(40.5, 0.25),
            end: Point::new(40.25, 30.75),
        });
        sk.add_constraint(Constraint::Coincident {
            g1: a,
            pos1: fc_sketcher::PointPos::End,
            g2: b,
            pos2: fc_sketcher::PointPos::Start,
        })
        .unwrap();
        sk.add_constraint(Constraint::Horizontal { g: a }).unwrap();
        sk
    }

    /// A one-circle sketch (fresh content for import-style swaps).
    fn circle_sketch() -> fc_sketcher::Sketch {
        let mut sk = fc_sketcher::Sketch::new();
        sk.add_geometry(Geometry::Circle {
            center: Point::new(5.0, 6.0),
            radius: 2.5,
        });
        sk
    }

    fn sketch_of(doc: &Document, name: &str) -> fc_sketcher::Sketch {
        doc.get_by_name(name)
            .unwrap()
            .as_any()
            .downcast_ref::<SketchObject>()
            .unwrap()
            .sketch
            .clone()
    }

    fn read_document_xml(path: &Path) -> String {
        let file = std::fs::File::open(path).unwrap();
        let mut zip = zip::ZipArchive::new(file).unwrap();
        let mut entry = zip.by_name("Document.xml").unwrap();
        let mut text = String::new();
        entry.read_to_string(&mut text).unwrap();
        text
    }

    /// Write a `.FCStd` archive whose Document.xml is exactly `xml` (the
    /// craft-a-file style of the io integration tests).
    fn craft_fcstd(path: &Path, xml: &str) {
        let file = std::fs::File::create(path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        zip.start_file("Document.xml", options).unwrap();
        zip.write_all(xml.as_bytes()).unwrap();
        zip.finish().unwrap();
    }

    /// Does `object` carry a Link/LinkList property pointing at `target`?
    fn links_to(object: &dyn DocumentObject, target: &str) -> bool {
        object.properties().values().any(|value| match value {
            Property::Link(name) => name == target,
            Property::LinkList(names) => names.iter().any(|name| name == target),
            _ => false,
        })
    }

    #[test]
    fn sketch_object_round_trips_byte_identically() {
        let path = temp_path("roundtrip");

        let mut doc = Document::new("test");
        doc.add(Box::new(SketchObject::with_sketch(
            "Sketch",
            corner_sketch(),
        )));
        doc.recompute_all().unwrap();
        let original = sketch_of(&doc, "Sketch").to_xml();
        save_document(&doc, &path).unwrap();

        let (loaded, report) = load_document(&path).unwrap();
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        assert_eq!(report.objects, 1);
        assert_eq!(report.skipped, 0);

        let object = loaded.get_by_name("Sketch").unwrap();
        assert_eq!(object.type_name(), "Sketcher::SketchObject");
        let sketch_object = object.as_any().downcast_ref::<SketchObject>().unwrap();
        // The strongest round-trip oracle: identical `to_xml()` output.
        assert_eq!(sketch_object.sketch.to_xml(), original);
        // Solve results are not persisted (solve info re-derives on the
        // next recompute), but the STATUS is: a solved-and-saved sketch
        // reloads as Valid, like every other object kind (FC-6.1 fix).
        assert_eq!(sketch_object.solved, None);
        assert_eq!(*sketch_object.status(), ObjectStatus::Valid);
        assert_eq!(
            sketch_object.container.get_string("Label"),
            Some("Sketch".into())
        );
        assert_eq!(
            sketch_object.container.get_placement("Placement"),
            Some(Placement::IDENTITY)
        );

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn empty_sketch_round_trips() {
        let path = temp_path("empty");
        let mut doc = Document::new("test");
        doc.add(Box::new(SketchObject::new("Sketch")));
        doc.recompute_all().unwrap();
        save_document(&doc, &path).unwrap();

        let (loaded, report) = load_document(&path).unwrap();
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        let object = loaded.get_by_name("Sketch").unwrap();
        let sketch_object = object.as_any().downcast_ref::<SketchObject>().unwrap();
        assert_eq!(sketch_object.sketch.to_xml(), "<Sketch>\n</Sketch>\n");

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn document_xml_carries_the_sketch_as_freecad_native_properties() {
        let path = temp_path("embed");
        let mut doc = Document::new("test");
        doc.add(Box::new(SketchObject::with_sketch(
            "Sketch",
            corner_sketch(),
        )));
        doc.recompute_all().unwrap();
        save_document(&doc, &path).unwrap();

        let xml = read_document_xml(&path);
        // The geometry property: FreeCAD's `Part::PropertyGeometryList`
        // form — `<GeometryList>` wrapper, one `<Geometry type=… migrated="1">`
        // block per element, the kind's own child element carrying the
        // parameters.
        assert!(
            xml.contains("<Property name=\"Geometry\" type=\"Part::PropertyGeometryList\">"),
            "{xml}"
        );
        assert!(xml.contains("<GeometryList count=\""), "{xml}");
        assert!(
            xml.contains("<Geometry type=\"Part::GeomLineSegment\" migrated=\"1\">"),
            "{xml}"
        );
        assert!(xml.contains("<LineSegment StartX="), "{xml}");
        // The constraints property: FreeCAD's
        // `Sketcher::PropertyConstraintList` form — `<ConstraintList>`
        // wrapper, one `<Constrain …/>` per constraint (FreeCAD's own
        // spelling), with FreeCAD's `ConstraintType` ordinals
        // (Coincident=1, not our dialect's 0).
        assert!(
            xml.contains(
                "<Property name=\"Constraints\" type=\"Sketcher::PropertyConstraintList\">"
            ),
            "{xml}"
        );
        assert!(xml.contains("<ConstraintList count=\""), "{xml}");
        assert!(xml.contains("<Constrain Name=\"\" "), "{xml}");
        assert!(
            xml.contains("Type=\"1\"") && xml.contains("FirstPos=\""),
            "{xml}"
        );
        // Both live inside the object's <Properties> (they are
        // properties, exactly where FreeCAD's own writer puts them)…
        let object_section = xml.split("<Object name=\"Sketch\">").nth(1).unwrap();
        let properties_end = object_section.find("</Properties>").unwrap();
        let geometry_start = object_section.find("<Property name=\"Geometry\"").unwrap();
        assert!(geometry_start < properties_end, "{xml}");
        // …and the legacy `<Sketch>` object-data fragment is gone.
        assert!(!xml.contains("<Sketch>\n"), "{xml}");
        // The property count accounts for both.
        assert!(xml.contains("<Properties Count=\"4\">"), "{xml}");

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn sketch_and_box_survive_a_round_trip_together() {
        let path = temp_path("mixed");
        let mut doc = Document::new("test");
        doc.add(Box::new(BoxFeature::new("Base"))); // default 10x10x10
        doc.add(Box::new(SketchObject::with_sketch(
            "Sketch",
            corner_sketch(),
        )));
        doc.recompute_all().unwrap();
        // The solve may have adjusted the geometry; compare against the
        // sketch exactly as it was saved.
        let original = sketch_of(&doc, "Sketch").to_xml();
        save_document(&doc, &path).unwrap();

        let (loaded, report) = load_document(&path).unwrap();
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        assert_eq!(report.objects, 2);

        let object = loaded.get_by_name("Base").unwrap();
        assert_eq!(object.type_name(), "Part::Box");
        assert_eq!(
            object.properties().get("Length"),
            Some(&Property::Float(10.0))
        );
        // The writer omits Shape payloads for parametric features (real
        // FreeCAD can only restore OCCT BRep); the box reloads Touched
        // and the first recompute rebuilds it — FreeCAD's own state
        // for a touched feature.
        assert_eq!(*object.status(), ObjectStatus::Touched);

        let object = loaded.get_by_name("Sketch").unwrap();
        let sketch_object = object.as_any().downcast_ref::<SketchObject>().unwrap();
        assert_eq!(sketch_object.sketch.to_xml(), original);

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn malformed_sketch_fragment_warns_and_the_rest_loads() {
        let path = temp_path("malformed");
        craft_fcstd(
            &path,
            r##"<Document SchemaVersion="4" ProgramVersion="test" FileVersion="1">
 <Objects Count="2" Dependencies="1">
  <Object type="Part::Box" name="Box" id="1"/>
  <Object type="Sketcher::SketchObject" name="Sketch" id="2"/>
 </Objects>
 <ObjectData Count="2">
  <Object name="Box">
   <Properties Count="1">
    <Property name="Length" type="App::PropertyFloat">
     <Float value="3.5"/>
    </Property>
   </Properties>
  </Object>
  <Object name="Sketch">
   <Properties Count="1">
    <Property name="Label" type="App::PropertyString">
     <String value="Mysketch"/>
    </Property>
   </Properties>
   <Sketch>
    <Bogus/>
   </Sketch>
  </Object>
 </ObjectData>
</Document>
"##,
        );

        let (doc, report) = load_document(&path).unwrap();
        // The bad fragment is a warning, not an error: the loader reports
        // and continues.
        assert!(
            report
                .warnings
                .iter()
                .any(|w| w.contains("Sketch") && w.contains("malformed <Sketch> fragment")),
            "{:?}",
            report.warnings
        );
        assert_eq!(report.objects, 2);
        assert_eq!(report.skipped, 0);

        // The other object is fully intact.
        let object = doc.get_by_name("Box").unwrap();
        assert_eq!(
            object.properties().get("Length"),
            Some(&Property::Float(3.5))
        );

        // The sketch object exists with its label but no content.
        let object = doc.get_by_name("Sketch").unwrap();
        let sketch_object = object.as_any().downcast_ref::<SketchObject>().unwrap();
        assert_eq!(
            sketch_object.container.get_string("Label"),
            Some("Mysketch".into())
        );
        assert!(sketch_object.sketch.geometry().is_empty());

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn sketch_element_on_a_non_sketch_object_is_ignored_with_a_warning() {
        let path = temp_path("stray");
        craft_fcstd(
            &path,
            r##"<Document SchemaVersion="4" ProgramVersion="test" FileVersion="1">
 <Objects Count="1" Dependencies="1">
  <Object type="Part::Box" name="Box" id="1"/>
 </Objects>
 <ObjectData Count="1">
  <Object name="Box">
   <Properties Count="0">
   </Properties>
   <Sketch>
    <Geometry type="0" x="1" y="2"/>
   </Sketch>
  </Object>
 </ObjectData>
</Document>
"##,
        );

        let (doc, report) = load_document(&path).unwrap();
        assert!(
            report
                .warnings
                .iter()
                .any(|w| w.contains("Box") && w.contains("<Sketch> element on a non-sketch object")),
            "{:?}",
            report.warnings
        );
        assert_eq!(report.objects, 1);
        assert!(doc.get_by_name("Box").is_some());

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn load_hook_swaps_sketch_content_and_touches_dependents() {
        let path = temp_path("hook");

        // Sketch + a dependent that links to it (Part::Transform with
        // Source=Sketch). The transform cannot execute (a sketch has no
        // computed mesh), so it sits in Error — which is fine here: the
        // hook must touch it so the next recompute re-evaluates it.
        let mut doc = Document::new("test");
        doc.add(Box::new(SketchObject::with_sketch(
            "Sketch",
            corner_sketch(),
        )));
        let mut transform = TransformFeature::new("Dep");
        transform
            .container
            .set("Source", Property::Link("Sketch".to_string()));
        doc.add(Box::new(transform));
        let _ = doc.recompute_all(); // transform fails on purpose

        save_document(&doc, &path).unwrap();
        let original_status = doc.get_by_name("Dep").unwrap().status().clone();
        assert!(matches!(original_status, ObjectStatus::Error(_)));

        // Import-style hook: swap the sketch, touch everything linking to
        // it (this is exactly what `sketch-import` does).
        let fresh = circle_sketch();
        let fresh_xml = fresh.to_xml();
        let mut hook = |name: &str, object: &mut dyn DocumentObject| {
            if name == "Sketch" {
                object
                    .as_any_mut()
                    .downcast_mut::<SketchObject>()
                    .expect("target must be a SketchObject")
                    .replace_sketch(fresh.clone());
            } else if links_to(object, "Sketch") {
                object.set_status(ObjectStatus::Touched);
            }
        };
        let (loaded, report) = load_document_with_hook(&path, Some(&mut hook)).unwrap();
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);

        // The content swap took, byte-identically.
        assert_eq!(sketch_of(&loaded, "Sketch").to_xml(), fresh_xml);
        let object = loaded.get_by_name("Sketch").unwrap();
        assert_eq!(*object.status(), ObjectStatus::Touched);
        // The dependent was touched (its persisted status was Error).
        let object = loaded.get_by_name("Dep").unwrap();
        assert_eq!(*object.status(), ObjectStatus::Touched);

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn parsed_fragments_round_trip_through_the_parser_too() {
        // Save-side fragment -> from_xml -> to_xml is the identity, the
        // property the whole persistence story leans on.
        let sketch = corner_sketch();
        let xml = sketch.to_xml();
        let parsed = from_xml(&xml).unwrap();
        assert_eq!(parsed.to_xml(), xml);
        assert_eq!(parsed.dof(), sketch.dof());
    }
}
