//! Command line tool to show clear git graphs arranged for your branching model.

// Configure clippy to look for complex functions
#![warn(clippy::cognitive_complexity)]
#![warn(clippy::too_many_lines)]

// TODO remove code once gleisbau API has stabilized
// Some of these features might return to the CLI tool (this application)
//mod config;
mod print;

use crate::print::svg::print_svg;
use camino::Utf8PathBuf;
use clap::{ArgAction, Args, Parser, Subcommand, ValueEnum};
use git2::Repository;
use gleisbau::config::{create_config, get_available_models, get_model, get_model_name, set_model};
use gleisbau::get_repo;
use gleisbau::graph::Builder as GraphBuilder;
use gleisbau::print::format::CommitFormat;
use gleisbau::print::unicode::print_unicode;
use gleisbau::settings::{
    BranchOrder, BranchSettings, BranchSettingsDef, Characters, MergePatterns, Settings,
};
use itertools::enumerate;
use platform_dirs::AppDirs;
use std::path::PathBuf;
use std::borrow::Borrow;
use std::convert::Infallible;
use std::fmt;
use std::rc::Rc;
use std::str::FromStr;
use std::time::Instant;

const REPO_CONFIG_FILE: &str = "git-graph.toml";

fn main() {
    std::process::exit(match from_args() {
        Ok(_) => 0,
        Err(err) => {
            eprintln!("{}", err);
            1
        }
    });
}

fn from_args() -> Result<(), String> {
    let mut ses = Session::new();
    store_default_models(&mut ses)?;

    let cli = Cli::parse();
    if !configure_session(&mut ses, &cli)? {
        return Ok(()); // If the configuration decided session should not start
    };

    run(
        ses.repository.unwrap(),
        Rc::new(ses.settings.unwrap()),
        ses.svg,
        ses.commit_limit,
        ses.refspecs,
    )
}

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
struct Cli {
    /// Open repository from this path or above
    #[arg(long, short, value_name = "path", default_value = ".")]
    path: Utf8PathBuf,

    /// Skip owner validation for the repository (insecure, see --help)
    ///
    /// This turns off libgit2's owner validation, which may increase security risks.
    /// Please do not disable this validation for repositories you do not trust.
    #[arg(long)]
    skip_repo_owner_validation: bool,

    /// Branching model: git-flow, simple or none
    ///
    /// Defaults to the model set for the repository, or git-flow if none is set.
    /// To permanently set the model for a repository, use 'git-graph model <model>'.
    #[arg(long, short, value_name = "model")]
    model: Option<Model>,

    /// Maximum number of commits to show
    #[arg(long, short = 'n', value_name = "n")]
    max_count: Option<u32>,

    /// When to use colors
    #[arg(long, value_name = "color", default_value = "auto")]
    color: ColorMode,

    /// Print without colors, overrides --color
    ///
    /// Missing color support should be detected automatically,
    /// e.g. when piping to a file.
    #[arg(long)]
    no_color: bool,

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
    wrap: Vec<WrapToken>,

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
    format: String,

    //
    //  other flags
    //
    /// Reverse the order of commits
    #[arg(long, short)]
    reverse: bool,

    /// Show only local branches, no remotes
    #[arg(long, short)]
    local: bool,

    /// Render graph as SVG instead of text-based
    #[arg(long)]
    svg: bool,

    /// Additional debug output and graphics
    #[arg(long, short)]
    debug: bool,

    /// Print a less compact graph
    ///
    /// Merge lines point to target lines rather than merge commits.
    #[arg(long, short = 'S')]
    sparse: bool,

    /// Output style: thin/normal, round, bold, double or ascii
    ///
    /// The first character can be used as abbreviation, e.g. '-s r'.
    #[arg(long, short, value_name = "style", default_value = "thin")]
    style: String,

    /// Branch names or refspecs to show
    ///
    /// Only the subgraph between the merge-base and the tips is displayed.
    #[arg(value_name = "refspecs", num_args = 1..)]
    refspecs: Vec<String>,

    #[command(subcommand)]
    command: Option<CliCommand>,
}

#[derive(Subcommand)]
enum CliCommand {
    /// Print or permanently set the branching model for a repository
    Model(ModelArgs),
}

#[derive(Args)]
struct ModelArgs {
    /// Branching model to set: git-flow, simple or none
    ///
    /// When not given, prints the currently set model.
    #[arg(value_name = "model")]
    model: Option<Model>,

    /// List all available branching models
    #[arg(long, short)]
    list: bool,
}

