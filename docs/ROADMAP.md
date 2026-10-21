# Roadmap

Honest, incremental, no vaporware.

> **Verified baseline** — `main` @ `65189af`, verified locally on
> 2026-10-21: `cargo build` / `cargo test --workspace --locked` green
> (**399 tests, 0 failed**, seven crates), `clippy --workspace
> --all-targets -- -D warnings` clean, `cargo fmt --all -- --check`
> clean.

## Done

- [x] Workspace scaffold + CI (build, test, clippy, fmt — the test step
      + `work/**` push triggers were added in FC-5)
- [x] `fc-base`: vectors, quaternions, matrices, placements, bounding boxes
- [x] `fc-app`: properties, documents, dependency-ordered recompute
- [x] `fc-part`: analytic primitives, tessellation, FCBREP
- [x] `fc-io`: `.FCStd` read/write (FreeCAD-compatible `Document.xml`)
- [x] `freecad-rs` CLI: new/add/set/get/list/recompute/info/stats/export/graph
- [x] Test suite: 361 tests across all six crates (unit + integration;
      re-verified at `08270c6`)
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
      arc geometry, 13 constraint kinds, Levenberg-Marquardt solver
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

## Next (candidates, in rough order)

- [ ] Application-layer milestones — interactive 3D viewer (wgpu),
      STEP — sequenced with acceptance gates in
      [`PLAN-APPLICATION.md`](PLAN-APPLICATION.md); STEP proceeds under
      [`adr/0002-step-milestone.md`](adr/0002-step-milestone.md)
      (trigger 1 of ADR-0001 fired; pure-Rust `ruststep` path)

## Non-goals (explicit)

- OCCT compatibility beyond reading parameters from real files (the
  kernel decision and its revisit triggers:
  [`adr/0001-geometry-kernel.md`](adr/0001-geometry-kernel.md))
- Python scripting as a FreeCAD-workbench equivalent — an *embedded*
  scripting milestone (rhai first, pyo3 optional) exists in
  [`PLAN-APPLICATION.md`](PLAN-APPLICATION.md), but full Python
  console/workbench parity remains a non-goal
- A FreeCAD replacement claim — this is a study of the architecture,
  and a toolkit
