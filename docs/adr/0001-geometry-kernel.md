# ADR 0001: The v1 geometry kernel is Rust-native and tessellated

- **Status:** Accepted (2026-10-21)
- **Scope:** `fc-part`, and every consumer of its geometry (`fc-io`
  shape persistence, the CLI's `stats`/exports, the PartDesign
  features, the planned viewer)
- **Related:** [`../PLAN-APPLICATION.md`](../PLAN-APPLICATION.md)
  (application milestones gated on this decision)

## Context

FreeCAD's entire geometry layer is Open CASCADE (OCCT): exact
B-rep/NURBS topology, exact Booleans, and the STEP and `.brp` file
formats. This project is a clean-room reimplementation of FreeCAD's
*architecture* — documents, typed properties, dependency-driven
recompute, expressions, sketcher, `.FCStd` persistence — not of OCCT
itself, and it must stay buildable, testable and auditable as a small
pure-Rust workspace (six crates, MIT, plain `cargo --locked` from
cold).

"Which geometry kernel?" therefore decides the project's build story,
its test determinism, and its licensing surface:

- OCCT is a multi-million-line C++ library. Binding it (e.g. via
  `opencascade-rs`) means a heavy native build for every contributor
  and every CI run, and turns "small, clean, readable core" into
  "OCCT with Rust plumbing".
- A pure-Rust exact B-rep/NURBS kernel written from scratch is a
  multi-year effort on its own — out of scope for a v1 by any honest
  estimate.
- What v1 actually needs from geometry is defined by the implemented
  features: primitives with *exact analytic* volume/area,
  deterministic tessellation for export and rendering, mesh
  Booleans, and — since the profile kernels — sketch-profile
  extrusion and revolution into watertight meshes.

## Decision

**The v1 geometry kernel is Rust-native and tessellated.**
Concretely:

1. **Zero C/C++ dependencies.** `fc-part` depends only on workspace
   crates (`fc-base`, `fc-sketcher`) and the standard library; the
   workspace builds with plain `cargo --locked` on stock toolchains.
2. **Approximation is explicit and bounded.** Curved geometry is
   tessellated under sagitta rules: segment counts satisfy
   `n = max(min_segments, ceil(sweep / (2·acos(max(0, 1 − deflection/r)))))`
   (floor 4 segments per arc, 16 per circle, safety cap 8192), so
   chord error stays within the requested deflection (0.01 mm
   default in the profile kernels). Deflection is a *parameter*, not
   a promise of exactness.
3. **Watertight by construction where the kernel builds solids.**
   Extrusion and revolution caps and walls share vertex indices
   (ear-clipped, hole-bridged caps); full-turn revolutions are
   seam-welded — no duplicated seam vertices.
4. **Determinism is a requirement.** Wire chaining, nesting
   classification and cap triangulation are first-come-first-served
   in creation order, documented, and stable — tests can and do
   assert exact triangle counts and volumes, the same determinism
   discipline the document layer has.

**Exact B-rep/NURBS via OCCT bindings (e.g. `opencascade-rs`) is a
gated future decision, not a v1 commitment.** Nothing in v1 may
assume OCCT is coming; every geometry-adjacent feature must keep
working with the mesh kernel alone.

## Consequences

**Positive:**

- Pure `cargo` build — no toolchain or native-build pain for
  contributors; CI stays fast and green on stock runners.
- Geometry results are deterministic (f64 arithmetic with explicit
  rules), so tests assert exact meshes, counts and volumes.
- The dependency tree stays tiny and MIT-licensed (see the license
  note below for the OCCT path).

**Accepted costs (honest):**

- Booleans (`Part::Cut/Fuse/Common`) operate on meshes and are
  approximate: watertight for planar/cylindrical seams; curved-surface
  seams (sphere-sphere) can leave open meshes at T-junctions.
- OCCT `.brp` payloads inside real `.FCStd` files are detected and
  skipped with a warning, not read (we write our FCBREP text format
  where FreeCAD writes `.brp`).
- No STEP support in v1 — STEP carries exact B-rep, and a mesh-only
  STEP export would be lossy in the other direction.
- Volumes and areas of *tessellated* results (booleans, profile
  solids) are mesh-true, not exact; the analytic primitives remain
  exact.

## License note

Both paths are LGPL-compatible with this project's MIT terms as
currently understood: the pure-Rust path has no third-party license
surface at all, and OCCT (LGPL-2.1 with exception, the license
FreeCAD itself uses) is compatible under the common dynamic-linking
reading. If the distribution model ever changes (static linking,
embedded or commercial product), re-verify before acting on any
trigger below. This is engineering due diligence, not legal advice.

## Triggers to revisit (explicit)

Revisit this decision — i.e. evaluate `opencascade-rs` behind an
**isolated `Shape` abstraction**, so the tessellated kernel remains
the buildable fallback — when any of these becomes a committed
requirement:

1. **STEP import/export becomes a milestone** — reading a STEP file
   into a document, or exporting a solid as STEP (this is
   [`../PLAN-APPLICATION.md`](../PLAN-APPLICATION.md) M3, and STEP is
   itself the primary trigger).
2. **Exact-geometry Boolean robustness becomes a hard requirement** —
   e.g. curved-seam Booleans must be watertight rather than
   approximate.
3. **Native `.brp`/OCCT file fidelity becomes a requirement** —
   reading real FreeCAD shape payloads instead of skipping them
   (feeds M2 of the application plan).

On a trigger: prototype behind the `Shape` abstraction first, keep the
mesh path green under `cargo build/test/clippy/fmt --workspace
--locked` in CI, and record the outcome as a new ADR superseding or
amending this one. A quiet dependency addition is not an option.

## Rejected alternatives

- **Full OCCT dependency now.** Solves exactness, STEP and `.brp`
  immediately, but the build weight and complexity are premature for
  the parametric-core milestone; the LGPL dynamics are acceptable but
  not worth invoking while nothing v1 does needs exact B-rep.
- **Pure-Rust exact B-rep from scratch.** A multi-year kernel effort —
  an honest "no" for v1.
