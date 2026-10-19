//! Command implementations. Each mutating command loads, mutates, saves.
//! Exit codes: 0 ok, 1 runtime error, 2 usage (clap).

use std::path::Path;

use fc_app::{Document, Property};
use fc_io::{load_document, save_document};
use fc_part::features::{
    BoxFeature, ConeFeature, CylinderFeature, SphereFeature, TorusFeature, TransformFeature,
};
use fc_base::{Placement, Rotation, Vector3};

use crate::{AddShape, Command};

pub fn dispatch(cmd: Command) -> Result<(), String> {
    match cmd {
        Command::New { file, label } => new(&file, &label),
        Command::Add { file, shape, name } => add(&file, shape, name.as_deref()),
        Command::Set { file, object, property, value } => set(&file, &object, &property, &value),
        Command::Get { file, object, property } => get(&file, &object, &property),
        Command::List { file, json } => list(&file, json),
        Command::Recompute { file, json } => recompute(&file, json),
        Command::Info { file } => info(&file),
        Command::Stats { file } => stats(&file),
        Command::Export { file, output, view } => export(&file, &output, &view),
        Command::Graph { file, output } => graph(&file, &output),
    }
}

/// Load-or-die helper. `must_exist` distinguishes "cannot read" from
/// "must exist for this command".
fn load(file: &str) -> Result<(Document, fc_io::LoadReport), String> {
    todo!()
}

fn save(file: &str, doc: &Document) -> Result<(), String> {
    todo!()
}

fn new(file: &str, label: &str) -> Result<(), String> {
    todo!()
}

fn add(file: &str, shape: AddShape, name: Option<&str>) -> Result<(), String> {
    todo!()
}

fn set(file: &str, object: &str, property: &str, value: &str) -> Result<(), String> {
    todo!()
}

fn get(file: &str, object: &str, property: &str) -> Result<(), String> {
    todo!()
}

fn list(file: &str, json: bool) -> Result<(), String> {
    todo!()
}

fn recompute(file: &str, json: bool) -> Result<(), String> {
    todo!()
}

fn info(file: &str) -> Result<(), String> {
    todo!()
}

fn stats(file: &str) -> Result<(), String> {
    todo!()
}

fn export(file: &str, output: &str, view: &str) -> Result<(), String> {
    todo!()
}

fn graph(file: &str, output: &str) -> Result<(), String> {
    todo!()
}
