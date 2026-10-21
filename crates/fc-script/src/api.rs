//! Engine construction: a fresh rhai engine with the document API
//! registered.
//!
//! Registration discipline:
//!
//! - every fallible API call returns `Result<T, Box<EvalAltResult>>` —
//!   failures cross into the script as honest rhai runtime errors carrying
//!   the CLI's message style; nothing panics across the boundary;
//! - numeric parameters arrive as `Dynamic` and are coerced (INT and
//!   FLOAT both accepted, like a shell's `30` and `30.0`); wrong types are
//!   honest errors, not conversions gone wrong;
//! - string parameters are plain `&str`;
//! - every mutating call appends to the run's transcript
//!   ([`ScriptLog::events`]), which the CLI's `script` verb prints.

use std::sync::{Arc, Mutex};

use rhai::{Dynamic, EvalAltResult};

use fc_base::Vector3;

use crate::session::{placement_of, PlacementVal, ScriptLog, ScriptSession};
use crate::value::{coerce_f64, property_text, Vec3};

/// The one honest error crossing: a kernel `String` message becomes a
/// rhai runtime error (an unambiguous conversion point for every
/// registration).
fn err(message: String) -> Box<EvalAltResult> {
    message.into()
}

/// A rhai engine carrying the document API plus the run's shared log.
/// Create one per script run ([`ScriptEngine::new`]), run the script
/// ([`ScriptEngine::run`]), then read back the outcome
/// ([`ScriptEngine::events`] / [`ScriptEngine::recompute_error`]).
pub struct ScriptEngine {
    engine: rhai::Engine,
    log: Arc<Mutex<ScriptLog>>,
}

