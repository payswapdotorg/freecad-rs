# `freecad-rs` — CLI reference

`freecad-rs` is the command-line front-end of the `fc-*` crates: a
FreeCAD-style **parametric document** you can drive from a shell, in the
spirit of FreeCAD's `FreeCADCmd`.

The model of use is **stateless-by-file**: every command loads a `.FCStd`
document, does its work — recomputing dependencies-first when needed — and
saves it back. There is no daemon and no session; the file *is* the session.

```
freecad-rs <COMMAND> [ARGS]        # try `freecad-rs help` and
freecad-rs <command> --help         # `freecad-rs add --help` etc.
```

## Files

`.FCStd` is FreeCAD's format: a zip archive whose first entry is
`Document.xml`, plus one side file per stored shape (we write our FCBREP
text format where FreeCAD writes OCCT `.brp`).

* Files written by this tool round-trip losslessly through `fc-io`.
* Real FreeCAD files open: `Document.xml` is schema-compatible, so all
  **parameters** come back — a `Part::Box`'s Length/Width/Height,
  placements, links — and the model recomputes from them. OCCT `.brp`
  shape payloads cannot be read by this kernel; they are detected and
  skipped with a warning (parameters kept, cached geometry regenerated
  where possible).

## Conventions

* Exit codes: `0` ok · `1` runtime error (printed as `error: …` on
  stderr) · `2` usage error.
* Every mutating command (`new`, `add`, `set`, `pad`, `pocket`,
  `revolution`, `sketch-import`, `recompute`, `undo`,
  `redo`) saves the file back. Even when a recompute fails, the document is saved — with
  the failing object marked `Error`, FreeCAD-style — before exiting 1, so
  no work is lost.
* `add`, `set` and the PartDesign verbs (`pad`, `pocket`,
  `revolution`) are **undoable**: each one appends a transaction
  to the document's session journal (`FILE.undolog`, see `undo`), and
  `undo`/`redo`/`history` operate on it. Expression bindings
  (`set-expr`/`clear-expr`) and sketch content swaps (`sketch-import`)
  are not journaled steps.
* Object names are unique and never renamed. `--name` is a *base* name:
  adding a second `Box` yields `Box001`, FreeCAD-style.
* Units are millimetres (FreeCAD's default); `stats` prints them.

## Commands

### `new` — create a document

```sh
freecad-rs new demo.fcstd --label "Playground"
# created demo.fcstd — label "Playground" (0 objects)
```

Refuses to overwrite an existing file.

### `add` — add a shape or transform feature

`--name` belongs to `add` itself, so it comes **before** the shape
subcommand:

```sh
freecad-rs add demo.fcstd --name Base box --length 30 --width 20 --height 5
freecad-rs add demo.fcstd --name Ball sphere --radius 4
freecad-rs add demo.fcstd --name Stem cylinder --radius 1.5 --height 12
freecad-rs add demo.fcstd cone --radius1 2 --radius2 0 --height 10
freecad-rs add demo.fcstd torus --radius1 10 --radius2 2
freecad-rs add demo.fcstd --name Lift transform --source Ball \
    --translate 0,0,12 --rotate-axis 0,0,1 --rotate-deg 25 --scale 1
```

Shapes are the FreeCAD `Part::` primitives with FreeCAD's defaults
(box 10×10×10, sphere r 5, cylinder r 2 h 10, cone r 2/0 h 10, torus
r 10/2). Omit `--name` to take the type's default name.

`transform` links to a `--source` object (which must exist), applies a
`Placement` built from `--translate x,y,z` and a rotation
`--rotate-deg` about `--rotate-axis x,y,z` (default `0,0,1`), and a
uniform `--scale`. It is the canonical parametric dependency: change the
source's parameters and the transform recomputes.

After every `add` the document is recomputed and each executed object is
reported:

```
  recomputed Lift (Part::Transform)
added Lift (Part::Transform) -> demo.fcstd
```

### `set` — change a property (the parametric trigger)

```sh
freecad-rs set demo.fcstd Ball Radius 6
```

The value is parsed as the property's existing type (see the text-form
table below). Setting a property marks the object **and everything that
(transitively) depends on it** as touched — the parametric cascade — then
recomputes and saves:

