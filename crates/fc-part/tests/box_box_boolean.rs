//! K4-2d/3 slice 3 acceptance — the box×box overlap taxonomy.
//!
//! One box (30×20×5 — the corpus' own `box.brp` dimensions) meets a
//! second, frame-parallel box at every station of the overlap
//! taxonomy, and the three booleans read against **two independent
//! references**: the hand-derived closed forms (`V_A ∓ V_O` for the
//! cut and common, `V_A + V_B − V_O` for the fuse, and the per-face
//! area sums over the rectilinear regions — every face a plane
//! trimmed by an axis-aligned polygon) and real OCCT's own numbers
//! (`expected_box_box.json`, authored by `BRepAlgoAPI` +
//! `BRepGProp` via `scripts/parity/regen_box_box_corpus.py`). The two
//! references agree to machine scale, and the kernel matches both.
//!
//! **The stations** (the overlap box `O = A ∩ B` classified by how
//! it touches `A`'s boundary): the corner (O contains exactly one
//! corner — the 9-face notch cut, the 12-face corner union), the
//! edge (O contains one edge — the through-notch cut, the
//! octagonal-prism fuse), the face (O contains one face — a plain
//! slab cut, the stepped-box fuse with the annular shoulder), the
//! slot (through on one axis, interior on two — the genus-1 cut with
//! holed caps), and the groove (through, touching one face, interior
//! on one). Every constructive face is a plane trimmed by a
//! rectilinear wire — the polygon and annulus paths the measure's
//! plane reader already reads exactly; no new measure was needed.
//!
//! The exactness discipline: the FCBREP v2 round-trip measures
//! bit-identically, the classification outcomes (whole shapes, the
//! exact empty set, the named refusals) are asserted by name, and
//! the render channel is honest BOTH ways — the hole-free results
//! tessellate WATERTIGHT with the mesh volume agreeing with the
//! exact volume (planar faces triangulate exactly), while the
//! hole-trimmed ones (the slot's caps, the fuse's shoulder ring)
//! refuse by name — the two-channel contract. The frame mapping is
//! part of the contract: permuted, sign-flipped, and mirrored frames
//! run exact (the extents recomputed in the base's frame), and a
//! rotated or near-parallel frame refuses by name — this kernel
//! never silently snaps.

use std::collections::HashMap;
use std::fs;

use fc_base::Vector3;
use fc_part::brep::read_brep_bytes;
use fc_part::brep::write_brep;
use fc_part::brep_boolean::{box_box_boolean, BooleanOp, BooleanOutcome, BooleanOutcomeKind};
use fc_part::measure::{brep_area, brep_volume};
use fc_part::tessellate_brep;

const O: Vector3 = Vector3::new(0.0, 0.0, 0.0);
const AX: Vector3 = Vector3::new(1.0, 0.0, 0.0);
const AY: Vector3 = Vector3::new(0.0, 1.0, 0.0);
const AZ: Vector3 = Vector3::new(0.0, 0.0, 1.0);
const IDENTITY: [Vector3; 3] = [AX, AY, AZ];
const DIMS: [f64; 3] = [30.0, 20.0, 5.0];

fn v3(x: f64, y: f64, z: f64) -> Vector3 {
    Vector3::new(x, y, z)
}

/// The kernel outcome for the canonical base box and a placed
/// identity-frame tool box.
fn run(op: BooleanOp, ob: Vector3, dims_b: [f64; 3]) -> BooleanOutcome {
    box_box_boolean(op, O, IDENTITY, DIMS, ob, IDENTITY, dims_b).expect("the kernel call itself")
}

/// The kernel outcome for general frames (base and tool).
#[allow(clippy::too_many_arguments)] // the two frames are the vocabulary
fn run_frame(
    op: BooleanOp,
    oa: Vector3,
    axes_a: [Vector3; 3],
    dims_a: [f64; 3],
    ob: Vector3,
    axes_b: [Vector3; 3],
    dims_b: [f64; 3],
) -> BooleanOutcome {
    box_box_boolean(op, oa, axes_a, dims_a, ob, axes_b, dims_b).expect("the kernel call itself")
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
        "{}/tests/fixtures/brp/expected_box_box.json",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("missing expected_box_box.json");
    let value: serde_json::Value =
        serde_json::from_str(&text).expect("expected_box_box.json parses");
    let case = &value["cases"][case];
    (
        case["volume"].as_f64().expect("volume"),
        case["area"].as_f64().expect("area"),
        case["faces"].as_u64().expect("faces") as usize,
    )
}

