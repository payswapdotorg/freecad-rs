//! `freecad-rs` — command-line front-end for the Rust FreeCAD core.
//!
//! Stateless-by-file CLI: every command loads `FILE`, mutates, recomputes
//! (when needed) and saves back, mirroring how a document lives on disk.
//! Run `docs/cli.md` for the full walkthrough.

mod calc;
mod commands;
mod script;
mod sketch;
mod svg;

use clap::{Parser, Subcommand, ValueEnum};

#[derive(Parser)]
#[command(
    name = "freecad-rs",
    version,
    about = "FreeCAD-style parametric documents in Rust (clean-room reimplementation of the core)",
    after_help = "Files are .FCStd zip archives. Every mutating command saves the file back."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Create a new empty document.
    New {
        file: String,
        /// Document label.
        #[arg(long, default_value = "untitled")]
        label: String,
    },
    /// Add a shape or transform feature, then save.
    Add {
        file: String,
        #[command(subcommand)]
        shape: AddShape,
        /// Object name (auto-uniquified when taken).
        #[arg(long)]
        name: Option<String>,
    },
    /// Set a property; touches the object + dependents, then saves.
    Set {
        file: String,
        /// Object name.
        object: String,
        /// Property name.
        property: String,
        /// Value in the property's text form (see `docs/cli.md`).
        value: String,
    },
    /// Bind a Float property to an expression (FreeCAD `setExpression`),
    /// then recompute + save. Example: `2 * Base.Length + sin(30)`.
    SetExpr {
        file: String,
        /// Object name.
        object: String,
        /// Property name (must be a Float).
        property: String,
        /// Expression text (see `fc_app::expression` grammar).
        expression: String,
    },
    /// Remove an expression binding; the property keeps its last value.
    ClearExpr {
        file: String,
        object: String,
        property: String,
    },
    /// Print a property's value.
    Get {
        file: String,
        object: String,
        property: String,
    },
    /// List objects (name, type, status).
    List {
        file: String,
        #[arg(long)]
        json: bool,
    },
    /// Recompute touched objects, save, and print what ran.
    Recompute {
        file: String,
        #[arg(long)]
        json: bool,
    },
    /// Undo the last N committed steps (default 1) — the App::Transaction
    /// undo stack. Saves FILE and its session journal.
    Undo {
        file: String,
        /// Number of steps to undo.
        #[arg(default_value_t = 1)]
        steps: usize,
    },
    /// Redo the last N undone steps (default 1) — symmetric to `undo`.
    /// Saves FILE and its session journal.
    Redo {
        file: String,
        /// Number of steps to redo.
        #[arg(default_value_t = 1)]
        steps: usize,
    },
    /// Show the undo/redo history of FILE (undo-stack labels + redo count).
    History { file: String },
    /// Evaluate a pure-quantity arithmetic expression with unit checking
    /// (e.g. `"25 mm + 3 cm"`, `"9.8 m/s^2 * 2 kg"`). No document needed.
    Calc {
        /// Quantity expression (quote it in the shell).
        expression: String,
    },
    /// Solve a sketch stored as a FreeCAD-style `<Sketch>` XML fragment
    /// and print the outcome + resulting geometry.
    SketchSolve {
        /// Path to the sketch XML fragment.
        file: String,
    },
    /// Print a sketch fragment's geometry and constraints without solving.
    SketchShow {
        /// Path to the sketch XML fragment.
        file: String,
    },
    /// Import a `<Sketch>` XML fragment into an existing
    /// `Sketcher::SketchObject` in the document (which must exist and be a
    /// sketch object), touch it + its dependents, recompute and save.
    SketchImport {
        file: String,
        /// Sketch object name.
        object: String,
        /// Path to the sketch XML fragment to import.
        fragment: String,
    },
    /// Export a `Sketcher::SketchObject`'s content as a `<Sketch>` XML
    /// fragment (no recompute).
    SketchExport {
        file: String,
        /// Sketch object name.
        object: String,
        /// Output path for the fragment.
        output: String,
    },
    /// Show a sketch object's geometry/constraint counts, DOF and last
    /// solve result (or "not yet solved").
    SketchInfo {
        file: String,
        /// Sketch object name.
        object: String,
    },
    /// PartDesign::Pad — extrude a sketch's profile into a solid.
    Pad {
        file: String,
        /// Profile sketch object name.
        #[arg(long)]
        sketch: String,
        /// Extrusion length (mm).
        #[arg(long, default_value_t = 10.0)]
        length: f64,
        /// Extrude symmetrically about the sketch plane.
        #[arg(long)]
        midplane: bool,
        /// Extrude along -Z instead of +Z.
        #[arg(long)]
        reversed: bool,
        /// Object name (auto-uniquified when taken).
        #[arg(long)]
        name: Option<String>,
    },
    /// PartDesign::Pocket — cut a sketch's extrusion out of a base
    /// feature.
    Pocket {
        file: String,
        /// Profile sketch object name.
        #[arg(long)]
        sketch: String,
        /// Base feature to cut from (any mesh-bearing object).
        #[arg(long)]
        base: String,
        /// Tool extrusion length (mm).
        #[arg(long, default_value_t = 5.0)]
        length: f64,
        /// Cut along -Z instead of +Z.
        #[arg(long)]
        inverted: bool,
        /// Object name (auto-uniquified when taken).
        #[arg(long)]
        name: Option<String>,
    },
    /// PartDesign::Revolution — sweep a sketch's profile around an axis.
    Revolution {
        file: String,
        /// Profile sketch object name.
        #[arg(long)]
        sketch: String,
        /// Sweep angle in degrees (|angle| >= 360 is a full turn).
        #[arg(long, default_value_t = 360.0)]
        angle: f64,
        /// Revolution axis (see docs/cli.md for the coordinate frames).
        #[arg(long, value_enum, default_value_t = AxisChoice::Z)]
        axis: AxisChoice,
        /// Object name (auto-uniquified when taken).
        #[arg(long)]
        name: Option<String>,
    },
    /// Document metadata.
    Info { file: String },
    /// Per-object analytic volume / surface area.
    Stats { file: String },
    /// Run a rhai script against a fresh engine (the scripting console
    /// surface: create documents, add objects, set properties, bind
    /// expressions, recompute, read stats, save — see `docs/cli.md`).
    Script {
        /// Path to the `.rhai` script.
        file: String,
    },
    /// Render the document offscreen to a PNG (the M1 viewer surface;
    /// the same cameras as `export svg`). Error objects render visibly
    /// flagged. `--pick x,y` reports the object at that pixel instead
    /// of writing the image.
    Render {
        file: String,
        /// Output PNG path.
        output: String,
        /// Projection: iso | top | front | right.
        #[arg(long, default_value = "iso")]
        view: String,
        #[arg(long, default_value_t = 1280)]
        width: u32,
        #[arg(long, default_value_t = 800)]
        height: u32,
        /// Report the object at pixel `x,y` instead of writing the PNG.
        #[arg(long, value_delimiter = ',')]
        pick: Option<Vec<u32>>,
        /// Also (re)write the committed golden image for this view — the
        /// regeneration path, so goldens are never hand-edited.
        #[arg(long)]
        bless: bool,
    },
    /// Import a STEP file into the document as a `Part::Feature` mesh
    /// (faceted import under an explicit deflection — ADR-0002).
    ImportStep {
        /// The .FCStd document (created when missing).
        file: String,
        /// Input STEP path (.step/.stp).
        input: String,
        /// Object name (auto-uniquified when taken).
        #[arg(long)]
        name: Option<String>,
        /// Chord deflection in mm (curved-face tessellation fidelity).
        #[arg(long, default_value_t = 0.1)]
        deflection: f64,
    },
    /// Export the document to another format.
    // FC-2-e fix: the task spec, README quick tour and roadmap all invoke
    // `export svg FILE OUT --view V`; the scaffolded variant had no
    // format subcommand, making that documented form unparseable.
    Export {
        #[command(subcommand)]
        format: ExportFormat,
    },
    /// Write the dependency graph as Graphviz DOT.
    Graph { file: String, output: String },
}

