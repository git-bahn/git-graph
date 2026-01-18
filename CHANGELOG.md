# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).


## [unreleased] - Next Date

### Added

- A CHANGELOG.md file.
- Details to the SVG output.
- Option to specify revset to render a subset of the graph.
- Gitea merge pattern.

### Changed

- Add clippy warning for complex functions.
- Code refactoring to make it easier to understand.
- Extract library part into crate gleisbau

### Removed

- Remove pager


## [0.7.0] - 2025-11-14

Last release where library is part of git-graph.

### Added

- (BREAKING) graph::get_repo, add argument skip_repo_owner_validation
  false gives the previous behaviour.
- (BREAKING) GitGraph::new, add argument start_point to control where
  traversal should start.
  Set to None to get the previous behaviour.
  
- Lots of API docs
- "trunk" as supported main branch name

### Changed

- Update git2 dependency to version 0.20

### Removed

- (BREAKING) GitGraph public fields "tags" and "branches"


## [0.6.0] - 2024-05-24

### Added

- Reverse order option
