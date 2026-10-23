# freecad-rs ↔ FreeCAD equivalence

What "the same as FreeCAD" means here, measured against **real FreeCAD
1.0.0** (AppImage r39109, `releases/FreeCAD-1-0`), and what it does not.
Every claim below was verified with the FreeCAD 1.0 binaries on this
machine; the corpus and the numbers live in this document so anyone can
re-derive them.

## The verification method

1. Real FreeCAD 1.0 (`freecadcmd`) builds a corpus of `.FCStd` files —
   primitives with non-default parameters, the three boolean families,
   a PartDesign Body/Sketch/Pad, placements and a `Part::Mirroring`.
2. freecad-rs loads each file, recomputes, and reports volumes/areas.
3. freecad-rs writes its own document with the same model; real FreeCAD
   opens it, verifies every parameter, placement, link and sketch, and
   rebuilds the geometry with OCCT.
4. Both directions must agree: primitives exactly, booleans within the
   tessellation deflection, placements bit-for-bit.

## Geometry semantics — verified equal

| Convention | FreeCAD 1.0 | freecad-rs |
|---|---|---|
| `Part::Box` local span | `[0,L] × [0,W] × [0,H]` (corner at origin) | same (verified via `Shape.BoundBox`) |
| `Part::Cylinder` | base on XY, `z ∈ [0,H]` | same |
| `Part::Cone` | `Radius1` at `z=0`, `Radius2` at `z=H` | same |
| `Part::Sphere`/`Part::Torus` | centered | same |
| Defaults | Box L/W/H 10, Sphere r 5, Cylinder r 2 h 10, Cone r1 2 h 10 (FreeCAD source) | same |
| `Placement` | positions the local shape; booleans consume placed operands | same rule (`placed_mesh_of`) |
| Mirroring | reflect through `(Base, Normal)`, orientation repaired | same, with re-wound triangles |

**Corpus numbers** (freecad-rs recomputing real FreeCAD files; OCCT's
exact values in parentheses): primitives and PartDesign Pad are **exact**
— Box 3000.000 (3000.000), Sphere 1436.755 (1436.755), Cylinder
603.186 (603.186), Cone 367.566 (367.566), Torus 631.655 (631.655),
Pad 800.000 (800.000), Mirroring 1000.000 (1000.000). Mesh booleans
agree to their deflection: Cut 3878.8 (3874.3, +0.12% at 0.2 mm),
MultiFuse 4181.2 (4187.2, −0.14%), Common 2507.9 (2512.4, −0.18%) —
the documented limit of a BSP boolean on tessellations.

## `.FCStd` interop — both directions

**FreeCAD files → freecad-rs.** Real FreeCAD 1.0 documents load: object
table, every modeled property type (including `App::PropertyPosition`
and `App::PropertyDirection` vectors, `LinkSub` profiles, nested
`<LinkList>`s), placements as quaternions, sketch geometry via
`Part::PropertyGeometryList` (point/line/circle/arc, XY-plane sketches),
**sketch constraints via `Sketcher::PropertyConstraintList`** (all
thirteen mappable kinds with FreeCAD's own ordinals — verified against
the r39109 binary, `fixtures/freecad_sketch_constraints.fcstd`),
expression bindings, and the FreeCAD 1.0 `<Sketch>` fragment family.
Constraint GeoIds route through a remap (skipped geometry shifts every
later index); axis/root-point/external references, `IsDriving="0"`
reference constraints and the unmappable kinds (Tangent, Symmetric,
Block, …) are skipped with warnings naming the FreeCAD kind.
Known-skipped payloads are reported as per-property warnings — OCCT BRep
shape payloads (`.brp`) keep their parameters and rebuild on recompute.

