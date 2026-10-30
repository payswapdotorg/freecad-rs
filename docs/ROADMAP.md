# Roadmap

Honest, incremental, no vaporware.

> **Verified baseline** — v0.1.7 (counted 2026-10-24; the counts
> include the v0.1.8 exact seam weld in flight at count time): nine
> crates, **512 tests** (per-crate counts and method in
> [`VERIFICATION.md`](VERIFICATION.md) — the authoritative green run
> is CI: `cargo build` / `cargo test --workspace --locked` /
> `clippy --workspace --all-targets -- -D warnings` / `cargo fmt
> --all -- --check`).

## Done

- [x] **FC-16 / parity** "Same as FreeCAD" — verified against the real
      FreeCAD 1.0 binaries (AppImage r39109) both directions: the
      kernel's primitive conventions now match FreeCAD's own
      (corner-origin box, base-on-XY cylinder/cone — checked with
      `Shape.BoundBox`); booleans consume placed operands; `Part::
      MultiFuse` and `Part::Mirroring` load and execute; real-FreeCAD
      files restore (nested `LinkList`s, `LinkSub` profiles, sketch
      `GeometryList`, quantity-typed dimensions, empty links); our
      files restore in real FreeCAD (parameters, links, placements,
      sketches — one touch+recompute rebuilds natively in OCCT).
      The full matrix: [`docs/EQUIVALENCE.md`](EQUIVALENCE.md).
- [x] Workspace scaffold + CI (build, test, clippy, fmt — the test step
      + `work/**` push triggers were added in FC-5)
- [x] `fc-base`: vectors, quaternions, matrices, placements, bounding boxes
- [x] `fc-app`: properties, documents, dependency-ordered recompute
- [x] `fc-part`: analytic primitives, tessellation, FCBREP
- [x] `fc-io`: `.FCStd` read/write (FreeCAD-compatible `Document.xml`)
- [x] `freecad-rs` CLI: new/add/set/get/list/recompute/info/stats/export/graph
- [x] Test suite: 361 tests across all six crates (unit + integration;
      re-verified at `08270c6`) — a point-in-time count; the current
      count, method and per-crate breakdown live in
      [`VERIFICATION.md`](VERIFICATION.md)
- [x] `Part::Cut / Fuse / Common` — BSP mesh booleans with a
      T-junction stitch pass; watertight for watertight box/cylinder
      operands; hybrid splitter (tangent-plane scoring + bounding-box
      midplanes) keeps the tree balanced and 2 MiB stacks safe
- [x] Expression engine — FreeCAD's `App::PropertyExpressionEngine`,
      distilled: parser, degree-mode trig, unit folding, document
      bindings with dependency-graph edges, cycle detection,
      `.FCStd` persistence (`<Expression path=… expression=…/>`)
      (superseded by the FC-6 quantity-typed engine below)
- [x] OBJ/STL mesh export (ASCII + binary STL; FreeCAD's
      `Mesh` export architecture)
- [x] **FC-5** Undo/redo — FreeCAD's `App::Transaction` architecture:
      transaction stack with add/del/change records, property
      snapshots, auto-record on mutation, stack cap; `undo`/`redo`/
      `history` CLI over a sidecar session journal (`FILE.undolog`)
- [x] **FC-5** Units & quantity — FreeCAD's `Base::Quantity`
      architecture: 8-dimension `Unit`, `Quantity` arithmetic with
      dimension checking, parser (`"9.8 m/s^2"`, `"30°"`, scientific
      notation), conversion table incl. affine `C`/`F`, canonical
      storage (mm/kg/s/A/K/mol/cd/deg)
- [x] **FC-5** Sketcher v1 — `fc-sketcher` crate: point/line/circle/
      arc geometry, 16 constraint kinds, Levenberg-Marquardt solver
      (fd Jacobian, normal equations, honest DOF/overconstraint
      reporting, failure leaves geometry untouched), FreeCAD-style
      `<Sketch>` XML fragments
- [x] **FC-5** CLI integration — `calc` (quantity arithmetic with unit
      checking), `sketch-solve` / `sketch-show`
- [x] **FC-6** Sketches inside documents — `Sketcher::SketchObject` as
      a first-class `DocumentObject` (execute = solve; a failed solve
      marks the sketch `Error` and fails its dependents, FreeCAD
      behavior), `.FCStd` persistence as a `<Sketch>` fragment, CLI
      verbs `add sketch` / `sketch-import` / `sketch-export` /
      `sketch-info` — d192564, with loaded-sketch status semantics
      fixed in 622da7e
- [x] **FC-6** Quantity-typed expressions — the engine evaluates to
      `Quantity` instead of folding units into `f64` at parse time:
      dimension-checked `+`/`-`, unit-combining `*`/`/`, integer
      exponent unit algebra, degree-mode trig, honest binding
      rejection (a plain-float property rejects a unit-carrying
      expression, FreeCAD `PropertyFloat` behavior) — e04c0a5
- [x] **FC-6** Profile kernels — closed-wire extraction from sketch
      geometry (deterministic head-to-tail chaining at
      `Precision::Confusion`, even-odd nesting classification with
      CCW outer / CW hole orientation), extrusion (Pad) and revolution
      (Revolution) into watertight-by-construction meshes:
      ear-clipped, hole-bridged caps; seam-welded full turns —
      8957afe, merged as 7726c1f
- [x] **FC-6 integration** PartDesign v1 — `Pad` / `Pocket` /
      `Revolution` document objects consuming sketches
      (Profile/Length/Midplane/Reversed, Angle/Axis, Base/Inverted
      semantics), CLI verbs `pad` / `pocket` / `revolution`, `.FCStd`
      persistence with round-tripped links and dependency edges;
      parametric propagation (sketch edit → feature re-executes) and
      save/reload round-trip proven by tests — 08270c6

- [x] **FC-8** Curved-seam boolean robustness — sphere-sphere seam
      repair: consensus-grid snap, tolerance-based trace interleaving
      (the T-junction stitch generalized), fin welding, overlap
      thinning, rim capping under a sagitta honesty bound —
      wholesale-or-nothing under a 5% volume guard; already-closed
      results pass through bit-identically; near-tangent lenses
      thinner than a chord stay open honestly — 66d2994
- [x] **FC-8 / M2** Native-file interop — reader breadth over real
      `Document.xml` variety (`App::Part`, `PartDesign::Body`, origin
      planes/axes, groups, links; quantity/vector/color/enumeration/
      string-list/float-list/integer-list/matrix properties; extended
      `LoadReport` with per-property skip records), a committed
      fixtures corpus with expectation files, round-trip `list`-parity
      gate, `.brp` skip asserted — c7770ea
- [x] **FC-8 / M4** Embedded scripting — `fc-script`: rhai 1.26
      sandboxed document API mirroring the CLI verbs, expression
      bindings through the `fc-app` engine, honest failure semantics
      (failed recompute → error surfaced, non-zero exit), `script`
      CLI verb, committed example (`build-house.rhai`) — 65189af

- [x] **FC-8 wave 1** Curved-seam boolean robustness — sphere-sphere
      T-junction repair (WOK) — 9f4ac82
- [x] **FC-8 wave 1** M2 native-file interop — reader breadth +
      fixtures corpus (WOM) — 47258ee
- [x] **FC-8 wave 1** M4 embedded scripting — rhai document API +
      `script` verb (WON) — 8761490
- [x] **FC-9/M1** Interactive 3D viewer — `fc-viewer`: wgpu 30
      surface-less offscreen renderer, svg-parity cameras, CPU
      orthographic picking, error-flagged objects, PNG output +
      `--bless` goldens, `interactive` feature (winit; build-tested),
      CI viewer job on lavapipe — 82005de
- [x] **FC-9/M3** STEP exchange — `fc-step`: ruststep 0.4 parser
      layer, faceted AP203 export with the marker in the file,
      deflection-parameterized import (planar + cylindrical faces),
      `Part::Feature` carrier with persistence, CLI verbs
      `import-step` / `export step` — 8c4668b
- [x] **FC-12** Boolean robustness on mixed curved operands — the
      seam repair's tolerance scale now measures the seam itself (the
      median boundary-edge step) instead of the facets the seam
      crosses, which overestimated it ~10–20× on coarse operands and
      made every downstream tolerance loose enough to weld across
      real structure; crossed cylinders (Steinmetz common = 16r³/3
      exact-form), parallel offset cylinders (extruded lens,
      closed form), a tilted cylinder through a sphere, a tilted
      cone against a sphere, and a skew cylinder pair all close
      watertight with cut+common partitioning within the
      deflection-derived tolerance — the final roadmap candidate,
      closed with the roadmap
- [x] **FC-13** Near-tangent mixed pairs & cone-cone seams — the
      degenerate pruner's reopen is a *needle slit* (a boundary edge
      with the same sheet's vertices lying on it), which caps cannot
      fan (sub-bar) and pinholes cannot collapse (arc vs span);
      `zip_boundary_t_junctions` inserts the chain into the owning
      triangle bit-exactly; `peel_overlapped_edges` removes the
      over-covers conservative thinning declines; the volume guard
      gains the repair's own authority floor (seam length x scale
      squared) so razor lenses are not held to zero tolerance; the
      acceptance gate now requires a manifold mesh (every edge
      exactly twice), not just an empty boundary. Fixtures: near
      tangent sphere pairs and sphere-cylinder pairs at d in
      {1.9, 1.95, 1.99} (all three ops watertight, degenerate-free,
      partitioning the operands), exact tangency (empty common
      honestly), crossed cones and coaxial opposed cones at
      deflection 0.06 (partition identities within 1%, hourglass
      common within the tessellation-derived tolerance of 3pi),
      bit-determinism on the razor fixture

