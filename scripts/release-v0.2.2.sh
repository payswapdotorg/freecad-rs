#!/bin/bash
# v0.2.2 release — run when a valid GITHUB_PAT (repo scope) is available.
#
#   GITHUB_PAT=ghp_... bash scripts/release-v0.2.2.sh
#
# Steps: push main → tag v0.2.2 → wait for CI + parity + the release
# workflow to go green → create-or-patch the release with notes → upload
# the local-only assets (AppImage, deb, the local linux tarball,
# SHA256SUMS) skipping any name CI already attached → verify the
# release → fix the v0.2.0 notes pointer (the studio is in-repo now).
#
# The release workflow (release.yml, triggered by the tag) attaches the
# per-target archives it builds plus the studio assets built from the
# tag; this script adds only what CI cannot build locally-verifiable
# copies of, and never fights CI over an asset name.
set -euo pipefail
cd "$(dirname "$0")/.."

PAT="${GITHUB_PAT:?set GITHUB_PAT to a valid token with repo scope}"
REPO="payswapdotorg/freecad-rs"
API="https://api.github.com/repos/$REPO"
AUTH=(-H "Authorization: token $PAT")
URL="https://x-access-token:$PAT@github.com/$REPO.git"

echo "== 1. push main =="
git push "$URL" main

echo "== 2. tag v0.2.2 =="
git tag -f v0.2.2 HEAD
git push "$URL" refs/tags/v0.2.2

