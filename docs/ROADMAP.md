# Roadmap

Honest, incremental, no vaporware.

## Done

- [x] Workspace scaffold + CI (build, clippy, fmt)
- [x] `fc-base`: vectors, quaternions, matrices, placements, bounding boxes
- [x] `fc-app`: properties, documents, dependency-ordered recompute
- [x] `fc-part`: analytic primitives, tessellation, FCBREP
- [x] `fc-io`: `.FCStd` read/write (FreeCAD-compatible `Document.xml`)
- [x] `freecad-rs` CLI: new/add/set/get/list/recompute/info/stats/export/graph
- [x] Test suite: 128 tests across all five crates (unit + integration)
- [x] `Part::Cut / Fuse / Common` — BSP mesh booleans with a
      T-junction stitch pass; watertight for watertight box/cylinder
      operands; hybrid splitter (tangent-plane scoring + bounding-box
      midplanes) keeps the tree balanced and 2 MiB stacks safe
- [x] Expression engine — FreeCAD's `App::PropertyExpressionEngine`,
      distilled: parser, degree-mode trig, unit folding, document
      bindings with dependency-graph edges, cycle detection,
      `.FCStd` persistence (`<Expression path=… expression=…/>`)
- [x] OBJ/STL mesh export (ASCII + binary STL; FreeCAD's
      `Mesh` export architecture)

## Next (candidates, in rough order)

- [ ] STEP reading *via* external crates, clearly marked non-core
- [ ] Undo/redo transactions (FreeCAD `App::Transaction`)
- [ ] A minimal 3D viewer (wgpu) — the first GUI surface
- [ ] Boolean robustness beyond boxes/cylinders: sphere-sphere seams
      (T-junctions on curved seams) still produce open meshes

## Non-goals (explicit)

- OCCT compatibility beyond reading parameters from real files
- Python scripting
- A FreeCAD replacement claim — this is a study of the architecture,
  and a toolkit
