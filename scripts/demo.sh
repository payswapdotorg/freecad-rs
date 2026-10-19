#!/usr/bin/env bash
# End-to-end demo: builds a small parametric model and renders it.
# Run from anywhere; artifacts land in examples/.
set -euo pipefail

# The CLI worker (Task FC-2-e) implements this script:
#   cargo run -p freecad-rs -- new examples/demo.fcstd --label "Demo model"
#   ... add box/sphere/cylinder/transform ...
#   ... recompute, stats, export svg, graph ...
echo "not implemented yet — owned by worker FC-2-e" >&2
exit 1
