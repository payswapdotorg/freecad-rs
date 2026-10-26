#!/usr/bin/env python
"""Generate the K3c boolean differential corpus: OCCT's own volumes
and boundary areas for the oblique plane cuts, serialized for the
Rust-side differential test (crates/fc-part/tests/measure_differential.rs).

The corpus pins the composed booleans and their closed-form measure
against the reference engine: for every canonical case the script
builds the finite primitive as an OCCT shape, builds the halfspace
tool as a generous box with one face ON the cutting plane (spanning
the +n side, laterally centered on the plane point — an order of
magnitude larger than the operand), asks OCCT for `cut` and `common`,
and records the results' `Volume()` and `Area()`.

The Rust side must compose the same cut with its exact booleans
(`cylinder_plane_boolean` / `cone_plane_boolean`) and measure the
same numbers with `measure::brep_volume` / `brep_area`.

Run inside the extracted AppImage (the parity gate's pinned binary):

    <squashfs-root>/AppRun freecadcmd scripts/parity/regen_k3c_booleans.py

Parameters ride the environment (freecadcmd treats argv as files):

    K3C_OUT  where k3c_booleans.json is written
             (default /tmp/k3c-corpus)

Honesty notes recorded in the JSON itself:
- the halfspace is proxied by a FINITE generous box; the corpus only
  carries configurations whose +n/−n pieces fit inside it (the script
  asserts the section is interior before recording);
- OCCT's `Volume()`/`Area()` on analytic B-reps is adaptive
  quadrature, not closed form — the recorded agreement tolerance is
  measured, not assumed (the Rust test pins what was observed);
- only secant configurations are recorded — the pass-throughs and
  refusals are pinned by the acceptance suite (exact expansion-sign
  classifications need no reference engine).
"""

import json
import math
import os

import FreeCAD as App
import Part

OUT = os.environ.get("K3C_OUT", "/tmp/k3c-corpus")
BOX = 60.0  # the halfspace proxy's extent — far beyond every operand


def say(message):
    print(message, flush=True)


def v(x, y, z):
    return App.Vector(x, y, z)


def unit(x, y, z):
    w = v(x, y, z)
    return w.normalize()


def halfspace_box(p, n):
    """A box occupying {n·(x − p) ∈ [0, BOX]}, laterally centered on p."""
    box = Part.makeBox(BOX, BOX, BOX)
    box.translate(v(-BOX / 2.0, -BOX / 2.0, 0.0))
    z = v(0.0, 0.0, 1.0)
    axis = z.cross(n)
    if axis.Length > 1e-12:
        angle = math.degrees(math.acos(max(-1.0, min(1.0, z.dot(n)))))
        box.rotate(v(0.0, 0.0, 0.0), axis, angle)
    box.translate(p)
    return box


def record_result(op, solid):
    if solid.isNull() or solid.Volume == 0.0:
        return {"op": op, "empty": True}
    return {
        "op": op,
        "empty": False,
        "volume": solid.Volume,
        "area": solid.Area,
    }


def run_case(name, primitive, plane_point, plane_normal):
    p = v(*plane_point)
    n = unit(*plane_normal)
    tool = halfspace_box(p, n)
    cut = primitive.cut(tool)
    common = primitive.common(tool)
    case = {
        "name": name,
        "primitive": primitive_record(name),
        "plane_point": list(plane_point),
        "plane_normal": [n.x, n.y, n.z],
        "freecad": "1.0.0 (conda AppImage r39109)",
        "results": [
            record_result("cut", cut),
            record_result("common", common),
        ],
    }
    say(f"{name}: cut V={cut.Volume:.12g} A={cut.Area:.12g} "
        f"common V={common.Volume:.12g} A={common.Area:.12g}")
    return case


# --- the canonical cases (the same numbers the acceptance suite pins) --

CASES = []


def cylinder_case(name, base, axis, radius, height, plane_point, plane_normal):
    def build():
        return Part.makeCylinder(radius, height, v(*base), v(*axis))

    def record():
        return {
            "kind": "Cylinder",
            "origin": list(base),
            "axis": list(axis),
            "radius": radius,
            "height": height,
        }

    CASES.append((name, build, record, plane_point, plane_normal))


def cone_case(name, base, axis, r1, r2, height, plane_point, plane_normal):
    def build():
        return Part.makeCone(r1, r2, height, v(*base), v(*axis))

    def record():
        return {
            "kind": "Cone",
            "origin": list(base),
            "axis": list(axis),
            "base_radius": r1,
            "top_radius": r2,
            "height": height,
        }

    CASES.append((name, build, record, plane_point, plane_normal))