/// The render-channel expectation: hole-free rectilinear faces
/// tessellate WATERTIGHT (the planar ear-clip path), the mesh volume
/// agreeing with the exact volume; hole-trimmed faces refuse by name.
#[derive(Clone, Copy, PartialEq)]
enum Render {
    /// Every face single-wired: the mesh exists, watertight, exact.
    Meshes,
    /// Some face carries a hole wire: the render channel refuses.
    RefusesHoles,
}

/// The hole-wire count of a B-rep (wires beyond each face's first).
fn hole_wires(brep: &fc_part::ExactBRep) -> usize {
    brep.faces
        .iter()
        .map(|f| f.wires.len().saturating_sub(1))
        .sum()
}

/// One constructive case: the solid's measures against the closed
/// form AND OCCT, the face count against both, the Euler
/// characteristic with the hole adjustment (the genus contract), the
/// FCBREP round-trip bit-identical, and the render channel honest.
#[allow(clippy::too_many_arguments)] // the case's own vocabulary
fn acceptance(
    name: &str,
    case: &str,
    op: BooleanOp,
    ob: Vector3,
    dims_b: [f64; 3],
    want_v: f64,
    want_a: f64,
    faces: usize,
    genus: usize,
    render: Render,
) {
    let brep = solid(run(op, ob, dims_b));
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
    // The FCBREP v2 round-trip: bit-identical measures.
    let mut bytes = Vec::new();
    write_brep(&mut bytes, &brep).expect("writes FCBREP v2");
    let round = read_brep_bytes(&bytes).expect("reads back");
    let rv = brep_volume(&round).expect("the round-trip measures");
    assert!(
        (rv - v).abs() <= 1e-12 * v.abs().max(1.0),
        "{name}: round-trip volume {rv:.12} vs {v:.12}"
    );
    // The render channel: watertight and numerically exact for the
    // hole-free rectilinear faces; a named refusal for the holed
    // ones (the annulus faces of this family).
    match render {
        Render::Meshes => {
            let mesh = tessellate_brep(&brep, 0.25)
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
            let mv = mesh.volume();
            assert!(
                (mv - v).abs() <= 1e-9 * v.abs().max(1.0),
                "{name}: the planar mesh volume {mv:.12} is the exact {v:.12}"
            );
        }
        Render::RefusesHoles => {
            let err = tessellate_brep(&brep, 0.25).expect_err("the render channel refuses");
            assert!(
                err.contains("holes"),
                "{name}: the tessellation refusal names its reason: {err}"
            );
        }
    }
}

/// The corner station: the overlap box contains exactly one corner
/// of the base — all three booleans, the closed forms, OCCT's own
/// numbers.
#[test]
fn corner_bite_measures_the_closed_forms_and_occt() {
    // O = [25,30]×[15,20]×[2,5] (5×5×3): the cut keeps A's area (the
    // three bitten faces lose their footprints, the three cut faces
    // return them), the fuse adds B's outside shell, the common is
    // the overlap box itself.
    let ob = v3(25.0, 15.0, 2.0);
    let dims = [10.0, 10.0, 6.0];
    acceptance(
        "corner cut",
        "box_box_cut_corner",
        BooleanOp::Cut,
        ob,
        dims,
        2925.0,
        1700.0,
        9,
        0,
        Render::Meshes,
    );
    acceptance(
        "corner fuse",
        "box_box_fuse_corner",
        BooleanOp::Fuse,
        ob,
        dims,
        3525.0,
        2030.0,
        12,
        0,
        Render::Meshes,
    );
    acceptance(
        "corner common",
        "box_box_common_corner",
        BooleanOp::Common,
        ob,
        dims,
        75.0,
        110.0,
        6,
        0,
        Render::Meshes,
    );
}

