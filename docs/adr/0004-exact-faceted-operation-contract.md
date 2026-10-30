# ADR 0004: The exact/faceted operation contract — one representation, named refusals, no silent downgrades

- **Status:** Accepted (2026-10-30). Extends
  [`0003-geometry-kernel-strategy.md`](0003-geometry-kernel-strategy.md)
  from the kernel program into the application's operation layer; does
  not revisit the kernel decision.
- **Scope:** `fc-part`'s feature execution and shape APIs
  ([`crates/fc-part/src/features.rs`](../../crates/fc-part/src/features.rs),
  [`crates/fc-part/src/shape.rs`](../../crates/fc-part/src/shape.rs)),
  `fc-io`'s FCStd result persistence
  ([`crates/fc-io/src/fcstd.rs`](../../crates/fc-io/src/fcstd.rs)), and
  every consumer that classifies a feature's result (document
  statistics, viewer, wasm/studio labels, export).
- **Related:** K4-1-integration (the `TopoShape::Brep` loader bridge
  and its two-channel honesty contract) in
  [`../ROADMAP.md`](../ROADMAP.md); the K4-2d/3 general-boolean
  milestone that will widen the dispatch table this ADR freezes.

## Context

The kernel program of ADR-0003 delivered two geometry paths, and
K4-1-integration joined them at the *loading* boundary only:

- **The exact path:** `ExactBRep` → exact analytic intersections →
  supported exact BRep booleans (`brep_boolean.rs`) → exact measures
  (`measure.rs`) → exact-model tessellation (`tessellate.rs`).
- **The application path:** `TopoShape` primitives and meshes →
  feature operations → tessellated results and viewer geometry.

`TopoShape::Brep` now carries live exact geometry read from real OCCT
payloads, with the honesty contract: `volume`/`tessellate` answer
exactly or visibly (`NaN`, the empty mesh), while `try_volume`/
`try_tessellate` carry the refusing face's name.

But the *operation* layer still reduces everything to meshes.
`BooleanFeature::execute_impl` resolves its operands through
`placed_mesh_of` — a tessellation — applies the BSP mesh boolean, and
stores `Option<Mesh>`. An imported exact BRep operand is tessellated
before a `Part::Cut` ever executes; the acceptance test
`occt_shape.rs::mesh_result_feature_bridges_the_brep_at_default_deflection`
documents the outcome: a boolean "holding the bridged mesh, not the
exact model". That was a deliberate, documented limitation. This ADR
retires it.

Meanwhile the exact kernel already has boolean families that are
document-level operands today — sphere×sphere,
cylinder×cylinder, cylinder×sphere, all placement-general in their
analytic parameters — and no document feature can reach them.

## Decision

Five rules. Together they make one architecture: the application can
always say whether a result is exact, faceted, or refused — and why.

### 1. One result representation: `TopoShape`

The document's shape abstraction already *is* the needed union:
analytic primitives, `Mesh`, `Brep(ExactBRep)`. Derived features'
result channels become `Option<TopoShape>` (starting with
`BooleanFeature`): `TopoShape::Brep` marks an exact result,
`TopoShape::Mesh` a faceted one, a primitive variant an analytic one.
No parallel result fields — a second "exact result" slot beside a mesh
slot would drift, and no consumer could tell which geometry is live.

### 2. Operand resolution resolves shapes, not meshes

A new `placed_shape_of(obj) -> Result<TopoShape, String>` resolves an
operand's shape with its `Placement` applied analytically where the
shape is exact (a sphere's center, a cylinder's origin and axis). The
mesh path *derives* from the shape (`try_tessellate`), so a B-spline
tessellation refusal names the refusing face instead of collapsing
into a generic "no computed geometry".

### 3. Boolean dispatch is explicit and total

For `Part::Cut` / `Part::Fuse` / `Part::Common`:

- **A faceted operand is present** → the mesh boolean runs and the
  result is `Mesh`. The approximation is the input's nature, recorded
  in the representation where statistics, the viewer, and the studio
  can label it.
- **Both operands are exact-representable and the combination is
  supported** — sphere×sphere, cylinder×cylinder, cylinder×sphere
  (each side either a `Brep` payload classified to that analytic
  class, or a `Sphere`/`Cylinder` primitive; placements applied) →
  the exact BRep boolean runs and the result is `Brep`. An `Empty`
  outcome is an exact answer represented as an empty mesh result
  (emptiness has no geometry identity to lose); a `Refused` outcome is
  an execution error carrying the kernel's own reason.
