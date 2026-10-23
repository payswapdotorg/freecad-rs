//! The parametric document: object graph + name registry, mirroring
//! FreeCAD `App::Document`.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::object::link_targets;
pub use crate::object::{DocumentObject, ObjectId, ObjectStatus};
use crate::property::Property;
use crate::recompute::RecomputeError;
use crate::transaction::{
    Change, HistoryEntry, JournalChange, Transaction, TransactionId, TransactionInfo,
    AUTO_TRANSACTION_LABEL, UNDO_STACK_CAP,
};

/// Flat, serializable view of one object (for CLI/JSON output).
#[derive(Debug, Clone)]
pub struct ObjectSummary {
    pub id: ObjectId,
    pub name: String,
    pub type_name: String,
    pub status: ObjectStatus,
}

/// A FreeCAD-style document: an ordered collection of typed, named,
/// linked objects that recompute dependencies-first, with an undo/redo
/// journal of transactions (FreeCAD `App::Document` + `App::Transaction`).
#[derive(Default)]
pub struct Document {
    label: String,
    creator: String,
    /// RFC 3339 UTC creation timestamp.
    created: String,
    /// Round-trip preservation bag (see [`crate::preservation`]): raw
    /// file data the model does not capture but the writer re-emits
    /// verbatim. Fills only on `.FCStd` load; API-created documents
    /// start empty.
    preservation: crate::preservation::DocumentPreservation,
    objects: BTreeMap<ObjectId, Box<dyn DocumentObject>>,
    /// Creation order for stable listings.
    order: Vec<ObjectId>,
    /// Unique name -> id.
    index: BTreeMap<String, ObjectId>,
    /// Objects absent from the document but known to the transaction
    /// engine (parked inside undo/redo records): the journal-restore
    /// fallback for name -> id resolution of changes that target an
    /// object which is currently not in the document.
    absent_index: BTreeMap<String, ObjectId>,
    next_id: ObjectId,
    /// Committed transactions, oldest first (undo pops the newest).
    undo_stack: Vec<Transaction>,
    /// Undone transactions, oldest-undone first (redo pops the top).
    redo_stack: Vec<Transaction>,
    /// Transaction currently collecting changes (`open_transaction`), or
    /// the temporary one auto-opened for a single unbracketed mutation.
    active_transaction: Option<Transaction>,
    next_transaction_id: TransactionId,
}

