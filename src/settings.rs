//! Graph generation settings.
//!
//! The settings control how a branching graph is layed out.
//! They are used in the [print][super::print] module when generating
//! a visualization and persisted to disk in the [config][super::config] module.
//!
//! These are the main structs
//! * [Settings] The main settings object, which contains:
//!   * [CommitFormat] Format of the commit summary text to the right of the graph.
//!   * [Characters] The symbols to use when rendering a graph as text.
//!   * [BranchSettings] Control how a graph is formatted.
//!   * [BranchOrder] Determines the left-to-right order of branches.
//!   * [MergePatterns] Regex that extract branch names from a merge commit.

use crate::print::format::CommitFormat;
use regex::{Error, Regex};
use serde_derive::{Deserialize, Serialize};
use std::str::FromStr;

/// Repository settings for the branching model.
/// Used to read repo's git-graph.toml
#[derive(Serialize, Deserialize)]
pub struct RepoSettings {
    /// The repository's branching model
    pub model: String,
}

/// Ordering policy for branches in visual columns.
pub enum BranchOrder {
    /// Recommended! Shortest branches are inserted left-most.
    ///
    /// For branches with equal length, branches ending last are inserted first.
    /// Reverse (arg = false): Branches ending first are inserted first.
    ShortestFirst(bool),
    /// Longest branches are inserted left-most.
    ///
    /// For branches with equal length, branches ending last are inserted first.
    /// Reverse (arg = false): Branches ending first are inserted first.
    LongestFirst(bool),
}

/// Top-level settings
pub struct Settings {
    /// Reverse the order of commits
    pub reverse_commit_order: bool,
    /// Debug printing and drawing
    pub debug: bool,
    /// Compact text-based graph
    pub compact: bool,
    /// Colored text-based graph
    pub colored: bool,
    /// Include remote branches?
    pub include_remote: bool,
    /// Formatting for commits
    pub format: CommitFormat,
    /// Text wrapping options
    pub wrapping: Option<(Option<usize>, Option<usize>, Option<usize>)>,
    /// Characters to use for text-based graph
    pub characters: Characters,
    /// Branch column sorting algorithm
    pub branch_order: BranchOrder,
    /// Settings for branches
    pub branches: BranchSettings,
    /// Regex patterns for finding branch names in merge commit summaries
    pub merge_patterns: MergePatterns,
}

/// Helper for reading BranchSettings, required due to RegEx.
#[derive(Serialize, Deserialize)]
pub struct BranchSettingsDef {
    /// Branch persistence
    pub persistence: Vec<String>,
    /// Branch ordering
    pub order: Vec<String>,
    /// Branch colors
    pub terminal_colors: ColorsDef,
    /// Branch colors for SVG output
    pub svg_colors: ColorsDef,
}

/// Helper for reading branch colors, required due to RegEx.
#[derive(Serialize, Deserialize)]
pub struct ColorsDef {
    matches: Vec<(String, Vec<String>)>,
    unknown: Vec<String>,
}

