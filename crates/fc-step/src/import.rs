//! STEP import — the geometry interpreter over the parsed entity table.
//!
//! The supported subset (ADR-0002 §5's "fixture class", honestly
//! enforced — anything else is an error, never a silent drop):
//!
//! - **Faceted solids**: `MANIFOLD_SOLID_BREP` of `CLOSED_SHELL` of
//!   `ADVANCED_FACE`s bounded by `POLY_LOOP`s on `PLANE`s (our own
//!   exports, and most tessellation STEP in the wild).
//! - **Exact planar faces**: `ADVANCED_FACE` on a `PLANE`, bounded by
//!   `EDGE_LOOP`s of `LINE` and `CIRCLE` edges (box-with-hole class).
//! - **Cylindrical faces**: `ADVANCED_FACE` on a
//!   `CYLINDRICAL_SURFACE`, full-turn bands (through holes) or partial
//!   turns unwrapped into the plane and ear-clipped.
//!
//! Curved geometry is tessellated under the caller's deflection with
//! the kernel's sagitta discipline (chord error ≤ deflection), and
//! every tolerance in the tests derives from it.

use crate::parse::{EntitySet, Param};

use fc_base::Vector3;
use fc_part::profile::{triangulate_planar_region, Point2};
use fc_part::Mesh;

/// A 3D point decoded from a `CARTESIAN_POINT`.
fn point(set: &EntitySet, id: u64) -> Result<Vector3, String> {
    let e = set.entity(id, "CARTESIAN_POINT")?;
    let coords = e
        .params
        .get(1)
        .ok_or("CARTESIAN_POINT without coordinates")?;
    let list = coords.as_list()?;
    if list.len() < 3 {
        return Err(format!(
            "CARTESIAN_POINT #{id} has {} coordinates, expected 3",
            list.len()
        ));
    }
    Ok(Vector3::new(
        list[0].as_f64()?,
        list[1].as_f64()?,
        list[2].as_f64()?,
    ))
}

/// An `AXIS2_PLACEMENT_3D`: origin + orthonormal z (axis) and x
/// (ref direction) basis.
struct Placement {
    origin: Vector3,
    axis: Vector3,
    ref_dir: Vector3,
}

fn direction(set: &EntitySet, id: u64) -> Result<Vector3, String> {
    let e = set.entity(id, "DIRECTION")?;
    let coords = e
        .params
        .get(1)
        .ok_or_else(|| format!("DIRECTION #{id} without coordinates"))?;
    let list = coords.as_list()?;
    if list.len() < 3 {
        return Err(format!(
            "DIRECTION #{id} has {} coordinates, expected 3",
            list.len()
        ));
    }
    let d = Vector3::new(list[0].as_f64()?, list[1].as_f64()?, list[2].as_f64()?);
    let len = (d.x * d.x + d.y * d.y + d.z * d.z).sqrt();
    if len < 1e-12 {
        return Err(format!("DIRECTION #{id} is degenerate"));
    }
    Ok(Vector3::new(d.x / len, d.y / len, d.z / len))
}

fn placement(set: &EntitySet, id: u64) -> Result<Placement, String> {
    let e = set.entity(id, "AXIS2_PLACEMENT_3D")?;
    let origin = point(set, e.params[1].as_ref_id()?)?;
    let axis = direction(set, e.params[2].as_ref_id()?)?;
    // The ref direction is optional ($); default to any x not parallel
    // to the axis.
    let ref_dir = match e.params.get(3) {
        Some(Param::Ref(r)) => {
            let mut d = direction(set, *r)?;
            // Project out the axis component and re-normalize.
            let dot = d.x * axis.x + d.y * axis.y + d.z * axis.z;
            d = Vector3::new(d.x - dot * axis.x, d.y - dot * axis.y, d.z - dot * axis.z);
            let len = (d.x * d.x + d.y * d.y + d.z * d.z).sqrt();
            if len < 1e-12 {
                fallback_x(&axis)
            } else {
                Vector3::new(d.x / len, d.y / len, d.z / len)
            }
        }
        _ => fallback_x(&axis),
    };
    Ok(Placement {
        origin,
        axis,
        ref_dir,
    })
}

fn fallback_x(axis: &Vector3) -> Vector3 {
    let candidate = if axis.x.abs() < 0.9 {
        Vector3::X
    } else {
        Vector3::Y
    };
    let dot = candidate.x * axis.x + candidate.y * axis.y + candidate.z * axis.z;
    let d = Vector3::new(
        candidate.x - dot * axis.x,
        candidate.y - dot * axis.y,
        candidate.z - dot * axis.z,
    );
    let len = (d.x * d.x + d.y * d.y + d.z * d.z).sqrt();
    Vector3::new(d.x / len, d.y / len, d.z / len)
}

/// Triangle accumulator; the mesh's vertices are deduplicated by exact
/// coordinates (imported STEP coordinates are canonical enough for the
/// subset, and exact-match dedup cannot weld distinct points by
/// accident).
#[derive(Default)]
struct MeshBuilder {
    vertices: Vec<Vector3>,
    triangles: Vec<[u32; 3]>,
}

impl MeshBuilder {
    fn vertex(&mut self, p: Vector3) -> u32 {
        if let Some(i) = self.vertices.iter().position(|v| {
            (v.x - p.x).abs() < 1e-9 && (v.y - p.y).abs() < 1e-9 && (v.z - p.z).abs() < 1e-9
        }) {
            return i as u32;
        }
        self.vertices.push(p);
        (self.vertices.len() - 1) as u32
    }

    fn tri(&mut self, a: Vector3, b: Vector3, c: Vector3) {
        let (ia, ib, ic) = (self.vertex(a), self.vertex(b), self.vertex(c));
        self.triangles.push([ia, ib, ic]);
    }
}

