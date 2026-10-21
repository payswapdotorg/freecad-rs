//! `fc-script` — embedded scripting for the document model, distilling
//! FreeCAD's Python console into **rhai** (pure Rust, no CPython).
//!
//! FreeCAD's power surface is its scripting console: a program drives the
//! parametric document — create documents, add objects, set properties,
//! bind expressions, recompute, read results, save. This crate exposes the
//! same *architectural* capability through a small, honest rhai API:
//!
//! ```text
//! let doc = new_document("House");
//! let base = doc.add_box("Base", 30.0, 20.0, 5.0);   // Part::Box
//! doc.bind(base, "Length", "2 * 15");                // setExpression
//! doc.set(base, "Placement", placement(vec3(0,0,0), vec3(0,0,1), 25));
//! doc.recompute();                                   // dependencies-first
//! let v = doc.volume(base);                          // exact analytic stats
//! doc.save("house.FCStd");                           // .FCStd via fc-io
//! ```
//!
//! # Capabilities (mirroring the CLI surface)
//!
//! - `new_document(label)` / `open_document(path)` — fresh or loaded
//!   documents (`.FCStd`, via `fc-io`)
//! - `doc.add_box / add_sphere / add_cylinder / add_cone / add_torus /
//!   add_transform / add_cut / add_fuse / add_common` — the part features
//!   behind the CLI's `add` verb
//! - `doc.set(object, property, value)` — parsed against the property's
//!   **existing type**, exactly like the CLI `set` verb (including honest
//!   parse errors); quantity-free float properties, links, vectors,
//!   placements
//! - `doc.bind(object, property, expression)` / `doc.unbind(...)` — the
//!   `App::PropertyExpressionEngine` surface of `fc-app`: parse-validated,
//!   Float-only bindings, evaluated dependencies-first at every recompute
//!   (dependency-graph edges come from expression references; cycles and
//!   type mismatches are rejected honestly)
//! - `doc.recompute()` — `true` on success; a failure records the failing
//!   object's error in the transcript **and** in
//!   [`ScriptEngine::recompute_error`] (the CLI's exit-code carrier)
//! - `doc.volume / doc.area / doc.bbox` — the exact analytic stats behind
//!   the CLI's `stats` verb
//! - `doc.save(path)` / `doc.export_stl(path, binary)` /
//!   `doc.export_obj(path)` — persistence and mesh export
//!
//! Every fallible call surfaces a rhai runtime error with an honest
//! message (no panics cross the boundary; errors are designed out, not
//! caught). Every action appends a line to the session transcript, which
//! the CLI's `script` verb prints — a small, honest log of what the script
//! did.
//!
//! # Sandbox
//!
//! rhai is sandboxed by construction: scripts get no file, network or FFI
//! access — only the functions registered here. An operation budget
//! ([`ScriptEngine`]) stops runaway loops. The document API is the whole
//! attack surface; there is nothing else to reach.

mod api;
mod session;
mod value;

pub use api::ScriptEngine;
pub use session::ScriptLog;
