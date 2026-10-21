# ADR 0002: The STEP milestone is opened — pure-Rust `ruststep` first, OCCT stays gated

- **Status:** Accepted (2026-10-21)
- **Scope:** STEP import/export (application-plan milestone M3), and any
  crate that touches STEP data
- **Supersedes:** nothing in
  [`0001-geometry-kernel.md`](0001-geometry-kernel.md); it **exercises**
  that ADR's trigger framework — trigger 1 has fired, and this ADR is the
  recorded outcome the framework demands
- **Related:** [`../PLAN-APPLICATION.md`](../PLAN-APPLICATION.md) (M3),
  the operator directive of 2026-10-21 committing the project to the
  full application roadmap

## Context

ADR-0001 made the geometry kernel Rust-native and tessellated, and named
explicit triggers for revisiting that decision. Trigger 1 — "STEP
import/export becomes a milestone" — has now fired: the operator
directive of 2026-10-21 commits the project to completing the
application roadmap, and that roadmap's M3 is STEP exchange.

ADR-0001's trigger clause says: *prototype behind an isolated `Shape`
abstraction, keep the mesh path green under the standard gates, and
record the outcome as a new ADR.* The clause names `opencascade-rs` as
the expected candidate because a trigger was assumed to mean "we now
need exact B-rep". This ADR records a more careful reading of what M3
actually needs, and a decision that keeps the project's core property —
**zero C/C++ dependencies, plain `cargo --locked` from cold** — intact.

## What M3 actually needs

1. **Import** — read a real STEP file into a document object whose
   geometry we can tessellate, measure and export like any other
   object. The input carries exact B-rep; our kernel consumes meshes.
   Import therefore = *parse* the B-rep (or its faceted representation)
   and *tessellate under an explicit deflection*.
2. **Export** — write a STEP file that the wider CAD ecosystem will
   accept, from our mesh solids. A mesh→STEP write is faceted B-rep:
   legal STEP (AP203/AP214 allow planar-faceted solids), not exact
   geometry. That is the honest ceiling of a tessellated kernel, and it
   must be labeled, not hidden.

Neither need requires OCCT's *geometry engine*. Import needs a STEP
*parser*; export needs a STEP *writer*. Exact Booleans, NURBS
intersections and `.brp` fidelity — the things only OCCT gives — are
not part of M3.

## Decision

1. **STEP work proceeds via `ruststep` (pure Rust) — not OCCT
   bindings.** The workspace keeps its zero-C/C++ property, its tiny
   build story, and its MIT-only license surface. `ruststep` is added
   as a workspace dependency, pinned in `Cargo.lock`, and must build
   under the standard gates (`cargo build/test/clippy/fmt --workspace
   --locked`) on the stock toolchain.
2. **Import is deflection-parameterized.** A STEP body becomes a
   `fc_part` `Mesh` tessellated under an explicit deflection (same
   sagitta discipline as the rest of the kernel). Import fidelity is
   asserted against deflection-derived tolerances, never hand-waved.
3. **Export is faceted and says so in the file.** The exported STEP
   carries the faceted nature in its metadata (product name carries a
   faceted marker), and the CLI/export docs state it. We do not
   present tessellated output as exact B-rep.
4. **The tessellated kernel remains the only kernel.** Nothing in this
   decision introduces a `Shape` abstraction flip, an OCCT prototype,
   or any native dependency. ADR-0001's remaining triggers (2: exact
   curved-seam Boolean *hard requirement*; 3: native `.brp` fidelity)
   stay armed exactly as before. M2's `.brp` skip-with-warning is
   unchanged — `ruststep` reads STEP, not OCCT `.brp`.
5. **Sanctioned fallback if `ruststep` fails the bar.** If `ruststep`
   proves unusable for the milestone's fixtures (build weight, MSRV
   above our toolchain, parser coverage gaps), the fallback is a
   clean-room minimal STEP subset reader/writer for the fixture
   class (planar + cylindrical faces, AP203 structure) — still pure
   Rust, still zero C/C++. Escalating to `opencascade-rs` remains a
   *new-ADR* decision, never a quiet dependency add.

## Consequences

**Positive:**

- M3 starts with the project's build story unchanged: six crates plus a
  script crate plus a STEP path, all `cargo --locked`.
- Import fidelity is explicit (deflection parameter, tested tolerance).
- The license surface stays MIT (verify `ruststep`'s dual
  MIT/Apache-2.0 terms at integration time and record the version).

**Accepted costs (honest):**

- STEP import is tessellated — exact-B-rep inputs lose exactness at
  import under the deflection parameter. This is the same honesty the
  booleans already practice.
- STEP export is faceted — a real CAD tool reading our file gets the
  tessellation, not the design intent. For mesh-first workflows (3D
  printing, visualization) this is exactly right; for parametric
  round-trips through other CAD tools it is a documented limitation.
- `ruststep` is a young ecosystem; parser depth on real-world files
  will be discovered, not assumed. The fallback in decision 5 exists
  for that discovery.

## Rejected alternatives

- **`opencascade-rs` now.** Solves import exactness and later `.brp`,
  but costs the zero-C/C++ property, the fast stock CI, and the small
  auditable-core story — for a milestone that needs a parser and a
  writer, not a geometry engine. It stays the documented escape hatch
  behind a future ADR.
- **Hand-writing the full STEP standard.** ISO 10303 is enormous; a
  clean-room full parser is a multi-year effort. The subset fallback in
  decision 5 is bounded to the fixture class and honestly labeled.
