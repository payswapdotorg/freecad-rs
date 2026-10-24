#!/usr/bin/env bash
# The FreeCAD acceptance gate: a pinned real FreeCAD 1.0 (r39109)
# binary exercised against the committed interop corpus, both
# directions, with strict exit codes. This is the check the plain
# Rust CI cannot be: it proves FreeCAD's own engine agrees.
#
#   FREECAD_APPDIR   (required) the extracted AppImage root — the
#                    directory containing AppRun (extract with
#                    ./FreeCAD_*.AppImage --appimage-extract; no FUSE
#                    needed). The gate always invokes freecadcmd
#                    through AppRun so PYTHONHOME etc. are set.
#   FREECAD_FIXTURES (optional) the fixture corpus; defaults to the
#                    repo's crates/fc-io/tests/fixtures.
#   PARITY_WORKDIR   (optional) scratch; defaults to /tmp/parity-work.
#
# Phases:
#   1. FreeCAD opens + recomputes every fixture, requires every
#      object Up-to-date, and writes FreeCAD-authored reference
#      copies (scripts/parity/freecad_audit.py).
#   2. freecad-rs (cargo test -p fc-io --test parity_gate, env-gated)
#      loads those references, round-trips them losslessly, and
#      compares recomputed volumes against FreeCAD's own numbers;
#      the round-tripped files are written for phase 3.
#   3. FreeCAD re-opens + recomputes the freecad-rs-written files.
#
# The audit's stdout marker ("AUDIT RESULT: N ok, M failed") is the
# authority for phases 1 and 3 (freecadcmd's exit code is checked as
# well, but the marker is checked regardless — belt and braces).
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
: "${FREECAD_APPDIR:?set FREECAD_APPDIR to the extracted AppImage root (the dir containing AppRun)}"
WORKDIR="${PARITY_WORKDIR:-/tmp/parity-work}"
FIXTURES="${FREECAD_FIXTURES:-$REPO_ROOT/crates/fc-io/tests/fixtures}"

# Recorded defects — quarantined from the gate until fixed (see
# docs/EQUIVALENCE.md "what the parity gate has caught"). All three
# first-contact finds (2026-10-24) were fixture-authoring defects, fixed
# by regenerating the fixtures through real FreeCAD
# (scripts/parity/regen_fixtures.py): the sketch's constraint set was
# redesigned DOF-consistent, the Pad's Profile now rides in FreeCAD
# 1.0's LinkSub form, and the groups are real App::DocumentObjectGroups
# with LinkList membership. The quarantine is empty and kept as the
# mechanism for the next find.
QUARANTINE="${PARITY_QUARANTINE-}"

rm -rf "$WORKDIR"
mkdir -p "$WORKDIR/refs" "$WORKDIR/roundtrips"

run_freecad() {
    "$FREECAD_APPDIR/AppRun" freecadcmd "$@"
}

# Runs one audit phase; fails the gate unless the marker line says
# zero failures (and the exit code, when nonzero, fails it too).
# Parameters ride the environment: freecadcmd treats extra argv as
# files to open, so argv cannot carry arguments.
run_audit() { # <kind> <in_dir>
    local kind="$1" in_dir="$2"
    local log="$WORKDIR/audit-$kind.log"
    local status="$WORKDIR/status-$kind.txt"
    rm -f "$status"
    set +e
    PARITY_IN_DIR="$in_dir" \
    PARITY_OUT_DIR="$WORKDIR/refs" \
    PARITY_TSV="$WORKDIR/audit.tsv" \
    PARITY_KIND="$kind" \
    PARITY_STATUS="$status" \
        run_freecad "$REPO_ROOT/scripts/parity/freecad_audit.py" \
        >"$log" 2>"$WORKDIR/audit-$kind.err"
    local rc=$?
    set -e
    if [ "$(cat "$status" 2>/dev/null)" != "ok" ]; then
        echo "phase [$kind] failed (rc=$rc, status=$(cat "$status" 2>/dev/null || echo missing)):" >&2
        grep -E "^(BAD STATE|OPEN FAILED)" "$log" >&2 || true
        tail -5 "$log" >&2 || true
        exit 1
    fi
    grep "^AUDIT RESULT" "$log" || cat "$status"
}

echo "== Phase 1: FreeCAD 1.0 opens + recomputes every fixture, writes references"
for stem in $QUARANTINE; do
    echo "   QUARANTINED: $stem (recorded defect — see docs/EQUIVALENCE.md)"
done
PARITY_QUARANTINE="$QUARANTINE" run_audit fixture "$FIXTURES"

echo "== Phase 2: freecad-rs loads the FreeCAD references (lossless round-trip + volumes)"
cd "$REPO_ROOT"
FREECAD_REF_DIR="$WORKDIR/refs" \
FREECAD_ROUNDTRIP_DIR="$WORKDIR/roundtrips" \
FREECAD_AUDIT_TSV="$WORKDIR/audit.tsv" \
    cargo test -p fc-io --test parity_gate --locked -- --nocapture

echo "== Phase 3: FreeCAD 1.0 re-opens the freecad-rs-written files"
run_audit roundtrip "$WORKDIR/roundtrips"

n_refs="$(find "$WORKDIR/refs" -iname '*.fcstd' | wc -l)"
n_rts="$(find "$WORKDIR/roundtrips" -iname '*.fcstd' | wc -l)"
n_q="$(echo $QUARANTINE | wc -w)"
echo "== PARITY GATE GREEN: $n_refs FreeCAD-authored references round-tripped; $n_rts files re-opened by real FreeCAD; $n_q quarantined (recorded defects)"
