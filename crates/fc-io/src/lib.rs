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
//! Value elements (ground truth: FreeCAD `src/App/PropertyStandard.cpp`,
//! `PropertyGeo.cpp`, `PropertyLinks.cpp`):
//!
//! | type                    | element                                        |
//! |-------------------------|------------------------------------------------|
//! | App::PropertyBool       | `<Bool value="true"/>`                         |
//! | App::PropertyInteger    | `<Integer value="42"/>`                        |
//! | App::PropertyFloat      | `<Float value="10.5"/>`                         |
//! | App::PropertyString     | `<String value="hello"/>`                      |
//! | App::PropertyVector     | `<PropertyVector valueX valueY valueZ/>`       |
//! | App::PropertyPlacement  | `<PropertyPlacement Px Py Pz Q0 Q1 Q2 Q3/>`    |
//! | App::PropertyLink       | `<Link value="Box"/>`                          |
//! | App::PropertyLinkList   | `<LinkList value="Box,Box001"/>`               |
//! | Part::PropertyPartShape | `<Part file="Shape001.brp"/>`                  |
//!
//! Fidelity, honestly stated:
//! - Files we write round-trip losslessly through this library.
//! - Real FreeCAD files: `Document.xml` is read (schema-compatible);
//!   analytic primitives come back fully (they are just properties);
//!   OCCT `.brp` shape payloads are detected and skipped with a warning
//!   (geometry loss, parameters kept).

pub mod fcstd;
pub mod mesh_export;

pub use fcstd::{load_document, save_document, FcstdError, LoadReport};