**freecad-rs files → FreeCAD.** Real FreeCAD 1.0 opens our files and
restores every parameter, link, placement and sketch — geometry AND
constraints (verified with the r39109 binary on
`fixtures/fcrs_constraints_roundtrip.fcStd`: all 13 constraints
restored with exact First/FirstPos/Second/SecondPos/Value, Angle's
radians bit-exact, recompute clean). Sketches save as the two
FreeCAD-native properties — `Geometry` (`Part::PropertyGeometryList`)
and `Constraints` (`Sketcher::PropertyConstraintList`, `<Constrain …/>`
elements with FreeCAD's `ConstraintType` ordinals and every attribute
FreeCAD's writer emits); our legacy `<Sketch>` fragment is retired from
the writer (the reader still accepts it — v0.1.0 files). The property
types are FreeCAD's own — dimension floats serialize as
`App::PropertyLength`, exactly as FreeCAD 1.0 writes them, because
FreeCAD's reader silently skips type-mismatched properties.

Geometry payloads are the honest boundary: FreeCAD restores **only**
OCCT BRep, our kernel emits parametric truth instead — the same
trade-off FreeCAD itself makes when it writes parameters. A one-line
touch-and-recompute in FreeCAD rebuilds the whole model natively:

```python
doc = FreeCAD.openDocument("fcrs-demo.FCStd")
for o in doc.Objects: o.touch()
doc.recompute()   # OCCT rebuilds: Base 3000.000, Cut 2497.345, Ball 904.779
```

(Files written by freecad-rs mark their parametric features `Touched`
in the object table, matching the state their geometry is in. The
**last-good shapes of errored features** and imported `Part::Feature`
meshes are the two cases that persist geometry, exactly the payloads a
FreeCAD file carries.)

## Format details that had to match

- `PropertyLinkList` serializes as `<LinkList count="N"><Link …/>…`,
  the only form FreeCAD's `Restore` accepts (it reads the `count`
  attribute and that many child elements).
- `Part::Box`-family dimensions are `App::PropertyLength`, not
  `App::PropertyFloat`; `PartDesign` lengths stay `App::PropertyFloat`
  (both checked against FreeCAD 1.0's own serialization).
- An empty `<Link value=""/>` means "no link" (every `PartDesign::Body`
  carries one in `BaseFeature`); it is not a dangling reference.
- Sketch geometry in FreeCAD 1.0 is the `Geometry` property
  (`Part::PropertyGeometryList`), not a `<Sketch>` fragment: point,
  line, circle and arc-of-circle elements map to this kernel's sketcher;
  construction geometry and non-XY planes are skipped with warnings.
- Sketch constraints are the `Constraints` property
  (`Sketcher::PropertyConstraintList`): `<Constrain …/>` elements
  (FreeCAD's own spelling — no trailing "t"), ordinals from FreeCAD's
  `ConstraintType` enum (Coincident=1, Tangent=5, Distance=6, Angle=9,
  Perpendicular=10, Radius=11, Equal=12, PointOnObject=13, Diameter=18,
  …), `PointPos` slots shared (start=1, end=2, mid=3 — a circle's
  center is its `mid`), unused references `GeoUndef` (-2000), Angle
  values in radians. The thirteen mappable kinds map both ways; the
  rest is skipped with warnings that name the FreeCAD kind.
- STEP exchange (import and export) was verified earlier against the
  same FreeCAD 1.0 binaries — see the `fc-step` crate tests for the
  exact fixtures (FreeCAD's own sphere/cone/torus exports included).

## Known differences — the honest list

- **Booleans are mesh-level** (BSP on tessellations, watertight,
  deflection-bounded volumes), not OCCT exact BRep booleans.
- **OCCT BRep payloads are not parsed** (`.brp` geometry is skipped,
  parameters kept); our own files persist parameters instead of shapes,
  so both systems rebuild on recompute.
- Sketch constraints beyond the thirteen mappable kinds (Tangent,
  Symmetric, Block, SnellsLaw, InternalAlignment, …) and their extended
  forms (two-point Horizontal, point-to-line Distance, angle-via-point)
  are skipped with warnings — this kernel's solver vocabulary is its own
  13-kind set, and unmappable constraints are never silently
  re-interpreted.
- `Part::Mirroring`'s `MirrorPlane` link (a face/datum override) has no
  geometry to read here — `Base`/`Normal` planes only.
- No GUI: the `fc-viewer` renders offscreen; there is no workbench,
  no interaction loop. This is the CLI/kernel side of FreeCAD.
