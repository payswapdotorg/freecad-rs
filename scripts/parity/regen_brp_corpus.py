#!/usr/bin/env python3
"""Regenerate the OCCT .brp corpus (`crates/fc-part/tests/fixtures/brp/`).

Every byte of the corpus is authored by real Open CASCADE (the `ocp`
wheels bundle OCCT 7.8.x — the family FreeCAD 1.0 ships), through
`BRepTools.Write_s`. `expected.json` carries OCCT's own volume and
area per shape (via `BRepGProp`), the topological counts, and — for
the located case — the world-space vertex coordinates, so the Rust
reader is differentially tested against the library that owns the
format.

Requires: `pip install ocp`. Run from the repository root:

    python3 scripts/parity/regen_brp_corpus.py
"""

import json
import math
import os
import sys

from OCP.BRepAlgoAPI import BRepAlgoAPI_Cut
from OCP.BRepBuilderAPI import BRepBuilderAPI_MakeFace
from OCP.BRepFilletAPI import BRepFilletAPI_MakeFillet
from OCP.BRepGProp import BRepGProp
from OCP.BRepPrimAPI import (
    BRepPrimAPI_MakeBox,
    BRepPrimAPI_MakeCone,
    BRepPrimAPI_MakeCylinder,
    BRepPrimAPI_MakePrism,
    BRepPrimAPI_MakeSphere,
    BRepPrimAPI_MakeTorus,
)
from OCP.BRepTools import BRepTools
from OCP.GProp import GProp_GProps
from OCP.Geom import Geom_BSplineSurface
from OCP.gp import gp_Ax1, gp_Dir, gp_Pnt, gp_Trsf, gp_Vec
from OCP.TColgp import TColgp_Array2OfPnt
from OCP.TColStd import TColStd_Array1OfInteger, TColStd_Array1OfReal
from OCP.TopAbs import TopAbs_EDGE, TopAbs_FACE, TopAbs_VERTEX
from OCP.TopExp import TopExp_Explorer
from OCP.TopLoc import TopLoc_Location
from OCP.TopoDS import TopoDS

OUT = os.path.join(
    os.path.dirname(__file__), "..", "..", "crates", "fc-part", "tests", "fixtures", "brp"
)


def count(shape, kind):
    ex = TopExp_Explorer(shape, kind)
    n = 0
    while ex.More():
        n += 1
        ex.Next()
    return n


def world_vertices(shape):
    from OCP.BRep import BRep_Tool

    ex = TopExp_Explorer(shape, TopAbs_VERTEX)
    seen = set()
    while ex.More():
        p = BRep_Tool.Pnt_s(TopoDS.Vertex_s(ex.Current()))
        seen.add((p.X(), p.Y(), p.Z()))
        ex.Next()
    return sorted(seen)


def save(shape, name, props, world_verts=False):
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
    if world_verts:
        entry["world_vertices"] = world_vertices(shape)
    props[name] = entry
    print(f"{name}: V={entry['volume']:.12g} A={entry['area']:.12g}")


