//! Create graphs in Unicode format with ANSI X3.64 / ISO 6429 colour codes

use gleisbau::graph::{BranchInfo, CommitInfo, GitGraph, HeadInfo};
use crate::print::format::CommitFormat;
use gleisbau::settings::{Characters, Settings};
use itertools::Itertools;
use std::cmp::max;
use std::collections::hash_map::Entry::{Occupied, Vacant};
use std::collections::HashMap;
use std::fmt::Write;
use textwrap::Options;
use yansi::Paint;

const SPACE: u8 = 0;
const DOT: u8 = 1;
const CIRCLE: u8 = 2;
const VER: u8 = 3;
const HOR: u8 = 4;
const CROSS: u8 = 5;
const R_U: u8 = 6;
const R_D: u8 = 7;
const L_D: u8 = 8;
const L_U: u8 = 9;
const VER_L: u8 = 10;
const VER_R: u8 = 11;
const HOR_U: u8 = 12;
const HOR_D: u8 = 13;

const ARR_L: u8 = 14;
const ARR_R: u8 = 15;

const WHITE: u8 = 7;
const HEAD_COLOR: u8 = 14;
const HASH_COLOR: u8 = 11;

/**
UnicodeGraphInfo is a type alias for a tuple containing three elements:
graph-lines, text-lines, start-row

1.  graph_lines: `Vec<String>` - This represents the lines of the generated text-based graph
    visualization. Each `String` in this vector corresponds to a single row of
    the graph output, containing characters that form the visual representation
    of the commit history (like lines, dots, and branch intersections).

2.  text_lines: `Vec<String>`: This represents the lines of the commit messages or other
    textual information associated with each commit in the graph. Each `String`
    in this vector corresponds to a line of text that is displayed alongside
    the graph. This can include commit hashes, author information, commit
    messages, branch names, and tags, depending on the formatting settings.
    Some entries in this vector might be empty strings or correspond to
    inserted blank lines for visual spacing.

3.  start_row: `Vec<usize>`: Starting row for commit in the `graph.commits` vector.
*/
pub type UnicodeGraphInfo = (Vec<String>, Vec<String>, Vec<usize>);

/// Creates a text-based visual representation of a graph.
pub fn print_unicode(graph: &GitGraph, settings: &Settings) -> Result<UnicodeGraphInfo, String> {
    if graph.all_branches.is_empty() {
        return Ok((vec![], vec![], vec![]));
    }

    // 1. Calculate dimensions and inserts
    let num_cols = calculate_graph_dimensions(graph);
    let inserts = get_inserts(graph, settings.compact);

    let (indent1, indent2) = if let Some((_, ind1, ind2)) = settings.wrapping {
        (" ".repeat(ind1.unwrap_or(0)), " ".repeat(ind2.unwrap_or(0)))
    } else {
        ("".to_string(), "".to_string())
    };

    // 2. Prepare wrapping for commit text (using references to the new indent strings)
    let wrap_options = get_wrapping_options(settings, num_cols, &indent1, &indent2)?;

    // 3. Compute commit text and index map
    let (mut text_lines, index_map) =
        build_commit_lines_and_map(graph, settings, &inserts, &wrap_options)?;

    // 4. Calculate total rows and initialize/draw the grid
    let total_rows = text_lines.len();

    let mut grid = draw_graph_lines(graph, settings, num_cols, &inserts, &index_map, total_rows);

    // 5. Handle reverse order
    if settings.reverse_commit_order {
        text_lines.reverse();
        grid.reverse();
    }

    // 6. Final printing and result
    let lines = print_graph(&settings.characters, &grid, text_lines, settings.colored);

    Ok((lines.0, lines.1, index_map))
}

/// Calculates the necessary column count for the graph grid.
fn calculate_graph_dimensions(graph: &GitGraph) -> usize {
    2 * graph
        .all_branches
        .iter()
        .map(|b| b.visual.column.unwrap_or(0))
        .max()
        .unwrap_or(0)
        + 1
}

/// Prepares wrapping options, returning the options structure.
// 'a now refers to the lifetime of the indent strings passed in.
fn get_wrapping_options<'a>(
    settings: &Settings,
    num_cols: usize,
    indent1: &'a str, // Takes reference to owned string
    indent2: &'a str, // Takes reference to owned string
) -> Result<Option<Options<'a>>, String> {
    if let Some((width, _, _)) = settings.wrapping {
        // We now pass the references directly to create_wrapping_options
        create_wrapping_options(width, indent1, indent2, num_cols + 4)
    } else {
        Ok(None)
    }
}

/// Iterates through commits to compute text lines, blank line inserts, and the index map.
fn build_commit_lines_and_map<'a>(
    graph: &GitGraph,
    settings: &Settings,
    inserts: &HashMap<usize, Vec<Vec<Occ>>>,
    wrap_options: &Option<Options<'a>>,
) -> Result<(Vec<Option<String>>, Vec<usize>), String> {
    let head_idx = graph.indices.get(&graph.head.oid);

    // Compute commit text into text_lines and add blank rows
    // if needed to match branch graph inserts.
    let mut index_map = vec![];
    let mut text_lines = vec![];
    let mut offset = 0;

    for (idx, info) in graph.commits.iter().enumerate() {
        index_map.push(idx + offset);

        // Calculate needed graph inserts (for ranges only)
        let cnt_inserts = if let Some(inserts) = inserts.get(&idx) {
            inserts
                .iter()
                .filter(|vec| {
                    vec.iter().all(|occ| match occ {
                        Occ::Commit(_, _) => false,
                        Occ::Range(_, _, _, _) => true,
                    })
                })
                .count()
        } else {
            0
        };

        let head = if head_idx == Some(&idx) {
            Some(&graph.head)
        } else {
            None
        };

        // Format the commit message lines
        let lines = format(
            &settings.format,
            graph,
            info,
            head,
            settings.colored,
            wrap_options,
        )?;

        let num_lines = if lines.is_empty() { 0 } else { lines.len() - 1 };
        let max_inserts = max(cnt_inserts, num_lines);
        let add_lines = max_inserts - num_lines;

        // Extend text_lines with commit lines and blank lines for padding
        text_lines.extend(lines.into_iter().map(Some));
        text_lines.extend((0..add_lines).map(|_| None));

        offset += max_inserts;
    }

    Ok((text_lines, index_map))
}

