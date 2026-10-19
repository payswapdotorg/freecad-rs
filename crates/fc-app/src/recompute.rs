//! Dependency-driven recompute, mirroring FreeCAD's `Document::recompute`
//! + dependency graph.

use crate::document::{put_back, take_out, Document, ObjectId};

/// Recompute failure modes.
#[derive(Debug, Clone, PartialEq)]
pub enum RecomputeError {
    /// The link graph contains a cycle; carries the offending objects in
    /// cycle order.
    Cyclic { cycle: Vec<String> },
    /// An object's `execute` failed. The object is left in `Error` status.
    ExecuteFailed { object: String, reason: String },
    /// A Link property points at a name that does not exist.
    MissingLink { object: String, target: String },
}

impl std::fmt::Display for RecomputeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RecomputeError::Cyclic { cycle } => {
                write!(f, "cyclic dependency: {}", cycle.join(" -> "))
            }
            RecomputeError::ExecuteFailed { object, reason } => {
                write!(f, "recompute of '{object}' failed: {reason}")
            }
            RecomputeError::MissingLink { object, target } => {
                write!(f, "object '{object}' links to missing object '{target}'")
            }
        }
    }
}

impl std::error::Error for RecomputeError {}

/// Dependency-first execution order (DFS post-order).
/// Verifies every link resolves (`MissingLink`) and that the graph is
/// acyclic (`Cyclic`).
pub fn topological_order(doc: &Document) -> Result<Vec<ObjectId>, RecomputeError> {
    todo!()
}

/// Execute every `Touched` object in topological order.
///
/// Semantics (FreeCAD-style):
/// 1. Resolve the order once; fail early on cycles/missing links.
/// 2. Skip objects already `Valid`.
/// 3. Take each touched object out of the map (aliasing-free
///    `execute(&Document)`), run it, then put it back.
/// 4. First failure marks that object `Error` and aborts the run.
pub(crate) fn run(doc: &mut Document) -> Result<Vec<ObjectId>, RecomputeError> {
    let order = topological_order(doc)?;
    let mut recomputed = Vec::new();
    for id in order {
        let touched = crate::recompute::is_touched(doc, id);
        if !touched {
            continue;
        }
        let mut object = match take_out(doc, id) {
            Some(o) => o,
            None => continue,
        };
        match object.execute(doc) {
            Ok(()) => {
                object.set_status(crate::object::ObjectStatus::Valid);
                put_back(doc, id, object);
                recomputed.push(id);
            }
            Err(reason) => {
                let name = object.name().to_string();
                object.set_status(crate::object::ObjectStatus::Error(reason.clone()));
                put_back(doc, id, object);
                return Err(RecomputeError::ExecuteFailed { object: name, reason });
            }
        }
    }
    Ok(recomputed)
}

pub(crate) fn is_touched(doc: &Document, id: ObjectId) -> bool {
    doc.get(id)
        .map(|o| matches!(o.status(), crate::object::ObjectStatus::Touched))
        .unwrap_or(false)
}