/// The surface of an `ADVANCED_FACE` (the supported subset).
enum Surface {
    Plane(Placement),
    Cylinder(Placement, f64),
    Sphere(Placement, f64),
    /// placement, radius at the placement plane, semi_angle (radians):
    /// radius(z) = radius + z * tan(semi_angle).
    Cone(Placement, f64, f64),
    /// placement, major radius (to the tube center), minor radius
    /// (the tube).
    Torus(Placement, f64, f64),
}

/// Import every `MANIFOLD_SOLID_BREP` in the file as a mesh.
pub fn import_solids(set: &EntitySet, deflection: f64) -> Result<Vec<(String, Mesh)>, String> {
    if !deflection.is_finite() || deflection <= 0.0 {
        return Err(format!(
            "deflection must be finite and > 0, got {deflection}"
        ));
    }
    let mut solids = Vec::new();
    for (id, brep) in set.all("MANIFOLD_SOLID_BREP") {
        let name = brep
            .params
            .first()
            .and_then(|p| p.as_str().ok())
            .unwrap_or("Solid")
            .to_string();
        let shell_id = brep.params[1].as_ref_id()?;
        let mesh = import_solid(set, shell_id, deflection)
            .map_err(|e| format!("solid '{name}' (#{id}): {e}"))?;
        solids.push((name, mesh));
    }
    if solids.is_empty() {
        return Err("no MANIFOLD_SOLID_BREP found (not a solid STEP export)".to_string());
    }
    Ok(solids)
}

fn import_solid(set: &EntitySet, shell_id: u64, deflection: f64) -> Result<Mesh, String> {
    let shell = set.entity(shell_id, "CLOSED_SHELL")?;
    let faces = shell
        .params
        .get(1)
        .ok_or("CLOSED_SHELL without face list")?
        .as_list()?;
    let mut builder = MeshBuilder::default();
    for face_ref in faces {
        let face_id = face_ref.as_ref_id()?;
        import_face(set, face_id, deflection, &mut builder)
            .map_err(|e| format!("face #{face_id}: {e}"))?;
    }
    let mut mesh = Mesh::new(builder.vertices, builder.triangles);
    mesh.validate()
        .map_err(|e| format!("imported mesh invalid: {e}"))?;
    // The shell is the solid's whole boundary: reconcile the face
    // windings through the shared-edge graph and set the global sign
    // by the divergence volume (the writer's per-surface natural
    // normals do not survive a convention-independent reader).
    fc_part::boolean::orient_closed_shell(&mut mesh);
    let volume = mesh.volume();
    if volume <= 0.0 {
        return Err(format!(
            "imported solid has non-positive volume ({volume:.6} over {} triangles — \
             the shell could not be oriented as a solid boundary)",
            mesh.triangles.len()
        ));
    }
    Ok(mesh)
}

fn import_face(
    set: &EntitySet,
    face_id: u64,
    deflection: f64,
    builder: &mut MeshBuilder,
) -> Result<(), String> {
    let face = set.entity(face_id, "ADVANCED_FACE")?;
    // ADVANCED_FACE(name, bounds, surface, same_sense)
    if face.params.len() < 4 {
        return Err(format!(
            "ADVANCED_FACE #{face_id} has {} params",
            face.params.len()
        ));
    }
    let same_sense = match &face.params[3] {
        // ruststep's Enumeration carries the bare token: `.T.` → "T".
        Param::Enum(e) => e == "T" || e == ".T.",
        _ => false,
    };
    let surface = match face.params[2].as_ref_id()? {
        id if set.get(id)?.name == "PLANE" => Surface::Plane(placement(
            set,
            set.entity(id, "PLANE")?.params[1].as_ref_id()?,
        )?),
        id if set.get(id)?.name == "CYLINDRICAL_SURFACE" => {
            let e = set.entity(id, "CYLINDRICAL_SURFACE")?;
            let pl = placement(set, e.params[1].as_ref_id()?)?;
            let radius = e.params[2].as_f64()?;
            Surface::Cylinder(pl, radius)
        }
        id if set.get(id)?.name == "SPHERICAL_SURFACE" => {
            let e = set.entity(id, "SPHERICAL_SURFACE")?;
            let pl = placement(set, e.params[1].as_ref_id()?)?;
            let radius = e.params[2].as_f64()?;
            Surface::Sphere(pl, radius)
        }
        id if set.get(id)?.name == "CONICAL_SURFACE" => {
            let e = set.entity(id, "CONICAL_SURFACE")?;
            let pl = placement(set, e.params[1].as_ref_id()?)?;
            let radius = e.params[2].as_f64()?;
            let semi_angle = e.params[3].as_f64()?;
            Surface::Cone(pl, radius, semi_angle)
        }
        id if set.get(id)?.name == "TOROIDAL_SURFACE" => {
            let e = set.entity(id, "TOROIDAL_SURFACE")?;
            let pl = placement(set, e.params[1].as_ref_id()?)?;
            let major = e.params[2].as_f64()?;
            let minor = e.params[3].as_f64()?;
            Surface::Torus(pl, major, minor)
        }
        id => {
            return Err(format!(
                "unsupported surface '#{}' ({}) — this reader covers                  planes, cylinders and spheres",
                id,
                set.get(id)?.name
            ))
        }
    };
    let bounds = face.params[1].as_list()?;
    let mut loops: Vec<Vec<Vector3>> = Vec::new();
    for bound in bounds {
        let bound_id = bound.as_ref_id()?;
        loops.push(
            face_bound_loop(set, bound_id, deflection)
                .map_err(|e| format!("bound #{bound_id}: {e}"))?,
        );
    }
    match &surface {
        Surface::Plane(pl) => import_planar_face(pl, &loops, same_sense, builder),
        Surface::Cylinder(pl, radius) => {
            import_cylindrical_face(pl, *radius, &loops, same_sense, deflection, builder)
        }
        Surface::Sphere(pl, radius) => {
            import_spherical_face(pl, *radius, &loops, same_sense, deflection, builder)
        }
        Surface::Cone(pl, radius, semi_angle) => import_conical_face(
            pl,
            *radius,
            *semi_angle,
            &loops,
            same_sense,
            deflection,
            builder,
        ),
        Surface::Torus(pl, major, minor) => {
            import_toroidal_face(pl, *major, *minor, &loops, same_sense, deflection, builder)
        }
    }
}

