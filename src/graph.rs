//! A graph structure representing the history of a Git repository.
//!
//! To generate a graph, call [GitGraph::new()].
//!
//! ### Visualization of branches
//! git-graph uses the term *branch* a little different from how git uses it.
//! In git-lingo this means "a label on some commit", whereas in git-graph
//! it means "a path in the ancestor graph of a repository". Nodes are
//! commits, edges are directed from a child to its parents.
//!
//! In the text below, the term
//! - *git-branch* is a label on a commit.
//! - *branch* is the visualization of an ancestor path.
//!
//! git-graph visualizes branches as a vertical line. Only
//! the primary parent of a commit can be on the same branch as the
//! commit. Horizontal lines represent forks (multiple children) or
//! merges (multiple parents), and show the remaining parent relations.

use crate::print::colors::to_terminal_color;
use crate::settings::{BranchOrder, BranchSettings, MergePatterns, Settings};
use git2::{BranchType, Commit, Error, Oid, Reference, Repository};
use regex::Regex;
use std::collections::{HashMap, HashSet};

const ORIGIN: &str = "origin/";
const FORK: &str = "fork/";

/// Represents a git history graph.
pub struct GitGraph {
    pub repository: Repository,
    pub commits: Vec<CommitInfo>,
    /// Mapping from commit id to index in `commits`
    pub indices: HashMap<Oid, usize>,
    /// All detected branches and tags, including merged and deleted
    pub all_branches: Vec<BranchInfo>,
    /// The current HEAD
    pub head: HeadInfo,
}

impl GitGraph {
    /// Generate a branch graph for a repository
    pub fn new(
        mut repository: Repository,
        settings: &Settings,
        start_point: Option<String>,
        max_count: Option<usize>,
        refspecs: Vec<String>,
    ) -> Result<Self, String> {
        #![doc = include_str!("../docs/branch_assignment.md")]
        let mut stashes = HashSet::new();
        repository
            .stash_foreach(|_, _, oid| {
                stashes.insert(*oid);
                true
            })
            .map_err(|err| err.message().to_string())?;

        let mut walk = repository
            .revwalk()
            .map_err(|err| err.message().to_string())?;

        walk.set_sorting(git2::Sort::TOPOLOGICAL | git2::Sort::TIME)
            .map_err(|err| err.message().to_string())?;

        configure_revwalk(&repository, &mut walk, start_point, &refspecs)?;

        if repository.is_shallow() {
            return Err("ERROR: git-graph does not support shallow clones due to a missing feature in the underlying libgit2 library.".to_string());
        }

        let head = HeadInfo::new(&repository.head().map_err(|err| err.message().to_string())?)?;

        // commits will hold the CommitInfo for all commits covered
        // indices maps git object id to an index into commits.
        let mut commits = Vec::new();
        let mut indices = HashMap::new();
        let mut idx = 0;
        for oid in walk {
            if let Some(max) = max_count {
                if idx >= max {
                    break;
                }
            }
            if let Ok(oid) = oid {
                if !stashes.contains(&oid) {
                    let commit = repository.find_commit(oid).unwrap();

                    commits.push(CommitInfo::new(&commit));
                    indices.insert(oid, idx);
                    idx += 1;
                }
            }
        }

        assign_children(&mut commits, &indices);

        let mut all_branches = assign_branches(&repository, &mut commits, &indices, settings)?;
        correct_fork_merges(&commits, &indices, &mut all_branches, settings)?;
        assign_sources_targets(&commits, &indices, &mut all_branches);

        let (shortest_first, forward) = match settings.branch_order {
            BranchOrder::ShortestFirst(fwd) => (true, fwd),
            BranchOrder::LongestFirst(fwd) => (false, fwd),
        };

        assign_branch_columns(
            &commits,
            &indices,
            &mut all_branches,
            &settings.branches,
            shortest_first,
            forward,
        );

        // Remove commits not on a branch. This will give all commits a new index.
        let filtered_commits: Vec<CommitInfo> = commits
            .into_iter()
            .filter(|info| info.branch_trace.is_some())
            .collect();

        // Create indices from git object id into the filtered commits
        let filtered_indices: HashMap<Oid, usize> = filtered_commits
            .iter()
            .enumerate()
            .map(|(idx, info)| (info.oid, idx))
            .collect();

        // Map from old index to new index. None, if old index was removed
        let index_map: HashMap<usize, Option<&usize>> = indices
            .iter()
            .map(|(oid, index)| (*index, filtered_indices.get(oid)))
            .collect();

        // Update branch.range from old to new index. Shrink if endpoints were removed.
        for branch in all_branches.iter_mut() {
            if let Some(mut start_idx) = branch.range.0 {
                let mut idx0 = index_map[&start_idx];
                while idx0.is_none() {
                    start_idx += 1;
                    idx0 = index_map[&start_idx];
                }
                branch.range.0 = Some(*idx0.unwrap());
            }
            if let Some(mut end_idx) = branch.range.1 {
                let mut idx0 = index_map[&end_idx];
                while idx0.is_none() {
                    end_idx -= 1;
                    idx0 = index_map[&end_idx];
                }
                branch.range.1 = Some(*idx0.unwrap());
            }
        }

        Ok(GitGraph {
            repository,
            commits: filtered_commits,
            indices: filtered_indices,
            all_branches,
            head,
        })
    }

