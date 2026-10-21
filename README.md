# freecad-rs

**FreeCAD's core architecture, reimplemented in Rust.**

[![CI](https://github.com/payswapdotorg/freecad-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/payswapdotorg/freecad-rs/actions/workflows/ci.yml)

[FreeCAD](https://github.com/FreeCAD/FreeCAD) is a 34k-star, ~3 GB C++
parametric 3D modeler. This repo is a **clean-room reimplementation of
its architectural core** in safe Rust — a headless CAD core + CLI, not
a GUI, not a full CAD kernel, and honest about both:

> ⚠️ **Not affiliated with or endorsed by the FreeCAD project.** FreeCAD
> is LGPL-2.1 software by the FreeCAD community. This project
> reimplements its *architecture and file format* from scratch; no
> FreeCAD code is used or included. License: MIT.

**Verified baseline** — `main` @ `08270c6` (2026-10-21): six crates,
**361 tests, 0 failed** (`cargo build` / `cargo test --workspace
--locked`), `clippy --workspace --all-targets -- -D warnings` clean,
`cargo fmt --all -- --check` clean. PartDesign v1 — `Pad`, `Pocket`,
`Revolution` as sketch-consuming document objects with CLI verbs and
`.FCStd` persistence — is implemented.

## What is implemented

The heart of FreeCAD is the **parametric document**: typed objects with
named properties, linked into a dependency graph, recomputed
dependencies-first when inputs change. That core, plus a geometry layer,
the sketcher, and the `.FCStd` file format:

| FreeCAD (C++)            | here (Rust)    | what it is                                              |
| ------------------------ | -------------- | ------------------------------------------------------- |
| `src/Base`               | `fc-base`      | `Vector3`, `Matrix4`, quaternion `Rotation`, `Placement`, `BoundBox3` |
| `src/Base/Quantity.cpp`   | `fc-base`      | units & quantity system: 8-dimension `Unit`, `Quantity` arithmetic with dimension checking, parser (`"9.8 m/s^2"`, `"30°"`), conversion table incl. affine `C`/`F` |
| `src/App`                | `fc-app`       | `Document`, `DocumentObject`, typed `Property` system, dependency-ordered recompute with cycle detection |
| `src/App/Transactions.cpp`| `fc-app`      | undo/redo: transaction stack with add/del/change records, property snapshots, auto-record on mutation, `undo`/`redo`/`history` CLI |
| `src/App/Expression.cpp` | `fc-app`       | expression engine **evaluating to `Quantity`**: dimensional analysis, unit-aware arithmetic (dimension-checked `+`/`-`, unit-combining `*`/`/`, integer-exponent `^`), degree-mode trig, `<<Label>>.Property` references, document bindings with dependency-graph edges + cycle detection, honest binding rejection (a plain-float property rejects a unit-carrying expression, FreeCAD `PropertyFloat` behavior) |
| `src/Mod/Part`           | `fc-part`      | analytic primitives (box/sphere/cylinder/cone/torus), exact volume/area, tessellation, FCBREP serialization, `Part::Box`-style features, `Part::Cut/Fuse/Common` mesh booleans, **profile kernels** (closed-wire extraction from sketch geometry, extrusion, revolution — watertight-by-construction meshes), **PartDesign v1 features** (`PartDesign::Pad/Pocket/Revolution` consuming a sketch, recompute-driven) |
| `src/Mod/PartDesign` (v1) | `fc-part`      | `Pad` (Profile/Length/Midplane/Reversed), `Pocket` (Base/Length/Inverted — boolean cut), `Revolution` (Angle/Axis) — sketch links in the dependency graph, error propagation from failed profiles, parametric recompute proven end-to-end |
| `src/Mod/Sketcher` (v1)  | `fc-sketcher`   | 2D geometry (point/line/circle/arc), 13 constraint kinds, Levenberg-Marquardt constraint solver with DOF report, FreeCAD-style `<Sketch>` XML fragments; `Sketcher::SketchObject` — a first-class document object (execute = solve) with `.FCStd` persistence |
| `.FCStd` persistence     | `fc-io`        | zip container + `Document.xml` reader/writer, FreeCAD-compatible XML schema (incl. expression bindings and `<Sketch>` fragments), OBJ/STL mesh export |
| `FreeCADCmd`             | `freecad-rs`   | CLI: create/edit/recompute documents, undo/redo history, boolean features, quantity-typed expressions, quantity arithmetic (`calc`), sketch verbs (`add sketch`, `sketch-import`, `sketch-export`, `sketch-info`, `sketch-solve`, `sketch-show`), **PartDesign verbs (`pad`, `pocket`, `revolution`)**, OBJ/STL/SVG export, dependency graphs |

**Not implemented (yet, if ever):** exact B-rep/NURBS geometry (OCCT),
STEP import/export, GUI, Python scripting. Real FreeCAD files load with
their parameters intact; OCCT `.brp` shape payloads are detected and
skipped with a warning rather than misparsed — see `fc-io` crate docs.

## Honest limitations

This is a **headless CAD core + CLI**, not a FreeCAD replacement. In
concrete terms:

* **Tessellated mesh kernel, not exact geometry.** Beyond the analytic
  primitives (whose volume/area are exact), curved geometry is a
  deflection-bounded approximation: sagitta rules govern arc and
  revolution tessellation (chord error ≤ the requested deflection),
  and profile solids are watertight-by-construction meshes — their
  volumes are mesh-true, not exact. Exact B-rep/NURBS via OCCT
  bindings is a *gated future decision*, not a v1 commitment — see
  [`docs/adr/0001-geometry-kernel.md`](docs/adr/0001-geometry-kernel.md).
* **Mesh Booleans are approximate.** `Part::Cut/Fuse/Common` are
  watertight for planar/cylindrical seams; curved-surface seams
  (sphere-sphere) can still leave open meshes at T-junctions.
* **Sketcher v1 solves in the XY plane.** The sketch `Placement` is
  persisted but not yet applied — sketches on arbitrary planes and
  attachments are future work, and v1's PartDesign features inherit
  the same XY-frame limitation.
* **No GUI, no scripting yet.** The interactive viewer, deeper FreeCAD
  native-file interop, STEP, and embedded scripting are sequenced as
  product workstreams with explicit acceptance gates in
  [`docs/PLAN-APPLICATION.md`](docs/PLAN-APPLICATION.md).
* **Parity is documented, not claimed.** Implemented v1 behavior is
  documented per crate; where the shape of a thing differs from
  FreeCAD (e.g. the `<Sketch>` persistence fragment, FCBREP instead
  of OCCT `.brp`), the crate docs say so.

## Install & build

Requires Rust 1.75+ (stable).

```sh
cargo build --workspace        # debug
cargo install --path crates/freecad-rs   # install the CLI
```

## Quick tour

```sh
freecad-rs new demo.fcstd --label "Playground"
freecad-rs add demo.fcstd --name Base box --length 30 --width 20 --height 5
freecad-rs add demo.fcstd --name Ball sphere --radius 4
freecad-rs add demo.fcstd --name Stem cylinder --radius 1.5 --height 12
freecad-rs add demo.fcstd --name Lift transform --source Ball --translate 0,0,12 --rotate-deg 25
freecad-rs stats demo.fcstd
freecad-rs export svg demo.fcstd demo.svg --view iso
freecad-rs graph demo.fcstd graph.dot
```

`stats` output (exact analytic values — the parametric core at work):

```text
  Base (Box): volume 3000.000 mm³, area 1700.000 mm², 12 triangles
  Ball (Sphere): volume 904.779 mm³, area 452.389 mm², 9024 triangles
  Stem (Cylinder): volume 84.823 mm³, area 127.235 mm², 96 triangles
  Lift (Mesh): volume 903.165 mm³, area 451.986 mm², 9024 triangles
total: 4 objects, volume 4892.767 mm³
```

Parametric behavior in one command — changing the sphere touches its
dependent transform, and recompute runs dependencies-first:

```sh
freecad-rs set demo.fcstd Ball Radius 6
#   touched Ball
#   touched Lift
#   recomputed Ball (Part::Sphere)
#   recomputed Lift (Part::Transform)
```

Expressions drive properties parametrically, FreeCAD-style — the
binding rides the dependency graph, so changing the source re-runs
everything downstream:

```sh
freecad-rs set-expr demo.fcstd Stem Height "2 * Base.Length / 3"
#   Stem.Height = 20
freecad-rs set demo.fcstd Base Length 45
#   touched Base, Stem — Stem.Height is now 30
```

Boolean features compose meshes (`Part::Cut/Fuse/Common`) and export
to OBJ/STL for the wider ecosystem:

```sh
freecad-rs add demo.fcstd --name Notch cut --base Base --tool Stem
freecad-rs export stl demo.fcstd demo.stl --binary
freecad-rs export obj demo.fcstd demo.obj
```

Sketches are first-class document objects: create one, import a
`<Sketch>` XML fragment into it, and recompute *is* the solve — a
failed solve marks the sketch `Error` and fails its dependents,
FreeCAD-style:

```sh
freecad-rs add demo.fcstd --name Prof sketch
freecad-rs sketch-import demo.fcstd Prof examples/sketch-quad.xml
#   Solved, 8 constraints, dof 7, residual 7.9e-14
freecad-rs sketch-info demo.fcstd Prof
```

Full walkthrough: [`docs/cli.md`](docs/cli.md) · rendered example:
[`examples/demo.svg`](examples/demo.svg) · runnable script:
[`scripts/demo.sh`](scripts/demo.sh) · roadmap:
[`docs/ROADMAP.md`](docs/ROADMAP.md) · kernel decision:
[`docs/adr/0001-geometry-kernel.md`](docs/adr/0001-geometry-kernel.md) ·
application-layer plan:
[`docs/PLAN-APPLICATION.md`](docs/PLAN-APPLICATION.md).

## Workspace layout

```
crates/fc-base      math vocabulary (vectors, quaternions, matrices, placements, quantities)
crates/fc-app       document model, properties, dependency-driven recompute, quantity-typed expressions, undo/redo
crates/fc-part      geometry kernel: primitives, tessellation, FCBREP, mesh booleans, profile kernels
crates/fc-sketcher  2D sketcher: geometry, constraints, solver, SketchObject
crates/fc-io        .FCStd container + Document.xml + mesh export
crates/freecad-rs   the CLI binary
```

## License

MIT — see [LICENSE](LICENSE). FreeCAD is a trademark of its community;
this project is independent.
