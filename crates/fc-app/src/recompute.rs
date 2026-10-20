//! Dependency-driven recompute, mirroring FreeCAD's `Document::recompute`
//! + dependency graph.

use std::collections::BTreeSet;

use fc_base::quantity::Quantity;

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
        // Expression bindings run first (FreeCAD evaluates
        // PropertyExpressionEngine before execute): dependencies have
        // already recomputed, so their properties are current; the
        // object's own execute then sees the driven values.
        if let Err(reason) = evaluate_bindings(object.as_mut(), doc) {
            // FreeCAD marks the object Error when its expression engine
            // fails; the object goes back into the document with that
            // status (dropping it would corrupt the document).
            let name = object.name().to_string();
            object.set_status(crate::object::ObjectStatus::Error(reason.clone()));
            put_back(doc, id, object);
            return Err(RecomputeError::ExecuteFailed {
                object: name,
                reason,
            });
        }
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

/// Evaluate one object's expression bindings against the current
/// document state and write the results into the bound `Float`
/// properties.
///
/// The bound property is a plain float (FreeCAD `App::PropertyFloat`):
/// the expression must evaluate to a **dimensionless** quantity and its
/// value is stored. A unit-carrying result is an honest
/// [`RecomputeError::ExecuteFailed`] naming the unit — e.g.
/// `expression for 'Length' has unit mm; property is a plain float
/// (FreeCAD PropertyFloat rejects units)`. This deliberately rejects
/// what the folding engine used to accept (`2 * 5mm` binding 10):
/// dimensional honesty is the point.
fn evaluate_bindings(object: &mut dyn crate::DocumentObject, doc: &Document) -> Result<(), String> {
    let bindings: Vec<(String, String)> = object
        .expressions()
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    if bindings.is_empty() {
        return Ok(());
    }
    // Self-references read the object's current (pre-execute) values.
    let self_values: std::collections::BTreeMap<String, f64> = object
        .properties()
        .iter()
        .filter_map(|(key, value)| match value {
            crate::Property::Float(v) => Some((key.clone(), *v)),
            _ => None,
        })
        .collect();
    for (property, text) in bindings {
        let expression = crate::expression::parse(&text)
            .map_err(|err| format!("expression for '{property}' does not parse: {err}"))?;
        let value = expression
            .eval(&|object: Option<&str>,
                    prop: &str|
             -> Result<Quantity, crate::expression::ExprError> {
                match object {
                    Some(object_name) => doc
                        .get_by_name(object_name)
                        .and_then(|target| match target.properties().get(prop) {
                            Some(crate::Property::Float(v)) => Some(Quantity::dimensionless(*v)),
                            _ => None,
                        })
                        .ok_or_else(|| crate::expression::ExprError::UnresolvableReference {
                            object: Some(object_name.to_string()),
                            property: prop.to_string(),
                        }),
                    None => self_values
                        .get(prop)
                        .copied()
                        .map(Quantity::dimensionless)
                        .ok_or_else(|| crate::expression::ExprError::UnresolvableReference {
                            object: None,
                            property: prop.to_string(),
                        }),
                }
            })
            .map_err(|err| format!("expression for '{property}' failed: {err}"))?;
        if !value.unit().is_dimensionless() {
            return Err(format!(
                "expression for '{property}' has unit {unit}; property is a plain float \
                 (FreeCAD PropertyFloat rejects units)",
                unit = value.unit(),
            ));
        }
        object
            .properties_mut()
            .insert(property.clone(), crate::Property::Float(value.value()));
    }
    Ok(())
}