impl Document {
    /// New empty document. `created` is stamped here.
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            creator: String::new(),
            created: rfc3339_now(),
            preservation: crate::preservation::DocumentPreservation::default(),
            objects: BTreeMap::new(),
            order: Vec::new(),
            index: BTreeMap::new(),
            absent_index: BTreeMap::new(),
            next_id: ObjectId(1),
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            active_transaction: None,
            next_transaction_id: TransactionId(1),
        }
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    /// Set the document label. Auto-records a `LabelChn` change into the
    /// open transaction (or a temp one), so label changes undo cleanly.
    /// `Document::new` sets the initial label without recording.
    pub fn set_label(&mut self, label: &str) {
        let before = self.label.clone();
        self.label = label.to_string();
        let after = self.label.clone();
        self.record_change(Change::LabelChn { before, after });
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

    /// The document's round-trip preservation bag (see
    /// [`crate::preservation`]): loader-set, writer-consumed raw data.
    pub fn preservation(&self) -> &crate::preservation::DocumentPreservation {
        &self.preservation
    }

    /// Mutable access to the preservation bag (loader use).
    pub fn preservation_mut(&mut self) -> &mut crate::preservation::DocumentPreservation {
        &mut self.preservation
    }

    /// Insert an object. The document assigns the id, keeps the object's
    /// current name as a *base*, and may suffix it for uniqueness
    /// (`Box`, `Box001`, `Box002`, FreeCAD-style). The final name is
    /// written back onto the object. Records a `New` change (into the
    /// open transaction, or a temp one — AutoTransaction semantics).
    pub fn add(&mut self, mut object: Box<dyn DocumentObject>) -> ObjectId {
        let id = self.next_id;
        self.next_id = ObjectId(id.0 + 1);
        let base = object.name().to_string();
        let name = self.unique_name(&base);
        object.set_name(&name);
        self.objects.insert(id, object);
        self.order.push(id);
        self.index.insert(name, id);
        self.record_change(Change::New { id, object: None });
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
        if let Some((_, order, object)) = self.raw_remove(id) {
            // Park the removed object in the transaction record so an
            // undo can restore it with its properties intact.
            self.record_change(Change::Del {
                id,
                order,
                object: Some(object),
            });
        }
        Ok(())
    }

    /// Set one property and mark the object **and its transitive
    /// dependents** as touched. This is the trigger of the parametric
    /// cascade. A property with an expression bound is read-only until
    /// the binding is cleared (FreeCAD behavior). Records a `Chn` change
    /// with before/after snapshots (into the open transaction, or a temp
    /// one — AutoTransaction semantics).
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
        let before = self
            .objects
            .get(&id)
            .and_then(|object| object.properties().get(key).cloned());
        if let Some(object) = self.objects.get_mut(&id) {
            object
                .properties_mut()
                .insert(key.to_string(), value.clone());
        }
        let after = Some(value);
        self.record_change(Change::PropertyChn {
            id,
            key: key.to_string(),
            before,
            after,
        });
        self.touch_cascade(id);
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
        self.touch_cascade(id);
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
        self.touch_cascade(id);
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Transactions: open/close, undo/redo, history — FreeCAD's
    // `App::Document` + `App::Transaction` AutoTransaction semantics.
    // -----------------------------------------------------------------------

    /// Start an explicit editing step: every subsequent mutation records
    /// into this transaction until [`Document::close_transaction`]
    /// (FreeCAD `openTransaction`). An already-open transaction is
    /// committed first, like FreeCAD closing the current one.
    pub fn open_transaction(&mut self, label: Option<&str>) {
        if self.active_transaction.is_some() {
            self.close_transaction();
        }
        let transaction = Transaction::new(self.next_transaction_id, label.map(str::to_string));
        self.next_transaction_id = TransactionId(self.next_transaction_id.0 + 1);
        self.active_transaction = Some(transaction);
    }

    /// Commit the open transaction (FreeCAD `commitTransaction`): push it
    /// onto the undo stack, **clear the redo stack** — a new editing step
    /// invalidates the undone future — and drop the oldest transaction
    /// beyond [`UNDO_STACK_CAP`]. An empty transaction is discarded: a
    /// step that changed nothing leaves nothing to undo.
    pub fn close_transaction(&mut self) {
        if let Some(transaction) = self.active_transaction.take() {
            if !transaction.is_empty() {
                self.commit(transaction);
            }
        }
    }

    /// Roll back the latest transaction: pop it off the undo stack, apply
    /// its changes backward, park it on the redo stack. `Ok(None)` when
    /// there is nothing to undo. Undoing marks the affected objects (and
    /// their transitive dependents) `Touched`, so the next
    /// [`Document::recompute_all`] re-evaluates them.
    ///
    /// Undoing is an error only while a transaction is open, or when the
    /// journal is inconsistent with the document (a record that does not
    /// match reality); in the latter case the broken transaction is
    /// dropped — FreeCAD's history cannot half-exist.
    pub fn undo(&mut self) -> Result<Option<TransactionInfo>, String> {
        self.ensure_no_open_transaction("undo")?;
        let Some(mut transaction) = self.undo_stack.pop() else {
            return Ok(None);
        };
        transaction.apply_backward(self)?;
        let info = transaction.info();
        self.redo_stack.push(transaction);
        Ok(Some(info))
    }

    /// Re-apply the latest undone transaction — symmetric to
    /// [`Document::undo`] (pop off the redo stack, apply forward, park on
    /// the undo stack, mark dependents `Touched`).
    pub fn redo(&mut self) -> Result<Option<TransactionInfo>, String> {
        self.ensure_no_open_transaction("redo")?;
        let Some(mut transaction) = self.redo_stack.pop() else {
            return Ok(None);
        };
        transaction.apply_forward(self)?;
        let info = transaction.info();
        self.undo_stack.push(transaction);
        Ok(Some(info))
    }

    /// Number of transactions currently on the undo stack.
    pub fn undo_count(&self) -> usize {
        self.undo_stack.len()
    }

    /// Number of transactions currently on the redo stack.
    pub fn redo_count(&self) -> usize {
        self.redo_stack.len()
    }

    /// Undo-stack entries, oldest committed first.
    pub fn undo_history(&self) -> Vec<HistoryEntry> {
        self.undo_stack
            .iter()
            .map(Transaction::history_entry)
            .collect()
    }

    /// Redo-stack entries in redo order: the entry [`Document::redo`]
    /// would apply first, then the ones after it.
    pub fn redo_history(&self) -> Vec<HistoryEntry> {
        self.redo_stack
            .iter()
            .rev()
            .map(Transaction::history_entry)
            .collect()
    }

    /// Forget all undo/redo history and release every object parked in
    /// it. The CLI calls this right after loading a `.FCStd`, so the
    /// loader's own object insertions do not surface as undoable steps;
    /// history starts with the first user edit of the session.
    pub fn clear_history(&mut self) {
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.absent_index.clear();
    }

    /// Restore a previously committed — and still un-undone — transaction
    /// onto the **undo stack** without applying it: the caller guarantees
    /// the document already reflects its effects (the CLI, after loading a
    /// `.FCStd` that contains them). This is how an external journal
    /// rehydrates the in-memory history. Respects [`UNDO_STACK_CAP`].
    pub fn restore_undo_transaction(
        &mut self,
        label: Option<&str>,
        changes: Vec<JournalChange>,
    ) -> Result<(), String> {
        let transaction = self.rebuild_transaction(label, changes)?;
        self.undo_stack.push(transaction);
        if self.undo_stack.len() > UNDO_STACK_CAP {
            self.undo_stack.remove(0);
        }
        Ok(())
    }

    /// Restore a previously committed **and undone** transaction onto the
    /// **redo stack** without applying it (the document already reflects
    /// having undone it). Restored redo transactions are re-queued: pass
    /// them in commit order and [`Document::redo`] replays them
    /// chronologically. Commit order also guarantees that an undone
    /// addition registers its (absent) object before any later property
    /// change of that same object resolves.
    pub fn restore_redo_transaction(
        &mut self,
        label: Option<&str>,
        changes: Vec<JournalChange>,
    ) -> Result<(), String> {
        let transaction = self.rebuild_transaction(label, changes)?;
        self.redo_stack.insert(0, transaction);
        Ok(())
    }

    /// Refuse while an explicit transaction is collecting changes.
    fn ensure_no_open_transaction(&self, action: &str) -> Result<(), String> {
        if self.active_transaction.is_some() {
            return Err(format!(
                "cannot {action}: a transaction is still open (close it first)"
            ));
        }
        Ok(())
    }

    /// Commit a transaction onto the undo stack: clear the redo stack,
    /// push, drop the oldest beyond [`UNDO_STACK_CAP`].
    fn commit(&mut self, transaction: Transaction) {
        self.redo_stack.clear();
        self.undo_stack.push(transaction);
        if self.undo_stack.len() > UNDO_STACK_CAP {
            self.undo_stack.remove(0);
        }
    }

    /// The AutoTransaction core: a mutation records into the open
    /// transaction, or — when none is open — into a temp transaction that
    /// commits immediately (label [`AUTO_TRANSACTION_LABEL`]), so
    /// unbracketed mutations are still individually undoable.
    fn record_change(&mut self, change: Change) {
        if let Some(transaction) = self.active_transaction.as_mut() {
            transaction.push(change);
            return;
        }
        let mut transaction = Transaction::new(
            self.next_transaction_id,
            Some(AUTO_TRANSACTION_LABEL.to_string()),
        );
        self.next_transaction_id = TransactionId(self.next_transaction_id.0 + 1);
        transaction.push(change);
        self.commit(transaction);
    }

    /// Turn serialized journal changes into a real transaction: names
    /// resolve against the document index first, then against the
    /// parked-object registry ([`JournalChange::AddedAbsent`] registers
    /// its rebuilt object so later property changes of that same,
    /// still-absent object resolve too). Transaction ids are handed out
    /// fresh — they are process-local, like FreeCAD's.
    fn rebuild_transaction(
        &mut self,
        label: Option<&str>,
        changes: Vec<JournalChange>,
    ) -> Result<Transaction, String> {
        let mut transaction = Transaction::new(self.next_transaction_id, label.map(str::to_string));
        self.next_transaction_id = TransactionId(self.next_transaction_id.0 + 1);
        for change in changes {
            let change = match change {
                JournalChange::Added { object } => Change::New {
                    id: self.resolve_id(&object).ok_or_else(|| {
                        format!("journal mismatch: added object '{object}' is not in the document")
                    })?,
                    object: None,
                },
                JournalChange::AddedAbsent { object } => {
                    let name = object.name().to_string();
                    let id = self.next_id;
                    self.next_id = ObjectId(id.0 + 1);
                    self.absent_index.insert(name, id);
                    Change::New {
                        id,
                        object: Some(object),
                    }
                }
                JournalChange::Property {
                    object,
                    key,
                    before,
                    after,
                } => Change::PropertyChn {
                    id: self.resolve_id(&object).ok_or_else(|| {
                        format!(
                            "journal mismatch: changed object '{object}' is not in the document"
                        )
                    })?,
                    key,
                    before,
                    after,
                },
                JournalChange::Label { before, after } => Change::LabelChn { before, after },
            };
            transaction.push(change);
        }
        Ok(transaction)
    }

    /// Name -> id through the document index, falling back to the
    /// parked-object registry of objects held inside undo/redo records.
    fn resolve_id(&self, name: &str) -> Option<ObjectId> {
        self.index
            .get(name)
            .copied()
            .or_else(|| self.absent_index.get(name).copied())
    }

    /// Remove an object by id, bypassing the dependent check and the
    /// transaction recording — the undo/redo engine's primitive (undo of
    /// an addition, redo of a removal) and [`Document::remove`]'s tail.
    /// Returns the name, the creation-order position and the object
    /// itself (to be parked). Objects that linked to the removed one are
    /// marked `Touched`, so the next recompute re-evaluates them and
    /// honestly reports the dangling link.
    pub(crate) fn raw_remove(
        &mut self,
        id: ObjectId,
    ) -> Option<(String, usize, Box<dyn DocumentObject>)> {
        let name = self.objects.get(&id)?.name().to_string();
        let dependents = self.dependents_of(&name);
        let object = self.objects.remove(&id)?;
        let order = self
            .order
            .iter()
            .position(|&other| other == id)
            .unwrap_or(self.order.len());
        self.order.retain(|&other| other != id);
        self.index.remove(&name);
        self.absent_index.insert(name.clone(), id);
        for dependent in dependents {
            if let Some(&dependent_id) = self.index.get(&dependent) {
                if let Some(object) = get_mut(self, dependent_id) {
                    object.set_status(ObjectStatus::Touched);
                }
            }
        }
        Some((name, order, object))
    }

    /// Insert an object at a known id and creation-order position — the
    /// inverse of [`Document::raw_remove`], bypassing the transaction
    /// recording. Renames the object FreeCAD-style when its name got
    /// taken in the meantime.
    pub(crate) fn raw_insert(
        &mut self,
        id: ObjectId,
        mut object: Box<dyn DocumentObject>,
        order: usize,
    ) -> Result<(), String> {
        if self.objects.contains_key(&id) {
            return Err(format!(
                "journal mismatch: object id {} already exists",
                id.0
            ));
        }
        let mut name = object.name().to_string();
        if self.index.contains_key(&name) {
            name = self.unique_name(&name);
            object.set_name(&name);
        }
        self.index.insert(name.clone(), id);
        self.absent_index.remove(&name);
        self.objects.insert(id, object);
        let order = order.min(self.order.len());
        self.order.insert(order, id);
        Ok(())
    }

    /// Mark the object and its transitive dependents `Touched` (by id):
    /// they re-execute on the next [`Document::recompute_all`]. The
    /// shared tail of every mutation and undo/redo application.
    pub(crate) fn touch_cascade(&mut self, id: ObjectId) {
        let Some(name) = self
            .objects
            .get(&id)
            .map(|object| object.name().to_string())
        else {
            return;
        };
        let dependents = self.dependents_of(&name);
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
    }

    /// Set one property by id, bypassing the expression check, the touch
    /// cascade and the transaction recording — the undo/redo engine's
    /// primitive for restoring a `before`/`after` snapshot. `None`
    /// removes the property again (it did not exist in that snapshot);
    /// the caller marks the object touched.
    pub(crate) fn raw_set_property(&mut self, id: ObjectId, key: &str, value: Option<Property>) {
        if let Some(object) = self.objects.get_mut(&id) {
            match value {
                Some(value) => {
                    object.properties_mut().insert(key.to_string(), value);
                }
                None => {
                    object.properties_mut().remove(key);
                }
            }
        }
    }

    /// Set the document label without recording a change — used by the
    /// transaction engine when it applies a `LabelChn` backward/forward.
    pub(crate) fn set_label_internal(&mut self, label: &str) {
        self.label = label.to_string();
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
