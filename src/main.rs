//! Command line tool to show clear git graphs arranged for your branching model.

// Configure clippy to look for complex functions
#![warn(clippy::cognitive_complexity)]
#![warn(clippy::too_many_lines)]

// TODO remove code once gleisbau API has stabilized
// Some of these features might return to the CLI tool (this application)
//mod config;
mod print;

use crate::print::svg::print_svg;
use clap::ArgMatches;
use clap::{crate_version, Arg, Command};
use git2::Repository;
use gleisbau::config::{create_config, get_available_models, get_model, get_model_name, set_model};
use gleisbau::get_repo;
use gleisbau::graph::Builder as GraphBuilder;
use gleisbau::print::format::CommitFormat;
use gleisbau::print::unicode::print_unicode;
use gleisbau::settings::{
    BranchOrder, BranchSettings, BranchSettingsDef, Characters, MergePatterns, Settings,
};
use platform_dirs::AppDirs;
use std::path::PathBuf;
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

    let matches = match_args();
    if !configure_session(&mut ses, &matches)? {
        return Ok(()); // If the configuration decided session should not start
    };

    run(
        ses.repository.unwrap(),
        ses.settings.as_ref().unwrap(),
        ses.svg,
        ses.commit_limit,
        ses.refspecs,
    )
}

fn match_args() -> ArgMatches {
    // Declare command line argument interface for clap
    let app = Command::new("git-graph").version(crate_version!()).about(
        "Structured Git graphs for your branching model.\n    \
                 https://github.com/mlange-42/git-graph\n\
             \n\
             EXAMPES:\n    \
                 git-graph                   -> Show graph\n    \
                 git-graph --style round     -> Show graph in a different style\n    \
                 git-graph --model <model>   -> Show graph using a certain <model>\n    \
                 git-graph model --list      -> List available branching models\n    \
                 git-graph model             -> Show repo's current branching models\n    \
                 git-graph model <model>     -> Permanently set model <model> for this repo",
    );
    let app = add_repo_args(app);
    let app = add_model_args(app);
    let app = add_commit_limit_args(app);
    let app = add_color_args(app);
    let app = add_wrap_args(app);
    let app = add_format_args(app);

    let app = app
        .arg(
            Arg::new("reverse")
                .long("reverse")
                .short('r')
                .help("Reverse the order of commits.")
                .required(false)
                .num_args(0),
        )
        .arg(
            Arg::new("local")
                .long("local")
                .short('l')
                .help("Show only local branches, no remotes.")
                .required(false)
                .num_args(0),
        )
        .arg(
            Arg::new("svg")
                .long("svg")
                .help("Render graph as SVG instead of text-based.")
                .required(false)
                .num_args(0),
        )
        .arg(
            Arg::new("debug")
                .long("debug")
                .short('d')
                .help("Additional debug output and graphics.")
                .required(false)
                .num_args(0),
        )
        .arg(
            Arg::new("sparse")
                .long("sparse")
                .short('S')
                .help(
                    "Print a less compact graph: merge lines point to target lines\n\
                       rather than merge commits.",
                )
                .required(false)
                .num_args(0),
        )
        .arg(
            Arg::new("style")
                .long("style")
                .short('s')
                .help(
                    "Output style. One of [normal/thin|round|bold|double|ascii].\n  \
                         (First character can be used as abbreviation, e.g. '-s r')",
                )
                .required(false)
                .num_args(1),
        )
        .arg(
            Arg::new("refspecs")
                .help(
                    "Branch names or refspecs to show.\n  \
                         Only the subgraph between the merge-base and the tips is displayed.",
                )
                .num_args(1..),
        );

    // Return match of declared arguments with what is present on command line
    app.get_matches()
}

