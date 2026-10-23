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

    /// History of these stacks, each oldest first, as [`History::stacks`]
    /// gives them: for history kept elsewhere, as a recorded session
    /// keeps it.
    pub fn from_stacks(undo: Vec<Transaction>, redo: Vec<Transaction>) -> History {
        History { undo, redo }
    }

    /// The transactions on Undo and on Redo, each oldest first, so the
    /// last of each is the next undone or redone.
    pub fn stacks(&self) -> (&[Transaction], &[Transaction]) {
        (&self.undo, &self.redo)
    }

    /// The name Undo would show, if there's anything to undo.
    pub fn undo_name(&self) -> Option<&str> {
        self.undo.last().map(|t| t.name.as_str())
    }

    /// The name Redo would show, if there's anything to redo.
    pub fn redo_name(&self) -> Option<&str> {
        self.redo.last().map(|t| t.name.as_str())
    }

    /// Adds `transaction`, just applied, to the last one on Undo, so one
    /// undo takes back both: for typing, which records each keystroke but
    /// undoes the run of them. Clears Redo. Returns whether there was a
    /// transaction to add to; if not, nothing changes.
    pub fn continue_last(&mut self, transaction: Transaction) -> bool {
        let Some(last) = self.undo.last_mut() else {
            return false;
        };
        last.ops.extend(transaction.ops);
        let mut inverse = transaction.inverse;
        inverse.append(&mut last.inverse);
        last.inverse = inverse;
        self.redo.clear();
        true
    }

    /// Moves the last transaction from Undo to Redo without applying it:
    /// for rebuilding history from a record of what was done. Returns
    /// whether there was one.
    pub fn replay_undo(&mut self) -> bool {
        let Some(t) = self.undo.pop() else {
            return false;
        };
        self.redo.push(t);
        true
    }

    /// Moves the last transaction from Redo to Undo without applying it;
    /// the mirror of [`History::replay_undo`].
    pub fn replay_redo(&mut self) -> bool {
        let Some(t) = self.redo.pop() else {
            return false;
        };
        self.undo.push(t);
        true
    }

    /// Undoes the last transaction on `document`. Returns it, for the file's
    /// record, or `None` if there was nothing to undo.
    pub fn undo(&mut self, document: &mut Document) -> Result<Option<&Transaction>, Error> {
        let Some(t) = self.undo.pop() else {
            return Ok(None);
        };
        if let Err(e) = document.apply_all(&t.inverse) {
            self.undo.push(t);
            return Err(e);
        }
        self.redo.push(t);
        Ok(self.redo.last())
    }

    /// Redoes the last undone transaction. Returns it, or `None` if there
    /// was nothing to redo.
    pub fn redo(&mut self, document: &mut Document) -> Result<Option<&Transaction>, Error> {
        let Some(t) = self.redo.pop() else {
            return Ok(None);
        };
        if let Err(e) = document.apply_all(&t.ops) {
            self.redo.push(t);
            return Err(e);
        }
        self.undo.push(t);
        Ok(self.undo.last())
    }
}
