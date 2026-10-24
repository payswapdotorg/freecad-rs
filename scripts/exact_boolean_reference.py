#!/usr/bin/env python3
"""Exact-rational reference for the boolean-exactness golden test.

Computes V(A ∩ B) — and with it V(A − B) = V(A) − V(A ∩ B) — for two
convex polyhedra, in exact rational arithmetic, from the *actual f64
coordinates the Rust test builds* (the same expressions evaluated in
Python's f64, which is IEEE-754 binary64 exactly like Rust's).

Method: plane triples of the (up to 12) halfspaces → candidate vertices
by exact 3x3 Cramer → keep those satisfying every halfspace → group the
kept vertices per face plane → order each face by exact 2D convex hull →
fan-triangulate → signed volume via the divergence sum
Σ (a×b + b×c + c×a)·(1/6) (exact rationals throughout).

The printed value is the mathematical truth of the actual inputs, to
all digits — the reference the Rust test asserts against.
"""

from fractions import Fraction
from itertools import combinations

F = Fraction


def v(x, y, z):
    return (F(x), F(y), F(z))


def cross(a, b):
    return (
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    )


def sub(a, b):
    return (a[0] - b[0], a[1] - b[1], a[2] - b[2])


def dot(a, b):
    return a[0] * b[0] + a[1] * b[1] + a[2] * b[2]


def det3(rows):
    (a, b, c) = rows
    return (
        a[0] * (b[1] * c[2] - b[2] * c[1])
        - a[1] * (b[0] * c[2] - b[2] * c[0])
        + a[2] * (b[0] * c[1] - b[1] * c[0])
    )


def solve3(rows, rhs):
    d = det3(rows)
    assert d != 0
    out = []
    for col in range(3):
        m = [list(r) for r in rows]
        for i in range(3):
            m[i][col] = rhs[i]
        out.append(det3(m) / d)
    return tuple(out)


def halfspace(a, b, c):
    """Outward-oriented (point, normal) so that inside is dot(n, x-p) <= 0."""
    n = cross(sub(b, a), sub(c, a))
    assert n != (0, 0, 0)
    return (a, n)


def faces_of_box(half):
    """The six outward halfspaces of a box [x0,x1]x[y0,y1]x[z0,z1]."""
    (x0, x1, y0, y1, z0, z1) = half
    return [
        ((F(x0), F(y0), F(z0)), (F(-1), F(0), F(0))),
        ((F(x1), F(y0), F(z0)), (F(1), F(0), F(0))),
        ((F(x0), F(y0), F(z0)), (F(0), F(-1), F(0))),
        ((F(x0), F(y1), F(z0)), (F(0), F(1), F(0))),
        ((F(x0), F(y0), F(z0)), (F(0), F(0), F(-1))),
        ((F(x0), F(y0), F(z1)), (F(0), F(0), F(1))),
    ]


def faces_from_triangles(corners, tris):
    """Outward halfspaces from a triangle mesh (point on plane + normal).

    `tris` are CCW-from-outside triangles (indices into corners)."""
    out = []
    for (i, j, k) in tris:
        p = corners[i]
        n = cross(sub(corners[j], p), sub(corners[k], p))
        assert n != (0, 0, 0)
        out.append((p, n))
    return out


def intersect_volume(faces_a, faces_b):
    faces = canonicalize_faces(faces_a + faces_b)
    verts = []
    for (f1, f2, f3) in combinations(range(len(faces)), 3):
        rows = [faces[i][1] for i in (f1, f2, f3)]
        rhs = [dot(faces[i][1], faces[i][0]) for i in (f1, f2, f3)]
        if det3(rows) == 0:
            continue
        x = solve3(rows, rhs)
        if all(dot(n, sub(x, p)) <= 0 for (p, n) in faces):
            verts.append(x)
    # Deduplicate exact-equal vertices.
    verts = list(dict.fromkeys(verts))
    if not verts:
        return F(0)
    # Per-face polygons: the vertices lying on that face, hull-ordered.
    total = F(0)
    for (p, n) in faces:
        on_face = [x for x in verts if dot(n, sub(x, p)) == 0]
        if len(on_face) < 3:
            continue
        poly = hull_order(on_face, n)
        if len(poly) < 3:
            continue
        # Fan triangulation, divergence sum term.
        for t in range(1, len(poly) - 1):
            (a, b, c) = (poly[0], poly[t], poly[t + 1])
            total += dot(cross(a, b), c) / F(6)
    return total


