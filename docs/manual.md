# git-graph manual

**Content**

* [Overview](#overview)
* [Options](#options)
* [Formatting](#formatting)
* [Custom branching models](#custom-branching-models)

## Overview

The most basic usage is to simply call git-graph from inside a Git repository:

```
git-graph
```

This works also deeper down the directory tree, so no need to be in the repository's root folder.

Alternatively, the path to the repository to visualize can be specified with option `--path`:

```
git-graph --path "path/to/repo"
```

**Branching models**

The above call assumes the GitFlow branching model (the default). Different branching models can be used with the option `--model` or `-m`:

```
git-graph --model simple
```

To *permanently* set the branching model for a repository, use subcommand `model`, like

```
git-graph model simple
```

Use the subcommand without argument to view the currently set branching model of a repository:

```
git-graph model
```

To view all available branching models, use option `--list` or `-l` of the subcommand:

```
git-graph model --list
```

For **defining your own models**, see section [Custom branching models](#custom-branching-models).

**Styles**

Git-graph supports different styles. Besides the default `normal` (alias `thin`), supported styles are `round`, `bold`, `double` and `ascii`. Use a style with option `--style` or `-s`:

```
git-graph --style round
```

![styles](https://user-images.githubusercontent.com/44003176/103467621-357ce780-4d51-11eb-8ff9-dd7be8b40f84.png)

Style `ascii` can be used for devices and media that do not support Unicode/UTF-8 characters. 

**Formatting**

Git-graph supports predefined as well as custom commit formatting through option `--format`. Available presets follow Git: `oneline` (the default), `short`, `medium` and `full`. For details and custom formatting, see section [Formatting](#formatting).

For a complete list of all available options, see the next section [Options](#options).

## Options

All options are explained in the CLI help. View it with `git-graph -h`:

```
Structured Git graphs for your branching model.
    https://github.com/mlange-42/git-graph

EXAMPES:
    git-graph                   -> Show graph
    git-graph --style round     -> Show graph in a different style
    git-graph --model <model>   -> Show graph using a certain <model>
    git-graph model --list      -> List available branching models
    git-graph model             -> Show repo's current branching models
    git-graph model <model>     -> Permanently set model <model> for this repo

USAGE:
    git-graph [FLAGS] [OPTIONS] [SUBCOMMAND]

FLAGS:
    -d, --debug       Additional debug output and graphics.
    -h, --help        Prints help information
    -l, --local       Show only local branches, no remotes.
        --no-color    Print without colors. Missing color support should be detected
                      automatically (e.g. when piping to a file).
                      Overrides option '--color'
        --no-pager    Use no pager (print everything at once without prompt).
    -r, --rotate      Rotate the graph 180 degrees.
    -S, --sparse      Print a less compact graph: merge lines point to target lines
                      rather than merge commits.
        --svg         Render graph as SVG instead of text-based.
    -V, --version     Prints version information

OPTIONS:
        --color <color>      Specify when colors should be used. One of [auto|always|never].
                             Default: auto.
    -f, --format <format>    Commit format. One of [oneline|short|medium|full|"<string>"].
                               (First character can be used as abbreviation, e.g. '-f m')
                             Default: oneline.
                             For placeholders supported in "<string>", consult 'git-graph --help'
    -n, --max-count <n>      Maximum number of commits
    -m, --model <model>      Branching model. Available presets are [simple|git-flow|none].
                             Default: git-flow.
                             Permanently set the model for a repository with
                             > git-graph model <model>
    -p, --path <path>        Open repository from this path or above. Default '.'
    -s, --style <style>      Output style. One of [normal/thin|round|bold|double|ascii].
                               (First character can be used as abbreviation, e.g. '-s r')
    -w, --wrap <wrap>        Line wrapping for formatted commit text. Default: 'auto 0 8'
                             Argument format: [<width>|auto|none[ <indent1>[ <indent2>]]]
                             For examples, consult 'git-graph --help'

SUBCOMMANDS:
    help     Prints this message or the help of the given subcommand(s)
    model    Prints or permanently sets the branching model for a repository.
```

For longer explanations, use `git-graph --help`.

## Formatting

Formatting can be specified with the `--format` option.

Predefined formats are `oneline` (the default), `short`, `medium` and `full`. They should behave like the Git formatting presets described in the [Git documentation](https://git-scm.com/docs/pretty-formats).

**oneline**

```
<hash> [<refs>] <title line>
```

**short**

```
commit <hash> [<refs>]
Author: <author>

<title line>
```

**medium**

```
commit <hash> [<refs>]
Author: <author>
Date:   <author date>

<title line>

<full commit message>
```

**full**

```
commit <hash> [<refs>]
Author: <author>
Commit: <committer>
Date:   <author date>

<title line>

<full commit message>
```

### Custom formatting

Formatting strings use a subset of the placeholders available in `git log --format="..."`:

| Placeholder | Replaced with                               |
| ----------- | ------------------------------------------- |
| %n          | newline                                     |
| %H          | commit hash                                 |
| %h          | abbreviated commit hash                     |
| %P          | parent commit hashes                        |
| %p          | abbreviated parent commit hashes            |
| %d          | refs (branches, tags)                       |
| %s          | commit summary                              |
| %b          | commit message body                         |
| %B          | raw body (subject and body)                 |
| %an         | author name                                 |
| %ae         | author email                                |
| %ad         | author date                                 |
| %as         | author date in short format `YYYY-MM-DD`    |
| %ar         | author date, relative format (e.g., "21 hours ago") |
| %cn         | committer name                              |
| %ce         | committer email                             |
| %cd         | committer date                              |
| %cs         | committer date in short format `YYYY-MM-DD` |
| %cr         | committer date, relative format (e.g., "4 days ago") |

If you add a '+' (plus sign) after % of a placeholder, a line-feed is inserted immediately before the expansion if and only if the placeholder expands to a non-empty string.

If you add a '-' (minus sign) after % of a placeholder, all consecutive line-feeds immediately preceding the expansion are deleted if and only if the placeholder expands to an empty string.

If you add a ' ' (space) after % of a placeholder, a space is inserted immediately before the expansion if and only if the placeholder expands to a non-empty string.

See also the [Git documentation](https://git-scm.com/docs/pretty-formats).

More formatting placeholders are planned for later releases.

**Examples**

Format recreating `oneline`:

```
git-graph --format "%h%d %s"
```

Format similar to `short`:

```
git-graph --format "commit %H%nAuthor: %an %ae%n%n    %s%n"
```

## Custom branching models

Branching models are configured using the files in `APP_DATA/git-graph/models`. 

* Windows: `C:\Users\<user>\AppData\Roaming\git-graph`
* Linux: `~/.config/git-graph`
* OSX: `~/Library/Application Support/git-graph`

File names of any `.toml` files in the `models` directory can be used in parameter `--model`, or via sub-command `model`. E.g., to use a branching model defined in `my-model.toml`, use:

```
git-graph --model my-model
```

**Branching model files** are in [TOML](https://toml.io/en/) format and have several sections, relying on Regular Expressions to categorize branches. The listing below shows the `git-flow` model (slightly abbreviated) with explanatory comments.

```toml
# RegEx patterns for branch groups by persistence, from most persistent
# to most short-leved branches. This is used to back-trace branches.
# Branches not matching any pattern are assumed least persistent.
persistence = [
    '^(master|main|trunk)$', # Matches exactly `master` or `main`  or `trunk`
    '^(develop|dev)$',
    '^feature.*$',     # Matches everything starting with `feature`
    '^release.*$',
    '^hotfix.*$',
    '^bugfix.*$',
]

# RegEx patterns for visual ordering of branches, from left to right.
# Here, `master`, `main` or `trunk` are shown left-most, followed by branches
# starting with `hotfix` or `release`, followed by `develop` or `dev`.
# Branches not matching any pattern (e.g. starting with `feature`)
# are displayed further to the right.
order = [
    '^(master|main|trunk)$',      # Matches exactly `master` or `main` or `trunk`
    '^(hotfix|release).*$', # Matches everything starting with `hotfix` or `release`
    '^(develop|dev)$',      # Matches exactly `develop` or `dev`
]

# Colors of branches in terminal output. 
# For supported colors, see section Colors (below this listing).
[terminal_colors]
# Each entry is composed of a RegEx pattern and a list of colors that
# will be used alternating (see e.g. `feature...`).
matches = [
    [
        '^(master|main|trunk)$',
        ['bright_blue'],
    ],
    [
        '^(develop|dev)$',
        ['bright_yellow'],
    ],
    [   # Branches obviously merged in from forks are prefixed with 'fork/'. 
        # The 'fork/' prefix is only available in order and colors, but not in persistence!
        '^(feature|fork/).*$',
        ['bright_magenta', 'bright_cyan'], # Multiple colors for alternating use
    ],
        [
        '^release.*$',
        ['bright_green'],
    ],
        [
        '^(bugfix|hotfix).*$',
        ['bright_red'],
    ],
    [
        '^tags/.*$',
        ['bright_green'],
    ],
]
# A list of colors that are used (alternating) for all branches
# not matching any of the above pattern. 
unknown = ['white']

# Colors of branches in SVG output. 
# Same structure as terminal_colors. 
# For supported colors, see section Colors (below this listing).
[svg_colors]
matches = [
    [
        '^(master|main|trunk)$',
        ['blue'],
    ],
    [ 
        '...',
    ]
]
unknown = ['gray']
```

**Tags**

Internally, all tags start with `tag/`. To match Git tags, use RegEx patterns like `^tags/.*$`. However, only tags that are not on any branch are ordered and colored separately.

**Colors**

**Terminal colors** support the 8 system color names `black`, `red`, `green`, `yellow`, `blue`, `magenta`, `cyan` and `white`, as well as each of them prefixed with `bright_` (e.g. `bright_blue`).

Further, indices of the 256-color palette are supported. For a full list, see [here](https://jonasjacek.github.io/colors/). Indices must be quoted as strings (e.g. `'16'`)

**SVG colors** support all named web colors (full list [here](https://htmlcolorcodes.com/color-names/)), as well as RGB colors in hex notation, like `#ffffff`.