```
  touched Ball
  touched Lift
  recomputed Ball (Part::Sphere)
  recomputed Lift (Part::Transform)
set Ball.Radius = 6
```

### `get` — print a property

```sh
freecad-rs get demo.fcstd Ball Radius
# 6
```

### `list` — objects in creation order

```sh
freecad-rs list demo.fcstd
# NAME   TYPE             STATUS
# Base   Part::Box        Valid
# Ball   Part::Sphere     Valid
# Lift   Part::Transform  Valid
```

`--json` emits the same rows as a JSON array (hand-rolled, no serde):

```sh
freecad-rs list demo.fcstd --json
# [{"name":"Base","type":"Part::Box","status":"Valid"}, ...]
```

### `recompute` — run touched objects, dependencies-first

```sh
freecad-rs recompute demo.fcstd
#   recomputed Ball (Part::Sphere)
#   recomputed Lift (Part::Transform)
# 2 recomputed, 4 total
```

Objects already `Valid` are skipped. `--json` prints the recomputed
objects as the same `name`/`type`/`status` array as `list --json`.

### `undo` — roll back committed steps

```sh
freecad-rs undo demo.fcstd        # one step
freecad-rs undo demo.fcstd 3      # three steps
```

```
undid "set Ball.Radius" (1 change)
  recomputed Ball (Part::Sphere)
  recomputed Lift (Part::Transform)
1 undone, 1 to redo
```

Each editing step made with `add` or `set` is a transaction in the
FreeCAD `App::Transaction` sense: a named, all-or-nothing record of what
it changed. Undoing applies it **backwards** — properties return to
their before-values, added objects are removed (parked, not lost) — and
marks the affected objects plus everything downstream of them touched,
so the document is recomputed exactly like a `set` would, then saved.
Asking for more steps than exist simply undoes what is there; asking
with no history prints `nothing to undo`.

### `redo` — re-apply undone steps

```sh
freecad-rs redo demo.fcstd        # one step
freecad-rs redo demo.fcstd 3      # three steps, oldest-undone first
```

Symmetric to `undo`, replaying the undone steps chronologically. A newly
committed step (`add`/`set`) clears the redo future, FreeCAD-style.

### `history` — the undo stack and redo count

```sh
freecad-rs history demo.fcstd
# undo stack (2)
#   1. add Ball (1 change)
#   2. set Ball.Radius (1 change)
# redo available: 1
```

#### The session journal (`FILE.undolog`)

The CLI is stateless-by-file, so the transaction stacks live in a
sidecar journal next to the document: `demo.fcstd` →
`demo.fcstd.undolog`. `add` and `set` append one transaction per step
(an added object is journaled with its full state — type, properties,
expressions — so undoing an addition and later redoing it can rebuild
it); `undo`/`redo` rehydrate the in-memory stacks from the journal,
run the real `fc_app` transaction machinery on the loaded document,
recompute, then save both the document and the rewritten journal — in
that order, so the two never disagree (a failed recompute still saves
both and then reports the error, exit 1).

The journal holds at most 100 transactions; a new step drops the oldest,
FreeCAD-style. Deleting the journal (or never creating one) is safe:
the document is self-contained, only its edit history is lost —
`undo` then reports `nothing to undo`. Editing a document with other
tools invalidates its journal; `undo`/`redo`/`history` report the
mismatch rather than guessing.

### `info` — document metadata

```sh
freecad-rs info demo.fcstd
# label:   Demo model
# creator: freecad-rs
# created: 2026-10-19T21:30:00Z
# objects: 4
#   Part::Box x 1
#   Part::Cylinder x 1
#   Part::Sphere x 1
#   Part::Transform x 1
```

