#!/usr/bin/env python
"""Generate the K3 differential corpus: OCCT's own section curves for
the canonical analytic pairs, serialized for the Rust-side
differential test (crates/fc-part/tests/intersect_differential.rs).

The corpus pins K3a's closed-form constructions against the reference
engine: for every case the script builds the two operands as OCCT
shapes (generous finite primitives so the wanted sections are
interior), asks OCCT for the section (BRepAlgoAPI_Section, exposed as
`shape.section(other)`), drops the edges that live on the finite
trims (caps, face rims — sampled against each operand's bounds), and
records the surviving analytic curves: their type, the parameters the
curve object itself reports, and 33 sample points from OCCT's own
edge parameterization. The Rust side must cover every sample with its
closed-form curves.

Run inside the extracted AppImage (the parity gate's pinned binary):

    <squashfs-root>/AppRun freecadcmd scripts/parity/regen_k3_sections.py

Parameters ride the environment (freecadcmd treats argv as files):

    K3_OUT   where k3_sections.json is written
             (default /tmp/k3-corpus)

Honesty notes recorded in the JSON itself:
- the operands are FINITE OCCT shapes; the Rust engine intersects
  the INFINITE analytic surfaces, so the Rust answer is a superset
  (the double-cone's mirror nappe, sections outside the frustum).
  The differential is one-directional coverage: every OCCT sample must
  lie on a Rust curve.
- tangency configurations are out of the corpus (BRepAlgoAPI's
  tangent sections are not a stable contract); exact tangency is
  pinned by the acceptance suite instead.
"""

import json
import math
import os

import FreeCAD as App
import Part

OUT = os.environ.get("K3_OUT", "/tmp/k3-corpus")
MARGIN = 0.1
SAMPLES = 33


def say(message):
    print(message, flush=True)


def v(x, y, z):
    return App.Vector(x, y, z)


def unit(x, y, z):
    v_ = v(x, y, z)
    return v_.normalize()


# --- operand builders (the shape for OCCT + the analytic record) ----

def plane_face(px, py, pz, nx, ny, nz, size=60.0):
    # makePlane spans from the base corner along its own in-plane axes;
    # translate so the CENTER lands on the intended point, keeping any
    # crossing section deep inside the face regardless of frame choice.
    shape = Part.makePlane(size, size, v(px, py, pz), v(nx, ny, nz))
    center = v(px, py, pz)
    shape.translate(center - shape.CenterOfMass)
    n = v(nx, ny, nz)
    n.normalize()
    record = {"kind": "Plane", "origin": [px, py, pz],
              "normal": [n.x, n.y, n.z]}
    bounds = {"kind": "face", "center": [px, py, pz],
              "normal": [n.x, n.y, n.z], "half": size / 2.0}
    return shape, record, bounds


def sphere_sol(cx, cy, cz, r):
    shape = Part.makeSphere(r, v(cx, cy, cz))
    record = {"kind": "Sphere", "center": [cx, cy, cz], "radius": r}
    return shape, record, None


def cylinder_sol(cx, cy, cz, ax, ay, az, r, height):
    axis = v(ax, ay, az)
    axis.normalize()
    shape = Part.makeCylinder(r, height, v(cx, cy, cz), axis)
    record = {"kind": "Cylinder", "origin": [cx, cy, cz],
              "axis": [axis.x, axis.y, axis.z], "radius": r}
    mid = v(cx, cy, cz) + axis * (height / 2.0)
    bounds = {"kind": "caps", "axis": [axis.x, axis.y, axis.z],
              "mid": [mid.x, mid.y, mid.z], "half": height / 2.0}
    return shape, record, bounds


def cone_sol(cx, cy, cz, ax, ay, az, r1, r2, height):
    """A frustum: bottom radius r1 at the base point, top radius r2
    at base + height·axis. The analytic record uses the axis direction
    that keeps the semi-angle positive (OCCT's Geom_ConicalSurface
    parameterization is the same double cone)."""
    axis = v(ax, ay, az)
    axis.normalize()
    shape = Part.makeCone(r1, r2, height, v(cx, cy, cz), axis)
    if r2 >= r1:
        rec_axis = [axis.x, axis.y, axis.z]
        rec_origin = [cx, cy, cz]
        ref, semi = r1, math.atan((r2 - r1) / height)
    else:
        rec_axis = [-axis.x, -axis.y, -axis.z]
        top = v(cx, cy, cz) + axis * height
        rec_origin = [top.x, top.y, top.z]
        ref, semi = r2, math.atan((r1 - r2) / height)
    record = {"kind": "Cone", "origin": rec_origin, "axis": rec_axis,
              "semi_angle": semi, "ref_radius": ref}
    mid = v(cx, cy, cz) + axis * (height / 2.0)
    bounds = {"kind": "caps", "axis": [axis.x, axis.y, axis.z],
              "mid": [mid.x, mid.y, mid.z], "half": height / 2.0}
    return shape, record, bounds


