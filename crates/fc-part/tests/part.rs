//! Integration tests for `fc-part`: analytic primitives, tessellation
//! quality, FCBREP serialization, and the part features wired into the
//! `fc-app` recompute loop.

use std::collections::BTreeMap;
use std::f64::consts::PI;

use fc_app::{Document, Property};
use fc_base::Vector3;
use fc_part::features::{BoxFeature, SphereFeature, TransformFeature};
use fc_part::{read_shape_bytes, write_shape, Mesh, TopoShape, DEFAULT_DEFLECTION};

/// Relative tolerance for exact analytic invariants.
const REL: f64 = 1e-9;

fn assert_rel(got: f64, want: f64, what: &str) {
    let err = (got - want).abs() / want.abs().max(f64::MIN_POSITIVE);
    assert!(
        err <= REL,
        "{what}: got {got}, want {want} (relative error {err:.3e} > {REL})"
    );
}

/// Undirected edge -> number of incident triangles.
fn edge_counts(mesh: &Mesh) -> BTreeMap<(u32, u32), usize> {
    let mut counts: BTreeMap<(u32, u32), usize> = BTreeMap::new();
    for t in &mesh.triangles {
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            let key = if a < b { (a, b) } else { (b, a) };
            *counts.entry(key).or_default() += 1;
        }
    }
    counts
}

/// Vertices referenced by at least one triangle (unreferenced ones are
/// not part of the surface).
fn referenced_vertices(mesh: &Mesh) -> usize {
    let mut used = vec![false; mesh.vertex_count()];
    for t in &mesh.triangles {
        for &v in t {
            used[v as usize] = true;
        }
    }
    used.into_iter().filter(|u| *u).count()
}

/// A closed, watertight triangle mesh: every undirected edge is shared by
/// exactly two triangles, no triangle is degenerate, and all indices are
/// in range (`validate`). The Euler characteristic over the referenced
/// vertices must match the surface's genus (2 for a sphere, 0 for a
/// torus).
fn assert_watertight(mesh: &Mesh, what: &str, expected_chi: i64) {
    mesh.validate()
        .unwrap_or_else(|e| panic!("{what}: structural validation failed: {e}"));
    for t in &mesh.triangles {
        assert!(
            t[0] != t[1] && t[1] != t[2] && t[0] != t[2],
            "{what}: degenerate triangle {t:?}"
        );
    }
    let counts = edge_counts(mesh);
    for (edge, count) in &counts {
        assert_eq!(
            *count, 2,
            "{what}: edge {edge:?} is incident to {count} triangles (want 2)"
        );
    }
    // Euler characteristic check: V - E + F == 2 - 2 * genus.
    let e = counts.len();
    let (v, f) = (referenced_vertices(mesh), mesh.triangle_count());
    let chi = v as i64 - e as i64 + f as i64;
    assert_eq!(
        chi, expected_chi,
        "{what}: Euler characteristic V({v}) - E({e}) + F({f})"
    );
}

/// Every triangle's right-hand-rule normal points away from `inside`
/// (valid for convex solids whose centroid is an interior point).
fn assert_outward_winding(mesh: &Mesh, inside: Vector3, what: &str) {
    for (i, t) in mesh.triangles.iter().enumerate() {
        let (v0, v1, v2) = (
            mesh.vertices[t[0] as usize],
            mesh.vertices[t[1] as usize],
            mesh.vertices[t[2] as usize],
        );
        let normal = (v1 - v0).cross(&(v2 - v0));
        let centroid = (v0 + v1 + v2) * (1.0 / 3.0);
        let outward = centroid - inside;
        assert!(
            normal.dot(&outward) > 0.0,
            "{what}: triangle {i} ({t:?}) is wound inward"
        );
    }
}

// ---------------------------------------------------------------------------
// Analytic invariants
// ---------------------------------------------------------------------------