/// The edge station: the overlap contains exactly one edge (the tool
/// through on z) — the cut is the 8-face through-notch, the fuse the
/// octagonal prism (two octagon caps, eight sides).
#[test]
fn edge_bite_measures_the_closed_forms_and_occt() {
    let ob = v3(25.0, 15.0, -2.0);
    let dims = [10.0, 10.0, 10.0];
    acceptance(
        "edge cut",
        "box_box_cut_edge",
        BooleanOp::Cut,
        ob,
        dims,
        2875.0,
        1650.0,
        8,
        0,
        Render::Meshes,
    );
    acceptance(
        "edge fuse",
        "box_box_fuse_edge",
        BooleanOp::Fuse,
        ob,
        dims,
        3875.0,
        2150.0,
        12,
        0,
        Render::Meshes,
    );
    acceptance(
        "edge common",
        "box_box_common_edge",
        BooleanOp::Common,
        ob,
        dims,
        125.0,
        150.0,
        6,
        0,
        Render::Meshes,
    );
}

/// The face station: the overlap contains exactly one face (the tool
/// through on x and y) — the cut is a plain slab box, the fuse the
/// stepped box whose shoulder is an annulus (the render refusal).
#[test]
fn face_bite_measures_the_closed_forms_and_occt() {
    let ob = v3(-5.0, -5.0, -1.0);
    let dims = [40.0, 30.0, 4.0];
    acceptance(
        "face cut",
        "box_box_cut_face",
        BooleanOp::Cut,
        ob,
        dims,
        1200.0,
        1400.0,
        6,
        0,
        Render::Meshes,
    );
    acceptance(
        "face fuse",
        "box_box_fuse_face",
        BooleanOp::Fuse,
        ob,
        dims,
        6000.0,
        3160.0,
        11,
        0,
        Render::RefusesHoles,
    );
    acceptance(
        "face common",
        "box_box_common_face",
        BooleanOp::Common,
        ob,
        dims,
        1800.0,
        1500.0,
        6,
        0,
        Render::Meshes,
    );
}

/// The slot station: through on one axis, strictly interior on the
/// other two — the cut is the genus-1 through-slot (holed top and
/// bottom caps: the render refusal, the Euler characteristic 0), the
/// fuse the box with the column welded through (its caps are holed
/// rings too — the same render refusal).
#[test]
fn slot_bite_measures_the_closed_forms_and_occt() {
    let ob = v3(10.0, 5.0, -2.0);
    let dims = [10.0, 10.0, 10.0];
    acceptance(
        "slot cut",
        "box_box_cut_slot",
        BooleanOp::Cut,
        ob,
        dims,
        2500.0,
        1700.0,
        10,
        1,
        Render::RefusesHoles,
    );
    acceptance(
        "slot fuse",
        "box_box_fuse_slot",
        BooleanOp::Fuse,
        ob,
        dims,
        3500.0,
        1900.0,
        16,
        0,
        Render::RefusesHoles,
    );
    acceptance(
        "slot common",
        "box_box_common_slot",
        BooleanOp::Common,
        ob,
        dims,
        500.0,
        400.0,
        6,
        0,
        Render::Meshes,
    );
}

/// The groove station: the slot's wall-side sibling — through on
/// the LONG axis, strictly interior on the other two, so the
/// genus-1 cut's rings sit on the side walls instead of the caps;
/// the fuse carries the same rings (the render refusal), the common
/// is the overlap box.
#[test]
fn groove_bite_measures_the_closed_forms_and_occt() {
    let ob = v3(10.0, -5.0, 2.0);
    let dims = [10.0, 30.0, 2.0];
    acceptance(
        "groove cut",
        "box_box_cut_groove",
        BooleanOp::Cut,
        ob,
        dims,
        2600.0,
        2140.0,
        10,
        1,
        Render::RefusesHoles,
    );
    acceptance(
        "groove fuse",
        "box_box_fuse_groove",
        BooleanOp::Fuse,
        ob,
        dims,
        3200.0,
        1940.0,
        16,
        0,
        Render::RefusesHoles,
    );
    acceptance(
        "groove common",
        "box_box_common_groove",
        BooleanOp::Common,
        ob,
        dims,
        400.0,
        520.0,
        6,
        0,
        Render::Meshes,
    );
}

