//! `fc-part` — the geometry layer, mirroring FreeCAD's `Mod/Part`.
//!
//! FreeCAD's actual kernel is OpenCascade (OCCT), a multi-million-line C++
//! monster. This crate is an honest, much smaller reimplementation of the
//! parts that matter for a parametric core: analytic primitives with exact
//! volume/area, tessellation to triangle meshes, a stable text
//! serialization ("FCBREP"), and the classic part features wired into the
//! `fc-app` recompute loop.
//!
//! What is **not** implemented (and honestly so): fillets, chamfers,
//! OCCT BRep interoperability, and B-spline *booleans* (the exact
//! topology model with NURBS curves/surfaces exists — [`topology`],
//! K1 — but general intersections on it are K3/K4 work). Booleans run
//! on tessellations with exact classification (see [`exact`]) — the
//! volumes of curved operands agree to their deflection, and planar
//! operands exactly.

mod exact;

pub mod boolean;
pub mod brep;
pub mod features;
pub mod partdesign;
pub mod profile;
pub mod shape;
pub mod topology;

pub use brep::{read_brep_bytes, read_shape_bytes, write_brep, write_shape, BrpError, BrpFormat};
pub use features::{
    mesh_of, shape_of, BooleanFeature, BooleanKind, BoxFeature, ConeFeature, CylinderFeature,
    MeshFeature, MirroringFeature, SphereFeature, TorusFeature, TransformFeature,
};
pub use partdesign::{PadFeature, PocketFeature, RevolutionFeature};
pub use profile::{
    classify_wires, extract_wires, extrude, revolve, Point2, ProfileError, Wire,
    DEFAULT_PROFILE_DEFLECTION, DEFAULT_TOLERANCE,
};
pub use shape::{Mesh, TopoShape, DEFAULT_DEFLECTION};
pub use topology::{Curve, ExactBRep, Surface, TopoEdge, TopoFace, TopoVertex, TopoWire, WireEdge};