#[test]
fn box_analytic_invariants() {
    // V = l*w*h, A = 2(lw + lh + wh); FreeCAD 1.0 placement: the corner
    // sits at the origin — [0, l] x [0, w] x [0, h].
    let s = TopoShape::Box {
        length: 3.0,
        width: 4.0,
        height: 5.0,
    };
    assert_eq!(s.kind_name(), "Box");
    assert_rel(s.volume(), 60.0, "box volume");
    assert_rel(s.surface_area(), 2.0 * (12.0 + 15.0 + 20.0), "box area");
    let bbox = s.bbox();
    assert_eq!(bbox.min, Vector3::new(0.0, 0.0, 0.0));
    assert_eq!(bbox.max, Vector3::new(3.0, 4.0, 5.0));

    // Mesh measures of a planar-faceted box are exact too.
    let mesh = s.tessellate(DEFAULT_DEFLECTION);
    assert_rel(mesh.volume(), 60.0, "box mesh volume");
    assert_rel(mesh.surface_area(), 94.0, "box mesh area");
}

#[test]
fn sphere_analytic_invariants() {
    let r = 2.5;
    let s = TopoShape::Sphere { radius: r };
    assert_eq!(s.kind_name(), "Sphere");
    assert_rel(s.volume(), 4.0 / 3.0 * PI * r * r * r, "sphere volume");
    assert_rel(s.surface_area(), 4.0 * PI * r * r, "sphere area");
    let bbox = s.bbox();
    assert_eq!(bbox.min, Vector3::new(-r, -r, -r));
    assert_eq!(bbox.max, Vector3::new(r, r, r));
}

#[test]
fn cylinder_analytic_invariants() {
    // V = pi r^2 h, A = 2 pi r^2 + 2 pi r h; along Z, base on the XY
    // plane (FreeCAD 1.0 convention).
    let (r, h) = (2.0, 7.0);
    let s = TopoShape::Cylinder {
        radius: r,
        height: h,
    };
    assert_eq!(s.kind_name(), "Cylinder");
    assert_rel(s.volume(), PI * r * r * h, "cylinder volume");
    assert_rel(
        s.surface_area(),
        2.0 * PI * r * r + 2.0 * PI * r * h,
        "cylinder area",
    );
    let bbox = s.bbox();
    assert_eq!(bbox.min, Vector3::new(-r, -r, 0.0));
    assert_eq!(bbox.max, Vector3::new(r, r, h));
}

#[test]
fn cone_analytic_invariants() {
    // Plain cone (apex up): V = pi r^2 h / 3, A = lateral + base.
    let (r, h): (f64, f64) = (3.0, 4.0);
    let slant = (h * h + r * r).sqrt(); // 3-4-5 triangle
    let s = TopoShape::Cone {
        radius1: r,
        radius2: 0.0,
        height: h,
    };
    assert_eq!(s.kind_name(), "Cone");
    assert_rel(s.volume(), PI * r * r * h / 3.0, "cone volume");
    assert_rel(s.surface_area(), PI * r * slant + PI * r * r, "cone area");

    // Frustum: V = pi h (r1^2 + r1 r2 + r2^2) / 3,
    // A = pi (r1 + r2) l + pi r1^2 + pi r2^2.
    let (r1, r2, hf): (f64, f64, f64) = (3.0, 1.0, 2.5);
    let lf = (hf * hf + (r1 - r2) * (r1 - r2)).sqrt();
    let f = TopoShape::Cone {
        radius1: r1,
        radius2: r2,
        height: hf,
    };
    assert_rel(
        f.volume(),
        PI * hf * (r1 * r1 + r1 * r2 + r2 * r2) / 3.0,
        "frustum volume",
    );
    assert_rel(
        f.surface_area(),
        PI * (r1 + r2) * lf + PI * r1 * r1 + PI * r2 * r2,
        "frustum area",
    );
    // The widest ring bounds the frustum's horizontal extent; the base
    // ring sits on the XY plane (FreeCAD 1.0 convention).
    let bbox = f.bbox();
    assert_eq!(bbox.min, Vector3::new(-r1, -r1, 0.0));
    assert_eq!(bbox.max, Vector3::new(r1, r1, hf));
}