/// Initializes the grid and draws all commit/branch connections.
fn draw_graph_lines(
    graph: &GitGraph,
    settings: &Settings,
    num_cols: usize,
    inserts: &HashMap<usize, Vec<Vec<Occ>>>,
    index_map: &[usize],
    total_rows: usize,
) -> Grid {
    let mut grid = Grid::new(
        num_cols,
        total_rows,
        GridCell {
            character: SPACE,
            color: WHITE,
            pers: settings.branches.persistence.len() as u8 + 2,
        },
    );

    for (idx, info) in graph.commits.iter().enumerate() {
        let Some(trace) = info.branch_trace else {
            continue;
        };
        let branch = &graph.all_branches[trace];
        let column = branch.visual.column.unwrap();
        let idx_map = index_map[idx];

        // Draw commit point (DOT or CIRCLE)
        grid.set(
            column * 2,
            idx_map,
            if info.is_merge { CIRCLE } else { DOT },
            branch.visual.term_color,
            branch.persistence,
        );

        // Draw parent lines from this commit
        draw_parent_lines(graph, branch, &mut grid, info, inserts, index_map, idx);
    }
    grid
}

fn draw_parent_lines(
    graph: &GitGraph,
    branch: &BranchInfo,
    grid: &mut Grid,
    info: &CommitInfo,
    inserts: &HashMap<usize, Vec<Vec<Occ>>>,
    index_map: &[usize],
    idx: usize,
) {
    let column = branch.visual.column.unwrap();
    let idx_map = index_map[idx];

    let branch_color = branch.visual.term_color;

    for p in 0..2 {
        let parent = info.parents[p];
        let Some(par_oid) = parent else {
            continue;
        };
        let Some(par_idx) = graph.indices.get(&par_oid) else {
            // Parent is outside scope of graph.indices
            // so draw a vertical line to the bottom
            let idx_bottom = grid.height;
            vline(
                grid,
                (idx_map, idx_bottom),
                column,
                branch_color,
                branch.persistence,
            );
            continue;
        };

        let par_idx_map = index_map[*par_idx];
        let par_info = &graph.commits[*par_idx];
        let par_branch = &graph.all_branches[par_info.branch_trace.unwrap()];
        let par_column = par_branch.visual.column.unwrap();

        let (color, pers) = if info.is_merge {
            (par_branch.visual.term_color, par_branch.persistence)
        } else {
            (branch_color, branch.persistence)
        };

        if branch.visual.column == par_branch.visual.column {
            if par_idx_map > idx_map + 1 {
                vline(grid, (idx_map, par_idx_map), column, color, pers);
            }
        } else {
            let split_index = super::get_deviate_index(graph, idx, *par_idx);
            let split_idx_map = index_map[split_index];
            let insert_idx = find_insert_idx(&inserts[&split_index], idx, *par_idx).unwrap();
            let idx_split = split_idx_map + insert_idx;

            let is_secondary_merge = info.is_merge && p > 0;

            let row123 = (idx_map, idx_split, par_idx_map);
            let col12 = (column, par_column);
            zig_zag_line(grid, row123, col12, is_secondary_merge, color, pers);
        }
    }
}

/// Create `textwrap::Options` from width and indent.
fn create_wrapping_options<'a>(
    width: Option<usize>,
    indent1: &'a str,
    indent2: &'a str,
    graph_width: usize,
) -> Result<Option<Options<'a>>, String> {
    let wrapping = if let Some(width) = width {
        Some(
            textwrap::Options::new(width)
                .initial_indent(indent1)
                .subsequent_indent(indent2),
        )
    } else if atty::is(atty::Stream::Stdout) {
        let width = crossterm::terminal::size()
            .map_err(|err| err.to_string())?
            .0 as usize;
        let text_width = width.saturating_sub(graph_width);
        if text_width < 40 {
            // If too little space left for text, do not wrap at all
            None
        } else {
            Some(
                textwrap::Options::new(text_width)
                    .initial_indent(indent1)
                    .subsequent_indent(indent2),
            )
        }
    } else {
        None
    };
    Ok(wrapping)
}

/// Find the index of the insert that connects the two commits
fn find_insert_idx(inserts: &[Vec<Occ>], child_idx: usize, parent_idx: usize) -> Option<usize> {
    for (insert_idx, sub_entry) in inserts.iter().enumerate() {
        for occ in sub_entry {
            if let Occ::Range(i1, i2, _, _) = occ {
                if *i1 == child_idx && *i2 == parent_idx {
                    return Some(insert_idx);
                }
            }
        }
    }
    None
}

/// Draw a line that connects two commits on different columns
fn zig_zag_line(
    grid: &mut Grid,
    row123: (usize, usize, usize),
    col12: (usize, usize),
    is_merge: bool,
    color: u8,
    pers: u8,
) {
    let (row1, row2, row3) = row123;
    let (col1, col2) = col12;
    vline(grid, (row1, row2), col1, color, pers);
    hline(grid, row2, (col2, col1), is_merge, color, pers);
    vline(grid, (row2, row3), col2, color, pers);
}

/// Draws a vertical line
fn vline(grid: &mut Grid, (from, to): (usize, usize), column: usize, color: u8, pers: u8) {
    for i in (from + 1)..to {
        let (curr, _, old_pers) = grid.get_tuple(column * 2, i);
        let (new_col, new_pers) = if pers < old_pers {
            (Some(color), Some(pers))
        } else {
            (None, None)
        };
        match curr {
            DOT | CIRCLE => {}
            HOR => {
                grid.set_opt(column * 2, i, Some(CROSS), Some(color), Some(pers));
            }
            HOR_U | HOR_D => {
                grid.set_opt(column * 2, i, Some(CROSS), Some(color), Some(pers));
            }
            CROSS | VER | VER_L | VER_R => grid.set_opt(column * 2, i, None, new_col, new_pers),
            L_D | L_U => {
                grid.set_opt(column * 2, i, Some(VER_L), new_col, new_pers);
            }
            R_D | R_U => {
                grid.set_opt(column * 2, i, Some(VER_R), new_col, new_pers);
            }
            _ => {
                grid.set_opt(column * 2, i, Some(VER), new_col, new_pers);
            }
        }
    }
}

