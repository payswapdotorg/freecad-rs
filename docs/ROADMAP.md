# Roadmap

Honest, incremental, no vaporware.

## Done

- [x] Workspace scaffold + CI (build, clippy, fmt)
- [x] `fc-base`: vectors, quaternions, matrices, placements, bounding boxes
- [x] `fc-app`: properties, documents, dependency-ordered recompute
- [x] `fc-part`: analytic primitives, tessellation, FCBREP
- [x] `fc-io`: `.FCStd` read/write (FreeCAD-compatible `Document.xml`)
- [x] `freecad-rs` CLI: new/add/set/get/list/recompute/info/stats/export/graph

## Next (candidates, in rough order)

- [ ] `Part::Cut / Fuse / Common` — mesh booleans first, analytic where possible
- [ ] Expression engine (`Height = Length / 2`) — FreeCAD's
      `App::PropertyExpressionEngine`, distilled
- [ ] OBJ/STL mesh export (`freecad-rs export obj|stl`)
- [ ] STEP reading *via* external crates, clearly marked non-core
- [ ] Undo/redo transactions (FreeCAD `App::Transaction`)
- [ ] A minimal 3D viewer (wgpu) — the first GUI surface

## Non-goals (explicit)

- OCCT compatibility beyond reading parameters from real files
- Python scripting
- A FreeCAD replacement claim — this is a study of the architecture,
  and a toolkit
