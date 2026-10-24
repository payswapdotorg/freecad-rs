# freecad-rs

**FreeCAD's core architecture, reimplemented in Rust.**

[![CI](https://github.com/payswapdotorg/freecad-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/payswapdotorg/freecad-rs/actions/workflows/ci.yml)

[FreeCAD](https://github.com/FreeCAD/FreeCAD) is a 34k-star, ~3 GB C++
parametric 3D modeler. This repo is a **clean-room reimplementation of
its architectural core** in safe Rust — a headless CAD core + CLI, not
a GUI, not a full CAD kernel, and honest about both:

**Same as FreeCAD?** The parity matrix — verified against the real
FreeCAD 1.0 binaries in both directions (files, geometry semantics,
placements, sketches) — lives in
[`docs/EQUIVALENCE.md`](docs/EQUIVALENCE.md).

> ⚠️ **Not affiliated with or endorsed by the FreeCAD project.** FreeCAD
> is LGPL-2.1 software by the FreeCAD community. This project
> reimplements its *architecture and file format* from scratch; no
> FreeCAD code is used or included. License: MIT.

**Verified baseline** — v0.1.8 (`main` @ 2026-10-24, plus the
v0.1.8 exact seam weld landing with it): nine crates, **486 tests**
(counted 2026-10-24; per-crate counts live in
[`docs/VERIFICATION.md`](docs/VERIFICATION.md) — the authoritative
green run is CI: `cargo build` / `cargo test --workspace --locked` /
`clippy --workspace --all-targets -- -D warnings` /
`cargo fmt --all -- --check`). PartDesign v1, **exact boolean
classification** (v0.1.7 — integer predicates, no tolerance;
certified `f64` filter → i256 → wide integer) with the exact planar
seam weld (v0.1.8, landing), curved-seam boolean robustness on mixed
curved operands (crossed/parallel/skew cylinders, tilted cone and
cylinder vs sphere, mirrored near-coincident seams at fine
deflections, exact-tangency sliver filtering), native-file interop
breadth, rhai embedded scripting, the M1 offscreen viewer, and STEP
exchange are implemented (details below).

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
| `src/Mod/Part`           | `fc-part`      | analytic primitives (box/sphere/cylinder/cone/torus), exact volume/area, tessellation, FCBREP serialization, `Part::Box`-style features, `Part::Cut/Fuse/Common` mesh booleans with **exact classification** (v0.1.7: certified integer predicates, no tolerance — `exact.rs`) and **curved-seam robustness** (consensus-grid snap at the seam's own sampling scale — never moving the operands' exact interior surfaces — tolerance-based trace interleaving, fin welding, float-noise vertex welds, folded-sliver zipping, and orientation consistency to the shared-edge contract OCCT enforces; watertight, consistently oriented results on sphere pairs and mixed curved operands — crossed/parallel/skew cylinders, tilted cone and cylinder vs sphere — under a wholesale-or-nothing volume guard), **profile kernels** (closed-wire extraction from sketch geometry, extrusion, revolution — watertight-by-construction meshes), **PartDesign v1 features** (`PartDesign::Pad/Pocket/Revolution` consuming a sketch, recompute-driven) |
| `src/Mod/PartDesign` (v1) | `fc-part`      | `Pad` (Profile/Length/Midplane/Reversed), `Pocket` (Base/Length/Inverted — boolean cut), `Revolution` (Angle/Axis) — sketch links in the dependency graph, error propagation from failed profiles, parametric recompute proven end-to-end |
| `src/Mod/Sketcher` (v1)  | `fc-sketcher`   | 2D geometry (point/line/circle/arc), 16 constraint kinds (incl. Tangent/Symmetric/Block), Levenberg-Marquardt constraint solver with DOF report, FreeCAD-style `<Sketch>` XML fragments; `Sketcher::SketchObject` — a first-class document object (execute = solve) with `.FCStd` persistence |
| `.FCStd` persistence     | `fc-io`        | zip container + `Document.xml` reader/writer, FreeCAD-compatible XML schema (incl. expression bindings and `<Sketch>` fragments), OBJ/STL mesh export, **native-file interop breadth** (`App::Part`/`PartDesign::Body`/origin planes & axes/groups/links load with their parameters; quantity/vector/color/list/matrix property family; a committed fixtures corpus pins round-trip behavior) |
| `Python console` (distilled) | `fc-script` | **embedded scripting via rhai** — sandboxed document API mirroring the CLI verbs (add/set/bind/recompute/stats/save/export), expression bindings through the `fc-app` engine, honest failure semantics (a failed recompute surfaces the error and exits non-zero), operation budget. *Embedded Rhai, not FreeCAD's Python API, macro system or workbench ecosystem* — that surface remains a non-goal |
| 3D view (M1, distilled)  | `fc-viewer`    | **offscreen wgpu 30 renderer** — the exports' tessellation rules + placements, flat shading with the SVG renderer's headlight, svg-parity cameras (`iso/top/front/right`, asserted equal to `export svg`'s matrices), orthographic CPU picking (nearest-hit), error objects rendered visibly flagged, PNG output with `--bless` goldens; `interactive` cargo feature adds a winit orbit/zoom/pan window (build-tested). *Viewing only — a renderer and orbit/pan/zoom, not a modeling workbench* |
| STEP (AP203/AP214)       | `fc-step`      | **exchange with the CAD ecosystem** — faceted-B-rep export (deterministic, the faceted marker in every product name), deflection-parameterized import via `ruststep` 0.4 (planar + cylindrical faces, LINE/CIRCLE edges, POLY_LOOP facets; honest errors on anything else) |
| `FreeCADCmd`             | `freecad-rs`   | CLI: create/edit/recompute documents, undo/redo history, boolean features, quantity-typed expressions, quantity arithmetic (`calc`), sketch verbs (`add sketch`, `sketch-import`, `sketch-export`, `sketch-info`, `sketch-solve`, `sketch-show`), **PartDesign verbs (`pad`, `pocket`, `revolution`)**, OBJ/STL/SVG export, **`render` (M1 viewer)**, **`import-step` / `export step` (M3)**, **`script` (rhai embedded scripting)**, dependency graphs |

**Not implemented (yet, if ever):** exact B-rep/NURBS geometry (OCCT),
a GUI workbench (M1 is a viewer — orbit/pan/zoom + picking, not
editing), Python scripting (the *embedded* scripting milestone is rhai
— `fc-script`; a full Python surface remains a non-goal). STEP is
**faceted** both ways (ADR-0002): exports carry the faceted marker in
the file, import tessellates under an explicit deflection. Real FreeCAD
files load with their parameters intact; OCCT `.brp` shape payloads are
detected and skipped with a warning rather than misparsed — see `fc-io`
crate docs.

## Honest limitations

This is a **headless CAD core + CLI**, not a FreeCAD replacement. In
concrete terms:

* **Tessellated mesh kernel, not exact geometry.** Beyond the analytic
  primitives (whose volume/area are exact), curved geometry is a
  deflection-bounded approximation: sagitta rules govern arc and
  revolution tessellation (chord error ≤ the requested deflection),
  and profile solids are watertight-by-construction meshes — their
  volumes are mesh-true, not exact. Exact B-rep/NURBS is pursued as
  the pure-Rust milestone program of
  [`docs/adr/0003-geometry-kernel-strategy.md`](docs/adr/0003-geometry-kernel-strategy.md)
  (OCCT bindings stay a gated decision —
  [`docs/adr/0001-geometry-kernel.md`](docs/adr/0001-geometry-kernel.md)).
* **Booleans classify exactly; curved operands stay deflection-bounded.**
  Every classification decision in `Part::Cut/Fuse/Common` is an exact
  integer computation (v0.1.7: certified `f64` filter → i256 → wide
  integer — a face offset a millionth of a unit is cut exactly where it
  belongs), and planar seams — rotated crossings included — weld exactly
  (v0.1.8, landing). Curved-surface seams are watertight within the
  deflection-derived tolerance (sphere-sphere at any overlap down to
  razors a fiftieth of the radius, sphere-cylinder near-tangent pairs,
  crossed/parallel/skew cylinders, tilted cylinder or cone against a
  sphere, crossed and coaxial opposed cones, mirrored near-coincident
  sheets at fine deflections, exact-tangency unions degenerate-free and
  volume-exact). The approximation that remains is the *tessellation*
  itself, not the classification — the honest list in
  [`docs/EQUIVALENCE.md`](docs/EQUIVALENCE.md) is the precise record.
* **Sketcher v1 solves in the XY plane.** The sketch `Placement` is
  persisted but not yet applied — sketches on arbitrary planes and
  attachments are future work, and v1's PartDesign features inherit
  the same XY-frame limitation.
* **No modeling workbench; embedded scripting only.** The GUI surface
  is the M1 viewer — an offscreen renderer plus an optional
  interactive orbit/pan/zoom window (`fc-viewer`'s `interactive`
  feature, build-tested) with picking; there is no editing UI, no
  tree view, no dockable anything. Scripting is **embedded Rhai**
  (`fc-script`: a sandboxed document API behind the `script` CLI
  verb) — *not* FreeCAD's Python API, macro system or workbench
  ecosystem; a full Python surface remains a non-goal. The plan these
  landed against is
  [`docs/PLAN-APPLICATION.md`](docs/PLAN-APPLICATION.md); the record
  of what has shipped is [`docs/ROADMAP.md`](docs/ROADMAP.md).
* **Parity is documented, not claimed.** Implemented v1 behavior is
  documented per crate; where the shape of a thing differs from
  FreeCAD (e.g. the `<Sketch>` persistence fragment, FCBREP instead
  of OCCT `.brp`), the crate docs say so.

## Verification

Every capability claim maps to evidence: a source-level test in this
repo, a CI job that runs it, or a documented manual verification
against the real FreeCAD 1.0 binary. The full capability → evidence
matrix — exact test names, where each suite runs, and which claims are
manual-only — lives in [`docs/VERIFICATION.md`](docs/VERIFICATION.md);
the bidirectional parity results against real FreeCAD 1.0 live in
[`docs/EQUIVALENCE.md`](docs/EQUIVALENCE.md). CI runs the workspace
gates (build, `cargo test --workspace --locked`, `clippy -D warnings`,
`fmt --check`) plus the GPU viewer job, and the `parity` workflow
([.github/workflows/parity.yml](.github/workflows/parity.yml), landing
with v0.1.8) is the continuous acceptance gate against real FreeCAD
1.0 — the manual verification loop `docs/EQUIVALENCE.md` records,
automated.

## Install & build

### Download a binary (v0.1.8)

Prebuilt executables live on the
[releases page](https://github.com/payswapdotorg/freecad-rs/releases/tag/v0.1.8),
with `SHA256SUMS` for every asset — pick what matches your system:

| Asset | Platform | Install |
|---|---|---|
| `freecad-rs-0.1.8-x86_64.AppImage` | Linux x86_64 | `chmod +x` and run; zero install. On FUSE-less systems use `APPIMAGE_EXTRACT_AND_RUN=1` |
| `freecad-rs-0.1.8-amd64.deb` | Debian/Ubuntu (amd64) | `sudo dpkg -i freecad-rs-0.1.8-amd64.deb` → `/usr/bin/freecad-rs` |
| `freecad-rs-0.1.8-x86_64-unknown-linux-gnu.tar.gz` | any Linux x86_64 | unpack, run `freecad-rs` from the bin dir |

```sh
# Linux AppImage, the FreeCAD-style way:
chmod +x freecad-rs-0.1.8-x86_64.AppImage
./freecad-rs-0.1.8-x86_64.AppImage --version
# freecad-rs 0.1.8
```

Rebuild the packages locally with `scripts/package.sh` (needs `dpkg-deb`;
AppImage via [appimagetool](https://github.com/AppImage/appimagetool)).
Releases are built and checksummed automatically by CI on every `v*` tag
(`.github/workflows/release.yml`).

### Build from source

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
freecad-rs render demo.fcstd demo.png                  # M1: wgpu offscreen
freecad-rs export step demo.fcstd demo.step            # M3: faceted AP203
freecad-rs import-step other.fcstd demo.step --deflection 0.1
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
kernel strategy (pure-Rust exactness program):
[`docs/adr/0003-geometry-kernel-strategy.md`](docs/adr/0003-geometry-kernel-strategy.md) ·
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