### `stats` — analytic geometry

```sh
freecad-rs stats demo.fcstd
#   Base (Box): volume 3000.000 mm³, area 1700.000 mm², 12 triangles
#   ...
# total: 4 objects, volume 7656.891 mm³
```

Volumes and areas are the primitives' **exact analytic** values
(`TopoShape::volume` / `surface_area`); triangle counts come from
tessellation at 0.2 mm chordal deflection. Objects that have not computed
a shape yet report `no shape yet` (run `recompute` first).

### `export svg` — render the document

```sh
freecad-rs export svg demo.fcstd demo.svg --view iso
# wrote demo.svg (view=iso, 4 objects, 948 triangles)
```

Views (orthographic, shaded, 960×720):

| view    | camera                                   |
|---------|------------------------------------------|
| `iso`   | above, slightly front-right; `+X` projects down-right, `+Z` up |
| `top`   | looking down `-Z`; screen x = world x, screen y = world y |
| `front` | looking along `+Y`; screen x = world x, screen y = world z |
| `right` | looking along `-X`; screen x = world y, screen y = world z |

Rendering is a painter-sorted, flat-shaded software rasterizer: one `<g>`
per object with a hover `<title>`, grayscale shading from a headlight
(`0.30 + 0.62 · max(0, n·L)`), hairline `#333` strokes, white
background. An object's `Placement` property is applied at render time
(`Part::Transform` results already embed theirs). A failed recompute
aborts the export without writing the file.

### `graph` — dependency graph as Graphviz DOT

```sh
freecad-rs graph demo.fcstd graph.dot
# wrote graph.dot (4 objects, 1 edges)
```

Edge `A -> B` means *A depends on B*. Render with `dot -Tsvg graph.dot`.

## Property text forms

Values for `set` (and the output of `get`) use `fc_app::Property`'s text
forms:

| FreeCAD type             | text form                             | example                                |
|--------------------------|---------------------------------------|----------------------------------------|
| `App::PropertyBool`      | `true` / `false`                      | `true`                                 |
| `App::PropertyFloat`     | decimal number                        | `2.5`                                  |
| `App::PropertyInteger`  | integer                               | `7`                                    |
| `App::PropertyString`    | any text                              | `hello`                                |
| `App::PropertyVector`    | `x,y,z`                               | `1,2,3`                                |
| `App::PropertyPlacement` | `pos=(x,y,z);axis=(x,y,z);angle=deg`  | `pos=(0,0,12);axis=(0,0,1);angle=25`   |
| `App::PropertyLink`      | object name                           | `Ball`                                 |
| `App::PropertyLinkList`  | object names, comma-separated         | `Ball,Stem`                            |

Whitespace around scalars is tolerated on parse. Placement segments may
appear in any order (angle in degrees).

Primitives' shapes (`Part::PropertyPartShape`) are computed, not typed —
they do not round-trip through `set`.

## The parametric story

FreeCAD's core idea, reproduced here: objects are **features** whose
inputs are typed properties, and whose outputs are computed from them,
*dependencies-first*.

1. `add` a `transform` that links `--source Ball`: the document now has
   the edge `Lift -> Ball` in its dependency graph.
2. `set demo.fcstd Ball Radius 6`: the property write touches `Ball`
   **and** everything downstream (`Lift`), in creation order.
3. `recompute` executes touched objects in topological order — `Ball`
   always runs before `Lift` — each object's `execute` reading its
   inputs from the other objects via links. A cycle or a dangling link
   is an error before anything runs; a failing `execute` marks that
   object `Error` and aborts the run (after saving).
4. `export svg` / `stats` then observe the recomputed geometry.

The full walkthrough lives in `scripts/demo.sh`; its artifacts
(`examples/demo.fcstd`, `examples/demo.svg`, `examples/graph.dot`) are
committed to the repo.

## Booleans: `add cut|fuse|common`

