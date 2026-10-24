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
sixteen mappable kinds with FreeCAD's own ordinals — verified against
the r39109 binary, `fixtures/freecad_sketch_constraints.fcstd` and
`fixtures/freecad_tangent_symmetric_block.fcstd`),
expression bindings, and the FreeCAD 1.0 `<Sketch>` fragment family.
Constraint GeoIds route through a remap (skipped geometry shifts every
later index); axis/root-point/external references, `IsDriving="0"`
reference constraints and the unmappable kinds are skipped with warnings
naming the FreeCAD kind.

**Nothing else is dropped either — it is preserved verbatim.** A
load→save cycle never destroys data a real FreeCAD wrote:

- **OCCT BRep shape payloads** (`.brp`) ride along byte-identically
  under their original entry names — imported `Part::Feature` shapes
  and the last-good shapes of errored features survive the trip (the
  two cases where the file's shape is the only copy of the data). A
  recomputed feature replaces its preserved payload with our own.
- **Unconsumed zip entries** re-emit byte-identically with their
  original storage form: `GuiDocument.xml`, `Thumbnails/Thumbnail.png`,
  element maps, anything else a real FreeCAD wrote.
- **Unmodeled property types** (and malformed values of modeled ones)
  keep their raw `<Property>` elements, re-emitted verbatim — including
  enumerations with their `CustomEnumList`s, transient `<_Property>`
  markers, `<Extensions>` blocks.
- **Property `status` bits round-trip on modeled properties too**: the
  `status` attribute on a `<Property>` is FreeCAD's persisted
  property-state bitmask (`1` Touched, `4` ReadOnly, `8` Hidden,
  `8388608` LockDynamic, `134217728` …) — the read-only/hidden editor
  preferences a user set live there. The model re-serializes modeled
  property values canonically but re-emits the file's `status`
  verbatim (unmodeled properties carry theirs inside the raw element
  by construction). Verified with the real r39109 binary: a fixture
  with `Length` read-only (`status="4"`) and `Width` hidden
  (`status="8"`) round-trips and FreeCAD still reports
  `getEditorMode('Length') == ['ReadOnly']`,
  `getEditorMode('Width') == ['Hidden']`; all 32 status attributes of
  the fixture survive identically.
- **Document metadata** (Uid, Company, License, `StringHasher`, root
  attributes, `ObjectDeps`) round-trips verbatim.
- **`App::PropertyLinkSub` is modeled end to end** — a link plus its
  subelement names (`PartDesign` profiles, `Part::Mirroring`'s
  `MirrorPlane`): the loader parses target *and* `<Sub>` children, the
  writer re-emits FreeCAD's own byte form (`<LinkSub value="…"
  count="N">` with `<Sub value="…"/>` children — verified against
  r39109's own serialization, which uses the open form even when
  empty), and `link_targets` follows the edge (a `MirrorPlane` →
  datum link orders the recompute). `Part::Mirroring`'s `MirrorPlane`
  resolves like FreeCAD's `FeatureMirroring`: a datum plane's placement
  gives the plane exactly (position, rotation · +Z — verified: a
  `Part::Plane` at (30, 0, 0) rotated 30° about X resolves to
  `Base` = (30, 0, 0), `Normal` = (0, −0.5, 0.8660254…), with
  FreeCAD's own write-back into `Base`/`Normal`); a bare non-datum
  link errors exactly where FreeCAD's does ("No subelement
  specified" — verified on r39109: state `Invalid`). Unknown
  `Part::`-family object types keep their real type in the table row
  (through the loader's `TypeId` convention) — a `Part::Plane` datum
  round-trips as a `Part::Plane`, so a real FreeCAD restores the right
  class instead of demoting it to the family root.
- **A round-trip injects nothing either.** The no-inversion of
  preservation: the document-level property *set* comes back exactly as
  the file carried it. The loader records which of the modeled document
  trio (`Label`/`Creator`/`CreationDate`) the source actually wrote, and
  the writer re-emits exactly that subset (fresh documents — never
  loaded from a file — still write all three). FreeCAD 1.0 writes no
  doc-level `Creator` (it moved to `CreatedBy`), so its files must not
  gain one; older FreeCAD files that carry both keep both. Verified with
  the real r39109 binary both directions: our re-save of a 1.0 file has
  the identical 20-name property set, FreeCAD's own re-save of our file
  has no `Creator` (with the injection, it would have materialized as a
  dynamic document property and contaminated every later save), and
  every object's property set round-trips exactly (17/7/23 properties
  on the lossless fixture's three objects).
- **Sketch constructs the solver cannot express** — construction
  geometry, off-plane/unmodeled curve kinds, `SnellsLaw`-style
  constraints — keep their raw elements at their original GeoId/list
  slots, and the reader's compaction remap is inverted on write, so
  every constraint reference keeps the file's own numbering.
- **Unknown object types** load as opaque `ForeignObject`s: real name
  and type in the document, raw `ObjectData` verbatim, links to them
  resolve, nothing recomputed.

The load report says so in as many words: every preservation carries a
`preserved verbatim` note, and `fc_app::is_preservation_warning`
separates "kept, not modeled" from "kept nothing".

Verified against the real r39109 binary both ways on
`fixtures/freecad_lossless_full.fcstd` (an imported STEP shape, a
parametric Box, a sketch with construction geometry and a `SnellsLaw`
constraint, document metadata, GUI entries): every preserved entry is
byte-identical across a round trip, and FreeCAD opens our re-saved
file with the same three objects, the same imported volume
(7178.097245096173), the same 6-geometries/9-constraints sketch, the
same metadata — recompute clean.

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

Geometry payloads: FreeCAD restores **only** OCCT BRep, our kernel emits
parametric truth instead — the same trade-off FreeCAD itself makes when
it writes parameters. A one-line touch-and-recompute in FreeCAD
rebuilds the whole model natively:

```python
doc = FreeCAD.openDocument("fcrs-demo.FCStd")
for o in doc.Objects: o.touch()
doc.recompute()   # OCCT rebuilds: Base 3000.000, Cut 2497.345, Ball 904.779
```

(Files written by freecad-rs mark their parametric features `Touched`
in the object table, matching the state their geometry is in. The
**last-good shapes of errored features** and imported `Part::Feature`
meshes are the two cases that persist geometry, exactly the payloads a
FreeCAD file carries — and for files loaded *from* FreeCAD, those
original payloads re-emerge byte-identically on save.)

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
  Perpendicular=10, Radius=11, Equal=12, PointOnObject=13, Symmetric=14,
  Block=17, Diameter=18, …), `PointPos` slots shared (start=1, end=2,
  mid=3 — a circle's center is its `mid`, and a standalone point
  resolves every slot, exactly FreeCAD's GeomPoint), unused references
  `GeoUndef` (-2000), Angle values in radians. The sixteen mappable
  kinds map both ways; the rest is skipped with warnings that name the
  FreeCAD kind. Tangent maps FreeCAD's simple (edge) form — line-circle
  in either order and on either side, circle-circle external tangency
  (FreeCAD's Orientation-less default), line-line collinearity (FreeCAD's
  own two-equation decomposition). Symmetric maps both forms — about a
  line (Third, ThirdPos=0) and about a point (Third, ThirdPos=the
  point's slot). Block freezes its geometry's parameters out of the
  solver's free set (FreeCAD's fix-parameter treatment), so other
  constraints solve around the frozen values.
- STEP exchange (import and export) was verified earlier against the
  same FreeCAD 1.0 binaries — see the `fc-step` crate tests for the
  exact fixtures (FreeCAD's own sphere/cone/torus exports included).

## Known differences — the honest list

- **Booleans classify exactly; curved operands still tessellate.**
  Vertex sides are decided by exact integer predicates (a certified
  `f64` filter, then a 256-bit fast path, then a wide integer path
  for adversarial exponent spreads) with a construction-noise band
  seven orders below the old `PLANE_EPS` — a face offset as small as
  one millionth of a model unit is now cut where it belongs (the old
  tolerance snapped it a full 1e-5 away). What remains approximate on
  curved operands is the *tessellation* — the deflection — not the
  classification. The rotated-crossing planar-seam defect recorded in
  v0.1.7 — a rotated planar boundary *crossing* the other operand's
  faces could lose or duplicate whole chunks (kernel 447.03 against
  the exact truth 430.5; recorded then as an executable `#[ignore]`d
  test) — is **closed by the exact seam weld**, landing in v0.1.8:
  a twin weld at construction noise 2^-40, T-junction interleave at
  the seam's own noise, and needle-cap collapse, under a
  wholesale-or-revert volume bound. The former defect record now runs
  and passes —
  `boolean_rotated_box_crossing_faces_matches_exact_reference`:
  cut 430.5, fuse 646.5, common 49.5 against the exact
  rational reference (`scripts/exact_boolean_reference.py`), all
  watertight. Axis-aligned crossings remain exact.
