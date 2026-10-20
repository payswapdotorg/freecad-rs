//! Transactions — the undo/redo journal, mirroring FreeCAD's
//! `App::Transaction` family.
//!
//! FreeCAD's undo model, distilled: a **transaction** records every change
//! made to a document during one editing step. Applying a transaction
//! *forward* is a redo, applying it *backward* is an undo. Each transaction
//! carries a monotonic id and an optional label, and holds one change
//! record per affected object (FreeCAD `TransactionObject` variants):
//!
//! - [`Change::New`] — the object was added. Undo removes it, redo re-adds
//!   it. While the object is *absent* (its addition has been undone), the
//!   object itself is parked inside the record.
//! - [`Change::Del`] — the object was removed. Undo re-adds it (at its
//!   original creation-order position), redo removes it again. While the
//!   object is *present* (its removal has been undone), the record parks
//!   nothing; while it is absent, it parks the object.
//! - [`Change::PropertyChn`] — one property changed. The record stores a
//!   before/after snapshot of exactly that property (`None` snapshot = the
//!   property did not exist). Undo restores the before, redo the after.
//! - [`Change::LabelChn`] — the document label changed.
//!
//! Documents auto-open a *temp* transaction for unbracketed mutations
//! (see [`crate::Document`]), so undo/redo work with or without explicit
//! `open_transaction` / `close_transaction` bracketing — FreeCAD's
//! AutoTransaction semantics.

use crate::document::Document;
use crate::object::{DocumentObject, ObjectId};
use crate::property::Property;

/// Maximum number of transactions kept on the undo stack. When a new
/// transaction is committed onto a full stack, the **oldest** one is
/// dropped (objects parked inside a dropped record — an addition whose
/// undo was dropped, a removal that can no longer be re-added — are gone
/// for good, exactly like FreeCAD dropping old history).
pub const UNDO_STACK_CAP: usize = 100;

/// Label given to auto-opened ("temp") transactions — the transactions the
/// document opens on its own for a mutation that happened outside explicit
/// `open_transaction`/`close_transaction` bracketing.
pub const AUTO_TRANSACTION_LABEL: &str = "temp";

/// Monotonic per-document transaction id (FreeCAD transaction ids are
/// document-scoped). Assigned by the document starting at 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct TransactionId(pub u64);

impl std::fmt::Display for TransactionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// What `undo`/`redo` return: the transaction that was just applied
/// backward/forward.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransactionInfo {
    pub id: TransactionId,
    pub label: Option<String>,
    /// Number of change records in the transaction (after collapsing).
    pub changes: usize,
}

/// One entry of the undo/redo history listing ([`Document::undo_history`]
/// / [`Document::redo_history`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryEntry {
    pub id: TransactionId,
    pub label: Option<String>,
    /// Number of change records in the transaction (after collapsing).
    pub changes: usize,
}

/// A serialized description of one change, used to **rebuild** a
/// transaction stack from a persisted journal (the CLI's session log):
/// `Document::restore_undo_transaction` / `restore_redo_transaction`
/// turn these into real records without applying them — the caller
/// guarantees the changes match the document's current state.
pub enum JournalChange {
    /// `object` was added and is currently **in** the document (its undo
    /// will remove it). Looked up by name.
    Added { object: String },
    /// `object` was added, the addition was later undone, and the object is
    /// currently **absent** — rebuilt by the caller and parked here so a
    /// redo re-adds it (used to restore the redo stack).
    AddedAbsent { object: Box<dyn DocumentObject> },
    /// Property `key` of `object` changed `before` → `after`
    /// (`None` = the property did not exist).
    Property {
        object: String,
        key: String,
        before: Option<Property>,
        after: Option<Property>,
    },
    /// The document label changed `before` → `after`.
    Label { before: String, after: String },
}

/// One per-object change record (FreeCAD `TransactionObject` variant).
/// `New`/`Del` park the owned object while it is absent from the document
/// (see the module docs).
pub(crate) enum Change {
    /// Object was added; redo re-adds (from the parked slot), undo removes.
    New {
        id: ObjectId,
        object: Option<Box<dyn DocumentObject>>,
    },
    /// Object was removed; undo re-adds (at `order`), redo removes.
    Del {
        id: ObjectId,
        /// Creation-order position to restore the object at.
        order: usize,
        object: Option<Box<dyn DocumentObject>>,
    },
    /// One property changed; undo restores `before`, redo restores `after`.
    PropertyChn {
        id: ObjectId,
        key: String,
        before: Option<Property>,
        after: Option<Property>,
    },
    /// The document label changed.
    LabelChn { before: String, after: String },
}