/// Return true if session should continue, false if it should exit now
fn configure_session(ses: &mut Session, matches: &ArgMatches) -> Result<bool, String> {
    // return values
    let exit_now = false;
    let run_application = true;

    if match_model_list(ses, matches)? {
        return Ok(exit_now); // Exit after showing model list
    }

    match_repo_args(ses, matches)?;

    if match_model_subcommand(ses, matches)? {
        return Ok(exit_now); // Exit after model subcommand
    }
    match_commit_limit_args(ses, matches)?;

    let include_remote = !matches.get_flag("local");

    let reverse_commit_order = matches.get_flag("reverse");

    ses.svg = matches.get_flag("svg");
    let compact = !matches.get_flag("sparse");
    let debug = matches.get_flag("debug");
    let style = matches
        .get_one::<String>("style")
        .map(|s| Characters::from_str(s))
        .unwrap_or_else(|| Ok(Characters::thin()))?;

    let style = if reverse_commit_order {
        style.reverse()
    } else {
        style
    };

    let model = match_model_opt(ses, matches)?;

    let format = match_format_args(ses, matches)?;

    let colored = match_color_args(ses, matches)?;

    let wrapping = match_wrap_args(ses, matches)?;

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

    ses.refspecs = matches
        .get_many::<String>("refspecs")
        .map(|vals| vals.cloned().collect())
        .unwrap_or_default();

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

fn add_repo_args(app: Command) -> Command {
    app.arg(
        Arg::new("path")
            .long("path")
            .short('p')
            .help("Open repository from this path or above. Default '.'")
            .required(false)
            .num_args(1),
    )
    .arg(
        Arg::new("skip-repo-owner-validation")
            .long("skip-repo-owner-validation")
            .help(
                "Skip owner validation for the repository.\n\
                This will turn off libgit2's owner validation, which may increase security risks.\n\
                Please do not disable this validation for repositories you do not trust.",
            )
            .required(false)
            .num_args(0),
    )
}

fn match_repo_args(ses: &mut Session, matches: &ArgMatches) -> Result<(), String> {
    let skip_repo_owner_validation = matches.get_flag("skip-repo-owner-validation");
    if skip_repo_owner_validation {
        println!("Warning: skip-repo-owner-validation is set! ");
    }
    let default_path = ".".to_string();
    let path = matches.get_one::<String>("path").unwrap_or(&default_path);
    let repository = get_repo(path, skip_repo_owner_validation)
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

fn add_model_args(app: Command) -> Command {
    app
        .arg(
            Arg::new("model")
                .long("model")
                .short('m')
                .help("Branching model. Available presets are [simple|git-flow|none].\n\
                       Default: git-flow. \n\
                       Permanently set the model for a repository with\n\
                         > git-graph model <model>")
                .required(false)
                .num_args(1),
        )
        .subcommand(Command::new("model")
        .about("Prints or permanently sets the branching model for a repository.")
        .arg(
            Arg::new("model")
                .help("The branching model to be used. Available presets are [simple|git-flow|none].\n\
                        When not given, prints the currently set model.")
                .value_name("model")
                .num_args(1)
                .required(false)
                .index(1))
        .arg(
            Arg::new("list")
                .long("list")
                .short('l')
                .help("List all available branching models.")
                .required(false)
                .num_args(0),
    ))
}

fn match_model_list(ses: &mut Session, matches: &ArgMatches) -> Result<bool, String> {
    if let Some(matches) = matches.subcommand_matches("model") {
        if matches.get_flag("list") {
            println!(
                "{}",
                itertools::join(get_available_models(&ses.models_dir)?, "\n")
            );
            return Ok(true);
        }
    }
    Ok(false)
}

fn match_model_subcommand(ses: &mut Session, matches: &ArgMatches) -> Result<bool, String> {
    if let Some(matches) = matches.subcommand_matches("model") {
        let repository = ses.repository.as_ref().unwrap();
        match matches.get_one::<String>("model") {
            None => {
                let curr_model = get_model_name(repository, REPO_CONFIG_FILE)?;
                match curr_model {
                    None => print!("No branching model set"),
                    Some(model) => print!("{}", model),
                }
            }
            Some(model) => {
                set_model(repository, model, REPO_CONFIG_FILE, &ses.models_dir)?;
                eprint!("Branching model set to '{}'", model);
            }
        };
        return Ok(true);
    }
    Ok(false)
}

fn match_model_opt(ses: &mut Session, matches: &ArgMatches) -> Result<BranchSettingsDef, String> {
    let model = get_model(
        ses.repository.as_ref().unwrap(),
        matches.get_one::<String>("model").map(|s| &s[..]),
        REPO_CONFIG_FILE,
        &ses.models_dir,
    )?;
    Ok(model)
}

//
//  commit_limit flag
//

fn add_commit_limit_args(app: Command) -> Command {
    app.arg(
        Arg::new("max-count")
            .long("max-count")
            .short('n')
            .help("Maximum number of commits")
            .required(false)
            .num_args(1)
            .value_name("n"),
    )
}

fn match_commit_limit_args(ses: &mut Session, matches: &ArgMatches) -> Result<(), String> {
    ses.commit_limit = match matches.get_one::<String>("max-count") {
        None => None,
        Some(str) => match str.parse::<usize>() {
            Ok(val) => Some(val),
            Err(_) => {
                return Err(format![
                    "Option max-count must be a positive number, but got '{}'",
                    str
                ])
            }
        },
    };

    Ok(())
}

//
//  color flag
//

fn add_color_args(app: Command) -> Command {
    app.arg(
        Arg::new("color")
            .long("color")
            .help(
                "Specify when colors should be used. One of [auto|always|never].\n\
                       Default: auto.",
            )
            .required(false)
            .num_args(1),
    )
    .arg(
        Arg::new("no-color")
            .long("no-color")
            .help(
                "Print without colors. Missing color support should be detected\n\
                       automatically (e.g. when piping to a file).\n\
                       Overrides option '--color'",
            )
            .required(false)
            .num_args(0),
    )
}

fn match_color_args(_ses: &mut Session, matches: &ArgMatches) -> Result<bool, String> {
    let colored = if matches.get_flag("no-color") {
        false
    } else if let Some(mode) = matches.get_one::<String>("color") {
        match &mode[..] {
            "auto" => {
                atty::is(atty::Stream::Stdout)
                    && (!cfg!(windows) || {
                        yansi::enable();
                        yansi::is_enabled()
                    })
            }
            "always" => {
                if cfg!(windows) {
                    yansi::enable();
                }
                true
            }
            "never" => false,
            other => {
                return Err(format!(
                    "Unknown color mode '{}'. Supports [auto|always|never].",
                    other
                ))
            }
        }
    } else {
        atty::is(atty::Stream::Stdout)
            && (!cfg!(windows) || {
                yansi::enable();
                yansi::is_enabled()
            })
    };

    Ok(colored)
}

//
//  wrap flag
//

fn add_wrap_args(app: Command) -> Command {
    app.arg(
        Arg::new("wrap")
            .long("wrap")
            .short('w')
            .help(
                "Line wrapping for formatted commit text. Default: 'auto 0 8'\n\
                       Argument format: [<width>|auto|none[ <indent1>[ <indent2>]]]\n\
                       For examples, consult 'git-graph --help'",
            )
            .long_help(
                "Line wrapping for formatted commit text. Default: 'auto 0 8'\n\
                       Argument format: [<width>|auto|none[ <indent1>[ <indent2>]]]\n\
                       Examples:\n    \
                           git-graph --wrap auto\n    \
                           git-graph --wrap auto 0 8\n    \
                           git-graph --wrap none\n    \
                           git-graph --wrap 80\n    \
                           git-graph --wrap 80 0 8\n\
                       'auto' uses the terminal's width if on a terminal.",
            )
            .required(false)
            .num_args(0..=3),
    )
}

type WrapType = Option<(Option<usize>, Option<usize>, Option<usize>)>;
fn match_wrap_args(_ses: &mut Session, matches: &ArgMatches) -> Result<WrapType, String> {
    let wrapping = if let Some(wrap_values) = matches.get_many::<String>("wrap") {
        let strings = wrap_values.map(|s| s.as_str()).collect::<Vec<_>>();
        if strings.is_empty() {
            Some((None, Some(0), Some(8)))
        } else {
            match strings[0] {
                "none" => None,
                "auto" => {
                    let wrap = strings
                        .iter()
                        .skip(1)
                        .map(|str| str.parse::<usize>())
                        .collect::<Result<Vec<_>, _>>()
                        .map_err(|_| {
                            format!(
                                "ERROR: Can't parse option --wrap '{}' to integers.",
                                strings.join(" ")
                            )
                        })?;
                    Some((None, wrap.first().cloned(), wrap.get(1).cloned()))
                }
                _ => {
                    let wrap = strings
                        .iter()
                        .map(|str| str.parse::<usize>())
                        .collect::<Result<Vec<_>, _>>()
                        .map_err(|_| {
                            format!(
                                "ERROR: Can't parse option --wrap '{}' to integers.",
                                strings.join(" ")
                            )
                        })?;
                    Some((
                        wrap.first().cloned(),
                        wrap.get(1).cloned(),
                        wrap.get(2).cloned(),
                    ))
                }
            }
        }
    } else {
        Some((None, Some(0), Some(8)))
    };

    Ok(wrapping)
}

//
//  commit format flags - format
//

fn add_format_args(app: Command) -> Command {
    app
        .arg(
            Arg::new("format")
                .long("format")
                .short('f')
                .help("Commit format. One of [oneline|short|medium|full|\"<string>\"].\n  \
                         (First character can be used as abbreviation, e.g. '-f m')\n\
                       Default: oneline.\n\
                       For placeholders supported in \"<string>\", consult 'git-graph --help'")
                .long_help("Commit format. One of [oneline|short|medium|full|\"<string>\"].\n  \
                              (First character can be used as abbreviation, e.g. '-f m')\n\
                            Formatting placeholders for \"<string>\":\n    \
                                %n    newline\n    \
                                %H    commit hash\n    \
                                %h    abbreviated commit hash\n    \
                                %P    parent commit hashes\n    \
                                %p    abbreviated parent commit hashes\n    \
                                %d    refs (branches, tags)\n    \
                                %s    commit summary\n    \
                                %b    commit message body\n    \
                                %B    raw body (subject and body)\n    \
                                %an   author name\n    \
                                %ae   author email\n    \
                                %ad   author date\n    \
                                %as   author date in short format 'YYYY-MM-DD'\n    \
                                %cn   committer name\n    \
                                %ce   committer email\n    \
                                %cd   committer date\n    \
                                %cs   committer date in short format 'YYYY-MM-DD'\n    \
                                \n    \
                                If you add a + (plus sign) after % of a placeholder,\n       \
                                   a line-feed is inserted immediately before the expansion if\n       \
                                   and only if the placeholder expands to a non-empty string.\n    \
                                If you add a - (minus sign) after % of a placeholder, all\n       \
                                   consecutive line-feeds immediately preceding the expansion are\n       \
                                   deleted if and only if the placeholder expands to an empty string.\n    \
                                If you add a ' ' (space) after % of a placeholder, a space is\n       \
                                   inserted immediately before the expansion if and only if\n       \
                                   the placeholder expands to a non-empty string.\n\
                            \n    \
                                See also the respective git help: https://git-scm.com/docs/pretty-formats\n")
                .required(false)
                .num_args(1),
        )
}

fn match_format_args(_ses: &mut Session, matches: &ArgMatches) -> Result<CommitFormat, String> {
    let format = match matches.get_one::<String>("format") {
        None => CommitFormat::OneLine,
        Some(str) => CommitFormat::from_str(str)?,
    };
    Ok(format)
}

//
//  Run application
//

fn run(
    repository: Repository,
    settings: &Settings,
    svg: bool,
    max_commits: Option<usize>,
    refspecs: Vec<String>,
) -> Result<(), String> {
    let now = Instant::now();
    let mut graph_builder = GraphBuilder::new()
        .with_repository(repository)
        .with_settings(settings)
        .with_refspecs(refspecs);
    if let Some(max_commits) = max_commits {
        graph_builder = graph_builder.with_max_count(max_commits);
    }
    let graph = graph_builder.build()?;

    let duration_graph = now.elapsed().as_micros();

    if settings.debug {
        for branch in &graph.all_branches {
            eprintln!(
                "{} (col {}) ({:?}) {} s: {:?}, t: {:?}",
                branch.name,
                branch.visual.column.unwrap_or(99),
                branch.range,
                if branch.is_merged { "m" } else { "" },
                branch.visual.source_order_group,
                branch.visual.target_order_group
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
            graph.commits.len()
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