    pub fn take_repository(self) -> Repository {
        self.repository
    }

    pub fn commit(&self, id: Oid) -> Result<Commit<'_>, Error> {
        self.repository.find_commit(id)
    }
}

/// Information about the current HEAD
pub struct HeadInfo {
    pub oid: Oid,
    pub name: String,
    pub is_branch: bool,
}
impl HeadInfo {
    fn new(head: &Reference) -> Result<Self, String> {
        let name = head.name().ok_or_else(|| "No name for HEAD".to_string())?;
        let name = if name == "HEAD" {
            name.to_string()
        } else {
            name[11..].to_string()
        };

        let h = HeadInfo {
            oid: head.target().ok_or_else(|| "No id for HEAD".to_string())?,
            name,
            is_branch: head.is_branch(),
        };
        Ok(h)
    }
}

/// Represents a commit.
pub struct CommitInfo {
    pub oid: Oid,
    pub is_merge: bool,
    pub parents: [Option<Oid>; 2],
    pub children: Vec<Oid>,
    pub branches: Vec<usize>,
    pub tags: Vec<usize>,
    pub branch_trace: Option<usize>,
}

impl CommitInfo {
    fn new(commit: &Commit) -> Self {
        CommitInfo {
            oid: commit.id(),
            is_merge: commit.parent_count() > 1,
            parents: [commit.parent_id(0).ok(), commit.parent_id(1).ok()],
            children: Vec::new(),
            branches: Vec::new(),
            tags: Vec::new(),
            branch_trace: None,
        }
    }
}

/// Represents a branch (real or derived from merge summary).
pub struct BranchInfo {
    pub target: Oid,
    pub merge_target: Option<Oid>,
    pub source_branch: Option<usize>,
    pub target_branch: Option<usize>,
    pub name: String,
    pub persistence: u8,
    pub is_remote: bool,
    pub is_merged: bool,
    pub is_tag: bool,
    pub visual: BranchVis,
    pub range: (Option<usize>, Option<usize>),
}
impl BranchInfo {
    #[allow(clippy::too_many_arguments)]
    fn new(
        target: Oid,
        merge_target: Option<Oid>,
        name: String,
        persistence: u8,
        is_remote: bool,
        is_merged: bool,
        is_tag: bool,
        visual: BranchVis,
        end_index: Option<usize>,
    ) -> Self {
        BranchInfo {
            target,
            merge_target,
            target_branch: None,
            source_branch: None,
            name,
            persistence,
            is_remote,
            is_merged,
            is_tag,
            visual,
            range: (end_index, None),
        }
    }
}

/// Branch properties for visualization.
pub struct BranchVis {
    /// The branch's column group (left to right)
    pub order_group: usize,
    /// The branch's merge target column group (left to right)
    pub target_order_group: Option<usize>,
    /// The branch's source branch column group (left to right)
    pub source_order_group: Option<usize>,
    /// The branch's terminal color (index in 256-color palette)
    pub term_color: u8,
    /// SVG color (name or RGB in hex annotation)
    pub svg_color: String,
    /// The column the branch is located in
    pub column: Option<usize>,
}

impl BranchVis {
    fn new(order_group: usize, term_color: u8, svg_color: String) -> Self {
        BranchVis {
            order_group,
            target_order_group: None,
            source_order_group: None,
            term_color,
            svg_color,
            column: None,
        }
    }
}

/// For a single refspec, find a base branch to compare against
/// using the branch's upstream tracking ref.
fn find_base_oid(repository: &Repository, refspec: &str, tip_oid: Oid) -> Option<Oid> {
    if let Ok(branch) = repository.find_branch(refspec, BranchType::Local) {
        if let Ok(upstream) = branch.upstream() {
            if let Some(oid) = upstream.get().target() {
                if oid != tip_oid {
                    return Some(oid);
                }
            }
        }
    }

    None
}

fn hide_ancestors_of(repository: &Repository, walk: &mut git2::Revwalk, merge_base: Oid) {
    if let Ok(commit) = repository.find_commit(merge_base) {
        for parent in commit.parents() {
            let _ = walk.hide(parent.id());
        }
    }
}

