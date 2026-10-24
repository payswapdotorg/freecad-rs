# ADR 0003: Geometry kernel strategy — pure-Rust exactness as its own program

- **Status:** Accepted (2026-10-24). Extends
  [`0001-geometry-kernel.md`](0001-geometry-kernel.md); does not revisit
  [`0002-step-milestone.md`](0002-step-milestone.md)'s pure-Rust STEP
  outcome (it is consistent with it).
- **Scope:** `fc-part`'s exactness program (`exact.rs`, `boolean.rs`),
  every doc that states what "exact" means here
  ([`../EQUIVALENCE.md`](../EQUIVALENCE.md),
  [`../VERIFICATION.md`](../VERIFICATION.md)), and the K-milestones
  now tracked in [`../ROADMAP.md`](../ROADMAP.md).
- **Related:** an external architecture assessment (received 2026-10-24;
  written against v0.1.6 — v0.1.7 had already landed) asked this
  question explicitly, alongside documentation-consistency findings
  addressed the same day.

## Context

ADR-0001 made the v1 kernel Rust-native and tessellated, and recorded
two honest judgments: a pure-Rust exact B-rep/NURBS kernel is "a
multi-year effort on its own — out of scope for a v1", and OCCT
bindings stay gated behind explicit triggers. Since then the project
has shipped, and an external architecture assessment now frames the
strategy question as a choice between two credible paths:

- **Path A — a stable geometry-kernel abstraction over OCCT bindings**
  (e.g. `opencascade-rs`). Immediate access to an established
  exact-geometry engine: exact intersections, NURBS, fillets, `.brp`
  fidelity. The costs are the ones ADR-0001 already named, now
  concrete: it compromises the clean-room Rust charter, forfeits the
  memory-safety and buildability properties the current release matrix
  demonstrates (the pure-Rust workspace ships on **five platform
  targets** — x86_64 and aarch64 Linux, x86_64 and aarch64 macOS,
  Windows MSVC — via plain `cargo --locked`; see
  [`.github/workflows/release.yml`](../../.github/workflows/release.yml)),
  and converts the engine into a wrapper: the auditable core becomes
  "OCCT with Rust plumbing".
- **Path B — pure Rust, with exact B-rep/NURBS treated as a major
  independent engineering program carrying its own milestones.** The
  kernel earns exactness the way it has earned everything else in this
  project: milestone by milestone, each gated by committed tests, each
  honestly labeled. No wrapper, no native dependency, no borrowed
  engine — and no pretending the program is small.

The assessment's question is fair, because the honest answer it was
written against (v0.1.6, tolerance-based classification) no longer
describes the tree: v0.1.7 shipped exact boolean classification, and
the exact planar seam weld is v0.1.8 (landing at time of writing).

## Decision

**Path B — remain pure Rust.** Exact B-rep/NURBS is promoted from "a
gated someday" to an explicit engineering program with its own
milestones (K1–K5 below). Three reasons, in order of weight:

1. **The project charter is a clean-room Rust reimplementation.** Zero
   C/C++ dependencies is a load-bearing property, not an aesthetic
   one: it is why a five-target release matrix builds from a cold
   `cargo --locked`, why the core stays small enough to audit, and why
   every determinism claim in
   [`../EQUIVALENCE.md`](../EQUIVALENCE.md) is reproducible by any
   contributor without a native toolchain. Path A trades that away for
   geometry we do not yet need to execute.
2. **The interop contract does not require executing OCCT.** OCCT
   `.brp` payloads inside real `.FCStd` files are preserved
   **byte-exact** under their original entry names (verified both
   directions against real FreeCAD 1.0; see
   [`../EQUIVALENCE.md`](../EQUIVALENCE.md), "preserved verbatim"), and
   FreeCAD remains the executor of exact geometry across the file
   boundary: a one-line touch-and-recompute in FreeCAD rebuilds the
   whole model natively in OCCT. The exchange is parametric truth plus
   preserved payloads — not kernel calls. A wrapper would buy
   redundancy on our side of that boundary, not interop.
