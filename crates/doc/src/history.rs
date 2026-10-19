use serde::{Deserialize, Serialize};

use crate::{Document, Error, Op};

/// One undoable step: the operations an edit made, with a name for the
/// Undo menu, and the operations that undo it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Transaction {
    /// What the edit was, as Undo and Redo name it: "Typing", "Bold".
    pub name: String,
    /// The operations, in the order they were applied.
    pub ops: Vec<Op>,
    /// The operations that undo `ops`, to be applied in order.
    pub inverse: Vec<Op>,
}

impl Document {
    /// Applies `ops` as one transaction, all or none.
    pub fn transact(&mut self, name: &str, ops: Vec<Op>) -> Result<Transaction, Error> {
        let inverse = self.apply_all(&ops)?;
        Ok(Transaction {
            name: name.to_string(),
            ops,
            inverse,
        })
    }
}

/// The undo and redo stacks.
#[derive(Clone, Debug, Default)]
pub struct History {
    undo: Vec<Transaction>,
    redo: Vec<Transaction>,
}

impl History {
    /// Empty stacks.
    pub fn new() -> History {
        History::default()
    }

    /// Records a transaction just applied. A new edit clears Redo.
    pub fn push(&mut self, transaction: Transaction) {
        self.undo.push(transaction);
        self.redo.clear();
    }

    /// The name Undo would show, if there's anything to undo.
    pub fn undo_name(&self) -> Option<&str> {
        self.undo.last().map(|t| t.name.as_str())
    }

    /// The name Redo would show, if there's anything to redo.
    pub fn redo_name(&self) -> Option<&str> {
        self.redo.last().map(|t| t.name.as_str())
    }

    /// Undoes the last transaction on `document`. Returns its name, or
    /// `None` if there was nothing to undo.
    pub fn undo(&mut self, document: &mut Document) -> Result<Option<String>, Error> {
        let Some(t) = self.undo.pop() else {
            return Ok(None);
        };
        if let Err(e) = document.apply_all(&t.inverse) {
            self.undo.push(t);
            return Err(e);
        }
        let name = t.name.clone();
        self.redo.push(t);
        Ok(Some(name))
    }

    /// Redoes the last undone transaction. Returns its name, or `None` if
    /// there was nothing to redo.
    pub fn redo(&mut self, document: &mut Document) -> Result<Option<String>, Error> {
        let Some(t) = self.redo.pop() else {
            return Ok(None);
        };
        if let Err(e) = document.apply_all(&t.ops) {
            self.redo.push(t);
            return Err(e);
        }
        let name = t.name.clone();
        self.undo.push(t);
        Ok(Some(name))
    }
}
