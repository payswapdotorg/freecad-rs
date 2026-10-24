//! `fc-io` — the `.FCStd` file format.
//!
//! An `.FCStd` document is a zip archive whose first entry is
//! `Document.xml`, plus side files referenced from it (FreeCAD stores
//! OCCT `.brp` shape payloads there; we store FCBREP).
//!
//! `Document.xml` (both in FreeCAD and here) looks like:
//!
//! ```xml
//! <Document SchemaVersion="4" ProgramVersion="0.1.0" FileVersion="1">
//!  <Properties Count="3">
//!   <Property name="Label" type="App::PropertyString">
//!    <String value="untitled"/>
//!   </Property>
//!  </Properties>
//!  <Objects Count="2" Dependencies="1">
//!   <Object type="Part::Box" name="Box" id="1"/>
//!  </Objects>
//!  <ObjectData Count="2">
//!   <Object name="Box">
//!    <Properties Count="4">
//!     <Property name="Length" type="App::PropertyFloat">
//!      <Float value="10"/>
//!     </Property>
//!     <Property name="Placement" type="App::PropertyPlacement">
//!      <PropertyPlacement Px="0" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1" A="0" Ox="0" Oy="0" Oz="1"/>
//!     </Property>
//!     <Property name="Shape" type="Part::PropertyPartShape">
//!      <Part file="Shape001.brp"/>
//!     </Property>
//!     <Property name="Source" type="App::PropertyLink">
//!      <Link value="Box"/>
//!     </Property>
//!    </Properties>
//!   </Object>
//!  </ObjectData>
//! </Document>
//! ```
//!
//! Value elements (ground truth: the WOM work order's structural facts —
//! FreeCAD `src/App/PropertyStandard.cpp`, `PropertyGeo.cpp`,
//! `PropertyLinks.cpp` were NOT consulted; the table below is this
//! loader's accepted contract):
//!
//! | type                          | element                                        |
//! |-------------------------------|------------------------------------------------|
//! | App::PropertyBool             | `<Bool value="true"/>`                         |
//! | App::PropertyInteger          | `<Integer value="42"/>`                        |
//! | App::PropertyIntegerConstraint| `<Integer value="42"/>` → integer              |
//! | App::PropertyFloat            | `<Float value="10.5"/>`                        |
//! | App::PropertyString           | `<String value="hello"/>`                      |
//! | App::PropertyEnumeration      | `<String value="ThroughAll"/>` → string        |
//! | App::PropertyVector           | `<PropertyVector valueX valueY valueZ/>`       |
//! | App::PropertyPlacement        | `<PropertyPlacement Px Py Pz Q0 Q1 Q2 Q3/>`    |
//! | App::PropertyLink             | `<Link value="Box"/>`                          |
//! | App::PropertyLinkList         | `<LinkList value="Box,Box001"/>`               |
//! | Part::PropertyPartShape       | `<Part file="Shape001.brp"/>`                  |
//!
//! ## Reader breadth (WOM): property types beyond our writer's set
//!
//! Real FreeCAD files use property types our writer never emits. The
//! loader reads them into the closest thing our property model holds and
//! every mapping is documented here — nothing is dropped silently:
//!
//! | file type                                  | accepted value form                                            | stored as |
//! |--------------------------------------------|----------------------------------------------------------------|-----------|
//! | `App::PropertyVector`                      | attribute form (`valueX/valueY/valueZ` on the element) **or** element form (first child carrying those attributes) | `Property::Vector` |
//! | `App::PropertyEnumeration`                 | `<String value="..."/>` (choices list is metadata, not enforced) | `Property::String` |
//! | `App::PropertyAngle` / `Length` / `Distance` | `<Float value=".." unit=".."/>` — unit attribute optional (also accepted on a child element) | `Property::Float`; the quantity typing is recorded in `LoadReport::quantity_properties` (`"Obj.Prop"` / `"Obj.Prop (unit X)"`) |
//! | `App::PropertyIntegerConstraint`           | `<Integer value=".."/>`                                        | `Property::Integer` |
//! | `App::PropertyColor`                       | packed uint `<PropertyColor value="16711680"/>` (also `<Color .../>`) | `Property::String` `"#RRGGBB"` (uppercase; bits above the low 24 — alpha — are dropped) |
//! | `App::PropertyStringList`                  | `value` attribute or inner text, whitespace-split, and/or `<String value=".."/>` child entries | `Property::String`, entries single-space joined |
//! | `App::PropertyFloatList`                   | `value` attribute or inner text, whitespace-split, and/or `<Float value=".."/>` child entries | `Property::String`, shortest round-trip floats single-space joined |
//! | `App::PropertyIntegerList`                 | `value` attribute or inner text, whitespace-split, and/or `<Integer value=".."/>` child entries | `Property::String`, integers single-space joined |
//! | `App::PropertyMatrix`                      | 16 attributes `a11..a44` (row-major) on the element or its first child | `Property::String`, 16 floats row-major single-space joined |
//!
//! Whitespace-separated payloads are the contract for the list types
//! (string lists are space/newline-separated per the work order), so
//! list entries cannot themselves contain whitespace. Numeric lists with
//! one malformed entry skip the whole property (recorded — see below),
//! never the load.
//!
//! Properties with an unknown value element (or a malformed payload) are
//! recorded per-property in `LoadReport::skipped_properties`
//! (`"Obj.Prop"`) with the reason in `warnings` — the owning object
//! still loads. `LoadReport` therefore tracks skips at both granularities:
//! whole objects (`skipped`) and individual properties
//! (`skipped_properties`).
//!
//! ## Reader breadth (WOM): object types
//!
//! Types the kernel models (`Part::Box`, `Part::Sphere`, `Part::Cylinder`,
//! `Part::Cone`, `Part::Torus`, `Part::Cut`/`Fuse`/`Common`,
//! `Part::Transform`, `PartDesign::Pad`/`Pocket`/`Revolution`,
//! `Sketcher::SketchObject`) build as before. Beyond those, the loader
//! knows the container/link/origin family and keeps it as plain property
//! containers (a load-side object with no geometry semantics and a no-op
//! `execute`; it lists and re-saves under its own type id):
//!
//! - `App::Part` — `Group` (string list) + `Placement` ride in as
//!   properties; it is NOT a geometric object for us
//! - `PartDesign::Body` — `Tip`, `Group`, `Placement`
//! - `App::DocumentObjectGroup` — `Group` (FreeCAD 1.0's real group
//!   type, `App::PropertyLinkList` membership; our legacy
//!   `App::DocumentGroup` spelling still loads and re-saves as-is)
//! - `App::Link` — `LinkedObject`, `LinkTransform` (bool)
//! - `App::Origin` and its `App::Plane` / `App::Line` children —
//!   `Placement` and `Role`; no attachment semantics are derived
//!   (documented non-goal)
//! - `Part::Feature` — the family root, a parameter container
//! - unknown `Part::*` members — loaded as `Part::Feature` parameter
//!   containers with the source type id kept in a `TypeId` string
//!   property and a warning; any shape payload is skipped with the usual
//!   warnings (see below)
//!
//! Unknown object types outside the `Part::` family are still skipped
//! whole (per-object warning + `skipped` count) — never a silent success.
//!
//! Fidelity, honestly stated:
//! - Files we write round-trip losslessly through this library.
//! - Real FreeCAD files: `Document.xml` is read (schema-compatible);
//!   analytic primitives come back fully (they are just properties);
//!   OCCT `.brp` shape payloads are detected and skipped with a warning
//!   (geometry loss, parameters kept). That policy is asserted by the
//!   fixtures corpus in `tests/fixtures/` (see `tests/fixtures_corpus.rs`):
//!   a `.brp` entry must produce the documented warning, load the
//!   readable parameters, leave no geometry, and never panic.
//! - Quantity-typed values, colors, string/number lists and matrices
//!   load into the documented flat representations above: load-and-map,
//!   not round-trip — the re-saved file carries the canonical form (a
//!   quantity becomes a plain float, a color its `"#RRGGBB"` string,
//!   lists their single-space-joined strings), and `list`-level parity
//!   (names/types/statuses) is preserved across save/load cycles.

pub mod fcstd;
pub mod mesh_export;

pub use fcstd::{
    load_document, load_document_with_hook, save_document, FcstdError, LoadReport, ObjectHook,
};