`Part::Cut`, `Part::Fuse` and `Part::Common` link to a `Base` and a
`Tool` object and combine their tessellated geometry at recompute time
(a BSP-tree mesh boolean; watertight for watertight operands, curved
seams included — the seam's weld keeps the operands' own surfaces
exact, and the output shell is consistently oriented, the shape
contract a B-rep kernel like OCCT enforces):

```sh
freecad-rs add demo.fcstd --name Notch cut --base Base --tool Stem
#   recomputed Notch (Part::Cut)
freecad-rs add demo.fcstd --name Blob fuse --base Ball --tool Stem
freecad-rs add demo.fcstd --name Core common --base Base --tool Ball
```

Changing either operand's parameters (e.g. `set demo.fcstd Stem Radius
3`) touches the boolean like any other dependent.

## PartDesign: `pad` / `pocket` / `revolution`

Sketch-consuming features — FreeCAD `Mod/PartDesign` distilled: the
`Profile` is a `Sketcher::SketchObject` in the document, and the
feature's recompute turns the **solved** sketch geometry into a solid
(the FC-6 profile kernels: wire extraction, extrusion, seam-welded
revolution). The dependency graph guarantees the sketch recomputes
first; a failed sketch or an open profile marks the feature `Error` and
aborts the recompute (exit 1, document saved with the failure on
record). Like every computed feature, a failing recompute drops the
previous result — dependents of an errored feature fail honestly on
their next run.

### `pad` — extrude a sketch into a solid

```sh
freecad-rs new demo.fcstd
freecad-rs add demo.fcstd sketch
freecad-rs sketch-import demo.fcstd Sketch rect.xml   # a 20x10 rectangle
freecad-rs pad demo.fcstd --sketch Sketch --length 10
#   recomputed Sketch (Sketcher::SketchObject)
#   recomputed Pad (PartDesign::Pad)
# added Pad (PartDesign::Pad) -> demo.fcstd
#   Pad: Valid, volume 2000.000 mm³, 12 triangles
```

Extrudes the sketch's profile along +Z (`--reversed` for −Z),
optionally symmetric about the sketch plane (`--midplane`, spanning
±Length/2). `--name` overrides the default object name `Pad`. The final
status line reports the recompute outcome honestly: a failed feature
prints its error and `no computed geometry` instead of a volume.

### `pocket` — cut an extrusion out of a base feature

```sh
freecad-rs add demo.fcstd --name Body box
freecad-rs add demo.fcstd --name Hole sketch
freecad-rs sketch-import demo.fcstd Hole square.xml     # a 4x4 square
freecad-rs pocket demo.fcstd --sketch Hole --base Body --length 4
#   Pocket: Valid, volume 936.000 mm³, 83 triangles
```

Extrudes the profile into a tool solid and subtracts it from `--base`
(any mesh-bearing object — a `Part::` primitive, another PartDesign
feature, a boolean). The tool extrudes along +Z — into a base sitting
on `z >= 0`, which is where this kernel's primitives live (FreeCAD's
pocket cuts along the negative sketch normal into *attached* material;
there is no attachment system yet, so the default is the direction
that intersects a canonical base). `--inverted` selects −Z, `--midplane`
spans the tool symmetrically about the sketch plane.

### `revolution` — sweep a sketch around an axis

```sh
freecad-rs add demo.fcstd --name Lathe sketch
freecad-rs sketch-import demo.fcstd Lathe half-disc.xml  # arc + axis line
freecad-rs revolution demo.fcstd --sketch Lathe --angle 360
#   Revolution: Valid, volume 520.163 mm³, 2400 triangles   # ~4/3*pi*5³
```

Sweeps the profile around an axis through the origin. `--angle` is
degrees (`--angle 180` sweeps half the volume; the sign is the sweep
direction, and `|angle| >= 360` is a seam-welded full turn). The two
`--axis` choices, honestly framed:

| axis | meaning |
|------|---------|
| `z` (default) | the kernel's classic **lathe frame**: sketch `x` is the radius and sketch `y` the height about the 3D Z axis. A sketch in the XY plane cannot be revolved about its own normal, so the coordinates are reinterpreted in the XZ half-plane. |
| `y` | the sketch's **vertical axis**: the drawn profile revolves about 3D Y (distance `|x|` from the axis at height `y`). The sweep starts at the YZ half-plane — the kernel's canonical frame — so the sketch's own plane sits 90° into the sweep; invisible for a full turn, a placement question otherwise. |

The profile must stay on the +x side of the sketch (`x` is the radial
coordinate; points on the axis itself are fine and snap to shared
vertices). A profile crossing the axis sweeps a self-intersecting
solid — the caller's problem, exactly as in FreeCAD.

### The parametric story, PartDesign edition

The features are ordinary document objects, so the whole cascade
applies: `set demo.fcstd Pad Length 25` touches the Pad and
recomputes it; re-importing a sketch (`sketch-import ... a wider
rectangle`) touches the sketch **and every feature profiling it**, and
the pad's volume scales with the new sketch width. `Length` and
`Angle` are `App::PropertyFloat` like the Part primitives' dimensions,
so expression bindings work on them (`set-expr demo.fcstd Pad Length
"2 * 3"` — dimensionless, like every Float binding).

## Expressions: `set-expr` / `clear-expr`

A Float property can be **bound** to an expression (FreeCAD
`setExpression`). The binding becomes a dependency-graph edge: changing
a referenced property re-evaluates the binding (dependencies first),
and the bound property is read-only until cleared.

```sh
freecad-rs set-expr demo.fcstd Stem Height "2 * Base.Length / 3"
freecad-rs get demo.fcstd Stem Height
#   20
#     (bound to expression: 2 * Base.Length / 3)
freecad-rs clear-expr demo.fcstd Stem Height
```

Grammar (module docs in `fc_app::expression`): arithmetic `+ - * / % ^`
(`^` right-associative), parentheses, references `Object.Property` and
`<<Object Label>>.Property` (a bare `.Property` reads the object
itself), functions `sin cos tan asin acos atan atan2 sqrt abs exp ln
log10 pow floor ceil round min max hypot` — **trig in degrees**
(`sin(30) = 0.5`), constants `pi`/`e`, and unit suffixes folded at parse
time (`5mm`, `5cm`, `5m`, `5in`/`5"`, `30deg`, `30rad`). Expression
cycles (`A.x = B.y`, `B.y = A.x`) are rejected by the recompute's cycle
check.