def main():
    from OCP.gp import gp_Ax3, gp_Pln

    props = {}

    # --- the analytic primitives -----------------------------------------
    save(BRepPrimAPI_MakeBox(30.0, 20.0, 5.0).Shape(), "box", props)
    save(BRepPrimAPI_MakeCylinder(2.0, 10.0).Shape(), "cylinder", props)
    save(BRepPrimAPI_MakeCone(3.0, 1.0, 7.0).Shape(), "cone", props)
    save(BRepPrimAPI_MakeSphere(5.0).Shape(), "sphere", props)
    save(BRepPrimAPI_MakeTorus(8.0, 2.0).Shape(), "torus", props)

    # --- a located cylinder (the Locations section, translation+rotation)
    t1 = gp_Trsf()
    t1.SetTranslation(gp_Vec(1.0, 2.0, 3.0))
    t2 = gp_Trsf()
    t2.SetRotation(gp_Ax1(gp_Pnt(0, 0, 0), gp_Dir(0, 0, 1)), math.pi / 6)
    loc = TopLoc_Location(t2) * TopLoc_Location(t1)
    cyl = BRepPrimAPI_MakeCylinder(2.0, 10.0).Shape()
    save(cyl.Located(loc), "cylinder_located", props, world_verts=True)

    # --- an oblique cut: the elliptical rim --------------------------------
    n = math.sqrt(0.3**2 + 0.4**2 + 1.0)
    d = gp_Dir(0.3 / n, 0.4 / n, 1.0 / n)
    pl = gp_Pln(gp_Ax3(gp_Pnt(0, 0, 6.0), d))
    big = BRepBuilderAPI_MakeFace(pl, -40.0, 40.0, -40.0, 40.0).Face()
    tool = BRepPrimAPI_MakePrism(
        big, gp_Vec(d.X() * 40.0, d.Y() * 40.0, d.Z() * 40.0)
    ).Shape()
    cut = BRepAlgoAPI_Cut(BRepPrimAPI_MakeCylinder(3.0, 10.0).Shape(), tool).Shape()
    save(cut, "cyl_oblique_cut", props)

    # --- fillets: cylinder-side (quarter cylinders, ellipse rims) and
    #     torus fillets (the top rim of a cylinder) -----------------------
    box = BRepPrimAPI_MakeBox(10.0, 10.0, 3.0).Shape()
    fil = BRepFilletAPI_MakeFillet(box)
    ex = TopExp_Explorer(box, TopAbs_EDGE)
    edges = []
    while ex.More():
        edges.append(TopoDS.Edge_s(ex.Current()))
        ex.Next()
    for e in edges[18:20]:  # two top edges
        fil.Add(2.0, e)
    save(fil.Shape(), "fillet_box", props)

    cyl2 = BRepPrimAPI_MakeCylinder(5.0, 8.0).Shape()
    fil2 = BRepFilletAPI_MakeFillet(cyl2)
    ex = TopExp_Explorer(cyl2, TopAbs_EDGE)
    while ex.More():
        fil2.Add(1.5, TopoDS.Edge_s(ex.Current()))
        ex.Next()
    save(fil2.Shape(), "cyl_top_fillet", props)

    # --- the NURBS face: a (2,2)-degree B-spline patch over [0,4]² -------
    nu, nv = 3, 3
    poles = TColgp_Array2OfPnt(1, nu, 1, nv)
    P = [
        [(0, 0, 0), (2, 0, 0.5), (4, 0, 0)],
        [(0, 2, 0.5), (2, 2, 1.2), (4, 2, 0.5)],
        [(0, 4, 0), (2, 4, 0.5), (4, 4, 0)],
    ]
    for u in range(3):
        for v in range(3):
            x, y, z = P[u][v]
            poles.SetValue(u + 1, v + 1, gp_Pnt(x, y, z))
    uk = TColStd_Array1OfReal(1, 2)
    uk.SetValue(1, 0.0)
    uk.SetValue(2, 1.0)
    um = TColStd_Array1OfInteger(1, 2)
    um.SetValue(1, 3)
    um.SetValue(2, 3)
    vk = TColStd_Array1OfReal(1, 2)
    vk.SetValue(1, 0.0)
    vk.SetValue(2, 1.0)
    vm = TColStd_Array1OfInteger(1, 2)
    vm.SetValue(1, 3)
    vm.SetValue(2, 3)
    patch = Geom_BSplineSurface(poles, uk, vk, um, vm, 2, 2)
    face = BRepBuilderAPI_MakeFace(patch, 1e-6).Shape()
    save(face, "bspline_face", props)

    with open(os.path.join(OUT, "expected.json"), "w") as f:
        json.dump({"generator": "ocp 0.1.9 (OCCT 7.8.x) via BRepTools.Write_s", "cases": props}, f, indent=1)
    print("expected.json written")


if __name__ == "__main__":
    main()
