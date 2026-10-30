#!/usr/bin/env python3
"""Regenerate the K4-2d/3 box×cylinder boolean corpus.

Every byte is authored by real Open CASCADE (the `ocp` wheels bundle
OCCT 7.8.x — the family FreeCAD 1.0 ships). One box (30×20×5, the
corpus' own `box.brp` dimensions) meets a cylinder whose axis is
bit-exactly parallel to a box axis, in the interior-cross-section
station taxonomy of the K4-2d/3 slice-4 family:

* `through`     — the axis-parallel cylinder straddles the slab (its
                  span covers `[0, 5]` strictly on both ends): the cut
                  is the genus-1 through hole (two annuli + the wall),
                  the fuse the box with the axle (two sticking-out
                  bands + caps), the common the clipped cylinder;
* `blind_low`   — the span crosses the bottom face only, ending
                  strictly inside: the cut is the blind bore from
                  below (annulus + wall + floor disk), the fuse the
                  boss below;
* `blind_high`  — the span crosses the top face only: the mirror
                  blind bore and the boss above;
* `through_y`   — the through station along the box's SECOND axis
                  (the frame contract's own permutation);
* `blind_flip`  — the blind bore with the cylinder's axis ANTI-
                  parallel to the box axis (the span read in the
                  flipped direction — the same solid as `blind_low`);
* `contained`   — the cylinder strictly inside the box: the common
                  the cylinder, the fuse the box, the cut OCCT's own
                  two-shell void (the kernel's named refusal);
* `engulf`      — the box inside the cylinder: the cut empty, the
                  fuse the cylinder, the common the box;
* `disjoint`    — the cylinder strictly below: the cut the box, the
                  common empty, the fuse OCCT's own two-shell
                  compound (the kernel's named refusal);
* `flush_through` — the span's low end EXACTLY on the bottom face
                  plane with a strict overhang above: the cut reads
                  the overlap only (the same through hole — the
                  flushed cylinder face contributes nothing), recorded
                  for the cut alone (the fuse's coplanar cap is where
                  OCCT keeps a redundant ninth face and the kernel
                  composes the minimal shell — not comparable).

For every constructive configuration the Cut / Fuse / Common results
are written as `.brp` payloads (the reader's differential corpus) and
OCCT's own volume and area recorded in `expected_box_cylinder.json`
alongside the topological counts — the hand-derived closed forms
(`V_box ∓ πr²·depth` for the bores, `V_box + πr²·overhang` for the
bosses and axles, the per-face area sums) are the independent
cross-check the Rust tests assert.

Requires: `pip install ocp`. Run from the repository root:

    python3 scripts/parity/regen_box_cylinder_corpus.py
"""

import json
import os
import sys

from OCP.BRepAlgoAPI import BRepAlgoAPI_Common, BRepAlgoAPI_Cut, BRepAlgoAPI_Fuse
from OCP.BRepGProp import BRepGProp
from OCP.BRepPrimAPI import BRepPrimAPI_MakeBox, BRepPrimAPI_MakeCylinder
from OCP.BRepTools import BRepTools
from OCP.GProp import GProp_GProps
from OCP.gp import gp_Ax2, gp_Dir, gp_Pnt
from OCP.TopAbs import TopAbs_EDGE, TopAbs_FACE, TopAbs_VERTEX
from OCP.TopExp import TopExp_Explorer

OUT = os.path.join(
    os.path.dirname(__file__), "..", "..", "crates", "fc-part", "tests", "fixtures", "brp"
)

# The corpus box: 30×20×5 at the origin (box.brp's own dimensions).
L, W, H = 30.0, 20.0, 5.0

# (name, cylinder origin, radius, height, axis, ops to record)
CONFIGS = [
    ("through", (15.0, 10.0, -2.0), 3.0, 10.0, (0.0, 0.0, 1.0), ("cut", "fuse", "common")),
    ("blind_low", (15.0, 10.0, -1.0), 2.0, 4.0, (0.0, 0.0, 1.0), ("cut", "fuse", "common")),
    ("blind_high", (15.0, 10.0, 2.0), 2.0, 5.0, (0.0, 0.0, 1.0), ("cut", "fuse", "common")),
    ("through_y", (10.0, -2.0, 2.5), 2.0, 27.0, (0.0, 1.0, 0.0), ("cut", "fuse", "common")),
    ("blind_flip", (15.0, 10.0, 3.0), 2.0, 4.0, (0.0, 0.0, -1.0), ("cut", "fuse", "common")),
    ("contained", (15.0, 10.0, 1.0), 2.0, 3.0, (0.0, 0.0, 1.0), ("cut", "fuse", "common")),
    ("engulf", (15.0, 10.0, -1.0), 25.0, 7.0, (0.0, 0.0, 1.0), ("cut", "fuse", "common")),
    ("disjoint", (15.0, 10.0, -5.0), 2.0, 2.0, (0.0, 0.0, 1.0), ("cut", "fuse", "common")),
    # The flush-through bite: the cut only — the fuse's coplanar cap
    # is the kernel's minimal-shell-vs-OCCT's-redundant-face boundary.
    ("flush_through", (15.0, 10.0, 0.0), 3.0, 8.0, (0.0, 0.0, 1.0), ("cut",)),
]


def count(shape, kind):
    ex = TopExp_Explorer(shape, kind)
    n = 0
    while ex.More():
        n += 1
        ex.Next()
    return n


def save(shape, name, props):
    os.makedirs(OUT, exist_ok=True)
    path = os.path.join(OUT, name + ".brp")
    if not BRepTools.Write_s(shape, path):
        print(f"{name}: BRepTools.Write failed", file=sys.stderr)
        sys.exit(1)
    v, a = GProp_GProps(), GProp_GProps()
    BRepGProp.VolumeProperties_s(shape, v)
    BRepGProp.SurfaceProperties_s(shape, a)
    entry = {
        "volume": v.Mass(),
        "area": a.Mass(),
        "faces": count(shape, TopAbs_FACE),
        "edges": count(shape, TopAbs_EDGE),
        "vertices": count(shape, TopAbs_VERTEX),
    }
    props[name] = entry
    print(f"{name}: V={entry['volume']:.12g} A={entry['area']:.12g} F={entry['faces']}")
    return entry


def main():
    props = {}
    box = BRepPrimAPI_MakeBox(gp_Pnt(0.0, 0.0, 0.0), L, W, H).Shape()

    for name, origin, radius, height, axis, ops in CONFIGS:
        tool = BRepPrimAPI_MakeCylinder(
            gp_Ax2(gp_Pnt(*origin), gp_Dir(*axis)), radius, height
        ).Shape()
        for op_name in ops:
            algo = {
                "cut": BRepAlgoAPI_Cut,
                "fuse": BRepAlgoAPI_Fuse,
                "common": BRepAlgoAPI_Common,
            }[op_name]
            bop = algo(box, tool)
            bop.Build()
            if not bop.IsDone():
                print(f"{op_name}/{name}: BRepAlgoAPI failed", file=sys.stderr)
                sys.exit(1)
            result = bop.Shape()
            save(result, f"box_cylinder_{op_name}_{name}", props)

    with open(os.path.join(OUT, "expected_box_cylinder.json"), "w") as f:
        json.dump(
            {
                "generator": "ocp 7.8.1 (cadquery-ocp) via BRepAlgoAPI + BRepTools.Write_s",
                "box": {"dims": [L, W, H]},
                "cases": props,
            },
            f,
            indent=1,
        )
    print("expected_box_cylinder.json written")


if __name__ == "__main__":
    main()
