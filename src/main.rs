//! Command line tool to show clear git graphs arranged for your branching model.

// Configure clippy to look for complex functions
#![warn(clippy::cognitive_complexity)]
#![warn(clippy::too_many_lines)]

mod cli;
// TODO remove code once gleisbau API has stabilized
// Some of these features might return to the CLI tool (this application)
//mod config;
mod print;

use crate::cli::{Cli, CliCommand, ColorMode, Model};
use crate::print::svg::print_svg;
use clap::Parser;
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
use std::borrow::Borrow;
use std::path::PathBuf;
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

    let format = {
        let _ses: &mut Session = ses;
        CommitFormat::from_str(&cli.format)
    }?;

    let colored = match_color_args(ses, cli)?;

    let wrapping = crate::cli::Wrap::try_from(&cli.wrap[..]).map(|w| w.into_settings())?;

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
                set_model(
                    repository,
                    model.as_str(),
                    REPO_CONFIG_FILE,
                    &ses.models_dir,
                )?;
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
