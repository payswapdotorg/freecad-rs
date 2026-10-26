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
- [ ] **K3 (remaining)** the wider analytic-pair booleans
      (cylinder×cylinder, the coaxial families) and the OCCT
      differential corpus for the boolean results (volumes vs
      FreeCAD's `cut`/`common`/`fuse` — the measure module is the
      comparison side)
- [ ] **K4** NURBS support and general booleans — subsurface
      classification, trimming
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