fn configure_revwalk(
    repository: &Repository,
    walk: &mut git2::Revwalk,
    start_point: Option<String>,
    refspecs: &[String],
) -> Result<(), String> {
    if !refspecs.is_empty() {
        let mut resolved_oids = Vec::with_capacity(refspecs.len());
        for refspec in refspecs {
            let object = repository
                .revparse_single(refspec)
                .map_err(|err| format!("Failed to resolve refspec '{}': {}", refspec, err))?;
            let oid = object.id();
            walk.push(oid).map_err(|err| err.message().to_string())?;
            resolved_oids.push(oid);
        }

        if resolved_oids.len() == 1 {
            // Single refspec: auto-detect base branch
            if let Some(base_oid) = find_base_oid(repository, &refspecs[0], resolved_oids[0]) {
                walk.push(base_oid)
                    .map_err(|err| err.message().to_string())?;
                if let Ok(mb) = repository.merge_base(resolved_oids[0], base_oid) {
                    hide_ancestors_of(repository, walk, mb);
                }
            }
        } else {
            // Multiple refspecs: compute merge-base of all
            let mut base = resolved_oids[0];
            let mut base_found = true;
            for oid in &resolved_oids[1..] {
                match repository.merge_base(base, *oid) {
                    Ok(mb) => base = mb,
                    Err(_) => {
                        base_found = false;
                        break;
                    }
                }
            }
            if base_found {
                hide_ancestors_of(repository, walk, base);
            }
        }
    } else if let Some(start) = start_point {
        let object = repository
            .revparse_single(&start)
            .map_err(|err| format!("Failed to resolve start point '{}': {}", start, err))?;
        walk.push(object.id())
            .map_err(|err| err.message().to_string())?;
    } else {
        walk.push_glob("*")
            .map_err(|err| err.message().to_string())?;
    }
    Ok(())
}

/// Walks through the commits and adds each commit's Oid to the children of its parents.
fn assign_children(commits: &mut [CommitInfo], indices: &HashMap<Oid, usize>) {
    for idx in 0..commits.len() {
        let (oid, parents) = {
            let info = &commits[idx];
            (info.oid, info.parents)
        };
        for par_oid in &parents {
            if let Some(par_idx) = par_oid.and_then(|oid| indices.get(&oid)) {
                commits[*par_idx].children.push(oid);
            }
        }
    }
}

/// Extracts branches from repository and merge summaries, assigns branches and branch traces to commits.
///
/// Algorithm:
/// * Find all actual branches (incl. target oid) and all extract branches from merge summaries (incl. parent oid)
/// * Sort all branches by persistence
/// * Iterating over all branches in persistence order, trace back over commit parents until a trace is already assigned
fn assign_branches(
    repository: &Repository,
    commits: &mut [CommitInfo],
    indices: &HashMap<Oid, usize>,
    settings: &Settings,
) -> Result<Vec<BranchInfo>, String> {
    let mut branch_idx = 0;

    let mut branches = extract_branches(repository, commits, indices, settings)?;

    let mut index_map: Vec<_> = (0..branches.len())
        .map(|old_idx| {
            let (target, is_tag, is_merged) = {
                let branch = &branches[old_idx];
                (branch.target, branch.is_tag, branch.is_merged)
            };
            if let Some(&idx) = &indices.get(&target) {
                let info = &mut commits[idx];
                if is_tag {
                    info.tags.push(old_idx);
                } else if !is_merged {
                    info.branches.push(old_idx);
                }
                let oid = info.oid;
                let any_assigned =
                    trace_branch(repository, commits, indices, &mut branches, oid, old_idx)
                        .unwrap_or(false);

                if any_assigned || !is_merged {
                    branch_idx += 1;
                    Some(branch_idx - 1)
                } else {
                    None
                }
            } else {
                None
            }
        })
        .collect();

    let mut commit_count = vec![0; branches.len()];
    for info in commits.iter_mut() {
        if let Some(trace) = info.branch_trace {
            commit_count[trace] += 1;
        }
    }

    let mut count_skipped = 0;
    for (idx, branch) in branches.iter().enumerate() {
        if let Some(mapped) = index_map[idx] {
            if commit_count[idx] == 0 && branch.is_merged && !branch.is_tag {
                index_map[idx] = None;
                count_skipped += 1;
            } else {
                index_map[idx] = Some(mapped - count_skipped);
            }
        }
    }

    for info in commits.iter_mut() {
        if let Some(trace) = info.branch_trace {
            info.branch_trace = index_map[trace];
            for br in info.branches.iter_mut() {
                *br = index_map[*br].unwrap();
            }
            for tag in info.tags.iter_mut() {
                *tag = index_map[*tag].unwrap();
            }
        }
    }

    let branches: Vec<_> = branches
        .into_iter()
        .enumerate()
        .filter_map(|(arr_index, branch)| {
            if index_map[arr_index].is_some() {
                Some(branch)
            } else {
                None
            }
        })
        .collect();

    Ok(branches)
}

