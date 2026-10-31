//! K4-2d/3 acceptance — the box×sphere bite taxonomy.
//!
//! One box (30×20×5 — the corpus' own `box.brp` dimensions) meets one
//! sphere at every configuration of the taxonomy, and the three
//! booleans read against **two independent references**: the
//! hand-derived closed forms (the octant/quarter/half shares of
//! `4πr³/3` and their area sums — checked against
//! [`brep_boolean::box_sphere_boolean`]'s own doc) and real OCCT's own
//! numbers (`expected_box_sphere.json`, authored by `BRepAlgoAPI` +
//! `BRepGProp` via `scripts/parity/regen_box_sphere_corpus.py`). The
//! two references agree to machine scale (≤ 4.6e-13), and the kernel
//! matches both: bit-exact at the corner and face stations, and to
//! ~2e-13 at the edge station (the rational-arc Green terms carry the
//! last-bit rounding — the fillet-box precedent).
//!
//! The exactness discipline: the FCBREP v2 round-trip measures
//! bit-identically, the classification outcomes (whole, empty,
//! refused) are asserted by name, and the render channel TESSELLATES
//! every constructive result watertight, within the deflection-derived
//! bound — the two-channel honesty contract with both channels now
//! answering (the trim-domain slice: the patch domains classify by
//! the wire traversal and tile under the sagitta discipline).

use std::collections::HashMap;
use std::f64::consts::PI;
use std::fs;

use fc_base::Vector3;
use fc_part::brep::read_brep_bytes;
use fc_part::brep::write_brep;
use fc_part::brep_boolean::{box_sphere_boolean, BooleanOp, BooleanOutcome, BooleanOutcomeKind};
use fc_part::measure::{brep_area, brep_volume};
use fc_part::tessellate_brep;

const O: Vector3 = Vector3::new(0.0, 0.0, 0.0);
const AX: Vector3 = Vector3::new(1.0, 0.0, 0.0);
const AY: Vector3 = Vector3::new(0.0, 1.0, 0.0);
const AZ: Vector3 = Vector3::new(0.0, 0.0, 1.0);
const L: f64 = 30.0;
const W: f64 = 20.0;
const H: f64 = 5.0;

fn v3(x: f64, y: f64, z: f64) -> Vector3 {
    Vector3::new(x, y, z)
}

/// The kernel outcome for the canonical box and a placed sphere.
fn run(op: BooleanOp, cs: Vector3, r: f64) -> BooleanOutcome {
    box_sphere_boolean(op, O, AX, AY, AZ, L, W, H, cs, r).expect("the kernel call itself")
}

/// The kernel outcome for a general box frame.
fn run_frame(
    op: BooleanOp,
    o: Vector3,
    a1: Vector3,
    a2: Vector3,
    a3: Vector3,
    cs: Vector3,
    r: f64,
) -> BooleanOutcome {
    box_sphere_boolean(op, o, a1, a2, a3, L, W, H, cs, r).expect("the kernel call itself")
}

fn solid(out: BooleanOutcome) -> fc_part::ExactBRep {
    match out.kind {
        BooleanOutcomeKind::Solid(b) => b,
        BooleanOutcomeKind::Empty => panic!("expected a solid, got the empty set"),
        BooleanOutcomeKind::Refused { why } => panic!("expected a solid, refused: {why}"),
    }
}

