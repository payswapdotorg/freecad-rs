//! K4-2d/3 slice 4 acceptance — the box×cylinder station taxonomy.
//!
//! One box (30×20×5 — the corpus' own `box.brp` dimensions) meets an
//! axis-parallel cylinder at every station of the
//! interior-cross-section taxonomy, and the three booleans read
//! against **two independent references**: the hand-derived closed
//! forms (`V_box − πr²·depth` for the bores, `V_box + πr²·overhang`
//! for the bosses and axles, the per-face area sums — every annulus
//! a rectangle minus its strictly-interior disk, every wall a
//! `2πrh` band) and real OCCT's own numbers
//! (`expected_box_cylinder.json`, authored by `BRepAlgoAPI` +
//! `BRepGProp` via `scripts/parity/regen_box_cylinder_corpus.py`).
//! The two references agree to machine scale, and the kernel matches
//! both — including the face counts against OCCT's own topology.
//!
//! **The stations**: the through (the cylinder straddling the slab —
//! the genus-1 through-hole cut with its two annuli and REVERSED
//! wall, the axle fuse, the clipped-cylinder common), the blind bores
//! (entered from either face — the annulus + wall + floor disk), the
//! boss fuses (the wall + cap beyond a crossed face), the
//! frame-contract permutations (the second box axis, the antiparallel
//! cylinder axis — the same solid read in the flipped direction), and
//! the flush far ends (the cap coplanar with a face plane
//! contributing nothing — the cut reading the overlap only, the fuse
//! composing the MINIMAL shell where OCCT keeps a redundant coplanar
//! ninth face).
//!
//! The exactness discipline: the FCBREP v2 round-trip is
//! bit-identical (the bytes AND the measures), the classification
//! outcomes (whole shapes, the exact empty set, the named refusals —
//! the rotated/oblique/near-parallel axis, the crossing and tangent
//! section disks, the flush landing, the internal void) are asserted
//! by name, and the render channel TESSELLATES every constructive
//! result watertight — the annulus-carrying cuts and fuses through
//! the hole-wire planar arm (the hole's circle rim samples are the
//! shared edge discretization, so the annulus and the wall close
//! bit-exactly, the mesh volume within the deflection-derived chord
//! bound), the clipped-cylinder commons and the whole-shape outcomes
//! outright. The
//! frame contract is part of the pairing: permuted, sign-flipped, and
//! mirrored frames run exact, and the kernel never silently snaps.

use std::collections::HashMap;
use std::fs;

use fc_base::Vector3;
use fc_part::brep::read_brep_bytes;
use fc_part::brep::write_brep;
use fc_part::brep_boolean::{box_cylinder_boolean, BooleanOp, BooleanOutcome, BooleanOutcomeKind};
use fc_part::measure::{brep_area, brep_volume};
use fc_part::tessellate_brep;

const O: Vector3 = Vector3::new(0.0, 0.0, 0.0);
const AX: Vector3 = Vector3::new(1.0, 0.0, 0.0);
const AY: Vector3 = Vector3::new(0.0, 1.0, 0.0);
const AZ: Vector3 = Vector3::new(0.0, 0.0, 1.0);
const IDENTITY: [Vector3; 3] = [AX, AY, AZ];
const DIMS: [f64; 3] = [30.0, 20.0, 5.0];
const PI: f64 = std::f64::consts::PI;

fn v3(x: f64, y: f64, z: f64) -> Vector3 {
    Vector3::new(x, y, z)
}

/// The kernel outcome for the canonical base box and a placed
/// cylinder (identity box frame, the axis one of the three).
fn run(op: BooleanOp, oc: Vector3, ac: Vector3, rc: f64, hc: f64) -> BooleanOutcome {
    box_cylinder_boolean(op, O, AX, AY, AZ, DIMS[0], DIMS[1], DIMS[2], oc, ac, rc, hc)
        .expect("the kernel call itself")
}

