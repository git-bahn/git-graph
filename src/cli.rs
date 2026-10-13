use std::convert::Infallible;
use std::fmt;

use clap::{ArgAction, Args, Parser, Subcommand, ValueEnum};
use camino::Utf8PathBuf;

pub type WrapType = Option<(Option<usize>, Option<usize>, Option<usize>)>;

/// Structured Git graphs for your branching model
///
/// https://github.com/mlange-42/git-graph
///
/// Examples:
///     git-graph                   Show graph
///     git-graph --style round     Show graph in a different style
///     git-graph --model <model>   Show graph using a certain <model>
///     git-graph model --list      List available branching models
///     git-graph model             Show repo's current branching model
///     git-graph model <model>     Permanently set model <model> for this repo
#[derive(Parser)]
#[command(version, verbatim_doc_comment)]
pub struct Cli {
    /// Open repository from this path or above
    #[arg(long, short, value_name = "path", default_value = ".")]
    pub path: Utf8PathBuf,

    /// Skip owner validation for the repository (insecure, see --help)
    ///
    /// This turns off libgit2's owner validation, which may increase security risks.
    /// Please do not disable this validation for repositories you do not trust.
    #[arg(long)]
    pub skip_repo_owner_validation: bool,

    /// Branching model: git-flow, simple or none
    ///
    /// Defaults to the model set for the repository, or git-flow if none is set.
    /// To permanently set the model for a repository, use 'git-graph model <model>'.
    #[arg(long, short, value_name = "model")]
    pub model: Option<Model>,

    /// Maximum number of commits to show
    #[arg(long, short = 'n', value_name = "n")]
    pub max_count: Option<u32>,

    /// When to use colors
    #[arg(long, value_name = "color", default_value = "auto")]
    pub color: ColorMode,

    /// Print without colors, overrides --color
    ///
    /// Missing color support should be detected automatically,
    /// e.g. when piping to a file.
    #[arg(long)]
    pub no_color: bool,

    /// Line wrapping for formatted commit text
    ///
    /// Argument format: [<width>|auto|none [<indent1> [<indent2>]]]
    /// 'auto' uses the terminal's width if on a terminal.
    ///
    /// Examples:
    ///     git-graph --wrap auto
    ///     git-graph --wrap auto 0 8
    ///     git-graph --wrap none
    ///     git-graph --wrap 80
    ///     git-graph --wrap 80 0 8
    #[arg(
        long,
        short,
        value_name = "wrap",
        action = ArgAction::Set,
        num_args = 0..=3,
        default_values = ["auto", "0", "8"],
        verbatim_doc_comment
    )]
    pub wrap: Vec<WrapToken>,

    /// Commit format: oneline, short, medium, full or "<string>"
    ///
    /// The first character can be used as abbreviation, e.g. '-f m'.
    ///
    /// Formatting placeholders for "<string>":
    ///     %n    newline
    ///     %H    commit hash
    ///     %h    abbreviated commit hash
    ///     %P    parent commit hashes
    ///     %p    abbreviated parent commit hashes
    ///     %d    refs (branches, tags)
    ///     %s    commit summary
    ///     %b    commit message body
    ///     %B    raw body (subject and body)
    ///     %an   author name
    ///     %ae   author email
    ///     %ad   author date
    ///     %as   author date in short format 'YYYY-MM-DD'
    ///     %cn   committer name
    ///     %ce   committer email
    ///     %cd   committer date
    ///     %cs   committer date in short format 'YYYY-MM-DD'
    ///
    /// Modifiers after the % of a placeholder:
    ///     +     (plus sign) a line-feed is inserted immediately before the
    ///           expansion if and only if the placeholder expands to a
    ///           non-empty string
    ///     -     (minus sign) all consecutive line-feeds immediately preceding
    ///           the expansion are deleted if and only if the placeholder
    ///           expands to an empty string
    ///     ' '   (space) a space is inserted immediately before the expansion
    ///           if and only if the placeholder expands to a non-empty string
    ///
    /// See also the respective git help: https://git-scm.com/docs/pretty-formats
    #[arg(
        long,
        short,
        value_name = "format",
        default_value = "oneline",
        verbatim_doc_comment
    )]
    pub format: String,

    //
    //  other flags
    //
    /// Reverse the order of commits
    #[arg(long, short)]
    pub reverse: bool,

    /// Show only local branches, no remotes
    #[arg(long, short)]
    pub local: bool,

    /// Render graph as SVG instead of text-based
    #[arg(long)]
    pub svg: bool,

    /// Additional debug output and graphics
    #[arg(long, short)]
    pub debug: bool,

    /// Print a less compact graph
    ///
    /// Merge lines point to target lines rather than merge commits.
    #[arg(long, short = 'S')]
    pub sparse: bool,

    /// Output style: thin/normal, round, bold, double or ascii
    ///
    /// The first character can be used as abbreviation, e.g. '-s r'.
    #[arg(long, short, value_name = "style", default_value = "thin")]
    pub style: String,

    /// Branch names or refspecs to show
    ///
    /// Only the subgraph between the merge-base and the tips is displayed.
    #[arg(value_name = "refspecs", num_args = 1..)]
    pub refspecs: Vec<String>,

    #[command(subcommand)]
    pub command: Option<CliCommand>,
}