- `Part::Mirroring`'s `MirrorPlane` **face-subelement** links (a mirror
  through "Face3" of a linked shape) cannot re-resolve at mesh level —
  no OCCT face numbering survives tessellation — so the plane FreeCAD
  itself persisted into `Base`/`Normal` on its last execute is used
  (verified against r39109: FreeCAD writes the resolved plane there on
  every execute, and its own files carry it). **Datum-plane links**
  (`Part::Plane` and family) resolve exactly from the datum's
  placement, with FreeCAD's own write-back into `Base`/`Normal`.
- No GUI: the `fc-viewer` renders offscreen; there is no workbench,
  no interaction loop. This is the CLI/kernel side of FreeCAD. (GUI
  *data* — `GuiDocument.xml`, thumbnails — round-trips verbatim.)

### What the parity gate has caught (2026-10-24, first contact — all fixed)

The `parity` workflow (`.github/workflows/parity.yml`, harness in
`scripts/parity/`) runs a pinned real FreeCAD 1.0 (r39109) over the
committed fixture corpus in both directions. On its first full local
run it found three fixture defects the Rust suite could never see:

1. `freecad_sketch_constraints.fcstd` — FreeCAD's solver reports
   *conflicting constraints* (2, 3, 5, 8, 9, 10, 11, 13) and leaves
   the sketch `Touched/Invalid`. The authored set was redundant
   (`Perpendicular` + `Angle` + `Horizontal` + `Vertical` overlap;
   `Radius` 9 and `Diameter` 18 on the same circle).
