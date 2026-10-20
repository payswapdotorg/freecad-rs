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
* Every mutating command (`new`, `add`, `set`, `recompute`, `undo`,
  `redo`) saves the file back. Even when a recompute fails, the document is saved — with
  the failing object marked `Error`, FreeCAD-style — before exiting 1, so
  no work is lost.
* `add` and `set` steps are **undoable**: each one appends a transaction
  to the document's session journal (`FILE.undolog`, see `undo`), and
  `undo`/`redo`/`history` operate on it. Expression bindings
  (`set-expr`/`clear-expr`) are not journaled steps.
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
(a BSP-tree mesh boolean; watertight for watertight box/cylinder
operands — curved-seam sphere booleans are on the roadmap):

```sh
freecad-rs add demo.fcstd --name Notch cut --base Base --tool Stem
#   recomputed Notch (Part::Cut)
freecad-rs add demo.fcstd --name Blob fuse --base Ball --tool Stem
freecad-rs add demo.fcstd --name Core common --base Base --tool Ball
```

Changing either operand's parameters (e.g. `set demo.fcstd Stem Radius
3`) touches the boolean like any other dependent.

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