/// The kernel outcome for a general base box frame.
#[allow(clippy::too_many_arguments)] // the box frame is the vocabulary
fn run_frame(
    op: BooleanOp,
    oa: Vector3,
    axes: [Vector3; 3],
    dims: [f64; 3],
    oc: Vector3,
    ac: Vector3,
    rc: f64,
    hc: f64,
) -> BooleanOutcome {
    box_cylinder_boolean(
        op, oa, axes[0], axes[1], axes[2], dims[0], dims[1], dims[2], oc, ac, rc, hc,
    )
    .expect("the kernel call itself")
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

fn is_empty(out: BooleanOutcome) -> bool {
    matches!(out.kind, BooleanOutcomeKind::Empty)
}

/// OCCT's own numbers for one case of the corpus: volume, area, and
/// the face count of real `BRepAlgoAPI`'s own result.
fn occt(case: &str) -> (f64, f64, usize) {
    let text = fs::read_to_string(format!(
        "{}/tests/fixtures/brp/expected_box_cylinder.json",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("missing expected_box_cylinder.json");
    let value: serde_json::Value =
        serde_json::from_str(&text).expect("expected_box_cylinder.json parses");
    let case = &value["cases"][case];
    (
        case["volume"].as_f64().expect("volume"),
        case["area"].as_f64().expect("area"),
        case["faces"].as_u64().expect("faces") as usize,
    )
}

/// The hole-wire count of a B-rep (wires beyond each face's first).
fn hole_wires(brep: &fc_part::ExactBRep) -> usize {
    brep.faces
        .iter()
        .map(|f| f.wires.len().saturating_sub(1))
        .sum()
}

/// The watertight discipline: every directed mesh edge used exactly
/// once, and exactly once in the opposite direction too.
fn assert_watertight(name: &str, brep: &fc_part::ExactBRep) {
    let mesh = tessellate_brep(brep, 0.25)
        .unwrap_or_else(|e| panic!("{name}: the render channel tessellates: {e}"));
    let mut directed: HashMap<(u32, u32), usize> = HashMap::new();
    for t in &mesh.triangles {
        for (x, y) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
            *directed.entry((x, y)).or_default() += 1;
        }
    }
    for (&(x, y), &n) in &directed {
        assert!(
            n == 1 && directed.get(&(y, x)).copied() == Some(1),
            "{name}: the mesh is watertight ({x}→{y} used {n} times)"
        );
    }
}

/// The annulus render discipline: watertight, and the mesh volume
/// within the deflection-derived chord bound of the exact — the
/// hole's inscribed polygon rim and the wall's chords are the only
/// approximations, each a circular-segment area
/// `(2/3)·perimeter·sagitta ≤ (2/3)·τ·r·d`, projected over the
/// prism's height.
fn assert_renders(name: &str, brep: &fc_part::ExactBRep, r: f64, height: f64) {
    let mesh = tessellate_brep(brep, 0.25)
        .unwrap_or_else(|e| panic!("{name}: the render channel tessellates: {e}"));
    let mut directed: HashMap<(u32, u32), usize> = HashMap::new();
    for t in &mesh.triangles {
        for (x, y) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
            *directed.entry((x, y)).or_default() += 1;
        }
    }
    for (&(x, y), &n) in &directed {
        assert!(
            n == 1 && directed.get(&(y, x)).copied() == Some(1),
            "{name}: the mesh is watertight ({x}→{y} used {n} times)"
        );
    }
    let exact = brep_volume(brep).unwrap_or_else(|e| panic!("{name}: volume refused: {e}"));
    let mv = mesh.volume();
    let bound = (4.0 / 3.0) * PI * r * 0.25 * height;
    assert!(
        (mv - exact).abs() <= bound,
        "{name}: the mesh volume {mv:.6} vs the exact {exact:.6} — the chord bound is {bound:.6}"
    );
}

/// One constructive case: the solid's measures against the closed
/// form AND OCCT, the face count against both, the Euler
/// characteristic with the hole adjustment (the genus contract), and
/// the FCBREP round-trip bit-identical.
#[allow(clippy::too_many_arguments)] // the case's own vocabulary
fn acceptance(
    name: &str,
    case: &str,
    op: BooleanOp,
    oc: Vector3,
    ac: Vector3,
    rc: f64,
    hc: f64,
    want_v: f64,
    want_a: f64,
    faces: usize,
    genus: usize,
) {
    let brep = solid(run(op, oc, ac, rc, hc));
    assert_eq!(brep.faces.len(), faces, "{name}: the face count");
    let v = brep_volume(&brep).unwrap_or_else(|e| panic!("{name}: volume refused: {e}"));
    let a = brep_area(&brep).unwrap_or_else(|e| panic!("{name}: area refused: {e}"));
    assert!(
        (v - want_v).abs() <= 1e-9 * want_v.abs().max(1.0),
        "{name}: volume {v:.12} vs the closed form {want_v:.12}"
    );
    assert!(
        (a - want_a).abs() <= 1e-9 * want_a.abs().max(1.0),
        "{name}: area {a:.12} vs the closed form {want_a:.12}"
    );
    let (ov, oa, of) = occt(case);
    assert_eq!(of, faces, "{name}: OCCT's own face count");
    assert!(
        (v - ov).abs() <= 5e-9 * ov.abs().max(1.0),
        "{name}: volume {v:.12} vs OCCT {ov:.12}"
    );
    assert!(
        (a - oa).abs() <= 5e-9 * oa.abs().max(1.0),
        "{name}: area {a:.12} vs OCCT {oa:.12}"
    );
    // The Euler characteristic with the hole-wire adjustment: a
    // closed shell of genus `g` reads V − E + F − H = 2 − 2g.
    let chi = brep.euler_characteristic() - hole_wires(&brep) as i64;
    assert_eq!(
        chi,
        2 - 2 * genus as i64,
        "{name}: V−E+F−H = {chi} (genus {genus})"
    );
    // The FCBREP v2 round-trip: bit-identical bytes AND measures.
    let mut bytes = Vec::new();
    write_brep(&mut bytes, &brep).expect("writes FCBREP v2");
    let round = read_brep_bytes(&bytes).expect("reads back");
    let mut bytes2 = Vec::new();
    write_brep(&mut bytes2, &round).expect("the round trip re-writes");
    assert_eq!(
        bytes, bytes2,
        "{name}: the FCBREP round trip is byte-identical"
    );
    let rv = brep_volume(&round).expect("the round-trip measures");
    assert_eq!(rv, v, "{name}: the round-trip volume is bit-identical");
}

/// The through station: the cylinder (r=3) straddling the slab from
/// z=−2 to z=8 — the genus-1 through-hole cut, the axle fuse, the
/// clipped-cylinder common. All three against the closed forms AND
/// OCCT's own numbers, the face counts against OCCT's own topology.
#[test]
fn through_hole_measures_the_closed_forms_and_occt() {
    // Cut: V = 3000 − 9π·5; A = 1700 − 18π (the two annuli) + 30π
    // (the wall) = 1700 + 12π; F = 4 sides + 2 annuli + wall = 7.
    acceptance(
        "through cut",
        "box_cylinder_cut_through",
        BooleanOp::Cut,
        v3(15.0, 10.0, -2.0),
        AZ,
        3.0,
        10.0,
        3000.0 - 45.0 * PI,
        1700.0 + 12.0 * PI,
        7,
        1,
    );
    // Fuse: the axle — V = 3000 + 9π·5; A = 1700 − 18π + 30π + 18π
    // (two caps); F = 4 + 2 annuli + 2 walls + 2 caps = 10.
    acceptance(
        "through fuse",
        "box_cylinder_fuse_through",
        BooleanOp::Fuse,
        v3(15.0, 10.0, -2.0),
        AZ,
        3.0,
        10.0,
        3000.0 + 45.0 * PI,
        1700.0 + 30.0 * PI,
        10,
        0,
    );
    // Common: the clipped cylinder — V = 9π·5, A = 30π + 18π; F = 3.
    acceptance(
        "through common",
        "box_cylinder_common_through",
        BooleanOp::Common,
        v3(15.0, 10.0, -2.0),
        AZ,
        3.0,
        10.0,
        45.0 * PI,
        48.0 * PI,
        3,
        0,
    );
    // The render channel: the annulus-carrying results tessellate
    // watertight with the deflection-derived chord bound, the
    // clipped cylinder watertight (its rims are the shared samples).
    assert_renders(
        "through cut",
        &solid(run(BooleanOp::Cut, v3(15.0, 10.0, -2.0), AZ, 3.0, 10.0)),
        3.0,
        5.0,
    );
    assert_renders(
        "through fuse",
        &solid(run(BooleanOp::Fuse, v3(15.0, 10.0, -2.0), AZ, 3.0, 10.0)),
        3.0,
        5.0,
    );
    assert_watertight(
        "through common",
        &solid(run(BooleanOp::Common, v3(15.0, 10.0, -2.0), AZ, 3.0, 10.0)),
    );
}

/// The blind bores: the cylinder crossing exactly one face plane,
/// ending strictly inside — entered from below (z ∈ [−1, 3]) and
/// from above (z ∈ [2, 7]). The cut is the annulus + wall + floor
/// disk (8 faces); the fuse the boss beyond the crossed face; the
/// common the clipped cylinder.
#[test]
fn blind_bores_measure_the_closed_forms_and_occt() {
    // Blind low (depth 3): V = 3000 − 4π·3; A = 1700 + 2π·2·3 (the
    // wall — the floor disk returns the annulus's πr²).
    acceptance(
        "blind-low cut",
        "box_cylinder_cut_blind_low",
        BooleanOp::Cut,
        v3(15.0, 10.0, -1.0),
        AZ,
        2.0,
        4.0,
        3000.0 - 12.0 * PI,
        1700.0 + 12.0 * PI,
        8,
        0,
    );
    // The boss below (overhang 1): V = 3000 + 4π·1; A = 1700 + 4π.
    acceptance(
        "blind-low fuse",
        "box_cylinder_fuse_blind_low",
        BooleanOp::Fuse,
        v3(15.0, 10.0, -1.0),
        AZ,
        2.0,
        4.0,
        3000.0 + 4.0 * PI,
        1700.0 + 4.0 * PI,
        8,
        0,
    );
    acceptance(
        "blind-low common",
        "box_cylinder_common_blind_low",
        BooleanOp::Common,
        v3(15.0, 10.0, -1.0),
        AZ,
        2.0,
        4.0,
        12.0 * PI,
        20.0 * PI,
        3,
        0,
    );
    // Blind high (depth 3, overhang 2).
    acceptance(
        "blind-high cut",
        "box_cylinder_cut_blind_high",
        BooleanOp::Cut,
        v3(15.0, 10.0, 2.0),
        AZ,
        2.0,
        5.0,
        3000.0 - 12.0 * PI,
        1700.0 + 12.0 * PI,
        8,
        0,
    );
    acceptance(
        "blind-high fuse",
        "box_cylinder_fuse_blind_high",
        BooleanOp::Fuse,
        v3(15.0, 10.0, 2.0),
        AZ,
        2.0,
        5.0,
        3000.0 + 8.0 * PI,
        1700.0 + 8.0 * PI,
        8,
        0,
    );
    acceptance(
        "blind-high common",
        "box_cylinder_common_blind_high",
        BooleanOp::Common,
        v3(15.0, 10.0, 2.0),
        AZ,
        2.0,
        5.0,
        12.0 * PI,
        20.0 * PI,
        3,
        0,
    );
}

/// The frame contract's own permutations: the cylinder along the
/// box's SECOND axis (through_y), and the ANTIPARALLEL axis (the
/// span read in the flipped direction — blind_flip is the same solid
/// as blind_low, and the flipped through the same hole as through).
#[test]
fn permuted_axes_read_the_same_solids() {
    // through_y: the hole depth is the box's Y extent 20 — the
    // area loses the two annuli's πr² and gains the wall's 2πrh.
    acceptance(
        "through-y cut",
        "box_cylinder_cut_through_y",
        BooleanOp::Cut,
        v3(10.0, -2.0, 2.5),
        AY,
        2.0,
        27.0,
        3000.0 - 80.0 * PI,
        1700.0 - 8.0 * PI + 80.0 * PI,
        7,
        1,
    );
    acceptance(
        "through-y fuse",
        "box_cylinder_fuse_through_y",
        BooleanOp::Fuse,
        v3(10.0, -2.0, 2.5),
        AY,
        2.0,
        27.0,
        3000.0 + 28.0 * PI,
        1700.0 + 28.0 * PI,
        10,
        0,
    );
    acceptance(
        "through-y common",
        "box_cylinder_common_through_y",
        BooleanOp::Common,
        v3(10.0, -2.0, 2.5),
        AY,
        2.0,
        27.0,
        80.0 * PI,
        88.0 * PI,
        3,
        0,
    );
    // The antiparallel blind bore: origin (15, 10, 3), axis −Z,
    // h=4 → the span z ∈ [−1, 3] — the same solid as blind_low.
    acceptance(
        "blind-flip cut",
        "box_cylinder_cut_blind_flip",
        BooleanOp::Cut,
        v3(15.0, 10.0, 3.0),
        v3(0.0, 0.0, -1.0),
        2.0,
        4.0,
        3000.0 - 12.0 * PI,
        1700.0 + 12.0 * PI,
        8,
        0,
    );
    acceptance(
        "blind-flip fuse",
        "box_cylinder_fuse_blind_flip",
        BooleanOp::Fuse,
        v3(15.0, 10.0, 3.0),
        v3(0.0, 0.0, -1.0),
        2.0,
        4.0,
        3000.0 + 4.0 * PI,
        1700.0 + 4.0 * PI,
        8,
        0,
    );
    acceptance(
        "blind-flip common",
        "box_cylinder_common_blind_flip",
        BooleanOp::Common,
        v3(15.0, 10.0, 3.0),
        v3(0.0, 0.0, -1.0),
        2.0,
        4.0,
        12.0 * PI,
        20.0 * PI,
        3,
        0,
    );
    // The antiparallel through: origin (15, 10, 8), axis −Z, h=10 →
    // the span z ∈ [−2, 8] — the same through hole.
    let flipped = solid(run(
        BooleanOp::Cut,
        v3(15.0, 10.0, 8.0),
        v3(0.0, 0.0, -1.0),
        3.0,
        10.0,
    ));
    let v = brep_volume(&flipped).unwrap();
    assert!(
        (v - (3000.0 - 45.0 * PI)).abs() <= 1e-9,
        "the flipped through-hole volume {v} is the closed form"
    );
    assert_eq!(flipped.faces.len(), 7, "the flipped through-hole topology");
}

/// The flush far ends: the cylinder's cap exactly coplanar with a
/// face plane contributes NOTHING — the cut reads the overlap only
/// (the same through hole), and the fuse composes the MINIMAL shell
/// (the flush cap lies interior, covered by the box's own full face)
/// where OCCT keeps the coplanar disk as its own redundant ninth
/// face — the volume and area agree, the face count does not.
#[test]
fn flush_far_ends_contribute_nothing() {
    // The flush-through cut: the span z ∈ [0, 8] — the low cap
    // exactly on the bottom face plane. The cut is the through hole.
    acceptance(
        "flush-through cut",
        "box_cylinder_cut_flush_through",
        BooleanOp::Cut,
        v3(15.0, 10.0, 0.0),
        AZ,
        3.0,
        8.0,
        3000.0 - 45.0 * PI,
        1700.0 + 12.0 * PI,
        7,
        1,
    );
    // The flush-through fuse: the box plus the overhang [5, 8] —
    // EIGHT faces (the bottom face full, not annular), the closed
    // forms V = 3000 + 27π, A = 1700 + 18π.
    let brep = solid(run(BooleanOp::Fuse, v3(15.0, 10.0, 0.0), AZ, 3.0, 8.0));
    assert_eq!(brep.faces.len(), 8, "the minimal flush fuse shell");
    let v = brep_volume(&brep).unwrap();
    let a = brep_area(&brep).unwrap();
    assert!(
        (v - (3000.0 + 27.0 * PI)).abs() <= 1e-9,
        "the flush fuse volume {v} is the closed form"
    );
    assert!(
        (a - (1700.0 + 18.0 * PI)).abs() <= 1e-9,
        "the flush fuse area {a} is the closed form"
    );
    // OCCT's own flush-through fuse (the corpus' cut-only sibling
    // measured at the probe stage): the SAME volume and area with a
    // ninth, redundant coplanar-disk face — the minimal shell is
    // ours. The render channel tessellates it (the high annulus).
    assert_renders("the flush fuse", &brep, 3.0, 5.0);
}

/// The classification tier: the whole shapes, the exact empty set,
/// and the internal void's named refusal.
#[test]
fn classification_tier_answers_whole_shapes() {
    // Disjoint (strictly below): the cut the whole box, the common
    // the empty set, the fuse the two-shell refusal.
    let cut = solid(run(BooleanOp::Cut, v3(15.0, 10.0, -5.0), AZ, 2.0, 2.0));
    assert_eq!(brep_volume(&cut).unwrap(), 3000.0);
    assert_eq!(cut.faces.len(), 6);
    assert_eq!(
        occt("box_cylinder_cut_disjoint").0,
        3000.0,
        "OCCT's own disjoint cut is the box"
    );
    assert!(is_empty(run(
        BooleanOp::Common,
        v3(15.0, 10.0, -5.0),
        AZ,
        2.0,
        2.0
    )));
    let why = refused(run(BooleanOp::Fuse, v3(15.0, 10.0, -5.0), AZ, 2.0, 2.0));
    assert!(why.contains("two-shell"), "{why}");
    // Engulf (r=25 covering the cross-section corners, the span
    // covering the slab): the cut empty, the fuse the cylinder, the
    // common the box.
    assert!(is_empty(run(
        BooleanOp::Cut,
        v3(15.0, 10.0, -1.0),
        AZ,
        25.0,
        7.0
    )));
    let fused = solid(run(BooleanOp::Fuse, v3(15.0, 10.0, -1.0), AZ, 25.0, 7.0));
    assert!((brep_volume(&fused).unwrap() - 4375.0 * PI).abs() <= 1e-9);
    assert_eq!(fused.faces.len(), 3);
    assert_watertight("the engulf fuse (the plain cylinder)", &fused);
    let common = solid(run(BooleanOp::Common, v3(15.0, 10.0, -1.0), AZ, 25.0, 7.0));
    assert_eq!(brep_volume(&common).unwrap(), 3000.0);
    assert_eq!(common.faces.len(), 6);
    // Contained (strictly inside): the common the cylinder, the fuse
    // the box, the cut the internal void's refusal.
    let common = solid(run(BooleanOp::Common, v3(15.0, 10.0, 1.0), AZ, 2.0, 3.0));
    assert!((brep_volume(&common).unwrap() - 12.0 * PI).abs() <= 1e-9);
    assert_eq!(common.faces.len(), 3);
    let fused = solid(run(BooleanOp::Fuse, v3(15.0, 10.0, 1.0), AZ, 2.0, 3.0));
    assert_eq!(brep_volume(&fused).unwrap(), 3000.0);
    assert_eq!(fused.faces.len(), 6);
    let why = refused(run(BooleanOp::Cut, v3(15.0, 10.0, 1.0), AZ, 2.0, 3.0));
    assert!(why.contains("void"), "{why}");
}

/// The named refusals: the rotated/oblique/near-parallel frame, the
/// crossing disk, the tangent disk, the flush landing — never a
/// silent snap, a silent degeneracy, or a silent merge.
#[test]
fn unsupported_configurations_refuse_by_name() {
    // An oblique axis (45° in the XZ plane): no box axis claims it.
    let diag = v3(
        std::f64::consts::FRAC_1_SQRT_2,
        0.0,
        std::f64::consts::FRAC_1_SQRT_2,
    );
    let why = refused(run(BooleanOp::Cut, v3(15.0, 10.0, 0.0), diag, 3.0, 10.0));
    assert!(why.contains("never silently snaps"), "{why}");
    assert!(why.contains("not bit-exactly parallel"), "{why}");
    // A NEAR-parallel axis (unit, but the dot with the box axis a
    // hair under 1 — the extents would be a snap's).
    let near = v3(1e-5, 0.0, 0.99999999995);
    let why = refused(run(BooleanOp::Cut, v3(15.0, 10.0, 0.0), near, 3.0, 10.0));
    assert!(why.contains("never silently snaps"), "{why}");
    // A crossing disk (r=15 at the cross-section's center — the hole
    // would break all four side faces).
    let why = refused(run(BooleanOp::Cut, v3(15.0, 10.0, -2.0), AZ, 15.0, 10.0));
    assert!(why.contains("crosses the box's cross-section"), "{why}");
    assert!(why.contains("trim-domain"), "{why}");
    // A tangent disk (r=10 at (10, 10): touching x=0 and x=20 — the
    // annulus would pinch).
    let why = refused(run(BooleanOp::Cut, v3(10.0, 10.0, -2.0), AZ, 10.0, 10.0));
    assert!(why.contains("tangent"), "{why}");
    assert!(why.contains("never silently degenerates"), "{why}");
    // The tangent Common still answers (the clipped cylinder — the
    // full-disk column, the tangency measure-zero).
    let common = solid(run(BooleanOp::Common, v3(10.0, 10.0, -2.0), AZ, 10.0, 10.0));
    assert!((brep_volume(&common).unwrap() - 500.0 * PI).abs() <= 1e-9);
    // The flush landing (the cylinder touching from below, its cap
    // exactly on the face plane): the cut the box, the common empty,
    // the fuse the coplanar-merge refusal.
    let cut = solid(run(BooleanOp::Cut, v3(15.0, 10.0, -2.0), AZ, 2.0, 2.0));
    assert_eq!(brep_volume(&cut).unwrap(), 3000.0);
    assert!(is_empty(run(
        BooleanOp::Common,
        v3(15.0, 10.0, -2.0),
        AZ,
        2.0,
        2.0
    )));
    let why = refused(run(BooleanOp::Fuse, v3(15.0, 10.0, -2.0), AZ, 2.0, 2.0));
    assert!(why.contains("flush landing"), "{why}");
    assert!(why.contains("never silently merges"), "{why}");
}

/// The mirrored and permuted box frames run exact — the extents
/// recomputed in the base's own frame, either handedness.
#[test]
fn mirrored_and_permuted_frames_run_exact() {
    // A mirrored (left-handed) frame: (X, −Y, Z) with the cylinder
    // along Z — the same through hole.
    let mirrored = [AX, v3(0.0, -1.0, 0.0), AZ];
    // The box occupies the SAME region only if the origin moves with
    // the mirrored axis: o' = (0, 20, 0) with −Y spans y ∈ [20, 0].
    let brep = solid(run_frame(
        BooleanOp::Cut,
        v3(0.0, 20.0, 0.0),
        mirrored,
        DIMS,
        v3(15.0, 10.0, -2.0),
        AZ,
        3.0,
        10.0,
    ));
    assert_eq!(brep.faces.len(), 7);
    assert!((brep_volume(&brep).unwrap() - (3000.0 - 45.0 * PI)).abs() <= 1e-9);
    // A permuted (right-handed) frame: (Y, Z, X) — the box spans
    // y ∈ [0,20], z ∈ [0,5], x ∈ [0,30]; the cylinder along the
    // frame's SECOND axis (Z), the same world through hole.
    let permuted = [AY, AZ, AX];
    let dims_p = [20.0, 5.0, 30.0];
    let brep = solid(run_frame(
        BooleanOp::Cut,
        O,
        permuted,
        dims_p,
        v3(15.0, 10.0, -2.0),
        AZ,
        3.0,
        10.0,
    ));
    assert_eq!(brep.faces.len(), 7);
    assert!((brep_volume(&brep).unwrap() - (3000.0 - 45.0 * PI)).abs() <= 1e-9);
    // A translated frame: the box at (−10, −10, −10), the cylinder
    // placed relatively — the same forms.
    let brep = solid(run_frame(
        BooleanOp::Cut,
        v3(-10.0, -10.0, -10.0),
        IDENTITY,
        DIMS,
        v3(5.0, 0.0, -12.0),
        AZ,
        3.0,
        10.0,
    ));
    assert_eq!(brep.faces.len(), 7);
    assert!((brep_volume(&brep).unwrap() - (3000.0 - 45.0 * PI)).abs() <= 1e-9);
}

/// The structural contract: exactly two rim circles authored per
/// wall, every edge used exactly twice, and the render channel
/// honest BOTH ways across the whole family — the annulus-carrying
/// results refusing by name, the clipped cylinders and whole shapes
/// meshing watertight.
#[test]
fn the_shell_and_render_contracts_hold() {
    // The through cut's structure: two holed faces, one cylindrical
    // wall, every edge used exactly twice (the closed-shell
    // contract the measure itself enforces).
    let brep = solid(run(BooleanOp::Cut, v3(15.0, 10.0, -2.0), AZ, 3.0, 10.0));
    let holed = brep.faces.iter().filter(|f| f.wires.len() == 2).count();
    assert_eq!(holed, 2, "the two annular faces");
    let walls = brep
        .faces
        .iter()
        .filter(|f| matches!(f.surface, fc_part::topology::Surface::Cylinder { .. }))
        .count();
    assert_eq!(walls, 1, "the one wall band");
    for e in 0..brep.edges.len() {
        assert_eq!(
            brep.edge_uses(e),
            2,
            "edge {e} used exactly twice (the closed shell)"
        );
    }
    // The render channel: every annulus-carrying constructive result
    // tessellates watertight (the volume bound is the dedicated
    // discipline's own test below).
    for (name, op, oc, r, h) in [
        (
            "through cut",
            BooleanOp::Cut,
            v3(15.0, 10.0, -2.0),
            3.0,
            10.0,
        ),
        (
            "through fuse",
            BooleanOp::Fuse,
            v3(15.0, 10.0, -2.0),
            3.0,
            10.0,
        ),
        ("blind cut", BooleanOp::Cut, v3(15.0, 10.0, -1.0), 2.0, 4.0),
        ("blind fuse", BooleanOp::Fuse, v3(15.0, 10.0, 2.0), 2.0, 5.0),
    ] {
        let brep = solid(run(op, oc, AZ, r, h));
        assert_watertight(name, &brep);
    }
    // The commons (the clipped cylinders) and the whole-shape
    // outcomes mesh WATERTIGHT — the display channel honest for
    // what it can display.
    for (name, op, oc, r, h) in [
        (
            "through common",
            BooleanOp::Common,
            v3(15.0, 10.0, -2.0),
            3.0,
            10.0,
        ),
        (
            "blind common",
            BooleanOp::Common,
            v3(15.0, 10.0, -1.0),
            2.0,
            4.0,
        ),
        (
            "engulf fuse",
            BooleanOp::Fuse,
            v3(15.0, 10.0, -1.0),
            25.0,
            7.0,
        ),
        (
            "disjoint cut",
            BooleanOp::Cut,
            v3(15.0, 10.0, -5.0),
            2.0,
            2.0,
        ),
    ] {
        let brep = solid(run(op, oc, AZ, r, h));
        assert_watertight(name, &brep);
    }
}

/// The annulus mesh's approximation is deflection-true in the
/// discipline's own sense: the hole's inscribed polygon deficit (and
/// the wall's chord band) shrinks with the deflection — quartering
/// the deflection quarters the volume error (the circular-segment
/// area is linear in the sagitta).
#[test]
fn annulus_mesh_volume_error_shrinks_with_deflection() {
    let brep = solid(run(BooleanOp::Cut, v3(15.0, 10.0, -2.0), AZ, 3.0, 10.0));
    let exact = brep_volume(&brep).unwrap();
    let err = |d: f64| {
        (tessellate_brep(&brep, d)
            .expect("the render channel")
            .volume()
            - exact)
            .abs()
    };
    let coarse = err(0.25);
    let fine = err(0.0625);
    assert!(
        coarse > 0.0,
        "the coarse annulus mesh sits strictly off the exact (the chords bite)"
    );
    assert!(
        fine <= coarse / 3.0,
        "quartering the deflection must shrink the deficit ({fine:.6} vs {coarse:.6})"
    );
}