def canonicalize_faces(faces):
    """Drop exact-duplicate halfspaces (same plane, same orientation) —
    a triangle soup describes each planar face several times."""
    out = []
    for (p, n) in faces:
        duplicate = False
        for (q, m) in out:
            same_direction = (
                n[0] * m[1] - n[1] * m[0] == 0
                and n[0] * m[2] - n[2] * m[0] == 0
                and n[1] * m[2] - n[2] * m[1] == 0
                and dot(m, n) > 0
            )
            if same_direction and dot(m, sub(p, q)) == 0:
                duplicate = True
                break
        if not duplicate:
            out.append((p, n))
    return out


def hull_order(points, normal):
    """Order coplanar points around their centroid via exact hull walk."""
    if len(points) < 3:
        return list(points)
    # Build an in-plane basis (exact).
    n = normal
    m = max(range(3), key=lambda i: abs(n[i]))
    u = [F(0), F(0), F(0)]
    v2 = [F(0), F(0), F(0)]
    axes = [i for i in range(3) if i != m]
    u[axes[0]] = F(1)
    v2[axes[1]] = F(1)
    # Project to 2D.
    def to2d(x):
        return (dot(x, tuple(u)), dot(x, tuple(v2)))

    def cross2(o, a, b):
        return (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])

    pts = [to2d(x) for x in points]
    # Andrew's monotone chain on exact rationals.
    pts = sorted(set(pts))
    if len(pts) < 3:
        return list(points)[: len(pts)]

    def half_hull(seq):
        h = []
        for p in seq:
            while len(h) >= 2 and cross2(h[-2], h[-1], p) <= 0:
                h.pop()
            h.append(p)
        return h

    lower = half_hull(pts)
    upper = half_hull(reversed(pts))
    hull2 = lower[:-1] + upper[:-1]
    # Map back to 3D (each hull point is unique after set()).
    back = {}
    for x in points:
        back.setdefault(to2d(x), x)
    ordered = [back[p] for p in hull2]
    # Ensure orientation matches the outward normal (right-hand rule).
    if len(ordered) >= 3:
        area_n = cross(sub(ordered[1], ordered[0]), sub(ordered[2], ordered[0]))
        if dot(area_n, n) < 0:
            ordered.reverse()
    return ordered


def main():
    # The exact geometry of the Rust test `boolean_cut_rotated_box_exact`:
    # A = box [0,10] x [0,8] x [0,6]  (tessellate_box corner convention)
    # B = a 6x6x6 box centered in XY at (10, 8, 3.5), rotated about Z by
    #     (c, s) = (0.6, 0.8) — FreeCAD row-vector convention:
    #     x' = c*x - s*y, y' = s*x + c*y (matching Matrix4::transform_point
    #     in fc-base: p.x*m00 + p.y*m10 + p.z*m20).
    a_faces = faces_of_box((0, 10, 0, 8, 0, 6))

    c, s = 0.6, 0.8
    cx, cy, cz = 10.0, 8.0, 3.5
    half = 3.0

    def corner(lx, ly, lz):
        x = c * lx - s * ly + cx
        y = s * lx + c * ly + cy
        z = lz + cz
        return v(x, y, z)

    # tessellate_box vertex order: 0:(-,-,-) 1:(+,-,-) 2:(+,+,-) 3:(-,+,-)
    #                             4:(-,-,+) 5:(+,-,+) 6:(+,+,+) 7:(-,+,+)
    corners = [
        corner(-half, -half, -half),
        corner(half, -half, -half),
        corner(half, half, -half),
        corner(-half, half, -half),
        corner(-half, -half, half),
        corner(half, -half, half),
        corner(half, half, half),
        corner(-half, half, half),
    ]
    tris = [
        (0, 3, 2), (0, 2, 1),  # bottom, outward -Z
        (4, 5, 6), (4, 6, 7),  # top, outward +Z
        (0, 1, 5), (0, 5, 4),  # y-, outward -Y
        (3, 7, 6), (3, 6, 2),  # y+, outward +Y
        (0, 4, 7), (0, 7, 3),  # x-, outward -X
        (1, 2, 6), (1, 6, 5),  # x+, outward +X
    ]
    # Rotation about Z preserves winding (det = +1).
    b_faces = faces_from_triangles(corners, tris)

    inter = intersect_volume(a_faces, b_faces)
    va = F(10 * 8 * 6)
    cut = va - inter
    print("V(A)          =", va, "=", float(va))
    print("V(A n B)      =", inter)
    print("V(A n B) float=", repr(float(inter)))
    print("V(A - B)      =", cut)
    print("V(A - B) float=", repr(float(cut)))


if __name__ == "__main__":
    main()