def torus_sol(cx, cy, cz, ax, ay, az, major, minor):
    axis = v(ax, ay, az)
    axis.normalize()
    shape = Part.makeTorus(major, minor, v(cx, cy, cz), axis)
    record = {"kind": "Torus", "center": [cx, cy, cz],
              "axis": [axis.x, axis.y, axis.z],
              "major_radius": major, "minor_radius": minor}
    return shape, record, None


# --- the case table (canonical pairs, generous bounds) --------------

Z = (0.0, 0.0, 1.0)
S60 = math.radians(60.0)
S45 = math.radians(45.0)


def build_cases():
    cases = []

    def add(name, a, b):
        cases.append({"name": name, "a": a, "b": b})

    add("plane_plane_crossing",
        plane_face(0.0, 0.0, 0.0, 0.0, 0.0, 1.0),
        plane_face(3.0, 0.0, 0.0, 1.0, 1.0, 0.0))
    add("plane_sphere_secant",
        plane_face(4.0, 2.0, -1.0, 1.0, 0.0, 0.0),
        sphere_sol(1.0, 2.0, -1.0, 5.0))
    add("plane_sphere_oblique_general",
        plane_face(3.4433756729740645, -1.5566243270259355, 2.4433756729740645,
                   1.0, 1.0, 1.0),
        sphere_sol(2.0, -3.0, 1.0, 4.0))
    add("sphere_sphere_secant",
        sphere_sol(-2.0, 0.0, 0.0, 3.0),
        sphere_sol(2.0, 1.0, 0.5, 4.0))
    add("plane_cylinder_perpendicular",
        plane_face(1.0, 1.0, 3.0, 0.0, 0.0, 1.0),
        cylinder_sol(1.0, 1.0, -12.0, 0.0, 0.0, 1.0, 2.0, 30.0))
    add("plane_cylinder_parallel",
        plane_face(1.0, 0.0, 0.0, 1.0, 0.0, 0.0),
        cylinder_sol(0.0, 0.0, -15.0, 0.0, 0.0, 1.0, 2.0, 30.0))
    add("plane_cylinder_oblique",
        plane_face(0.0, 0.0, 1.0, 0.0, math.sin(S60), math.cos(S60)),
        cylinder_sol(0.0, 0.0, -20.0, 0.0, 0.0, 1.0, 1.0, 40.0))
    add("plane_cone_perpendicular",
        plane_face(0.0, 0.0, 2.0, 0.0, 0.0, 1.0),
        cone_sol(0.0, 0.0, -1.0, 0.0, 0.0, 1.0, 2.0, 4.0, 6.0))
    add("plane_cone_through_apex_generators",
        plane_face(0.0, 0.0, 4.0, 1.0, 0.0, 0.0),
        cone_sol(0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 4.0, 8.0))
    add("plane_cone_oblique_ellipse",
        plane_face(0.0, 0.0, 2.0, 0.0, math.sin(S45), math.cos(S45)),
        cone_sol(0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 6.0, 12.0))
    add("plane_torus_zcut",
        plane_face(0.0, 0.0, 0.6, 0.0, 0.0, 1.0),
        torus_sol(0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 4.0, 1.0))
    add("cylinder_cylinder_parallel",
        cylinder_sol(-1.0, 0.0, -15.0, 0.0, 0.0, 1.0, 2.0, 30.0),
        cylinder_sol(1.0, 0.0, -15.0, 0.0, 0.0, 1.0, 1.0, 30.0))
    add("cylinder_sphere_coaxial",
        cylinder_sol(0.0, 0.0, -10.0, 0.0, 0.0, 1.0, 3.0, 20.0),
        sphere_sol(0.0, 0.0, 0.0, 5.0))
    add("cylinder_cone_coaxial",
        cylinder_sol(0.0, 0.0, -6.0, 0.0, 0.0, 1.0, 2.0, 20.0),
        cone_sol(0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 3.0, 1.0, 8.0))
    add("sphere_cone_coaxial",
        cone_sol(0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 5.0, 10.0),
        sphere_sol(0.0, 0.0, 3.0, 2.5))
    add("cylinder_torus_coaxial",
        cylinder_sol(0.0, 0.0, -5.0, 0.0, 0.0, 1.0, 4.6, 10.0),
        torus_sol(0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 4.0, 1.0))
    add("torus_torus_coaxial",
        torus_sol(0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 4.0, 1.0),
        torus_sol(0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 4.0, 1.0))
    return cases


