//! The pending interactive rebase of a repository tab.

use imgit_git::{Author, RebaseStep, Sha};

/// One commit of a rebase plan: what to do with it, and what the list shows of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanRow {
    pub step: RebaseStep,
    pub summary: String,
    pub author: Author,
}

/// A pending interactive rebase.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RebasePlan {
    pub active: bool,
    pub base: Sha,
    pub base_label: String,
    /// The branch being rebased.
    pub head_label: String,
    /// Oldest first.
    pub steps: Vec<PlanRow>,
    /// The step the keyboard acts on.
    pub selected: Option<usize>,
    /// The step whose message is being edited (reword).
    pub editing: Option<usize>,
    /// The plan as it started, for Reset.
    pub original_steps: Vec<PlanRow>,
}

impl RebasePlan {
    /// Swaps steps `a` and `b` (with their summaries and authors).
    ///
    /// # Panics
    ///
    /// When `a` or `b` is not a step.
    pub fn swap_steps(&mut self, a: usize, b: usize) {
        self.steps.swap(a, b);
    }

    /// Back to the plan as it started, nothing selected or being edited.
    pub fn reset(&mut self) {
        self.steps.clone_from(&self.original_steps);
        self.selected = None;
        self.editing = None;
    }
}
