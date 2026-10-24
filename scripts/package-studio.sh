#!/usr/bin/env bash
# Package the fc-studio browser workbench for release.
#
# Produces in dist/:
#   fc_wasm.wasm     — the browser engine (wasm32-unknown-unknown, release)
#   fc-studio.html   — the workbench with the engine embedded as base64:
#                      ONE file, no server, works from file://
#
# Usage: scripts/package-studio.sh [wasm-path] [out-dir]
# The wasm is built if missing.
set -euo pipefail
cd "$(dirname "$0")/.."

WASM=${1:-target/wasm32-unknown-unknown/release/fc_wasm.wasm}
OUT=${2:-dist}

if [ ! -f "$WASM" ]; then
  echo "building $WASM (cargo build -p fc-wasm --release --target wasm32-unknown-unknown --locked)" >&2
  cargo build -p fc-wasm --release --target wasm32-unknown-unknown --locked
fi

[ -f studio/fc-studio.html ] || { echo "missing studio/fc-studio.html" >&2; exit 1; }
mkdir -p "$OUT"
cp "$WASM" "$OUT/fc_wasm.wasm"

python3 - "$WASM" "studio/fc-studio.html" "$OUT/fc-studio.html" <<'PY'
import base64, sys

wasm_path, src, dst = sys.argv[1:4]
html = open(src, encoding="utf-8").read()
marker = "<!--fc-wasm-embed-->"
if marker not in html:
    sys.exit("studio/fc-studio.html is missing the embed marker " + marker)
b64 = base64.b64encode(open(wasm_path, "rb").read()).decode("ascii")
inject = '<script>window.__FC_WASM_B64 = "' + b64 + '";</script>'
out = html.replace(marker, inject, 1)
open(dst, "w", encoding="utf-8").write(out)
print(f"wrote {dst}: {len(out)} bytes (wasm embedded: {len(b64)} b64 chars)")
PY
echo "wrote $OUT/fc_wasm.wasm"
