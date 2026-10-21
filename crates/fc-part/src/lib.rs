//! `fc-part` — the geometry layer, mirroring FreeCAD's `Mod/Part`.
//!
//! FreeCAD's actual kernel is OpenCascade (OCCT), a multi-million-line C++
//! monster. This crate is an honest, much smaller reimplementation of the
//! parts that matter for a parametric core: analytic primitives with exact
//! volume/area, tessellation to triangle meshes, a stable text
//! serialization ("FCBREP"), and the classic part features wired into the
//! `fc-app` recompute loop.
//!
//! What is **not** implemented (and honestly so): NURBS, booleans, fillets,
//! chamfers, STEP/BREP interoperability with OCCT.

pub mod boolean;
pub mod brep;
pub mod features;
pub mod profile;
pub mod shape;

pub use brep::{read_shape_bytes, write_shape, BrpError, BrpFormat};
pub use features::{
    mesh_of, shape_of, BooleanFeature, BooleanKind, BoxFeature, ConeFeature, CylinderFeature,
    SphereFeature, TorusFeature, TransformFeature,
};
pub use profile::{
    classify_wires, extract_wires, extrude, revolve, Point2, ProfileError, Wire,
    DEFAULT_PROFILE_DEFLECTION, DEFAULT_TOLERANCE,
};
pub use shape::{Mesh, TopoShape, DEFAULT_DEFLECTION};
