//! The parametric document: object graph + name registry, mirroring
//! FreeCAD `App::Document`.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::object::link_targets;
pub use crate::object::{DocumentObject, ObjectId, ObjectStatus};
use crate::property::Property;
use crate::recompute::RecomputeError;

/// Flat, serializable view of one object (for CLI/JSON output).
#[derive(Debug, Clone)]
pub struct ObjectSummary {
    pub id: ObjectId,
    pub name: String,
    pub type_name: String,
    pub status: ObjectStatus,
}

/// A FreeCAD-style document: an ordered collection of typed, named,
/// linked objects that recompute dependencies-first.
#[derive(Default)]
pub struct Document {
    label: String,
    creator: String,
    /// RFC 3339 UTC creation timestamp.
    created: String,
    objects: BTreeMap<ObjectId, Box<dyn DocumentObject>>,
    /// Creation order for stable listings.
    order: Vec<ObjectId>,
    /// Unique name -> id.
    index: BTreeMap<String, ObjectId>,
    next_id: ObjectId,
}

impl Document {
    /// New empty document. `created` is stamped here.
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            creator: String::new(),
            created: rfc3339_now(),
            objects: BTreeMap::new(),
            order: Vec::new(),
            index: BTreeMap::new(),
            next_id: ObjectId(1),
        }
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    pub fn set_label(&mut self, label: &str) {
        self.label = label.to_string();
    }

    pub fn creator(&self) -> &str {
        &self.creator
    }

    pub fn set_creator(&mut self, creator: &str) {
        self.creator = creator.to_string();
    }

    pub fn created(&self) -> &str {
        &self.created
    }

    /// Overwrite the creation stamp (used by the FCStd loader, which
    /// restores the persisted value).
    pub fn set_created(&mut self, created: String) {
        self.created = created;
    }

    /// Insert an object. The document assigns the id, keeps the object's
    /// current name as a *base*, and may suffix it for uniqueness
    /// (`Box`, `Box001`, `Box002`, FreeCAD-style). The final name is
    /// written back onto the object.
    pub fn add(&mut self, mut object: Box<dyn DocumentObject>) -> ObjectId {
        let id = self.next_id;
        self.next_id = ObjectId(id.0 + 1);
        let base = object.name().to_string();
        let name = self.unique_name(&base);
        object.set_name(&name);
        self.objects.insert(id, object);
        self.order.push(id);
        self.index.insert(name, id);
        id
    }

    /// The name `add` would assign for base `base` if it were added now:
    /// sanitized, used as-is when free, otherwise suffixed `001`, `002`,
    /// ... (3 digits, growing when needed).
    pub fn unique_name(&self, base: &str) -> String {
        let base = sanitize_name(base);
        if !self.index.contains_key(&base) {
            return base;
        }
        let mut counter: u64 = 1;
        loop {
            let candidate = format!("{base}{counter:03}");
            if !self.index.contains_key(&candidate) {
                return candidate;
            }
            counter += 1;
        }
    }

    pub fn object_count(&self) -> usize {
        self.objects.len()
    }

    /// Creation order of all object ids (crate-internal; used by the
    /// recompute graph walks for deterministic traversal).
    pub(crate) fn creation_order(&self) -> &[ObjectId] {
        &self.order
    }

    pub fn get(&self, id: ObjectId) -> Option<&dyn DocumentObject> {
        self.objects.get(&id).map(|object| &**object)
    }

    pub fn get_by_name(&self, name: &str) -> Option<&dyn DocumentObject> {
        self.get(*self.index.get(name)?)
    }

    pub fn get_id(&self, name: &str) -> Option<ObjectId> {
        self.index.get(name).copied()
    }

    /// Object summaries in creation order.
    pub fn summary(&self) -> Vec<ObjectSummary> {
        self.order
            .iter()
            .filter_map(|id| {
                let object = self.objects.get(id)?;
                Some(ObjectSummary {
                    id: *id,
                    name: object.name().to_string(),
                    type_name: object.type_name().to_string(),
                    status: object.status().clone(),
                })
            })
            .collect()
    }

    /// Link targets of `name` (from Link/LinkList properties).
    pub fn links_of(&self, name: &str) -> Vec<String> {
        match self.get_by_name(name) {
            Some(object) => link_targets(object),
            None => Vec::new(),
        }
    }

    /// Names of objects that (transitively) link to `name`, breadth-first
    /// from the direct dependents, creation-ordered within each level.
    /// Matched by link *names*, so dangling links cannot break the walk.
    pub fn dependents_of(&self, name: &str) -> Vec<String> {
        // Reverse adjacency: link target name -> direct dependents, in
        // creation order.
        let mut reverse: BTreeMap<String, Vec<ObjectId>> = BTreeMap::new();
        for id in &self.order {
            if let Some(object) = self.objects.get(id) {
                for target in link_targets(&**object) {
                    reverse.entry(target).or_default().push(*id);
                }
            }
        }
        let mut queue: VecDeque<ObjectId> = VecDeque::new();
        if let Some(direct) = reverse.get(name) {
            queue.extend(direct.iter().copied());
        }
        let mut visited = BTreeSet::new();
        let mut names = Vec::new();
        while let Some(id) = queue.pop_front() {
            if !visited.insert(id) {
                continue;
            }
            if let Some(object) = self.objects.get(&id) {
                let name = object.name().to_string();
                if let Some(direct) = reverse.get(&name) {
                    queue.extend(direct.iter().copied());
                }
                names.push(name);
            }
        }
        names
    }

    /// Remove an object. Fails when other objects still link to it
    /// (list them in the error) — parametric integrity, FreeCAD-style.
    /// Direct links suffice for the check: a transitive dependent is
    /// anchored by a chain whose first link is direct.
    pub fn remove(&mut self, name: &str) -> Result<(), String> {
        let Some(&id) = self.index.get(name) else {
            return Err(format!("object '{name}' not found"));
        };
        let mut dependents = Vec::new();
        for other_id in &self.order {
            if *other_id == id {
                continue;
            }
            if let Some(object) = self.objects.get(other_id) {
                if link_targets(&**object).iter().any(|target| target == name) {
                    dependents.push(object.name().to_string());
                }
            }
        }
        if !dependents.is_empty() {
            return Err(format!(
                "cannot remove '{name}': still linked by {}",
                dependents.join(", ")
            ));
        }
        self.objects.remove(&id);
        self.index.remove(name);
        self.order.retain(|&other| other != id);
        Ok(())
    }

    /// Set one property and mark the object **and its transitive
    /// dependents** as touched. This is the trigger of the parametric
    /// cascade. A property with an expression bound is read-only until
    /// the binding is cleared (FreeCAD behavior).
    pub fn set_property(&mut self, name: &str, key: &str, value: Property) -> Result<(), String> {
        let Some(&id) = self.index.get(name) else {
            return Err(format!("object '{name}' not found"));
        };
        if let Some(object) = self.objects.get(&id) {
            if object.expressions().contains_key(key) {
                return Err(format!(
                    "property '{key}' of '{name}' is bound to an expression \
                     (clear it first)"
                ));
            }
        }
        if let Some(object) = self.objects.get_mut(&id) {
            object.properties_mut().insert(key.to_string(), value);
        }
        let dependents = self.dependents_of(name);
        if let Some(object) = get_mut(self, id) {
            object.set_status(ObjectStatus::Touched);
        }
        for dependent in dependents {
            if let Some(&dependent_id) = self.index.get(&dependent) {
                if let Some(object) = get_mut(self, dependent_id) {
                    object.set_status(ObjectStatus::Touched);
                }
            }
        }
        Ok(())
    }

    /// Bind a property to an expression (FreeCAD's
    /// `setExpression`): the property is recomputed from `text` at
    /// every recompute, before the object executes, dependencies first.
    /// The expression must parse, and the property must exist and be a
    /// `Float` (the binding drives it). Touches the object and its
    /// dependents, like `set_property`.
    pub fn bind_expression(
        &mut self,
        name: &str,
        property: &str,
        text: &str,
    ) -> Result<(), String> {
        let Some(&id) = self.index.get(name) else {
            return Err(format!("object '{name}' not found"));
        };
        // Validate before mutating anything.
        crate::expression::parse(text).map_err(|err| err.to_string())?;
        if let Some(object) = self.objects.get(&id) {
            match object.properties().get(property) {
                Some(Property::Float(_)) => {}
                Some(other) => {
                    return Err(format!(
                        "property '{property}' of '{name}' is {} — only Float \
                         properties can carry expressions",
                        other.type_name()
                    ))
                }
                None => {
                    return Err(format!(
                        "object '{name}' has no property '{property}' to bind"
                    ))
                }
            }
        }
        if let Some(object) = self.objects.get_mut(&id) {
            if let Some(expressions) = object.expressions_mut() {
                expressions.insert(property.to_string(), text.to_string());
            }
        }
        let dependents = self.dependents_of(name);
        if let Some(object) = get_mut(self, id) {
            object.set_status(ObjectStatus::Touched);
        }
        for dependent in dependents {
            if let Some(&dependent_id) = self.index.get(&dependent) {
                if let Some(object) = get_mut(self, dependent_id) {
                    object.set_status(ObjectStatus::Touched);
                }
            }
        }
        Ok(())
    }

    /// Remove an expression binding; the property becomes settable again
    /// and keeps its last evaluated value. Touches the object and its
    /// dependents.
    pub fn clear_expression(&mut self, name: &str, property: &str) -> Result<(), String> {
        let Some(&id) = self.index.get(name) else {
            return Err(format!("object '{name}' not found"));
        };
        let removed = self
            .objects
            .get_mut(&id)
            .and_then(|object| object.expressions_mut())
            .and_then(|expressions| expressions.remove(property));
        if removed.is_none() {
            return Err(format!(
                "property '{property}' of '{name}' has no expression bound"
            ));
        }
        let dependents = self.dependents_of(name);
        if let Some(object) = get_mut(self, id) {
            object.set_status(ObjectStatus::Touched);
        }
        for dependent in dependents {
            if let Some(&dependent_id) = self.index.get(&dependent) {
                if let Some(object) = get_mut(self, dependent_id) {
                    object.set_status(ObjectStatus::Touched);
                }
            }
        }
        Ok(())
    }

    /// Recompute all touched objects, dependencies first.
    /// Returns the ids that were recomputed, in execution order.
    pub fn recompute_all(&mut self) -> Result<Vec<ObjectId>, RecomputeError> {
        crate::recompute::run(self)
    }

    /// Graphviz DOT rendering of the dependency graph. Edge `A -> B`
    /// means *A depends on B*. Nodes are emitted in creation order,
    /// edges deduplicated. A link to a name with no object still gets an
    /// edge (Graphviz creates a bare node) — the graph shows the link as
    /// declared, and `recompute` is the place that reports it broken.
    pub fn dependencies_dot(&self) -> String {
        let mut dot = String::from("digraph dependencies {\n");
        for id in &self.order {
            if let Some(object) = self.objects.get(id) {
                dot.push_str(&format!(
                    "    \"{}\" [label=\"{}\\n{}\"];\n",
                    escape_dot(object.name()),
                    escape_dot(object.name()),
                    escape_dot(object.type_name())
                ));
            }
        }
        let mut seen: BTreeSet<(String, String)> = BTreeSet::new();
        for id in &self.order {
            let Some(object) = self.objects.get(id) else {
                continue;
            };
            let source = object.name().to_string();
            for target in link_targets(&**object) {
                if seen.insert((source.clone(), target.clone())) {
                    dot.push_str(&format!(
                        "    \"{}\" -> \"{}\";\n",
                        escape_dot(&source),
                        escape_dot(&target)
                    ));
                }
            }
        }
        dot.push_str("}\n");
        dot
    }
}