/// Sagitta-rule angular step: the arc whose chord sags one
/// deflection on the given radius.
fn step_angle_for(radius: f64, deflection: f64) -> f64 {
    let ratio = (1.0 - deflection / radius).clamp(-1.0, 1.0);
    2.0 * ratio.acos().max(1e-3)
}

/// Latitude/longitude bands over the whole sphere (mirroring the
/// kernel's own sphere tessellation): `segments` meridians,
/// `bands` latitudes, two triangles per cell.
fn emit_latlong_bands(
    builder: &mut MeshBuilder,
    pl: &Placement,
    radius: f64,
    segments: usize,
    bands: usize,
    flip: bool,
) {
    let pi = std::f64::consts::PI;
    let second_v = second(pl);
    let world = |theta: f64, phi: f64| -> Vector3 {
        // The poles are ONE point, bit-exact for every meridian:
        // sin(pi) is 1.2e-16, and a per-meridian sprinkling of
        // last-bit pole copies would pepper the cap with micro
        // edges the exact-weld cannot pair.
        if phi <= 0.0 {
            return Vector3::new(
                pl.origin.x + radius * pl.axis.x,
                pl.origin.y + radius * pl.axis.y,
                pl.origin.z + radius * pl.axis.z,
            );
        }
        if phi >= std::f64::consts::PI {
            return Vector3::new(
                pl.origin.x - radius * pl.axis.x,
                pl.origin.y - radius * pl.axis.y,
                pl.origin.z - radius * pl.axis.z,
            );
        }
        let (st, ct) = (theta.sin(), theta.cos());
        let (sp, cp) = (phi.sin(), phi.cos());
        let radial = Vector3::new(
            ct * pl.ref_dir.x + st * second_v.x,
            ct * pl.ref_dir.y + st * second_v.y,
            ct * pl.ref_dir.z + st * second_v.z,
        );
        Vector3::new(
            pl.origin.x + radius * (sp * radial.x + cp * pl.axis.x),
            pl.origin.y + radius * (sp * radial.y + cp * pl.axis.y),
            pl.origin.z + radius * (sp * radial.z + cp * pl.axis.z),
        )
    };
    let area2_ok = |a: Vector3, b: Vector3, c: Vector3| -> bool {
        // Twice the area must clear a degeneracy bar of the cell's
        // own scale (the pole cells carry one zero-area triangle by
        // construction and one real one — each decides for itself).
        (b - a).cross(&(c - a)).length() > 1e-18 * radius.max(1.0)
    };
    for b in 0..bands {
        let f0 = b as f64 / bands as f64 * pi;
        let f1 = (b + 1) as f64 / bands as f64 * pi;
        for i in 0..segments {
            let t0 = i as f64 / segments as f64 * std::f64::consts::TAU;
            let t1 = (i + 1) as f64 / segments as f64 * std::f64::consts::TAU;
            let (p00, p10, p01, p11) = (world(t0, f0), world(t1, f0), world(t0, f1), world(t1, f1));
            if flip {
                if area2_ok(p00, p10, p11) {
                    builder.tri(p00, p10, p11);
                }
                if area2_ok(p00, p11, p01) {
                    builder.tri(p00, p11, p01);
                }
            } else {
                if area2_ok(p00, p11, p10) {
                    builder.tri(p00, p11, p10);
                }
                if area2_ok(p00, p01, p11) {
                    builder.tri(p00, p01, p11);
                }
            }
        }
    }
}