3. **Progress is real, and it is testable.** v0.1.7 shipped exact
   classification: every vertex-vs-plane decision in the boolean kernel
   is a certified `f64` filter (bound `1024·ε·m³`, exponent window
   `2^-100..2^300`) falling back to an `i256` fast path (exponent
   spread ≤ 28 — every real mesh) and a 104-word sign/magnitude integer
   path for adversarial spreads (`crates/fc-part/src/exact.rs`;
   `orient3d_sign`, `crossing_parameter`). The 1e-6 blind-spot golden
   test —
   `boolean::tests::boolean_cut_face_one_millionth_inside_is_cut_exactly`
   — pins the regime where the old `PLANE_EPS = 1e-5` snapped a cut a
   full tolerance away from where it belonged. The exact seam weld,
   v0.1.8 (landing), closes planar crossings exactly:
   `weld_exact_seams` (twin weld at construction noise `2^-40`,
   T-junction interleave at the seam's own noise, needle-cap collapse,
   under a wholesale-or-revert volume bound) turns the rotated-crossing
   defect recorded in v0.1.7's honest list into a passing executable
   test —
   `boolean::tests::boolean_rotated_box_crossing_faces_matches_exact_reference`:
   cut 430.5, fuse 646.5, common 49.5 against the exact rational
   reference, watertight.

## The milestones

Stated as acceptance criteria; each lands with committed tests, and
[`../VERIFICATION.md`](../VERIFICATION.md) records the evidence. K2 is
already done and recorded as achieved — that is the point of writing
the program down: progress against it is checkable.

- **K1 — exact topology model.** Vertices, edges and faces carry their
  parametric curves and surfaces: line, circle, ellipse, plane,
  cylinder, cone, sphere, torus, B-spline. *Acceptance:* a structural
  BRep payload round-trips — parse, model, write — byte-preserving for
  payloads this project does not author, losslessly for those it does.
- **K2 — robust predicates.** *DONE (v0.1.7, recorded as achieved):*
  classification decisions in the boolean kernel are exact integer
  computations, certified at every tier; the construction-noise band is
  seven orders below the old tolerance. Evidence: the eight
  `exact::tests::*` units (scale-invariance, one-ulp-off-plane,
  extreme dynamic range) plus the golden tests cited above.
- **K3 — exact intersections and booleans on analytic surfaces.**
  Intersections of the K1 surface set computed exactly (analytic
  curves where they exist, certified algebraic placement where they do
  not), booleans composing them. *Acceptance:* differential-tested
  against a committed OCCT-generated fixture corpus with expected
  results committed alongside, in the FreeCAD 1.0 binary's own
  serialization.
- **K4 — NURBS support and general booleans.** Subsurface
  classification, trimming. *Acceptance:* NURBS operands load, classify
  and round-trip; boolean results on them are watertight and
  differential-tested as in K3.
- **K5 — fillets, chamfers, offsets.** The feature layer exactness
  FreeCAD users actually touch. *Acceptance:* same differential-test
  discipline as K3/K4.

## What we do NOT claim

- Mesh-level booleans are **not exact geometry**. Until K3/K4, curved
  operands remain deflection-bounded — a *tessellation* property, not a
  classification defect (classification has been exact since v0.1.7).
- Exactness of *volumes* remains the analytic-primitives-only property
  it was under ADR-0001 for everything tessellated.
- Nothing here retires ADR-0001's honesty rules: deflection stays a
  parameter, determinism stays a requirement, and every claim keeps a
  test or a documented "no" behind it.

## Revisit triggers (explicit)

Re-evaluate Path A — prototype behind an isolated `Shape` abstraction,
keep the tessellated path green, record the outcome as a new ADR, per
ADR-0001's escape-hatch discipline — if either of these happens:

1. **K1 or K3 stall indefinitely** — no honest, test-backed progress
   across multiple releases. The program is then not a program but a
   pose, and the trade should be re-priced openly.
2. **Upstream FreeCAD changes document or payload semantics** in ways
   that require executing OCCT to preserve (e.g. a `.FCStd` format
   change whose fidelity cannot be met by byte-preservation plus
   parametric truth). Interop is the charter; the kernel strategy
   serves it, not the reverse.

A quiet dependency addition is not an option — neither is a quiet
milestone deletion: if a K-milestone is abandoned, it is abandoned in
an ADR.

## Rejected alternatives

- **Path A now.** Buys exact intersections, NURBS and fillets
  immediately, and costs the charter, the five-target pure-Rust
  matrix, the audit surface, and the demonstrated ability to ship
  exactness incrementally. Rejected while the interop contract does
  not need it (reason 2) and while K-progress is real (reason 3).
- **Claiming mesh exactness.** Dishonest under ADR-0001's rules; the
  honest list exists precisely to prevent it.
- **Deferring the question until "later".** The assessment is right
  that a strategy unstated is a strategy unreviewable. This ADR is the
  statement.
