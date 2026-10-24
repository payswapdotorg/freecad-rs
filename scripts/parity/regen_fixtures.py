#!/usr/bin/env python
"""Regenerate the fixture-truthfulness corpus through real FreeCAD 1.0.

The parity gate's first-contact finds (2026-10-24, docs/EQUIVALENCE.md
"What the parity gate has caught") were fixture-authoring defects, not
engine defects — three hand-authored files carried forms real FreeCAD
never writes:

  group_links.fcstd            `App::DocumentGroup` (an invented type
                               FreeCAD drops on restore) and `Group`
                               as a string list (FreeCAD 1.0 writes
                               `App::DocumentObjectGroup` with
                               `App::PropertyLinkList` membership)
  partdesign_body.fcstd         the Pad's `Profile` as
                               `App::PropertyLink` (FreeCAD 1.0's Pad
                               uses `App::PropertyLinkSub`)
  freecad_sketch_constraints.fcstd  a constraint set FreeCAD's solver
                               reports as conflicting/redundant
                               (Perpendicular+Angle+Horizontal+Vertical
                               overlap; Radius 9 + Diameter 18 on one
                               circle)

This script re-authors all three through freecadcmd so every byte is
the real program's own. Run it inside the extracted AppImage:

    <squashfs-root>/AppRun freecadcmd scripts/parity/regen_fixtures.py

Parameters ride the environment (freecadcmd treats argv as files):

    REGEN_OUT   where the three .FCStd files are written
                (default: /tmp/regen-fixtures)

Each sketch is authored DOF-consistent and asserted solved: solve() == 0
and FullyConstrained, and every document recomputes with no object left
Touched/Error/Invalid — the same bars the parity gate applies.
"""

import os
import sys

import FreeCAD as App
import Part
import Sketcher

OUT = os.environ.get("REGEN_OUT", "/tmp/regen-fixtures")
BAD_STATES = ("Error", "Invalid", "Touched")


def say(message):
    print(message, flush=True)


def check_states(doc, label):
    doc.recompute()
    bad = []
    for obj in doc.Objects:
        state = list(obj.State)
        if any(b in state for b in BAD_STATES):
            bad.append((obj.Name, state))
    assert not bad, "%s: objects left in %r" % (label, bad)
    say("   %s: all %d objects Up-to-date" % (label, len(doc.Objects)))


def check_sketch(sk, label):
    rc = sk.solve()
    assert rc == 0, "%s: solve() = %d" % (label, rc)
    assert sk.FullyConstrained, "%s: not fully constrained" % (label,)
    say("   %s: solved, fully constrained, %d geometries, %d constraints"
        % (label, len(sk.Geometry), len(sk.Constraints)))


def dump_sketch(sk, label):
    """Print the solved geometry + every constraint's authored form.

    The fixtures_corpus.rs expectations are written from this dump.
    """
    say("   --- %s geometry:" % label)
    for i, g in enumerate(sk.Geometry):
        say("       %2d %r" % (i, g))
    say("   --- %s constraints:" % label)
    for i, c in enumerate(sk.Constraints):
        say("       %2d %s First=%r FirstPos=%r Second=%r SecondPos=%r "
            "Third=%r ThirdPos=%r Value=%r"
            % (i, c.Type, c.First, c.FirstPos, c.Second, c.SecondPos,
               c.Third, c.ThirdPos, c.Value))


def check_box(doc, name, dims):
    obj = doc.addObject("Part::Box", name)
    obj.Length, obj.Width, obj.Height = dims
    return obj


