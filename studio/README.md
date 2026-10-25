# fc-studio — the freecad-rs workbench in the browser

A dark, FreeCAD-style CAD workbench that runs **the real freecad-rs
document engine in the browser** — not a mock, not a server round-trip:
the same Rust crates the CLI and CI run (`fc-app` / `fc-part` /
`fc-sketcher` / `fc-io`), compiled to `wasm32-unknown-unknown` by
[`crates/fc-wasm`](../crates/fc-wasm).

Everything is one HTML file with **zero JavaScript dependencies** — the
3D viewport is hand-rolled WebGL (orbit / pan / zoom, click-to-select
ray picking, the engine's own tessellations, flat shaded).

```
┌────────────────────────────────────────────────────────────┐
│ header: New Open Save STL OBJ · Undo Redo Recompute · Add │
├──────────┬──────────────────────────────────┬─────────────┤
│ document │  3D viewport (WebGL, Z-up)       │ properties  │
│ tree     │  orbit / pan / zoom / select     │ + stats +   │
│ + status │                                  │ expressions │
├──────────┴──────────────────────────────────┴─────────────┤
│ footer: label · objects · Σ volume · engine version        │
└────────────────────────────────────────────────────────────┘
```

What it drives (all computed client-side):

- **Document tree** with type icons and live status badges
  (Valid / Touched / Error), delete-on-hover
- **Add × 15 feature kinds**: box, sphere, cylinder, cone, torus;
  cut / fuse / common; sketch (a fully-constrained rectangle
  profile), pad, pocket, revolution; linear pattern, polar pattern;
  mirroring
- **Property editor**: every property in its text form — edit →
  set → recompute; per-property **expression binding** (the engine's
  quantity-typed expressions, e.g. `Box.Height * 2`)
- **Shape stats**: volume / area / triangle count per object
- **Undo / redo** (the engine's transaction history), recompute
- **`.FCStd` save/open** (drag-and-drop too), **STL / OBJ export**
- **Keyboard**: Ctrl+Z / Ctrl+Y, Ctrl+S, Delete, Escape

## Run it

The page fetches `fc_wasm.wasm` next to itself, so serve the folder
over http (browsers block `file://` fetches):

```bash
cargo build -p fc-wasm --release --target wasm32-unknown-unknown --locked
cp target/wasm32-unknown-unknown/release/fc_wasm.wasm studio/
cd studio && python3 -m http.server 8000
# → http://localhost:8000/fc-studio.html
```

Or build the **self-contained single file** — the engine embedded as
base64, no server needed at all (this is the release asset):

```bash
scripts/package-studio.sh
# → dist/fc-studio.html — double-click it, it just works
```

## The engine contract

`fc_wasm.wasm` exports a flat-memory C ABI; the studio shuttles bytes
through it. One JSON command in, one response out:

```js
const ptr = wasm.fc_alloc(bytes.length);        // allocate an input buffer
new Uint8Array(wasm.memory.buffer, ptr, n).set(bytes);
const packed = wasm.fc_command(ptr, n);          // run one command
wasm.fc_free(ptr, n);
const resultPtr = Number(BigInt(packed) >> 32n); // (ptr << 32) | len
const resultLen = Number(BigInt(packed) & 0xffffffffn);
```

The full command surface (`new` / `load` / `save` / `add` / `set` /
`set_expr` / …) is specified and tested in
[`crates/fc-wasm/src/lib.rs`](../crates/fc-wasm/src/lib.rs) — the
studio is a consumer of that contract, nothing more.

## Honest scope

A first workbench, not FreeCAD's Qt application: no dockable panels,
sketch editing is a composite rectangle preset (the solver runs inside
the engine, the interactive sketcher UI is future work), and the
viewport renders the tessellated kernel — exact B-rep geometry is the
kernel's own program (ADR-0003). See the repo
[README](../README.md#honest-limitations) for the full limitation list.