fn correct_fork_merges(
    commits: &[CommitInfo],
    indices: &HashMap<Oid, usize>,
    branches: &mut [BranchInfo],
    settings: &Settings,
) -> Result<(), String> {
    for idx in 0..branches.len() {
        if let Some(merge_target) = branches[idx]
            .merge_target
            .and_then(|oid| indices.get(&oid))
            .and_then(|idx| commits.get(*idx))
            .and_then(|info| info.branch_trace)
            .and_then(|trace| branches.get(trace))
        {
            if branches[idx].name == merge_target.name {
                let name = format!("{}{}", FORK, branches[idx].name);
                let term_col = to_terminal_color(
                    &branch_color(
                        &name,
                        &settings.branches.terminal_colors[..],
                        &settings.branches.terminal_colors_unknown,
                        idx,
                    )[..],
                )?;
                let pos = branch_order(&name, &settings.branches.order);
                let svg_col = branch_color(
                    &name,
                    &settings.branches.svg_colors,
                    &settings.branches.svg_colors_unknown,
                    idx,
                );

                branches[idx].name = format!("{}{}", FORK, branches[idx].name);
                branches[idx].visual.order_group = pos;
                branches[idx].visual.term_color = term_col;
                branches[idx].visual.svg_color = svg_col;
            }
        }
    }
    Ok(())
}
fn assign_sources_targets(
    commits: &[CommitInfo],
    indices: &HashMap<Oid, usize>,
    branches: &mut [BranchInfo],
) {
    for idx in 0..branches.len() {
        let target_branch_idx = branches[idx]
            .merge_target
            .and_then(|oid| indices.get(&oid))
            .and_then(|idx| commits.get(*idx))
            .and_then(|info| info.branch_trace);

        branches[idx].target_branch = target_branch_idx;

        let group = target_branch_idx
            .and_then(|trace| branches.get(trace))
            .map(|br| br.visual.order_group);

        branches[idx].visual.target_order_group = group;
    }
    for info in commits {
        let mut max_par_order = None;
        let mut source_branch_id = None;
        for par_oid in info.parents.iter() {
            let par_info = par_oid
                .and_then(|oid| indices.get(&oid))
                .and_then(|idx| commits.get(*idx));
            if let Some(par_info) = par_info {
                if par_info.branch_trace != info.branch_trace {
                    if let Some(trace) = par_info.branch_trace {
                        source_branch_id = Some(trace);
                    }

                    let group = par_info
                        .branch_trace
                        .and_then(|trace| branches.get(trace))
                        .map(|br| br.visual.order_group);
                    if let Some(gr) = max_par_order {
                        if let Some(p_group) = group {
                            if p_group > gr {
                                max_par_order = group;
                            }
                        }
                    } else {
                        max_par_order = group;
                    }
                }
            }
        }
        let branch = info.branch_trace.and_then(|trace| branches.get_mut(trace));
        if let Some(branch) = branch {
            if let Some(order) = max_par_order {
                branch.visual.source_order_group = Some(order);
            }
            if let Some(source_id) = source_branch_id {
                branch.source_branch = Some(source_id);
            }
        }
    }
}

/// Extracts and processes actual Git branches (local and remote) from the repository.
///
/// This function iterates through the branches found in the Git repository,
/// filters them based on the `include_remote` setting, and constructs `BranchInfo`
/// objects for each valid branch. It assigns properties like name, type (local/remote),
/// visual order, and colors based on the provided settings.
///
/// Arguments:
/// - `repository`: A reference to the Git `Repository` object.
/// - `indices`: A HashMap mapping commit OIDs to their corresponding indices in the `commits` list.
/// - `settings`: A reference to the application `Settings` containing branch configuration.
/// - `counter`: A mutable reference to a counter, incremented for each processed branch to aid in color assignment.
///
/// Returns:
/// A `Result` containing a `Vec<BranchInfo>` on success, or a `String` error message on failure.
fn extract_actual_branches(
    repository: &Repository,
    indices: &HashMap<Oid, usize>,
    settings: &Settings,
    counter: &mut usize,
) -> Result<Vec<BranchInfo>, String> {
    // Determine if remote branches should be included based on settings.
    let filter = if settings.include_remote {
        None
    } else {
        Some(BranchType::Local)
    };

    // Retrieve branches from the repository, handling potential errors.
    let actual_branches = repository
        .branches(filter)
        .map_err(|err| err.message().to_string())?
        .collect::<Result<Vec<_>, Error>>()
        .map_err(|err| err.message().to_string())?;

    // Process each actual branch to create `BranchInfo` objects.
    let valid_branches = actual_branches
        .iter()
        .filter_map(|(br, tp)| {
            br.get().name().and_then(|n| {
                br.get().target().map(|t| {
                    *counter += 1; // Increment counter for unique branch identification/coloring.

                    // Determine the starting index for slicing the branch name string.
                    let start_index = match tp {
                        BranchType::Local => 11,  // "refs/heads/"
                        BranchType::Remote => 13, // "refs/remotes/"
                    };
                    let name = &n[start_index..];
                    let end_index = indices.get(&t).cloned();

                    // Convert branch color to a terminal-compatible format.
                    let term_color = match to_terminal_color(
                        &branch_color(
                            name,
                            &settings.branches.terminal_colors[..],
                            &settings.branches.terminal_colors_unknown,
                            *counter,
                        )[..],
                    ) {
                        Ok(col) => col,
                        Err(err) => return Err(err), // Propagate color conversion errors.
                    };

                    // Create and return the BranchInfo object.
                    Ok(BranchInfo::new(
                        t,
                        None, // No merge OID for actual branches.
                        name.to_string(),
                        branch_order(name, &settings.branches.persistence) as u8,
                        &BranchType::Remote == tp, // Check if it's a remote branch.
                        false,                     // Not a derived merge branch.
                        false,                     // Not a tag.
                        BranchVis::new(
                            branch_order(name, &settings.branches.order),
                            term_color,
                            branch_color(
                                name,
                                &settings.branches.svg_colors,
                                &settings.branches.svg_colors_unknown,
                                *counter,
                            ),
                        ),
                        end_index,
                    ))
                })
            })
        })
        .collect::<Result<Vec<_>, String>>()?; // Collect results, propagating any errors.

    Ok(valid_branches)
}