## Mesh export: `export obj|stl`

```sh
freecad-rs export obj demo.fcstd demo.obj      # named groups per object
freecad-rs export stl demo.fcstd demo.stl        # ASCII STL
freecad-rs export stl demo.fcstd demo.stl --binary
```

All exports recompute first (no stale geometry is ever written). OBJ
emits one `o` group per document object with shared vertices; STL
merges everything into one solid (the format carries no names) with
exact `84 + 50·n` binary sizing and outward normals from the CCW
winding.

## Viewer: `render`

```sh
freecad-rs render demo.fcstd demo.png                  # iso-view PNG
freecad-rs render demo.fcstd demo.png --view top       # top/front/right too
freecad-rs render demo.fcstd x.png --width 800 --height 600
freecad-rs render demo.fcstd demo.png --pick 640,400   # object at that pixel
freecad-rs render demo.fcstd demo.png --bless          # refresh the golden
```

The M1 viewer surface: a headless, offscreen render of the document —
the same tessellation the CLI exports, flat-shaded, through the same
four standard cameras as `export svg` (`iso`/`top`/`front`/`right`).
Objects in the `Error` state still render, visibly flagged;
half-computed documents never crash the viewer. `--pick x,y` runs the
same render and reports which object (if any) is at that pixel.
`--bless` rewrites the committed golden image used by the crate's
render tests — goldens are regenerated, never hand-edited. Rendering
needs a GPU (or a software adapter such as lavapipe); without one the
command fails honestly.