/// A record of the changes made to a document during one editing step.
pub(crate) struct Transaction {
    id: TransactionId,
    label: Option<String>,
    changes: Vec<Change>,
}

impl Transaction {
    pub(crate) fn new(id: TransactionId, label: Option<String>) -> Self {
        Self {
            id,
            label,
            changes: Vec::new(),
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    pub(crate) fn info(&self) -> TransactionInfo {
        TransactionInfo {
            id: self.id,
            label: self.label.clone(),
            changes: self.changes.len(),
        }
    }

    pub(crate) fn history_entry(&self) -> HistoryEntry {
        HistoryEntry {
            id: self.id,
            label: self.label.clone(),
            changes: self.changes.len(),
        }
    }

    /// Append a change record. Setting the same property twice within one
    /// transaction collapses into a single record: the original `before`
    /// snapshot is kept, the `after` snapshot is updated (the intermediate
    /// values are unobservable once the step is done).
    pub(crate) fn push(&mut self, change: Change) {
        if let Change::PropertyChn { id, key, .. } = &change {
            for existing in self.changes.iter_mut() {
                if let Change::PropertyChn {
                    id: existing_id,
                    key: existing_key,
                    after: existing_after,
                    ..
                } = existing
                {
                    if existing_id == id && existing_key == key {
                        if let Change::PropertyChn { after, .. } = change {
                            *existing_after = after;
                        }
                        return;
                    }
                }
            }
        }
        self.changes.push(change);
    }

    /// Apply every change forward (= redo this transaction). The caller
    /// owns the stack bookkeeping.
    pub(crate) fn apply_forward(&mut self, doc: &mut Document) -> Result<(), String> {
        for change in self.changes.iter_mut() {
            change.redo(doc)?;
        }
        Ok(())
    }

    /// Apply every change backward (= undo this transaction), in reverse
    /// recording order so mutually consistent states are restored.
    pub(crate) fn apply_backward(&mut self, doc: &mut Document) -> Result<(), String> {
        for change in self.changes.iter_mut().rev() {
            change.undo(doc)?;
        }
        Ok(())
    }
}

impl Change {
    /// Apply this single change backward (undo direction).
    fn undo(&mut self, doc: &mut Document) -> Result<(), String> {
        match self {
            Change::New { id, object } => {
                // Remove the object from the document and park it in the
                // record so a redo can restore it byte-for-byte.
                let (_, _, removed) = doc.raw_remove(*id).ok_or_else(|| {
                    format!("journal inconsistency: object {id:?} is not in the document")
                })?;
                *object = Some(removed);
                Ok(())
            }
            Change::Del { id, order, object } => {
                // Re-add the parked object at its original creation-order
                // position; it recomputes on the next recompute_all.
                let parked = object.take().ok_or_else(|| {
                    format!("journal inconsistency: removed object {id:?} was not parked")
                })?;
                doc.raw_insert(*id, parked, *order)?;
                doc.touch_cascade(*id);
                Ok(())
            }
            Change::PropertyChn {
                id, key, before, ..
            } => {
                doc.raw_set_property(*id, key, before.clone());
                doc.touch_cascade(*id);
                Ok(())
            }
            Change::LabelChn { before, .. } => {
                doc.set_label_internal(before);
                Ok(())
            }
        }
    }

    /// Apply this single change forward (redo direction).
    fn redo(&mut self, doc: &mut Document) -> Result<(), String> {
        match self {
            Change::New { id, object } => {
                let parked = object.take().ok_or_else(|| {
                    format!("journal inconsistency: added object {id:?} was not parked")
                })?;
                doc.raw_insert(*id, parked, doc.creation_order().len())?;
                doc.touch_cascade(*id);
                Ok(())
            }
            Change::Del { id, object, .. } => {
                let (_, _, removed) = doc.raw_remove(*id).ok_or_else(|| {
                    format!("journal inconsistency: object {id:?} is not in the document")
                })?;
                *object = Some(removed);
                Ok(())
            }
            Change::PropertyChn { id, key, after, .. } => {
                doc.raw_set_property(*id, key, after.clone());
                doc.touch_cascade(*id);
                Ok(())
            }
            Change::LabelChn { after, .. } => {
                doc.set_label_internal(after);
                Ok(())
            }
        }
    }
}
