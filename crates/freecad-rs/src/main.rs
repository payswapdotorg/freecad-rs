//! `freecad-rs` — command-line front-end for the Rust FreeCAD core.
//!
//! Stateless-by-file CLI: every command loads `FILE`, mutates, recomputes
//! (when needed) and saves back, mirroring how a document lives on disk.
//! Run `docs/cli.md` for the full walkthrough.

mod commands;
mod svg;

use clap::{Parser, Subcommand};

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
    /// Document metadata.
    Info { file: String },
    /// Per-object analytic volume / surface area.
    Stats { file: String },
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