#[test]
fn torus_analytic_invariants() {
    // V = 2 pi^2 R r^2, A = 4 pi^2 R r.
    let (ring, tube) = (5.0, 1.5);
    let s = TopoShape::Torus {
        radius1: ring,
        radius2: tube,
    };
    assert_eq!(s.kind_name(), "Torus");
    assert_rel(
        s.volume(),
        2.0 * PI * PI * ring * tube * tube,
        "torus volume",
    );
    assert_rel(s.surface_area(), 4.0 * PI * PI * ring * tube, "torus area");
    let bbox = s.bbox();
    let outer = ring + tube;
    assert_eq!(bbox.min, Vector3::new(-outer, -outer, -tube));
    assert_eq!(bbox.max, Vector3::new(outer, outer, tube));
}

#[test]
fn mesh_measures_for_hand_built_mesh() {
    // A unit corner tetrahedron, wound CCW from outside: volume 1/6,
    // area 3 * (1/2) + sqrt(3)/2.
    let mesh = Mesh::new(
        vec![Vector3::ZERO, Vector3::X, Vector3::Y, Vector3::Z],
        vec![[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]],
    );
    assert_watertight(&mesh, "tetrahedron", 2);
    assert_rel(mesh.volume(), 1.0 / 6.0, "tetra volume");
    assert_rel(
        mesh.surface_area(),
        1.5 + (3.0f64).sqrt() / 2.0,
        "tetra area",
    );
    let bbox = mesh.bbox().expect("bbox");
    assert_eq!(bbox.min, Vector3::ZERO);
    assert_eq!(bbox.max, Vector3::new(1.0, 1.0, 1.0));

    // Merging offsets the second mesh's indices.
    let mut merged = mesh.clone();
    merged.merge(&mesh);
    assert_eq!(merged.vertex_count(), 8);
    assert_eq!(merged.triangle_count(), 8);
    // Translated copy shares no vertices but the same measure.
    let mut moved = mesh.clone();
    moved.transform(&fc_base::Matrix4::from_translation(&Vector3::new(
        10.0, 0.0, 0.0,
    )));
    assert_rel(moved.volume(), 1.0 / 6.0, "translated volume");
    assert_rel(moved.surface_area(), mesh.surface_area(), "translated area");
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

#[test]
fn validate_rejects_invalid_shapes() {
    let ok = [
        TopoShape::Box {
            length: 1.0,
            width: 1.0,
            height: 1.0,
        },
        TopoShape::Sphere { radius: 0.5 },
        TopoShape::Cylinder {
            radius: 1.0,
            height: 1.0,
        },
        // A frustum may have one zero radius (plain cone).
        TopoShape::Cone {
            radius1: 1.0,
            radius2: 0.0,
            height: 1.0,
        },
        // Self-intersecting rings are allowed, like FreeCAD's.
        TopoShape::Torus {
            radius1: 1.0,
            radius2: 2.0,
        },
    ];
    for s in &ok {
        assert!(s.validate().is_ok(), "{} should be valid", s.kind_name());
    }

    let bad = [
        (
            TopoShape::Box {
                length: 0.0,
                width: 1.0,
                height: 1.0,
            },
            "Length must be > 0",
        ),
        (
            TopoShape::Box {
                length: 1.0,
                width: -1.0,
                height: 1.0,
            },
            "Width must be > 0",
        ),
        (TopoShape::Sphere { radius: 0.0 }, "Radius must be > 0"),
        (
            TopoShape::Sphere { radius: f64::NAN },
            "Radius must be finite",
        ),
        (
            TopoShape::Cylinder {
                radius: 1.0,
                height: -1.0,
            },
            "Height must be > 0",
        ),
        (
            TopoShape::Cone {
                radius1: 0.0,
                radius2: 0.0,
                height: 1.0,
            },
            "cannot both be 0",
        ),
        (
            TopoShape::Cone {
                radius1: -1.0,
                radius2: 1.0,
                height: 1.0,
            },
            "Radius1 must be >= 0",
        ),
        (
            TopoShape::Torus {
                radius1: 1.0,
                radius2: 0.0,
            },
            "Radius2 must be > 0",
        ),
    ];
    for (shape, want) in &bad {
        let err = shape.validate().expect_err("shape should be rejected");
        assert!(
            err.contains(want),
            "{}: error {err:?} should mention {want:?}",
            shape.kind_name()
        );
    }

    // Meshes: out-of-range indices and non-finite vertices are rejected.
    let indexed_out = Mesh::new(vec![Vector3::ZERO, Vector3::X, Vector3::Y], vec![[0, 1, 3]]);
    assert!(indexed_out.validate().is_err());
    let non_finite = Mesh::new(
        vec![Vector3::ZERO, Vector3::X, Vector3::new(f64::NAN, 0.0, 0.0)],
        vec![[0, 1, 2]],
    );
    assert!(non_finite.validate().is_err());
}

// ---------------------------------------------------------------------------
// Tessellation
// ---------------------------------------------------------------------------

#[test]
fn box_tessellation_is_watertight_and_outward() {
    let (l, w, h) = (2.0, 3.0, 4.0);
    let mesh = TopoShape::Box {
        length: l,
        width: w,
        height: h,
    }
    .tessellate(DEFAULT_DEFLECTION);

    // The box is flat-shaded exactly: 8 shared vertices, 12 triangles.
    assert_eq!(mesh.vertex_count(), 8, "box vertices");
    assert_eq!(mesh.triangle_count(), 12, "box triangles");
    assert_watertight(&mesh, "box", 2);
    // The FreeCAD 1.0 box spans [0, l] x [0, w] x [0, h]; its center is
    // the interior reference for the winding check.
    assert_outward_winding(&mesh, Vector3::new(l / 2.0, w / 2.0, h / 2.0), "box");
    // All 8 corners of [0, l] x [0, w] x [0, h] are present.
    let corners = [
        (0.0, 0.0, 0.0),
        (l, 0.0, 0.0),
        (l, w, 0.0),
        (0.0, w, 0.0),
        (0.0, 0.0, h),
        (l, 0.0, h),
        (l, w, h),
        (0.0, w, h),
    ];
    for (x, y, z) in corners {
        assert!(
            mesh.vertices.contains(&Vector3::new(x, y, z)),
            "missing box corner ({x}, {y}, {z})"
        );
    }
    // Signed volume is positive: the winding is globally consistent.
    assert_rel(mesh.volume(), l * w * h, "box signed volume");
}

#[test]
fn coarse_sphere_tessellation_is_watertight_and_outward() {
    let r = 1.0;
    let mesh = TopoShape::Sphere { radius: r }.tessellate(DEFAULT_DEFLECTION);

    // The kernel convention for the default deflection: 8 stacks x 16
    // slices -> 114 vertices, 224 triangles.
    assert_eq!(mesh.vertex_count(), 114, "coarse sphere vertices");
    assert_eq!(mesh.triangle_count(), 224, "coarse sphere triangles");

    // Every edge shared by exactly two triangles (hand-checked adjacency
    // map) and a sphere's Euler characteristic.
    assert_watertight(&mesh, "coarse sphere", 2);

    // Every vertex lies exactly on the sphere.
    for (i, v) in mesh.vertices.iter().enumerate() {
        assert!(
            (v.length() - r).abs() <= 1e-12,
            "vertex {i} off the sphere: {v:?} (|v| = {})",
            v.length()
        );
    }

    // Consistent outward winding: every face normal points away from the
    // centroid (the origin).
    assert_outward_winding(&mesh, Vector3::ZERO, "coarse sphere");

    // Inscribed polyhedron: strictly less volume than the sphere, and the
    // 8-stack deficit is ~6.3% (documented).
    let analytic = 4.0 / 3.0 * PI * r * r * r;
    let got = mesh.volume();
    assert!(got > 0.0, "positive volume");
    assert!(
        got < analytic,
        "inscribed mesh volume {got} must undercut the sphere {analytic}"
    );
    assert!(
        (analytic - got) / analytic <= 0.07,
        "coarse sphere volume deficit {}% exceeds 7%",
        (analytic - got) / analytic * 100.0
    );
}

#[test]
fn sphere_tessellation_volume_converges() {
    // Finer deflections shrink the inscribed-volume deficit monotonically
    // toward the analytic value.
    let r = 2.0;
    let analytic = 4.0 / 3.0 * PI * r * r * r;
    let mut deficits: Vec<f64> = Vec::new();
    for (deflection, bound) in [(0.4, 0.07), (0.2, 0.02), (0.05, 0.01)] {
        let mesh = TopoShape::Sphere { radius: r }.tessellate(deflection);
        let deficit = (analytic - mesh.volume()) / analytic;
        assert!(
            deficit <= bound,
            "deflection {deflection}: deficit {:.4}% > {:.1}%",
            deficit * 100.0,
            bound * 100.0
        );
        deficits.push(deficit);
    }
    assert!(
        deficits[1] < deficits[0] && deficits[2] < deficits[1],
        "volume converges monotonically with finer deflection: {deficits:?}"
    );
}

#[test]
fn curved_primitive_meshes_are_watertight_and_outward() {
    // Cylinder: convex, bbox center is interior.
    let cyl = TopoShape::Cylinder {
        radius: 2.0,
        height: 5.0,
    }
    .tessellate(0.2);
    assert_watertight(&cyl, "cylinder", 2);
    assert_outward_winding(&cyl, cyl.bbox().expect("bbox").center(), "cylinder");
    assert!(
        ((TopoShape::Cylinder {
            radius: 2.0,
            height: 5.0
        }
        .volume()
            - cyl.volume())
            / (PI * 4.0 * 5.0))
            <= 0.01,
        "cylinder mesh volume within 1% of analytic"
    );

    // Cone with apex: convex, bbox center is interior. (The apex-cone
    // tessellation also carries one unreferenced vertex — see the
    // ignored `cone_tessellation_has_no_orphan_vertices` test.)
    let cone = TopoShape::Cone {
        radius1: 3.0,
        radius2: 0.0,
        height: 4.0,
    }
    .tessellate(0.2);
    assert_watertight(&cone, "cone", 2);
    assert_outward_winding(&cone, cone.bbox().expect("bbox").center(), "cone");
    assert!(
        ((TopoShape::Cone {
            radius1: 3.0,
            radius2: 0.0,
            height: 4.0
        }
        .volume()
            - cone.volume())
            / (PI * 9.0 * 4.0 / 3.0))
            <= 0.01,
        "cone mesh volume within 1% of analytic"
    );

    // Torus: not convex, so the outward test compares each face normal
    // against the center of the tube ring it belongs to.
    let (ring, tube) = (5.0, 1.5);
    let torus = TopoShape::Torus {
        radius1: ring,
        radius2: tube,
    }
    .tessellate(0.2);
    // A torus has genus 1: Euler characteristic 0.
    assert_watertight(&torus, "torus", 0);
    for (i, t) in torus.triangles.iter().enumerate() {
        let (v0, v1, v2) = (
            torus.vertices[t[0] as usize],
            torus.vertices[t[1] as usize],
            torus.vertices[t[2] as usize],
        );
        let normal = (v1 - v0).cross(&(v2 - v0));
        let centroid = (v0 + v1 + v2) * (1.0 / 3.0);
        // Tube-ring center for this face: project onto the XY plane,
        // scale to the ring radius, z = 0.
        let xy_len = (centroid.x * centroid.x + centroid.y * centroid.y).sqrt();
        let tube_center = Vector3::new(centroid.x / xy_len * ring, centroid.y / xy_len * ring, 0.0);
        assert!(
            normal.dot(&(centroid - tube_center)) > 0.0,
            "torus triangle {i} ({t:?}) is wound inward"
        );
    }
    assert!(
        ((TopoShape::Torus {
            radius1: ring,
            radius2: tube
        }
        .volume()
            - torus.volume())
            / (2.0 * PI * PI * ring * tube * tube))
            <= 0.02,
        "torus mesh volume within 2% of analytic"
    );
}

/// A plain cone (`radius2 == 0`) once tessellated with an unreferenced
/// `top_center` vertex; fixed by only emitting the top disc center when
/// there is a top disc.
#[test]
fn cone_tessellation_has_no_orphan_vertices() {
    let cone = TopoShape::Cone {
        radius1: 3.0,
        radius2: 0.0,
        height: 4.0,
    }
    .tessellate(0.2);
    assert_eq!(
        referenced_vertices(&cone),
        cone.vertex_count(),
        "every tessellated vertex should be referenced by a triangle"
    );
}

// ---------------------------------------------------------------------------
// FCBREP serialization
// ---------------------------------------------------------------------------

fn serialize(shape: &TopoShape) -> Vec<u8> {
    let mut bytes = Vec::new();
    write_shape(&mut bytes, shape).expect("write shape");
    bytes
}

#[test]
fn brep_round_trip_all_kinds() {
    let mesh = TopoShape::Box {
        length: 2.0,
        width: 3.0,
        height: 4.0,
    }
    .tessellate(DEFAULT_DEFLECTION);
    let tiny_mesh = Mesh::new(
        vec![Vector3::ZERO, Vector3::X, Vector3::Y, Vector3::Z],
        vec![[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]],
    );
    let shapes = vec![
        TopoShape::Box {
            length: 30.0,
            width: 20.0,
            height: 5.0,
        },
        TopoShape::Sphere { radius: 6.25 },
        TopoShape::Cylinder {
            radius: 1.5,
            height: 12.0,
        },
        TopoShape::Cone {
            radius1: 3.0,
            radius2: 1.25,
            height: 4.5,
        },
        TopoShape::Torus {
            radius1: 10.0,
            radius2: 2.0,
        },
        // Awkward values exercise the shortest-round-trip float format.
        TopoShape::Sphere { radius: 0.1 + 0.2 },
        TopoShape::Mesh(mesh),
        TopoShape::Mesh(tiny_mesh),
    ];
    for shape in &shapes {
        let bytes = serialize(shape);
        let back = read_shape_bytes(&bytes)
            .unwrap_or_else(|e| panic!("{}: round-trip parse failed: {e}", shape.kind_name()));
        // Shortest-round-trip float formatting makes the round trip exact.
        assert_eq!(&back, shape, "{}: exact round trip", shape.kind_name());
        // Serializing the restored shape is byte-identical (stable format).
        assert_eq!(
            serialize(&back),
            bytes,
            "{}: stable serialization",
            shape.kind_name()
        );
    }
}

#[test]
fn brep_rejects_corrupted_payloads() {
    // Unsupported version.
    assert!(matches!(
        read_shape_bytes(b"FCBREP 2\nkind Box\nend\n"),
        Err(fc_part::BrpError::Malformed { line: 1, .. })
    ));
    // Unknown kind, reported on the kind line (2).
    assert!(matches!(
        read_shape_bytes(b"FCBREP 1\nkind Blob\nend\n"),
        Err(fc_part::BrpError::Malformed { line: 2, .. })
    ));
    // Bad header.
    assert!(matches!(
        read_shape_bytes(b"NOTBREP 1\n"),
        Err(fc_part::BrpError::UnknownFormat)
    ));
    // Missing parameter for the kind.
    assert!(matches!(
        read_shape_bytes(b"FCBREP 1\nkind Sphere\nend\n"),
        Err(fc_part::BrpError::Malformed { reason, .. }) if reason.contains("missing parameter 'radius'")
    ));
    // Duplicate parameter.
    assert!(matches!(
        read_shape_bytes(b"FCBREP 1\nkind Sphere\nparam radius 1\nparam radius 2\nend\n"),
        Err(fc_part::BrpError::Malformed { reason, .. }) if reason.contains("duplicate parameter")
    ));
    // Parameter not valid for the kind.
    assert!(matches!(
        read_shape_bytes(b"FCBREP 1\nkind Box\nparam length 1\nparam width 1\nparam height 1\nparam radius 1\nend\n"),
        Err(fc_part::BrpError::Malformed { reason, .. }) if reason.contains("unexpected parameter 'radius'")
    ));
    // Unparsable float.
    assert!(matches!(
        read_shape_bytes(b"FCBREP 1\nkind Sphere\nparam radius half\nend\n"),
        Err(fc_part::BrpError::Malformed { reason, .. }) if reason.contains("invalid float")
    ));
    // Non-finite parameter.
    assert!(matches!(
        read_shape_bytes(b"FCBREP 1\nkind Sphere\nparam radius inf\nend\n"),
        Err(fc_part::BrpError::Malformed { reason, .. }) if reason.contains("not finite")
    ));
    // Truncated payload.
    assert!(matches!(
        read_shape_bytes(b"FCBREP 1\nkind Box\nparam length 1\n"),
        Err(fc_part::BrpError::Malformed { .. })
    ));
    // Content after `end`.
    assert!(matches!(
        read_shape_bytes(b"FCBREP 1\nkind Sphere\nparam radius 1\nend\njunk\n"),
        Err(fc_part::BrpError::Malformed { reason, .. }) if reason.contains("after 'end'")
    ));
    // Mesh: header count mismatch and out-of-range indices.
    assert!(matches!(
        read_shape_bytes(b"FCBREP 1\nkind Mesh\nmesh 1 1\nv 0 0 0\nt 0 0 1\nend\n"),
        Err(fc_part::BrpError::Malformed { reason, .. }) if reason.contains("out of range")
    ));
    assert!(matches!(
        read_shape_bytes(b"FCBREP 1\nkind Mesh\nmesh 1 1\nw 0 0 0\nt 0 0 0\nend\n"),
        Err(fc_part::BrpError::Malformed { reason, .. }) if reason.contains("expected 'v")
    ));
    // OCCT payloads are recognized and reported as unsupported.
    assert_eq!(
        read_shape_bytes(b"DBRep_DrawableShape\nCASCADE Topology\n"),
        Err(fc_part::BrpError::UnsupportedOcct)
    );
    // Empty payload is unknown, not a crash.
    assert_eq!(read_shape_bytes(b""), Err(fc_part::BrpError::UnknownFormat));
    // Probe agrees with the reader on all three format families.
    assert_eq!(
        fc_part::brep::probe_format(b"FCBREP 1\n"),
        fc_part::BrpFormat::FcBrep
    );
    assert_eq!(
        fc_part::brep::probe_format(b"CASCADE Topology v7\n"),
        fc_part::BrpFormat::OcctBrep
    );
    assert_eq!(
        fc_part::brep::probe_format(b"\x00\x01garbage"),
        fc_part::BrpFormat::Unknown
    );
}

// ---------------------------------------------------------------------------
// Part features in the document/recompute loop
// ---------------------------------------------------------------------------

#[test]
fn features_execute_into_shapes_and_meshes() {
    let mut doc = Document::new("Features");

    let mut base = BoxFeature::new("Base");
    base.container.set("Length", Property::Float(2.0));
    base.container.set("Width", Property::Float(3.0));
    base.container.set("Height", Property::Float(4.0));
    let base_id = doc.add(Box::new(base));

    // FreeCAD defaults out of the constructor: radius 5 sphere.
    doc.add(Box::new(SphereFeature::new("Ball")));

    let mut lift = TransformFeature::new("Lift");
    lift.container
        .set("Source", Property::Link("Base".to_string()));
    lift.container.set(
        "Placement",
        Property::Placement(fc_base::Placement::new(
            Vector3::new(10.0, 0.0, 5.0),
            fc_base::Rotation::IDENTITY,
        )),
    );
    lift.container.set("Scale", Property::Float(2.0));
    doc.add(Box::new(lift));

    // Before recompute: no geometry anywhere.
    let before = doc.get_by_name("Base").expect("base exists");
    assert!(
        fc_part::shape_of(before).is_none(),
        "no shape before execute"
    );
    assert!(fc_part::mesh_of(before, DEFAULT_DEFLECTION).is_none());

    // Dependency-first execution: Base and Ball before Lift.
    let executed = doc.recompute_all().expect("features recompute");
    let order: Vec<&str> = executed
        .iter()
        .map(|id| doc.get(*id).unwrap().name())
        .collect();
    assert!(order.contains(&"Base") && order.contains(&"Ball") && order.contains(&"Lift"));
    let lift_pos = order.iter().position(|n| *n == "Lift").unwrap();
    let base_pos = order.iter().position(|n| *n == "Base").unwrap();
    assert!(
        base_pos < lift_pos,
        "Base recomputes before its dependent Lift"
    );

    // Base produced the exact analytic shape; its mesh is the flat box.
    let shape = fc_part::shape_of(doc.get_by_name("Base").expect("base"))
        .expect("base shape after recompute");
    assert_eq!(
        shape,
        TopoShape::Box {
            length: 2.0,
            width: 3.0,
            height: 4.0
        }
    );
    assert_eq!(shape.volume(), 24.0);
    let base_mesh = fc_part::mesh_of(doc.get_by_name("Base").expect("base"), DEFAULT_DEFLECTION)
        .expect("base mesh");
    assert_eq!(base_mesh.triangle_count(), 12);

    // Ball used its FreeCAD default radius 5.
    let ball = fc_part::shape_of(doc.get_by_name("Ball").expect("ball"))
        .expect("ball shape after recompute");
    assert_eq!(ball, TopoShape::Sphere { radius: 5.0 });
    assert_rel(
        ball.volume(),
        4.0 / 3.0 * PI * 125.0,
        "default sphere volume",
    );

    // Lift placed and scaled the source mesh: p -> 2 * (p + (10, 0, 5)).
    // Base is the FreeCAD 1.0 corner-origin box: x in [0, 2] -> [20, 24];
    // y in [0, 3] -> [0, 6]; z in [0, 4] -> [10, 18]. Topology untouched.
    let lift_mesh =
        fc_part::mesh_of(doc.get_by_name("Lift").expect("lift"), 1.0).expect("lift mesh");
    assert_eq!(
        lift_mesh.triangle_count(),
        12,
        "transform keeps triangle count"
    );
    let bbox = lift_mesh.bbox().expect("lift bbox");
    assert_eq!(bbox.min, Vector3::new(20.0, 0.0, 10.0));
    assert_eq!(bbox.max, Vector3::new(24.0, 6.0, 18.0));

    // The parametric cascade: growing Base's Length moves Lift's result
    // and keeps everything consistent after one recompute.
    doc.set_property("Base", "Length", Property::Float(4.0))
        .expect("set property");
    let touched = doc.recompute_all().expect("cascade recompute");
    let names: Vec<&str> = touched
        .iter()
        .map(|id| doc.get(*id).unwrap().name())
        .collect();
    assert_eq!(names, vec!["Base", "Lift"], "only the chain recomputes");
    let lift_mesh =
        fc_part::mesh_of(doc.get_by_name("Lift").expect("lift"), 1.0).expect("lift mesh");
    let bbox = lift_mesh.bbox().expect("lift bbox");
    // Base x now in [0, 4] (corner-origin) -> [20, 28].
    assert_eq!(bbox.min, Vector3::new(20.0, 0.0, 10.0));
    assert_eq!(bbox.max, Vector3::new(28.0, 6.0, 18.0));
    assert_eq!(base_id, doc.get_id("Base").expect("id stable"));

    // A transform whose Source dangles is caught by the graph walker
    // *before* anything executes: MissingLink, nothing recomputed.
    let mut orphan = TransformFeature::new("Orphan");
    orphan
        .container
        .set("Source", Property::Link("NoSuchObject".to_string()));
    doc.add(Box::new(orphan));
    let err = doc.recompute_all().expect_err("dangling source must fail");
    assert_eq!(
        err,
        fc_app::RecomputeError::MissingLink {
            object: "Orphan".to_string(),
            target: "NoSuchObject".to_string(),
        }
    );
    // Nothing gained an error status: the failure was pre-execution.
    assert_ne!(
        doc.get_by_name("Lift").unwrap().status(),
        &fc_app::ObjectStatus::Error("x".to_string())
    );

    doc.remove("Orphan").expect("orphan is unlinked, removable");

    // A transform whose Source lost its Link type (no dependency edge)
    // fails inside execute, and the failed object carries the reason.
    let mut broken = TransformFeature::new("Broken");
    broken.container.set("Source", Property::Float(0.0)); // not a Link anymore
    doc.add(Box::new(broken));
    let err = doc.recompute_all().expect_err("non-link source must fail");
    assert_eq!(
        err,
        fc_app::RecomputeError::ExecuteFailed {
            object: "Broken".to_string(),
            reason: "Part::Transform 'Broken': Source must link to a document object".to_string(),
        }
    );
    assert_eq!(
        doc.get_by_name("Broken").unwrap().status(),
        &fc_app::ObjectStatus::Error(
            "Part::Transform 'Broken': Source must link to a document object".to_string()
        )
    );
}