/// The revolution axis choices (`revolution --axis`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum AxisChoice {
    /// 3D Z axis, the kernel's classic lathe frame: sketch x = radius,
    /// sketch y = height.
    Z,
    /// The sketch's vertical axis (3D Y).
    Y,
}

#[derive(Subcommand)]
pub enum AddShape {
    /// FreeCAD Part::Box.
    Box {
        #[arg(long, default_value_t = 10.0)]
        length: f64,
        #[arg(long, default_value_t = 10.0)]
        width: f64,
        #[arg(long, default_value_t = 10.0)]
        height: f64,
    },
    /// FreeCAD Part::Sphere.
    Sphere {
        #[arg(long, default_value_t = 5.0)]
        radius: f64,
    },
    /// FreeCAD Part::Cylinder.
    Cylinder {
        #[arg(long, default_value_t = 2.0)]
        radius: f64,
        #[arg(long, default_value_t = 10.0)]
        height: f64,
    },
    /// FreeCAD Part::Cone.
    Cone {
        #[arg(long, default_value_t = 2.0)]
        radius1: f64,
        #[arg(long, default_value_t = 0.0)]
        radius2: f64,
        #[arg(long, default_value_t = 10.0)]
        height: f64,
    },
    /// FreeCAD Part::Torus.
    Torus {
        #[arg(long, default_value_t = 10.0)]
        radius1: f64,
        #[arg(long, default_value_t = 2.0)]
        radius2: f64,
    },
    /// Part::Transform — link, place and scale another object.
    Transform {
        /// Source object name.
        #[arg(long)]
        source: String,
        /// Translation `x,y,z`.
        #[arg(long, value_delimiter = ',', default_values_t = [0.0, 0.0, 0.0])]
        translate: Vec<f64>,
        /// Rotation axis `x,y,z` (with --rotate-deg).
        #[arg(long, value_delimiter = ',', default_values_t = [0.0, 0.0, 1.0])]
        rotate_axis: Vec<f64>,
        /// Rotation angle in degrees (about --rotate-axis).
        #[arg(long, default_value_t = 0.0)]
        rotate_deg: f64,
        /// Uniform scale factor.
        #[arg(long, default_value_t = 1.0)]
        scale: f64,
    },
    /// Part::Cut — subtract the Tool mesh from the Base mesh.
    Cut {
        /// Base object name.
        #[arg(long)]
        base: String,
        /// Tool object name.
        #[arg(long)]
        tool: String,
    },
    /// Sketcher::SketchObject — a 2D parametric sketch as a document
    /// object. Created empty; fill it with `sketch-import`.
    Sketch,
    /// Part::Fuse — union of the Base and Tool meshes.
    Fuse {
        /// Base object name.
        #[arg(long)]
        base: String,
        /// Tool object name.
        #[arg(long)]
        tool: String,
    },
    /// Part::Common — intersection of the Base and Tool meshes.
    Common {
        /// Base object name.
        #[arg(long)]
        base: String,
        /// Tool object name.
        #[arg(long)]
        tool: String,
    },
    /// Part::Mirroring — reflect the Source shape through a plane
    /// (given directly, or by a linked datum plane).
    Mirroring {
        /// Source object name.
        #[arg(long)]
        source: String,
        /// Point on the mirror plane, `x,y,z`.
        #[arg(long, value_delimiter = ',', default_values_t = [0.0, 0.0, 0.0])]
        base: Vec<f64>,
        /// Mirror plane normal, `x,y,z`.
        #[arg(long, value_delimiter = ',', default_values_t = [0.0, 0.0, 1.0])]
        normal: Vec<f64>,
        /// Datum-plane object name (overrides --base/--normal: the plane
        /// is the datum's placement — position, rotation · +Z).
        #[arg(long)]
        mirror_plane: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum ExportFormat {
    /// Render a shaded orthographic SVG of the document.
    Svg {
        file: String,
        output: String,
        /// Projection: iso | top | front | right.
        #[arg(long, default_value = "iso")]
        view: String,
    },
    /// Export the document's meshes to a Wavefront OBJ file.
    Obj { file: String, output: String },
    /// Export the document's meshes to STL (ASCII by default).
    Stl {
        file: String,
        output: String,
        /// Binary STL instead of ASCII.
        #[arg(long)]
        binary: bool,
    },
    /// Export the document's solids as faceted AP203 STEP (ADR-0002).
    Step { file: String, output: String },
}

fn main() {
    let cli = Cli::parse();
    let code = match commands::dispatch(cli.command) {
        Ok(()) => 0,
        Err(err) => {
            eprintln!("error: {err}");
            1
        }
    };
    std::process::exit(code);
}