## STEP exchange: `import-step` / `export step`

```sh
freecad-rs import-step doc.fcstd part.step --name Body --deflection 0.1
freecad-rs export step doc.fcstd part.step
```

STEP (AP203) exchange per ADR-0002. Import tessellates the STEP body
under an explicit chord `--deflection` (mm) and adds it as a
`Part::Feature` mesh object — import fidelity is deflection-derived
and tested against it. Export writes the document's solids as
*faceted* B-rep — legal STEP, honestly labeled as a tessellation (the
product name carries a faceted marker, asserted by tests). Pure Rust;
no OCCT.

## Quantity arithmetic: `calc`

```sh
freecad-rs calc "25 mm + 3 cm"          # 55 mm
freecad-rs calc "9.8 m/s^2 * 2 kg"      # 19600 mm*kg/s^2
freecad-rs calc "1 m + 1 kg"            # error: dimension mismatch
freecad-rs calc "30° + 30°"             # 60° ...in canonical form: 60 deg
```

`calc` evaluates pure-quantity arithmetic with unit checking
(`fc_base::quantity`): dimension-checked `+`/`-`, unit-combining
`*`/`/`, unary sign, parentheses, integer `^` powers. Values are stored
and printed in canonical units (mm, kg, s, A, K, mol, cd, deg).

## Sketches: `sketch-solve` / `sketch-show`

```sh
freecad-rs sketch-solve examples/sketch-quad.xml
#   sketch: 5 geometries, 8 constraints, DOF 7 (underconstrained)
#   solve: Solved in 4 iteration(s), residual 7.9e-14
#   geometry: ...
freecad-rs sketch-show examples/sketch-quad.xml
```

Sketches are FreeCAD-style `<Sketch>` XML fragments: `<Geometry>`
elements (point `0`, line `1`, circle `2`, arc `3`) and `<Constraint>`
elements with the crate's documented ordinals (`0` coincident,
`1` horizontal, `2` vertical, `3` parallel, `4` perpendicular,
`5`/`6`/`7` distance/X/Y, `8` radius, `9` diameter, `10` equal,
`11` angle, `12` point-on-object). `sketch-solve` runs the
Levenberg-Marquardt solver and prints the DOF report plus the solved
geometry (unsolved sketches are left bit-identical); `sketch-show`
prints geometry + constraints without solving.

Inside documents, sketches are `Sketcher::SketchObject`s (`add sketch`),
and three verbs work on their content:

```sh
freecad-rs add demo.fcstd sketch                       # empty, auto-named "Sketch"
freecad-rs sketch-import demo.fcstd Sketch rect.xml    # swap in fragment content
#   touched Sketch
#   recomputed Sketch (Sketcher::SketchObject)
#   Solved, 10 constraints, dof 2, residual 2.4e-13
freecad-rs sketch-info demo.fcstd Sketch               # counts, DOF, solve state
freecad-rs sketch-export demo.fcstd Sketch out.xml     # the <Sketch> fragment back
```

`sketch-import` verifies everything before writing (fragment parses,
object exists, object is a sketch), then swaps the content through the
load-time mutation window and touches the sketch **and its transitive
dependents** — the PartDesign features profiling it re-execute on the
recompute. A failed solve still saves (the sketch is marked `Error`,
FreeCAD-style) and exits 1. `sketch-info` re-runs the recompute in
memory (the file is not modified) and reports the fresh solve state;
solve results are recompute products and are never persisted.

## Scripting: `script`

```sh
freecad-rs script examples/build-house.rhai
# new document "House" (0 objects)
# added Base (Part::Box)
# added Chimney (Part::Cylinder)
# bound Chimney.Height = 2 * Base.Length
# ...
# 4 recomputed, 4 total
# Base volume: 3000.0
# saved house.FCStd
```