# --- interior filtering + curve serialization -------------------------

def trimmed(sample, bounds):
    """Is this point within MARGIN of the operand's finite trim?"""
    if bounds is None:
        return False
    if bounds["kind"] == "caps":
        axis = v(*bounds["axis"])
        mid = v(*bounds["mid"])
        along = (sample - mid).dot(axis)
        return abs(along) > bounds["half"] - MARGIN
    if bounds["kind"] == "face":
        normal = v(*bounds["normal"])
        center = v(*bounds["center"])
        d = sample - center
        # in-plane offset from the face center
        d = d - normal * d.dot(normal)
        return d.Length > bounds["half"] - MARGIN
    return False


def curve_record(edge):
    """Best-effort analytic parameters + samples from an OCCT edge."""
    curve = edge.Curve
    type_name = type(curve).__name__
    if type_name not in ("Line", "Circle", "Ellipse"):
        return None
    params = {}
    for attr in ("Position", "Center"):
        if hasattr(curve, attr):
            p = getattr(curve, attr)
            params[attr.lower()] = [p.x, p.y, p.z]
    for attr in ("Direction", "Axis"):
        if hasattr(curve, attr):
            d = getattr(curve, attr)
            params[attr.lower()] = [d.x, d.y, d.z]
    for attr in ("Radius", "MajorRadius", "MinorRadius"):
        if hasattr(curve, attr):
            params[attr.lower()] = float(getattr(curve, attr))
    samples = []
    f, l = edge.FirstParameter, edge.LastParameter
    for i in range(SAMPLES):
        u = f + (l - f) * i / (SAMPLES - 1.0)
        p = edge.valueAt(u)
        samples.append([round(p.x, 17), round(p.y, 17), round(p.z, 17)])
    return {"kind": type_name, "params": params, "samples": samples}


def interior_count(samples):
    """Samples strictly inside both operands' finite trims (the
    current case's bounds are installed in TRIM_BOUNDS)."""
    count = 0
    for s in samples:
        p = v(*s)
        if not trimmed(p, TRIM_BOUNDS[0]) and not trimmed(p, TRIM_BOUNDS[1]):
            count += 1
    return count


TRIM_BOUNDS = [None, None]



def run():
    say("K3 differential corpus generation (OCCT section curves)")
    try:
        say("FreeCAD %s %s %s, build %s" % (
            App.Version()[0], App.Version()[1], App.Version()[2],
            App.BuildVersionMajor))
    except Exception:
        say("FreeCAD %s" % (App.Version(),))
    cases = build_cases()
    out = []
    for case in cases:
        (shape_a, rec_a, bounds_a) = case["a"]
        (shape_b, rec_b, bounds_b) = case["b"]
        assert shape_a.isValid() and shape_b.isValid(), case["name"]
        section = shape_a.section(shape_b)
        TRIM_BOUNDS[0] = bounds_a
        TRIM_BOUNDS[1] = bounds_b
        kept, dropped = 0, 0
        curves = []
        for edge in section.Edges:
            record = curve_record(edge)
            if record is None:
                dropped += 1
                continue
            # Interior filter, majority rule: an edge counts as a
            # wanted lateral section when at least half its samples
            # sit strictly inside both operands' trims. (A finite
            # solid's lateral sections are CLIPPED at the caps, so
            # their endpoints legitimately touch the trim — only
            # edges that live ON a trim surface, like cap circles,
            # have all their samples there.)
            if interior_count(record["samples"]) * 2 < len(record["samples"]):
                dropped += 1
                continue
            curves.append(record)
            kept += 1
        out.append({
            "name": case["name"],
            "a": rec_a,
            "b": rec_b,
            "curves": curves,
            "dropped_boundary_edges": dropped,
        })
        say("  %-36s %2d analytic curves (%d trim edges dropped)"
            % (case["name"], kept, dropped))
    os.makedirs(OUT, exist_ok=True)
    path = os.path.join(OUT, "k3_sections.json")
    payload = {
        "generator": "FreeCAD 1.0.0 AppImage (pinned r39109, the parity "
                      "gate binary) via Part.Shape.section (BRepAlgoAPI)",
        "one_directional": "the Rust engine intersects the infinite "
                           "analytic surfaces; finite OCCT shapes make "
                           "the Rust answer a superset",
        "cases": out,
    }
    with open(path, "w") as f:
        json.dump(payload, f, indent=1)
    say("wrote %s (%d cases)" % (path, len(out)))


run()
