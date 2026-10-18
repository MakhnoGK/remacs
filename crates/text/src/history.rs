use crate::piece_tree::PieceTree;

/// A text state to return to, and where the edit that left it happened.
pub(crate) struct Revision {
    pub pieces: PieceTree,
    pub offset: usize,
}

/// Linear undo/redo over piece-tree snapshots. Edits between two boundaries undo as one
/// group, like Emacs undo boundaries set by the command loop.
#[derive(Default)]
pub(crate) struct History {
    undo: Vec<Revision>,
    redo: Vec<Revision>,
    group_open: bool,
}

impl History {
    pub fn record(&mut self, before: &PieceTree, offset: usize) {
        if !self.group_open {
            self.undo.push(Revision {
                pieces: before.clone(),
                offset,
            });
            self.group_open = true;
        }
        self.redo.clear();
    }

    /// The texts undo would go back to, the latest first.
    pub fn states(&self) -> impl Iterator<Item = &PieceTree> {
        self.undo.iter().rev().map(|revision| &revision.pieces)
    }

    pub fn add_boundary(&mut self) {
        self.group_open = false;
    }

    pub fn undo(&mut self, current: &PieceTree) -> Option<Revision> {
        self.add_boundary();
        step(&mut self.undo, &mut self.redo, current)
    }

    pub fn redo(&mut self, current: &PieceTree) -> Option<Revision> {
        self.add_boundary();
        step(&mut self.redo, &mut self.undo, current)
    }
}

fn step(from: &mut Vec<Revision>, to: &mut Vec<Revision>, current: &PieceTree) -> Option<Revision> {
    let revision = from.pop()?;
    to.push(Revision {
        pieces: current.clone(),
        offset: revision.offset,
    });
    Some(revision)
}