/// A conical face: the cylinder's sampler with the radius linear in
/// z (`radius(z) = radius + z·tan(semi_angle)`), which also covers
/// the cylinder as the semi_angle → 0 limit. The apex (radius → 0)
/// is handled by the degenerate-triangle guard.
fn import_conical_face(
    pl: &Placement,
    radius: f64,
    semi_angle: f64,
    loops: &[Vec<Vector3>],
    same_sense: bool,
    deflection: f64,
    builder: &mut MeshBuilder,
) -> Result<(), String> {
    let tan_a = semi_angle.tan();
    let to_params = |p: &Vector3| -> (f64, f64) {
        let d = Vector3::new(p.x - pl.origin.x, p.y - pl.origin.y, p.z - pl.origin.z);
        let x = d.x * pl.ref_dir.x + d.y * pl.ref_dir.y + d.z * pl.ref_dir.z;
        let y = d.x * second(pl).x + d.y * second(pl).y + d.z * second(pl).z;
        let z = d.x * pl.axis.x + d.y * pl.axis.y + d.z * pl.axis.z;
        (y.atan2(x), z)
    };
    let world = |theta: f64, z: f64| -> Vector3 {
        let r = (radius + z * tan_a).max(0.0);
        let (c, s) = (theta.cos(), theta.sin());
        Vector3::new(
            pl.origin.x + r * (c * pl.ref_dir.x + s * second(pl).x) + z * pl.axis.x,
            pl.origin.y + r * (c * pl.ref_dir.y + s * second(pl).y) + z * pl.axis.y,
            pl.origin.z + r * (c * pl.ref_dir.z + s * second(pl).z) + z * pl.axis.z,
        )
    };
    // Extents.
    let mut z_min = f64::INFINITY;
    let mut z_max = f64::NEG_INFINITY;
    let mut thetas: Vec<f64> = Vec::new();
    for l in loops {
        for p in l {
            let (theta, z) = to_params(p);
            thetas.push(theta);
            z_min = z_min.min(z);
            z_max = z_max.max(z);
        }
    }
    if thetas.is_empty() {
        return Err("conical face without loop points".to_string());
    }
    thetas.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let step_angle = step_angle_for(radius.max(1e-9), deflection);
    let mut max_gap: f64 = 0.0;
    if thetas.len() >= 2 {
        for w in thetas.windows(2) {
            max_gap = max_gap.max(w[1] - w[0]);
        }
        max_gap = max_gap.max(thetas[0] + std::f64::consts::TAU - thetas[thetas.len() - 1]);
    }
    let full_turn = max_gap < step_angle * 1.5;
    let segments = ((std::f64::consts::TAU / step_angle).ceil() as usize).clamp(8, 4096);
    let flip = !same_sense;
    if full_turn {
        // A full band: subdivide z so every cell's slant stays within
        // the deflection (the slant is the cone's own sampling
        // direction between its exact circular rims).
        let slant = ((z_max - z_min) * (1.0 + tan_a * tan_a).sqrt()).max(1e-9);
        let rows = ((slant / deflection.max(1e-9)).ceil() as usize).clamp(1, 4096);
        for row in 0..rows {
            let z0 = z_min + (z_max - z_min) * row as f64 / rows as f64;
            let z1 = z_min + (z_max - z_min) * (row + 1) as f64 / rows as f64;
            for i in 0..segments {
                let t0 = i as f64 / segments as f64 * std::f64::consts::TAU;
                let t1 = (i + 1) as f64 / segments as f64 * std::f64::consts::TAU;
                let (p00, p10, p01, p11) =
                    (world(t0, z0), world(t1, z0), world(t0, z1), world(t1, z1));
                let ok =
                    |a: Vector3, b: Vector3, c: Vector3| (b - a).cross(&(c - a)).length() > 1e-18;
                if flip {
                    if ok(p00, p10, p11) {
                        builder.tri(p00, p10, p11);
                    }
                    if ok(p00, p11, p01) {
                        builder.tri(p00, p11, p01);
                    }
                } else {
                    if ok(p00, p11, p10) {
                        builder.tri(p00, p11, p10);
                    }
                    if ok(p00, p01, p11) {
                        builder.tri(p00, p01, p11);
                    }
                }
            }
        }
        return Ok(());
    }
    // Partial: ear-clip in the developed (r·θ, s) plane, where s is
    // the slant parameter (z scaled by the slant factor).
    // (partial patches: reported below)
    // Partial conical patches (not written by the primitives) need a
    // development inversion the full-turn path does not; report it.
    Err("partial conical face (full-turn bands are supported)".to_string())
}

/// A toroidal face. FreeCAD's `Part::Torus` is ONE face whose seam
/// loop carries both full circles (the major seam and the tube's),
/// so the loop points cover θ and ψ completely — the whole-torus
/// grid. Partial patches are reported (not written by the
/// primitives).
fn import_toroidal_face(
    pl: &Placement,
    major: f64,
    minor: f64,
    loops: &[Vec<Vector3>],
    same_sense: bool,
    deflection: f64,
    builder: &mut MeshBuilder,
) -> Result<(), String> {
    if major <= 0.0 || minor <= 0.0 || minor > major {
        return Err(format!("toroidal face with radii {major}/{minor}"));
    }
    let second_v = second(pl);
    let to_params = |p: &Vector3| -> (f64, f64) {
        let d = Vector3::new(p.x - pl.origin.x, p.y - pl.origin.y, p.z - pl.origin.z);
        let x = d.x * pl.ref_dir.x + d.y * pl.ref_dir.y + d.z * pl.ref_dir.z;
        let y = d.x * second_v.x + d.y * second_v.y + d.z * second_v.z;
        let z = d.x * pl.axis.x + d.y * pl.axis.y + d.z * pl.axis.z;
        let rho = (x * x + y * y).sqrt();
        (y.atan2(x), (z).atan2(rho - major))
    };
    let world = |theta: f64, psi: f64| -> Vector3 {
        let (st, ct) = (theta.sin(), theta.cos());
        let (sp, cp) = (psi.sin(), psi.cos());
        let ring = major + minor * cp;
        let radial = Vector3::new(
            ct * pl.ref_dir.x + st * second_v.x,
            ct * pl.ref_dir.y + st * second_v.y,
            ct * pl.ref_dir.z + st * second_v.z,
        );
        Vector3::new(
            pl.origin.x + ring * radial.x + minor * sp * pl.axis.x,
            pl.origin.y + ring * radial.y + minor * sp * pl.axis.y,
            pl.origin.z + ring * radial.z + minor * sp * pl.axis.z,
        )
    };
    // Coverage census over the loop points.
    let mut thetas: Vec<f64> = Vec::new();
    let mut psis: Vec<f64> = Vec::new();
    for l in loops {
        for p in l {
            let (theta, psi) = to_params(p);
            thetas.push(theta);
            psis.push(psi);
        }
    }
    if thetas.is_empty() {
        return Err("toroidal face without loop points".to_string());
    }
    thetas.sort_by(|a, b| a.partial_cmp(b).unwrap());
    psis.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let full = |xs: &[f64], step: f64| -> bool {
        let mut gap: f64 = 0.0;
        if xs.len() >= 2 {
            for w in xs.windows(2) {
                gap = gap.max(w[1] - w[0]);
            }
            gap = gap.max(xs[0] + std::f64::consts::TAU - xs[xs.len() - 1]);
        }
        gap < step * 1.5
    };
    let step_theta = step_angle_for(major + minor, deflection);
    let step_psi = step_angle_for(minor, deflection);
    if full(&thetas, step_theta) && full(&psis, step_psi) {
        let segments = ((std::f64::consts::TAU / step_theta).ceil() as usize).clamp(8, 4096);
        let rings = ((std::f64::consts::TAU / step_psi).ceil() as usize).clamp(4, 4096);
        let flip = !same_sense;
        let ok = |a: Vector3, b: Vector3, c: Vector3| {
            (b - a).cross(&(c - a)).length() > 1e-18 * (major + minor).max(1.0)
        };
        for r in 0..rings {
            let s0 = r as f64 / rings as f64 * std::f64::consts::TAU;
            let s1 = (r + 1) as f64 / rings as f64 * std::f64::consts::TAU;
            for i in 0..segments {
                let t0 = i as f64 / segments as f64 * std::f64::consts::TAU;
                let t1 = (i + 1) as f64 / segments as f64 * std::f64::consts::TAU;
                let (p00, p10, p01, p11) =
                    (world(t0, s0), world(t1, s0), world(t0, s1), world(t1, s1));
                if flip {
                    if ok(p00, p10, p11) {
                        builder.tri(p00, p10, p11);
                    }
                    if ok(p00, p11, p01) {
                        builder.tri(p00, p11, p01);
                    }
                } else {
                    if ok(p00, p11, p10) {
                        builder.tri(p00, p11, p10);
                    }
                    if ok(p00, p01, p11) {
                        builder.tri(p00, p01, p11);
                    }
                }
            }
        }
        return Ok(());
    }
    Err("partial toroidal face (the whole-torus grid is supported)".to_string())
}

