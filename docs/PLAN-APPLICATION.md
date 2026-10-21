# Application layer & interoperability plan

- **Status:** planning (2026-10-21) — the audit P2 response
- **Companions:** [`ROADMAP.md`](ROADMAP.md) (what the core does now),
  [`adr/0001-geometry-kernel.md`](adr/0001-geometry-kernel.md) (the
  kernel decision that gates M3 and part of M2)

This is the plan for everything *around* the parametric core: getting
geometry in front of a human, getting real FreeCAD data in and out,
exchanging with the wider CAD ecosystem, and driving documents from
scripts.

It is deliberately written as **product workstreams with explicit
acceptance gates, not finishing touches**. Each milestone is real
engineering with testable exit criteria, sequenced against the kernel
decision — which must not be quietly assumed away.

Sizes are rough: **S** ≤ a focused PR · **M** = multi-PR effort on
existing architecture · **L** = new crate/subsystem · **XL** = new
subsystem *plus* external dependency weight.

## Order & dependencies

| Milestone                            | Size   | Depends on                                                    |
| ------------------------------------ | ------ | ------------------------------------------------------------- |
| M1 — Interactive 3D viewer           | L      | the tessellation layer only; kernel-decision-independent      |
| M2 — FreeCAD native-file interop     | M      | independent; `.brp` reading is ADR-0001 trigger 3              |
| M4 — Embedded scripting              | M      | independent; runs parallel to M1/M2                          |
| M3 — STEP import/export              | XL / L | **gated on the ADR-0001 triggers** (opencascade-rs vs ruststep) |

Rough order: **M1 → M2 → (M3, M4)**. M3 cannot start before its
ADR-0001 gate fires — or is deliberately opened by a superseding ADR.
M4 can proceed whenever the core is quiet.

## M1 — Interactive 3D viewer

**Goal.** The first GUI surface: a `viewer` crate that opens a
`.FCStd`, tessellates its objects, and lets a human orbit, pan and
zoom — FreeCAD's 3D view, distilled.

**Scope.**

- New `crates/viewer`: wgpu + winit — a pure-Rust rendering stack (no
  C/C++ beyond the GPU drivers themselves), keeping the workspace's
  zero-C/C++ property intact.