fn refused(out: BooleanOutcome) -> String {
    match out.kind {
        BooleanOutcomeKind::Refused { why } => why,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

/// OCCT's own numbers for one case of the corpus.
fn occt(case: &str) -> (f64, f64) {
    let text = fs::read_to_string(format!(
        "{}/tests/fixtures/brp/expected_box_sphere.json",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("missing expected_box_sphere.json");
    let value: serde_json::Value =
        serde_json::from_str(&text).expect("expected_box_sphere.json parses");
    let case = &value["cases"][case];
    (
        case["volume"].as_f64().expect("volume"),
        case["area"].as_f64().expect("area"),
    )
}

/// One constructive case: the solid's measures against the closed
/// form and OCCT, the face count against OCCT's own topology, the
/// FCBREP round-trip bit-identical, and the render channel
/// tessellating watertight within the deflection bound (the
/// two-channel contract — both channels now answering).
#[allow(clippy::too_many_arguments)] // the case's own vocabulary
fn acceptance(
    name: &str,
    case: &str,
    op: BooleanOp,
    cs: Vector3,
    r: f64,
    want_v: f64,
    want_a: f64,
    faces: usize,
) {
    let brep = solid(run(op, cs, r));
    assert_eq!(brep.faces.len(), faces, "{name}: the face count");
    let v = brep_volume(&brep).unwrap_or_else(|e| panic!("{name}: volume refused: {e}"));
    let a = brep_area(&brep).unwrap_or_else(|e| panic!("{name}: area refused: {e}"));
    let dv = (v - want_v).abs();
    let da = (a - want_a).abs();
    assert!(
        dv <= 1e-9 * want_v.abs().max(1.0),
        "{name}: volume {v:.12} vs the closed form {want_v:.12} (d {dv:.3e})"
    );
    assert!(
        da <= 1e-9 * want_a.abs().max(1.0),
        "{name}: area {a:.12} vs the closed form {want_a:.12} (d {da:.3e})"
    );
    let (ov, oa) = occt(case);
    assert!(
        (v - ov).abs() <= 5e-9 * ov.abs().max(1.0),
        "{name}: volume {v:.12} vs OCCT {ov:.12}"
    );
    assert!(
        (a - oa).abs() <= 5e-9 * oa.abs().max(1.0),
        "{name}: area {a:.12} vs OCCT {oa:.12}"
    );
    // The FCBREP v2 round-trip: bit-identical measures.
    let mut bytes = Vec::new();
    write_brep(&mut bytes, &brep).expect("writes FCBREP v2");
    let round = read_brep_bytes(&bytes).expect("reads back");
    let rv = brep_volume(&round).expect("the round-trip measures");
    assert!(
        (rv - v).abs() <= 1e-12 * v.abs().max(1.0),
        "{name}: round-trip volume {rv:.12} vs {v:.12}"
    );
    // The render channel: the trim-domain slice's own arm — the patch
    // domains (the spherical polygon, the cap, the complement, the
    // pole-run fan) classify by the wire's traversal and tile under
    // the sagitta discipline, watertight with the mesh volume within
    // the deflection-derived bound. Every constructive result of this
    // family renders — the pole-crossing patches (the edge station's
    // quarter ball, either side) through the pole-run fan.
    let mesh = tessellate_brep(&brep, 0.25)
        .unwrap_or_else(|e| panic!("{name}: the render channel tessellates: {e}"));
    let mut directed: HashMap<(u32, u32), usize> = HashMap::new();
    for t in &mesh.triangles {
        for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
            *directed.entry((a, b)).or_default() += 1;
        }
    }
    for (&(a, b), &n) in &directed {
        assert!(
            n == 1 && directed.get(&(b, a)).copied() == Some(1),
            "{name}: the mesh is watertight ({a}→{b} used {n} times)"
        );
    }
    let bound = 4.0 * PI * r * r * 0.25;
    assert!(
        (mesh.volume() - v).abs() <= bound,
        "{name}: the mesh volume {:.6} vs the exact {v:.6} — the deflection bound is                  {bound:.6}",
        mesh.volume()
    );
}

/// The corner station: the octant bite behind the corner at the
/// origin — all three booleans, the closed forms, OCCT's own numbers.
#[test]
fn corner_bite_measures_the_closed_forms_and_occt() {
    // V = 3000 ∓ (1/8 or 7/8)·(4/3)πr³; A = 1700 − 3·(πr²/4) plus the
    // patch share (πr²/2 octant, 7πr²/2 complement).
    acceptance(
        "corner cut",
        "box_sphere_cut_corner",
        BooleanOp::Cut,
        v3(0.0, 0.0, 0.0),
        3.0,
        3000.0 - 4.5 * PI,
        1700.0 - 9.0 * PI / 4.0,
        7,
    );
    acceptance(
        "corner fuse",
        "box_sphere_fuse_corner",
        BooleanOp::Fuse,
        v3(0.0, 0.0, 0.0),
        3.0,
        3000.0 + 31.5 * PI,
        1700.0 + 99.0 * PI / 4.0,
        7,
    );
    acceptance(
        "corner common",
        "box_sphere_common_corner",
        BooleanOp::Common,
        v3(0.0, 0.0, 0.0),
        3.0,
        4.5 * PI,
        45.0 * PI / 4.0,
        4,
    );
}

/// The edge station: the quarter ball behind the edge point (12, 0, 0)
/// — the area of the cut is the box's own (two half-disks out, the
/// quarter patch in). The patch's boundary arc crosses the sphere's
/// pole (a meridian jump of exactly half a period) — the pole-run fan
/// renders it watertight, both the patch and its complement.
#[test]
fn edge_bite_measures_the_closed_forms_and_occt() {
    acceptance(
        "edge cut",
        "box_sphere_cut_edge",
        BooleanOp::Cut,
        v3(12.0, 0.0, 0.0),
        3.0,
        3000.0 - 9.0 * PI,
        1700.0,
        7,
    );
    acceptance(
        "edge fuse",
        "box_sphere_fuse_edge",
        BooleanOp::Fuse,
        v3(12.0, 0.0, 0.0),
        3.0,
        3000.0 + 27.0 * PI,
        1700.0 + 18.0 * PI,
        7,
    );
    acceptance(
        "edge common",
        "box_sphere_common_edge",
        BooleanOp::Common,
        v3(12.0, 0.0, 0.0),
        3.0,
        9.0 * PI,
        18.0 * PI,
        3,
    );
}

/// The face station: the half ball behind the interior circle at
/// (15, 10, 0) — the disk removed, the hemisphere added; the cut and
/// the fuse share one area.
#[test]
fn face_bite_measures_the_closed_forms_and_occt() {
    acceptance(
        "face cut",
        "box_sphere_cut_face",
        BooleanOp::Cut,
        v3(15.0, 10.0, 0.0),
        2.0,
        3000.0 - 16.0 * PI / 3.0,
        1700.0 + 4.0 * PI,
        7,
    );
    acceptance(
        "face fuse",
        "box_sphere_fuse_face",
        BooleanOp::Fuse,
        v3(15.0, 10.0, 0.0),
        2.0,
        3000.0 + 16.0 * PI / 3.0,
        1700.0 + 4.0 * PI,
        7,
    );
    acceptance(
        "face common",
        "box_sphere_common_face",
        BooleanOp::Common,
        v3(15.0, 10.0, 0.0),
        2.0,
        16.0 * PI / 3.0,
        12.0 * PI,
        2,
    );
}

/// The classification outcomes: the contained, disjoint, and engulfed
/// configurations answer with whole shapes, the empty set, or the
/// family-wide named refusals.
#[test]
fn classification_wholes_empties_and_refusals() {
    // Contained (the ball strictly inside): the void cut refuses by
    // name; fuse is the box; common is the sphere.
    let why = refused(run(BooleanOp::Cut, v3(15.0, 10.0, 2.5), 1.5));
    assert!(
        why.contains("contained cut") && why.contains("second shell"),
        "the void refusal names itself: {why}"
    );
    let fuse = solid(run(BooleanOp::Fuse, v3(15.0, 10.0, 2.5), 1.5));
    assert_eq!(brep_volume(&fuse).unwrap(), 3000.0);
    assert_eq!(brep_area(&fuse).unwrap(), 1700.0);
    let common = solid(run(BooleanOp::Common, v3(15.0, 10.0, 2.5), 1.5));
    assert_eq!(brep_volume(&common).unwrap(), 4.5 * PI);
    assert_eq!(brep_area(&common).unwrap(), 9.0 * PI);

    // Disjoint: the cut is the whole box; the common is empty; the
    // fuse refuses (a two-shell compound).
    let cut = solid(run(BooleanOp::Cut, v3(-10.0, -10.0, -10.0), 5.0));
    assert_eq!(brep_volume(&cut).unwrap(), 3000.0);
    assert!(matches!(
        run(BooleanOp::Common, v3(-10.0, -10.0, -10.0), 5.0).kind,
        BooleanOutcomeKind::Empty
    ));
    let why = refused(run(BooleanOp::Fuse, v3(-10.0, -10.0, -10.0), 5.0));
    assert!(
        why.contains("disjoint fuse") && why.contains("two-shell"),
        "the disjoint-fuse refusal names itself: {why}"
    );

    // Engulfed (the box inside the ball): the cut is empty; the fuse
    // is the whole sphere; the common is the whole box.
    assert!(matches!(
        run(BooleanOp::Cut, v3(15.0, 10.0, 2.5), 50.0).kind,
        BooleanOutcomeKind::Empty
    ));
    let fuse = solid(run(BooleanOp::Fuse, v3(15.0, 10.0, 2.5), 50.0));
    assert_eq!(brep_volume(&fuse).unwrap(), 4.0 * PI * 125000.0 / 3.0);
    let common = solid(run(BooleanOp::Common, v3(15.0, 10.0, 2.5), 50.0));
    assert_eq!(brep_volume(&common).unwrap(), 3000.0);
}

/// The honest boundaries, each refusing by name: the center off the
/// skeleton, the interior crossing, the bite beyond its faces, and
/// the near-but-not-exact station (never a silent snap).
#[test]
fn unsupported_configurations_refuse_by_name() {
    // The center outside the slabs near a corner: a general overlap.
    let why = refused(run(BooleanOp::Cut, v3(-1.0, 3.0, 2.0), 4.0));
    assert!(
        why.contains("off the box's boundary skeleton"),
        "the off-skeleton refusal names itself: {why}"
    );
    // The center interior, the ball crossing the faces.
    let why = refused(run(BooleanOp::Cut, v3(15.0, 10.0, 2.5), 4.0));
    assert!(
        why.contains("interior center"),
        "the interior-crossing refusal names itself: {why}"
    );
    // The corner bite beyond the smallest extent (r > 5).
    let why = refused(run(BooleanOp::Cut, v3(0.0, 0.0, 0.0), 6.0));
    assert!(
        why.contains("leaves the box's adjacent faces"),
        "the oversized corner bite refuses by name: {why}"
    );
    // The edge bite running past the edge's end.
    let why = refused(run(BooleanOp::Cut, v3(1.0, 0.0, 0.0), 3.0));
    assert!(
        why.contains("leaves its adjacent faces or the edge span"),
        "the oversized edge bite refuses by name: {why}"
    );
    // The face bite's circle crossing a face edge.
    let why = refused(run(BooleanOp::Cut, v3(1.0, 10.0, 0.0), 2.0));
    assert!(
        why.contains("leaves the face"),
        "the oversized face bite refuses by name: {why}"
    );
    // The near-but-not-exact corner: 1e-12 off the corner point —
    // within recognition tolerance, beyond bit-exactness, refused
    // rather than snapped.
    let why = refused(run(BooleanOp::Cut, v3(1e-12, 0.0, 0.0), 3.0));
    assert!(
        why.contains("never silently snaps"),
        "the near-station refusal names the discipline: {why}"
    );
}

/// The mirror stations: the opposite corner (the interior frame's
/// handedness flips), an edge on another axis pair, and a rotated box
/// frame — the construction is frame-general and the closed forms
/// follow the geometry, not the axis labels.
#[test]
fn mirror_stations_and_rotated_frames_measure_the_same_forms() {
    // The FAR corner (30, 20, 5): the same octant share.
    let brep = solid(run(BooleanOp::Cut, v3(30.0, 20.0, 5.0), 3.0));
    assert!(
        (brep_volume(&brep).unwrap() - (3000.0 - 4.5 * PI)).abs() <= 1e-9,
        "the far-corner cut measures the octant form"
    );
    // An edge on the y-axis pair at (0, 12, 0) — the MIRRORED frame
    // handedness (the parity the slice-7 audit found latent-broken in
    // the edge station's cut/fuse walks: the wrong corner order left
    // the detour legs running diagonally across the face, the shell
    // validator catching it as an error — never a wrong solid). All
    // three ops now construct the quarter forms on that parity.
    let brep = solid(run(BooleanOp::Common, v3(0.0, 12.0, 0.0), 3.0));
    assert!(
        (brep_volume(&brep).unwrap() - 9.0 * PI).abs() <= 1e-9,
        "the y-edge common measures the quarter form"
    );
    let brep = solid(run(BooleanOp::Cut, v3(0.0, 12.0, 0.0), 3.0));
    assert!(
        (brep_volume(&brep).unwrap() - (3000.0 - 9.0 * PI)).abs() <= 1e-9,
        "the y-edge cut measures the quarter form (the mirrored parity)"
    );
    let a = brep_area(&brep).unwrap();
    assert!(
        (a - (1700.0 - 9.0 * PI + 9.0 * PI)).abs() <= 1e-9,
        "the y-edge cut area measures the notched form ({a})"
    );
    let brep = solid(run(BooleanOp::Fuse, v3(0.0, 12.0, 0.0), 3.0));
    assert!(
        (brep_volume(&brep).unwrap() - (3000.0 + 27.0 * PI)).abs() <= 1e-9,
        "the y-edge fuse measures the quarter complement form (the mirrored parity)"
    );
    // A rotated box (a quarter turn about z, the corner at (5, −3)):
    // axes (ŷ, −x̂, ẑ), the same dimensions, the corner station at
    // the rotated origin.
    let o = v3(5.0, -3.0, 0.0);
    let (ry, neg_rx, z) = (AY, v3(-1.0, 0.0, 0.0), AZ);
    let out = run_frame(BooleanOp::Cut, o, ry, neg_rx, z, o, 3.0);
    let brep = solid(out);
    assert!(
        (brep_volume(&brep).unwrap() - (3000.0 - 4.5 * PI)).abs() <= 1e-9,
        "the rotated-frame corner cut measures the octant form"
    );
    // A face station in the rotated frame: the center ten units
    // along +ŷ and ten along −x̂ from the corner (the rotated in-face
    // station), r=2 — well inside the face.
    let cs = v3(5.0 - 10.0, -3.0 + 10.0, 0.0);
    let out = run_frame(BooleanOp::Common, o, ry, neg_rx, z, cs, 2.0);
    let brep = solid(out);
    assert!(
        (brep_volume(&brep).unwrap() - 16.0 * PI / 3.0).abs() <= 1e-9,
        "the rotated-frame face common measures the half-ball form"
    );
    // The crossing-disk edge bite in the rotated frame: the center one
    // unit along +ŷ and 1.5 up ẑ from the corner, ten along −x̂ —
    // depths (1, 1.5) from the +ŷ and +ẑ faces (the shared edge along
    // −x̂), r = 2 — the same crossing-disk lens forms as the
    // axis-aligned corpus case.
    let cs2 = v3(5.0 - 10.0, -3.0 + 1.0, 1.5);
    let lens_v = {
        // The hand form (the same derivation as the acceptance test,
        // the depths swapped to the rotated faces): the footprints,
        // the lens, the divergence identity.
        let (r, a, b) = (2.0f64, 1.0f64, 1.5f64);
        let rho_p = (r * r - a * a).sqrt();
        let rho_q = (r * r - b * b).sqrt();
        let h = (r * r - a * a - b * b).sqrt();
        let seg = |rho: f64, cut: f64| {
            rho * rho * (cut / rho).acos() - cut * (rho * rho - cut * cut).sqrt()
        };
        let a_fp = PI * rho_p * rho_p - seg(rho_p, b);
        let a_fq = PI * rho_q * rho_q - seg(rho_q, a);
        // The tangents in the rotated frame's inward directions
        // (u_p = +ŷ, u_q = +ẑ, a_e = −x̂) — the same component pattern.
        let (upx, upy, upz) = (0.0, 1.0, 0.0);
        let (uqx, uqy, uqz) = (0.0, 0.0, 1.0);
        let (aex, aey, aez) = (-1.0, 0.0, 0.0);
        let unit = |(x, y, z): (f64, f64, f64)| {
            let n = (x * x + y * y + z * z).sqrt();
            (x / n, y / n, z / n)
        };
        let dot = |u: (f64, f64, f64), v: (f64, f64, f64)| u.0 * v.0 + u.1 * v.1 + u.2 * v.2;
        let cross = |u: (f64, f64, f64), v: (f64, f64, f64)| {
            (
                u.1 * v.2 - u.2 * v.1,
                u.2 * v.0 - u.0 * v.2,
                u.0 * v.1 - u.1 * v.0,
            )
        };
        let tau = |t_in: (f64, f64, f64), t_out: (f64, f64, f64), p: (f64, f64, f64)| {
            let v_hat = unit(p);
            dot(v_hat, cross(t_in, t_out)).atan2(dot(t_in, t_out))
        };
        // The crossing points P± in WORLD coordinates (the basis
        // tuple spelling would only coincide with the axis-aligned
        // frame's).
        let p_m = (
            -a * upx - b * uqx - h * aex,
            -a * upy - b * uqy - h * aey,
            -a * upz - b * uqz - h * aez,
        );
        let p_p = (
            -a * upx - b * uqx + h * aex,
            -a * upy - b * uqy + h * aey,
            -a * upz - b * uqz + h * aez,
        );
        let tau_m = tau(
            unit((h * uqx - b * aex, h * uqy - b * aey, h * uqz - b * aez)),
            unit((a * aex - h * upx, a * aey - h * upy, a * aez - h * upz)),
            p_m,
        );
        let tau_p = tau(
            unit((a * aex + h * upx, a * aey + h * upy, a * aez + h * upz)),
            unit((
                -(b * aex + h * uqx),
                -(b * aey + h * uqy),
                -(b * aez + h * uqz),
            )),
            p_p,
        );
        let dphi_p = PI + 2.0 * b.atan2(h);
        let dphi_q = PI + 2.0 * a.atan2(h);
        let a_lens = r * r * (2.0 * PI - tau_m - tau_p + a / r * dphi_p + b / r * dphi_q);
        (a * a_fp + b * a_fq + r * a_lens) / 3.0
    };
    let out = run_frame(BooleanOp::Common, o, ry, neg_rx, z, cs2, 2.0);
    let brep = solid(out);
    assert!(
        (brep_volume(&brep).unwrap() - lens_v).abs() <= 1e-9,
        "the rotated-frame crossing-disk common measures the lens form"
    );
    let out = run_frame(BooleanOp::Cut, o, ry, neg_rx, z, cs2, 2.0);
    let brep = solid(out);
    assert!(
        (brep_volume(&brep).unwrap() - (3000.0 - lens_v)).abs() <= 1e-9,
        "the rotated-frame crossing-disk cut measures the notched form"
    );
}

/// The watertight discipline on the whole-shape channel: the
/// whole-box and whole-sphere classification outcomes tessellate
/// watertight (the plain primitives) — the constructive bites are
/// covered by every `acceptance` call above.
#[test]
fn render_channel_is_watertight_on_the_whole_shapes() {
    let watertight = |brep: &fc_part::ExactBRep| {
        let mesh = tessellate_brep(brep, 0.25).expect("the plain solids tessellate");
        let mut directed: HashMap<(u32, u32), usize> = HashMap::new();
        for t in &mesh.triangles {
            for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                *directed.entry((a, b)).or_default() += 1;
            }
        }
        for (&(a, b), &n) in &directed {
            assert!(
                n == 1 && directed.get(&(b, a)).copied() == Some(1),
                "the mesh is watertight ({a}→{b} used {n} times)"
            );
        }
    };
    let cut = solid(run(BooleanOp::Cut, v3(-10.0, -10.0, -10.0), 5.0));
    watertight(&cut);
    let fuse = solid(run(BooleanOp::Fuse, v3(15.0, 10.0, 2.5), 50.0));
    watertight(&fuse);
}

/// The spherical-polygon measure itself: the octant patch and its
/// complement on one sphere — the excess area and the vector-area
/// flux against the closed forms, including the reversed-face region
/// choice (Cut vs Fuse on the same wire).
#[test]
fn spherical_polygon_reads_the_octant_and_complement() {
    // The corner Common (the octant ball) and Fuse (the 7/8 bulge)
    // share the same arc wire geometry on the patch; the `reversed`
    // flag picks the region.
    let r = 3.0;
    let octant = solid(run(BooleanOp::Common, v3(0.0, 0.0, 0.0), r));
    let bulge = solid(run(BooleanOp::Fuse, v3(0.0, 0.0, 0.0), r));
    let v_oct = brep_volume(&octant).unwrap();
    let a_oct = brep_area(&octant).unwrap();
    let v_bul = brep_volume(&bulge).unwrap();
    let a_bul = brep_area(&bulge).unwrap();
    assert!((v_oct - 4.5 * PI).abs() < 1e-9, "the octant volume");
    assert!((a_oct - 45.0 * PI / 4.0).abs() < 1e-9, "the octant area");
    assert!(
        (v_bul - (3000.0 + 31.5 * PI)).abs() < 1e-9,
        "the bulge volume"
    );
    assert!(
        (a_bul - (1700.0 + 99.0 * PI / 4.0)).abs() < 1e-9,
        "the bulge area"
    );
    // The octant patch + the complement patch = the whole sphere:
    // the octant's area minus its three quarter-disk faces is πr²/2;
    // the fuse's area minus the three bitten faces (each having lost
    // its quarter-disk) is 7πr²/2; the sum is 4πr².
    let octant_patch = 45.0 * PI / 4.0 - 3.0 * (PI * r * r / 4.0);
    let bulge_patch = (1700.0 + 99.0 * PI / 4.0) - (1700.0 - 27.0 * PI / 4.0);
    assert!(
        (octant_patch + bulge_patch - 4.0 * PI * r * r).abs() < 1e-9,
        "the patch areas partition the sphere"
    );
}

/// The offset face stations (K4-2d/3 slice 2): the center at depth
/// δ inside the box, the ball crossing exactly ONE face — the bite is
/// the small-circle cap. Both sides (the low face and the mirrored
/// high face), all three ops, against the closed forms AND real
/// OCCT's own numbers, the FCBREP round-trip, and the render refusal.
#[test]
fn offset_face_bite_measures_the_closed_forms_and_occt() {
    let r = 2.0;
    let delta = 1.5;
    let rho2 = r * r - delta * delta; // 1.75
                                      // The cap closed forms: deep (height δ + r), shallow (r − δ).
    let deep_v = PI * (delta + r) * (delta + r) * (2.0 * r - delta) / 3.0;
    let shallow_v = PI * (r - delta) * (r - delta) * (2.0 * r + delta) / 3.0;
    let deep_a = 2.0 * PI * r * (delta + r);
    let shallow_a = 2.0 * PI * r * (r - delta);
    let disk_a = PI * rho2;
    // The low face: the center at (15, 10, 1.5).
    let cs_lo = v3(15.0, 10.0, 1.5);
    // The mirrored high face: (15, 10, 3.5) — δ = 5 − 3.5.
    let cs_hi = v3(15.0, 10.0, 3.5);
    for (tag, cs) in [("low", cs_lo), ("high", cs_hi)] {
        acceptance(
            &format!("offset-{tag} cut"),
            "box_sphere_cut_face_offset",
            BooleanOp::Cut,
            cs,
            r,
            3000.0 - deep_v,
            1700.0 - disk_a + deep_a,
            7,
        );
        acceptance(
            &format!("offset-{tag} fuse"),
            "box_sphere_fuse_face_offset",
            BooleanOp::Fuse,
            cs,
            r,
            3000.0 + shallow_v,
            1700.0 - disk_a + shallow_a,
            7,
        );
        acceptance(
            &format!("offset-{tag} common"),
            "box_sphere_common_face_offset",
            BooleanOp::Common,
            cs,
            r,
            deep_v,
            disk_a + deep_a,
            2,
        );
    }
}

/// The offset boundaries, refusing by name: a ball crossing TWO
/// faces has a construction only when the two are ADJACENT and their
/// section disks CROSS each other's planes — the through-going slab
/// span, the non-crossing separate sections, the exact tangency, the
/// corner-ward three-face overlap, and the along-edge pole touch each
/// refuse with the reason, never a guess.
#[test]
fn offset_multi_face_crossings_refuse_by_name() {
    // The center mid-height, r spanning the slab: bottom AND top —
    // the through-going two-mouth section on one axis.
    let why = refused(run(BooleanOp::Cut, v3(15.0, 10.0, 2.5), 4.0));
    assert!(
        why.contains("crosses both faces of the box's slab"),
        "the through-slab crossing names itself: {why}"
    );
    // Two adjacent crossings whose disks do NOT cross (a² + b² =
    // 1 + 2.25 = 3.25 > r² = 3.24): the separate two-mouth sections.
    let why = refused(run(BooleanOp::Cut, v3(1.0, 10.0, 1.5), 1.8));
    assert!(
        why.contains("do not cross each other's planes"),
        "the non-crossing two-face case names itself: {why}"
    );
    // The exact tangency a² + b² = r² — with exactly representable
    // operands (0.75² + 1² = 0.5625 + 1 = 1.5625 = 1.25²): the
    // degenerate lens, refused, never snapped to the edge bite.
    let why = refused(run(BooleanOp::Cut, v3(0.75, 10.0, 1.0), 1.25));
    assert!(
        why.contains("tangent on the shared box edge"),
        "the tangent disks name the degeneracy: {why}"
    );
    // Three crossings near a corner: the corner-ward general overlap.
    let why = refused(run(BooleanOp::Cut, v3(1.0, 1.0, 1.5), 2.0));
    assert!(
        why.contains("crosses 3 of the box's faces"),
        "the three-face crossing names its count: {why}"
    );
    // The along-edge pole tangency: the lens spans ±r along the edge,
    // and d(e) == r puts the pole exactly ON the end face.
    let why = refused(run(BooleanOp::Cut, v3(1.0, 2.0, 1.5), 2.0));
    assert!(
        why.contains("touches the box's along-edge face at the sphere's pole"),
        "the pole tangency names itself: {why}"
    );
}

/// The crossing-disk edge bite (K4-2d/3 slice 7): the center inside
/// at depths (a, b) = (1, 1.5) from two adjacent faces, r = 2 — the
/// section disks cross each other's planes (a² + b² = 3.25 < 4), the
/// bite wraps the shared edge. Both edges (the low-low and the
/// mirrored low-high — the two frame handednesses), all three ops,
/// against the hand-derived closed forms AND real OCCT's own numbers.
///
/// The hand forms, derived from the input parameters alone (the box's
/// inward frame, the sphere's center as the origin; the two corpus
/// cases share them — the frame parities cancel in every term):
///
/// * the **footprints** — each face's section disk (ρ_p = √(r² − a²))
///   clipped by the other face's plane into the disk minus the
///   circular segment beyond a chord at the other depth:
///   `A_fp = πρ_p² − (ρ_p²·arccos(b/ρ_p) − b·√(ρ_p² − b²))`;
/// * the **lens** `{p ≥ −a, q ≥ −b}` — Gauss–Bonnet with the
///   small-circle geodesic curvature, the walk the construction
///   authors (the region on the left viewed from outside; each
///   section plane sits at offset −depth along its walk's CCW axis,
///   so the curvature integrals are negative and the depths enter
///   positively): `A_lens = r²(2π − τ₋ − τ₊ + (a/r)·Δφ_p +
///   (b/r)·Δφ_q)` with the MAJOR-arc sweeps `Δφ_p = π + 2·atan2(b,
///   h)`, `Δφ_q = π + 2·atan2(a, h)` (`h = √(r² − a² − b²)` — the
///   half-chord on the shared edge), and the exterior turns at the
///   crossing points P± from the circles' CCW-around-axis tangents
///   `â × r̂` there;
/// * the **volume** — the divergence identity over the bite's own
///   boundary: `V_bite = (a·A_fp + b·A_fq + r·A_lens)/3` (the flat
///   faces at constant `p·n̂` = the depths, the sphere at `p·n̂ = r`).
#[test]
fn edge_offset_bite_measures_the_closed_forms_and_occt() {
    let r = 2.0;
    let (a, b): (f64, f64) = (1.0, 1.5); // the depths from the x-face and the z-face
    let rho_p = (r * r - a * a).sqrt(); // the section circle on the x-face
    let rho_q = (r * r - b * b).sqrt(); // the section circle on the z-face
    let h = (r * r - a * a - b * b).sqrt(); // the half-chord along the edge
    let seg =
        |rho: f64, cut: f64| rho * rho * (cut / rho).acos() - cut * (rho * rho - cut * cut).sqrt();
    let a_fp = PI * rho_p * rho_p - seg(rho_p, b); // the x-face footprint
    let a_fq = PI * rho_q * rho_q - seg(rho_q, a); // the z-face footprint
                                                   // The junction tangents in the box's inward frame (u_p = x̂, u_q =
                                                   // ẑ, a_e = ŷ): at P− the arc_p arrival (h·u_q − b·a_e)/ρ_p and the
                                                   // arc_q departure (a·a_e − h·u_p)/ρ_q; at P+ the arc_q arrival
                                                   // (a·a_e + h·u_p)/ρ_q and the arc_p departure −(b·a_e + h·u_q)/ρ_p.
    let (u_p, u_q, a_e) = (AX, AZ, AY);
    let p_m = v3(-a, -h, -b);
    let p_p = v3(-a, h, -b);
    let unit = |v: Vector3| v * (1.0 / v.length());
    let tau = |t_in: Vector3, t_out: Vector3, p: Vector3| {
        let v_hat = unit(p);
        v_hat.dot(&t_in.cross(&t_out)).atan2(t_in.dot(&t_out))
    };
    let t_pm_in = unit(v3(
        h * u_q.x - b * a_e.x,
        h * u_q.y - b * a_e.y,
        h * u_q.z - b * a_e.z,
    ));
    let t_pm_out = unit(v3(
        a * a_e.x - h * u_p.x,
        a * a_e.y - h * u_p.y,
        a * a_e.z - h * u_p.z,
    ));
    let tau_m = tau(t_pm_in, t_pm_out, p_m);
    let t_pp_in = unit(v3(
        a * a_e.x + h * u_p.x,
        a * a_e.y + h * u_p.y,
        a * a_e.z + h * u_p.z,
    ));
    let t_pp_out = unit(v3(
        -(b * a_e.x + h * u_q.x),
        -(b * a_e.y + h * u_q.y),
        -(b * a_e.z + h * u_q.z),
    ));
    let tau_p = tau(t_pp_in, t_pp_out, p_p);
    let dphi_p = PI + 2.0 * b.atan2(h);
    let dphi_q = PI + 2.0 * a.atan2(h);
    let a_lens = r * r * (2.0 * PI - tau_m - tau_p + a / r * dphi_p + b / r * dphi_q);
    // The divergence identity and the area sums.
    let v_bite = (a * a_fp + b * a_fq + r * a_lens) / 3.0;
    let ball_v = 4.0 * PI * r * r * r / 3.0;
    let ball_a = 4.0 * PI * r * r;
    let box_a = 2.0 * (30.0 * 20.0 + 30.0 * 5.0 + 20.0 * 5.0);
    // The low-low edge: the center at (1, 10, 1.5) — depths (1, 1.5)
    // from x=0 and z=0 (one frame handedness); the mirrored low-high
    // edge: the center at (1, 10, 3.5) — depths (1, 1.5) from x=0 and
    // z=5 (the other). Both construct the same closed forms.
    for (tag, cs) in [
        ("low-low", v3(1.0, 10.0, 1.5)),
        ("low-high", v3(1.0, 10.0, 3.5)),
    ] {
        acceptance(
            &format!("edge-offset-{tag} cut"),
            "box_sphere_cut_edge_offset",
            BooleanOp::Cut,
            cs,
            r,
            3000.0 - v_bite,
            box_a - a_fp - a_fq + a_lens,
            7,
        );
        acceptance(
            &format!("edge-offset-{tag} fuse"),
            "box_sphere_fuse_edge_offset",
            BooleanOp::Fuse,
            cs,
            r,
            3000.0 + ball_v - v_bite,
            box_a - a_fp - a_fq + ball_a - a_lens,
            7,
        );
        acceptance(
            &format!("edge-offset-{tag} common"),
            "box_sphere_common_edge_offset",
            BooleanOp::Common,
            cs,
            r,
            v_bite,
            a_fp + a_fq + a_lens,
            3,
        );
    }
}

/// The lens mesh's approximation is deflection-true in the referee's
/// own sense: the crossing-disk bite's chord-and-panel deficit shrinks
/// with the deflection — quartering the deflection quarters the volume
/// error (the chord sagitta is linear in the deflection), the same
/// discipline as the octant patch and the pole-run fan.
#[test]
fn lens_mesh_volume_error_shrinks_with_deflection() {
    let bite = solid(run(BooleanOp::Common, v3(1.0, 10.0, 1.5), 2.0));
    let exact = brep_volume(&bite).unwrap();
    let err = |d: f64| {
        (tessellate_brep(&bite, d)
            .expect("the render channel")
            .volume()
            - exact)
            .abs()
    };
    let coarse = err(0.25);
    let fine = err(0.0625);
    assert!(
        coarse > 0.0,
        "the coarse lens mesh sits strictly off the exact (the chords bite)"
    );
    assert!(
        fine <= coarse / 3.0,
        "quartering the deflection must shrink the deficit ({fine:.6} vs {coarse:.6})"
    );
}

/// The patch mesh's approximation is deflection-true in the
/// discipline's own sense: the octant ball's chord-and-panel deficit
/// shrinks with the deflection — quartering the deflection quarters
/// the volume error (the chord sagitta is linear in the deflection).
#[test]
fn patch_mesh_volume_error_shrinks_with_deflection() {
    let octant = solid(run(BooleanOp::Common, v3(0.0, 0.0, 0.0), 3.0));
    let exact = brep_volume(&octant).unwrap();
    let err = |d: f64| {
        (tessellate_brep(&octant, d)
            .expect("the render channel")
            .volume()
            - exact)
            .abs()
    };
    let coarse = err(0.25);
    let fine = err(0.0625);
    assert!(
        coarse > 0.0,
        "the coarse patch mesh sits strictly off the exact (the chords bite)"
    );
    assert!(
        fine <= coarse / 3.0,
        "quartering the deflection must shrink the deficit ({fine:.6} vs {coarse:.6})"
    );
}

/// The pole-run fan's approximation is deflection-true the same way:
/// the edge station's quarter ball (the patch whose boundary arc
/// crosses the pole) carries the same shrinking deficit — the fan's
/// refinement is the referee's own discipline, not a fixed coarseness.
#[test]
fn pole_fan_mesh_volume_error_shrinks_with_deflection() {
    let quarter = solid(run(BooleanOp::Common, v3(12.0, 0.0, 0.0), 3.0));
    let exact = brep_volume(&quarter).unwrap();
    let err = |d: f64| {
        (tessellate_brep(&quarter, d)
            .expect("the render channel")
            .volume()
            - exact)
            .abs()
    };
    let coarse = err(0.25);
    let fine = err(0.0625);
    assert!(
        coarse > 0.0,
        "the coarse fan mesh sits strictly off the exact (the chords bite)"
    );
    assert!(
        fine <= coarse / 3.0,
        "quartering the deflection must shrink the fan's deficit ({fine:.6} vs {coarse:.6})"
    );
}
