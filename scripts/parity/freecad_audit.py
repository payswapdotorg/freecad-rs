#!/usr/bin/env python
"""FreeCAD-side audit of the freecad-rs interop corpus.

Runs INSIDE freecadcmd — the FreeCAD 1.0 (r39109) console binary —
never under a host python:

    <squashfs-root>/AppRun freecadcmd scripts/parity/freecad_audit.py

Parameters arrive via the ENVIRONMENT (freecadcmd treats extra argv
as files to open, so argv is unusable for arguments):

    PARITY_IN_DIR   the directory of ``*.FCStd`` files to audit
    PARITY_OUT_DIR  where fixture-mode reference copies are written
    PARITY_TSV      the report file (appended, TSV)
    PARITY_KIND     ``fixture`` (phase 1: audit + saveAs references)
                    or ``roundtrip`` (phase 3: audit only)

* ``fixture`` (phase 1): every ``*.FCStd`` under ``PARITY_IN_DIR`` is
  opened, recomputed, and required to leave every object
  ``Up-to-date`` (any ``Error``/``Invalid``/``Touched`` fails the
  file). Per object one TSV row is appended to the report:

      kind \t stem \t name \t TypeId \t volume

  (``volume`` is ``repr(float)`` — round-trip safe — or empty for
  objects without a shape). The document is then ``saveAs``-ed into
  ``PARITY_OUT_DIR`` as the FreeCAD-authored *reference* copy the
  Rust side of the gate consumes.

* ``roundtrip`` (phase 3): the same open/recompute/state check over
  freecad-rs-written files; nothing is saved.

One unopenable or erroring file records a failure row and continues —
the audit is a census, not a crash. The final stdout line is

    AUDIT RESULT: <ok> ok, <fail> failed

and the process exits nonzero iff any file failed.
"""

import os
import sys

import FreeCAD  # noqa: F401 - freecadcmd preloads it; be explicit anyway.

# States that mean "FreeCAD could not bring this object to rest".
BAD_STATES = ("Error", "Invalid", "Touched")

# freecadcmd's console swallows buffered stdout when the script exits
# through SystemExit: every print is flushed, and the verdict also
# lands in a status file the shell reads (stdout markers alone are
# not trustworthy).
STATUS_PATH = os.environ.get("PARITY_STATUS", "")

# Recorded defects (space-separated stems) the gate must not score
# — set by run_parity.sh, each with a rationale there and in
# docs/EQUIVALENCE.md. Quarantined files are skipped loudly.
QUARANTINE = set(os.environ.get("PARITY_QUARANTINE", "").split())


def say(message):
    print(message, flush=True)


def audit(path, out_dir, tsv, kind):
    stem = os.path.splitext(os.path.basename(path))[0]
    try:
        doc = FreeCAD.openDocument(path)
    except Exception as exc:  # noqa: BLE001 - one bad file must not abort the census
        say("OPEN FAILED: %s: %s" % (path, exc))
        tsv.write("%s\t%s\t<open>\t<failed>\t\n" % (kind, stem))
        return False
    ok = True
    try:
        doc.recompute()
        for obj in doc.Objects:
            state = list(obj.State)
            if any(bad in state for bad in BAD_STATES):
                ok = False
                say("BAD STATE: %s.%s %s" % (stem, obj.Name, state))
            volume = ""
            try:
                volume = repr(float(obj.Shape.Volume))
            except Exception:  # noqa: BLE001 - no shape is normal, not an error
                pass
            tsv.write(
                "%s\t%s\t%s\t%s\t%s\n" % (kind, stem, obj.Name, obj.TypeId, volume)
            )
        if kind == "fixture":
            doc.saveAs(os.path.join(out_dir, stem + ".FCStd"))
    finally:
        try:
            FreeCAD.closeDocument(doc.Name)
        except Exception:  # noqa: BLE001 - best effort cleanup
            pass
    return ok


def verdict(n_ok, n_fail):
    """Announce and enforce the audit's outcome."""
    say("AUDIT RESULT: %d ok, %d failed" % (n_ok, n_fail))
    if STATUS_PATH:
        with open(STATUS_PATH, "w") as status:
            status.write("ok\n" if n_fail == 0 else "failed: %d\n" % n_fail)
    sys.exit(1 if n_fail else 0)


def main():
    in_dir = os.environ["PARITY_IN_DIR"]
    out_dir = os.environ["PARITY_OUT_DIR"]
    tsv_path = os.environ["PARITY_TSV"]
    kind = os.environ["PARITY_KIND"]
    files = sorted(
        os.path.join(in_dir, name)
        for name in os.listdir(in_dir)
        if name.lower().endswith(".fcstd")
    )
    if not files:
        say("no .FCStd files under %s" % in_dir)
        verdict(0, 1)
    n_ok = 0
    n_fail = 0
    n_quarantined = 0
    with open(tsv_path, "a") as tsv:
        for path in files:
            stem = os.path.splitext(os.path.basename(path))[0]
            if stem in QUARANTINE:
                n_quarantined += 1
                say("QUARANTINED: %s (recorded defect — skipped)" % stem)
                continue
            if audit(path, out_dir, tsv, kind):
                n_ok += 1
            else:
                n_fail += 1
    if n_quarantined:
        say("note: %d quarantined file(s) not scored" % n_quarantined)
    verdict(n_ok, n_fail)


main()