/// Draw a horizontal line.
/// If from > to, this will cause a backward draw.
fn hline(
    grid: &mut Grid,
    index: usize,
    (from, to): (usize, usize),
    merge: bool,
    color: u8,
    pers: u8,
) {
    if from == to {
        return;
    }

    let from_2 = from * 2;
    let to_2 = to * 2;

    if from < to {
        update_range_forward(grid, index, from_2, to_2, merge, color, pers);
        update_left_cell_forward(grid, index, from_2, color, pers);
        update_right_cell_forward(grid, index, to_2, color, pers);
    } else {
        update_range_backward(grid, index, from_2, to_2, merge, color, pers);
        update_left_cell_backward(grid, index, to_2, color, pers);
        update_right_cell_backward(grid, index, from_2, color, pers);
    }
}

fn update_range_forward(
    grid: &mut Grid,
    index: usize,
    from_2: usize,
    to_2: usize,
    merge: bool,
    color: u8,
    pers: u8,
) {
    for column in (from_2 + 1)..to_2 {
        if merge && column == to_2 - 1 {
            grid.set(column, index, ARR_R, color, pers);
        } else {
            let (curr, _, old_pers) = grid.get_tuple(column, index);
            let (new_col, new_pers) = if pers < old_pers {
                (Some(color), Some(pers))
            } else {
                (None, None)
            };
            match curr {
                DOT | CIRCLE => {}
                VER => grid.set_opt(column, index, Some(CROSS), None, None),
                HOR | CROSS | HOR_U | HOR_D => grid.set_opt(column, index, None, new_col, new_pers),
                L_U | R_U => grid.set_opt(column, index, Some(HOR_U), new_col, new_pers),
                L_D | R_D => grid.set_opt(column, index, Some(HOR_D), new_col, new_pers),
                _ => {
                    grid.set_opt(column, index, Some(HOR), new_col, new_pers);
                }
            }
        }
    }
}

fn update_left_cell_forward(grid: &mut Grid, index: usize, from_2: usize, color: u8, pers: u8) {
    let (left, _, old_pers) = grid.get_tuple(from_2, index);
    let (new_col, new_pers) = if pers < old_pers {
        (Some(color), Some(pers))
    } else {
        (None, None)
    };
    match left {
        DOT | CIRCLE => {}
        VER => grid.set_opt(from_2, index, Some(VER_R), new_col, new_pers),
        VER_L => grid.set_opt(from_2, index, Some(CROSS), None, None),
        VER_R => {}
        HOR | L_U => grid.set_opt(from_2, index, Some(HOR_U), new_col, new_pers),
        _ => {
            grid.set_opt(from_2, index, Some(R_D), new_col, new_pers);
        }
    }
}

fn update_right_cell_forward(grid: &mut Grid, index: usize, to_2: usize, color: u8, pers: u8) {
    let (right, _, old_pers) = grid.get_tuple(to_2, index);
    let (new_col, new_pers) = if pers < old_pers {
        (Some(color), Some(pers))
    } else {
        (None, None)
    };
    match right {
        DOT | CIRCLE => {}
        VER => grid.set_opt(to_2, index, Some(VER_L), None, None),
        VER_L | HOR_U => grid.set_opt(to_2, index, None, new_col, new_pers),
        HOR | R_U => grid.set_opt(to_2, index, Some(HOR_U), new_col, new_pers),
        _ => {
            grid.set_opt(to_2, index, Some(L_U), new_col, new_pers);
        }
    }
}

fn update_range_backward(
    grid: &mut Grid,
    index: usize,
    from_2: usize,
    to_2: usize,
    merge: bool,
    color: u8,
    pers: u8,
) {
    for column in (to_2 + 1)..from_2 {
        if merge && column == to_2 + 1 {
            grid.set(column, index, ARR_L, color, pers);
        } else {
            let (curr, _, old_pers) = grid.get_tuple(column, index);
            let (new_col, new_pers) = if pers < old_pers {
                (Some(color), Some(pers))
            } else {
                (None, None)
            };
            match curr {
                DOT | CIRCLE => {}
                VER => grid.set_opt(column, index, Some(CROSS), None, None),
                HOR | CROSS | HOR_U | HOR_D => grid.set_opt(column, index, None, new_col, new_pers),
                L_U | R_U => grid.set_opt(column, index, Some(HOR_U), new_col, new_pers),
                L_D | R_D => grid.set_opt(column, index, Some(HOR_D), new_col, new_pers),
                _ => {
                    grid.set_opt(column, index, Some(HOR), new_col, new_pers);
                }
            }
        }
    }
}

fn update_left_cell_backward(grid: &mut Grid, index: usize, to_2: usize, color: u8, pers: u8) {
    let (left, _, old_pers) = grid.get_tuple(to_2, index);
    let (new_col, new_pers) = if pers < old_pers {
        (Some(color), Some(pers))
    } else {
        (None, None)
    };
    match left {
        DOT | CIRCLE => {}
        VER => grid.set_opt(to_2, index, Some(VER_R), None, None),
        VER_R => grid.set_opt(to_2, index, None, new_col, new_pers),
        HOR | L_U => grid.set_opt(to_2, index, Some(HOR_U), new_col, new_pers),
        _ => {
            grid.set_opt(to_2, index, Some(R_U), new_col, new_pers);
        }
    }
}