pub(crate) fn is_touched(doc: &Document, id: ObjectId) -> bool {
    doc.get(id)
        .map(|o| matches!(o.status(), crate::object::ObjectStatus::Touched))
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::{ObjectStatus, PropertyContainer};
    use crate::Property;

    /// Minimal float-only document object for binding tests. fc-app
    /// ships no concrete object types (features live in fc-part), so
    /// the tests define their own double, like the integration tests.
    struct TestBlock {
        container: PropertyContainer,
    }

    impl TestBlock {
        fn new(name: &str, length: f64) -> Self {
            let mut container = PropertyContainer::new(name);
            container.set("Length", Property::Float(length));
            Self { container }
        }

        fn execute_impl(&mut self, _doc: &Document) -> Result<(), String> {
            Ok(())
        }
    }

    crate::impl_document_object!(TestBlock, "Test::Block");

    fn doc_with_blocks() -> Document {
        let mut doc = Document::new("binding-tests");
        doc.add(Box::new(TestBlock::new("Base", 30.0)));
        doc.add(Box::new(TestBlock::new("Stem", 1.0)));
        doc
    }

    fn float_of(doc: &Document, name: &str, property: &str) -> Option<f64> {
        match doc
            .get_by_name(name)
            .map(|object| object.properties().get(property))
        {
            Some(Some(crate::Property::Float(value))) => Some(*value),
            _ => None,
        }
    }

    #[test]
    fn binding_dimensionless_expression_binds_value() {
        let mut doc = doc_with_blocks();
        doc.bind_expression("Stem", "Length", "2 * Base.Length / 3")
            .expect("binding is valid");
        doc.recompute_all()
            .expect("dimensionless binding recomputes");
        assert_eq!(float_of(&doc, "Stem", "Length"), Some(20.0));
        assert!(matches!(
            doc.get_by_name("Stem").map(|o| o.status().clone()),
            Some(ObjectStatus::Valid)
        ));
    }

    #[test]
    fn binding_unit_result_is_rejected_honestly() {
        let mut doc = doc_with_blocks();
        // `2 * 5mm` used to bind 10 (folding engine); now it is an
        // honest rejection — the point of the work order.
        doc.bind_expression("Stem", "Length", "2 * 5mm")
            .expect("binding parses");
        let err = doc
            .recompute_all()
            .expect_err("unit-carrying result must be rejected");
        match err {
            RecomputeError::ExecuteFailed { object, reason } => {
                assert_eq!(object, "Stem");
                assert_eq!(
                    reason,
                    "expression for 'Length' has unit mm; property is a plain float \
                     (FreeCAD PropertyFloat rejects units)"
                );
            }
            other => panic!("expected ExecuteFailed, got {other:?}"),
        }
        // The object survives, marked Error, and the property keeps its
        // pre-binding value.
        let stem = doc
            .get_by_name("Stem")
            .expect("object survives the failure");
        assert!(matches!(
            stem.status(),
            ObjectStatus::Error(reason) if reason.contains("has unit mm")
        ));
        assert_eq!(float_of(&doc, "Stem", "Length"), Some(1.0));
    }

    #[test]
    fn binding_angle_result_is_rejected() {
        let mut doc = doc_with_blocks();
        doc.bind_expression("Stem", "Length", "asin(0.5)")
            .expect("binding parses");
        let err = doc
            .recompute_all()
            .expect_err("angle result must be rejected");
        match err {
            RecomputeError::ExecuteFailed { object, reason } => {
                assert_eq!(object, "Stem");
                assert_eq!(
                    reason,
                    "expression for 'Length' has unit deg; property is a plain float \
                     (FreeCAD PropertyFloat rejects units)"
                );
            }
            other => panic!("expected ExecuteFailed, got {other:?}"),
        }
    }

    #[test]
    fn binding_unit_mismatch_marks_object_error() {
        let mut doc = doc_with_blocks();
        // mm + deg: both parseable suffixes, incompatible dimensions.
        doc.bind_expression("Stem", "Length", "2mm + 30deg")
            .expect("binding parses");
        let err = doc.recompute_all().expect_err("mismatch must fail");
        assert!(
            err.to_string().contains("unit mismatch in '+'"),
            "honest mismatch error: {err}"
        );
        let stem = doc.get_by_name("Stem").expect("object survives");
        assert!(matches!(stem.status(), ObjectStatus::Error(_)));
        assert_eq!(float_of(&doc, "Stem", "Length"), Some(1.0));
    }

    #[test]
    fn binding_ratio_of_units_is_dimensionless() {
        let mut doc = doc_with_blocks();
        // A ratio of same-dimension quantities is dimensionless and
        // binds fine — units are honest, not banned.
        doc.bind_expression("Stem", "Length", "10mm / 2mm")
            .expect("binding is valid");
        doc.recompute_all().expect("recompute");
        assert_eq!(float_of(&doc, "Stem", "Length"), Some(5.0));
    }

    #[test]
    fn binding_parse_failure_marks_object_error_and_keeps_it() {
        let mut doc = doc_with_blocks();
        doc.bind_expression("Stem", "Length", "Base.Length")
            .expect("valid binding");
        doc.recompute_all().expect("first recompute");
        // bind_expression validates parsing, so a broken binding can
        // only arrive through persistence — smuggle one in directly.
        let stem_id = doc.get_id("Stem").expect("present");
        let mut stem = take_out(&mut doc, stem_id).expect("taken out");
        stem.expressions_mut()
            .expect("container-backed expressions")
            .insert("Length".to_string(), "2 +".to_string());
        // A loader would leave the object touched; mark it so the
        // recompute loop actually visits it.
        stem.set_status(ObjectStatus::Touched);
        put_back(&mut doc, stem_id, stem);
        let err = doc.recompute_all().expect_err("broken binding must fail");
        assert!(err.to_string().contains("does not parse"), "{err}");
        // The object is back in the document with Error status (this
        // used to drop the object from the document entirely).
        let stem = doc
            .get_by_name("Stem")
            .expect("object survives the failure");
        assert!(matches!(
            stem.status(),
            ObjectStatus::Error(reason) if reason.contains("does not parse")
        ));
    }
}