# ---------------------------------------------------------------------------
# 1. group_links — nesting + a link to a grouped object, real types
# ---------------------------------------------------------------------------
def regen_group_links():
    say("== group_links")
    doc = App.newDocument("group_links")
    doc.Label = "Grouped links"
    box_a = check_box(doc, "BoxA", (3.0, 2.0, 2.0))
    leaf_box = check_box(doc, "LeafBox", (4.0, 3.0, 2.0))
    leaf_sphere = doc.addObject("Part::Sphere", "LeafSphere")
    leaf_sphere.Radius = 1.5
    outer = doc.addObject("App::DocumentObjectGroup", "Outer")
    nested = doc.addObject("App::DocumentObjectGroup", "Nested")
    outer.addObject(box_a)
    outer.addObject(nested)
    nested.addObject(leaf_box)
    nested.addObject(leaf_sphere)
    link = doc.addObject("App::Link", "LinkLeaf")
    link.LinkedObject = leaf_box
    doc.recompute()
    check_states(doc, "group_links")
    path = os.path.join(OUT, "group_links.FCStd")
    doc.saveAs(path)
    say("   wrote %s" % path)
    App.closeDocument(doc.Name)


# ---------------------------------------------------------------------------
# 2. partdesign_body — Body/Origin/Sketch/Pad, the real 1.0 forms
# ---------------------------------------------------------------------------
def regen_partdesign_body():
    say("== partdesign_body")
    doc = App.newDocument("partdesign_body")
    doc.Label = "Bracket body"
    body = doc.addObject("PartDesign::Body", "Body")
    say("   objects after Body: %s" % [o.Name for o in doc.Objects])
    assert body.Origin is not None, "PartDesign::Body created its Origin"
    sk = body.newObject("Sketcher::SketchObject", "Sketch")

    # A fully constrained 20x12 rectangle centered on the origin: the
    # canonical recipe (4 coincident + H/H/V/V + width + height +
    # position), DOF 0, no redundancy.
    V = App.Vector
    pts = [V(-10, -6, 0), V(10, -6, 0), V(10, 6, 0), V(-10, 6, 0)]
    for i in range(4):
        sk.addGeometry(Part.LineSegment(pts[i], pts[(i + 1) % 4]), False)
    for i in range(4):
        sk.addConstraint(Sketcher.Constraint("Coincident", i, 2, (i + 1) % 4, 1))
    sk.addConstraint(Sketcher.Constraint("Horizontal", 0))
    sk.addConstraint(Sketcher.Constraint("Horizontal", 2))
    sk.addConstraint(Sketcher.Constraint("Vertical", 1))
    sk.addConstraint(Sketcher.Constraint("Vertical", 3))
    sk.addConstraint(Sketcher.Constraint("Distance", 0, 1, 0, 2, 20.0))
    sk.addConstraint(Sketcher.Constraint("Distance", 1, 1, 1, 2, 12.0))
    sk.addConstraint(Sketcher.Constraint("DistanceX", -1, 1, 0, 1, -10.0))
    sk.addConstraint(Sketcher.Constraint("DistanceY", -1, 1, 0, 1, -6.0))
    check_sketch(sk, "partdesign_body/Sketch")

    pad = body.newObject("PartDesign::Pad", "Pad")
    pad.Profile = sk
    pad.Length = 6.0
    doc.recompute()
    check_states(doc, "partdesign_body")
    say("   pad volume: %r (expect 1440)" % pad.Shape.Volume)
    assert abs(pad.Shape.Volume - 1440.0) < 1e-6
    path = os.path.join(OUT, "partdesign_body.FCStd")
    doc.saveAs(path)
    say("   wrote %s" % path)
    App.closeDocument(doc.Name)