2. `partdesign_body.fcstd` — the Pad's `Profile` is persisted as
   `App::PropertyLink`, but FreeCAD 1.0's Pad uses
   `App::PropertyLinkSub`; on restore the link drops ("No object
   linked") and the Pad goes `Touched/Invalid`.
3. `group_links.fcstd` — the hand-authored type `App::DocumentGroup`
   is not a FreeCAD 1.0 document type (the real type is
   `App::DocumentObjectGroup`); FreeCAD drops the group objects
   *silently* when opening the file.

**Resolution (2026-10-26):** all three were fixture-authoring defects,
not engine defects — the corpus predated the parity gate and had been
hand-authored in forms real FreeCAD never writes. All three fixtures
are now regenerated **through real FreeCAD itself**
(`scripts/parity/regen_fixtures.py`, freecadcmd r39109): the sketch
constraint set is redesigned DOF-consistent (two sketches, every one of
the thirteen mappable kinds, FreeCAD reports both solved and fully
constrained), the Pad's `Profile` rides in the LinkSub form FreeCAD
1.0 wrote, and the groups are real `App::DocumentObjectGroup`s with
`App::PropertyLinkList` membership. The loader grew to match real
FreeCAD's forms: `App::DocumentObjectGroup` is a mapped container type
(the legacy spelling still loads), and `App::PropertyXLink` with an
empty `file` (an `App::Link`'s `LinkedObject`) maps as the `LinkSub`
it is semantically (external-file XLinks stay preserved verbatim —
outside this kernel's model). The quarantines are lifted
(`PARITY_QUARANTINE` stays as the mechanism for the next find).
