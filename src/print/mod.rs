//! Create visual representations of git graphs.

use gleisbau::graph::GitGraph;
use std::cmp::max;

pub mod colors;
pub mod format;
pub mod svg;
pub mod unicode;

/// Find the index at which a between-branch connection
/// has to deviate from the current branch's column.
///
/// Returns the last index on the current column.
fn get_deviate_index(graph: &GitGraph, index: usize, par_index: usize) -> usize {
    let info = &graph.commits[index];

    let par_info = &graph.commits[par_index];
    let par_branch = &graph.all_branches[par_info.branch_trace.unwrap()];

    let mut min_split_idx = index;
    for sibling_oid in &par_info.children {
        if let Some(&sibling_index) = graph.indices.get(sibling_oid) {
            if let Some(sibling) = graph.commits.get(sibling_index) {
                if let Some(sibling_trace) = sibling.branch_trace {
                    let sibling_branch = &graph.all_branches[sibling_trace];
                    if sibling_oid != &info.oid
                        && sibling_branch.visual.column == par_branch.visual.column
                        && sibling_index > min_split_idx
                    {
                        min_split_idx = sibling_index;
                    }
                }
            }
        }
    }

    // TODO: in cases where no crossings occur, the rule for merge commits can also be applied to normal commits
    // See also branch::trace_branch()
    if info.is_merge {
        max(index, min_split_idx)
    } else {
        (par_index as i32 - 1) as usize
    }
}