echo "== 3. wait for CI + parity + release on the tag sha =="
SHA=$(git rev-parse HEAD)
STATE=""
for i in $(seq 1 100); do
  STATE=$(curl -s "${AUTH[@]}" "$API/actions/runs?head_sha=$SHA&per_page=50" | python3 -c "
import json, sys
runs = json.load(sys.stdin).get('workflow_runs', [])
out = {}
for r in runs:
    n = r['name']
    if n not in out or r['created_at'] > out[n]['created_at']:
        out[n] = r
need = ['CI', 'release']
have = {n: (out[n]['status'], out[n].get('conclusion')) for n in need if n in out}
parts = [f'{n}:{s}/{c}' for n, (s, c) in sorted(have.items())]
if 'parity' in out:
    parts.append(f\"parity:{out['parity']['status']}/{out['parity'].get('conclusion')}\")
print('|'.join(parts))")
  echo "  [$i] $STATE"
  if python3 -c "
import sys
m = {}
for p in '$STATE'.split('|'):
    n, _, v = p.partition(':')
    m[n] = v
sys.exit(0 if all(n in m and m[n].startswith('completed') for n in ('CI', 'release'))
           and ('parity' not in m or m['parity'].startswith('completed')) else 1)"; then
    break
  fi
  sleep 20
done
# gate on conclusions
python3 -c "
import sys
m = {}
for p in '$STATE'.split('|'):
    n, _, v = p.partition(':')
    m[n] = v
bad = [n for n in ('CI', 'release') if m.get(n) != 'completed/success']
if 'parity' in m and m['parity'] != 'completed/success':
    bad.append('parity')
if bad:
    print('workflows not green:', bad, m); sys.exit(1)
print('all required workflows green')"

echo "== 4. create-or-patch the release with notes =="
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
import json, sys, urllib.request, urllib.error
pat, notes = sys.argv[1], sys.argv[2]
auth = {"Authorization": "token " + pat, "Content-Type": "application/json"}
api = "https://api.github.com/repos/payswapdotorg/freecad-rs/releases"
body = {"tag_name": "v0.2.2", "name": "v0.2.2 — feature patterns", "body": notes}
try:
    req = urllib.request.Request(api, data=json.dumps(body).encode(), headers=auth, method="POST")
    with urllib.request.urlopen(req) as r:
        rel = json.load(r)
    print("created", rel["id"], rel["html_url"])
except urllib.error.HTTPError as e:
    if e.code != 422:
        raise
    # the release workflow already created it — patch instead
    with urllib.request.urlopen(urllib.request.Request(api + "/tags/v0.2.2")) as r:
        rel = json.load(r)
    req = urllib.request.Request(api + "/" + str(rel["id"]),
                                 data=json.dumps({"name": body["name"], "body": notes}).encode(),
                                 headers=auth, method="PATCH")
    with urllib.request.urlopen(req) as r:
        rel = json.load(r)
    print("patched", rel["id"], rel["html_url"])
PY

echo "== 5. upload the local-only assets (skip names CI already attached) =="
# SHA256SUMS covers exactly the local extras (the v0.2.0 convention);
# every CI-built artifact carries its own .sha256 sidecar.
(cd dist && sha256sum freecad-rs-0.2.2-x86_64-linux-gnu.tar.gz \
    freecad-rs-0.2.2-x86_64.AppImage \
    freecad-rs-0.2.2-amd64.deb > SHA256SUMS)
RID=$(curl -s "${AUTH[@]}" "$API/releases/tags/v0.2.2" | python3 -c "import json,sys; print(json.load(sys.stdin)['id'])")
echo "  release id: $RID"
python3 - "$PAT" "$RID" <<'PY' > /tmp/existing-assets.txt
import json, sys, urllib.request
pat, rid = sys.argv[1], sys.argv[2]
req = urllib.request.Request(
    f"https://api.github.com/repos/payswapdotorg/freecad-rs/releases/{rid}/assets",
    headers={"Authorization": "token " + pat})
for a in json.load(urllib.request.urlopen(req)):
    print(a["name"])
PY
for ASSET in freecad-rs-0.2.2-x86_64.AppImage \
             freecad-rs-0.2.2-amd64.deb \
             freecad-rs-0.2.2-x86_64-linux-gnu.tar.gz \
             SHA256SUMS \
             fc-studio.html fc_wasm.wasm; do
  if grep -qx "$ASSET" /tmp/existing-assets.txt; then
    echo "  skip $ASSET (already attached by CI)"
    continue
  fi
  echo "  uploading $ASSET"
  curl -sf "${AUTH[@]}" -X POST -H "Content-Type: application/octet-stream" \
    --data-binary "@dist/$ASSET" \
    "https://uploads.github.com/repos/$REPO/releases/$RID/assets?name=$ASSET" \
    | python3 -c "import json,sys; a=json.load(sys.stdin); print('   ', a['name'], a['size'])"
done

echo "== 6. verify the release =="
curl -s "${AUTH[@]}" "$API/releases/tags/v0.2.2" | python3 -c "
import json,sys
r=json.load(sys.stdin)
print(r['tag_name'], '| draft:', r['draft'], '| assets:', len(r['assets']))
for a in sorted(r['assets'], key=lambda x: x['name']): print(' ', a['name'], a['size'])"

echo "== 7. fix the v0.2.0 notes pointer (the studio is in-repo now) =="
python3 - "$PAT" <<'PY'
import json, sys, urllib.request, urllib.error
pat = sys.argv[1]
auth = {"Authorization": "token " + pat, "Content-Type": "application/json"}
api = "https://api.github.com/repos/payswapdotorg/freecad-rs/releases"
with urllib.request.urlopen(api + "/tags/v0.2.0") as r:
    rel = json.load(r)
fixed = rel["body"].replace(
    "(separate frontend, served from this sandbox's preview)",
    "(separate frontend — in-repo and shipped as a self-contained release asset since v0.2.2: studio/ + fc-studio.html)")
if fixed != rel["body"]:
    req = urllib.request.Request(api + "/" + str(rel["id"]),
                                 data=json.dumps({"body": fixed}).encode(),
                                 headers=auth, method="PATCH")
    with urllib.request.urlopen(req) as r:
        print("v0.2.0 notes fixed:", r.status)
else:
    print("v0.2.0 notes already fine")
PY

echo "done."
