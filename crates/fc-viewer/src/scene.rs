//! Scene assembly — the pure-CPU half of the viewer.
//!
//! [`build_scene`] loads a document with `fc-io`, recomputes it,
//! tessellates every object with the **same deflection rule the CLI
//! exports use** ([`EXPORT_DEFLECTION`] = 0.2 mm, exactly what
//! `freecad-rs`'s `placed_meshes`/`export svg|obj|stl` pass to
//! `fc_part::mesh_of`), applies object placements with the same
//! `Part::Transform` exemption, and produces flat-shaded triangle soups
//! with per-object deterministic colors and error flags. The GPU pass
//! consumes the result; every structural property (object list,
//! triangle counts, flags, colors) is testable without a GPU.
//!
//! One deliberate divergence from the exporters: a **failed recompute
//! does not fail the scene** (PLAN-APPLICATION M1, acceptance 4). The
//! CLI's exporters refuse to write output when `recompute_all` errors;
//! the viewer keeps going — objects in `Error` status keep their
//! last-good geometry and are flagged for a distinct red tint, so
//! half-computed documents still open and render.

use std::path::Path;

use fc_app::{Document, ObjectStatus, Property};
use fc_io::load_document;
use fc_part::mesh_of;

/// Tessellation deflection — the CLI exports' rule (see `placed_meshes`
/// in `freecad-rs/src/commands.rs`). The viewer does not fork it.
pub const EXPORT_DEFLECTION: f64 = 0.2;

/// Unlit color of `Error`-status objects: a distinct red tint (FreeCAD
/// marks broken features red in the tree; this is the render-side flag).
pub const ERROR_COLOR: [f32; 4] = [0.86, 0.12, 0.12, 1.0];

/// One drawable object: a flat-shaded triangle soup in world space.
#[derive(Debug, Clone)]
pub struct SceneObject {
    /// Internal document name (what `pick_object` reports).
    pub name: String,
    /// Unlit base color actually rendered: the deterministic per-object
    /// color, or [`ERROR_COLOR`] when flagged.
    pub color: [f32; 4],
    /// The deterministic color before error tinting (inspection/tests).
    pub base_color: [f32; 4],
    /// `true` when the object's recompute failed (rendered red).
    pub flagged: bool,
    /// World-space triangle soup: three positions per triangle.
    pub positions: Vec<[f64; 3]>,
    /// Face normal per soup vertex (flat shading).
    pub normals: Vec<[f64; 3]>,
}

impl SceneObject {
    pub fn triangle_count(&self) -> usize {
        self.positions.len() / 3
    }
}

/// The assembled scene: everything the GPU pass (or a CPU test) needs.
#[derive(Debug, Clone, Default)]
pub struct Scene {
    pub objects: Vec<SceneObject>,
    /// Bounding-sphere center (framing target).
    pub center: [f64; 3],
    /// Bounding-sphere radius (bbox diagonal / 2 — a safe upper bound).
    pub radius: f64,
    /// The recompute failure, if the document had one (recorded, not
    /// fatal — see the module docs).
    pub recompute_error: Option<String>,
}

impl Scene {
    pub fn triangle_count(&self) -> usize {
        self.objects.iter().map(SceneObject::triangle_count).sum()
    }

    pub fn object_names(&self) -> Vec<&str> {
        self.objects.iter().map(|o| o.name.as_str()).collect()
    }
}