/// The open-groove station: through on one axis and REACHING one
/// face with the tool sticking past it — the channel breaks the top
/// face open (two bands), the walls are L-shaped, and the cut is
/// genus 0 and renders: the richest fully-meshable topology of the
/// family (bands, Ls, the flush drop, and the C-shaped fuse walls
/// in one configuration).
#[test]
fn open_groove_bite_measures_the_closed_forms_and_occt() {
    let ob = v3(10.0, -5.0, 2.0);
    let dims = [10.0, 30.0, 5.0];
    acceptance(
        "open groove cut",
        "box_box_cut_open_groove",
        BooleanOp::Cut,
        ob,
        dims,
        2400.0,
        1760.0,
        10,
        0,
        Render::Meshes,
    );
    acceptance(
        "open groove fuse",
        "box_box_fuse_open_groove",
        BooleanOp::Fuse,
        ob,
        dims,
        3900.0,
        2120.0,
        14,
        0,
        Render::Meshes,
    );
    acceptance(
        "open groove common",
        "box_box_common_open_groove",
        BooleanOp::Common,
        ob,
        dims,
        600.0,
        580.0,
        6,
        0,
        Render::Meshes,
    );
}

/// The flush edge bite: the tool's low face exactly on the base's
/// bottom face. The CUT reads the overlap box only (the flushed tool
/// face contributes nothing — the construction is the input's own
/// arithmetic, no snap); the FUSE would need the coplanar face
/// merge, which refuses by name — the named next slice, never a
/// silent merge.
#[test]
fn flush_edge_bite_cut_reads_the_overlap_and_fuse_refuses() {
    let ob = v3(25.0, 15.0, 0.0);
    let dims = [10.0, 10.0, 8.0];
    // The cut: O = [25,30]×[15,20]×[0,5] — the same numbers as the
    // edge station (the overlap is a z-through bite that happens to
    // be flush at the bottom).
    acceptance(
        "flush edge cut",
        "box_box_cut_edge_flush",
        BooleanOp::Cut,
        ob,
        dims,
        2875.0,
        1650.0,
        8,
        0,
        Render::Meshes,
    );
    let why = refused(run(BooleanOp::Fuse, ob, dims));
    assert!(
        why.contains("coplanar"),
        "the flush fuse refusal names the merge: {why}"
    );
}

