//! Branching model configurations.
//!
//! In this module you will find functions to read and write branching model
//! configurations on disk.
//!
//! The [branching models][BranchSettingsDef] themselves are defined in
//! module [settings][super::settings]

use gleisbau::settings::{BranchSettingsDef, RepoSettings};
use git2::Repository;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// Creates the directory `APP_DATA/git-graph/models` if it does not exist,
/// and writes the files for built-in branching models there.
pub fn create_config<P: AsRef<Path> + AsRef<OsStr>>(app_model_path: &P) -> Result<(), String> {
    let path: &Path = app_model_path.as_ref();
    if !path.exists() {
        std::fs::create_dir_all(app_model_path).map_err(|err| err.to_string())?;

        let models = [
            (BranchSettingsDef::git_flow(), "git-flow.toml"),
            (BranchSettingsDef::simple(), "simple.toml"),
            (BranchSettingsDef::none(), "none.toml"),
        ];
        for (model, file) in &models {
            let mut path = PathBuf::from(&app_model_path);
            path.push(file);
            let str = toml::to_string_pretty(&model).map_err(|err| err.to_string())?;
            std::fs::write(&path, str).map_err(|err| err.to_string())?;
        }
    }

    Ok(())
}

/// Get models available in `APP_DATA/git-graph/models`.
pub fn get_available_models<P: AsRef<Path>>(app_model_path: &P) -> Result<Vec<String>, String> {
    let models = std::fs::read_dir(app_model_path)
        .map_err(|err| err.to_string())?
        .filter_map(|e| match e {
            Ok(e) => {
                if let (Some(name), Some(ext)) = (e.path().file_name(), e.path().extension()) {
                    if ext == "toml" {
                        name.to_str()
                            .map(|name| (name[..(name.len() - 5)]).to_string())
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            Err(_) => None,
        })
        .collect::<Vec<_>>();

    Ok(models)
}

/// Get the currently set branching model for a repo.
pub fn get_model_name(repository: &Repository, file_name: &str) -> Result<Option<String>, String> {
    let mut config_path = PathBuf::from(repository.path());
    config_path.push(file_name);

    if config_path.exists() {
        let repo_config: RepoSettings =
            toml::from_str(&std::fs::read_to_string(config_path).map_err(|err| err.to_string())?)
                .map_err(|err| err.to_string())?;

        Ok(Some(repo_config.model))
    } else {
        Ok(None)
    }
}

/// Try to get the branch settings for a given model.
/// If no model name is given, returns the branch settings set for the repo, or the default otherwise.
pub fn get_model<P: AsRef<Path> + AsRef<OsStr>>(
    repository: &Repository,
    model: Option<&str>,
    repo_config_file: &str,
    app_model_path: &P,
) -> Result<BranchSettingsDef, String> {
    match model {
        Some(model) => read_model(model, app_model_path),
        None => {
            let mut config_path = PathBuf::from(repository.path());
            config_path.push(repo_config_file);

            if config_path.exists() {
                let repo_config: RepoSettings = toml::from_str(
                    &std::fs::read_to_string(config_path).map_err(|err| err.to_string())?,
                )
                .map_err(|err| err.to_string())?;

                read_model(&repo_config.model, app_model_path)
            } else {
                Ok(read_model("git-flow", app_model_path)
                    .unwrap_or_else(|_| BranchSettingsDef::git_flow()))
            }
        }
    }
}

/// Read a branching model file.
fn read_model<P: AsRef<Path> + AsRef<OsStr>>(
    model: &str,
    app_model_path: &P,
) -> Result<BranchSettingsDef, String> {
    let mut model_file = PathBuf::from(&app_model_path);
    model_file.push(format!("{}.toml", model));

    if model_file.exists() {
        toml::from_str::<BranchSettingsDef>(
            &std::fs::read_to_string(model_file).map_err(|err| err.to_string())?,
        )
        .map_err(|err| err.to_string())
    } else {
        let models = get_available_models(&app_model_path)?;
        let path: &Path = app_model_path.as_ref();
        Err(format!(
            "ERROR: No branching model named '{}' found in {}\n       Available models are: {}",
            model,
            path.display(),
            itertools::join(models, ", ")
        ))
    }
}
/// Permanently sets the branching model for a repository
pub fn set_model<P: AsRef<Path>>(
    repository: &Repository,
    model: &str,
    repo_config_file: &str,
    app_model_path: &P,
) -> Result<(), String> {
    let models = get_available_models(&app_model_path)?;

    if !models.contains(&model.to_string()) {
        return Err(format!(
            "ERROR: No branching model named '{}' found in {}\n       Available models are: {}",
            model,
            app_model_path.as_ref().display(),
            itertools::join(models, ", ")
        ));
    }

    let mut config_path = PathBuf::from(repository.path());
    config_path.push(repo_config_file);

    let config = RepoSettings {
        model: model.to_string(),
    };

    let str = toml::to_string_pretty(&config).map_err(|err| err.to_string())?;
    std::fs::write(&config_path, str).map_err(|err| err.to_string())?;

    Ok(())
}
