//! Parametric primitives and triangle meshes.

use fc_base::{BoundBox3, Matrix4, Vector3};

/// Tessellation quality used when a caller doesn't care
/// (max chordal deviation, model units).
pub const DEFAULT_DEFLECTION: f64 = 0.2;

/// An analytic primitive or a tessellated mesh — the `TopoShape` of this
/// kernel (FreeCAD/OCCT `TopoDS_Shape`, distilled to its honest core).
#[derive(Debug, Clone, PartialEq)]
pub enum TopoShape {
    /// Axis-aligned box, centered on the origin like FreeCAD `Part::Box`:
    /// occupies `[-length/2, length/2] x [-width/2, width/2] x
    /// [0, height]`.
    Box {
        length: f64,
        width: f64,
        height: f64,
    },
    Sphere {
        radius: f64,
    },
    /// Along +Z: `[-height/2, height/2]` like FreeCAD `Part::Cylinder`.
    Cylinder {
        radius: f64,
        height: f64,
    },
    Cone {
        radius1: f64,
        radius2: f64,
        height: f64,
    },
    /// FreeCAD `Part::Torus`: `radius1` = ring radius, `radius2` = tube
    /// radius, centered on origin in the XY plane.
    Torus {
        radius1: f64,
        radius2: f64,
    },
    Mesh(Mesh),
}

impl TopoShape {
    /// `"Box"`, `"Sphere"`, `"Cylinder"`, `"Cone"`, `"Torus"`, `"Mesh"`.
    pub fn kind_name(&self) -> &'static str {
        todo!()
    }

    /// Reject non-finite or non-positive dimensions (an empty box is a
    /// modeling error, not a shape).
    pub fn validate(&self) -> Result<(), String> {
        todo!()
    }

    /// Exact analytic bounding box.
    pub fn bbox(&self) -> BoundBox3 {
        todo!()
    }

    /// Exact analytic volume (mesh variant: sum of signed tetrahedra
    /// against the origin).
    pub fn volume(&self) -> f64 {
        todo!()
    }

    /// Exact analytic surface area (mesh variant: sum of triangle areas).
    pub fn surface_area(&self) -> f64 {
        todo!()
    }

    /// Tessellate. `deflection` controls chordal deviation: smaller is
    /// finer. For `Mesh` variants the mesh is returned as-is.
    pub fn tessellate(&self, deflection: f64) -> Mesh {
        todo!()
    }
}

/// Indexed triangle mesh.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Mesh {
    pub vertices: Vec<Vector3>,
    /// Ccw winding when viewed from outside.
    pub triangles: Vec<[u32; 3]>,
}

impl Mesh {
    pub fn new(vertices: Vec<Vector3>, triangles: Vec<[u32; 3]>) -> Self {
        Self { vertices, triangles }
    }

    /// In-place transform by `m` (points, not directions — translation
    /// applies).
    pub fn transform(&mut self, m: &Matrix4) {
        todo!()
    }

    pub fn transformed(&self, m: &Matrix4) -> Mesh {
        todo!()
    }

    /// Append `other`'s geometry with index offsets.
    pub fn merge(&mut self, other: &Mesh) {
        todo!()
    }

    pub fn bbox(&self) -> Option<BoundBox3> {
        todo!()
    }

    pub fn triangle_count(&self) -> usize {
        self.triangles.len()
    }

    pub fn vertex_count(&self) -> usize {
        self.vertices.len()
    }
}