/// A spherical face. The whole-sphere case — one face, one seam
/// loop, both poles — is the shape FreeCAD's `Part::Sphere` exports;
/// partial patches ear-clip in the unwrapped (θ, φ) plane the same
/// way the cylinder's partial turn does.
fn import_spherical_face(
    pl: &Placement,
    radius: f64,
    loops: &[Vec<Vector3>],
    same_sense: bool,
    deflection: f64,
    builder: &mut MeshBuilder,
) -> Result<(), String> {
    if radius <= 0.0 {
        return Err(format!("spherical face with radius {radius}"));
    }
    let to_params = |p: &Vector3| -> (f64, f64) {
        let d = Vector3::new(p.x - pl.origin.x, p.y - pl.origin.y, p.z - pl.origin.z);
        let x = d.x * pl.ref_dir.x + d.y * pl.ref_dir.y + d.z * pl.ref_dir.z;
        let y = d.x * second(pl).x + d.y * second(pl).y + d.z * second(pl).z;
        let z = d.x * pl.axis.x + d.y * pl.axis.y + d.z * pl.axis.z;
        let rho = (x * x + y * y).sqrt();
        (y.atan2(x), rho.atan2(z))
    };
    let world = |theta: f64, phi: f64| -> Vector3 {
        let (st, ct) = (theta.sin(), theta.cos());
        let (sp, cp) = (phi.sin(), phi.cos());
        let radial = Vector3::new(
            ct * pl.ref_dir.x + st * second(pl).x,
            ct * pl.ref_dir.y + st * second(pl).y,
            ct * pl.ref_dir.z + st * second(pl).z,
        );
        Vector3::new(
            pl.origin.x + radius * (sp * radial.x + cp * pl.axis.x),
            pl.origin.y + radius * (sp * radial.y + cp * pl.axis.y),
            pl.origin.z + radius * (sp * radial.z + cp * pl.axis.z),
        )
    };

    // Parameter extents over every loop point.
    let mut phi_min = f64::INFINITY;
    let mut phi_max = f64::NEG_INFINITY;
    let mut thetas: Vec<f64> = Vec::new();
    for l in loops {
        for p in l {
            let (theta, phi) = to_params(p);
            thetas.push(theta);
            phi_min = phi_min.min(phi);
            phi_max = phi_max.max(phi);
        }
    }
    // A spherical face whose every bound is degenerate is the whole
    // sphere: FreeCAD writes `Part::Sphere` as one face with a single
    // VERTEX_LOOP at a pole — the surface itself is the domain.
    if thetas.is_empty() {
        let flip = !same_sense;
        let segments = ((std::f64::consts::TAU / step_angle_for(radius, deflection)).ceil()
            as usize)
            .clamp(8, 4096);
        let pi = std::f64::consts::PI;
        let bands = ((pi / step_angle_for(radius, deflection)).ceil() as usize)
            .clamp(4, 2048)
            .max(2);
        emit_latlong_bands(builder, pl, radius, segments, bands, flip);
        return Ok(());
    }
    thetas.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let step_angle = step_angle_for(radius, deflection);
    let mut max_gap: f64 = 0.0;
    if thetas.len() >= 2 {
        for w in thetas.windows(2) {
            max_gap = max_gap.max(w[1] - w[0]);
        }
        max_gap = max_gap.max(thetas[0] + std::f64::consts::TAU - thetas[thetas.len() - 1]);
    }
    let pi = std::f64::consts::PI;
    let whole_sphere = phi_min <= 1e-9 && phi_max >= pi - 1e-9 && max_gap < step_angle * 1.5;
    if whole_sphere {
        let segments = ((std::f64::consts::TAU / step_angle).ceil() as usize).clamp(8, 4096);
        let bands = ((pi / step_angle).ceil() as usize).clamp(4, 2048).max(2);
        emit_latlong_bands(builder, pl, radius, segments, bands, !same_sense);
        return Ok(());
    }

    // Partial patch: unwrap θ (shift by the biggest gap so the loop
    // does not straddle the wrap), then ear-clip in the scaled
    // (θ, φ) plane. The metric is exact at the patch's own latitudes
    // to the extent the plane can carry it — patches are small.
    let mut rings: Vec<Vec<Point2>> = loops
        .iter()
        .filter(|l| l.len() >= 3)
        .map(|l| {
            let mut ring: Vec<Point2> = l
                .iter()
                .map(|p| {
                    let (theta, phi) = to_params(p);
                    Point2 {
                        x: theta * radius,
                        y: phi * radius,
                    }
                })
                .collect();
            if ring.len() > 1 && ring[0] == ring[ring.len() - 1] {
                ring.pop();
            }
            ring
        })
        .filter(|r| r.len() >= 3)
        .collect();
    if rings.is_empty() {
        return Err("spherical face's loops all degenerate".to_string());
    }
    rings.sort_by(|a, b| ring_area(a).abs().total_cmp(&ring_area(b).abs()).reverse());
    let outer = rings.remove(0);
    let triangles = triangulate_planar_region(&outer, &rings)
        .map_err(|e| format!("spherical face triangulation failed: {e}"))?;
    let flip = !same_sense;
    let mut all_pts: Vec<Point2> = outer;
    for r in &rings {
        all_pts.extend(r.iter().copied());
    }
    for t in triangles {
        let to3 = |p: &Point2| world(p.x / radius, p.y / radius);
        let (a, b, c) = (
            to3(&all_pts[t[0] as usize]),
            to3(&all_pts[t[1] as usize]),
            to3(&all_pts[t[2] as usize]),
        );
        if flip {
            builder.tri(a, c, b);
        } else {
            builder.tri(a, b, c);
        }
    }
    Ok(())
}