/// Iterates through commits, identifies merge commits, and derives branch information
/// from their summaries.
///
/// This function processes each commit in the provided list. If a commit is identified
/// as a merge commit and has a summary, it attempts to parse a branch name from the summary.
/// A `BranchInfo` object is then created for this derived branch, representing the merge
/// point and its properties.
///
/// Arguments:
/// - `repository`: A reference to the Git `Repository` object.
/// - `commits`: A slice of `CommitInfo` objects, representing the commits to process.
/// - `settings`: A reference to the application `Settings` containing branch and merge pattern configuration.
/// - `counter`: A mutable reference to a counter, incremented for each processed merge branch.
///
/// Returns:
/// A `Result` containing a `Vec<BranchInfo>` on success, or a `String` error message on failure.
fn extract_merge_branches(
    repository: &Repository,
    commits: &[CommitInfo],
    settings: &Settings,
    counter: &mut usize,
) -> Result<Vec<BranchInfo>, String> {
    let mut merge_branches = Vec::new();

    for (idx, info) in commits.iter().enumerate() {
        // Only process if the commit is a merge.
        if info.is_merge {
            let commit = repository
                .find_commit(info.oid)
                .map_err(|err| err.message().to_string())?;

            // Attempt to get the commit summary.
            if let Some(summary) = commit.summary() {
                *counter += 1; // Increment counter for unique branch identification/coloring.

                let parent_oid = commit
                    .parent_id(1)
                    .map_err(|err| err.message().to_string())?;

                // Parse the branch name from the merge summary using configured patterns.
                let branch_name = parse_merge_summary(summary, &settings.merge_patterns)
                    .unwrap_or_else(|| "unknown".to_string());

                // Determine persistence and order for the derived branch.
                let persistence = branch_order(&branch_name, &settings.branches.persistence) as u8;
                let pos = branch_order(&branch_name, &settings.branches.order);

                // Get terminal and SVG colors for the branch.
                let term_col = to_terminal_color(
                    &branch_color(
                        &branch_name,
                        &settings.branches.terminal_colors[..],
                        &settings.branches.terminal_colors_unknown,
                        *counter,
                    )[..],
                )?;
                let svg_col = branch_color(
                    &branch_name,
                    &settings.branches.svg_colors,
                    &settings.branches.svg_colors_unknown,
                    *counter,
                );

                // Create and add the BranchInfo for the derived merge branch.
                let branch_info = BranchInfo::new(
                    parent_oid,     // Target is the parent of the merge.
                    Some(info.oid), // The merge commit itself.
                    branch_name,
                    persistence,
                    false, // Not a remote branch.
                    true,  // This is a derived merge branch.
                    false, // Not a tag.
                    BranchVis::new(pos, term_col, svg_col),
                    Some(idx + 1), // End index typically points to the commit after the merge.
                );
                merge_branches.push(branch_info);
            }
        }
    }
    Ok(merge_branches)
}

