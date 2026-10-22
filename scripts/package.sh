#!/usr/bin/env bash
# Reproducible local packaging for freecad-rs releases.
#
# Produces in dist/:
#   freecad-rs-<VER>-x86_64-linux-gnu.tar.gz
#   freecad-rs-<VER>-x86_64.AppImage   (requires appimagetool; optional)
#   freecad-rs-<VER>-amd64.deb        (requires dpkg-deb)
#   SHA256SUMS
#
# Usage: scripts/package.sh [--skip-appimage] [--skip-deb]
set -euo pipefail
cd "$(dirname "$0")/.."

VER=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -1)
BIN=target/release/freecad-rs
OUT=dist
REPO_ROOT=$(pwd)

[ -x "$BIN" ] || { echo "missing $BIN — run: cargo build --release --locked -p freecad-rs" >&2; exit 1; }

SKIP_APPIMAGE=0
SKIP_DEB=0
for arg in "$@"; do
  case "$arg" in
    --skip-appimage) SKIP_APPIMAGE=1 ;;
    --skip-deb) SKIP_DEB=1 ;;
  esac
done

rm -rf "$OUT"
mkdir -p "$OUT"

# --- tarball (always) --------------------------------------------------------
TB="$OUT/freecad-rs-$VER-x86_64-linux-gnu"
mkdir -p "$TB/docs"
cp "$BIN" "$TB/"
cp README.md LICENSE "$TB/"
cp docs/cli.md "$TB/docs/"
tar -C "$OUT" -czf "$TB.tar.gz" "freecad-rs-$VER-x86_64-linux-gnu"
echo "wrote $TB.tar.gz"

# --- .deb --------------------------------------------------------------------
if [ "$SKIP_DEB" -eq 0 ] && command -v dpkg-deb >/dev/null; then
  DEBROOT="$OUT/freecad-rs-$VER-amd64"
  mkdir -p "$DEBROOT/DEBIAN" "$DEBROOT/usr/bin" "$DEBROOT/usr/share/doc/freecad-rs" \
           "$DEBROOT/usr/share/icons/hicolor/512x512/apps"
  cp "$BIN" "$DEBROOT/usr/bin/"
  cp README.md "$DEBROOT/usr/share/doc/freecad-rs/"
  cp LICENSE "$DEBROOT/usr/share/doc/freecad-rs/copyright"
  cp assets/freecad-rs.png "$DEBROOT/usr/share/icons/hicolor/512x512/apps/freecad-rs.png"
  cat > "$DEBROOT/DEBIAN/control" <<EOF
Package: freecad-rs
Version: $VER
Section: graphics
Priority: optional
Architecture: amd64
Depends: libc6 (>= 2.29)
Maintainer: payswapdotorg
Description: FreeCAD-style parametric documents in Rust
 Clean-room Rust reimplementation of FreeCAD's core architecture:
 parametric documents, sketcher, PartDesign, mesh booleans,
 .FCStd / STL / OBJ / STEP exchange, offscreen viewer.
Homepage: https://github.com/payswapdotorg/freecad-rs
EOF
  dpkg-deb --build --root-owner-group "$DEBROOT" >/dev/null
  echo "wrote $OUT/freecad-rs-$VER-amd64.deb"
fi

# --- AppImage ----------------------------------------------------------------
if [ "$SKIP_APPIMAGE" -eq 0 ] && command -v appimagetool >/dev/null; then
  APPDIR="$OUT/freecad-rs.AppDir"
  mkdir -p "$APPDIR/usr/bin" "$APPDIR/usr/share/icons/hicolor/512x512/apps"
  cp "$BIN" "$APPDIR/usr/bin/"
  cp assets/freecad-rs.png "$APPDIR/usr/share/icons/hicolor/512x512/apps/freecad-rs.png"
  cp assets/freecad-rs.png "$APPDIR/freecad-rs.png"
  cat > "$APPDIR/freecad-rs.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=freecad-rs
GenericName=Parametric CAD CLI
Comment=FreeCAD-style parametric documents in Rust
Exec=freecad-rs
Icon=freecad-rs
Terminal=true
Categories=Graphics;Science;Engineering;
X-AppImage-Version=$VER
EOF
  printf '#!/bin/sh\nHERE="$(dirname "$(readlink -f "$0")")"\nexec "$HERE/usr/bin/freecad-rs" "$@"\n' > "$APPDIR/AppRun"
  chmod +x "$APPDIR/AppRun" "$APPDIR/usr/bin/freecad-rs"
  ( cd "$OUT" && APPIMAGE_EXTRACT_AND_RUN=1 appimagetool freecad-rs.AppDir "freecad-rs-$VER-x86_64.AppImage" >/dev/null 2>&1 \
      || APPIMAGE_EXTRACT_AND_RUN=1 appimagetool freecad-rs.AppDir "freecad-rs-$VER-x86_64.AppImage" )
  echo "wrote $OUT/freecad-rs-$VER-x86_64.AppImage"
fi

# --- checksums ---------------------------------------------------------------
( cd "$OUT" && sha256sum freecad-rs-$VER-*.tar.gz freecad-rs-$VER-*.AppImage freecad-rs-$VER-*.deb 2>/dev/null > SHA256SUMS || true )
echo "wrote $OUT/SHA256SUMS"