impl BranchSettingsDef {
    /// The Git-Flow model.
    pub fn git_flow() -> Self {
        BranchSettingsDef {
            persistence: vec![
                r"^(master|main|trunk)$".to_string(),
                r"^(develop|dev)$".to_string(),
                r"^feature.*$".to_string(),
                r"^release.*$".to_string(),
                r"^hotfix.*$".to_string(),
                r"^bugfix.*$".to_string(),
            ],
            order: vec![
                r"^(master|main|trunk)$".to_string(),
                r"^(hotfix|release).*$".to_string(),
                r"^(develop|dev)$".to_string(),
            ],
            terminal_colors: ColorsDef {
                matches: vec![
                    (
                        r"^(master|main|trunk)$".to_string(),
                        vec!["bright_blue".to_string()],
                    ),
                    (
                        r"^(develop|dev)$".to_string(),
                        vec!["bright_yellow".to_string()],
                    ),
                    (
                        r"^(feature|fork/).*$".to_string(),
                        vec!["bright_magenta".to_string(), "bright_cyan".to_string()],
                    ),
                    (r"^release.*$".to_string(), vec!["bright_green".to_string()]),
                    (
                        r"^(bugfix|hotfix).*$".to_string(),
                        vec!["bright_red".to_string()],
                    ),
                    (r"^tags/.*$".to_string(), vec!["bright_green".to_string()]),
                ],
                unknown: vec!["white".to_string()],
            },

            svg_colors: ColorsDef {
                matches: vec![
                    (
                        r"^(master|main|trunk)$".to_string(),
                        vec!["blue".to_string()],
                    ),
                    (r"^(develop|dev)$".to_string(), vec!["orange".to_string()]),
                    (
                        r"^(feature|fork/).*$".to_string(),
                        vec!["purple".to_string(), "turquoise".to_string()],
                    ),
                    (r"^release.*$".to_string(), vec!["green".to_string()]),
                    (r"^(bugfix|hotfix).*$".to_string(), vec!["red".to_string()]),
                    (r"^tags/.*$".to_string(), vec!["green".to_string()]),
                ],
                unknown: vec!["gray".to_string()],
            },
        }
    }

    /// Simple feature-based model.
    pub fn simple() -> Self {
        BranchSettingsDef {
            persistence: vec![r"^(master|main|trunk)$".to_string()],
            order: vec![
                r"^tags/.*$".to_string(),
                r"^(master|main|trunk)$".to_string(),
            ],
            terminal_colors: ColorsDef {
                matches: vec![
                    (
                        r"^(master|main|trunk)$".to_string(),
                        vec!["bright_blue".to_string()],
                    ),
                    (r"^tags/.*$".to_string(), vec!["bright_green".to_string()]),
                ],
                unknown: vec![
                    "bright_yellow".to_string(),
                    "bright_green".to_string(),
                    "bright_red".to_string(),
                    "bright_magenta".to_string(),
                    "bright_cyan".to_string(),
                ],
            },

            svg_colors: ColorsDef {
                matches: vec![
                    (
                        r"^(master|main|trunk)$".to_string(),
                        vec!["blue".to_string()],
                    ),
                    (r"^tags/.*$".to_string(), vec!["green".to_string()]),
                ],
                unknown: vec![
                    "orange".to_string(),
                    "green".to_string(),
                    "red".to_string(),
                    "purple".to_string(),
                    "turquoise".to_string(),
                ],
            },
        }
    }

    /// Very simple model without any defined branch roles.
    pub fn none() -> Self {
        BranchSettingsDef {
            persistence: vec![],
            order: vec![],
            terminal_colors: ColorsDef {
                matches: vec![],
                unknown: vec![
                    "bright_blue".to_string(),
                    "bright_yellow".to_string(),
                    "bright_green".to_string(),
                    "bright_red".to_string(),
                    "bright_magenta".to_string(),
                    "bright_cyan".to_string(),
                ],
            },

            svg_colors: ColorsDef {
                matches: vec![],
                unknown: vec![
                    "blue".to_string(),
                    "orange".to_string(),
                    "green".to_string(),
                    "red".to_string(),
                    "purple".to_string(),
                    "turquoise".to_string(),
                ],
            },
        }
    }
}

/// Settings defining branching models
pub struct BranchSettings {
    /// Branch persistence
    pub persistence: Vec<Regex>,
    /// Branch ordering
    pub order: Vec<Regex>,
    /// Branch colors
    pub terminal_colors: Vec<(Regex, Vec<String>)>,
    /// Colors for branches not matching any of `colors`
    pub terminal_colors_unknown: Vec<String>,
    /// Branch colors for SVG output
    pub svg_colors: Vec<(Regex, Vec<String>)>,
    /// Colors for branches not matching any of `colors` for SVG output
    pub svg_colors_unknown: Vec<String>,
}