- **Exact operands, unsupported combination** — a box-class BRep, a
  B-spline face, cone or torus classes, unsupported axis
  configurations → **a named refusal**: the error names the operands,
  their classes, and the reason. Never a silent tessellation
  downgrade. (The message says how to get the faceted workflow
  explicitly: tessellate first or use mesh operands.)
- The cone/plane, sphere/plane and cylinder/plane kernel APIs stay
  kernel-level: no document object is a halfspace, so they are not
  dispatch targets.

### 4. Persistence follows provenance

- **Imported `Brep` (preserved payload):** the file's own bytes remain
  the only truthful serialization — verbatim round-trip, unchanged
  (ADR-0003: this kernel parses OCCT and writes none of it).
- **Derived exact results** (a boolean's freshly computed `Brep`):
  **recomputed, not serialized.** The feature's links and parameters
  are the serialization; the save side skips the result payload and
  the reload leaves the feature touched, so the recompute re-derives
  the exact result from the same inputs — the FreeCAD
  touched-feature discipline. A derived `Brep` is *never* written as
  FCBREP bytes into an entry a real FreeCAD would read as OCCT BRep.
- **A loaded last-good payload on a boolean** (real FreeCAD files
  carry the previous computation): a readable payload becomes the
  feature's exact result — the mesh bridge for booleans is retired; an
  unreadable one leaves the feature uncomputed and touched, with the
  reader's own warning. The preserved bytes ride back out either way.
- **Faceted results** keep today's FCBREP v1 payload, unchanged.
- **Provenance-free `Brep` on a shape-*holding* object**
  (`Part::Feature`) still refuses the save loudly — unchanged; that
  refusal is about *data* with no parameter set to rebuild it, which a
  derived feature is not.

### 5. Honesty channels survive every hop

Execution refusals ride `execute`'s `Err` into the object's error
status, recompute reports, CLI, wasm responses, and studio
notifications (the existing plumbing); operand refusals arrive named
via rule 2. No consumer has to guess between "no object", "not yet
computed", "rejected payload", and "exact-kernel refusal".

## Slice boundaries (explicit)

This architecture lands in slices; each stays green on its own.

- **Slice 1 (landing now):** `BooleanFeature` only — the dispatch of
  rule 3, the `Option<TopoShape>` result of rule 1, the loader and
  save-side behavior of rule 4, the `placed_shape_of` of rule 2.
- **Later slices:** migrating the remaining mesh-result features
  (multi-fuse, mirroring, transform, PartDesign) to `Option<TopoShape>`
  results; widening the supported-combination table as K4-2d/3 lands
  more exact-boolean families; PartDesign exact dispatch.

## What we do NOT claim

- **General BRep booleans through features.** Three families
  (sphere×sphere, cylinder×cylinder, cylinder×sphere) are wired; the
  rest refuse by name. K4-2d/3 is the milestone that widens this.
- **That the mesh fallback for unsupported primitive combinations is
  exactness.** It is faceted, the representation says so, and the
  narrowing path is more exact families, not relabeling.
- **NURBS operands in booleans.** Refused by name (the measure channel
  reads them; the boolean table does not).
- **A serialization for authored exact geometry.** Derived results are
  recomputed, not written; authoring exact payloads is a separate,
  explicit decision if ever taken.

## Revisit triggers

- When K4-2d/3 lands additional exact-boolean families, the dispatch
  table grows and the faceted-fallback set shrinks — by amendment to
  this ADR, not silently.
- If a real downstream workflow requires saved derived-exact payloads
  (consumers that cannot recompute), revisit rule 4 with an explicit
  serialization ADR — an FCBREP exact form is the candidate, and it
  needs a round-trip proof before any compatibility claim.

## Rejected alternatives

- **A parallel exact field beside `result: Option<Mesh>`.** Two
  channels drift; consumers cannot tell which geometry is live. The
  union already exists — use it.
- **Always-mesh with a warning.** The silent downgrade this ADR exists
  to retire; warnings do not survive into document state, statistics,
  or the viewer.
- **Refusing unsupported combinations even for mesh-only operands.**
  Breaks every existing mesh workflow for zero honesty gain — the
  faceted representation already records the approximation.
- **Serializing derived exact results as OCCT-branded payloads.** We
  author no OCCT bytes (ADR-0003); the honest derived-result
  serialization is recomputation.