`script FILE.rhai` runs a [rhai](https://rhai.rs) script against a fresh
sandboxed engine — the console surface, distilled: a program drives the
parametric document the way FreeCAD's Python console does, in pure Rust
(no CPython embedding, `--locked`-friendly). The committed example
(`crates/fc-script/examples/build-house.rhai`) is a full walkthrough.

The engine prints a small honest transcript of what the script did —
added / set / bound / recomputed / saved, plus anything the script
`print`s. A worked session:

```rhai
let doc = new_document("House");
let base = doc.add_box("Base", 30.0, 20.0, 5.0);   // Part::Box
let chimney = doc.add_cylinder("Chimney", 1.5, 12.0);
doc.bind(chimney, "Height", "2 * Base.Length");    // setExpression
let lift = doc.add_transform("Lift", base);
doc.set(lift, "Placement",
        placement(vec3(0.0, 0.0, 14.0), vec3(0.0, 0.0, 1.0), 25.0));
doc.recompute();                                   // dependencies-first
print(doc.volume(base));                           // exact analytic stats
doc.save("house.FCStd");                           // .FCStd via fc-io
```

The API (one document per `new_document`/`open_document`; method syntax
on the handle):

| call | mirrors |
|------|---------|
| `new_document(label)` / `open_document(path)` | `new` / load (`.FCStd`) |
| `doc.add_box(name, l, w, h)` · `add_sphere(n, r)` · `add_cylinder(n, r, h)` · `add_cone(n, r1, r2, h)` · `add_torus(n, r1, r2)` | `add` (the `Part::` primitives) |
| `doc.add_transform(name, source)` · `add_cut/add_fuse/add_common(name, base, tool)` | `add transform` / the booleans |
| `doc.set(object, property, value)` | `set` — parsed against the property's **existing** type, exactly like the CLI (`6` into a Float, `"Ball"` into a Link, a `vec3` into a Vector, a `placement(...)` into a Placement); mismatches are honest errors |
| `doc.bind(object, property, "expr")` / `doc.unbind(...)` | `set-expr` / `clear-expr` — the `App::PropertyExpressionEngine` surface: parse-validated, Float-only, evaluated dependencies-first at every recompute |
| `doc.recompute()` | `recompute` — returns `true`/`false`; a failure is recorded, not raised |
| `doc.volume(name)` · `doc.area(name)` · `doc.bbox(name)` | `stats` — exact analytic numbers (`bbox` is a map with `min`/`max`/`size`/`center` arrays) |
| `doc.save(path)` | save (`.FCStd` via `fc-io`) |
| `doc.export_stl(path, binary)` · `doc.export_obj(path)` | `export stl` / `export obj` |

Numbers may be written `30` or `30.0` — both coerce, like a shell
argument; anything else is an honest `expected a number` error.

**Failure semantics mirror the CLI's.** Every API failure (unknown
object, unparseable value, bad binding, failed save) surfaces as a rhai
runtime error with the CLI's message style and stops the script with
`error: …` on stderr and exit 1. A **failed recompute** is the CLI's
honest state, not an exception: the script keeps running, the transcript
records `recompute failed: …`, the failing object stays in the document
marked `Error` — and the process exits 1 at the end with the same
message the CLI's recompute failure prints. A rhai parse or runtime
error is likewise a clear message and exit 1 (the transcript printed so
far stays on stdout). Persistence is the script's choice: only explicit
`save` calls write anything.

**Sandbox.** rhai gives a script no file, network or FFI access — the
document API above is the whole surface. An operation budget stops
runaway loops with an honest error instead of a hang.

Two honest kernel notes: a primitive's own `Placement` property is
carried but not baked into its analytic shape (set placements on a
`Part::Transform` to move geometry), and sketches are not reachable from
scripts (they need `fc-sketcher` fragments; use `add sketch` +
`sketch-import` on the CLI).