/// Extracts Git tags and treats them as branches, assigning appropriate properties.
///
/// This function iterates through all tags in the repository, resolves their target
/// commit OID, and if the target commit is found within the `commits` list,
/// a `BranchInfo` object is created for the tag. Tags are assigned a higher
/// persistence value to ensure they are displayed prominently.
///
/// Arguments:
/// - `repository`: A reference to the Git `Repository` object.
/// - `indices`: A HashMap mapping commit OIDs to their corresponding indices in the `commits` list.
/// - `settings`: A reference to the application `Settings` containing branch configuration.
/// - `counter`: A mutable reference to a counter, incremented for each processed tag.
///
/// Returns:
/// A `Result` containing a `Vec<BranchInfo>` on success, or a `String` error message on failure.
fn extract_tags_as_branches(
    repository: &Repository,
    indices: &HashMap<Oid, usize>,
    settings: &Settings,
    counter: &mut usize,
) -> Result<Vec<BranchInfo>, String> {
    let mut tags_info = Vec::new();
    let mut tags_raw = Vec::new();

    // Iterate over all tags in the repository.
    repository
        .tag_foreach(|oid, name| {
            tags_raw.push((oid, name.to_vec()));
            true // Continue iteration.
        })
        .map_err(|err| err.message().to_string())?;

    for (oid, name_bytes) in tags_raw {
        // Convert tag name bytes to a UTF-8 string. Tags typically start with "refs/tags/".
        let name = std::str::from_utf8(&name_bytes[5..]).map_err(|err| err.to_string())?;

        // Resolve the target OID of the tag. It could be a tag object or directly a commit.
        let target = repository
            .find_tag(oid)
            .map(|tag| tag.target_id())
            .or_else(|_| repository.find_commit(oid).map(|_| oid)); // If not a tag object, try as a direct commit.

        if let Ok(target_oid) = target {
            // If the target commit is within our processed commits, create a BranchInfo.
            if let Some(target_index) = indices.get(&target_oid) {
                *counter += 1; // Increment counter for unique tag identification/coloring.

                // Get terminal and SVG colors for the tag.
                let term_col = to_terminal_color(
                    &branch_color(
                        name,
                        &settings.branches.terminal_colors[..],
                        &settings.branches.terminal_colors_unknown,
                        *counter,
                    )[..],
                )?;
                let pos = branch_order(name, &settings.branches.order);
                let svg_col = branch_color(
                    name,
                    &settings.branches.svg_colors,
                    &settings.branches.svg_colors_unknown,
                    *counter,
                );

                // Create the BranchInfo object for the tag.
                let tag_info = BranchInfo::new(
                    target_oid,
                    None, // No merge OID for tags.
                    name.to_string(),
                    settings.branches.persistence.len() as u8 + 1, // Tags usually have highest persistence.
                    false,                                         // Not a remote branch.
                    false,                                         // Not a derived merge branch.
                    true,                                          // This is a tag.
                    BranchVis::new(pos, term_col, svg_col),
                    Some(*target_index),
                );
                tags_info.push(tag_info);
            }
        }
    }
    Ok(tags_info)
}

/// Extracts (real or derived from merge summary) and assigns basic properties to branches and tags.
///
/// This function orchestrates the extraction of branch information from various sources:
/// 1. Actual Git branches (local and remote).
/// 2. Branches derived from merge commit summaries.
/// 3. Git tags, treated as branches for visualization purposes.
///
/// It combines the results from these extraction steps, sorts them based on
/// persistence and merge status, and returns a comprehensive list of `BranchInfo` objects.
///
/// Arguments:
/// - `repository`: A reference to the Git `Repository` object.
/// - `commits`: A slice of `CommitInfo` objects, representing all relevant commits.
/// - `indices`: A HashMap mapping commit OIDs to their corresponding indices in the `commits` list.
/// - `settings`: A reference to the application `Settings` containing all necessary configuration.
///
/// Returns:
/// A `Result` containing a `Vec<BranchInfo>` on success, or a `String` error message on failure.
fn extract_branches(
    repository: &Repository,
    commits: &[CommitInfo],
    indices: &HashMap<Oid, usize>,
    settings: &Settings,
) -> Result<Vec<BranchInfo>, String> {
    let mut counter = 0; // Counter for unique branch/tag identification, especially for coloring.
    let mut all_branches: Vec<BranchInfo> = Vec::new();

    // 1. Extract actual local and remote branches.
    let actual_branches = extract_actual_branches(repository, indices, settings, &mut counter)?;
    all_branches.extend(actual_branches);

    // 2. Extract branches derived from merge commit summaries.
    let merge_branches = extract_merge_branches(repository, commits, settings, &mut counter)?;
    all_branches.extend(merge_branches);

    // 3. Extract tags and treat them as branches for visualization.
    let tags_as_branches = extract_tags_as_branches(repository, indices, settings, &mut counter)?;
    all_branches.extend(tags_as_branches);

    // Sort all collected branches and tags.
    // Sorting criteria: first by persistence, then by whether they are merged (unmerged first).
    all_branches.sort_by_cached_key(|branch| (branch.persistence, !branch.is_merged));

    Ok(all_branches)
}

