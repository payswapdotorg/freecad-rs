# Roadmap

Honest, incremental, no vaporware.

> **Verified baseline** — v0.1.7 (counted 2026-10-24; the counts
> include the v0.1.8 exact seam weld in flight at count time): nine
> crates, **486 tests** (per-crate counts and method in
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

## Next: the kernel exactness program (K1–K5)

Accepted in
[`adr/0003-geometry-kernel-strategy.md`](adr/0003-geometry-kernel-strategy.md):
pure-Rust exactness as its own program, with its own milestones —
not an OCCT wrapper. Each lands with committed tests; K2 is already
behind us.

- [ ] **K1** exact topology model — vertices/edges/faces carrying
      parametric curves & surfaces (line, circle, ellipse, plane,
      cylinder, cone, sphere, torus, B-spline); structural BRep
      payload round-trip
- [x] **K2** robust predicates — **done in v0.1.7**: exact
      classification (`crates/fc-part/src/exact.rs`; the 1e-6
      blind-spot golden test)
- [ ] **K3** exact intersections & booleans on analytic surfaces —
      differential-tested against a committed OCCT-generated fixture
      corpus with expected results
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
