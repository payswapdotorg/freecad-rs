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
    let mesh = Mesh::new(builder.vertices, builder.triangles);
    mesh.validate()
        .map_err(|e| format!("imported mesh invalid: {e}"))?;
    let volume = mesh.volume();
    if volume <= 0.0 {
        return Err(format!(
            "imported solid has non-positive volume ({volume:.6} over {} triangles — \
             orientation could not be resolved for this face combination)",
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
        id => {
            let e = set.entity(id, "CYLINDRICAL_SURFACE")?;
            let pl = placement(set, e.params[1].as_ref_id()?)?;
            let radius = e.params[2].as_f64()?;
            Surface::Cylinder(pl, radius)
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
    }
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
    let orientation = bound
        .params
        .get(2)
        .and_then(|p| p.as_str().ok())
        .map(|s| s == "T" || s == ".T.")
        .unwrap_or(true);
    let loop_entity = set.get(loop_id)?;
    let mut points = match loop_entity.name.as_str() {
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
                let forward = oriented
                    .params
                    .last()
                    .and_then(|p| p.as_str().ok())
                    .map(|s| s == ".T.")
                    .unwrap_or(true);
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
    if points.len() < 3 {
        return Err(format!("loop #{loop_id} degenerates to < 3 points"));
    }
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
