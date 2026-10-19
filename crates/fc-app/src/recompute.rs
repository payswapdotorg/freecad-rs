//! Dependency-driven recompute, mirroring FreeCAD's `Document::recompute`
//! + dependency graph.

use std::collections::BTreeSet;

use crate::document::{put_back, take_out, Document, ObjectId};
use crate::object::link_targets;

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
///
/// NOTE: FreeCAD tolerates dangling links (they simply never propagate a
/// recompute); we fail honestly instead of silently dropping the
/// dependency.
pub fn topological_order(doc: &Document) -> Result<Vec<ObjectId>, RecomputeError> {
    let mut done = BTreeSet::new();
    let mut path: Vec<ObjectId> = Vec::new();
    let mut order = Vec::new();
    for &id in doc.creation_order() {
        visit(doc, id, &mut done, &mut path, &mut order)?;
    }
    Ok(order)
}

/// Depth-first post-order visit: dependencies are appended before their
/// dependents. `path` is the current DFS chain; running into one of its
/// members closes a cycle, reported as object names in cycle order with
/// the first element repeated at the end (`A -> B -> C -> A`).
fn visit(
    doc: &Document,
    id: ObjectId,
    done: &mut BTreeSet<ObjectId>,
    path: &mut Vec<ObjectId>,
    order: &mut Vec<ObjectId>,
) -> Result<(), RecomputeError> {
    if done.contains(&id) {
        return Ok(());
    }
    if let Some(start) = path.iter().position(|&member| member == id) {
        let mut cycle: Vec<String> = path[start..]
            .iter()
            .map(|&member| object_name(doc, member))
            .collect();
        cycle.push(cycle[0].clone());
        return Err(RecomputeError::Cyclic { cycle });
    }
    let Some(object) = doc.get(id) else {
        // Not in the map (only possible while the recompute loop has taken
        // an object out for execution); treat as a leaf.
        return Ok(());
    };
    let name = object.name().to_string();
    let targets = link_targets(object);
    path.push(id);
    for target in targets {
        match doc.get_id(&target) {
            None => {
                return Err(RecomputeError::MissingLink {
                    object: name,
                    target,
                });
            }
            Some(target_id) => visit(doc, target_id, done, path, order)?,
        }
    }
    path.pop();
    done.insert(id);
    order.push(id);
    Ok(())
}

/// Name of `id`, or an empty string if it is not currently in the map.
fn object_name(doc: &Document, id: ObjectId) -> String {
    doc.get(id)
        .map(|object| object.name().to_string())
        .unwrap_or_default()
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
                return Err(RecomputeError::ExecuteFailed {
                    object: name,
                    reason,
                });
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
