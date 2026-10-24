# Verification — the capability → evidence map

What backs each claim this repo makes. Every capability in the
[README](../README.md) and [`EQUIVALENCE.md`](EQUIVALENCE.md) is
listed below with its **source-level tests** (exact names, greppable in
this tree), **where they run**, and its **FreeCAD-bidirectional
evidence** — or an honest mark that it has none. This file exists
because an external assessment (2026-10-24) demanded exactly that
distinction; it is maintained by the same discipline as the honest
list: a claim with no evidence gets marked, not made.

- **As of:** 2026-10-24, `main` = v0.1.7 (575f3b4) plus the v0.1.8
  exact-seam-weld diff in flight (the fc-part counts below include
  its formerly-`#[ignore]`d crossing test, now running).
- **The authoritative green run is CI**
  ([`.github/workflows/ci.yml`](../.github/workflows/ci.yml)):
  `cargo build` / `cargo test --workspace --locked` /
  `clippy --workspace --all-targets -- -D warnings` /
  `cargo fmt --all -- --check`, plus the GPU viewer job.

## Legend

| Mark | Meaning |
|---|---|
| ✔ | source-level test in this repo (name cited; runs in CI) |
| ◐ | verified against the real FreeCAD 1.0 binary (AppImage r39109) per [`EQUIVALENCE.md`](EQUIVALENCE.md) — manual/local today, the target of the `parity` gate |
| ✗ | documented, no committed test |

## Test counts (measured 2026-10-24)

**486 tests** across nine crates. Method: `cargo test -p <crate> --
--list` for the five crates this sandbox can build (exact: includes
unit, integration and doc-test targets); `#[test]`-function count via
grep for the four crates whose dependency builds did not fit the
sandbox disk (`ruststep` / `wgpu` / `rhai` / the CLI binary) — CI
builds and runs them, and a grep cross-check found no `#[cfg]`-gated
or doc tests those counts would miss.

| Crate | Unit | Integration | Doc-tests | Total | Method |
|---|---:|---:|---:|---:|---|
| `fc-base` | 0 | 58 (`tests/base.rs` 19, `tests/quantity.rs` 39) | 2 | **60** | listed |
| `fc-app` | 60 (`expression.rs` 54, `recompute.rs` 6) | 37 (`tests/app.rs` 16, `tests/transaction.rs` 21) | 3 | **100** | listed |
| `fc-part` | 94 (`boolean.rs` 39, `profile.rs` 32, `partdesign.rs` 15, `exact.rs` 8) | 15 (`tests/part.rs`) | 0 | **109** | listed |
| `fc-sketcher` | 74 (`solver.rs` 40, `xml.rs` 11, `constraint.rs` 8, `sketch_object.rs` 9, `geometry.rs` 6) | 0 | 1 | **75** | listed |
| `fc-io` | 20 (`fcstd.rs` 9, `mesh_export.rs` 11) | 43 (`io.rs` 12, `fixtures_corpus.rs` 11, `expressions_booleans.rs` 10, `linksub_mirroring.rs` 3, `lossless_roundtrip.rs` 3, `partdesign.rs` 2, `property_status.rs` 2) | 0 | **63** | listed |
| `fc-step` | 0 | 6 (`tests/step.rs`) | 0 | **6** | grep |
| `fc-viewer` | 16 (`camera.rs` 12, `scene.rs` 2, `pick.rs` 2) | 9 (`tests/scene_cpu.rs` 7, `tests/gpu_render.rs` 2) | 0 | **25** | grep |
| `fc-script` | 0 | 18 (`tests/api.rs`) | 0 | **18** | grep |
| `freecad-rs` (CLI) | 11 (`src/calc.rs`) | 19 (`tests/partdesign_cli.rs` 6, `tests/script_cli.rs` 5, `tests/sketch_cli.rs` 5, `tests/mirroring_cli.rs` 3) | 0 | **30** | grep |
| **Total** | | | | **486** | |