/// Traces back branches by following 1st commit parent,
/// until a commit is reached that already has a trace.
fn trace_branch(
    repository: &Repository,
    commits: &mut [CommitInfo],
    indices: &HashMap<Oid, usize>,
    branches: &mut [BranchInfo],
    oid: Oid,
    branch_index: usize,
) -> Result<bool, Error> {
    let mut curr_oid = oid;
    let mut prev_index: Option<usize> = None;
    let mut start_index: Option<i32> = None;
    let mut any_assigned = false;
    while let Some(index) = indices.get(&curr_oid) {
        let info = &mut commits[*index];
        if let Some(old_trace) = info.branch_trace {
            let (old_name, old_term, old_svg, old_range) = {
                let old_branch = &branches[old_trace];
                (
                    &old_branch.name.clone(),
                    old_branch.visual.term_color,
                    old_branch.visual.svg_color.clone(),
                    old_branch.range,
                )
            };
            let new_name = &branches[branch_index].name;
            let old_end = old_range.0.unwrap_or(0);
            let new_end = branches[branch_index].range.0.unwrap_or(0);
            if new_name == old_name && old_end >= new_end {
                let old_branch = &mut branches[old_trace];
                if let Some(old_end) = old_range.1 {
                    if index > &old_end {
                        old_branch.range = (None, None);
                    } else {
                        old_branch.range = (Some(*index), old_branch.range.1);
                    }
                } else {
                    old_branch.range = (Some(*index), old_branch.range.1);
                }
            } else {
                let branch = &mut branches[branch_index];
                if branch.name.starts_with(ORIGIN) && branch.name[7..] == old_name[..] {
                    branch.visual.term_color = old_term;
                    branch.visual.svg_color = old_svg;
                }
                match prev_index {
                    None => start_index = Some(*index as i32 - 1),
                    Some(prev_index) => {
                        // TODO: in cases where no crossings occur, the rule for merge commits can also be applied to normal commits
                        // see also print::get_deviate_index()
                        if commits[prev_index].is_merge {
                            let mut temp_index = prev_index;
                            for sibling_oid in &commits[*index].children {
                                if sibling_oid != &curr_oid {
                                    let sibling_index = indices[sibling_oid];
                                    if sibling_index > temp_index {
                                        temp_index = sibling_index;
                                    }
                                }
                            }
                            start_index = Some(temp_index as i32);
                        } else {
                            start_index = Some(*index as i32 - 1);
                        }
                    }
                }
                break;
            }
        }

        info.branch_trace = Some(branch_index);
        any_assigned = true;

        let commit = repository.find_commit(curr_oid)?;
        if commit.parent_count() == 0 {
            // If no parents, this is the root commit, set `start_index` and break.
            start_index = Some(*index as i32);
            break;
        }
        // Set `prev_index` to the current commit's index and move to the first parent.
        prev_index = Some(*index);
        curr_oid = commit.parent_id(0)?;
    }

    let branch = &mut branches[branch_index];
    if let Some(end) = branch.range.0 {
        if let Some(start_index) = start_index {
            if start_index < end as i32 {
                // TODO: find a better solution (bool field?) to identify non-deleted branches that were not assigned to any commits, and thus should not occupy a column.
                branch.range = (None, None);
            } else {
                branch.range = (branch.range.0, Some(start_index as usize));
            }
        } else {
            branch.range = (branch.range.0, None);
        }
    } else {
        branch.range = (branch.range.0, start_index.map(|si| si as usize));
    }
    Ok(any_assigned)
}

