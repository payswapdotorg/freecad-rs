# ADR 0004: The exact/faceted operation contract — one representation, named refusals, no silent downgrades

- **Status:** Accepted (2026-10-30); **amended 2026-10-30 by the
  K4-2d/3 slice 1 landing** (the dispatch table widened with the
  box×sphere family — the revisit trigger this ADR itself defined,
  exercised for the first time) **and again 2026-10-30 by the
  K4-2d/3 slice 3 landing** (box×box joins the pairing table — the
  rectilinear composition, the second amendment) **and a third time
  2026-10-30 by the K4-2d/3 slice 4 landing** (box×cylinder joins —
  the cylindrical bore taxonomy, and the pairing table CLOSES over
  the three analytic classes: the catch-all refusal arm retires, the
  compiler's match exhaustiveness the proof that every pairing lands
  somewhere) **and a fourth time 2026-10-30 by the K4-2d/3 slice 5
  landing** (the render channel's trim-domain classification — the
  wire-orientation convention this ADR's own `TopoFace` contract
  spells is now READ by the tessellator: the constructive results of
  all three boolean families tessellate watertight under the sagitta
  discipline, the annuli and rings through the planar hole-wire arm,
  the caps, patches and complements through the winding classifier —
  the render channel's honest refusals shrinking to the named
  boundaries). Extends
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

For `Part::Cut` / `Part::Fuse` / `Part::Common`, in priority order
(a `Brep` operand signals exact intent and dominates):

- **A `Brep` operand is present** → exact execution is required:
  - Both operands classify to the supported analytic classes (a
    `Brep` payload recognized as a sphere, cylinder, or hexahedral
    box solid, or a `Sphere`/`Cylinder`/`Box` primitive; placements
    applied) → the exact BRep boolean runs and the result is `Brep`.
    An `Empty` outcome is an exact answer represented as an empty
    mesh result (emptiness has no geometry identity to lose); a
    `Refused` outcome is an execution error carrying the kernel's own
    reason. The pairing table (amended by K4-2d/3): sphere×sphere,
    cylinder×cylinder, cylinder×sphere, **box×sphere** — the
    box×sphere family covering the boundary-skeleton stations
    (corner/edge/face bites), the offset face stations (slice 2: an
    interior center whose ball crosses exactly one face — the
    small-circle cap), and the classification tier (whole shapes, the
    empty set, the named refusals the kernel itself carries) — and
    **box×box** (slice 3: the rectilinear composition of two
    frame-parallel boxes — every result face a plane trimmed by an
    axis-aligned rectilinear wire; Common always the overlap box; the
    cut reading the overlap only, the flush fuse refusing the
    coplanar merge by name, the split refusing by shell count; the
    hole-free results rendering watertight with the mesh volume
    exactly the exact volume) — and **box×cylinder** (slice 4: the
    cylindrical bore taxonomy of the interior-cross-section stations
    — the through hole and the blind bores, the axle and boss
    fuses, the clipped-cylinder common; the frame contract demanding
    the bit-exact ±1 axis dot; the crossing disk the trim-domain
    refusal, the tangent disk the pinch refusal, the flush landing
    the coplanar-merge refusal). **The table is closed over the
    three analytic classes** — there is no unpaired combination of
    sphere/cylinder/box operands left; a fourth class joining
    `ExactSolid` grows the table by amendment, not by compiling past
    it.
  - The other operand is faceted, or either operand is an exact class
    the kernel cannot pair (a multi-surface B-rep, a B-spline face,
    cone or torus classes — the three analytic classes pair
    exhaustively since slice 4, so an unpaired OPERAND is a class
    outside the table, not a combination inside it) → **a named
    refusal**: the error names
    the operands, their classes, and the reason, and says how to get
    the faceted workflow explicitly (tessellate first or use mesh
    operands). Never a silent tessellation downgrade of the imported
    geometry.
- **No `Brep` operand is present** → today's behavior: a faceted
  operand runs the mesh boolean (result `Mesh` — the approximation
  is the input's nature, recorded in the representation where
  statistics, the viewer, and the studio can label it); unsupported
  primitive combinations likewise stay on the mesh path until they
  join a later slice.
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

- **Slice 1 (landed 2026-10-30):** `BooleanFeature` only — the `Brep`-operand
  dispatch of rule 3 (exact when imported exact geometry participates;
  primitive-pair combinations keep today's faceted path until the
  refused-configuration handling — a disjoint fuse, say — and the
  recompute-on-load of derived exact results are field-proven), the
  `Option<TopoShape>` result of rule 1, the loader and save-side
  behavior of rule 4, the `placed_shape_of` of rule 2.