/// The classification tier: whole shapes, the exact empty set, and
/// the named refusals — the disjoint fuse (a two-shell compound),
/// the strict-interior cut (the internal void), the flush-containment
/// cut (a legitimate notch, the construction's own path), and the
/// degenerate B == A.
#[test]
fn classification_tier_answers_whole_shapes_emptiness_and_refusals() {
    // Disjoint (external): the cut is the whole base, the common the
    // empty set, the fuse a two-shell compound — refused.
    let ob = v3(40.0, 0.0, 0.0);
    let dims = [5.0, 5.0, 5.0];
    let cut = solid(run(BooleanOp::Cut, ob, dims));
    assert_eq!(brep_volume(&cut).unwrap(), 3000.0, "the disjoint cut");
    assert!(
        is_empty(run(BooleanOp::Common, ob, dims)),
        "the disjoint common"
    );
    let why = refused(run(BooleanOp::Fuse, ob, dims));
    assert!(
        why.contains("two-shell compound"),
        "the disjoint fuse refusal: {why}"
    );

    // Engulf, strictly interior: the cut is the internal void —
    // refused by name; the fuse is the whole base; the common the
    // whole tool.
    let ob = v3(10.0, 5.0, 1.0);
    let dims = [5.0, 5.0, 2.0];
    let why = refused(run(BooleanOp::Cut, ob, dims));
    assert!(
        why.contains("internal void"),
        "the engulf cut refusal: {why}"
    );
    let fuse = solid(run(BooleanOp::Fuse, ob, dims));
    assert_eq!(brep_volume(&fuse).unwrap(), 3000.0, "the engulf fuse");
    let common = solid(run(BooleanOp::Common, ob, dims));
    assert_eq!(brep_volume(&common).unwrap(), 50.0, "the engulf common");

    // Engulf with the flush faces: the cut is a legitimate notch
    // (B touches the base's boundary, so the complement is a single
    // shell) — the construction's own path, the closed form exact.
    let ob = v3(0.0, 0.0, 0.0);
    let dims = [10.0, 20.0, 5.0];
    let cut = solid(run(BooleanOp::Cut, ob, dims));
    assert_eq!(cut.faces.len(), 6, "the flush-containment cut is a box");
    assert_eq!(brep_volume(&cut).unwrap(), 2000.0, "the flush cut");
    assert_eq!(brep_area(&cut).unwrap(), 1200.0, "the flush cut area");
    let fuse = solid(run(BooleanOp::Fuse, ob, dims));
    assert_eq!(brep_volume(&fuse).unwrap(), 3000.0, "the flush engulf fuse");
    let common = solid(run(BooleanOp::Common, ob, dims));
    assert_eq!(
        brep_volume(&common).unwrap(),
        1000.0,
        "the flush engulf common"
    );

    // Contained (the base inside the tool): the cut is empty, the
    // fuse the whole tool, the common the whole base.
    let ob = v3(-5.0, -5.0, -1.0);
    let dims = [40.0, 30.0, 7.0];
    assert!(
        is_empty(run(BooleanOp::Cut, ob, dims)),
        "the contained cut is the empty set"
    );
    let fuse = solid(run(BooleanOp::Fuse, ob, dims));
    assert_eq!(brep_volume(&fuse).unwrap(), 8400.0, "the contained fuse");
    let common = solid(run(BooleanOp::Common, ob, dims));
    assert_eq!(
        brep_volume(&common).unwrap(),
        3000.0,
        "the contained common"
    );

    // The degenerate identity: B == A — the cut is empty, the fuse
    // and the common the box itself.
    let ob = v3(0.0, 0.0, 0.0);
    let dims = [30.0, 20.0, 5.0];
    assert!(is_empty(run(BooleanOp::Cut, ob, dims)), "A − A is empty");
    for op in [BooleanOp::Fuse, BooleanOp::Common] {
        let s = solid(run(op, ob, dims));
        assert_eq!(brep_volume(&s).unwrap(), 3000.0, "{op:?}: the identity");
    }
}

/// The split cut: the tool slices the base into two disconnected
/// slabs — the construction detects the two shells structurally and
/// refuses by name (a compound is not a single-solid result). The
/// fuse of the same placement is the connected ridge — exact.
#[test]
fn the_split_cut_refuses_by_name() {
    let ob = v3(-5.0, -5.0, 2.0);
    let dims = [40.0, 30.0, 2.0];
    let why = refused(run(BooleanOp::Cut, ob, dims));
    assert!(
        why.contains("splits") && why.contains("2 shells"),
        "the split refusal names the count: {why}"
    );
    let fuse = solid(run(BooleanOp::Fuse, ob, dims));
    assert_eq!(
        brep_volume(&fuse).unwrap(),
        4200.0,
        "the ridge fuse: V = 3000 + 2400 − 1200"
    );
}