/// Branching model, either one of the presets or a custom model
#[derive(Clone, Debug, PartialEq, Eq)]
enum Model {
    GitFlow,
    Simple,
    None,
    /// Name of a custom model file in the models directory, without `.toml`
    Custom(String),
}

impl Model {
    fn as_str(&self) -> &str {
        match self {
            Model::GitFlow => "git-flow",
            Model::Simple => "simple",
            Model::None => "none",
            Model::Custom(name) => name,
        }
    }
}

impl FromStr for Model {
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
enum ColorMode {
    /// Use colors if printing to a terminal
    Auto,
    /// Always use colors
    Always,
    /// Never use colors
    Never,
}

/// A single value passed to `--wrap`
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WrapToken {
    None,
    Auto,
    Number(usize),
}

impl FromStr for WrapToken {
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
enum Wrap {
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
    fn into_settings(self) -> WrapType {
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

/// Return true if session should continue, false if it should exit now
fn configure_session(ses: &mut Session, cli: &Cli) -> Result<bool, String> {
    // return values
    let exit_now = false;
    let run_application = true;

    if match_model_list(ses, cli)? {
        return Ok(exit_now); // Exit after showing model list
    }

    match_repo_args(ses, cli)?;

    if match_model_subcommand(ses, cli)? {
        return Ok(exit_now); // Exit after model subcommand
    }
    match_commit_limit_args(ses, cli)?;

    let include_remote = !cli.local;

    let reverse_commit_order = cli.reverse;

    ses.svg = cli.svg;
    let compact = !cli.sparse;
    let debug = cli.debug;
    let style = Characters::from_str(&cli.style)?;

    let style = if reverse_commit_order {
        style.reverse()
    } else {
        style
    };

    let model = match_model_opt(ses, cli)?;

    let format = match_format_args(ses, cli)?;

    let colored = match_color_args(ses, cli)?;

    let wrapping = match_wrap_args(ses, cli)?;

    let settings = Settings {
        reverse_commit_order,
        debug,
        colored,
        compact,
        include_remote,
        format,
        wrapping,
        characters: style,
        branch_order: BranchOrder::ShortestFirst(true),
        branches: BranchSettings::from(model).map_err(|err| err.to_string())?,
        merge_patterns: MergePatterns::default(),
    };
    ses.settings = Some(settings);

    ses.refspecs = cli.refspecs.clone();

    Ok(run_application)
}

struct Session {
    // models related fields
    models_dir: PathBuf,

    // Settings related fields
    pub settings: Option<Settings>,

    // Other fields
    pub repository: Option<Repository>,
    pub svg: bool,
    pub commit_limit: Option<usize>,
    pub refspecs: Vec<String>,
}

impl Session {
    pub fn new() -> Self {
        Self {
            // models related fields
            models_dir: PathBuf::new(),

            // Settings related fields
            settings: None,

            // Other fields
            repository: None,
            svg: false,
            commit_limit: None,
            refspecs: Vec::new(),
        }
    }
}

fn match_repo_args(ses: &mut Session, cli: &Cli) -> Result<(), String> {
    let skip_repo_owner_validation = cli.skip_repo_owner_validation;
    if skip_repo_owner_validation {
        println!("Warning: skip-repo-owner-validation is set! ");
    }
    let repository = get_repo(&cli.path, skip_repo_owner_validation)
        .map_err(|err| format!("ERROR: {}\n       Navigate into a repository before running git-graph, or use option --path", err.message()))?;

    ses.repository = Some(repository);
    Ok(())
}

//
//  "model" subcommand
//

/// Fill APP_dir/git-graph folder with default models
fn store_default_models(ses: &mut Session) -> Result<(), String> {
    let app_dir = AppDirs::new(Some("git-graph"), false).unwrap().config_dir;
    let mut models_dir = app_dir;
    models_dir.push("models");

    create_config(&models_dir)?;

    ses.models_dir = models_dir;
    Ok(())
}

fn match_model_list(ses: &mut Session, cli: &Cli) -> Result<bool, String> {
    if let Some(CliCommand::Model(model_args)) = &cli.command {
        if model_args.list {
            println!(
                "{}",
                itertools::join(get_available_models(&ses.models_dir)?, "\n")
            );
            return Ok(true);
        }
    }
    Ok(false)
}

fn match_model_subcommand(ses: &mut Session, cli: &Cli) -> Result<bool, String> {
    if let Some(CliCommand::Model(model_args)) = &cli.command {
        let repository = ses.repository.as_ref().unwrap();
        match &model_args.model {
            None => {
                let curr_model = get_model_name(repository, REPO_CONFIG_FILE)?;
                match curr_model {
                    None => print!("No branching model set"),
                    Some(model) => print!("{}", model),
                }
            }
            Some(model) => {
                set_model(repository, model.as_str(), REPO_CONFIG_FILE, &ses.models_dir)?;
                eprint!("Branching model set to '{}'", model);
            }
        };
        return Ok(true);
    }
    Ok(false)
}

fn match_model_opt(ses: &mut Session, cli: &Cli) -> Result<BranchSettingsDef, String> {
    let model = get_model(
        ses.repository.as_ref().unwrap(),
        cli.model.as_ref().map(Model::as_str),
        REPO_CONFIG_FILE,
        &ses.models_dir,
    )?;
    Ok(model)
}

fn match_commit_limit_args(ses: &mut Session, cli: &Cli) -> Result<(), String> {
    ses.commit_limit = cli.max_count.map(|max_count| max_count as usize);

    Ok(())
}

fn match_color_args(_ses: &mut Session, cli: &Cli) -> Result<bool, String> {
    let colored = if cli.no_color {
        false
    } else {
        match cli.color {
            ColorMode::Auto => {
                atty::is(atty::Stream::Stdout)
                    && (!cfg!(windows) || {
                        yansi::enable();
                        yansi::is_enabled()
                    })
            }
            ColorMode::Always => {
                if cfg!(windows) {
                    yansi::enable();
                }
                true
            }
            ColorMode::Never => false,
        }
    };

    Ok(colored)
}

//
//  wrap flag
//

type WrapType = Option<(Option<usize>, Option<usize>, Option<usize>)>;
fn match_wrap_args(_ses: &mut Session, cli: &Cli) -> Result<WrapType, String> {
    let wrap = Wrap::try_from(&cli.wrap[..])?;
    Ok(wrap.into_settings())
}

fn match_format_args(_ses: &mut Session, cli: &Cli) -> Result<CommitFormat, String> {
    let format = CommitFormat::from_str(&cli.format)?;
    Ok(format)
}

//
//  Run application
//

fn run(
    repository: Repository,
    settings: Rc<Settings>,
    svg: bool,
    max_commits: Option<usize>,
    refspecs: Vec<String>,
) -> Result<(), String> {
    let now = Instant::now();
    let mut graph_builder = GraphBuilder::new()
        .with_repository(repository)
        .with_settings(settings.clone())
        .with_refspecs(refspecs);
    if let Some(max_commits) = max_commits {
        graph_builder = graph_builder.with_max_count(max_commits);
    }
    let graph = graph_builder.build()?;
    let settings: &Settings = settings.borrow();

    let duration_graph = now.elapsed().as_micros();

    if settings.debug {
        let tracks = &graph.tracks;
        for (br_inx, branch) in enumerate(&tracks.all_branches) {
            let Some(branch_vis) = graph.layout.track_visual(br_inx.into()) else {
                eprintln!(
                    "#{} {} (col --) ({:?}) {} s: --, t: --",
                    br_inx,
                    branch.name,
                    branch.range,
                    if branch.is_merged { "m" } else { "" },
                );
                continue;
            };
            eprintln!(
                "#{} {} (col {}) ({:?}) {} s: {:?}, t: {:?}",
                br_inx,
                branch.name,
                branch_vis.column.unwrap_or(99),
                branch.range,
                if branch.is_merged { "m" } else { "" },
                branch_vis.source_order_group,
                branch_vis.target_order_group
            );
        }
    }

    let now = Instant::now();

    if svg {
        println!("{}", print_svg(&graph, settings)?);
    } else {
        let (g_lines, t_lines, _indices) = print_unicode(&graph, settings)?;
        print_unpaged(&g_lines, &t_lines);
    };

    let duration_print = now.elapsed().as_micros();

    if settings.debug {
        eprintln!(
            "Graph construction: {:.1} ms, printing: {:.1} ms ({} commits)",
            duration_graph as f32 / 1000.0,
            duration_print as f32 / 1000.0,
            graph.tracks.commits.len()
        );
    }
    Ok(())
}

/// Print the graph, un-paged.
fn print_unpaged(graph_lines: &[String], text_lines: &[String]) {
    for (g_line, t_line) in graph_lines.iter().zip(text_lines.iter()) {
        println!(" {}  {}", g_line, t_line);
    }
}