/// Sorts branches into columns for visualization, that all branches can be
/// visualizes linearly and without overlaps. Uses Shortest-First scheduling.
fn assign_branch_columns(
    commits: &[CommitInfo],
    indices: &HashMap<Oid, usize>,
    branches: &mut [BranchInfo],
    settings: &BranchSettings,
    shortest_first: bool,
    forward: bool,
) {
    let mut occupied: Vec<Vec<Vec<(usize, usize)>>> = vec![vec![]; settings.order.len() + 1];

    let length_sort_factor = if shortest_first { 1 } else { -1 };
    let start_sort_factor = if forward { 1 } else { -1 };

    let mut branches_sort: Vec<_> = branches
        .iter()
        .enumerate()
        .filter(|(_idx, br)| br.range.0.is_some() || br.range.1.is_some())
        .map(|(idx, br)| {
            (
                idx,
                br.range.0.unwrap_or(0),
                br.range.1.unwrap_or(branches.len() - 1),
                br.visual
                    .source_order_group
                    .unwrap_or(settings.order.len() + 1),
                br.visual
                    .target_order_group
                    .unwrap_or(settings.order.len() + 1),
            )
        })
        .collect();

    branches_sort.sort_by_cached_key(|tup| {
        (
            std::cmp::max(tup.3, tup.4),
            (tup.2 as i32 - tup.1 as i32) * length_sort_factor,
            tup.1 as i32 * start_sort_factor,
        )
    });

    for (branch_idx, start, end, _, _) in branches_sort {
        let branch = &branches[branch_idx];
        let group = branch.visual.order_group;
        let group_occ = &mut occupied[group];

        let align_right = branch
            .source_branch
            .map(|src| branches[src].visual.order_group > branch.visual.order_group)
            .unwrap_or(false)
            || branch
                .target_branch
                .map(|trg| branches[trg].visual.order_group > branch.visual.order_group)
                .unwrap_or(false);

        let len = group_occ.len();
        let mut found = len;
        for i in 0..len {
            let index = if align_right { len - i - 1 } else { i };
            let column_occ = &group_occ[index];
            let mut occ = false;
            for (s, e) in column_occ {
                if start <= *e && end >= *s {
                    occ = true;
                    break;
                }
            }
            if !occ {
                if let Some(merge_trace) = branch
                    .merge_target
                    .and_then(|t| indices.get(&t))
                    .and_then(|t_idx| commits[*t_idx].branch_trace)
                {
                    let merge_branch = &branches[merge_trace];
                    if merge_branch.visual.order_group == branch.visual.order_group {
                        if let Some(merge_column) = merge_branch.visual.column {
                            if merge_column == index {
                                occ = true;
                            }
                        }
                    }
                }
            }
            if !occ {
                found = index;
                break;
            }
        }

        let branch = &mut branches[branch_idx];
        branch.visual.column = Some(found);
        if found == group_occ.len() {
            group_occ.push(vec![]);
        }
        group_occ[found].push((start, end));
    }

    // Compute start column of each group
    let mut group_offset: Vec<usize> = vec![];
    let mut acc = 0;
    for group in occupied {
        group_offset.push(acc);
        acc += group.len();
    }

    // Compute branch column. Up till now we have computed the branch group
    // and the column offset within that group. This was to make it easy to
    // insert columns between groups. Now it is time to convert offset relative
    // to the group the final column.
    for branch in branches {
        if let Some(column) = branch.visual.column {
            let offset = group_offset[branch.visual.order_group];
            branch.visual.column = Some(column + offset);
        }
    }
}

/// Finds the index for a branch name from a slice of prefixes
fn branch_order(name: &str, order: &[Regex]) -> usize {
    order
        .iter()
        .position(|b| (name.starts_with(ORIGIN) && b.is_match(&name[7..])) || b.is_match(name))
        .unwrap_or(order.len())
}

/// Finds the svg color for a branch name.
fn branch_color<T: Clone>(
    name: &str,
    order: &[(Regex, Vec<T>)],
    unknown: &[T],
    counter: usize,
) -> T {
    let stripped_name = name.strip_prefix(ORIGIN).unwrap_or(name);

    for (regex, colors) in order {
        if regex.is_match(stripped_name) {
            return colors[counter % colors.len()].clone();
        }
    }

    unknown[counter % unknown.len()].clone()
}

/// Tries to extract the name of a merged-in branch from the merge commit summary.
pub fn parse_merge_summary(summary: &str, patterns: &MergePatterns) -> Option<String> {
    for regex in &patterns.patterns {
        if let Some(captures) = regex.captures(summary) {
            if captures.len() == 2 && captures.get(1).is_some() {
                return captures.get(1).map(|m| m.as_str().to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use crate::settings::MergePatterns;

    #[test]
    fn parse_merge_summary() {
        let patterns = MergePatterns::default();

        let gitlab_pull = "Merge branch 'feature/my-feature' into 'master'";
        let git_default = "Merge branch 'feature/my-feature' into dev";
        let git_master = "Merge branch 'feature/my-feature'";
        let github_pull = "Merge pull request #1 from user-x/feature/my-feature";
        let github_pull_2 = "Merge branch 'feature/my-feature' of github.com:user-x/repo";
        let bitbucket_pull = "Merged in feature/my-feature (pull request #1)";

        assert_eq!(
            super::parse_merge_summary(gitlab_pull, &patterns),
            Some("feature/my-feature".to_string()),
        );
        assert_eq!(
            super::parse_merge_summary(git_default, &patterns),
            Some("feature/my-feature".to_string()),
        );
        assert_eq!(
            super::parse_merge_summary(git_master, &patterns),
            Some("feature/my-feature".to_string()),
        );
        assert_eq!(
            super::parse_merge_summary(github_pull, &patterns),
            Some("feature/my-feature".to_string()),
        );
        assert_eq!(
            super::parse_merge_summary(github_pull_2, &patterns),
            Some("feature/my-feature".to_string()),
        );
        assert_eq!(
            super::parse_merge_summary(bitbucket_pull, &patterns),
            Some("feature/my-feature".to_string()),
        );
    }
}