/// The frame contract: a rotated tool frame refuses by name (never a
/// silent snap), and a NEAR-parallel frame refuses too — the extents
/// would be the snap's, not the input's.
#[test]
fn rotated_frames_refuse_by_name() {
    let c = std::f64::consts::FRAC_1_SQRT_2;
    // Rotated 45° about z.
    let rotated = [v3(c, c, 0.0), v3(-c, c, 0.0), AZ];
    let why = refused(run_frame(
        BooleanOp::Cut,
        O,
        IDENTITY,
        DIMS,
        v3(25.0, 15.0, 2.0),
        rotated,
        [10.0, 10.0, 6.0],
    ));
    assert!(
        why.contains("not exactly parallel") && why.contains("snap"),
        "the rotated-frame refusal: {why}"
    );
    // Near-parallel: one ten-billionth of a turn — still refused.
    let eps: f64 = 1e-10;
    let near = [
        v3((1.0 - eps).sqrt(), eps.sqrt(), 0.0),
        v3(-eps.sqrt(), (1.0 - eps).sqrt(), 0.0),
        AZ,
    ];
    let why = refused(run_frame(
        BooleanOp::Cut,
        O,
        IDENTITY,
        DIMS,
        v3(25.0, 15.0, 2.0),
        near,
        [10.0, 10.0, 6.0],
    ));
    assert!(
        why.contains("not exactly parallel"),
        "the near-parallel refusal: {why}"
    );
}

/// The frame mapping runs exact: a permuted, sign-flipped tool frame
/// (the same corner bite expressed in a different basis), a mirrored
/// base frame (the left-handed handedness), and the whole scene
/// rigidly rotated — the measures are the canonical corner bite's.
#[test]
fn permuted_signed_and_mirrored_frames_run_exact() {
    // The permuted + sign-flipped tool: axes (AY, −AX, AZ) with dims
    // (10, 10, 6) at origin (35, 15, 2) spans x ∈ [25, 35],
    // y ∈ [15, 25], z ∈ [2, 8] — the canonical corner bite.
    let out = solid(run_frame(
        BooleanOp::Cut,
        O,
        IDENTITY,
        DIMS,
        v3(35.0, 15.0, 2.0),
        [AY, v3(-1.0, 0.0, 0.0), AZ],
        [10.0, 10.0, 6.0],
    ));
    assert_eq!(out.faces.len(), 9, "the permuted frame corner bite");
    assert_eq!(
        brep_volume(&out).unwrap(),
        2925.0,
        "the permuted frame volume"
    );
    assert_eq!(brep_area(&out).unwrap(), 1700.0, "the permuted frame area");

    // The mirrored (left-handed) base frame: axes (AX, AZ, AY) with
    // dims (30, 5, 20) spans the same base box; the tool in the same
    // frame — the corner bite again.
    let out = solid(run_frame(
        BooleanOp::Cut,
        O,
        [AX, AZ, AY],
        [30.0, 5.0, 20.0],
        v3(25.0, 15.0, 2.0),
        [AX, AZ, AY],
        [10.0, 6.0, 10.0],
    ));
    assert_eq!(out.faces.len(), 9, "the mirrored frame corner bite");
    assert_eq!(
        brep_volume(&out).unwrap(),
        2925.0,
        "the mirrored frame volume"
    );
    assert_eq!(brep_area(&out).unwrap(), 1700.0, "the mirrored frame area");

    // The rigidly rotated scene (90° about z): the base and the tool
    // both rotated — the measures are invariant.
    let out = solid(run_frame(
        BooleanOp::Cut,
        O,
        [AY, v3(-1.0, 0.0, 0.0), AZ],
        [30.0, 20.0, 5.0],
        v3(-15.0, 25.0, 2.0),
        [AY, v3(-1.0, 0.0, 0.0), AZ],
        [10.0, 10.0, 6.0],
    ));
    assert_eq!(out.faces.len(), 9, "the rotated scene corner bite");
    assert_eq!(
        brep_volume(&out).unwrap(),
        2925.0,
        "the rotated scene volume"
    );
    assert_eq!(brep_area(&out).unwrap(), 1700.0, "the rotated scene area");

    // The common is the overlap box in ANY of the frames: the
    // permuted tool frame as Base, the canonical box as Tool (Common
    // is commutative).
    let common = solid(run_frame(
        BooleanOp::Common,
        v3(35.0, 15.0, 2.0),
        [AY, v3(-1.0, 0.0, 0.0), AZ],
        [10.0, 10.0, 6.0],
        O,
        IDENTITY,
        DIMS,
    ));
    assert_eq!(brep_volume(&common).unwrap(), 75.0, "the permuted common");
}