/// A `FACE_OUTER_BOUND` (or `FACE_BOUND`) → sampled 3D polyline.
fn face_bound_loop(
    set: &EntitySet,
    bound_id: u64,
    deflection: f64,
) -> Result<Vec<Vector3>, String> {
    let bound = set.get(bound_id)?;
    if bound.name != "FACE_OUTER_BOUND" && bound.name != "FACE_BOUND" {
        return Err(format!(
            "unsupported face bound {} #{bound_id} (expected FACE_OUTER_BOUND)",
            bound.name
        ));
    }
    let loop_id = bound.params[1].as_ref_id()?;
    // The bound's orientation flag is an enumeration token too.
    let orientation = match bound.params.get(2) {
        Some(Param::Enum(e)) => e == "T" || e == ".T.",
        Some(Param::Str(s)) => s == "T" || s == ".T.",
        _ => true,
    };
    let loop_entity = set.get(loop_id)?;
    let mut points = match loop_entity.name.as_str() {
        // A degenerate boundary (a sphere's pole, a cone's apex): one
        // vertex, no edges — its face's geometry is the surface, not
        // the loop, so it samples nothing here.
        "VERTEX_LOOP" => Vec::new(),
        "POLY_LOOP" => {
            let mut pts = Vec::new();
            let list = loop_entity
                .params
                .get(1)
                .ok_or("POLY_LOOP without points")?
                .as_list()?;
            for p in list {
                pts.push(point(set, p.as_ref_id()?)?);
            }
            pts
        }
        "EDGE_LOOP" => {
            let mut pts: Vec<Vector3> = Vec::new();
            let edges = loop_entity
                .params
                .get(1)
                .ok_or("EDGE_LOOP without edges")?
                .as_list()?;
            for edge in edges {
                let oriented = set.get(edge.as_ref_id()?)?;
                if oriented.name != "ORIENTED_EDGE" {
                    return Err(format!(
                        "unsupported edge {} in EDGE_LOOP #{loop_id} (expected ORIENTED_EDGE)",
                        oriented.name
                    ));
                }
                // `.T.` / `.F.` are enumeration tokens, not strings.
                let forward = match oriented.params.last() {
                    Some(Param::Enum(e)) => e == "T" || e == ".T.",
                    Some(Param::Str(s)) => s == "T" || s == ".T.",
                    _ => true,
                };
                let edge_id = oriented.params[3].as_ref_id()?;
                let mut segment = sample_edge(set, edge_id, deflection)?;
                if !forward {
                    segment.reverse();
                }
                // Drop the segment's first point when it repeats the
                // accumulated tail (edges share vertices).
                if let Some(&last) = pts.last() {
                    if let Some(first) = segment.first() {
                        if (first.x - last.x).abs() < 1e-9
                            && (first.y - last.y).abs() < 1e-9
                            && (first.z - last.z).abs() < 1e-9
                        {
                            segment.remove(0);
                        }
                    }
                }
                pts.extend(segment);
            }
            // Close the ring explicitly (the last point may repeat the
            // first).
            if let (Some(first), Some(last)) = (pts.first().copied(), pts.last().copied()) {
                if (first.x - last.x).abs() > 1e-9
                    || (first.y - last.y).abs() > 1e-9
                    || (first.z - last.z).abs() > 1e-9
                {
                    pts.push(first);
                }
            }
            pts
        }
        other => {
            return Err(format!(
                "unsupported loop {other} #{loop_id} (the subset is POLY_LOOP + EDGE_LOOP)"
            ))
        }
    };
    if !orientation {
        points.reverse();
    }
    // A degenerate loop (its face's whole boundary is a vertex — a
    // sphere written as one face over its full surface) samples to
    // nothing; the FACE's geometry then decides what it covers.
    Ok(points)
}

/// Sample an `EDGE_CURVE`'s geometry (LINE or CIRCLE) under deflection.
fn sample_edge(set: &EntitySet, edge_id: u64, deflection: f64) -> Result<Vec<Vector3>, String> {
    let edge = set.entity(edge_id, "EDGE_CURVE")?;
    // EDGE_CURVE(name, start, end, curve, same_sense)
    let start = vertex_point(set, edge.params[1].as_ref_id()?)?;
    let end = vertex_point(set, edge.params[2].as_ref_id()?)?;
    let curve_id = edge.params[3].as_ref_id()?;
    let curve = set.get(curve_id)?;
    match curve.name.as_str() {
        "LINE" => Ok(vec![start, end]),
        "CIRCLE" => {
            let radius = curve.params[2].as_f64()?;
            let pl = placement(set, curve.params[1].as_ref_id()?)?;
            sample_circle_arc(&pl, radius, &start, &end, deflection)
        }
        other => Err(format!(
            "unsupported edge curve {other} #{curve_id} (the subset is LINE + CIRCLE)"
        )),
    }
}