#[derive(Subcommand)]
pub enum CliCommand {
    /// Print or permanently set the branching model for a repository
    Model(ModelArgs),
}

#[derive(Args)]
pub struct ModelArgs {
    /// Branching model to set: git-flow, simple or none
    ///
    /// When not given, prints the currently set model.
    #[arg(value_name = "model")]
    pub model: Option<Model>,

    /// List all available branching models
    #[arg(long, short)]
    pub list: bool,
}

/// Branching model, either one of the presets or a custom model
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Model {
    GitFlow,
    Simple,
    None,
    /// Name of a custom model file in the models directory, without `.toml`
    Custom(String),
}

impl Model {
    pub fn as_str(&self) -> &str {
        match self {
            Model::GitFlow => "git-flow",
            Model::Simple => "simple",
            Model::None => "none",
            Model::Custom(name) => name,
        }
    }
}

impl std::str::FromStr for Model {
    type Err = Infallible;

    fn from_str(str: &str) -> Result<Self, Self::Err> {
        Ok(match str {
            "git-flow" => Model::GitFlow,
            "simple" => Model::Simple,
            "none" => Model::None,
            other => Model::Custom(other.to_string()),
        })
    }
}

impl fmt::Display for Model {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// When to use colors
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum ColorMode {
    /// Use colors if printing to a terminal
    Auto,
    /// Always use colors
    Always,
    /// Never use colors
    Never,
}

/// A single value passed to `--wrap`
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WrapToken {
    None,
    Auto,
    Number(usize),
}

impl std::str::FromStr for WrapToken {
    type Err = String;

    fn from_str(str: &str) -> Result<Self, Self::Err> {
        match str {
            "none" => Ok(WrapToken::None),
            "auto" => Ok(WrapToken::Auto),
            _ => str
                .parse()
                .map(WrapToken::Number)
                .map_err(|_| "expected 'auto', 'none' or a positive number".to_string()),
        }
    }
}

impl fmt::Display for WrapToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WrapToken::None => f.write_str("none"),
            WrapToken::Auto => f.write_str("auto"),
            WrapToken::Number(number) => write!(f, "{}", number),
        }
    }
}

/// Line wrapping for formatted commit text, built from the values passed to `--wrap`
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wrap {
    /// No wrapping
    None,
    /// Wrap at the terminal's width if on a terminal
    Auto {
        indent1: Option<usize>,
        indent2: Option<usize>,
    },
    /// Wrap at a fixed width
    Width {
        width: usize,
        indent1: Option<usize>,
        indent2: Option<usize>,
    },
}

impl Wrap {
    pub fn into_settings(self) -> WrapType {
        match self {
            Wrap::None => None,
            Wrap::Auto { indent1, indent2 } => Some((None, indent1, indent2)),
            Wrap::Width {
                width,
                indent1,
                indent2,
            } => Some((Some(width), indent1, indent2)),
        }
    }
}

impl TryFrom<&[WrapToken]> for Wrap {
    type Error = String;

    fn try_from(tokens: &[WrapToken]) -> Result<Self, Self::Error> {
        // Indents following the first value must be numbers
        let indents = |rest: &[WrapToken]| {
            rest.iter()
                .map(|token| match token {
                    WrapToken::Number(number) => Ok(*number),
                    _ => Err(format!(
                        "ERROR: Can't parse option --wrap '{}' to integers.",
                        itertools::join(tokens, " ")
                    )),
                })
                .collect::<Result<Vec<_>, _>>()
        };

        match tokens {
            [] => Ok(Wrap::Auto {
                indent1: Some(0),
                indent2: Some(8),
            }),
            [WrapToken::None, ..] => Ok(Wrap::None),
            [WrapToken::Auto, rest @ ..] => {
                let indents = indents(rest)?;
                Ok(Wrap::Auto {
                    indent1: indents.first().copied(),
                    indent2: indents.get(1).copied(),
                })
            }
            [WrapToken::Number(width), rest @ ..] => {
                let indents = indents(rest)?;
                Ok(Wrap::Width {
                    width: *width,
                    indent1: indents.first().copied(),
                    indent2: indents.get(1).copied(),
                })
            }
        }
    }
}

