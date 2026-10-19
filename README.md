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
freecad-rs add box demo.fcstd --name Base --length 30 --width 20 --height 5
freecad-rs add sphere demo.fcstd --name Ball --radius 4
freecad-rs add transform demo.fcstd --name Lift --source Ball --translate 0,0,10
freecad-rs recompute demo.fcstd
freecad-rs stats demo.fcstd
freecad-rs export svg demo.fcstd demo.svg --view iso
freecad-rs graph demo.fcstd graph.dot
```

Parametric behavior in one command:

```sh
freecad-rs set demo.fcstd Ball Radius=6   # touches Ball AND Lift
freecad-rs recompute demo.fcstd           # recomputes both, dependencies first
```

Full walkthrough: [`docs/cli.md`](docs/cli.md) · rendered example:
[`examples/demo.svg`](examples/demo.svg) · roadmap:
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
