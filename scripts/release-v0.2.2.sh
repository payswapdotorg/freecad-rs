#!/bin/bash
# v0.2.2 release — run when a valid GITHUB_PAT (repo scope) is available.
#
#   GITHUB_PAT=ghp_... bash scripts/release-v0.2.2.sh
#
# Steps: push main → tag v0.2.2 → wait for CI green → create the release
# with notes → upload the local assets (AppImage, deb, tarball, the
# one-file fc-studio.html, fc_wasm.wasm, SHA256SUMS) → verify the
# release → fix the v0.2.0 notes pointer (the studio is in-repo now).
set -euo pipefail
cd "$(dirname "$0")/.."

PAT="${GITHUB_PAT:?set GITHUB_PAT to a valid token with repo scope}"
REPO="payswapdotorg/freecad-rs"
API="https://api.github.com/repos/$REPO"
AUTH=(-u "payswapdotorg:$PAT")
URL="https://x-access-token:$PAT@github.com/$REPO.git"

echo "== 1. push main =="
git push "$URL" main

echo "== 2. tag v0.2.2 =="
git tag -f v0.2.2 HEAD
git push "$URL" refs/tags/v0.2.2

echo "== 3. wait for CI on the tag =="
SHA=$(git rev-parse HEAD)
for i in $(seq 1 60); do
  STATUS=$(curl -s "${AUTH[@]}" "$API/actions/runs?head_sha=$SHA" | python3 -c "
import json,sys
runs=[r for r in json.load(sys.stdin).get('workflow_runs',[]) if r['name']=='CI']
print(runs[0]['status']+'|'+str(runs[0]['conclusion']) if runs else 'none')")
  echo "  CI: $STATUS"
  [ "${STATUS%%|*}" = "completed" ] && break
  sleep 20
done
[ "${STATUS#completed|}" = "success" ] || { echo "CI did not go green: $STATUS"; exit 1; }

echo "== 4. create the release with notes =="
NOTES=$(cat <<'EOF'
## Feature patterns — the mechanical workflow

`PartDesign::LinearPattern` and `PartDesign::PolarPattern`, distilled
from FreeCAD and composed into real parts:

- **The original's tool, re-derived every recompute** — patterns are
  parametric through the whole chain (change the pocket's depth, the
  bolt circle follows).
- **Nested patterns compose multiplicatively** (FreeCAD `Transformed`
  semantics): a 2×2 hole grid from one pocket via two chained linear
  patterns.
- **FreeCAD's spacing rules**: linear span `Length/(N−1)`; a polar
  full turn spaces occurrences `360/N` apart so the last copy never
  stacks on the first.
- Copies fold into one tool union applied in a single boolean —
  set-theoretically identical to per-copy folds and ~140× faster.

New CLI verbs `pattern-linear` / `pattern-polar`, `.FCStd` round-trip,
wasm `add` kinds, and the fc-studio Add menu (the browser workbench —
one self-contained `fc-studio.html`, works from `file://`).

The acceptance test (the bracket plate: 120×80 plate, four corner
holes via nested patterns, a six-hole bolt circle via polar — every
intermediate volume asserted against engine-measured tool volumes)
was written **before** the implementation, per the external review's
acceptance-criteria-first process.

**512 tests** / 0 failed · clippy -D warnings clean · fmt clean.
EOF
)
python3 - "$PAT" "$NOTES" <<'PY'
import json, sys, urllib.request
pat, notes = sys.argv[1], sys.argv[2]
req = urllib.request.Request(
    "https://api.github.com/repos/payswapdotorg/freecad-rs/releases",
    data=json.dumps({"tag_name": "v0.2.2", "name": "v0.2.2 — feature patterns",
                     "body": notes}).encode(),
    headers={"Authorization": "Basic " + __import__("base64").b64encode(
        f"payswapdotorg:{pat}".encode()).decode(),
        "Content-Type": "application/json"},
    method="POST")
print(urllib.request.urlopen(req).read().decode()[:200])
PY

echo "== 5. upload local assets to the v0.2.2 release =="
for ASSET in freecad-rs-0.2.2-x86_64.AppImage \
             freecad-rs-0.2.2-amd64.deb \
             freecad-rs-0.2.2-x86_64-linux-gnu.tar.gz \
             fc-studio.html fc_wasm.wasm SHA256SUMS; do
  echo "  uploading $ASSET"
  curl -s "${AUTH[@]}" -X POST -H "Content-Type: application/octet-stream" \
    --data-binary "@dist/$ASSET" \
    "$API/releases/tags/v0.2.2/assets?name=$ASSET"
  echo
done

echo "== 6. verify the release =="
curl -s "${AUTH[@]}" "$API/releases/tags/v0.2.2" | python3 -c "
import json,sys
r=json.load(sys.stdin)
print(r['tag_name'], '| draft:', r['draft'], '| assets:', len(r['assets']))
for a in r['assets']: print(' ', a['name'], a['size'])"

echo "== 7. fix the v0.2.0 notes pointer (the studio is in-repo now) =="
python3 - "$PAT" <<'PY'
import json, sys, urllib.request
pat = sys.argv[1]
body = json.load(urllib.request.urlopen(urllib.request.Request(
    "https://api.github.com/repos/payswapdotorg/freecad-rs/releases/tags/v0.2.0",
    headers={})))["body"]
fixed = body.replace(
    "(separate frontend, served from this sandbox's preview)",
    "(separate frontend — in-repo and shipped as a self-contained release asset since v0.2.2: studio/ + fc-studio.html)")
if fixed != body:
    req = urllib.request.Request(
        "https://api.github.com/repos/payswapdotorg/freecad-rs/releases/tags/v0.2.0",
        data=json.dumps({"body": fixed}).encode(),
        headers={"Authorization": "Basic " + __import__("base64").b64encode(
            f"payswapdotorg:{pat}".encode()).decode(),
            "Content-Type": "application/json"},
        method="PATCH")
    print("v0.2.0 notes fixed:", urllib.request.urlopen(req).status)
else:
    print("v0.2.0 notes already fine")
PY

echo "done."