fn update_right_cell_backward(grid: &mut Grid, index: usize, from_2: usize, color: u8, pers: u8) {
    let (right, _, old_pers) = grid.get_tuple(from_2, index);
    let (new_col, new_pers) = if pers < old_pers {
        (Some(color), Some(pers))
    } else {
        (None, None)
    };
    match right {
        DOT | CIRCLE => {}
        VER => grid.set_opt(from_2, index, Some(VER_L), new_col, new_pers),
        VER_R => grid.set_opt(from_2, index, Some(CROSS), None, None),
        VER_L => grid.set_opt(from_2, index, None, new_col, new_pers),
        HOR | R_D => grid.set_opt(from_2, index, Some(HOR_D), new_col, new_pers),
        _ => {
            grid.set_opt(from_2, index, Some(L_D), new_col, new_pers);
        }
    }
}

/// Calculates required additional rows to visually connect commits that
/// are not direct descendants in the main commit list. These "inserts"
//  represent the horizontal lines in the graph.
///
/// # Arguments
///
/// * `graph`: A reference to the `GitGraph` structure containing the
//             commit and branch information.
/// * `compact`: A boolean indicating whether to use a compact layout,
//               potentially merging some insertions with commits.
///
/// # Returns
///
/// A `HashMap` where the keys are the indices of commits in the
/// `graph.commits` vector, and the values are vectors of vectors
/// of `Occ`. Each inner vector represents a potential row of
/// insertions needed *before* the commit at the key index. The
/// `Occ` enum describes what occupies a cell in that row
/// (either a commit or a range representing a connection).
///
fn get_inserts(graph: &GitGraph, compact: bool) -> HashMap<usize, Vec<Vec<Occ>>> {
    // Initialize an empty HashMap to store the required insertions. The key is the commit
    // index, and the value is a vector of rows, where each row is a vector of Occupations (`Occ`).
    let mut inserts: HashMap<usize, Vec<Vec<Occ>>> = HashMap::new();

    // First, for each commit, we initialize an entry in the `inserts`
    // map with a single row containing the commit itself. This ensures
    // that every commit has a position in the grid.
    for (idx, info) in graph.commits.iter().enumerate() {
        // Get the visual column assigned to the branch of this commit. Unwrap is safe here
        // because `branch_trace` should always point to a valid branch with an assigned column
        // for commits that are included in the filtered graph.
        let column = graph.all_branches[info.branch_trace.unwrap()]
            .visual
            .column
            .unwrap();

        inserts.insert(idx, vec![vec![Occ::Commit(idx, column)]]);
    }

    // Now, iterate through the commits again to identify connections
    // needed between parents that are not directly adjacent in the
    // `graph.commits` list.
    for (idx, info) in graph.commits.iter().enumerate() {
        // If the commit has a branch trace (meaning it belongs to a visualized branch).
        if let Some(trace) = info.branch_trace {
            // Get the `BranchInfo` for the current commit's branch.
            let branch = &graph.all_branches[trace];
            // Get the visual column of the current commit's branch. Unwrap is safe as explained above.
            let column = branch.visual.column.unwrap();

            // Iterate through the two possible parents of the current commit.
            for p in 0..2 {
                let parent = info.parents[p];
                let Some(par_oid) = parent else {
                    continue;
                };
                // Try to find the index of the parent commit in the `graph.commits` vector.
                if let Some(par_idx) = graph.indices.get(&par_oid) {
                    let par_info = &graph.commits[*par_idx];
                    let par_branch = &graph.all_branches[par_info.branch_trace.unwrap()];
                    let par_column = par_branch.visual.column.unwrap();
                    // Determine the sorted range of columns between the current commit and its parent.
                    let column_range = sorted(column, par_column);

                    // If the column of the current commit is different from the column of its parent,
                    // it means we need to draw a horizontal line (an "insert") to connect them.
                    if column != par_column {
                        // Find the index in the `graph.commits` list where the visual connection
                        // should deviate from the parent's line. This helps in drawing the graph
                        // correctly when branches diverge or merge.
                        let split_index = super::get_deviate_index(graph, idx, *par_idx);
                        // Access the entry in the `inserts` map for the `split_index`.
                        match inserts.entry(split_index) {
                            // If there's already an entry at this `split_index` (meaning other
                            // insertions might be needed before this commit).
                            Occupied(mut entry) => {
                                // Find the first available row in the existing vector of rows
                                // where the new range doesn't overlap with existing occupations.
                                let mut insert_at = entry.get().len();
                                for (insert_idx, sub_entry) in entry.get().iter().enumerate() {
                                    let mut occ = false;
                                    // Check for overlaps with existing `Occ` in the current row.
                                    for other_range in sub_entry {
                                        // Check if the current column range overlaps with the other range.
                                        if other_range.overlaps(&column_range) {
                                            match other_range {
                                                // If the other occupation is a commit.
                                                Occ::Commit(target_index, _) => {
                                                    // In compact mode, we might allow overlap with the commit itself
                                                    // for merge commits (specifically the second parent) to keep the
                                                    // graph tighter.
                                                    if !compact
                                                        || !info.is_merge
                                                        || idx != *target_index
                                                        || p == 0
                                                    {
                                                        occ = true;
                                                        break;
                                                    }
                                                }
                                                // If the other occupation is a range (another connection).
                                                Occ::Range(o_idx, o_par_idx, _, _) => {
                                                    // Avoid overlap with connections between the same commits.
                                                    if idx != *o_idx && par_idx != o_par_idx {
                                                        occ = true;
                                                        break;
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    // If no overlap is found in this row, we can insert here.
                                    if !occ {
                                        insert_at = insert_idx;
                                        break;
                                    }
                                }
                                // Get a mutable reference to the vector of rows for this `split_index`.
                                let vec = entry.get_mut();
                                // If no suitable row was found, add a new row.
                                if insert_at == vec.len() {
                                    vec.push(vec![Occ::Range(
                                        idx,
                                        *par_idx,
                                        column_range.0,
                                        column_range.1,
                                    )]);
                                } else {
                                    // Otherwise, insert the new range into the found row.
                                    vec[insert_at].push(Occ::Range(
                                        idx,
                                        *par_idx,
                                        column_range.0,
                                        column_range.1,
                                    ));
                                }
                            }
                            // If there's no entry at this `split_index` yet.
                            Vacant(entry) => {
                                // Create a new entry with a single row containing the range.
                                entry.insert(vec![vec![Occ::Range(
                                    idx,
                                    *par_idx,
                                    column_range.0,
                                    column_range.1,
                                )]]);
                            }
                        }
                    }
                }
            }
        }
    }

    // Return the map of required insertions.
    inserts
}

/// Creates the complete graph visualization, incl. formatter commits.
fn print_graph(
    characters: &Characters,
    grid: &Grid,
    text_lines: Vec<Option<String>>,
    color: bool,
) -> (Vec<String>, Vec<String>) {
    let mut g_lines = vec![];
    let mut t_lines = vec![];

    for (row, line) in grid.data.chunks(grid.width).zip(text_lines.into_iter()) {
        let mut g_out = String::new();
        let mut t_out = String::new();

        if color {
            for cell in row {
                let chars = cell.char(characters);
                if cell.character == SPACE {
                    write!(g_out, "{}", chars)
                } else {
                    write!(g_out, "{}", chars.to_string().fixed(cell.color))
                }
                .unwrap();
            }
        } else {
            let str = row
                .iter()
                .map(|cell| cell.char(characters))
                .collect::<String>();
            write!(g_out, "{}", str).unwrap();
        }

        if let Some(line) = line {
            write!(t_out, "{}", line).unwrap();
        }

        g_lines.push(g_out);
        t_lines.push(t_out);
    }

    (g_lines, t_lines)
}

/// Format a commit.
fn format(
    format: &CommitFormat,
    graph: &GitGraph,
    info: &CommitInfo,
    head: Option<&HeadInfo>,
    color: bool,
    wrapping: &Option<Options>,
) -> Result<Vec<String>, String> {
    let commit = graph
        .repository
        .find_commit(info.oid)
        .map_err(|err| err.message().to_string())?;

    let branch_str = format_branches(graph, info, head, color);

    let hash_color = if color { Some(HASH_COLOR) } else { None };

    crate::print::format::format(&commit, branch_str, wrapping, hash_color, format)
}

/// Format branches and tags.
pub fn format_branches(
    graph: &GitGraph,
    info: &CommitInfo,
    head: Option<&HeadInfo>,
    color: bool,
) -> String {
    let curr_color = info
        .branch_trace
        .map(|branch_idx| &graph.all_branches[branch_idx].visual.term_color);

    let mut branch_str = String::new();

    let head_str = "HEAD ->";
    if let Some(head) = head {
        if !head.is_branch {
            if color {
                write!(branch_str, " {}", head_str.fixed(HEAD_COLOR))
            } else {
                write!(branch_str, " {}", head_str)
            }
            .unwrap();
        }
    }

    if !info.branches.is_empty() {
        write!(branch_str, " (").unwrap();

        let branches = info.branches.iter().sorted_by_key(|br| {
            if let Some(head) = head {
                head.name != graph.all_branches[**br].name
            } else {
                false
            }
        });

        for (idx, branch_index) in branches.enumerate() {
            let branch = &graph.all_branches[*branch_index];
            let branch_color = branch.visual.term_color;

            if let Some(head) = head {
                if idx == 0 && head.is_branch {
                    if color {
                        write!(branch_str, "{} ", head_str.fixed(14))
                    } else {
                        write!(branch_str, "{} ", head_str)
                    }
                    .unwrap();
                }
            }

            if color {
                write!(branch_str, "{}", &branch.name.fixed(branch_color))
            } else {
                write!(branch_str, "{}", &branch.name)
            }
            .unwrap();

            if idx < info.branches.len() - 1 {
                write!(branch_str, ", ").unwrap();
            }
        }
        write!(branch_str, ")").unwrap();
    }

    if !info.tags.is_empty() {
        write!(branch_str, " [").unwrap();
        for (idx, tag_index) in info.tags.iter().enumerate() {
            let tag = &graph.all_branches[*tag_index];
            let tag_color = curr_color.unwrap_or(&tag.visual.term_color);

            let tag_name = &tag.name[5..];
            if color {
                write!(branch_str, "{}", tag_name.fixed(*tag_color))
            } else {
                write!(branch_str, "{}", tag_name)
            }
            .unwrap();

            if idx < info.tags.len() - 1 {
                write!(branch_str, ", ").unwrap();
            }
        }
        write!(branch_str, "]").unwrap();
    }

    branch_str
}

/// Occupied row ranges
enum Occ {
    /// Horizontal position of commit markers
    // First  field (usize): The index of a commit within the graph.commits vector.
    // Second field (usize): The visual column in the grid where this commit is located. This column is determined by the branch the commit belongs to.
    // Purpose: This variant of Occ signifies that a specific row in the grid is occupied by a commit marker (dot or circle) at a particular column.
    Commit(usize, usize), // index in Graph.commits, column

    /// Horizontal line connecting two commits
    // First  field (usize): The index of the starting commit of a visual connection (usually the child commit).
    // Second field (usize): The index of the ending commit of a visual connection (usually the parent commit).
    // Third  field (usize): The starting visual column of the range occupied by the connection line between the two commits. This is the minimum of the columns of the two connected commits.
    // Fourth field (usize): The ending visual column of the range occupied by the connection line between the two commits. This is the maximum of the columns of the two connected commits.
    // Purpose: This variant of Occ signifies that a range of columns in a particular row is occupied by a horizontal line segment connecting a commit to one of its parents. The range spans from the visual column of one commit to the visual column of the other.
    Range(usize, usize, usize, usize), // ?child index, parent index, leftmost column, rightmost column
}

impl Occ {
    fn overlaps(&self, (start, end): &(usize, usize)) -> bool {
        match self {
            Occ::Commit(_, col) => start <= col && end >= col,
            Occ::Range(_, _, s, e) => s <= end && e >= start,
        }
    }
}

/// Sorts two numbers in ascending order
fn sorted(v1: usize, v2: usize) -> (usize, usize) {
    if v2 > v1 {
        (v1, v2)
    } else {
        (v2, v1)
    }
}

/// One cell in a [Grid]
#[derive(Clone, Copy)]
struct GridCell {
    /// The symbol shown, encoded as in index into settings::Characters
    character: u8,
    /// Standard 8-bit terminal colour code
    color: u8,
    /// Persistence level. z-order, lower numbers take preceedence.
    pers: u8,
}

impl GridCell {
    pub fn char(&self, characters: &Characters) -> char {
        characters.chars[self.character as usize]
    }
}

/// Two-dimensional grid used to hold the graph layout.
///
/// This can be rendered as unicode text or as SVG.
struct Grid {
    width: usize,
    height: usize,

    /// Grid cells are stored in row-major order.
    data: Vec<GridCell>,
}

impl Grid {
    pub fn new(width: usize, height: usize, initial: GridCell) -> Self {
        Grid {
            width,
            height,
            data: vec![initial; width * height],
        }
    }

    pub fn reverse(&mut self) {
        self.data.reverse();
    }
    /// Turn a 2D coordinate into an index of Grid.data
    pub fn index(&self, x: usize, y: usize) -> usize {
        y * self.width + x
    }
    pub fn get_tuple(&self, x: usize, y: usize) -> (u8, u8, u8) {
        let v = self.data[self.index(x, y)];
        (v.character, v.color, v.pers)
    }
    pub fn set(&mut self, x: usize, y: usize, character: u8, color: u8, pers: u8) {
        let idx = self.index(x, y);
        self.data[idx] = GridCell {
            character,
            color,
            pers,
        };
    }
    pub fn set_opt(
        &mut self,
        x: usize,
        y: usize,
        character: Option<u8>,
        color: Option<u8>,
        pers: Option<u8>,
    ) {
        let idx = self.index(x, y);
        let cell = &mut self.data[idx];
        if let Some(character) = character {
            cell.character = character;
        }
        if let Some(color) = color {
            cell.color = color;
        }
        if let Some(pers) = pers {
            cell.pers = pers;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // A dummy `Characters` struct is needed for `GridCell::char` but is not
    // directly used in `hline` tests, so we can omit it by not calling `char()`.

    // --- Test Cases ---

    /* Testing hline

    Note that hline is given a graph column as input,
    which indexes a grid column at 2*graph_col
        // Graph column: 0   1   2   3   4   5
        // Grid columns: 0 1 2 3 4 5 6 7 8 9
        // Grid row 0:   _ _ _ _ _ _ _ _ _ _
        // Grid row 1:   _ _ _ _ _ _ _ _ _ _
        // Grid row 2:   _ _ _ _ _ _ _ _ _ _

    A horizontal line from 1 to 3, would occupy columns 2, 3, 4, 5, 6 inclusive

    */

    const DEF_CH: u8 = SPACE;
    const DEF_COL: u8 = 0;
    const DEF_PERS: u8 = 10; // low persistence, will always be overwritten
    const DEFAULT_CELL: GridCell = GridCell {
        character: DEF_CH,
        color: DEF_COL,
        pers: DEF_PERS,
    };
    const ROW_INDEX: usize = 1;
    const LINE_COLOR: u8 = 14;
    const LINE_PERS: u8 = 5;

    #[test]
    fn hline_skip() {
        let (width, height) = (10, 3);
        let mut grid = Grid::new(width, height, DEFAULT_CELL);
        // Graph column: 0   1   2   3   4   5
        // Grid columns: 0 1 2 3 4 5 6 7 8 9
        // Grid row 0:   _ _ _ _ _ _ _ _ _ _
        // Grid row 1:   _ _ _ _ _ _ _ _ _ _
        // Grid row 2:   _ _ _ _ _ _ _ _ _ _

        // Case 1: from == to (should do nothing)
        let initial_char = grid.get_tuple(4 * 2, ROW_INDEX).0;
        super::hline(&mut grid, ROW_INDEX, (4, 4), true, LINE_COLOR, LINE_PERS);
        // Graph column: 0   1   2   3   4   5
        // Grid columns: 0 1 2 3 4 5 6 7 8 9
        // Grid row 0:   _ _ _ _ _ _ _ _ _ _
        // Grid row 1:   _ _ _ _ _ _ _ _X_ _
        // Grid row 2:   _ _ _ _ _ _ _ _ _ _

        assert_eq!(
            grid.get_tuple(4 * 2, ROW_INDEX).0,
            initial_char,
            "Same index call should not modify grid"
        );
    }

    /// Case 2: Forward draw (from < to), no merge
    /// Case 2a: out of bounds
    #[test]
    fn hline_forward_no_merge_out_of_bounds() {
        let (width, height) = (10, 3);
        let mut grid = Grid::new(width, height, DEFAULT_CELL);
        super::hline(&mut grid, ROW_INDEX, (2, 5), false, LINE_COLOR, LINE_PERS);
        // Graph column: 0   1   2   3   4   5
        // Grid columns: 0 1 2 3 4 5 6 7 8 9
        // Grid row 0:   _ _ _ _ _ _ _ _ _ _
        // Grid row 1:   _ _ _ _F- - - - - - *T  (F=from, T=to)
        // Grid row 2:   _ _ _ _ _ _ _ _ _ _

        // from: 2, to: 5
        // Start: from*2 = 4, End: to*2 = 10.
        // Range: start+1..end = 5..=9. Grid columns updated: 5, 6, 7, 8, 9. (HOR)
        // Ends updated: start=4, end=10. (VER_R)

        // Columns outside the line range (before start) should be default
        assert_eq!(
            grid.get_tuple(0, ROW_INDEX).0,
            SPACE,
            "SPACE at start of row"
        );
        assert_eq!(grid.get_tuple(3, ROW_INDEX).0, SPACE, "SPACE before hline");

        // Start (column 4): Should be R_D - assuming a vline below
        assert_eq!(grid.get_tuple(4, ROW_INDEX).0, R_D, "R_D at start of hline");
        assert_eq!(
            grid.get_tuple(4, ROW_INDEX).1,
            LINE_COLOR,
            "line_color at start of hline"
        );
        assert_eq!(
            grid.get_tuple(4, ROW_INDEX).2,
            LINE_PERS,
            "line_pers at start of hline"
        );

        // End (column 10) is out of bounds for width 10 (index 0-9). The `Grid`
        // implementation should handle this (or it's an expected panic/logic error).
        // *Assuming* the provided `Grid` is simplified for this example and we should
        // test only within bounds. Let's adjust the indices to be safe and meaningful.
    }

    /// Case 2: Forward draw (from < to), no merge
    /// Case 2b: Inside bounds
    #[test]
    fn hline_forward_no_merge_at_bounds() {
        let safe_width = 7; // Max column index 6, max graph column 2 = grid col 5
        let height = 3;
        let mut grid = Grid::new(safe_width, height, DEFAULT_CELL);
        // Graph column: 0   1   2   3
        // Grid columns: 0 1 2 3 4 5 6
        // Grid row 0:   _ _ _ _ _ _ _
        // Grid row 1:   _ _ _ _ _ _ _
        // Grid row 2:   _ _ _ _ _ _ _

        let from_idx = 1;
        let to_idx = 3;
        // Index: 0 1 2 3
        // Cell:  - F - T
        // From: 1, To: 3.
        // Start: 2, End: 6.
        // Range: 3..5 (Columns 3, 4, 5) -> HOR
        // Ends: 2, 6 -> R_D, L_U

        assert_eq!(
            grid.get_tuple(2, ROW_INDEX).0,
            SPACE,
            "SPACE at start of line, before written"
        );
        super::hline(
            &mut grid,
            ROW_INDEX,
            (from_idx, to_idx),
            false,
            LINE_COLOR,
            LINE_PERS,
        );
        // Graph column: 0   1   2   3
        // Grid columns: 0 1 2 3 4 5 6
        // Grid row 0:   _ _ _ _ _ _ _
        // Grid row 1:   _ _(╭ ─ ─ ─ ┘)
        // Grid row 2:   _ _ _ _ _ _ _

        // Check column before start
        let grid_cell = grid.get_tuple(1, ROW_INDEX);
        assert_eq!(grid_cell.0, SPACE, "SPACE before hline");
        assert_eq!(grid_cell.1, DEF_COL, "default colour before hline");
        assert_eq!(grid_cell.2, DEF_PERS, "default persistence before hline");

        // Start (column 2): R_D
        let grid_cell = grid.get_tuple(2, ROW_INDEX);
        assert_eq!(grid_cell.0, R_D, "R_D at start of hline");
        assert_eq!(grid_cell.1, LINE_COLOR, "line_color at start of hline");
        assert_eq!(grid_cell.2, LINE_PERS, "line_pers at start of hline");

        // Range (columns 3, 4, 5): HOR
        let grid_cell = grid.get_tuple(3, ROW_INDEX);
        assert_eq!(grid_cell.0, HOR, "HOR in range of hline");
        assert_eq!(grid_cell.1, LINE_COLOR, "line_color in range of hline");
        assert_eq!(grid_cell.2, LINE_PERS, "line_pers in range of hline");

        let grid_cell = grid.get_tuple(4, ROW_INDEX);
        assert_eq!(grid_cell.0, HOR, "HOR in range of hline");
        assert_eq!(grid_cell.1, LINE_COLOR, "line_color in range of hline");
        assert_eq!(grid_cell.2, LINE_PERS, "line_pers in range of hline");

        let grid_cell = grid.get_tuple(5, ROW_INDEX);
        assert_eq!(grid_cell.0, HOR, "HOR in range of hline");
        assert_eq!(grid_cell.1, LINE_COLOR, "line_color in range of hline");
        assert_eq!(grid_cell.2, LINE_PERS, "line_pers in range of hline");

        // End (column 6): L_U
        let grid_cell = grid.get_tuple(6, ROW_INDEX);
        assert_eq!(grid_cell.0, L_U, "L_U at end of hline");
        assert_eq!(grid_cell.1, LINE_COLOR, "line_color at end of hline");
        assert_eq!(grid_cell.2, LINE_PERS, "line_pers at end of hline");

        // Check column after end
        // This is undefined, as max grid col is 6
        // TODO make expected panic
        let grid_cell = grid.get_tuple(7, ROW_INDEX);
        assert_eq!(grid_cell.0, SPACE, "SPACE before hline");
        assert_eq!(grid_cell.1, DEF_COL, "default colour before hline");
        assert_eq!(grid_cell.2, DEF_PERS, "default persistence before hline");
    }

    /// Case 3: Backward draw (from > to), with merge
    #[test]
    fn hline_backward() {
        let (width, height) = (10, 3);
        let mut grid = Grid::new(width, height, DEFAULT_CELL);
        // Set an existing symbol at an end for better coverage:
        grid.set(4, ROW_INDEX, VER, 10, 10); // Start/From pos
        grid.set(8, ROW_INDEX, HOR, 10, 10); // End/To pos

        // Graph column: 0   1   2   3   4
        // Grid columns: 0 1 2 3 4 5 6 7 8 9
        // Grid row 0:   _ _ _ _ _ _ _ _ _ _
        // Grid row 1:   _ _ _ _ │ _ _ _ ─ _
        // Grid row 2:   _ _ _ _ _ _ _ _ _ _

        let from_idx = 4;
        let to_idx = 2;
        let merge = true;
        // Index: 0 1 2 3 4
        // Cell:  - - T - F
        // Forward is false.
        // start (orig from*2) = 8, end (orig to*2) = 4. Swapped: start=4, end=8.
        // Range: start+1..end = 5..8. Columns updated: 5, 6, 7 -> HOR
        // Merge: column = start = 4. Symbol = ARR_L.
        // Ends: start=4 (backward), end=8 (forward). (Both should be L_D/R_U if they weren't SPACE)

        super::hline(
            &mut grid,
            ROW_INDEX,
            (from_idx, to_idx),
            merge,
            LINE_COLOR,
            LINE_PERS,
        );
        // Graph column: 0   1   2   3   4
        // Grid columns: 0 1 2 3 4 5 6 7 8 9
        // Grid row 0:   _ _ _ _ _ _ _ _ _ _
        // Grid row 1:   _ _ _ _ ├ < ─ ─ ┬ _
        // Grid row 2:   _ _ _ _ _ _ _ _ _ _

        // Check columns before start
        assert_eq!(grid.get_tuple(3, ROW_INDEX).0, SPACE, "SPACE before hline");
        assert_eq!(
            grid.get_tuple(3, ROW_INDEX).1,
            DEF_COL,
            "default colour before hline"
        );
        assert_eq!(
            grid.get_tuple(3, ROW_INDEX).2,
            DEF_PERS,
            "default persistence before hline"
        );

        // Merge: column 4 (start). Should be VER_R.
        assert_eq!(grid.get_tuple(4, ROW_INDEX).0, VER_R, "VER_R at hline 'to'");
        assert_eq!(
            grid.get_tuple(4, ROW_INDEX).1,
            10,
            "unchanged color at hline 'to'"
        );
        assert_eq!(
            grid.get_tuple(4, ROW_INDEX).2,
            10,
            "unchanged pers at hline 'to'"
        );

        // Merge (column 5): ARR_l
        assert_eq!(
            grid.get_tuple(5, ROW_INDEX).0,
            ARR_L,
            "ARR_L before hline 'to'"
        );
        assert_eq!(
            grid.get_tuple(5, ROW_INDEX).1,
            LINE_COLOR,
            "line_color in hline"
        );
        assert_eq!(
            grid.get_tuple(5, ROW_INDEX).2,
            LINE_PERS,
            "line_pers in hline"
        );

        // Range (columns 5, 6): HOR
        assert_eq!(grid.get_tuple(6, ROW_INDEX).0, HOR, "HOR in hline");
        assert_eq!(
            grid.get_tuple(6, ROW_INDEX).1,
            LINE_COLOR,
            "line_color in hline"
        );
        assert_eq!(
            grid.get_tuple(6, ROW_INDEX).2,
            LINE_PERS,
            "line_pers in hline"
        );

        assert_eq!(grid.get_tuple(7, ROW_INDEX).0, HOR, "HOR in hline");
        assert_eq!(
            grid.get_tuple(7, ROW_INDEX).1,
            LINE_COLOR,
            "line_color in hline"
        );
        assert_eq!(
            grid.get_tuple(7, ROW_INDEX).2,
            LINE_PERS,
            "line_pers in hline"
        );

        // Cell 8 (end/from): HOR_D
        assert_eq!(
            grid.get_tuple(8, ROW_INDEX).0,
            HOR_D,
            "HOR_D at hline 'from'"
        );
        assert_eq!(
            grid.get_tuple(8, ROW_INDEX).1,
            LINE_COLOR,
            "line_color at hline 'from'"
        );
        assert_eq!(
            grid.get_tuple(8, ROW_INDEX).2,
            LINE_PERS,
            "line_pers at hline 'from'"
        );
    }

    /// Case 4: Forward draw, with merge, onto a crossing symbol
    #[test]
    fn hline_forward_merge() {
        let merge = true;
        let (width, height) = (7, 3);
        let mut grid = Grid::new(width, height, DEFAULT_CELL);
        grid.set(5, ROW_INDEX, R_U, 10, 10); // Set a symbol that changes range
        grid.set(6, ROW_INDEX, VER, 11, 10); // Set symbol for merge target

        // Graph column: 0   1   2   3
        // Grid columns: 0 1 2 3 4 5 6
        // Grid row 0:   _ _ _ _ _ _ _
        // Grid row 1:   _ _ _ _ _ └ │
        // Grid row 2:   _ _ _ _ _ _ _

        let from_idx = 1;
        let to_idx = 3;
        // Start: 2, End: 6.
        // Index: 0 1 2 3 4 5   6
        // Cell:  - - F - - R_U T
        // Range: 3..6. Columns: 3, 4, 5.
        // Column 5: R_U -> HOR_D (in update_range)
        // Merge: column = end - 1 = 5. Symbol = ARR_R. Overwrites HOR_D.
        // Ends: 2 (forward), 6 (forward).

        super::hline(
            &mut grid,
            ROW_INDEX,
            (from_idx, to_idx),
            merge,
            LINE_COLOR,
            LINE_PERS,
        );
        // Graph column: 0   1   2   3
        // Grid columns: 0 1 2 3 4 5 6
        // Grid row 0:   _ _ _ _ _ _ _
        // Grid row 1:   _ _(╭ ─ ─ > ┤)
        // Grid row 2:   _ _ _ _ _ _ _

        // Start (column 2): R_D
        assert_eq!(grid.get_tuple(2, ROW_INDEX).0, R_D);
        assert_eq!(grid.get_tuple(2, ROW_INDEX).1, LINE_COLOR);
        assert_eq!(grid.get_tuple(2, ROW_INDEX).2, LINE_PERS);

        // Range (column 3, 4): HOR
        assert_eq!(grid.get_tuple(3, ROW_INDEX).0, HOR);
        assert_eq!(grid.get_tuple(3, ROW_INDEX).1, LINE_COLOR);
        assert_eq!(grid.get_tuple(3, ROW_INDEX).2, LINE_PERS);

        assert_eq!(grid.get_tuple(4, ROW_INDEX).0, HOR);
        assert_eq!(grid.get_tuple(4, ROW_INDEX).1, LINE_COLOR);
        assert_eq!(grid.get_tuple(4, ROW_INDEX).2, LINE_PERS);

        // Merge column (end - 1 = 5): ARR_R (Merge overwrites update_range)
        assert_eq!(grid.get_tuple(5, ROW_INDEX).0, ARR_R);
        assert_eq!(grid.get_tuple(5, ROW_INDEX).1, LINE_COLOR);
        assert_eq!(grid.get_tuple(5, ROW_INDEX).2, LINE_PERS);

        // End (column 6): VER_L
        assert_eq!(grid.get_tuple(6, ROW_INDEX).0, VER_L);
        assert_eq!(grid.get_tuple(6, ROW_INDEX).1, 11);
        assert_eq!(grid.get_tuple(6, ROW_INDEX).2, 10);
    }
}