/// Assemble the render scene from a `.FCStd` document.
///
/// Mirrors the CLI exporters' rules (`load` + `placed_meshes` in
/// `commands.rs`): warnings are surfaced on stderr, loaded history is
/// cleared, meshes come from `mesh_of(obj, EXPORT_DEFLECTION)`, and the
/// `Placement` property is applied to everything except
/// `Part::Transform` (which bakes its own placement into the mesh).
pub fn build_scene(file: &str) -> Result<Scene, String> {
    let path = Path::new(file);
    if !path.exists() {
        return Err(format!("file not found: '{file}'"));
    }
    let (mut doc, report) =
        load_document(path).map_err(|e| format!("cannot load '{file}': {e}"))?;
    for warning in &report.warnings {
        eprintln!("warning: {warning}");
    }
    // A freshly loaded document carries no undoable history (same as
    // the CLI's `load` helper).
    doc.clear_history();
    // The exporters refuse to render on a failed recompute; the viewer
    // deliberately does not — M1 acceptance 4. Record and continue.
    let recompute_error = doc
        .recompute_all()
        .err()
        .map(|e| format!("recompute failed: {e}"));

    let mut scene = Scene {
        recompute_error,
        ..Scene::default()
    };
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for summary in doc.summary() {
        let Some(obj) = doc.get(summary.id) else {
            continue;
        };
        let Some(mut mesh) = mesh_of(obj, EXPORT_DEFLECTION) else {
            continue; // no computed geometry (never-computed or skipped)
        };
        // Primitives keep their shape at the origin: their `Placement`
        // is applied here, at render time. Part::Transform features
        // already bake their Placement (and Scale) into the computed
        // mesh — re-applying would double the transform. (Same rule as
        // `placed_meshes`.)
        if obj.type_name() != "Part::Transform" {
            if let Some(Property::Placement(p)) = obj.properties().get("Placement") {
                mesh.transform(&p.to_matrix());
            }
        }
        let flagged = matches!(obj.status(), ObjectStatus::Error(_));
        let base_color = object_color(&summary.name);
        let mut object = SceneObject {
            name: summary.name,
            color: if flagged { ERROR_COLOR } else { base_color },
            base_color,
            flagged,
            positions: Vec::with_capacity(mesh.triangles.len() * 3),
            normals: Vec::with_capacity(mesh.triangles.len() * 3),
        };
        for t in &mesh.triangles {
            let (ia, ib, ic) = (t[0] as usize, t[1] as usize, t[2] as usize);
            if ia >= mesh.vertices.len() || ib >= mesh.vertices.len() || ic >= mesh.vertices.len() {
                continue; // malformed index; skip defensively (svg.rs does too)
            }
            let a = mesh.vertices[ia];
            let b = mesh.vertices[ib];
            let c = mesh.vertices[ic];
            // Flat face normal; degenerate slivers are skipped (same
            // |n| <= 1e-12 rule as the SVG renderer).
            let n = [
                (b.y - a.y) * (c.z - a.z) - (b.z - a.z) * (c.y - a.y),
                (b.z - a.z) * (c.x - a.x) - (b.x - a.x) * (c.z - a.z),
                (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x),
            ];
            if n[0] * n[0] + n[1] * n[1] + n[2] * n[2] <= 1e-24 {
                continue;
            }
            let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            let normal = [n[0] / len, n[1] / len, n[2] / len];
            for v in [a, b, c] {
                let position = [v.x, v.y, v.z];
                for i in 0..3 {
                    lo[i] = lo[i].min(position[i]);
                    hi[i] = hi[i].max(position[i]);
                }
                object.positions.push(position);
                object.normals.push(normal);
            }
        }
        if !object.positions.is_empty() {
            scene.objects.push(object);
        }
    }

    if scene.objects.is_empty() {
        scene.center = [0.0; 3];
        scene.radius = 0.0;
    } else {
        for i in 0..3 {
            scene.center[i] = (lo[i] + hi[i]) * 0.5;
        }
        let diagonal = (0..3).map(|i| (hi[i] - lo[i]).powi(2)).sum::<f64>().sqrt();
        scene.radius = diagonal * 0.5;
    }
    Ok(scene)
}

/// Deterministic per-object display color: FNV-1a hash of the internal
/// name mapped onto the hue wheel at fixed saturation/value. Determinism
/// across runs and machines is the point (std's randomized hashers are
/// deliberately not used).
pub fn object_color(name: &str) -> [f32; 4] {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in name.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    let hue = (hash % 360) as f64;
    let (r, g, b) = hsv_to_rgb(hue, 0.55, 0.88);
    [r, g, b, 1.0]
}

/// HSV → RGB (h in degrees, s/v in `[0, 1]`), plain f64 arithmetic.
fn hsv_to_rgb(h: f64, s: f64, v: f64) -> (f32, f32, f32) {
    let c = v * s;
    let x = c * (1.0 - ((h % 60.0) / 60.0 - 1.0).abs());
    let m = v - c;
    let (r, g, b) = match (h / 60.0).floor() as i32 % 6 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    ((r + m) as f32, (g + m) as f32, (b + m) as f32)
}

// Keep the unused-import warning away when `Document` is only used in
// doc position — it *is* used (clear_history), but the import is easy
// to trip over in refactors.
#[allow(dead_code)]
fn _document_type_witness(_: &Document) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors_are_deterministic_and_distinct_for_the_demo_names() {
        let names = ["Base", "Ball", "Stem", "Lift"];
        let colors: Vec<[f32; 4]> = names.iter().map(|n| object_color(n)).collect();
        for color in &colors {
            assert_eq!(color[3], 1.0, "opaque");
        }
        for i in 0..colors.len() {
            for j in i + 1..colors.len() {
                assert_ne!(
                    colors[i], colors[j],
                    "{} and {} collide",
                    names[i], names[j]
                );
            }
        }
        // Stable across calls (hash stability).
        assert_eq!(object_color("Base"), object_color("Base"));
    }

    #[test]
    fn hsv_boundary_values() {
        assert_eq!(hsv_to_rgb(0.0, 0.0, 1.0), (1.0, 1.0, 1.0));
        assert_eq!(hsv_to_rgb(0.0, 1.0, 0.5), (0.5, 0.0, 0.0));
    }
}
