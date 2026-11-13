//! The graph's column / edge assignment: lane reservation per row, then the starting, ending and
//! pass-through edges of each column.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::types::{Commit, NodeType, Sha};

/// A line of the graph, heading down to `parent_sha`'s row.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Edge {
    pub parent_sha: Sha,
    /// Of the child row that started the edge.
    pub node_type: NodeType,
}

/// The edges crossing one lane (column) of one row.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ColumnEdges {
    /// Leaves this row's node downward (own column, or a merge parent's).
    pub starting: Option<Edge>,
    /// Arrives from above into this row's node.
    pub ending: Option<Edge>,
    /// Crosses the whole row.
    pub pass_through: Option<Edge>,
}

impl ColumnEdges {
    /// The edge that carries on into the next row: a solid line beats a dashed one in the same lane,
    /// pass-throughs beat starts.
    fn continued(&self) -> Option<&Edge> {
        let candidates = || [&self.pass_through, &self.starting].into_iter().flatten();
        candidates()
            .find(|edge| !edge.node_type.is_dashed())
            .or_else(|| candidates().next())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RowLayout {
    pub column: usize,
    /// By column.
    pub edges: BTreeMap<usize, ColumnEdges>,
    pub edge_column_max: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Layout {
    /// Parallel to the input commits.
    pub rows: Vec<RowLayout>,
    /// Highest column touched by any edge.
    pub max_columns: usize,
}

/// The lane held for a commit by a child that has already been placed.
#[derive(Debug, Clone, Copy)]
struct Reservation {
    /// Of the child that reserved it.
    node_type: NodeType,
    newest_date: Option<i64>,
    column: usize,
}

/// The per-pass bookkeeping: which lanes are taken, which parent has a lane reserved for it, and which
/// lanes a row frees when it is finally reached.
struct Assigner<'a> {
    pinned: &'a HashSet<Sha>,
    columns_used: HashSet<usize>,
    reserver_by_sha: HashMap<Sha, Reservation>,
    columns_to_free_when_found: HashMap<Sha, Vec<usize>>,
    has_merge_node_child: HashSet<Sha>,
}

impl<'a> Assigner<'a> {
    fn new(pinned: &'a HashSet<Sha>) -> Self {
        Self {
            pinned,
            columns_used: HashSet::new(),
            reserver_by_sha: HashMap::new(),
            columns_to_free_when_found: HashMap::new(),
            has_merge_node_child: HashSet::new(),
        }
    }

    /// Takes the lowest free lane; column 0 belongs to the pinned chain when there is one.
    fn free_column(&mut self) -> usize {
        let first = usize::from(!self.pinned.is_empty());
        let column = (first..)
            .find(|column| !self.columns_used.contains(column))
            .expect("there are more columns than rows");
        self.columns_used.insert(column);
        column
    }

    fn assign(&mut self, row: &Commit) -> usize {
        self.has_merge_node_child.remove(&row.sha);

        if let Some(columns) = self.columns_to_free_when_found.get(&row.sha) {
            for column in columns {
                self.columns_used.remove(column);
            }
        }

        let own_reservation = self.reserver_by_sha.get(&row.sha).copied();
        let column = if self.pinned.contains(&row.sha) {
            self.columns_used.insert(0);
            0
        } else if let Some(own) = own_reservation {
            self.reserver_by_sha.remove(&row.sha);
            own.column
        } else {
            self.free_column()
        };

        for (index, parent) in row.parents.iter().enumerate() {
            if row.node_type == NodeType::Merge {
                self.has_merge_node_child.insert(parent.clone());
            }
            match self.reserver_by_sha.get(parent).copied() {
                Some(theirs) if index == 0 && theirs.column != column => {
                    // The first parent already has a lane: either pull it into our lane (and free its old
                    // one when it is reached) or let our lane end at it.
                    let own_date = own_reservation.and_then(|own| own.newest_date);
                    let stash_displaced = theirs.node_type == NodeType::Stash
                        && row.node_type != NodeType::Stash
                        && matches!((own_date, theirs.newest_date), (Some(ours), Some(stashed)) if ours > stashed);
                    let pull = (theirs.column > column || stash_displaced)
                        && !self.has_merge_node_child.contains(parent);
                    if pull {
                        self.reserver_by_sha.insert(
                            parent.clone(),
                            Reservation {
                                node_type: row.node_type,
                                newest_date: own_date,
                                column,
                            },
                        );
                    }
                    self.columns_to_free_when_found
                        .entry(parent.clone())
                        .or_default()
                        .push(if pull { theirs.column } else { column });
                }
                Some(_) => {}
                None => {
                    let newest_date = match own_reservation {
                        Some(own) if own.column == column => own.newest_date,
                        _ => Some(row.time),
                    };
                    let parent_column = if self.pinned.contains(parent) {
                        0
                    } else if index == 0 {
                        column
                    } else {
                        self.free_column()
                    };
                    self.reserver_by_sha.insert(
                        parent.clone(),
                        Reservation {
                            node_type: row.node_type,
                            newest_date,
                            column: parent_column,
                        },
                    );
                }
            }
        }
        // A root commit ends its lane: nothing below carries it on, so the column is free again.
        if row.parents.is_empty() {
            self.columns_used.remove(&column);
        }
        column
    }
}

/// The first-parent chain of `tip_sha` through `commits`, for [`compute`]'s pinned set.
#[must_use]
pub fn first_parent_chain(commits: &[Commit], tip_sha: &str) -> HashSet<Sha> {
    let mut chain = HashSet::new();
    if tip_sha.is_empty() {
        return chain;
    }
    let mut next = tip_sha;
    for commit in commits {
        if commit.sha != next {
            continue;
        }
        chain.insert(commit.sha.clone());
        match commit.parents.first() {
            Some(parent) => next = parent,
            None => break,
        }
    }
    chain
}

/// Lays out `commits` (topological, newest first).
///
/// `pinned_shas`: the first-parent chain of the pinned branch, forced into column 0; pass an empty set for
/// none.
#[must_use]
pub fn compute(commits: &[Commit], pinned_shas: &HashSet<Sha>) -> Layout {
    // Pass 1: lanes.
    let mut assigner = Assigner::new(pinned_shas);
    let columns: Vec<usize> = commits
        .iter()
        .map(|commit| assigner.assign(commit))
        .collect();

    let mut column_by_sha: HashMap<&str, usize> = HashMap::with_capacity(commits.len());
    for (commit, &column) in commits.iter().zip(&columns) {
        column_by_sha.entry(&commit.sha).or_insert(column);
    }

    // Pass 2: edges, each row from the previous row's edges plus its own starting ones.
    let mut layout = Layout {
        rows: Vec::with_capacity(commits.len()),
        max_columns: 0,
    };
    for (commit, &column) in commits.iter().zip(&columns) {
        let mut edges: BTreeMap<usize, ColumnEdges> = BTreeMap::new();

        if let Some(previous_row) = layout.rows.last() {
            for (&lane, previous) in &previous_row.edges {
                let Some(continued) = previous.continued() else {
                    continue;
                };
                let slot = edges.entry(lane).or_default();
                if continued.parent_sha == commit.sha {
                    slot.ending = Some(continued.clone());
                } else {
                    slot.pass_through = Some(continued.clone());
                }
            }
        }

        if let Some((first, merged)) = commit.parents.split_first() {
            let edge_to = |parent: &Sha| Edge {
                parent_sha: parent.clone(),
                node_type: commit.node_type,
            };
            edges.entry(column).or_default().starting = Some(edge_to(first));
            // A merged parent outside the loaded history has no lane to head for.
            for parent in merged {
                if let Some(&parent_column) = column_by_sha.get(parent.as_str()) {
                    edges.entry(parent_column).or_default().starting = Some(edge_to(parent));
                }
            }
        }

        let edge_column_max = edges
            .last_key_value()
            .map_or(column, |(&last, _)| last.max(column));
        layout.max_columns = layout.max_columns.max(edge_column_max);
        layout.rows.push(RowLayout {
            column,
            edges,
            edge_column_max,
        });
    }
    layout
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commit(sha: &str, parents: &[&str], time: i64) -> Commit {
        Commit {
            sha: sha.to_owned(),
            parents: parents.iter().map(|&parent| parent.to_owned()).collect(),
            time,
            node_type: if parents.len() > 1 {
                NodeType::Merge
            } else {
                NodeType::Commit
            },
            ..Commit::default()
        }
    }

    fn typed(sha: &str, parents: &[&str], time: i64, node_type: NodeType) -> Commit {
        Commit {
            node_type,
            ..commit(sha, parents, time)
        }
    }

    fn edge(parent: &str, node_type: NodeType) -> Option<Edge> {
        Some(Edge {
            parent_sha: parent.to_owned(),
            node_type,
        })
    }

    fn columns(layout: &Layout) -> Vec<usize> {
        layout.rows.iter().map(|row| row.column).collect()
    }

    /// One row's edges as `(column, starting, ending, pass_through)`.
    type Lanes = Vec<(usize, Option<Edge>, Option<Edge>, Option<Edge>)>;

    fn lanes(row: &RowLayout) -> Lanes {
        row.edges
            .iter()
            .map(|(&column, edges)| {
                (
                    column,
                    edges.starting.clone(),
                    edges.ending.clone(),
                    edges.pass_through.clone(),
                )
            })
            .collect()
    }

    const C: NodeType = NodeType::Commit;
    const M: NodeType = NodeType::Merge;

    #[test]
    fn empty_history() {
        assert_eq!(compute(&[], &HashSet::new()), Layout::default());
    }

    #[test]
    fn linear_history_stays_in_one_lane() {
        let commits = [
            commit("a", &["b"], 3),
            commit("b", &["c"], 2),
            commit("c", &[], 1),
        ];
        let layout = compute(&commits, &HashSet::new());
        assert_eq!(columns(&layout), [0, 0, 0]);
        assert_eq!(layout.max_columns, 0);
        assert_eq!(lanes(&layout.rows[0]), [(0, edge("b", C), None, None)]);
        assert_eq!(
            lanes(&layout.rows[1]),
            [(0, edge("c", C), edge("b", C), None)]
        );
        assert_eq!(lanes(&layout.rows[2]), [(0, None, edge("c", C), None)]);
    }

    #[test]
    fn merge_opens_a_lane_that_closes_at_the_fork_point() {
        let commits = [
            commit("m", &["a", "b"], 4),
            commit("b", &["c"], 3),
            commit("a", &["c"], 2),
            commit("c", &[], 1),
        ];
        let layout = compute(&commits, &HashSet::new());
        // `c` was reserved in lane 1 by `b`, then pulled into lane 0 by `a`.
        assert_eq!(columns(&layout), [0, 1, 0, 0]);
        assert_eq!(layout.max_columns, 1);
        assert_eq!(
            lanes(&layout.rows[0]),
            [(0, edge("a", M), None, None), (1, edge("b", M), None, None)]
        );
        assert_eq!(
            lanes(&layout.rows[1]),
            [
                (0, None, None, edge("a", M)),
                (1, edge("c", C), edge("b", M), None)
            ]
        );
        assert_eq!(
            lanes(&layout.rows[2]),
            [
                (0, edge("c", C), edge("a", M), None),
                (1, None, None, edge("c", C))
            ]
        );
        assert_eq!(
            lanes(&layout.rows[3]),
            [(0, None, edge("c", C), None), (1, None, edge("c", C), None)]
        );
        assert_eq!(layout.rows[3].edge_column_max, 1);
    }

    #[test]
    fn merged_parent_keeps_its_lane_when_the_first_parent_reaches_it() {
        // `b` is both the merge's second parent (lane 1) and `a`'s parent: having a merge child, it is not
        // pulled into lane 0; `a`'s lane ends at it instead.
        let commits = [
            commit("m", &["a", "b"], 3),
            commit("a", &["b"], 2),
            commit("b", &[], 1),
        ];
        let layout = compute(&commits, &HashSet::new());
        assert_eq!(columns(&layout), [0, 0, 1]);
        assert_eq!(
            lanes(&layout.rows[1]),
            [
                (0, edge("b", C), edge("a", M), None),
                (1, None, None, edge("b", M))
            ]
        );
        assert_eq!(
            lanes(&layout.rows[2]),
            [(0, None, edge("b", C), None), (1, None, edge("b", M), None)]
        );
    }

    #[test]
    fn second_branch_ends_at_the_shared_parent_and_frees_its_lane() {
        let commits = [
            commit("x", &["p"], 5),
            commit("y", &["p"], 4),
            commit("p", &["q"], 3),
            // A later tip reuses lane 1, freed when `p` was reached.
            commit("z", &["q"], 2),
            commit("q", &[], 1),
        ];
        let layout = compute(&commits, &HashSet::new());
        assert_eq!(columns(&layout), [0, 1, 0, 1, 0]);
        assert_eq!(layout.max_columns, 1);
        assert_eq!(
            lanes(&layout.rows[1]),
            [(0, None, None, edge("p", C)), (1, edge("p", C), None, None)]
        );
        assert_eq!(
            lanes(&layout.rows[2]),
            [
                (0, edge("q", C), edge("p", C), None),
                (1, None, edge("p", C), None)
            ]
        );
        assert_eq!(
            lanes(&layout.rows[3]),
            [(0, None, None, edge("q", C)), (1, edge("q", C), None, None)]
        );
        assert_eq!(
            lanes(&layout.rows[4]),
            [(0, None, edge("q", C), None), (1, None, edge("q", C), None)]
        );
    }

    #[test]
    fn merged_parent_outside_the_history_gets_no_edge() {
        let commits = [commit("m", &["a", "gone"], 2), commit("a", &[], 1)];
        let layout = compute(&commits, &HashSet::new());
        assert_eq!(columns(&layout), [0, 0]);
        assert_eq!(layout.max_columns, 0);
        assert_eq!(lanes(&layout.rows[0]), [(0, edge("a", M), None, None)]);
    }

    #[test]
    fn first_parent_chain_follows_first_parents_only() {
        let commits = [
            commit("m2", &["m1", "f"], 3),
            commit("f", &["m1"], 2),
            commit("m1", &[], 1),
        ];
        let chain = first_parent_chain(&commits, "m2");
        assert_eq!(chain, HashSet::from(["m2".to_owned(), "m1".to_owned()]));
        assert_eq!(
            first_parent_chain(&commits, "f"),
            HashSet::from(["f".to_owned(), "m1".to_owned()])
        );
        assert!(first_parent_chain(&commits, "").is_empty());
        assert!(first_parent_chain(&commits, "unknown").is_empty());
    }

    #[test]
    fn pinned_chain_is_forced_into_column_zero() {
        // The feature tip is newer than main's, so unpinned it would take lane 0.
        let commits = [
            commit("f", &["b"], 3),
            commit("main", &["b"], 2),
            commit("b", &[], 1),
        ];
        assert_eq!(columns(&compute(&commits, &HashSet::new())), [0, 1, 0]);

        let pinned = first_parent_chain(&commits, "main");
        let layout = compute(&commits, &pinned);
        assert_eq!(columns(&layout), [1, 0, 0]);
        assert_eq!(layout.max_columns, 1);
        assert_eq!(layout.rows[0].edge_column_max, 1);
        assert_eq!(lanes(&layout.rows[0]), [(1, edge("b", C), None, None)]);
        assert_eq!(
            lanes(&layout.rows[1]),
            [(0, edge("b", C), None, None), (1, None, None, edge("b", C))]
        );
        assert_eq!(
            lanes(&layout.rows[2]),
            [(0, None, edge("b", C), None), (1, None, edge("b", C), None)]
        );
    }

    #[test]
    fn work_dir_and_stash_rows_start_dashed_edges() {
        const W: NodeType = NodeType::WorkDir;
        const S: NodeType = NodeType::Stash;
        let commits = [
            typed("wip", &["h"], 10, W),
            typed("stash", &["h"], 5, S),
            commit("h", &["r"], 2),
            commit("r", &[], 1),
        ];
        let layout = compute(&commits, &HashSet::new());
        assert_eq!(columns(&layout), [0, 1, 0, 0]);
        assert_eq!(lanes(&layout.rows[0]), [(0, edge("h", W), None, None)]);
        assert_eq!(
            lanes(&layout.rows[1]),
            [(0, None, None, edge("h", W)), (1, edge("h", S), None, None)]
        );
        assert_eq!(
            lanes(&layout.rows[2]),
            [
                (0, edge("r", C), edge("h", W), None),
                (1, None, edge("h", S), None)
            ]
        );
        assert_eq!(lanes(&layout.rows[3]), [(0, None, edge("r", C), None)]);
        assert!(
            layout.rows[1].edges[&1]
                .starting
                .as_ref()
                .is_some_and(|edge| edge.node_type.is_dashed())
        );
    }

    #[test]
    fn newer_branch_displaces_a_stash_reservation() {
        const S: NodeType = NodeType::Stash;
        // The stash reserves lane 0 for `h`; the branch in lane 1 is newer, so `h` moves to its lane even
        // though that lane is further right.
        let commits = [
            typed("stash", &["h"], 1, S),
            commit("y", &["x"], 9),
            commit("x", &["h"], 8),
            commit("h", &[], 0),
        ];
        let layout = compute(&commits, &HashSet::new());
        assert_eq!(columns(&layout), [0, 1, 1, 1]);
        assert_eq!(
            lanes(&layout.rows[2]),
            [
                (0, None, None, edge("h", S)),
                (1, edge("h", C), edge("x", C), None)
            ]
        );
        assert_eq!(
            lanes(&layout.rows[3]),
            [(0, None, edge("h", S), None), (1, None, edge("h", C), None)]
        );

        // An older branch does not: its lane ends at the stash's.
        let mut older = commits.clone();
        older[0].time = 20;
        assert_eq!(columns(&compute(&older, &HashSet::new())), [0, 1, 1, 0]);
    }

    #[test]
    fn solid_edge_is_continued_over_a_dashed_one() {
        let dashed = Edge {
            parent_sha: "a".to_owned(),
            node_type: NodeType::WorkDir,
        };
        let solid = Edge {
            parent_sha: "b".to_owned(),
            node_type: C,
        };
        let lane = |pass_through: &Edge, starting: &Edge| ColumnEdges {
            starting: Some(starting.clone()),
            ending: None,
            pass_through: Some(pass_through.clone()),
        };
        assert_eq!(lane(&dashed, &solid).continued(), Some(&solid));
        assert_eq!(lane(&solid, &dashed).continued(), Some(&solid));
        assert_eq!(lane(&dashed, &dashed.clone()).continued(), Some(&dashed));
        let ending_only = ColumnEdges {
            ending: Some(solid.clone()),
            ..ColumnEdges::default()
        };
        assert_eq!(ending_only.continued(), None);
    }
}