- Loads a document via `fc-io`, reuses `fc-part` tessellation (the
  same deflection rules as the CLI's exports), applies placements.
- Orbit / pan / zoom camera; the four standard views (`iso`, `top`,
  `front`, `right`) matching `export svg`'s cameras.
- Flat-shaded triangles, per-object picking (click → object name);
  *viewing* is in scope, *editing* is not.

**Acceptance tests.**

1. `viewer` opens `examples/demo.fcstd` (the repo's own committed
   artifact) and renders all 4 objects — asserted by a headless
   offscreen render test running in CI on a software adapter
   (lavapipe/llvmpipe), checking exit code and rendered triangle
   count.
2. Screenshot criteria: a golden-image comparison of the offscreen
   ISO-view frame — SMOK (ScreenshotMOcKing)-style diff with a
   per-channel tolerance, since GPU rasterizers differ — with an
   intentional regeneration path (`--bless`-style) so the golden file
   is never hand-edited.
3. Camera math is unit-tested: orbit keeps the target fixed, zoom is
   exponential in wheel ticks, pan translates the target parallel to
   the screen plane.
4. A document containing a failed recompute (an `Error` object) still
   opens, with the failing object visibly flagged — no crash on
   half-computed documents.

**Rough size.** L — a new crate and a frame loop, but no new geometry:
it consumes the existing tessellation layer.

## M2 — FreeCAD native-file interop

**Goal.** Real FreeCAD files open *more* than today: more object
types read from real `Document.xml`, and an explicit, documented
policy for OCCT `.brp` shape payloads.

**Scope.**

- Broaden `fc-io`'s reader over real-world `Document.xml` variety:
  more property types, more object types (`App::Part` containers,
  `PartDesign::Body`, origin planes/axes skeletons, groups, links).
- `.brp` payloads: either read them via gated bindings — only if
  ADR-0001 trigger 3 has fired — or keep the documented skip with a
  sharper warning. The choice is recorded here, not made in silence.
- A fixtures corpus of small real FreeCAD-saved files under
  `tests/fixtures/` (files we are legally clear to commit).

**Acceptance tests.**

1. Round-trip: every fixture loads (no panics, no silent drops), its
   readable parameters are extracted, the document re-saves, and the
   re-saved file loads back with identical `list` output.
2. Per object type in the corpus: loaded parameters asserted against
   a recorded expectation file (name/type/status plus selected
   properties).
3. `.brp` handling is asserted, not just logged: skip-with-warning
   fixtures produce the documented warning and a correct
   parameters-only object; if bindings are ever enabled, the same
   fixtures assert real geometry (volume within a stated tolerance).

**Rough size.** M — reader breadth plus a fixtures corpus; no kernel
changes unless the `.brp` gate fires.

## M3 — STEP import/export

**Goal.** Exchange solids with the wider CAD ecosystem through STEP
(AP203/AP214) — the format real CAD tools actually accept.

**Scope** — deliberately dependent on the kernel decision:

- If an ADR-0001 trigger has fired (STEP is itself trigger 1):
  evaluate `opencascade-rs` behind the isolated `Shape` abstraction;
  the tessellated kernel remains the fallback, and the mesh path
  stays green under `cargo build/test/clippy/fmt --workspace --locked`.
- Otherwise evaluate `ruststep` (pure Rust, exact-B-rep parsing, young
  ecosystem) for import first — with export quality honestly
  assessed: mesh→STEP writes *faceted* B-rep, which is legal STEP but
  not exact, and must be labeled as such in the export documentation.

**Acceptance tests.**

1. Import: a known, committed STEP fixture (a box with a cylindrical
   hole) loads as a document object whose mesh volume is within a
   stated tolerance of the fixture's recorded exact volume — tolerance
   derived from the import deflection, not hand-waved.
2. Export: a Pad or Revolution solid (the PartDesign features) from
   the repo's own examples exports as STEP, re-imports (by us, or by
   a second tool in a manual gate), and the volume round-trips within
   the same deflection-derived tolerance.
3. The faceted-vs-exact nature of an export is asserted in the file's
   metadata (e.g. product name or a header comment), not merely
   documented.

**Rough size.** XL with OCCT bindings (build weight plus the `Shape`
abstraction); L with ruststep (import-first, export possibly
deferred).

## M4 — Embedded scripting

**Goal.** Drive the document model from a script — FreeCAD's
Python-console equivalent, minus Python's build weight by default.

**Scope.**

- First: **rhai** — pure Rust, `--locked`-friendly, sandboxed by
  design — as the embedded language, with a small document API:
  create/add objects, set properties, read computed results, run
  recompute, bind expressions.
- Optional cargo feature `pyo3` (clearly marked, not default) for a
  real-Python surface where embedding a CPython is acceptable.
  Enabling it must not degrade the default build in any way.

**Acceptance tests.**

1. Script-driven document build in CI: a committed script creates a
   document with primitives, a bound expression and a transform,
   recomputes, and asserts the resulting `stats` output (exact
   expected volume/area lines).
2. Scripting failure semantics mirror the CLI's: a script that
   triggers a failed recompute surfaces the `Error` object honestly
   and exits non-zero.
3. The default build (no `pyo3` feature) keeps
   `cargo build/test/clippy/fmt --workspace --locked` green — the
   optional feature is proven additive, never invasive.

**Rough size.** M for the rhai path; +M for the optional pyo3
feature.

## What this plan does not promise

- No dates, and no crew-of-one heroics: every milestone above is sized
  so it can be honestly started or honestly deferred.
- No GUI *workbench* — sketch-editing UIs, tree views, dockable
  anything. M1 is a viewer; a workbench is a much larger product
  decision that comes after it, if at all.
- No kernel flip by stealth: M3, and M2's `.brp` reading, go through
  the ADR-0001 triggers or through a deliberate superseding ADR —
  never through a quiet dependency addition.
