#!/usr/bin/env bash
# End-to-end demo: builds a small parametric model and renders it.
# Run from anywhere; artifacts land in examples/.
set -euo pipefail

# Repo root = parent of this script's directory.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(dirname "$SCRIPT_DIR")"
cd "$ROOT"

echo "==> building freecad-rs (release)"
cargo build --release
BIN=target/release/freecad-rs

mkdir -p examples
rm -f examples/demo.fcstd examples/demo.svg examples/graph.dot

echo "==> new document"
"$BIN" new examples/demo.fcstd --label "Demo model"

echo "==> adding primitives + a parametric transform"
# NOTE: `--name` belongs to `add` itself, so it comes *before* the shape
# subcommand (clap 4 rule for mixed positional/subcommand commands).
# Scene: a slab, a ball and a cylinder resting on it, and a transformed
# copy of the ball stacked on top of it (all objects visible in the SVG).
"$BIN" add examples/demo.fcstd --name Base box --length 30 --width 20 --height 5
"$BIN" add examples/demo.fcstd --name Ball sphere --radius 4
"$BIN" add examples/demo.fcstd --name Stem cylinder --radius 1.5 --height 12
# Placements: Ball rests on the slab (slab top z=5, ball r -> center z=11),
# Stem stands on the slab beside it (cylinder h=12, center z=11).
"$BIN" set examples/demo.fcstd Ball Placement "pos=(-6,0,11);axis=(0,0,1);angle=0"
"$BIN" set examples/demo.fcstd Stem Placement "pos=(8,0,11);axis=(0,0,1);angle=0"
# TransformFeature composes onto the (canonical) source mesh, so the
# translated copy sits on the ball's top: center z = 11 + 12 = 23.
"$BIN" add examples/demo.fcstd --name Lift transform --source Ball \
    --translate=-6,0,23 --rotate-deg 25

echo "==> parametric change: Ball.Radius 4 -> 6 (touches Ball and Lift)"
"$BIN" set examples/demo.fcstd Ball Radius 6

echo "==> explicit recompute (idempotent: nothing is touched anymore)"
"$BIN" recompute examples/demo.fcstd

echo "==> document stats (analytic volume / area, tessellation counts)"
"$BIN" stats examples/demo.fcstd

echo "==> render + dependency graph"
"$BIN" export svg examples/demo.fcstd examples/demo.svg --view iso
"$BIN" graph examples/demo.fcstd examples/graph.dot

echo "done — see examples/ (demo.fcstd, demo.svg, graph.dot)"
