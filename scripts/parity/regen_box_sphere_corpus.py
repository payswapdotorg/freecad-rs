#!/usr/bin/env python3
"""Regenerate the K4-2d/3 box×sphere boolean corpus (`box_sphere_*.brp`).

Every byte is authored by real Open CASCADE (the `ocp` wheels bundle
OCCT 7.8.x — the family FreeCAD 1.0 ships). One box (30×20×5, the
corpus' own `box.brp` dimensions) meets one sphere in the six
configurations of the K4-2d/3 taxonomy:

* `corner`  — the sphere's center IS a box corner; the bite inside
              the box is an octant ball (r below all three extents);
* `edge`    — the center sits on a box edge; the bite is a quarter
              ball spanning the two adjacent faces;
* `face`    — the center sits inside a face; the bite is a half ball
              behind an interior circular hole;
* `face_offset` / `face_offset_high` — the center at depth δ
              inside, the ball crossing exactly one face: the bite
              is a small-circle cap (slice 2);
* `edge_offset` / `edge_offset_high` — the center inside at depths
              (a, b) from two ADJACENT faces, the ball crossing both
              with the section disks crossing each other's planes
              (a² + b² < r²): the bite wraps the shared edge, the
              sphere patch the lens between the two major arcs
              (slice 7 — the crossing-disk side-clipped sections);
* `inside`  — the sphere entirely inside the box (containment);
* `disjoint`— no contact;
* `engulf`  — the box entirely inside the sphere.

For the three constructive configurations the Cut / Fuse / Common
results are written as `.brp` payloads (the reader's differential
corpus) and OCCT's own volume and area recorded in
`expected_box_sphere.json` alongside the topological counts — the
hand-derived closed forms (octant/quarter/half ball shares of
`4πr³/3`, the `-k·(disk share) + (patch share)` area sums) are the
independent cross-check the Rust tests assert.

Requires: `pip install ocp`. Run from the repository root:

    python3 scripts/parity/regen_box_sphere_corpus.py
"""

import json
import math
import os
import sys

from OCP.BRepAlgoAPI import BRepAlgoAPI_Common, BRepAlgoAPI_Cut, BRepAlgoAPI_Fuse
from OCP.BRepGProp import BRepGProp
from OCP.BRepPrimAPI import BRepPrimAPI_MakeBox, BRepPrimAPI_MakeSphere
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

CONFIGS = [
    # (name, center, radius)
    ("corner", (0.0, 0.0, 0.0), 3.0),
    ("edge", (12.0, 0.0, 0.0), 3.0),
    ("face", (15.0, 10.0, 0.0), 2.0),
    ("inside", (15.0, 10.0, 2.5), 1.5),
    ("disjoint", (-10.0, -10.0, -10.0), 5.0),
    ("engulf", (15.0, 10.0, 2.5), 50.0),
    # K4-2d/3 slice 2 — the offset face stations: the center at depth
    # δ = 1.5 inside, crossing exactly one face (r = 2): the low face
    # (bottom) and the mirrored high face (top). The bite is the
    # small-circle cap: V = π(δ+r)²(2r−δ)/3, A = π(r²−δ²) + 2πr(δ+r).
    ("face_offset", (15.0, 10.0, 1.5), 2.0),
    ("face_offset_high", (15.0, 10.0, 3.5), 2.0),
    # K4-2d/3 slice 7 — the offset edge stations: the center inside at
    # depths (a, b) = (1, 1.5) from two adjacent faces (r = 2, a²+b² =
    # 3.25 < 4): the section disks cross each other's planes and the
    # bite wraps the shared edge. The low-low edge (x=0, z=0) and the
    # mirrored low-high edge (x=0, z=5).
    ("edge_offset", (1.0, 10.0, 1.5), 2.0),
    ("edge_offset_high", (1.0, 10.0, 3.5), 2.0),
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
    # The ADAPTIVE integration (eps = 1e-12): the default fixed-order
    # Gauss undershoots the two-plane section trims by ~1.5e-6 (the
    # edge_offset lens) — the authored geometry is exact, only the
    # default quadrature coarse. On the older corpus cases the two
    # agree to 8.5e-15 relative, so the switch moves nothing but the
    # last-bit noise.
    BRepGProp.VolumeProperties_s(shape, v, 1e-12, True)
    BRepGProp.SurfaceProperties_s(shape, a, 1e-12, True)
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

    for name, center, r in CONFIGS:
        sphere = BRepPrimAPI_MakeSphere(
            gp_Ax2(gp_Pnt(*center), gp_Dir(0.0, 0.0, 1.0)), r
        ).Shape()
        for op_name, algo in (
            ("cut", BRepAlgoAPI_Cut),
            ("fuse", BRepAlgoAPI_Fuse),
            ("common", BRepAlgoAPI_Common),
        ):
            bop = algo(box, sphere)
            bop.Build()
            if not bop.IsDone():
                print(f"{op_name}/{name}: BRepAlgoAPI failed", file=sys.stderr)
                sys.exit(1)
            result = bop.Shape()
            # The constructive configurations carry diagnostics (the
            # authored surface kinds); the classification ones keep the
            # plain entries.
            save(result, f"box_sphere_{op_name}_{name}", props, diagnostics=True)

    with open(os.path.join(OUT, "expected_box_sphere.json"), "w") as f:
        json.dump(
            {
                "generator": "ocp 7.8.1 (cadquery-ocp) via BRepAlgoAPI + BRepTools.Write_s",
                "box": {"dims": [L, W, H]},
                "cases": props,
            },
            f,
            indent=1,
        )
    print("expected_box_sphere.json written")


if __name__ == "__main__":
    main()