impl BranchSettings {
    pub fn from(def: BranchSettingsDef) -> Result<Self, Error> {
        let persistence = def
            .persistence
            .iter()
            .map(|str| Regex::new(str))
            .collect::<Result<Vec<_>, Error>>()?;

        let order = def
            .order
            .iter()
            .map(|str| Regex::new(str))
            .collect::<Result<Vec<_>, Error>>()?;

        let terminal_colors = def
            .terminal_colors
            .matches
            .into_iter()
            .map(|(str, vec)| Regex::new(&str).map(|re| (re, vec)))
            .collect::<Result<Vec<_>, Error>>()?;

        let terminal_colors_unknown = def.terminal_colors.unknown;

        let svg_colors = def
            .svg_colors
            .matches
            .into_iter()
            .map(|(str, vec)| Regex::new(&str).map(|re| (re, vec)))
            .collect::<Result<Vec<_>, Error>>()?;

        let svg_colors_unknown = def.svg_colors.unknown;

        Ok(BranchSettings {
            persistence,
            order,
            terminal_colors,
            terminal_colors_unknown,
            svg_colors,
            svg_colors_unknown,
        })
    }
}

/// RegEx patterns for extracting branch names from merge commit summaries.
pub struct MergePatterns {
    /// The patterns. Evaluated in the given order.
    pub patterns: Vec<Regex>,
}

impl Default for MergePatterns {
    fn default() -> Self {
        MergePatterns {
            patterns: vec![
                // GitLab pull request
                Regex::new(r"^Merge branch '(.+)' into '.+'$").unwrap(),
                // Git default
                Regex::new(r"^Merge branch '(.+)' into .+$").unwrap(),
                // Git default into main branch
                Regex::new(r"^Merge branch '(.+)'$").unwrap(),
                // GitHub pull request
                Regex::new(r"^Merge pull request #[0-9]+ from .[^/]+/(.+)$").unwrap(),
                // GitHub pull request (from fork?)
                Regex::new(r"^Merge branch '(.+)' of .+$").unwrap(),
                // BitBucket pull request
                Regex::new(r"^Merged in (.+) \(pull request #[0-9]+\)$").unwrap(),
                // Gitea pull request
                Regex::new(r"^Merge pull request '.*' \(#[0-9]+\) from (.+) into .+$").unwrap(),
            ],
        }
    }
}

/// The characters used for drawing text-based graphs.
pub struct Characters {
    pub chars: Vec<char>,
}

impl FromStr for Characters {
    type Err = String;

    fn from_str(str: &str) -> Result<Self, Self::Err> {
        match str {
            "normal" | "thin" | "n" | "t" => Ok(Characters::thin()),
            "round" | "r" => Ok(Characters::round()),
            "bold" | "b" => Ok(Characters::bold()),
            "double" | "d" => Ok(Characters::double()),
            "ascii" | "a" => Ok(Characters::ascii()),
            _ => Err(format!("Unknown characters/style '{}'. Must be one of [normal|thin|round|bold|double|ascii]", str)),
        }
    }
}

impl Characters {
    /// Default/thin graphs
    pub fn thin() -> Self {
        Characters {
            chars: " ●○│─┼└┌┐┘┤├┴┬<>".chars().collect(),
        }
    }
    /// Graphs with rounded corners
    pub fn round() -> Self {
        Characters {
            chars: " ●○│─┼╰╭╮╯┤├┴┬<>".chars().collect(),
        }
    }
    /// Bold/fat graphs
    pub fn bold() -> Self {
        Characters {
            chars: " ●○┃━╋┗┏┓┛┫┣┻┳<>".chars().collect(),
        }
    }
    /// Double-lined graphs
    pub fn double() -> Self {
        Characters {
            chars: " ●○║═╬╚╔╗╝╣╠╩╦<>".chars().collect(),
        }
    }
    /// ASCII-only graphs
    pub fn ascii() -> Self {
        Characters {
            chars: " *o|-+'..'||++<>".chars().collect(),
        }
    }

    pub fn reverse(self) -> Self {
        let mut chars = self.chars;

        chars.swap(6, 8);
        chars.swap(7, 9);
        chars.swap(10, 11);
        chars.swap(12, 13);
        chars.swap(14, 15);

        Characters { chars }
    }
}