# The acceptance suite's cylinder: R = 2, h = 5, base (0.5, −0.3, 1.0),
# the plane n = (0.6, 0, 0.8) through the axis point at v* = 2.5.
cylinder_case(
    "cylinder_oblique_general",
    (0.5, -0.3, 1.0), (0.0, 0.0, 1.0), 2.0, 5.0,
    (0.5, -0.3, 3.5), (0.6, 0.0, 0.8),
)

# The same cut with the normal flipped — the +n piece is the bottom,
# exercising the keep-side selection.
cylinder_case(
    "cylinder_oblique_flipped",
    (0.5, -0.3, 1.0), (0.0, 0.0, 1.0), 2.0, 5.0,
    (0.5, -0.3, 3.5), (-0.6, 0.0, -0.8),
)

# A tilted cylinder axis in general position: axis = (1, 0.5, 5)/√26.25,
# the plane n = (0.5, −0.3, 0.8) through the axis point at v* = 1.8.
cylinder_case(
    "cylinder_oblique_tilted_axis",
    (0.3, 0.2, 0.1), (0.19518001458970666, 0.09759000729485333, 0.9759000729485332),
    1.5, 4.0,
    (0.6513240262614719, 0.375662013130736, 1.8566201313073598),
    (0.5, -0.3, 0.8),
)

# The perpendicular section (the axis-meet circle).
cylinder_case(
    "cylinder_perpendicular",
    (0.5, -0.3, 1.0), (0.0, 0.0, 1.0), 2.0, 5.0,
    (0.5, -0.3, 3.0), (0.0, 0.0, 1.0),
)

# The acceptance suite's plain cone: r1 = 3, h = 4, base at the
# origin; the plane n = (0.28, 0, 0.96) through (0, 0, 2).
cone_case(
    "cone_oblique_plain",
    (0.0, 0.0, 0.0), (0.0, 0.0, 1.0), 3.0, 0.0, 4.0,
    (0.0, 0.0, 2.0), (0.28, 0.0, 0.96),
)

# The frustum on the same plane.
cone_case(
    "cone_oblique_frustum",
    (0.0, 0.0, 0.0), (0.0, 0.0, 1.0), 3.0, 1.0, 4.0,
    (0.0, 0.0, 2.0), (0.28, 0.0, 0.96),
)

# The frustum cut with the normal flipped.
cone_case(
    "cone_oblique_frustum_flipped",
    (0.0, 0.0, 0.0), (0.0, 0.0, 1.0), 3.0, 1.0, 4.0,
    (0.0, 0.0, 2.0), (-0.28, 0.0, -0.96),
)

# The perpendicular cone section.
cone_case(
    "cone_perpendicular_plain",
    (0.0, 0.0, 0.0), (0.0, 0.0, 1.0), 3.0, 0.0, 4.0,
    (0.0, 0.0, 2.0), (0.0, 0.0, 1.0),
)


def primitive_record(name):
    for (n, _build, record, _p, _nn) in CASES:
        if n == name:
            return record()
    raise KeyError(name)


def main():
    cases = []
    for (name, build, _record, plane_point, plane_normal) in CASES:
        shape = build()
        # The box (laterally centered on the plane point, spanning
        # ±BOX/2, thickness BOX along +n) must locally contain the +n
        # piece: the operand's diagonal well inside half the box.
        bb = shape.BoundBox
        diagonal = bb.DiagonalLength
        assert diagonal < BOX / 2.0, (
            f"{name}: operand diagonal {diagonal} exceeds the box margin"
        )
        cases.append(run_case(name, shape, plane_point, plane_normal))

    payload = {
        "generator": "scripts/parity/regen_k3c_booleans.py",
        "freecad": "1.0.0 (conda AppImage r39109)",
        "tool": "Part.cut / Part.common with a generous box proxying "
                "the +n halfspace",
        "note": "OCCT's Volume()/Area() on analytic B-reps is adaptive "
                "quadrature; the Rust differential pins the observed "
                "agreement (see tests/measure_differential.rs).",
        "cases": cases,
    }
    os.makedirs(OUT, exist_ok=True)
    path = os.path.join(OUT, "k3c_booleans.json")
    with open(path, "w") as fh:
        json.dump(payload, fh, indent=1, sort_keys=True)
        fh.write("\n")
    say(f"wrote {path} ({len(cases)} cases)")


main()
