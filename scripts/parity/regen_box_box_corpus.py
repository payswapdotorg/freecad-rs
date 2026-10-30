#!/usr/bin/env python3
"""Regenerate the K4-2d/3 box×box boolean corpus (`box_box_*.brp`).

Every byte is authored by real Open CASCADE (the `ocp` wheels bundle
OCCT 7.8.x — the family FreeCAD 1.0 ships). One box (30×20×5, the
corpus' own `box.brp` dimensions) meets a second, frame-parallel box
in the overlap-station taxonomy of the K4-2d/3 box×box family:

* `corner`     — the overlap box contains exactly ONE corner of the
                 base (it sticks out past the three faces meeting
                 there): the cut is the 9-face notch, the fuse the
                 12-face corner union;
* `edge`       — the overlap contains exactly one EDGE (the tool is
                 THROUGH on one axis, end-touched on two): the cut is
                 the 8-face through-notch, the fuse the octagonal
                 prism (2 octagon caps + 8 sides);
* `face`       — the overlap contains exactly one FACE (through on
                 two axes, end-touched on one): the cut is a plain
                 slab box, the fuse the stepped box with the annular
                 shoulder;
* `slot`       — through on one axis (the short one), strictly
                 interior on the other two: the cut is the genus-1
                 through-slot (holed top and bottom faces), the fuse
                 the box with the column welded through;
* `groove`     — the slot's wall-side sibling: through on the LONG
                 axis, strictly interior on the other two — the
                 genus-1 cut's rings sit on the side walls instead
                 of the caps;
* `open_groove`— through on one axis and REACHING one face with the
                 tool sticking past it: the channel breaks the top
                 face open (two bands), the walls are L-shaped, the
                 cut is genus 0 and renders;
* `edge_flush` — the edge station with the tool's low face FLUSH on
                 the base's bottom face: the cut reads the overlap
                 box only (the flushed tool face contributes nothing)
                 — the cut is recorded; the fuse would need the
                 coplanar face merge, which the kernel refuses by
                 name.

For every constructive configuration the Cut / Fuse / Common results
are written as `.brp` payloads (the reader's differential corpus) and
OCCT's own volume and area recorded in `expected_box_box.json`
alongside the topological counts — the hand-derived closed forms
(the `V_A ∓ V_O` / `V_A + V_B − V_O` volumes and the per-face area
sums over the rectilinear regions) are the independent cross-check
the Rust tests assert.

Requires: `pip install ocp`. Run from the repository root:

    python3 scripts/parity/regen_box_box_corpus.py
"""

import json
import os
import sys

from OCP.BRepAlgoAPI import BRepAlgoAPI_Common, BRepAlgoAPI_Cut, BRepAlgoAPI_Fuse
from OCP.BRepGProp import BRepGProp
from OCP.BRepPrimAPI import BRepPrimAPI_MakeBox
from OCP.BRepTools import BRepTools
from OCP.GProp import GProp_GProps
from OCP.gp import gp_Pnt
from OCP.TopAbs import TopAbs_EDGE, TopAbs_FACE, TopAbs_VERTEX
from OCP.TopExp import TopExp_Explorer

OUT = os.path.join(
    os.path.dirname(__file__), "..", "..", "crates", "fc-part", "tests", "fixtures", "brp"
)

# The corpus box: 30×20×5 at the origin (box.brp's own dimensions).
L, W, H = 30.0, 20.0, 5.0

# (name, tool corner, tool dims, ops to record)
CONFIGS = [
    ("corner", (25.0, 15.0, 2.0), (10.0, 10.0, 6.0), ("cut", "fuse", "common")),
    ("edge", (25.0, 15.0, -2.0), (10.0, 10.0, 10.0), ("cut", "fuse", "common")),
    ("face", (-5.0, -5.0, -1.0), (40.0, 30.0, 4.0), ("cut", "fuse", "common")),
    ("slot", (10.0, 5.0, -2.0), (10.0, 10.0, 10.0), ("cut", "fuse", "common")),
    ("groove", (10.0, -5.0, 2.0), (10.0, 30.0, 2.0), ("cut", "fuse", "common")),
    ("open_groove", (10.0, -5.0, 2.0), (10.0, 30.0, 5.0), ("cut", "fuse", "common")),
    # The flush edge bite: the cut only — the fuse is the kernel's
    # named coplanar-merge refusal (OCCT would merge it silently).
    ("edge_flush", (25.0, 15.0, 0.0), (10.0, 10.0, 8.0), ("cut",)),
]


def count(shape, kind):
    ex = TopExp_Explorer(shape, kind)
    n = 0
    while ex.More():
        n += 1
        ex.Next()
    return n


def surface_kinds(shape):
    """The face surface types, in exploration order (diagnostics)."""
    from OCP.BRep import BRep_Tool
    from OCP.TopoDS import TopoDS

    kinds = []
    ex = TopExp_Explorer(shape, TopAbs_FACE)
    while ex.More():
        face = TopoDS.Face_s(ex.Current())
        surf = BRep_Tool.Surface_s(face)
        kinds.append(type(surf).__name__.replace("Geom_", ""))
        ex.Next()
    return kinds


def save(shape, name, props, diagnostics=False):
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
    if diagnostics:
        entry["surface_kinds"] = surface_kinds(shape)
    props[name] = entry
    print(f"{name}: V={entry['volume']:.12g} A={entry['area']:.12g} F={entry['faces']}")
    return entry


def main():
    props = {}
    box = BRepPrimAPI_MakeBox(gp_Pnt(0.0, 0.0, 0.0), L, W, H).Shape()

    for name, corner, dims, ops in CONFIGS:
        tool = BRepPrimAPI_MakeBox(gp_Pnt(*corner), *dims).Shape()
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
            save(result, f"box_box_{op_name}_{name}", props, diagnostics=True)

    with open(os.path.join(OUT, "expected_box_box.json"), "w") as f:
        json.dump(
            {
                "generator": "ocp 7.8.1 (cadquery-ocp) via BRepAlgoAPI + BRepTools.Write_s",
                "box": {"dims": [L, W, H]},
                "cases": props,
            },
            f,
            indent=1,
        )
    print("expected_box_box.json written")


if __name__ == "__main__":
    main()
