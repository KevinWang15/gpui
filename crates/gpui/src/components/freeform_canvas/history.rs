//! Bounded transactional undo/redo history stack for [`super::FreeformCanvas`] documents.

use std::collections::VecDeque;

/// Bounded undo/redo history stack with transactional coalescing for continuous canvas gestures
/// (such as dragging a resize handle, resizing the canvas frame, or dragging a property slider).
///
/// Store your document snapshot type `T` (e.g. a struct holding placed items and
/// [`super::CanvasSize`]):
/// - Call [`CanvasHistory::record`] or [`CanvasHistory::record_if_changed`] around discrete edits
///   (`Move`, `DropNew`, `Remove`, `Cut`, `Paste`, `Duplicate`, `Align`, z-order changes).
/// - Call [`CanvasHistory::begin_transaction`] on continuous drag updates (`committed: false`)
///   and [`CanvasHistory::commit_transaction`] when the gesture finishes (`committed: true`), so
///   an entire drag-resize collapses into a single undo step.
#[derive(Clone, Debug)]
pub struct CanvasHistory<T> {
    undo_stack: VecDeque<T>,
    redo_stack: VecDeque<T>,
    pending_transaction: Option<T>,
    max_entries: usize,
}

impl<T> Default for CanvasHistory<T> {
    fn default() -> Self {
        Self::new(100)
    }
}

impl<T> CanvasHistory<T> {
    /// Creates a new [`CanvasHistory`] retaining up to `max_entries` snapshots per stack (min `1`).
    pub fn new(max_entries: usize) -> Self {
        Self {
            undo_stack: VecDeque::new(),
            redo_stack: VecDeque::new(),
            pending_transaction: None,
            max_entries: max_entries.max(1),
        }
    }

    /// Maximum number of snapshots retained in each stack.
    pub fn max_entries(&self) -> usize {
        self.max_entries
    }

    /// Returns `true` if there is at least one snapshot available to undo.
    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    /// Returns `true` if there is at least one snapshot available to redo.
    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    /// Number of snapshots currently on the undo stack.
    pub fn undo_len(&self) -> usize {
        self.undo_stack.len()
    }

    /// Number of snapshots currently on the redo stack.
    pub fn redo_len(&self) -> usize {
        self.redo_stack.len()
    }

    /// Returns `true` while a continuous gesture transaction is in progress.
    pub fn has_pending_transaction(&self) -> bool {
        self.pending_transaction.is_some()
    }

    /// Drops any in-progress transaction without recording it.
    pub fn cancel_transaction(&mut self) {
        self.pending_transaction = None;
    }

    /// Clears all undo, redo, and pending transaction state.
    pub fn clear(&mut self) {
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.pending_transaction = None;
    }

    /// Returns the most recent snapshot on the undo stack without removing it.
    pub fn peek_undo(&self) -> Option<&T> {
        self.undo_stack.back()
    }

    /// Returns the most recent snapshot on the redo stack without removing it.
    pub fn peek_redo(&self) -> Option<&T> {
        self.redo_stack.back()
    }

    fn push_undo(&mut self, snapshot: T) {
        self.undo_stack.push_back(snapshot);
        while self.undo_stack.len() > self.max_entries {
            self.undo_stack.pop_front();
        }
    }

    fn push_redo(&mut self, snapshot: T) {
        self.redo_stack.push_back(snapshot);
        while self.redo_stack.len() > self.max_entries {
            self.redo_stack.pop_front();
        }
    }
}

impl<T: Clone + PartialEq> CanvasHistory<T> {
    /// Records `before` as an undo checkpoint and clears the redo stack.
    pub fn record(&mut self, before: T) {
        self.pending_transaction = None;
        self.push_undo(before);
        self.redo_stack.clear();
    }

    /// Records `before` only if it differs from `after`, returning `true` when recorded.
    pub fn record_if_changed(&mut self, before: T, after: &T) -> bool {
        self.pending_transaction = None;
        if &before == after {
            return false;
        }
        self.push_undo(before);
        self.redo_stack.clear();
        true
    }

    /// Captures `before` at the start of a continuous gesture if no transaction is active yet.
    pub fn begin_transaction(&mut self, before: T) {
        if self.pending_transaction.is_none() {
            self.pending_transaction = Some(before);
        }
    }

    /// Commits the pending transaction onto the undo stack if the document changed relative to
    /// `current`. Returns `true` when an undo step was recorded.
    pub fn commit_transaction(&mut self, current: &T) -> bool {
        let Some(before) = self.pending_transaction.take() else {
            return false;
        };
        if &before == current {
            return false;
        }
        self.push_undo(before);
        self.redo_stack.clear();
        true
    }

    /// Pops the previous snapshot from the undo stack and pushes `current` onto the redo stack.
    pub fn undo(&mut self, current: T) -> Option<T> {
        self.pending_transaction = None;
        let prev = self.undo_stack.pop_back()?;
        self.push_redo(current);
        Some(prev)
    }

    /// Pops the next snapshot from the redo stack and pushes `current` onto the undo stack.
    pub fn redo(&mut self, current: T) -> Option<T> {
        self.pending_transaction = None;
        let next = self.redo_stack.pop_back()?;
        self.push_undo(current);
        Some(next)
    }
}