fn vertex_point(set: &EntitySet, id: u64) -> Result<Vector3, String> {
    let v = set.entity(id, "VERTEX_POINT")?;
    point(set, v.params[1].as_ref_id()?)
}

/// Tessellate the arc `start → end` on the circle (placement, radius),
/// chords within `deflection` of the true arc.
fn sample_circle_arc(
    pl: &Placement,
    radius: f64,
    start: &Vector3,
    end: &Vector3,
    deflection: f64,
) -> Result<Vec<Vector3>, String> {
    let local = |p: &Vector3| -> (f64, f64, f64) {
        let d = Vector3::new(p.x - pl.origin.x, p.y - pl.origin.y, p.z - pl.origin.z);
        (
            d.x * pl.ref_dir.x + d.y * pl.ref_dir.y + d.z * pl.ref_dir.z,
            // Second basis: axis × ref
            d.x * (pl.axis.y * pl.ref_dir.z - pl.axis.z * pl.ref_dir.y)
                + d.y * (pl.axis.z * pl.ref_dir.x - pl.axis.x * pl.ref_dir.z)
                + d.z * (pl.axis.x * pl.ref_dir.y - pl.axis.y * pl.ref_dir.x),
            d.x * pl.axis.x + d.y * pl.axis.y + d.z * pl.axis.z,
        )
    };
    let world = |theta: f64, z: f64| -> Vector3 {
        let (c, s) = (theta.cos(), theta.sin());
        Vector3::new(
            pl.origin.x + radius * (c * pl.ref_dir.x + s * second(pl).x) + z * pl.axis.x,
            pl.origin.y + radius * (c * pl.ref_dir.y + s * second(pl).y) + z * pl.axis.y,
            pl.origin.z + radius * (c * pl.ref_dir.z + s * second(pl).z) + z * pl.axis.z,
        )
    };
    let (x0, y0, _z0) = local(start);
    let (x1, y1, _z1) = local(end);
    let t0: f64 = y0.atan2(x0);
    let mut t1: f64 = y1.atan2(x1);
    // Positive (CCW) sweep, per EDGE_CURVE's curve orientation.
    while t1 <= t0 {
        t1 += std::f64::consts::TAU;
    }
    let sweep = t1 - t0;
    // Sagitta discipline: chord ≤ deflection for radius r means
    // step angle ≤ 2·acos(1 − d/r), clamped to a sane minimum.
    let ratio = (1.0 - deflection / radius).clamp(-1.0, 1.0);
    let max_step = 2.0 * ratio.acos().max(1e-3);
    let steps = ((sweep / max_step).ceil() as usize).clamp(1, 4096);
    let mut pts = Vec::with_capacity(steps + 1);
    let z0 = local(start).2;
    let z1 = local(end).2;
    for i in 0..=steps {
        let t = i as f64 / steps as f64;
        let theta = t0 + sweep * t;
        let z = z0 + (z1 - z0) * t;
        pts.push(world(theta, z));
    }
    Ok(pts)
}

fn second(pl: &Placement) -> Vector3 {
    Vector3::new(
        pl.axis.y * pl.ref_dir.z - pl.axis.z * pl.ref_dir.y,
        pl.axis.z * pl.ref_dir.x - pl.axis.x * pl.ref_dir.z,
        pl.axis.x * pl.ref_dir.y - pl.axis.y * pl.ref_dir.x,
    )
}

/// A planar face: project its loops to the plane, triangulate with
/// holes, orient by the surface/same-sense normal.
fn import_planar_face(
    pl: &Placement,
    loops: &[Vec<Vector3>],
    same_sense: bool,
    builder: &mut MeshBuilder,
) -> Result<(), String> {
    if loops.is_empty() {
        return Err("planar face without bounds".to_string());
    }
    let u = pl.ref_dir;
    let v = second(pl);
    let to_2d = |p: &Vector3| -> Point2 {
        let d = Vector3::new(p.x - pl.origin.x, p.y - pl.origin.y, p.z - pl.origin.z);
        Point2 {
            x: d.x * u.x + d.y * u.y + d.z * u.z,
            y: d.x * v.x + d.y * v.y + d.z * v.z,
        }
    };
    // Rings: drop the duplicated closing point (ear-clip wants open
    // rings) and degenerate ones.
    let mut rings: Vec<Vec<Point2>> = loops
        .iter()
        .map(|l| {
            let mut pts: Vec<Point2> = l.iter().map(to_2d).collect();
            if pts.len() > 1 && pts[0] == pts[pts.len() - 1] {
                pts.pop();
            }
            pts
        })
        .filter(|r| r.len() >= 3)
        .collect();
    if rings.is_empty() {
        return Err("planar face's loops all degenerate".to_string());
    }
    // Largest ring by |area| is the outer boundary; the rest are holes.
    rings.sort_by(|a, b| ring_area(a).abs().total_cmp(&ring_area(b).abs()).reverse());
    let outer = rings.remove(0);
    let triangles = triangulate_planar_region(&outer, &rings)
        .map_err(|e| format!("planar face triangulation failed: {e}"))?;
    // Orient: the face's outward normal.
    let flip = !same_sense;
    let mut all_pts: Vec<Point2> = outer;
    for r in &rings {
        all_pts.extend(r.iter().copied());
    }
    let to_3d = |p: &Point2| -> Vector3 {
        Vector3::new(
            pl.origin.x + p.x * u.x + p.y * v.x,
            pl.origin.y + p.x * u.y + p.y * v.y,
            pl.origin.z + p.x * u.z + p.y * v.z,
        )
    };
    for t in triangles {
        let (a, b, c) = (
            to_3d(&all_pts[t[0] as usize]),
            to_3d(&all_pts[t[1] as usize]),
            to_3d(&all_pts[t[2] as usize]),
        );
        if flip {
            builder.tri(a, c, b);
        } else {
            builder.tri(a, b, c);
        }
    }
    Ok(())
}