impl ScriptEngine {
    /// Build the engine: sandboxed rhai (no file/network/FFI access, an
    /// operation budget against runaway loops) with the document API
    /// registered — nothing else is reachable from a script.
    pub fn new() -> Self {
        let mut engine = rhai::Engine::new();
        // Sandbox budget: each script step counts against a fixed
        // operation limit, so an infinite loop is an honest runtime error
        // ("Operation count exceeds maximum") rather than a hang.
        engine.set_max_operations(10_000_000);

        engine.register_type_with_name::<ScriptSession>("Document");
        engine.register_type_with_name::<Vec3>("Vec3");
        engine.register_type_with_name::<PlacementVal>("Placement");

        let log: Arc<Mutex<ScriptLog>> = Arc::default();

        // `print` in a script routes into the transcript, in order with
        // the API's own events.
        {
            let log = log.clone();
            engine.on_print(move |text| {
                log.lock()
                    .expect("script log poisoned")
                    .events
                    .push(text.to_string());
            });
        }

        // --- constructors ------------------------------------------------

        {
            let log = log.clone();
            engine.register_fn("new_document", move |label: &str| {
                ScriptSession::new(label, log.clone())
            });
        }
        {
            let log = log.clone();
            engine.register_fn(
                "open_document",
                move |path: &str| -> Result<ScriptSession, Box<EvalAltResult>> {
                    ScriptSession::open(path, log.clone()).map_err(err)
                },
            );
        }
        engine.register_fn(
            "vec3",
            |x: Dynamic, y: Dynamic, z: Dynamic| -> Result<Vec3, Box<EvalAltResult>> {
                let x = coerce_f64(&x).map_err(err)?;
                let y = coerce_f64(&y).map_err(err)?;
                let z = coerce_f64(&z).map_err(err)?;
                Ok(Vec3(Vector3::new(x, y, z)))
            },
        );
        engine.register_fn(
            "placement",
            |pos: Vec3,
             axis: Vec3,
             angle_deg: Dynamic|
             -> Result<PlacementVal, Box<EvalAltResult>> {
                let angle_deg = coerce_f64(&angle_deg).map_err(err)?;
                placement_of(pos.0, axis.0, angle_deg).map_err(err)
            },
        );

        // `print(vec3(...))` / `print(placement(...))` read back sanely.
        engine.register_fn("to_string", |v: &mut Vec3| -> String { v.to_string() });
        engine.register_fn("to_string", |p: &mut PlacementVal| -> String {
            fc_app::Property::Placement(p.0).to_text()
        });
        // Vec3 coordinate read-back (`p.x()` etc.).
        engine.register_fn("x", |v: &mut Vec3| -> f64 { v.x() });
        engine.register_fn("y", |v: &mut Vec3| -> f64 { v.y() });
        engine.register_fn("z", |v: &mut Vec3| -> f64 { v.z() });

        // --- add: the CLI `add` surface ----------------------------------

        engine.register_fn(
            "add_box",
            |doc: &mut ScriptSession,
             name: &str,
             length: Dynamic,
             width: Dynamic,
             height: Dynamic|
             -> Result<String, Box<EvalAltResult>> {
                let length = coerce_f64(&length).map_err(err)?;
                let width = coerce_f64(&width).map_err(err)?;
                let height = coerce_f64(&height).map_err(err)?;
                Ok(doc.add_box(name, length, width, height))
            },
        );
        engine.register_fn(
            "add_sphere",
            |doc: &mut ScriptSession,
             name: &str,
             radius: Dynamic|
             -> Result<String, Box<EvalAltResult>> {
                Ok(doc.add_sphere(name, coerce_f64(&radius).map_err(err)?))
            },
        );
        engine.register_fn(
            "add_cylinder",
            |doc: &mut ScriptSession,
             name: &str,
             radius: Dynamic,
             height: Dynamic|
             -> Result<String, Box<EvalAltResult>> {
                let radius = coerce_f64(&radius).map_err(err)?;
                let height = coerce_f64(&height).map_err(err)?;
                Ok(doc.add_cylinder(name, radius, height))
            },
        );
        engine.register_fn(
            "add_cone",
            |doc: &mut ScriptSession,
             name: &str,
             radius1: Dynamic,
             radius2: Dynamic,
             height: Dynamic|
             -> Result<String, Box<EvalAltResult>> {
                let radius1 = coerce_f64(&radius1).map_err(err)?;
                let radius2 = coerce_f64(&radius2).map_err(err)?;
                let height = coerce_f64(&height).map_err(err)?;
                Ok(doc.add_cone(name, radius1, radius2, height))
            },
        );
        engine.register_fn(
            "add_torus",
            |doc: &mut ScriptSession,
             name: &str,
             radius1: Dynamic,
             radius2: Dynamic|
             -> Result<String, Box<EvalAltResult>> {
                let radius1 = coerce_f64(&radius1).map_err(err)?;
                let radius2 = coerce_f64(&radius2).map_err(err)?;
                Ok(doc.add_torus(name, radius1, radius2))
            },
        );
        engine.register_fn(
            "add_transform",
            |doc: &mut ScriptSession,
             name: &str,
             source: &str|
             -> Result<String, Box<EvalAltResult>> {
                doc.add_transform(name, source).map_err(err)
            },
        );
        engine.register_fn(
            "add_cut",
            |doc: &mut ScriptSession,
             name: &str,
             base: &str,
             tool: &str|
             -> Result<String, Box<EvalAltResult>> {
                doc.add_cut(name, base, tool).map_err(err)
            },
        );
        engine.register_fn(
            "add_fuse",
            |doc: &mut ScriptSession,
             name: &str,
             base: &str,
             tool: &str|
             -> Result<String, Box<EvalAltResult>> {
                doc.add_fuse(name, base, tool).map_err(err)
            },
        );
        engine.register_fn(
            "add_common",
            |doc: &mut ScriptSession,
             name: &str,
             base: &str,
             tool: &str|
             -> Result<String, Box<EvalAltResult>> {
                doc.add_common(name, base, tool).map_err(err)
            },
        );

        // --- set / bind: properties and expressions ----------------------

        engine.register_fn(
            "set",
            |doc: &mut ScriptSession,
             object: &str,
             property: &str,
             value: Dynamic|
             -> Result<(), Box<EvalAltResult>> {
                let text = property_text(&value).map_err(err)?;
                doc.set(object, property, &text).map_err(err)
            },
        );
        engine.register_fn(
            "bind",
            |doc: &mut ScriptSession,
             object: &str,
             property: &str,
             expression: &str|
             -> Result<(), Box<EvalAltResult>> {
                doc.bind(object, property, expression).map_err(err)
            },
        );
        engine.register_fn(
            "unbind",
            |doc: &mut ScriptSession,
             object: &str,
             property: &str|
             -> Result<(), Box<EvalAltResult>> {
                doc.unbind(object, property).map_err(err)
            },
        );

        // --- recompute + stats -------------------------------------------

        engine.register_fn("recompute", |doc: &mut ScriptSession| -> bool {
            doc.recompute()
        });
        engine.register_fn(
            "volume",
            |doc: &mut ScriptSession, name: &str| -> Result<f64, Box<EvalAltResult>> {
                doc.volume(name).map_err(err)
            },
        );
        engine.register_fn(
            "area",
            |doc: &mut ScriptSession, name: &str| -> Result<f64, Box<EvalAltResult>> {
                doc.area(name).map_err(err)
            },
        );
        engine.register_fn(
            "bbox",
            |doc: &mut ScriptSession, name: &str| -> Result<rhai::Map, Box<EvalAltResult>> {
                let bb = doc.bbox(name).map_err(err)?;
                let axis = |v: Vector3| {
                    Dynamic::from(vec![
                        Dynamic::from(v.x),
                        Dynamic::from(v.y),
                        Dynamic::from(v.z),
                    ])
                };
                let mut map = rhai::Map::new();
                map.insert("min".into(), axis(bb.min));
                map.insert("max".into(), axis(bb.max));
                map.insert("size".into(), axis(bb.size()));
                map.insert("center".into(), axis(bb.center()));
                Ok(map)
            },
        );

        // --- persistence + export ----------------------------------------

        engine.register_fn(
            "save",
            |doc: &mut ScriptSession, path: &str| -> Result<(), Box<EvalAltResult>> {
                doc.save(path).map_err(err)
            },
        );
        engine.register_fn(
            "export_stl",
            |doc: &mut ScriptSession, path: &str, binary: bool| -> Result<(), Box<EvalAltResult>> {
                doc.export_stl(path, binary).map_err(err)
            },
        );
        engine.register_fn(
            "export_obj",
            |doc: &mut ScriptSession, path: &str| -> Result<(), Box<EvalAltResult>> {
                doc.export_obj(path).map_err(err)
            },
        );

        Self { engine, log }
    }

    /// Run a script (parse + evaluate). A rhai syntax or runtime error
    /// comes back as-is — the caller surfaces it honestly (clear message,
    /// non-zero exit).
    pub fn run(&mut self, script: &str) -> Result<(), Box<EvalAltResult>> {
        self.engine.run(script)
    }

    /// The transcript: what the script did, in order.
    pub fn events(&self) -> Vec<String> {
        self.log.lock().expect("script log poisoned").events.clone()
    }

    /// Load warnings from `open_document` (the CLI surfaces these on
    /// stderr).
    pub fn warnings(&self) -> Vec<String> {
        self.log
            .lock()
            .expect("script log poisoned")
            .warnings
            .clone()
    }

    /// The recorded recompute failure, if any — the CLI's exit-code
    /// carrier for a script whose recompute failed.
    pub fn recompute_error(&self) -> Option<String> {
        self.log
            .lock()
            .expect("script log poisoned")
            .recompute_error
            .clone()
    }
}

impl Default for ScriptEngine {
    fn default() -> Self {
        Self::new()
    }
}