- **Slice 2 (landed 2026-10-30, K4-2d/3):** the dispatch table
  widens with the box×sphere family — the hexahedral box class (an
  imported box-class B-rep or a `Box` primitive, placements applied
  analytically) pairs with the sphere through the kernel's
  boundary-skeleton bite taxonomy and (the same day's second slice)
  the offset face stations; the tessellator refuses the new patch
  classes by name (the spherical polygon, the holed face, and — a
  defect the offset stations flushed out — the sphere cap face whose
  single-closed-circle trim sent the band machinery into a silently
  open shell; trim-domain/wire-orientation classification is the
  named widening path, and tiling these through the band machinery
  would tear or vanish). The box-class refusal of slice 1 is retired:
  what refused as "a planar-faced (box-class) solid" now runs exact
  against a sphere and refuses by name against everything else (the
  pairing-table message).
- **Slice 3 (landed 2026-10-30, K4-2d/3):** the dispatch table
  widens again — box×box, the rectilinear composition. The frame
  contract is part of the pairing: EXACTLY parallel frames (the axis
  dots bit-exact ±1 — permuted, sign-flipped, mirrored all run
  exact), rotated or near-parallel frames refusing by name (never a
  silent snap). The render channel gains its first constructive
  WORKING path: the hole-free rectilinear faces tessellate
  watertight with the mesh volume exactly the exact volume, while the
  ring faces keep the named refusal. The box×box entry retires from
  the pairing-table refusal message; box×cylinder remains outside by
  name (one more slice).
- **Slice 4 (landed 2026-10-30, K4-2d/3):** the dispatch table
  closes — box×cylinder, the cylindrical bore. The frame contract
  (the axis dot bit-exactly ±1 against exactly one box axis) and the
  interior-cross-section contract (the disk strictly inside the
  cross-section rectangle — every annular face a regular region) are
  part of the pairing, their honest boundaries refusing by name: the
  crossing disk the trim-domain slice, the tangent disk the pinch,
  the near-parallel axis the never-snap discipline, the flush
  landing the coplanar-merge family. The render channel stays honest
  both ways: the annulus-carrying results refuse by name (the
  hole-trimmed faces are not tessellated by this reader), the
  clipped-cylinder commons and whole-shape outcomes mesh watertight.
  The pairing table now covers every combination of the three
  analytic classes — the catch-all refusal arm retires, and the
  compiler's match exhaustiveness is the proof; the
  outside-the-pairing-table refusal message retires with it (the
  crossing-disk boundary inherits the honest role for the
  configuration the old test pinned).
- **Slice 5 (landed 2026-10-30, K4-2d/3):** the render channel's
  trim-domain classification. The tessellator reads the
  wire-orientation convention (region on the left of the traversal,
  viewed from the face's outward normal): the walk's UV winding
  against the parameterization's own orientation picks the side —
  interiors tile as UV polygons (the planar kernel plus the curvature
  grid plus the deflection referee), exteriors as the sphere's
  parameter box minus the walk (the pole lines and the seam
  synthesized the way the primitives spell their own), and a single
  closed latitude circle as the cap band to the classified pole. The
  planar path went multi-wire: the hole wires bridge through the
  kernel's own hole-bridging triangulator, the hole rims entering as
  the shared edge samples they are. The box×box family renders
  entirely (rectilinear holes triangulate exactly), the box×cylinder
  family entirely, the box×sphere corner/face/offset stations for all
  three ops — and through the real dispatch the imported-box cuts
  carry working display channels. Honest boundaries, named: the
  edge-station's pole-crossing patch (the authored pole-run tiling is
  the next slice), the exterior trim on non-sphere surfaces,
  hole-trimmed curved domains, B-splines.
- **Later slices:** migrating the remaining mesh-result features
  (multi-fuse, mirroring, transform, PartDesign) to `Option<TopoShape>`
  results; the trim-domain classification (the crossing disk's
  side-clipped sections), the coplanar-face merge, and the
  OCCT-authored-result reader widening;
  PartDesign exact dispatch.

## What we do NOT claim

- **General BRep booleans through features.** Six families
  (sphere×sphere, cylinder×cylinder, cylinder×sphere, box×sphere at
  the boundary-skeleton and offset stations, box×box among
  frame-parallel boxes, and box×cylinder at the
  interior-cross-section stations) are wired
  (sphere×sphere, cylinder×cylinder, cylinder×sphere, box×sphere at
  the boundary-skeleton and offset stations, box×box among
  frame-parallel boxes, and box×cylinder at the
  interior-cross-section stations) are wired — the pairing table
  closed over the three analytic classes; the rest
  refuse by name — including the families' own honest boundaries
  (the off-skeleton general overlaps, the interior center crossing
  two or more faces, the oversized bite, the near-but-not-bit-exact
  station, the rotated box×box frame, the box×box coplanar fuse and
  split cut, the box×cylinder crossing and tangent disks, the
  box×cylinder flush landing and internal void) — and the RENDER
  channel's own named boundaries (the edge-station's pole-crossing
  patch, the exterior trim on non-sphere surfaces, hole-trimmed
  curved domains, B-spline faces). The trim-domain classification
  (the crossing disk's side-clipped sections, the authored pole-run
  tiling) is the named widening path.
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
  this ADR, not silently. *(The render channel widens the same way:
  slice 5's landing amended the render-refusal lists above — the
  annuli, rings, caps, patches and period-safe complements now
  tessellate, the named boundaries shrinking to the pole-run tiling
  and the crossing-disk sections.)* *(Exercised 2026-10-30: the K4-2d/3
  landings — slice 1 and the offset-station slice 2 the same day —
  amended rules 3 and this section's lists: the box×sphere family
  above; slice 3 the same day: the box×box family, the second
  amendment; and slice 4 the same day: the box×cylinder family, the
  third amendment — and the table's closure over the three analytic
  classes, the catch-all arm retiring with the compiler's
  exhaustiveness as the witness.)*
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
