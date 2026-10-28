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
- [ ] **K4-2c** the wavy-band seam corner (the oblique cut's
      single-cell window carrier), then NURBS measure (Gauss–
      Legendre on polynomial integrands), the mixed-strip fillet
      corner, and general booleans with subsurface classification
      and trimming on the loaded operands
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