# ---------------------------------------------------------------------------
# 3. freecad_sketch_constraints — every mappable kind, DOF-consistent
# ---------------------------------------------------------------------------
def regen_sketch_constraints():
    say("== freecad_sketch_constraints")
    doc = App.newDocument("freecad_sketch_constraints")
    doc.Label = "Sketch constraint kinds"
    V = App.Vector

    # Sketch: trapezoid + two circles + a point — eleven kinds:
    # Coincident, Horizontal, Parallel, Perpendicular, Angle, Distance,
    # DistanceX, DistanceY, Radius, Diameter, PointOnObject. 20
    # constraints on 24 DOF, designed to close exactly; only the two
    # absolute-position pins reference the root point / X axis (kinds
    # duplicated elsewhere — the reader preserves root-referencing
    # constraints verbatim, so the mapping coverage stays whole).
    sk = doc.addObject("Sketcher::SketchObject", "Sketch")
    geo = [
        Part.LineSegment(V(-25, 0, 0), V(25, 0, 0)),    # 0 bottom
        Part.LineSegment(V(25, 0, 0), V(7, 40, 0)),    # 1 right
        Part.LineSegment(V(7, 40, 0), V(-25, 40, 0)),  # 2 top
        Part.LineSegment(V(-25, 40, 0), V(-25, 0, 0)), # 3 left
        Part.Circle(V(0, 20, 0), V(0, 0, 1), 6.0),    # 4
        Part.Circle(V(0, 32, 0), V(0, 0, 1), 4.0),     # 5
        Part.Point(V(-11, 40, 0)),                     # 6
    ]
    for g in geo:
        sk.addGeometry(g, False)
    cons = [
        Sketcher.Constraint("Coincident", 0, 2, 1, 1),
        Sketcher.Constraint("Coincident", 1, 2, 2, 1),
        Sketcher.Constraint("Coincident", 2, 2, 3, 1),
        Sketcher.Constraint("Coincident", 3, 2, 0, 1),
        Sketcher.Constraint("Horizontal", 0),
        Sketcher.Constraint("Parallel", 2, 0),
        Sketcher.Constraint("Perpendicular", 3, 2),
        Sketcher.Constraint("Angle", 0, 1, 1.9942294539833942),
        Sketcher.Constraint("Distance", 0, 1, 0, 2, 50.0),
        Sketcher.Constraint("DistanceX", 2, 1, 2, 2, -32.0),
        Sketcher.Constraint("PointOnObject", 0, 1, -1),
        Sketcher.Constraint("DistanceX", -1, 1, 0, 1, -25.0),
        Sketcher.Constraint("Radius", 4, 6.0),
        Sketcher.Constraint("DistanceX", 0, 1, 4, 3, 25.0),
        Sketcher.Constraint("DistanceY", 0, 1, 4, 3, 20.0),
        Sketcher.Constraint("Diameter", 5, 8.0),
        Sketcher.Constraint("DistanceX", 0, 1, 5, 3, 25.0),
        Sketcher.Constraint("DistanceY", 0, 1, 5, 3, 32.0),
        Sketcher.Constraint("PointOnObject", 6, 1, 2),
        Sketcher.Constraint("DistanceX", 2, 1, 6, 1, -18.0),
    ]
    for c in cons:
        sk.addConstraint(c)
    check_sketch(sk, "Sketch")
    dump_sketch(sk, "Sketch")

    # Sketch001: an equal-length H/V pair from the origin — the last two
    # kinds (Vertical, Equal) that have no non-redundant home in the
    # trapezoid above.
    sk2 = doc.addObject("Sketcher::SketchObject", "Sketch001")
    sk2.addGeometry(Part.LineSegment(V(0, 0, 0), V(10, 0, 0)), False)
    sk2.addGeometry(Part.LineSegment(V(0, 0, 0), V(0, 10, 0)), False)
    cons2 = [
        Sketcher.Constraint("Coincident", 0, 1, -1, 1),
        Sketcher.Constraint("Coincident", 1, 1, 0, 1),
        Sketcher.Constraint("Horizontal", 0),
        Sketcher.Constraint("Vertical", 1),
        Sketcher.Constraint("Equal", 0, 1),
        Sketcher.Constraint("Distance", 0, 1, 0, 2, 10.0),
    ]
    for c in cons2:
        sk2.addConstraint(c)
    check_sketch(sk2, "Sketch001")
    dump_sketch(sk2, "Sketch001")

    doc.recompute()
    check_states(doc, "freecad_sketch_constraints")
    path = os.path.join(OUT, "freecad_sketch_constraints.FCStd")
    doc.saveAs(path)
    say("   wrote %s" % path)
    App.closeDocument(doc.Name)


def main():
    os.makedirs(OUT, exist_ok=True)
    regen_group_links()
    regen_partdesign_body()
    regen_sketch_constraints()
    say("REGEN DONE")


main()