/// Current time as an RFC 3339 UTC stamp with second resolution, e.g.
/// `2026-10-19T21:20:00Z`. Pure `std`: wall-clock seconds since the Unix
/// epoch converted to a civil date with Howard Hinnant's
/// `civil_from_days` algorithm.
fn rfc3339_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or(0);
    let days = secs.div_euclid(86_400);
    let secs_of_day = secs.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let (hour, minute, second) = (
        secs_of_day / 3_600,
        (secs_of_day % 3_600) / 60,
        secs_of_day % 60,
    );
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// Civil (year, month, day) from days since 1970-01-01.
/// Howard Hinnant, "chrono-Compatible Low-Level Date Algorithms"
/// (public domain / MIT), `civil_from_days`.
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let day = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let month = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    let year = era * 400 + yoe + i64::from(month <= 2);
    (year, month, day)
}

/// FreeCAD-style name sanitization (a distillation of FreeCAD's
/// `Base::Tools::getIdentifier` + unique-name handling): keep
/// `[A-Za-z0-9_]`, replace runs of other characters with a single `_`,
/// and default to `Object` when nothing remains.
fn sanitize_name(base: &str) -> String {
    let mut name = String::with_capacity(base.len());
    let mut last_inserted_underscore = false;
    for c in base.chars() {
        if c.is_ascii_alphanumeric() || c == '_' {
            name.push(c);
            last_inserted_underscore = false;
        } else if !last_inserted_underscore {
            name.push('_');
            last_inserted_underscore = true;
        }
    }
    if name.is_empty() {
        name.push_str("Object");
    }
    name
}

/// Escape a string for a Graphviz DOT quoted string body.
fn escape_dot(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            _ => escaped.push(c),
        }
    }
    escaped
}

/// Take an object out of the map so it can be executed with `&Document`
/// (aliasing-free). Not part of the public API.
pub(crate) fn take_out(doc: &mut Document, id: ObjectId) -> Option<Box<dyn DocumentObject>> {
    doc.objects.remove(&id)
}

/// Put a (possibly executed) object back. Not part of the public API.
pub(crate) fn put_back(doc: &mut Document, id: ObjectId, object: Box<dyn DocumentObject>) {
    doc.objects.insert(id, object);
}

/// Direct mutable access (used by the recompute loop and the loader).
pub(crate) fn get_mut(doc: &mut Document, id: ObjectId) -> Option<&mut dyn DocumentObject> {
    doc.objects
        .get_mut(&id)
        .map(|b| &mut **b as &mut dyn DocumentObject)
}
