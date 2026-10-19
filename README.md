# freecad-rs

**FreeCAD's core architecture, reimplemented in Rust.**

[![CI](https://github.com/payswapdotorg/freecad-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/payswapdotorg/freecad-rs/actions/workflows/ci.yml)

[FreeCAD](https://github.com/FreeCAD/FreeCAD) is a 34k-star, ~3 GB C++
parametric 3D modeler. This repo is a **clean-room reimplementation of
its architectural core** in safe Rust — not a GUI, not a full CAD
kernel, and honest about both:

> ⚠️ **Not affiliated with or endorsed by the FreeCAD project.** FreeCAD
> is LGPL-2.1 software by the FreeCAD community. This project
> reimplements its *architecture and file format* from scratch; no
> FreeCAD code is used or included. License: MIT.

## What is implemented

The heart of FreeCAD is the **parametric document**: typed objects with
named properties, linked into a dependency graph, recomputed
dependencies-first when inputs change. That core, plus a geometry layer
and the `.FCStd` file format:

| FreeCAD (C++)            | here (Rust)    | what it is                                              |
| ------------------------ | -------------- | ------------------------------------------------------- |
| `src/Base`               | `fc-base`      | `Vector3`, `Matrix4`, quaternion `Rotation`, `Placement`, `BoundBox3` |
| `src/App`                | `fc-app`       | `Document`, `DocumentObject`, typed `Property` system, dependency-ordered recompute with cycle detection |
| `src/Mod/Part`           | `fc-part`      | analytic primitives (box/sphere/cylinder/cone/torus), exact volume/area, tessellation, FCBREP serialization, `Part::Box`-style features |
| `.FCStd` persistence     | `fc-io`        | zip container + `Document.xml` reader/writer, FreeCAD-compatible XML schema |
| `FreeCADCmd`             | `freecad-rs`   | CLI: create/edit/recompute documents, render SVG, export dependency graphs |

**Not implemented (yet, if ever):** OCCT NURBS/BRep booleans, sketcher,
GUI, Python scripting, STEP import/export. Real FreeCAD files load with
their parameters intact; OCCT `.brp` shape payloads are detected and
skipped with a warning rather than misparsed — see `fc-io` crate docs.

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

Full walkthrough: [`docs/cli.md`](docs/cli.md) · rendered example:
[`examples/demo.svg`](examples/demo.svg) · runnable script:
[`scripts/demo.sh`](scripts/demo.sh) · roadmap:
[`docs/ROADMAP.md`](docs/ROADMAP.md).

## Workspace layout

```
crates/fc-base      math vocabulary (vectors, quaternions, matrices, placements)
crates/fc-app       document model, properties, dependency-driven recompute
crates/fc-part      geometry kernel: primitives, tessellation, FCBREP
crates/fc-io        .FCStd container + Document.xml
crates/freecad-rs   the CLI binary
```

## License

MIT — see [LICENSE](LICENSE). FreeCAD is a trademark of its community;
this project is independent.