fn ring_area(ring: &[Point2]) -> f64 {
    let n = ring.len();
    if n < 3 {
        return 0.0;
    }
    let mut area = 0.0;
    for i in 0..n {
        let a = ring[i];
        let b = ring[(i + 1) % n];
        area += a.x * b.y - b.x * a.y;
    }
    area / 2.0
}

/// A cylindrical face: (θ, z) parameter space.
fn import_cylindrical_face(
    pl: &Placement,
    radius: f64,
    loops: &[Vec<Vector3>],
    same_sense: bool,
    deflection: f64,
    builder: &mut MeshBuilder,
) -> Result<(), String> {
    if radius <= 0.0 {
        return Err(format!("cylindrical face with radius {radius}"));
    }
    let to_params = |p: &Vector3| -> (f64, f64) {
        let d = Vector3::new(p.x - pl.origin.x, p.y - pl.origin.y, p.z - pl.origin.z);
        let x = d.x * pl.ref_dir.x + d.y * pl.ref_dir.y + d.z * pl.ref_dir.z;
        let y = d.x * second(pl).x + d.y * second(pl).y + d.z * second(pl).z;
        let z = d.x * pl.axis.x + d.y * pl.axis.y + d.z * pl.axis.z;
        (y.atan2(x), z)
    };
    let world = |theta: f64, z: f64| -> Vector3 {
        let (c, s) = (theta.cos(), theta.sin());
        Vector3::new(
            pl.origin.x + radius * (c * pl.ref_dir.x + s * second(pl).x) + z * pl.axis.x,
            pl.origin.y + radius * (c * pl.ref_dir.y + s * second(pl).y) + z * pl.axis.y,
            pl.origin.z + radius * (c * pl.ref_dir.z + s * second(pl).z) + z * pl.axis.z,
        )
    };

    // Full-turn detection: if every sampled θ over all loops covers the
    // circle (max gap between consecutive samples, wrapped, below the
    // step angle), the face is a full band — tessellate the band between
    // the loop z-extents.
    let step_angle = {
        let ratio = (1.0 - deflection / radius).clamp(-1.0, 1.0);
        2.0 * ratio.acos().max(1e-3)
    };
    let mut thetas: Vec<f64> = Vec::new();
    let mut z_min = f64::INFINITY;
    let mut z_max = f64::NEG_INFINITY;
    for l in loops {
        for p in l {
            let (theta, z) = to_params(p);
            thetas.push(theta);
            z_min = z_min.min(z);
            z_max = z_max.max(z);
        }
    }
    thetas.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let mut max_gap: f64 = 0.0;
    if thetas.len() >= 2 {
        for w in thetas.windows(2) {
            max_gap = max_gap.max(w[1] - w[0]);
        }
        max_gap = max_gap.max(thetas[0] + std::f64::consts::TAU - thetas[thetas.len() - 1]);
    }
    if max_gap < step_angle * 1.5 {
        // Full band: segments from the sagitta rule.
        let segments = ((std::f64::consts::TAU / step_angle).ceil() as usize).clamp(8, 4096);
        let flip = !same_sense;
        for i in 0..segments {
            let t0 = i as f64 / segments as f64 * std::f64::consts::TAU;
            let t1 = (i + 1) as f64 / segments as f64 * std::f64::consts::TAU;
            let (p00, p10, p01, p11) = (
                world(t0, z_min),
                world(t1, z_min),
                world(t0, z_max),
                world(t1, z_max),
            );
            if flip {
                builder.tri(p00, p10, p11);
                builder.tri(p00, p11, p01);
            } else {
                builder.tri(p00, p11, p10);
                builder.tri(p00, p01, p11);
            }
        }
        return Ok(());
    }

    // Partial turn: unwrap into (r·θ, z) and ear-clip.
    let mut rings: Vec<Vec<Point2>> = loops
        .iter()
        .map(|l| {
            let mut ring: Vec<Point2> = l
                .iter()
                .map(|p| {
                    let (theta, z) = to_params(p);
                    Point2 {
                        x: theta * radius,
                        y: z,
                    }
                })
                .collect();
            if ring.len() > 1 && ring[0] == ring[ring.len() - 1] {
                ring.pop();
            }
            ring
        })
        .filter(|r| r.len() >= 3)
        .collect();
    if rings.is_empty() {
        return Err("cylindrical face's loops all degenerate".to_string());
    }
    rings.sort_by(|a, b| ring_area(a).abs().total_cmp(&ring_area(b).abs()).reverse());
    let outer = rings.remove(0);
    let triangles = triangulate_planar_region(&outer, &rings)
        .map_err(|e| format!("cylindrical face triangulation failed: {e}"))?;
    let flip = !same_sense;
    let mut all_pts: Vec<Point2> = outer;
    for r in &rings {
        all_pts.extend(r.iter().copied());
    }
    for t in triangles {
        let to3 = |p: &Point2| world(p.x / radius, p.y);
        let (a, b, c) = (
            to3(&all_pts[t[0] as usize]),
            to3(&all_pts[t[1] as usize]),
            to3(&all_pts[t[2] as usize]),
        );
        if flip {
            builder.tri(a, c, b);
        } else {
            builder.tri(a, b, c);
        }
    }
    Ok(())
}