- [x] **FC-14** Mirrored near-coincident seams at fine deflections —
      a cone against its own z-mirror (the hourglass pair) at
      deflection 0.04 left an **apex slit**: four boundary spokes
      from the two tips down to two adjacent waist samples. Welding
      through the slit's shared-vertex pairs satisfied the old
      segment-distance gates while merging the two apexes 3.99 apart
      (a pair of adjacent slit spokes puts each far endpoint on the
      other's *span* at a full spoke's length), annihilating one
      bicone flank; the honest volume guard then reverted the whole
      repair and the result stayed open. Fin welding now requires the
      merged endpoints themselves to be sub-scale — endpoint to
      endpoint, not endpoint to span — so the slit closes through its
      razor-width waist weld instead. Fixture: the mirrored pair swept
      at deflections {0.06, 0.05, 0.04, 0.03}, all three ops
      watertight, degenerate-free, partitioning within the operands'
      tessellation tolerance, bit-deterministic
- [x] **FC-14** Exact-tangency slivers — at d = r1 + r2 the
      exact-weld path closes bit-identically but carried zero-area
      triangles at the tangent point (distinct, collinear vertices —
      the grazing clip's slices through one operand facet). Pruning
      them would violate the closed-mesh contract, so the dedicated
      filter **collapses** instead: each same-sheet fold's shortest
      edge merges onto the lower index, dropping the sliver plus the
      sub-scale needle strip that shared the edge, re-pairing the
      surrounding link edge for edge. Cross-sheet degenerates (the
      coplanar seam's exact-weld interlock) and folds whose collapse
      dents real structure (caught by a 0.1% volume bar — the slivers
      live at a zero-measure contact, so an honest collapse moves
      noise, never volume) stay untouched, wholesale-or-not-at-all.
      Fixtures: exactly tangent sphere pairs at deflections
      {0.1, 0.2, 0.3, 0.4} — fuse/cut watertight, degenerate-free,
      volume-exact, `common` honestly empty, bit-deterministic; the
      planar box fixtures stay bit-identical byte for byte
- [x] **v0.1.7** Exact boolean classification — the tolerance regime
      is gone: vertex sides decided by exact integer predicates
      (certified `f64` filter → i256 fast path → 104-word wide integer
      for adversarial exponent spreads, `crates/fc-part/src/exact.rs`)
      with a construction-noise band seven orders below the old
      `PLANE_EPS`; golden tests against exact rational references
      (`scripts/exact_boolean_reference.py`) — the 1e-6 blind spot
      cut exactly, the rotated-interior box exact to 1e-9; the
      rotated-crossing planar-seam defect discovered and recorded
      executably (an `#[ignore]`d test + honest-list entry) — 575f3b4
- [x] **v0.1.8 (landing)** Exact seam weld — closes the
      rotated-crossing defect: `weld_exact_seams` (twin weld at
      construction noise 2^-40, T-junction interleave at the seam's
      own noise) plus `collapse_needle_caps`, wholesale-or-revert
      under a noise-implied volume bound; the former defect record
      runs green — `boolean_rotated_box_crossing_faces_matches_exact_reference`:
      cut 430.5 / fuse 646.5 / common 49.5 against the exact rational
      reference, watertight
- [x] **v0.2.0 + v0.2.1** The browser workbench — **`fc-wasm`**, the
      whole document engine compiled to `wasm32-unknown-unknown`
      behind a C ABI (`fc_alloc`/`fc_free`/`fc_set_time`/
      `fc_command`; one JSON command protocol — 20 ops, 11 native
      tests), and **`fc-studio`** ([`studio/`](../studio/)), the
      workbench over it: document tree with status badges, property
      editor with live set→recompute and per-property expression
      binding, add × 15 feature kinds (primitives, booleans,
      sketch→pad/pocket/revolution, mirroring), undo/redo, `.FCStd`
      in/out (drag-and-drop), STL/OBJ export — all computed
      client-side, the viewport hand-rolled WebGL (zero JS
      dependencies: Z-up orbit/pan/zoom, ray-triangle picking, the
      engine's tessellations, flat shading). Browser-verified
      end-to-end: sketch→pad lands on exactly **1440 mm³** (the same
      number real FreeCAD 1.0 writes for the same body), parametric
      edits, expression bindings, undo/redo, the save→load round-trip.
      v0.2.1 makes it **durable**: the workbench lives in-repo
      (previously only in the sandbox that happened to serve it) and
      releases ship a single self-contained `fc-studio.html` (engine
      embedded as base64 — no server, works from `file://`), gated by
      a CI `studio` job (build + package + sanity checks) and a
      `release` workflow job that attaches the asset
- [x] **v0.2.2** Feature patterns — the mechanical workflow
      (recommendation #5 of the external review):
      `PartDesign::LinearPattern` (Direction/Length/Occurrences —
      spacing `Length/(N−1)`, occurrence 1 untransformed) and
      `PartDesign::PolarPattern` (Axis/Angle/Occurrences — FreeCAD's
      360° = 360/N rule so the last copy never stacks on the first).
      The original's **tool** is re-derived from live properties on
      every recompute (patterns are parametric through the whole
      chain); additive originals fuse their copies, pockets cut them;
      **nested patterns compose multiplicatively** (FreeCAD
      `Transformed` semantics — the 2×2 hole grid from one pocket via
      two chained linear patterns). Copies fold into one tool union
      applied in a single boolean — set-theoretically identical to
      per-copy folds and ~140× faster (the polar bolt circle went
      from >4 min to 1.7 s). CLI verbs `pattern-linear` /
      `pattern-polar`, `.FCStd` persistence round-trip, wasm kinds +
      the studio Add menu. Acceptance-first (recommendation #6): the
      E2E test `tests/mechanical_cli.rs` — the bracket plate (120×80
      plate, 4 corner holes via nested patterns, 6-hole bolt circle
      via polar, every intermediate volume asserted against
      engine-measured tool volumes, parametric cascade, undo,
      reload) — was written BEFORE the implementation and watched
      fail. Browser-verified end-to-end in fc-studio: the same
      bracket built through the real dialogs, exact volumes at every
      step, the parametric cascade live, VLM-verified clean render
      (the viewport now draws the tip — FreeCAD's Body→Tip rule —
      instead of every cumulative solid, which z-fought)

## Next: the kernel exactness program (K1–K5)

Accepted in
[`adr/0003-geometry-kernel-strategy.md`](adr/0003-geometry-kernel-strategy.md):
pure-Rust exactness as its own program, with its own milestones —
not an OCCT wrapper. Each lands with committed tests; K2 is already
behind us.

- [x] **K1** exact topology model — `fc-part` `topology.rs` + FCBREP v2:
      vertices/edges/faces carrying parametric curves & surfaces (line,
      circle, ellipse, plane, cylinder, cone, sphere, torus, rational
      B-spline — de Boor evaluation and derivatives, quotient rule for
      the weighted ones). Exact generators for the five analytic
      primitives: closed edges carry one vertex, the sphere poles and
      plain-cone apex are zero-radius degenerate circles (excluded from
      the Euler count — `V − E_nd + F` gives 2/2/2/2/0), every
      non-degenerate edge is used exactly twice (closed shells), edge
      endpoints are bit-exact against the vertex table (a closed edge's
      range end denotes the closure — full-turn trig is not
      bit-reproducible at 2π, and the model says so), wires are closed
      vertex-continuous loops, and the structural payload round-trips
      byte-identically (FCBREP v2 in `brep.rs`, strict line-numbered
      parser; v1 primitive payloads still parse, cross-version reads
      refuse honestly). The sphere's surface follows OCCT's latitude
      convention (`p = r·(cos v·ring(u) + sin v·axis)`, ∂u×∂v outward).
      Acceptance-first: the 17-test suite in
      `crates/fc-part/tests/topology.rs` was written and watched
      failing before the implementation; it caught the sphere
      parameterization bug (a colatitude/latitude mixup that
      double-covered the northern hemisphere) and the torus/sphere
      ring-component swaps before they could ship
- [x] **K2** robust predicates — **done in v0.1.7**: exact
      classification (`crates/fc-part/src/exact.rs`; the 1e-6
      blind-spot golden test)
- [x] **K3a (landing 2026-10-26)** exact intersections of the
      analytic surface pairs — `fc-part` `intersect.rs`:
      `intersect_surfaces` computes, in closed form from the
      operands' parameters (no iteration, no tolerance, no
      sampling), the section curves of every pair whose section is
      representable in the K1 curve set — plane×plane (the
      cross-product line), plane×sphere (projection circle),
      sphere×sphere (radical-plane circle), plane×cylinder (the
      three alignments: axis-meet circle / two-or-tangent generator
      lines / stretched ellipse), plane×cone (axis-meet circles on
      **both nappes** of the OCCT double-cone parameterization,
      generator line pairs / tangent generator / apex point for
      bit-exact apex planes, the oblique section ellipse from the
      completed quadratic), plane×torus (axis-perpendicular z-cuts),
      and the coaxial families (cylinder×sphere, cylinder×cone,
      cylinder×torus, sphere×cone, cone×cone, torus×torus with
      identical radii). Every configuration decision — secant /
      tangent / disjoint / coincident — is an **exact integer-sign
      computation** over the stored `f64`s (error-free-transform
      expansions, Shewchuk-style: `two_prod`/`two_sum`/partial
      accumulation — the `exact.rs` discipline at the classification
      tier), so the one-ulp-off tangencies classify honestly
      (`tangency_classification_is_exact_not_tolerant`). Pairs whose
      sections are quartic space curves (Steinmetz
      cylinder pairs, non-coaxial mixes), hyperbolas/parabolas
      (steep plane×cone), oblique Villarceau torus cuts and B-spline
      surfaces are **refused by name** — `NotAnalytic { why }` —
      never approximated. Same-kind pairs canonicalize operand
      order lexicographically, so `intersect(a, b)` and
      `intersect(b, a)` are bit-identical. Acceptance-first: the
      39-test suite in `crates/fc-part/tests/intersect.rs` (every
      returned curve dense-sampled against **both** operands'
      implicit equations, distinguished points re-derived
      test-side from the classical constructions) was written
      before the implementation; it caught the author's own
      double-cone blindspot (cone×cylinder coaxial is **two**
      circles, one per nappe, and same-semi shifted-apex cones meet
      in **one** circle, not none) before it could ship. Remaining
      for full K3 closure: K3b — the differential OCCT fixture
      corpus (committed FreeCAD 1.0 section results) — and K3c —
      booleans composing the sections
- [x] **K3b (landing 2026-10-26)** the differential OCCT fixture
      corpus — `scripts/parity/regen_k3_sections.py` generates, via
      the parity gate's own pinned FreeCAD 1.0.0 AppImage (r39109),
      OCCT's `BRepAlgoAPI_Section` curves for the 17 canonical pairs
      (finite primitives generous enough that the wanted sections
      are interior; cap/rim edges filtered by a majority-interior
      rule), recording each curve's parameters and 33 sample points
      from OCCT's own edge parameterization into
      `crates/fc-part/tests/fixtures/k3_sections.json` (committed,
      regenerable). The differential —
      `crates/fc-part/tests/intersect_differential.rs` — is honest
      about direction: OCCT intersected finite shapes, we intersect
      the infinite surfaces, so our answer is a superset; the
      contract is **every OCCT sample point lies within machine
      scale of a closed-form curve** (28 curves, 924 points, all
      covered; lines and circles analytically, ellipses by
      coarse-scan + bisection refinement). It caught a real defect
      before it could ship: `cylinder_cone_coaxial` computed the
      cone's apex along the canonical axis instead of the cone's
      own stored axis — wrong by the apex offset for every
      antiparallel-stored (contracting-frustum) cone; fixed with
      signed slopes in `cylinder_cone` and `cone_cone`
- [x] **K3c-1 (landing 2026-10-26)** exact B-rep booleans composing
      the sections — the sphere×sphere family
      (`crates/fc-part/src/brep_boolean.rs`): the three booleans are
      the four face choices around the shared radical-plane circle
      (the K3a construction, reused bit-identically as the single
      edge) — Common = both caps (the lens), Cut = A's complement +
      the tool's cap as the cut wall (the crescent), Fuse = both
      complements (the peanut) — each a 2-face, 1-edge, 1-vertex
      closed solid with Euler 2 and `validate()` green. The wire
      orientation convention (kept region on the left of the
      traversal viewed from the face's OUTWARD normal) encodes what
      OCCT calls TopoDS_REVERSED — K3c-1 believed that was enough
      for trimmed faces; K3c-2 found it was not (see below) and
      the orientation now lives explicitly on `TopoFace::reversed`.
      The cut wall (solid on the OUTSIDE of the tool sphere) is
      the case that exercises it.
      Exact non-secant classification (expansion signs) with honest
      pass-throughs (whole spheres, empty results) and refusals by
      name: contained cuts (an internal void needs a face-orientation
      flag for untrimmed periodic faces — K4 territory) and
      disjoint/tangent fuses (multi-shell compounds, point
      junctions). 7 acceptance-first tests (`tests/brep_boolean.rs`):
      topology, validation, the bit-identical K3a circle, opposite
      traversals, cap-apex side checks, cap-formula volume partition
      identities (V_lens + V_(A−B) = V_a; V_fuse + V_lens = V_a +
      V_b), FCBREP round-trip, and the d1 line probe. They caught
      the author's own inverted containment sign and the missing
      cut-wall reversal before either could ship
- [x] **K3c-2 (landing 2026-10-26)** exact measure on composed
      B-reps + the sphere×halfspace plane cuts — and the
      face-orientation model both forced
      (`crates/fc-part/src/measure.rs`, `brep_boolean.rs`):
      * **The model fix.** Writing the volume tests exposed a real
        K3c-1 defect: Cut and Fuse of a secant pair produced
        byte-identical face lists — the crescent's cut wall and
        the peanut's complement face share surface, wire, and flag,
        differing only in which side of the surface the solid lies
        on. `TopoFace` now carries `reversed: bool` (the OCCT
        REVERSED-face flag: the outward normal is the natural
        `∂u×∂v` negated); FCBREP v3 serializes it (v2 payloads
        still read, defaulting unreversed; the version-strict
        parser rejects v3 lines under a v2 header).
      * **`measure.rs`**: `brep_volume`/`brep_area` — the
        divergence theorem `V = (1/3)∮p·n̂dA` per face kind in
        closed form, reading every trim from the WIRES (never the
        uv boxes): plane polygons (shoelace) and disks (`p·n̂` is
        constant on a plane — no centroids ever), sphere full/cap
        (the patch's vector area `πr(r²−s²)k̂` makes any oblique
        circle exact), cylinder strips (`∫r̂du = 0` kills the
        origin term), cone frusta (all tangent planes through the
        apex — the definite integral collapses), the full torus.
        Honest refusals by name: B-splines (K4), untrimmed
        planes/cylinders (unbounded), unrecognized rims.
      * **`sphere_plane_boolean`**: the halfspace tool
        `{x : n·(x−p) ≥ 0}` — Common keeps the +n cap (spherical
        patch + planar disk on the K3a plane×sphere circle,
        bit-identical), Cut the −n cap; exact secant/tangent/
        disjoint classification (expansion signs); Fuse refused as
        unbounded. The disk's frame is the circle's own, so its UV
        box is exactly `[−ρ, ρ]²`.
      * 10 acceptance-first tests (8 in `tests/measure.rs` + 2 in
        `tests/brep_boolean.rs`): every K1 primitive measures what
        `TopoShape::volume()/surface_area()` report (two
        independent computations agreeing, including the
        contracting-frustum signed-slope case); the lens/crescent/
        peanut measure their classical cap compositions with
        `V(peanut) − V(crescent) = V(b)` exactly — the identity that
        is IMPOSSIBLE on the pre-fix byte-identical B-reps; plane
        caps verify through `πh²(3r−h)/3` for oblique normals,
        negative offsets, and exact hemispheres; FCBREP v3
        round-trips byte-identically with v2 back-compat. They
        caught the author's own test-side errors too (a sign slip
        in the cap-height expectation — the +n cap's height is
        `r + s`, not `r − s` — and a "hemisphere" whose plane
        missed the center: the measure returned the exact cap
        volume for the actual offset, 30.025 vs the intended
        16.755, which is how the setup error was found)
- [x] **K3c-3 (landing 2026-10-26)** the oblique cylinder/cone
      plane cuts with elliptical rims
      (`cylinder_plane_boolean` / `cone_plane_boolean` in
      `brep_boolean.rs`): the halfspace tool composed around the K3a
      section curve, bit-identically. The cylinder pieces verify by
      **Cavalieri** (`V = πR²·(mean height)` — an oblique cut leaves
      the mean height alone; the two pieces sum to the whole
      cylinder); the cone's apex piece by the classical **oblique
      cone** `V = (1/3)·πab·d(apex, plane)` — exact for any
      obliqueness; the frustum pieces by cone subtraction. The
      composed faces carry the cone in the apex-origin
      parameterization (axis apex→base, semi in `(0, π/2)`,
      ref_radius 0) so the K3a section is the curve OCCT's own
      parameterization would name; the strip's seam — the
      periodicity glue the Euler count needs — is a degree-1
      B-spline whose de Boor evaluation returns its endpoints
      exactly (a `Line`'s `origin + direction` cannot, in general).
      `measure.rs` reads the new trims in closed form: the
      elliptical disk (`πab`), the **sinusoid trim** on the cylinder
      (the rim's own plane reduced in the cylinder's frame; the
      `z0`-coupling vanishes over the turn, the `(A, B)` part
      doesn't), and the **Möbius trim** on the cone — the plane's
      height `v_e(u) = (γ0 − r1·m(u))/(κ + M̃·cosθ)` — through the
      standard `∫du/(κ + M̃·cosθ)` and `∫cosθ·du/(κ + M̃·cosθ)²`
      moment families, the catastrophically-cancelling `I_c`
      rationalized. Every configuration classification is an exact
      expansion sign (`g²` vs `r²(1 − β²)` through EFT products);
      unrepresentable sections refuse by name: generator lines
      (plane ∥ axis, secant), the hyperbola/parabola class
      (steep cone planes), cap-clipping cuts, a plane through the
      apex. 11 acceptance-first tests (9 in `tests/brep_boolean.rs`
      + 2 in `tests/measure.rs`); the measure-recognition refusals
      pinned by hand-built shells (an ellipse not on the surface, a
      sinusoid touching the rim).
- [x] **K3c differential (landing 2026-10-26)** the OCCT corpus for
      the boolean results — `scripts/parity/regen_k3c_booleans.py`
      generates, through the parity gate's pinned FreeCAD 1.0
      AppImage, OCCT's own `cut`/`common` volumes and boundary areas
      for the eight canonical plane-cut configurations (general
      oblique, flipped normals, a tilted cylinder axis, perpendicular
      sections; the plain cone, the frustum) into the committed
      `crates/fc-part/tests/fixtures/k3c_booleans.json`. The
      differential (`tests/measure_differential.rs`): our exact
      composition + closed-form measure agrees with OCCT's quadrature
      to 4.4e-9 relative (volume) / 2.8e-9 (area) — the perpendicular
      sections came out bit-exact. The halfspace tool is proxied by a
      generous box (an order of magnitude larger than the operand),
      one face ON the cutting plane — the honest proxy is recorded in
      the JSON itself.
- [x] **K3d (landing 2026-10-27)** cylinder×cylinder — the
      parallel-axis compositions. The section (the two generator
      lines, K3a, reused bit-identically) makes every plane ⊥ the
      axes the SAME two-circle picture, so the booleans are prisms of
      the planar circle-circle booleans over the z-overlap: the
      **Common** lens prism (each cylinder's inside-arc strip over
      the clamped overlap — any z-stagger), the **Cut** with the
      tool's z-range covering the target's (the target's outside-arc
      strip, the tool's inside-arc wall — a REVERSED face, the solid
      outside the tool — and the disk-minus-lens caps), the **Fuse**
      of equal ranges (both outside-arc strips, union caps), and the
      nested **tube** (the tool strictly inside cross-wise: the
      target's mantle, the tool's mantle reversed, annular
      multi-wire caps). The enabling representation: the arcs are the
      classical **rational quadratic B-splines** (NURBS circle arcs —
      control points [start, tangent-intersection, end], weights
      [1, cos(sweep/2), 1], clamped knots, de Boor exact at both
      ends), because a `Circle` parameter range cannot share the
      crossing vertices bit-exactly across the two operands' frames;
      sweeps wider than a quadrant split (the classical
      conditioning rule). The measurement reads them back exactly:
      cap areas by the closed-form line integral
      `∫(x dy − y dx) = R²Δθ + c × Δp` per arc, strips as swept
      angular intervals (`F = σRΔv(R|Δθ| + o·W)`). Volumes verify
      against the classical lens-area prisms and the partition
      identities to 1e-12. The **blind drill** (K3d-2, same day):
      the tool strictly inside the target's range — the notched
      mantle (the outer wire the K1 rim pattern, the hole wire the
      lens window; the measure reads the full strip minus the swept
      window), the tool's wall, its two lens caps, and the target's
      full caps. Honest refusals by name: the Steinmetz
      quartic (non-parallel axes, K3a's own), the one-sided drills
      and the staggered fuse (the split-circle mantles — a later
      slice), the pinched tube of the internal cross-tangency
      (measure zero), tangent/disjoint fuses, and the
      coincident-to-machine-precision reading of ulp-scale axis
      residues. The coaxial cylinder×sphere
      family (the capsule cuts) landed as K3e; the nested staggered
      pieces (the partial bore, the chimney, the sticking-out piece)
      as K3f. Remaining for full K3 closure: the crossing split-circle
      mantles (the one-sided drill and the staggered fuse).
- [x] **K3e (landing 2026-10-27)** cylinder×sphere — the coaxial
      family, the capsule cuts (`cylinder_sphere_boolean` in
      `brep_boolean.rs`): the compositions follow the cross-section
      profile `min(r_c, s(z))` over the z-overlap. **Common** is the
      stacked profile — the capsule (the cylinder band between the
      two K3a section circles joined to the spherical caps; a tangent
      C¹ junction, the smooth spherocylinder), the plane-clipped
      variants (the sphere band between the z=0 mouth and the section
      carried as one four-leg wire — seam, circle, seam back, circle —
      with the seam the classical NURBS meridian arc, exact at both
      ends by the de Boor control points), the slab-clipped sphere
      (the cap, the slice), the whole cylinder (the zone covering the
      slab). **Cut** (cylinder − sphere): the rim-bowl (the mouth
      inside the rim: the annular cap + the reversed bowl to the
      pole), the pinch-bowl (the sphere wider, the section circle
      inside the slab — the piece pinches off, no cap face), the
      groove (the reach covering the slab: the annular caps + the
      reversed spherical inner wall — a solid torus), the empty and
      the whole pass-throughs. **Fuse**: the dome bumps (the annulus +
      the dome per crossing, the mantle intact), the whole-cylinder
      and whole-sphere pass-throughs. The measure reads the new trims
      in closed form: the sphere band `2πr(s_hi − s_lo)` with the
      vector-area flux term (both boundary circles' kept sides
      recovered from the wire flags, the seam chain verified used
      once each way within the face). Every configuration decision is
      an exact expansion sign (the reach/zone/rim-mount/equator
      classifications); the sub-machine perpendicular offset of the
      sphere's center reads as the intended coaxial (the K3d
      precedent), larger offsets propagate K3a's quartic refusal by
      name. Honest refusals by name: the contained cut (the internal
      void), the two-piece cut, the collar (a genus-1 pinch piece),
      the split-circle mantles, the rim-mount (a mouth coinciding
      with a rim), the equator tangency, the tangent/disjoint fuses.
      9 acceptance-first tests (`tests/brep_boolean.rs`): topology
      and orientation per shape, the volumes against the classical
      segment/cap/prism compositions (recomputed test-side), the
      partition identities `V(cut) + V(common) = V(cyl)` and
      `V(fuse) + V(common) = V(cyl) + V(sph)`, the K3a circle
      bit-identity, dense edge sampling against both operand
      surfaces, the machine-precision coaxial reading, FCBREP
      round-trips — including the mirrored from-below compositions.
- [x] **K3f (landing 2026-10-27)** the nested staggered pieces — the
      compositions the covering cases refused as "the next slice"
      (`nested_partial_bore` / `nested_chimney` in `brep_boolean.rs`):
      the **partial bore** (the cut, b inside a cross-wise, b's range
      crossing exactly one of a's ends): one piece — a's full mantle
      and far cap, the annular near cap (a's rim minus b's
      cross-section circle at the cap plane, the K3a-perpendicular cut
      of b at that height), b's wall (REVERSED, the solid outside b)
      down to the bore's floor — b's end disk at the inner rim height,
      the solid beyond it; strictly inside instead: the internal void,
      refused by name. The **chimney** (the fuse, b inside a
      cross-wise, b sticking out of a's range through one or both
      ends): a's whole mantle, the annular caps where the chimney
      passes, the full caps where it does not, and the chimney itself
      (b's mantle beyond the plane, naturally oriented, and its end
      cap) — with the mirrored variant (a inside b, a the chimney).
      The **sticking-out piece** (the cut, a inside b cross-wise): a's
      z-parts beyond b's range are ordinary cylinder pieces — one, or
      two shells (refused). 2 acceptance-first tests: per-shape
      topology and orientation, the volumes against the classical
      prism compositions (the bore's prism over the z-overlap, the
      protruding prisms), the reversed-wall and natural-orientation
      pins, the void refusal, the empty containment, and the mirrored
      variants both ways.
- [x] **K4-1 (landing 2026-10-28)** the OCCT BRep reader — real
      FreeCAD shape payloads (`DBRep_DrawableShape`, the CASCADE
      Topology V3 dialect FreeCAD writes as `.brp` into `.FCStd`
      archives) parsed into the exact topology model
      (`crates/fc-part/src/occt_brp.rs`): the ADR-0001 trigger-3
      question answered by parsing the format instead of binding OCCT.
      The format's own traps, each verified against real
      `BRepTools::Write_s` output: TShape references count in
      **reverse** file order (a `-34` is record 34 of `n`, children
      added first), a wire's children are a *set* (the traversal is
      the vertex-connectivity chain, re-ordered by the reader), an
      edge's vertex children carry start `+` and end `-`, location
      chains list `(datum, exponent)` pairs first-applied first, the
      closed-surface pcurve pair glues its second index to the
      regularity token ("3CN"), trimmed curves (kind 8) wrap their
      basis in the same parameter space, and — the subtlest — an
      **indirect Ax3** (stored YAxis opposing axis×XAxis, as the
      bottom fillet torus carries) mirrors the natural normal, which
      the reader composes into the face orientation and the edge
      parameterization (`t → −t`). **The reconciliation tier**: OCCT's
      own format is not bit-exact (vertices carry 15 significant
      digits, curves 17), so every edge endpoint *resolves* its file
      vertex from the edge's own curve evaluation (recovering the
      full-precision line origins), processed closed-conics-first so
      circle rims define canonical vertices; later edges **pin** to
      them — lines as degree-1 splines (the K3c-3 seam pattern),
      arcs as quadrant-split rational quadratics (the K3d NURBS
      arc), closed circles as the 9-pole NURBS circle with
      `P0 = P8`, B-splines by end-pole replacement — every pin
      measured against the file's own vertex tolerance, refused by
      name beyond it. Degenerate pole edges are the K1 zero-radius
      circles. Measure grew with it: open-circle arc rims (the
      fillet-class trimmed cylinders) and **torus bands** — two-rim
      trimmed tori with the closed-form area and `p·n̂` flux
      (including the center-offset axial term, verified against
      OCCT's own `BRepGProp` volumes). 13 acceptance-first tests
      (`tests/occt_brp.rs`) over a committed OCCT-authored corpus
      (`fixtures/brp/`, regenerated by
      `scripts/parity/regen_brp_corpus.py` under `pip install ocp`):
      the five primitives plus box-with-fillets, oblique-cut cylinder
      (the elliptical rim survives as a real Ellipse), the located
      cylinder (location chains composed against OCCT's world
      vertices), the torus-rim fillet, and the degree-(2,2) B-spline
      patch — each loads, validates (the K1 bit-exactness contract),
      carries its Euler characteristic, measures against OCCT's own
      numbers (the box exactly), and FCBREP round-trips. Honest
      refusals by name: tessellated payloads, periodic B-splines,
      offset/revolution/extrusion/trimmed/bezier surfaces, untrimmed
      periodic faces, void shells, the mixed arc/elliptical-arc
      corner strip of the straight-edge fillet, and the lone
      B-spline patch (not a closed shell).
- [x] **K4-2a (landing 2026-10-28)** the exact-model tessellator,
      `crates/fc-part/src/tessellate.rs` — the infrastructure of the
      viewer path: every edge discretized once (adaptive sagitta
      bisection), so faces sharing an edge inherit the same 3D
      points; per-face parameter-domain triangulation with closed-form
      UV projection (plus a Gauss–Newton refinement for B-spline
      surfaces), seam-continuous parameter unwrapping (the periodic
      seam walks `u = 0` up and `u = 2π` down), degenerate pole edges
      as the K1 sphere wire's pole lines, and exact-coordinate vertex
      deduplication. The honest scope: **planar-faced solids
      tessellate exactly** (the OCCT box reproduces its exact volume
      bit-for-bit through the mesh); curved faces refuse by name —
      the 2D ear-clip fans from a corner across the parameter-
      collinear rim runs (slanted panels through the solid's
      interior), so a curvature-respecting domain triangulation
      (constrained interior points — a grid) is the named next slice.
      The boundary-stitching machinery that re-inserts dropped
      UV-collinear vertices (splitting the spanning triangle edges)
      is in place and correct for planar faces; `ear_clip` grew the
      keep-boundary variant for it.
- [x] **K4-2b** the curvature-respecting domain triangulation —
      DONE (2026-10-28). `tessellate_brep` now triangulates the
      analytic curved five (cylinder, cone, sphere, torus, and
      torus/cylinder bands such as the top fillet) in their parameter
      domains: the boundary walk splits into two u-monotone chains,
      curvature-respecting grid rows (the same sagitta discipline the
      rim sampler applies) stack between them, a two-pointer band
      merge tiles the stack directly (the rim chains enter as genuine
      boundary edges, bit-exact against the neighboring face), and a
      deflection referee splits any interior edge whose chord or
      panel whose center misses the surface by more than the
      deflection. Periodic seams close through shared-point columns
      (one evaluated Vector3 per level, both sides); pole runs
      collapse onto the one 3D pole vertex with the fan emerging from
      the merges. Fixes that landed with it: the sphere's pole-vertex
      u is atan2 of noise (the walk now inherits its u at the axis),
      the pole line's v comes from the closed-form inversion (not the
      walk's last sample), and the torus branch of `surface_uv_of`
      returned the *complement* of v (asin of the cosine — now the
      atan2 of z against rho − R). Acceptance: cylinder, cone,
      sphere, torus, located cylinder and the top fillet tessellate
      watertight (every directed edge once each way), deflection-true
      at the chord midpoints, with inscribed volume/area deficits
      inside the measured polygonal tolerances (cylinder r=2 d=0.05:
      2.712e-2, tolerance 3.0e-2) that quarter when the deflection
      quarters. Known limitation, named: the oblique elliptical cut
      (`cyl_oblique_cut`) — where the rim's window over the mantle
      pinches to a single cell, the seam column's closing edges lack
      a carrier (9 unpaired edges at d=0.05; volume and area already
      within tolerance) — the corner-carrier fix is the next slice,
      and its test carries an `#[ignore]` naming exactly that.
      B-spline faces still refuse by name (no curvature estimate yet
      drives a grid over the patch), as do hole-trimmed faces and
      non-monotone (non-band) trims.
- [x] **K4-2c (landing 2026-10-29)** the wavy-band closures — the
      oblique cut tessellates watertight at every deflection. Three
      defects fell out of the fine runs, all in
      `crates/fc-part/src/tessellate.rs`'s band stack: (1) the
      **pinch-out wedge** — a run whose window shuts before the
      frontier does ended its band at an edge that still faced
      untiled region on its upper side; one closure triangle
      `{under-stretch's last, run's last, first not-under vertex}`
      tiles it, pairing its bottom edge with the previous band's
      upper boundary and its right edge with the final merge's
      carrier of the new chain edge (in the full-width case the three
      vertices are the collinear seam corner and the zero-area emit
      skips it exactly — the analytic five's paths unchanged);
      (2) the **lone seam point's carrier** — a level whose side's
      runs were all punched away joined the frontier but nothing
      carried its seam-column segment; the insertion now emits the
      strip triangle `{frontier vertex before, lone point, frontier
      vertex after}`; (3) the **per-level seam registry** — a wavy
      row punches into several runs, and per-run seam points
      duplicated an attached point with an unattached twin (one 3D
      point, two domain vertices) whose frontier insertion made the
      final merge walk the twin, emitting carriers whose edges
      skipped over the true column points (five unpaired edges at
      d=0.0125); every run of one level now shares one pair of seam
      points, and loneship is decided only after every run had its
      chance to attach. Acceptance: the formerly-`#[ignore]`d
      `oblique_cut_tessellates_watertight_within_deflection` now runs,
      watertight and deflection-true at d=0.05 AND d=0.0125, with the
      volume and area deficits inside the measured polygonal
      tolerances and the deficit shrinking when the deflection
      quarters. The full suite: 649 tests, none ignored. B-spline
      faces still refuse by
      name, as do hole-trimmed faces and non-monotone (non-band)
      trims.
- [x] **K4-1-integration (landing 2026-10-29, worker-b)** the
      payloads live — a `.FCStd` carrying real OCCT BRep geometry
      loads it as `TopoShape::Brep`: the loader's
      `BrpFormat::OcctBrep` branch hands the bytes to K4-1's reader,
      a readable payload becomes live exact geometry (measured and
      tessellated exactly wherever the exact tiers support the
      geometry, refusing by name where they do not — the two-channel
      honesty contract: `volume`/`tessellate` answer exactly or
      visibly — NaN, the empty mesh — while `try_volume`/
      `try_tessellate` carry the refusing face's name), and the
      payload STILL rides back out verbatim on re-save: this kernel
      parses OCCT but writes none of it, so the file's own bytes stay
      the only truthful serialization, and the object keeps the
      file's `Valid` verdict (exactly what a real FreeCAD leaves
      after its own restore). A rejected payload keeps the honest
      skip with the reader's own reason in the warning (non-UTF-8
      bytes are one such verdict). The save side refuses loudly when
      a Brep has no preserved payload to round-trip (built in memory
      — the loader always pairs the two) rather than silently
      dropping geometry no parameter set can rebuild. Acceptance:
      `tests/shape_brep.rs` (4) + `tests/occt_shape.rs` (7); the
      corpus' group_links boxes read (Valid, the OCCT measure
      reconciling with the parameters to 1e-9), partdesign_body's
      payloads reject by name (negative references, wire roots — the
      reader's coverage boundary, documented). Suite: 660, none
      ignored.
- [x] **K4-studio (landing 2026-10-29, worker-c)** the studio's
      golden path — the browser workbench driving the real wasm
      engine, verified end-to-end in a real browser (agent-browser
      over the packaged self-contained build; evidence in
      `docs/evidence/wc-*.png`, the dated section in
      VERIFICATION.md): the five primitives' stats exact against the
      closed forms, property edits exact, expression binding live,
      the Cut boolean exact (V 3.000 / A 14.000), undo/redo exact,
      the `.FCStd` round-trip carrying the expression binding, STL
      byte-exact (684 = 84 + 12·50). The pass pulled one real engine
      defect out with it: the same-object expression binding
      (`Box.Length ← Box.Height * 2` — the studio README's own
      flagship example) failed twice in `fc-app`'s recompute — the
      topological walk read the expression-sourced self-edge as a
      one-object cycle, and the binding evaluator could not resolve
      the self-name spelling (the object sits out of the map while it
      executes). Same-object expression references are
      property-to-property edges inside one object — now skipped in
      the walk and resolved by the evaluator, exactly like the
      bare-name form; a self-LINK stays pathological, two-object
      cycles unchanged. Two studio defects fixed with it (stats on an
      empty document reported as zero, the OBJ text download). Suite:
      662, none ignored.
- [x] **K4-2d/1 (landing 2026-10-30)** the NURBS measure —
      Gauss–Legendre on polynomial integrands, exact, never an
      approximation. A non-rational B-spline face over its
      **natural domain** (one wire whose four edges are the patch's
      own boundary pole rows, bit-exactly — the representation OCCT
      payloads carry; each edge over its full knot domain) reads
      through the quadrature: the flux integrand
      `p·(∂u×∂v)` is piecewise polynomial of degree `3d−1` per
      direction per knot span, and an `n`-node rule integrates
      degree `2n−1` with **zero quadrature error** — the residual is
      evaluation rounding at machine scale, the same scale the
      closed forms carry. The node count is arithmetic, not tuning:
      `n = ceil(3d/2)` per direction (2, 3, 5, 6, 8 for degrees
      1–5). The trims those faces force on their plane neighbors —
      B-spline boundary curves, the only way a closed shell can
      carry a B-spline face — read through the exact **Green line
      integral** `∮ (x dy − y dx)`, itself piecewise polynomial
      (degree `2d−1`) per span, GL-exact with `n = d` nodes. The
      table rows {1,2,3,4,5,6,8} each exist because a degree path
      reaches them (a size-7 row serves no degree on either path —
      absent on purpose; lookups refuse what is not in the table),
      and the inline `quadrature_tests` pin every row against the
      monomial closed forms. With the Green path open, the plane
      reader also gained the **native conic arcs** (`Circle`/`Ellipse`
      edges with partial ranges — the fillet caps' rims): the
      closed-form Green term `Q·(â·n̂)·Δt` + the center's cross with
      the displacement, `Q = R²` or `ab` — previously such wires
      fell to the polygon path, which silently chord-read the arcs
      (never exercised by a measuring test, but dishonest by
      construction; the `has_arc` routing now takes any B-spline
      edge, closing that class). Acceptance: `tests/bspline_measure.rs`
      (10) — hand-built graph "tents" over rectangle footprints with
      hand-derived closed forms (the spline-moment integrals of the
      chosen pole rows): degree (2,2) single-span and two-u-span,
      (3,3) the 5-node rule, (4,4) the 6-node, (5,5) the 8-node,
      the mixed (1,2) ruled tent whose side wall is a plane face
      genuinely trimmed by a curved B-spline column, the bit-exact
      σ reversal, and the three refusals (rational, non-natural
      wire, area); the FCBREP v2 round-trip measures bit-identically.
      Honest limits, named in the module doc and the refusals
      themselves: **rational** B-spline faces refuse (the flux is a
      ratio of polynomials — GL is exact only on polynomial
      integrands), trimmed patches refuse (only the full domain
      reads), the **area** of a B-spline face refuses
      (`|∂u×∂v|` lives under a square root — the flux-only face kind
      returns `NaN` in the area channel, the named refusal in
      `brep_area`), faces above degree 5 and trims above degree 6
      refuse (the table's reach). The straight-edge fillet's corner
      strip is now its ONLY refusal (the caps read exactly) — the
      K4-2d/2 slice. Suite: 673, none ignored.
- [x] **ADR 0004 + slice 1 (landing 2026-10-30)** the exact/faceted
      operation contract, and the exact booleans through the real
      document features — the central integration the handoff asked
      for. [`adr/0004-exact-faceted-operation-contract.md`](adr/0004-exact-faceted-operation-contract.md)
      freezes five rules (one result representation `TopoShape`; shape
      — not mesh — operand resolution with named refusals; an
      explicit, total boolean dispatch; persistence follows
      provenance; honesty channels survive every hop), and slice 1
      implements them on `Part::Cut/Fuse/Common`: a `Brep` operand
      (imported exact geometry) signals exact intent — the operands
      classify to the sphere/cylinder analytic classes (placements
      applied analytically) and the kernel families run through the
      feature (`sphere_sphere`, `cylinder_cylinder`,
      `cylinder_sphere`, operand order respected), the result
      carrying `TopoShape::Brep`; everything the kernel cannot pair
      refuses with the operands and classes named (box-class B-rep,
      B-spline-faced geometry, a faceted tool beside an exact base,
      sphere-Base × cylinder-Tool cut says the swap) — never a silent
      tessellation of the imported geometry. Without a `Brep`
      operand, today's faceted path runs unchanged (primitive pairs
      keep it until a later slice opts them in — the refused
      configurations and recompute-on-load need field time first).
      The loader's mesh bridge for booleans is retired: a readable
      payload IS the feature's last-good computation and lands as the
      exact model (`result_from_payload` keeping the `Valid` promise
      for the bytes that still are the geometry); derived exact
      results are recomputed, not serialized (the save side already
      skipped healthy parametric results; the errored last-good
      refusal stands). Acceptance: `tests/exact_boolean.rs` (7) — the
      decisive chain (an imported real OCCT sphere cuts exactly, the
      operand's bytes riding out verbatim through save → reopen →
      recompute to a bit-identical exact result), the cylinder fuse,
      the exact empty set, the named refusals, the untouched faceted
      path, and the errored last-good + save refusal; the retired
      bridge test became `boolean_payload_lands_as_the_exact_last_good_result`
      (the boolean now measuring OCCT's own cylinder numbers exactly).
      The dispatch's first live exercise pulled a REAL defect out of
      the display tier: the exact boolean outcomes' complement faces
      (a sphere minus its cap) were silently mis-tiled as their
      inverse by the band machinery — a plausible-looking wrong mesh;
      `tessellate_brep` now refuses interior/exterior-ambiguous trims
      by name (trim-domain classification is the K4-2d slice), the
      two-channel contract holding: the crescent measures exactly and
      says why it will not render. Suite: 680, none ignored.
- [x] **K4-2d/2 (landing 2026-10-30)** the mixed-strip fillet corner
      — the straight-edge fillet's corner strip reads: a cylinder
      patch between one circular ARC rim (the constant-v flat-face
      cut) and one open elliptical ARC rim (the Steinmetz section of
      two crossing equal-radius fillet cylinders), the u-interval
      closed by the two tangent generators. The elliptical rim's
      plane reduces to the oblique section's own sinusoid
      `v(u) = z0 + A·cos u + B·sin u`; over the PARTIAL interval the
      full-turn identities do not apply, so both domain integrals
      carry in closed form (`∫g`, `∫g·cos`, `∫g·sin` — `u`, `sin 2u`,
      `sin²u` primitives, the flux's `oₓ/o_y` moments included), the
      full-turn limit reproducing `cylinder_ellipse_strip` exactly.
      `fillet_box` measures OCCT's own numbers to rounding (V
      283.5988157907698 vs 283.59881577988904 — 3.8e-11; A
      300.5486677646161 vs 300.5486677570851 — 2.5e-11), the FCBREP
      round-trip bit-identically. The tessellation needed two real
      band-machinery defects fixed: `same_seam`'s interior check
      matched two-point runs vacuously (the corner's two UNRELATED
      tangent generators paired as a "seam", minting unshared points
      interior to shared straight edges — a torn mesh; a real curved
      seam carries interior samples to verify, n ≥ 3), and
      `splice_merge`'s stale-stretch cut assumed the post-run
      frontier was seam samples (hugging the u-extremes) — the
      widening case (a wavy LOWER chain punching ever-wider runs)
      leaves genuine untiled chain vertices strictly inside the run's
      span, and cutting before them left a torn wedge; the merge now
      reaches interior frontier points. `fillet_box` tessellates
      watertight at every deflection with monotone convergence
      (1.40e-2 → 1.34e-2 → 6.92e-3 → 4.33e-3 toward the exact), the
      wavy-band corpus untouched. Acceptance: the rewritten
      `fillet_box_loads_validates_measures_and_round_trips`
      (measure, round-trip, watertightness, convergence). Honest
      limits, named in the refusals: cone faces carrying open
      elliptical arcs refuse (the corner is the cylinder's), malformed
      corner wires (rims not sharing two corner u values, a sinusoid
      crossing its arc rim) refuse by name. Suite: 680, none ignored.
- [x] **K4-2d/3 slice 1 (landing 2026-10-30)** the box×sphere bite
      taxonomy — an imported box-class operand meets a sphere whose
      center sits bit-exactly on the box's boundary skeleton, and the
      three booleans run exactly through the real `Part::Cut/Fuse/
      Common` (the ADR 0004 dispatch table widened by amendment, the
      first family the exact-intent rule reaches with a LOADED
      box). **The stations**: the corner (the octant bite — the three
      adjacent faces each losing a quarter-disk, their wires carrying
      the boundary arc; the REVERSED spherical patch on the negative
      arc cycle for the cut, the unreversed complement for the fuse —
      Cut and Fuse differ only in the flag, the K3c-2 discipline; the
      octant ball itself for the common), the edge (the quarter ball —
      two faces bitten by half-disks, the patch bounded by two
      half-great-circles between the edge's two sphere points), the
      face (the half ball behind an interior circular hole — the
      holed plane face plus the hemisphere cap, the existing
      machinery). **The spherical-polygon measure**
      (`measure.rs`): a sphere face whose single wire is a loop of
      open, single-use, great-circle arcs reads exactly — the area by
      Gauss–Bonnet (`r²(2π − s·Στ)`, the geodesic turn angles at the
      vertices), the outward vector area by the Stokes line integral
      (`½r²Σ±Θₖn̂ₖ`), the flux `σ(c·V + r·A)` — with the arcs
      recognized in both representations this model carries (the
      native `Circle` arc and the rational-quadratic NURBS chain the
      construction itself authors). **The exactness discipline**: the
      station points are the input parameters themselves, every arc is
      the classical NURBS arc with canonical exact endpoints
      (`push_arc_chain`), every shared vertex comes from one canonical
      corner formula, and the authored arc directions chain
      all-forward into the patch's positive boundary cycle — a small
      theorem the construction leans on, verified for both box
      handednesses at every station (the mirror and rotated-frame
      tests). Acceptance: `tests/box_sphere_boolean.rs` (8) — the
      three stations × the three ops against the hand closed forms
      (the octant/quarter/half shares of `4πr³/3`) AND real OCCT's own
      `BRepAlgoAPI` numbers (`expected_box_sphere.json`, authored by
      `scripts/parity/regen_box_sphere_corpus.py` via the `ocp` 7.8.1
      wheels — the two references agree to ≤4.6e-13; the kernel
      bit-exact at the corner and face stations, ~2e-13 at the edge —
      the rational-arc Green terms' last-bit rounding, the
      fillet-box precedent), the face counts matching OCCT's own
      topology, the FCBREP v2 round-trip measuring bit-identically,
      the classification tier (whole shapes, the exact empty set, the
      named refusals), and `tests/exact_boolean.rs` +4 — the decisive
      chain (the imported 30×20×5 OCCT box cut exactly by a placed
      sphere primitive, save → reopen → recompute re-deriving the
      bit-identical exact result, the operand's bytes verbatim), the
      fuse/common, the edge/face stations through the features, and
      the sphere-Base swap refusal. **Honest limits, named**: the
      center must sit bit-exactly on the station (a within-tolerance
      but off-point center refuses — the section circles would not be
      great circles, and this kernel never silently snaps); the bite
      must stay within its adjacent faces (r below the extents and the
      along-edge span, tangency refused); the general overlaps (the
      center off the skeleton, or interior with the ball crossing
      faces) refuse by name; the contained cut (the internal void)
      and the disjoint fuse refuse as before; box×box and
      box×cylinder are outside the pairing table by name; the render
      channel refuses the polygon-patched and holed faces by name
      (trim-domain classification — the tessellator's band machinery
      reads latitude rims, and tiling a polygon domain through it
      would tear: the refusal replaced a genuinely torn mesh found by
      the first run); OCCT's own authored RESULT payloads
      (`box_sphere_*.brp`, committed as the differential corpus)
      structurally validate but refuse face reading — their
      multi-segment NURBS arcs and split-circle/seam wires are the
      named next reader slice. Suite: 692, none ignored. **The
      remainder of K4-2d/3** (the next slices): the offset face
      stations (the sphere biting from off the surface — small-circle
      sections), box×box and box×cylinder families, the
      polygon-domain tessellation, and the OCCT-authored-result
      reader widening.
- [x] **K4-2d/3 slice 2 (landing 2026-10-30)** the offset face
      stations — the interior center whose ball crosses exactly ONE
      face. The classification's crossing count (axis k's low face
      crossed iff `d(k) < r`, the high likewise — strict, tangency the
      contained arm's own boundary) routes the single-crossing case to
      the new station; two or more crossings refuse by name with the
      count. The bite is the **small-circle cap** the face plane cuts
      from the ball: `ρ = √(r² − δ²)` at center depth `δ ∈ (0, r)`,
      the section circle on the face plane at the center's foot. Cut
      = the box minus the deep cap (the holed face + the REVERSED
      deep cap, height `δ + r`); Fuse = the box plus the shallow cap
      (height `r − δ`, unreversed); Common = the disk (`ρ`) + the
      deep cap. Every patch is a single-circle-trimmed sphere face —
      the CAP class the measure's cap reader already reads exactly
      (`s = ∓δ`, `ρ² + s² = r²`); no new measure was needed, only the
      station. Both sides measured (the low face and the mirrored
      high face): the closed forms (`V_deep = π(δ+r)²(2r−δ)/3`,
      `V_shallow = π(r−δ)²(2r+δ)/3`, the disk `πρ²`, the cap areas
      `2πr(δ+r)` / `2πr(r−δ)`) AND real OCCT's own numbers
      (`face_offset` / `face_offset_high` in the regenerated corpus —
      `box_sphere_cut_face_offset: V=2967.92957499 A=1738.48451001`,
      the face counts 7/7/2 matching the construction), the FCBREP
      round-trip bit-identical. The near-plane centers route here
      exactly too: a center at depth `1e-12` (classified interior,
      not on-plane) constructs honestly — no snap, no refusal —
      because the station's arithmetic IS the input's. **A real
      tessellator defect fell out**: the sphere CAP face (a
      single-closed-circle trim) sailed past the loop-bounds check
      (a latitude spans the u period) into the band machinery, whose
      two u-monotone chains COINCIDE — emitting NOTHING, then passing
      the shell's volume check as an open flat patch (the disk at
      z=5 alone, volume `area·z/3 > 0` — a plausible-looking Ok that
      is not a shell; the slice-1 face station's hemisphere had the
      same nothing-emitted path, masked by its disk sitting at z=0
      where the flat volume reads 0). The tessellator now refuses the
      class by name — the cap and its complement share the loop, the
      wire-orientation classification is the K4-2d slice — never a
      silently open shell. Acceptance: `tests/box_sphere_boolean.rs`
      (10 total) + `tests/exact_boolean.rs` (12 total — the offset
      bite through the real dispatch, all three ops). Suite: 695,
      none ignored. The remaining K4-2d/3 surface: box×box,
      box×cylinder, the polygon-domain and cap tessellations, and the
      OCCT-authored-result reader widening.
- [x] **K4-2d/3 slice 3 (landing 2026-10-30)** the box×box family —
      the rectilinear composition. Two frame-parallel boxes (an
      imported box-class B-rep or a `Box` primitive, placements
      applied analytically) compose exactly through the real
      `Part::Cut/Fuse/Common` — the pairing table's second amendment
      in ADR 0004. **The frame contract**: every tool axis's dot with
      a base axis is EXACTLY ±1 or 0 (permuted, sign-flipped, and
      mirrored frames run exact — the extents recomputed in the
      base's frame, the emission's reading order chosen per face
      handedness); a rotated or near-parallel frame refuses by name —
      this kernel never silently snaps (the extents would be a
      snap's, not the input's). **The construction**: every result
      face is a plane trimmed by an axis-aligned rectilinear wire —
      the rect-minus-rect solver (the full rect, the L at two
      adjacent touches, the step-C at one, the ring at none — outer
      CCW, hole CW — and the one-two bands at a spanning touch),
      every vertex from ONE canonical accumulation formula, every
      edge a `Draft` seam shared by exact endpoint equality. **The
      stations** (the overlap box `O = A ∩ B` classified by how it
      touches the base): the corner (O contains one corner — the
      9-face notch cut whose area is the box's own, the 12-face
      corner union), the edge (one edge — the 8-face through-notch,
      the octagonal-prism fuse), the face (one face — a plain slab
      cut, the stepped-box fuse with the annular shoulder), the slot
      and its wall-side sibling the groove (through on one axis,
      interior on two — genus-1 tunnels, the holed faces, the
      hole-adjusted Euler characteristic 0), and the open groove
      (through one axis, reaching one face with the tool sticking
      past — the L walls, the 2-band top, genus 0).
      **Common is always the overlap box** — the intersection of two
      co-oriented boxes is a box, for every overlap topology at once.
      **Cut reads the overlap box only** (flush tool faces contribute
      nothing — the construction is the input's own arithmetic, no
      snap); **Fuse demands strict straddling** — a flush tool face
      is the coplanar merge, refused by name as the named next
      slice. **The structural shell count** closes both: a
      construction that splits the base into several shells (the
      through-two-axes cut) refuses by name WITH the count — a
      compound is never a plausible two-solid answer. Both sides
      measured, every station: the hand closed forms (`V_A ∓ V_O`,
      `V_A + V_B − V_O`, the per-face area sums) AND real OCCT's own
      numbers (`expected_box_box.json`, authored by
      `scripts/parity/regen_box_box_corpus.py` via the `ocp` 7.8.1
      wheels — 19 result payloads committed as the reader
      differential corpus), the face counts against OCCT's own
      topology, the FCBREP round-trip bit-identical. **The render
      channel is honest BOTH ways — a first**: the hole-free
      rectilinear faces tessellate WATERTIGHT through the planar
      ear-clip with the MESH VOLUME EXACTLY THE EXACT VOLUME (planar
      faces triangulate exactly), while the ring faces (the slot's
      caps, the fuse's shoulders) refuse by name as before. The
      classification tier answers the whole shapes: disjoint (the
      cut the base, the common empty, the fuse the refused two-shell
      compound), contained, engulf (the strictly-interior cut the
      refused internal void — a flush containment is a legitimate
      notch through the construction), and the identity degenerate.
      Acceptance: `tests/box_box_boolean.rs` (11) +
      `tests/exact_boolean.rs` (+4 — the decisive imported-box × placed-box
      cut round trip WITH the working display channel, the
      fuse/common, the edge and open-groove stations, and the four
      named refusals through the features: the rotated frame, the
      flush fuse, the split, the void). Suite: 710, none ignored.
      The remaining K4-2d/3 surface: box×cylinder, the
      polygon-domain and cap tessellations (the trim-domain
      classification), the coplanar-face merge, and the
      OCCT-authored-result reader widening.
- [x] **K4-2d/3 slice 4 (landing 2026-10-30)** the box×cylinder
      family — the cylindrical bore. A box (imported box-class B-rep
      or `Box` primitive, placements applied analytically) meets a
      cylinder whose axis is BIT-EXACTLY parallel to one box axis —
      **the pairing table's third amendment, and its closure: the
      three analytic classes (sphere, cylinder, box) pair
      exhaustively, the compiler's match exhaustiveness the proof —
      the catch-all refusal arm retired.** **The frame contract**: the
      axis dot with exactly one box axis is exactly ±1 (an oblique,
      rotated, or near-parallel axis refuses by name — the section
      circles would not lie in the face planes, and their extents
      would be a snap's); the cylinder is then read in the box's own
      frame — the cross-section disk against the rectangle, the
      axial span against the slab — the antiparallel axis (the
      180°-placement's exact matrix) the same solid read in the
      flipped direction. **The interior-cross-section contract**: the
      disk strictly inside the rectangle, so every annular face is a
      rectangle with a strictly-interior circular hole — a regular
      region; a CROSSING disk (the hole breaking the box's side
      faces) is the trim-domain slice, refused by name; a TANGENT
      disk would pinch the annulus at a point that is no vertex of
      either wire — refused, never silently degenerate (Common
      alone unaffected: the clipped cylinder is the full-disk column
      either way). **The stations** (the overlap against the slab):
      the **through** (the cylinder straddling both faces — the
      genus-1 through-hole cut: four plain faces, two annuli, the
      REVERSED wall band, the hole-adjusted Euler characteristic 0;
      the fuse the box with the axle: the sticking-out bands with
      their caps; the common the clipped cylinder — a plain
      three-face piece), the **blind bores** (crossing exactly one
      face — the cut's annulus + wall + floor disk with the floor's
      outward normal INTO the hole; the fuse the boss: the band and
      cap beyond the crossed face), and **the flush far ends** (the
      cap coplanar with a face plane contributing NOTHING — the cut
      reading the overlap only, the fuse composing the MINIMAL
      eight-face shell where OCCT keeps the coplanar disk as its own
      redundant ninth face: the volumes and areas agree, the face
      counts do not, and the minimal shell is ours). Exactly TWO rim
      circles authored per wall, every edge shared by exactly two
      faces — the closed-shell contract by construction, the
      K1-cylinder mantle pattern (seam + rims) and the nested-tube
      wall precedent (`reversed` carrying which side the solid lies
      on) composed, no new measure needed: the annulus is the plane
      reader's own (shoelace outer, πr² hole), the wall the
      two-rim band, the disk πr². Both sides measured, every station:
      the hand closed forms (`V_box ∓ πr²·depth`, `V_box +
      πr²·overhang`, the per-face area sums) AND real OCCT's own
      numbers (`expected_box_cylinder.json`, authored by
      `scripts/parity/regen_box_cylinder_corpus.py` via the `ocp`
      7.8.1 wheels — 25 result payloads committed as the reader
      differential corpus), the face counts against OCCT's own
      topology, the FCBREP round-trip byte-identical. **The render
      channel is honest both ways**: every annulus-carrying result
      refuses by name (hole-trimmed faces are not tessellated by
      this reader), while the clipped-cylinder commons and the
      whole-shape outcomes (the engulfed cylinder, the untouched
      box) tessellate WATERTIGHT. **The classification tier** answers
      the whole shapes: engulf (the box inside the cylinder — the
      corner-distance test's own convexity argument), lateral
      disjoint (external tangency included), axial disjoint (a
      two-shell fuse refusal), the exactly-flush LANDING (the
      coplanar-merge boundary — a strict overhang composes, a
      landing merges), contained (the common the cylinder, the fuse
      the box, the strictly-interior cut the refused internal void —
      a flush containment the legitimate bore through the
      construction). Acceptance: `tests/box_cylinder_boolean.rs`
      (8) + `tests/exact_boolean.rs` (+5 — the decisive
      imported-box × placed-cylinder cut round trip with the annulus
      display refusal, the fuse/common, the blind and
      antiparallel-axis bites, the four named refusals through the
      features: the rotated placement, the void, the flush landing,
      the cylinder-Base swap — and the retired
      outside-the-pairing-table refusal re-pinned to the
      trim-domain boundary it now is). Suite: 723, none ignored. The
      remaining K4-2d/3 surface: the polygon-domain and cap
      tessellations (the trim-domain classification — the crossing
      disk's side-clipped sections), the coplanar-face merge (the
      flush landing's union), and the OCCT-authored-result reader
      widening (multi-segment NURBS arcs).
- [x] **K4-2d/3 slice 5 (landing 2026-10-30)** the trim-domain
      classification — the render channel closes over the constructive
      families. **The wire-orientation convention every authored face
      carries (the `TopoFace` contract: the traversal keeps the region
      on the LEFT viewed from the face's OUTWARD normal, the outward
      normal `σ·(∂u×∂v)`) is now READ by the tessellator**: the walk's
      UV winding against the parameterization's own orientation picks
      the side — the interior (a spherical polygon, a wavy cap) tiles
      as a UV polygon through the profile kernel's planar-region
      triangulator with the curvature grid inserted as Steiner points
      and the deflection referee splitting chords and panels; the
      exterior (a complement patch — the fuse bulge, the crescent's
      outer face) tiles as the SPHERE's parameter box minus the walk,
      the box walk synthesized with one evaluated seam Vector3 per
      level shared by both sides and one 3D pole per pole line
      (reusing the walk's own pole vertex when the bite reaches one,
      so the shared Draft vertex welds bit-exactly), the walk entering
      as the hole through the kernel's own bridging; a sphere's single
      closed latitude circle is the CAP line case — the two half-boxes
      the walk direction separates, the classified pole's row
      synthesized at the walk's own u values and the band merge
      fanning the degenerate side exactly as the full sphere's pole
      lines do. **The planar path went multi-wire**: the hole wires
      (the annuli, the rectilinear rings) bridge and ear-clip through
      the kernel's hole-bridging triangulator, the hole rims entering
      as the shared edge discretizations they are — the annulus and
      the wall close bit-exactly, and a ring face's rectilinear hole
      triangulates EXACTLY (the box×box family's every constructive
      result now renders with the mesh volume exactly the exact
      volume). Two latent defects the new paths flushed out, both
      fixed: the planar retain filter dropped the collinear ear's
      keep-boundary degenerate panel — the very triangle that carries
      the cross-edge pairing when its vertex is only partially unused
      (the stitch never fires) — so the emit's identical-vertex check
      is now the single dropping point; and a walk through a pole
      tears the u-unwrap (the two meridians up to half a period apart,
      the ±half-period window cannot span the jump) — the
      `split_pole_jumps` post-pass now spells the jump as a pole RUN
      (the full sphere's own spelling) and re-anchors a torn closure
      by shifting the prefix that leads into the run by whole periods.
      **The box-bounding dispatch is winding-blind on purpose**: every
      band-like walk — the K4-1/K4-2 corpus's OCCT-authored wires
      carry OCCT's own orientation conventions, not this kernel's
      region-on-left spelling — takes the band machinery as before
      (the oblique-cut regression proved the point), and the
      primitives' full-box walks (whose authored directions predate
      the convention) are classified by extent alone. Acceptance: the
      three boolean families' constructive results render WATERTIGHT
      with deflection-derived volume bounds — the box×box family
      entirely (rings exact), the box×cylinder family entirely (the
      genus-1 through hole, the axle, the blind bores, the boss, the
      flush far end — the annulus chord bound `(4/3)·τ·r·d·h`, with
      the quartering convergence test), the box×sphere corner and
      face and offset stations for all three ops (the octant patch,
      the hemisphere cap, the small-circle caps and their complements,
      the holed face — the patch bound `4πr²·d`, with the
      quartering convergence test), and through the real dispatch the
      imported-box cuts (fc-io: the crescent's period-safe complement,
      the corner bite, the offset bite, the through hole all carrying
      working display channels). Honest boundaries, all named: the
      edge-station's pole-crossing patch (the quarter ball whose walk
      jumps half a period at the pole — the authored pole-run tiling
      is the next slice, both the patch and its complement), a curved
      face keeping the exterior of its trim on a non-sphere surface,
      hole-trimmed CURVED domains, and B-splines — unchanged. Suite:
      725, none ignored. The remaining K4-2d/3 surface: the authored
      pole-run tiling (the edge station), the crossing-disk
      side-clipped sections, the coplanar-face merge (the flush
      landing's union), and the OCCT-authored-result reader widening
      (multi-segment NURBS arcs).
- [ ] **K5** fillets, chamfers, offsets

## Non-goals (explicit)

- OCCT compatibility beyond reading parameters from real files (the
  kernel decision and its revisit triggers:
  [`adr/0001-geometry-kernel.md`](adr/0001-geometry-kernel.md); the
  accepted pure-Rust exactness strategy — K1–K5, OCCT stays gated —
  [`adr/0003-geometry-kernel-strategy.md`](adr/0003-geometry-kernel-strategy.md))
- Python scripting as a FreeCAD-workbench equivalent — an *embedded*
  scripting milestone (rhai first, pyo3 optional) exists in
  [`PLAN-APPLICATION.md`](PLAN-APPLICATION.md), but full Python
  console/workbench parity remains a non-goal
- A FreeCAD replacement claim — this is a study of the architecture,
  and a toolkit