Notes, honestly: `#[ignore]`d tests — none anywhere in `crates/**`
at this writing (the one that used to exist, the rotated-crossing
defect record, runs as a passing test under the v0.1.8 weld). The two
`fc-app` tests that shell out to the CLI binary
(`cli_undo_redo_history_on_real_fcstd`,
`cli_undo_add_removes_object_and_redo_rebuilds_it`,
`tests/transaction.rs`) run in CI; locally they need the built
binary. The two `fc-viewer` `tests/gpu_render.rs` tests assert only
under `FC_VIEWER_GPU=1` — the CI viewer job sets it on lavapipe;
without it they self-skip with a message and pass trivially.

## The matrix

| # | Capability | Source-level tests (✔ = in repo) | Where they run | FreeCAD bidirectional evidence | Manual-only |
|---|---|---|---|---|---|
| 1 | Parametric document engine — objects, links, dependency-ordered recompute, cycle detection | ✔ `fc-app` `tests/app.rs`: `recompute_runs_dependencies_first`, `recompute_detects_two_object_cycle`, `recompute_execute_failure_marks_object_error`, `recompute_reports_missing_link`, `touch_propagation_recomputes_only_dependents`, `document_links_of_and_dependents_of`, `document_remove_respects_parametric_integrity`; unit `recompute.rs` (6) | CI build job | ◐ [EQUIVALENCE](EQUIVALENCE.md) "freecad-rs files → FreeCAD": a touch-and-recompute in FreeCAD rebuilds the model natively in OCCT | — |
| 2 | Typed property system | ✔ `tests/app.rs`: `property_container_set_get_and_typed_accessors`, `property_text_round_trip_scalars_and_aggregates`, `property_parse_tolerates_whitespace_and_reordering`, `property_parse_rejects_malformed_text`; `fc-io` `tests/fixtures_corpus.rs`: `property_zoo_maps_every_documented_property_type` | CI build job | ◐ property zoo fixture authored by real FreeCAD (`fixtures/property_zoo.fcstd`) | — |
| 3 | Quantity-typed expressions — parser, dimensional analysis, bindings, cycle detection | ✔ `fc-app` unit `expression.rs` (54 tests, e.g. `trig_is_degree_mode`, `mul_div_combine_units`, `sqrt_of_even_power_units`, `round_half_away_from_zero_like_freecad`); `fc-io` `tests/expressions_booleans.rs`: `expression_binds_and_recomputes`, `expression_cascades_when_source_changes`, `expression_cycle_is_rejected`, `bound_property_is_read_only`, `expressions_survive_a_save_load_round_trip` | CI build job | ◐ [EQUIVALENCE](EQUIVALENCE.md) "Format details that had to match": `<Expression path=… expression=…/>` persistence round-trips through real FreeCAD | — |
| 4 | Units & quantities (`Base::Quantity` distillate) | ✔ `fc-base` `tests/quantity.rs` (39 tests, e.g. `parse_compound_expressions`, `value_in_temperature_affine`, `parse_affine_units_rejected_in_compounds`); CLI `src/calc.rs` unit (11, e.g. `compound_units_and_precedence`) | CI build job | — (dimension semantics are FreeCAD-source-derived, not binary-verified; no gap claimed) | — |
| 5 | Undo/redo — transaction stack, snapshots, journal | ✔ `fc-app` `tests/transaction.rs` (21 tests: `bracketed_transaction_groups_changes_into_one_undo`, `undo_stack_caps_at_100_oldest_dropped`, `restore_rejects_changes_for_unknown_objects`, `cli_undo_redo_history_on_real_fcstd`, …) | CI build job (the two `cli_*` ones need the built binary — CI runs them) | ◐ [EQUIVALENCE](EQUIVALENCE.md) "freecad-rs files → FreeCAD" (journal sidecar is ours, not FreeCAD's — no parity claim) | — |
| 6 | `.FCStd` load/save — container + `Document.xml` | ✔ `fc-io` `tests/io.rs`: `save_load_round_trip_preserves_everything`, `empty_document_round_trip`, `load_rejects_broken_files`, `load_report_has_the_documented_fields`, `load_report_is_sensible_for_imperfect_archives`; `tests/fixtures_corpus.rs`: `fixture_zips_carry_the_committed_xml_sources`, `every_fixture_round_trips_to_identical_list_output`, `demo_fcstd_loads_four_objects` | CI build job | ◐ [EQUIVALENCE](EQUIVALENCE.md) "`.FCStd` interop — both directions"; the committed fixtures corpus is real-FreeCAD-authored where named `freecad_*` | — |
| 7 | Unknown-data preservation (`.brp` payloads, GUI entries, unmodeled properties, foreign objects) | ✔ `fc-io` `tests/lossless_roundtrip.rs`: `real_freecad_document_round_trips_losslessly`, `fresh_document_saves_without_preservation_noise`; `tests/io.rs`: `load_report_is_sensible_for_imperfect_archives`; `tests/fixtures_corpus.rs`: `unknown_object_type_is_reported_not_silent` (preservation logic: `fc-app` `src/preservation.rs`, exercised through the `fc-io` suites) | CI build job | ◐ [EQUIVALENCE](EQUIVALENCE.md) "preserved verbatim" — verified both directions on `fixtures/freecad_lossless_full.fcstd` (same objects, imported volume 7178.097245096173, recompute clean) | — |
| 8 | Property `status` bits round-trip | ✔ `fc-io` `tests/property_status.rs`: `property_status_bits_round_trip`, `reloaded_roundtrip_keeps_values` | CI build job | ◐ [EQUIVALENCE](EQUIVALENCE.md): `getEditorMode('Length') == ['ReadOnly']` verified on the r39109 binary; all 32 status attributes survive | — |
| 9 | `App::PropertyLinkSub` end to end | ✔ `fc-io` `tests/linksub_mirroring.rs`: `link_sub_properties_round_trip_in_freecads_own_form`; `tests/fixtures_corpus.rs`: `partdesign_body_loads_per_expectations` | CI build job | ◐ [EQUIVALENCE](EQUIVALENCE.md): the writer's open `<LinkSub count="N">` form verified against r39109's own serialization | — |
| 10 | `Part::Mirroring` plane links (datum + authored write-back) | ✔ `fc-io` `tests/linksub_mirroring.rs`: `mirror_plane_datum_resolves_and_writes_back`, `authored_mirroring_writes_freecads_property_set`; CLI `tests/mirroring_cli.rs`: `mirroring_verb_reflects_through_the_given_plane`, `link_sub_values_round_trip_through_set_and_get`, `bare_non_datum_mirror_plane_is_an_honest_error` | CI build job | ◐ [EQUIVALENCE](EQUIVALENCE.md): datum-plane resolution (position, rotation · +Z) and the "No subelement specified" error verified on r39109 | — |
| 11 | Lossless round-trip, both directions (real FreeCAD files load & re-save; our files open in FreeCAD) | ✔ `fc-io` `tests/lossless_roundtrip.rs` (3 tests), `tests/fixtures_corpus.rs` (11), `tests/io.rs` `freecad_native_constraints_*` (4) | CI build job | ◐ [EQUIVALENCE](EQUIVALENCE.md) "`.FCStd` interop — both directions" — the full matrix, run with the real r39109 binary | — |
| 12 | No-injection round-trip (a load→save adds no document property) | ✔ `fc-io` `tests/lossless_roundtrip.rs`: `round_trip_injects_no_document_property` | CI build job | ◐ [EQUIVALENCE](EQUIVALENCE.md) "A round-trip injects nothing either": our re-save keeps the identical 20-name set; FreeCAD's re-save of our file gains no `Creator` | — |
| 13 | Analytic primitives + tessellation (exact volume/area, sagitta rules, watertight meshes, FCBREP) | ✔ `fc-part` `tests/part.rs`: `box_analytic_invariants`, `sphere_tessellation_volume_converges`, `curved_primitive_meshes_are_watertight_and_outward`, `coarse_sphere_tessellation_is_watertight_and_outward`, `cone_tessellation_has_no_orphan_vertices`, `brep_round_trip_all_kinds`, `validate_rejects_invalid_shapes` | CI build job | ◐ [EQUIVALENCE](EQUIVALENCE.md) "Geometry semantics — verified equal" (corner-origin box etc. via `Shape.BoundBox`) + corpus numbers (Box 3000.000, Sphere 1436.755, … vs OCCT exact) | — |
| 14 | Booleans — exact classification (v0.1.7) | ✔ `fc-part` unit `exact.rs` (8: `orient3d_sign_scale_invariant`, `orient3d_sign_one_ulp_off_plane`, `orient3d_sign_extreme_dynamic_range`, …) + `boolean.rs`: `boolean_cut_face_one_millionth_inside_is_cut_exactly`, `boolean_rotated_interior_box_matches_exact_reference`, `boolean_steinmetz_crossed_cylinders_exact_volume`, `boolean_planar_cases_stay_bit_exact` | CI build job | ◐ [EQUIVALENCE](EQUIVALENCE.md) honest list, first bullet; exact rational reference: `scripts/exact_boolean_reference.py` | — |
| 15 | Booleans — exact planar seam weld, rotated crossings (v0.1.8, landing) | ✔ `fc-part` `boolean.rs`: `boolean_rotated_box_crossing_faces_matches_exact_reference` (cut 430.5 / fuse 646.5 / common 49.5 vs the exact rational reference, watertight — the v0.1.7 `#[ignore]`d defect record, now running) | CI build job | ◐ same rational-reference method (`scripts/exact_boolean_reference.py`) | — |
| 16 | Booleans — curved operands, deflection-bounded (watertight curved seams, near-tangent, mirrored, tangent slivers) | ✔ `fc-part` `boolean.rs`: `boolean_sphere_sphere_fuse_cut_common_watertight_and_honest`, `boolean_near_tangent_sphere_pair_partitions`, `boolean_mirrored_cone_fine_deflections_close`, `boolean_exactly_tangent_sphere_pair`, `boolean_crossed_cones_partitions`, `boolean_coaxial_opposed_cones_hourglass`, `boolean_mixed_curved_is_bit_deterministic`, … | CI build job | ◐ [EQUIVALENCE](EQUIVALENCE.md) corpus numbers (Cut 3878.8 vs OCCT 3874.3, +0.12% at 0.2 mm deflection) | — |
| 17 | Profile kernels — closed-wire extraction, extrusion, revolution | ✔ `fc-part` unit `profile.rs` (32: `extract_square_from_four_lines`, `classify_two_concentric_circles`, `extrude_square_exact_volume_and_bbox`, `revolve_annulus_full_turn_is_watertight`, `arc_tessellation_respects_deflection_and_formula`, …) | CI build job | ◐ [EQUIVALENCE](EQUIVALENCE.md) corpus: Pad 800.000 (OCCT 800.000) | — |
| 18 | PartDesign v1 — Pad / Pocket / Revolution | ✔ `fc-part` unit `partdesign.rs` (15: `pad_from_an_open_profile_fails_and_so_do_its_dependents`, `pocket_cuts_a_notch_out_of_a_box`, `revolution_of_a_half_disc_is_a_sphere`, …); `fc-io` `tests/partdesign.rs`: `partdesign_document_round_trips_with_links_and_geometry`, `replacing_sketch_content_propagates_to_the_pad`; CLI `tests/partdesign_cli.rs` (6) | CI build job | ◐ [EQUIVALENCE](EQUIVALENCE.md): a FreeCAD-authored `PartDesign::Body/Sketch/Pad` corpus file loads and recomputes to OCCT's exact values | — |
| 19 | Sketcher — 16 constraint kinds + LM solver, DOF reporting | ✔ `fc-sketcher` unit `solver.rs` (40), `constraint.rs` (8), `xml.rs` (11), `sketch_object.rs` (9); `fc-io` `tests/io.rs`: `freecad_native_constraints_all_sixteen_kinds_load`, `freecad_native_constraints_remap_through_skipped_geometry`, `our_writer_emits_freecad_native_constraint_ordinals`; `tests/fixtures_corpus.rs`: `freecad_authored_sketch_loads_all_thirteen_constraint_kinds`, `freecad_authored_tangent_symmetric_block_loads_every_constraint` | CI build job | ◐ [EQUIVALENCE](EQUIVALENCE.md): ordinals vs FreeCAD's `ConstraintType` enum verified on r39109; `fixtures/fcrs_constraints_roundtrip.fcstd` restores all 13 constraints bit-exact | — |
| 20 | STEP faceted exchange (AP203, marker in file, deflection-parameterized import) | ✔ `fc-step` `tests/step.rs`: `fixture_imports_within_deflection_derived_tolerance`, `partdesign_pad_exports_reimports_within_tolerance`, `exported_file_carries_the_faceted_marker_in_metadata`, `honest_errors_on_unsupported_content`, `freecad_whole_sphere_vertex_loop_imports`, `freecad_cone_and_torus_import` | CI build job | ◐ [EQUIVALENCE](EQUIVALENCE.md): verified earlier against the same FreeCAD 1.0 binaries (FreeCAD's own sphere/cone/torus exports are the fixtures) | — |
| 21 | Viewer — offscreen rendering, cameras, picking, error flags | ✔ `fc-viewer` `tests/scene_cpu.rs` (7: `demo_scene_loads_all_four_objects`, `ball_centroid_pixel_picks_the_nearest_object`, `error_doc_scene_flags_the_broken_object_with_geometry`, …); unit `camera.rs` (12: `world_to_camera_of_standard_views_is_svg_matrix`, `orbit_keeps_target_fixed_and_basis_orthonormal`, …), `pick.rs` (2); `tests/gpu_render.rs`: `golden_iso_frame_matches`, `error_doc_renders_without_crashing_and_shows_the_flag` — assert only under `FC_VIEWER_GPU=1` | CPU suite: CI build job; GPU pair: CI viewer job (lavapipe) | — (rendering parity is against our own SVG cameras and committed goldens, not the FreeCAD binary) | — |
| 22 | Rhai embedded scripting (`fc-script`) | ✔ `fc-script` `tests/api.rs` (18: `primitives_add_recompute_and_exact_stats`, `sandbox_operation_budget_stops_runaway_loops`, `recompute_failure_is_recorded_and_stats_fail_honestly`, `save_and_open_round_trip_exactly`, …); CLI `tests/script_cli.rs` (5: `example_script_builds_document_with_exact_stats`, `script_with_failed_recompute_exits_nonzero_with_the_error`, …) | CI build job | — (the script API is our own surface; it drives the same tested document engine) | — |
| 23 | CLI (`FreeCADCmd` distillate) — verbs, exit codes, journals | ✔ `freecad-rs` `tests/sketch_cli.rs`, `tests/mirroring_cli.rs`, `tests/partdesign_cli.rs`, `tests/script_cli.rs` (19 total, e.g. `cli_sketch_golden_path_from_new_to_export`, `cli_revolution_sweeps_a_half_disc_into_a_sphere`); unit `src/calc.rs` (11); `fc-app` `tests/transaction.rs`'s two `cli_*` tests | CI build job | ◐ via the file format (every CLI verb's output is verified by loading what it wrote in real FreeCAD — [EQUIVALENCE](EQUIVALENCE.md) both-direction sections) | — |

## What is only manual today

Rows marked ◐ cite [EQUIVALENCE.md](EQUIVALENCE.md): the FreeCAD
side of those verifications was performed by hand against the real
1.0 binary (AppImage r39109) — the fixtures and the numbers are
committed, but no CI job re-runs the FreeCAD side. That is the gap the
`parity` workflow
([.github/workflows/parity.yml](../.github/workflows/parity.yml),
landing with v0.1.8) closes: a continuous acceptance gate that runs
real FreeCAD 1.0 against this repo's interop surface, turning the ◐
column from "verified once, recorded honestly" into "verified every
push". Its exact checks are being authored concurrently with this
document; the workflow file is the source of truth once it lands.

No row in the matrix is ✗ at this writing: every capability the
README claims has at least one committed source-level test. The
two places the docs make claims *without* asserting them in tests are
not capability claims but negative statements, and they stay that
way deliberately:

- **`Part::Mirroring`'s face-subelement `MirrorPlane` links cannot
  re-resolve at mesh level** — a documented limitation (the honest
  list), recorded as prose, with the working datum-plane path and the
  honest error both tested (row 10). The limitation itself has no
  dedicated failing-test record.
- **The 5-platform release matrix** — the release workflow builds
  x86_64/aarch64 Linux, x86_64/aarch64 macOS and Windows MSVC; that
  is CI's claim to verify (and it has, on every `v*` tag), not a
  source-level test's.

## Keeping this file honest

- The per-crate counts above rot by design (every landing PR adds
  tests). The count in [README](../README.md) points here; this file
  re-states its measurement date, and CI's green run — not any number
  in prose — is the authority on pass/fail.
- If a capability loses its test or gains a manual-only claim, its row
  gets the mark it deserves (✗), in the same commit that made it so.
