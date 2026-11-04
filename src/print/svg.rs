//! Create graphs in SVG format (Scalable Vector Graphics).

use itertools::enumerate;

use gleisbau::graph::CommitInfo;
use gleisbau::graph::GitGraph;
use gleisbau::layout::get_deviate_index;
use gleisbau::settings::Settings;
use svg::node::element::path::Data;
use svg::node::element::{Circle, Group, Line, Path, Text, Title};
use svg::Document;

/// Creates a SVG visual representation of a graph.
pub fn print_svg(graph: &GitGraph, settings: &Settings) -> Result<String, String> {
    let tracks = &graph.tracks;
    let layout = &graph.layout;
    let mut document = Document::new();

    let max_idx = tracks.commits.len();
    let rotate = settings.reverse_commit_order;
    let row = |idx: usize| {
        if rotate {
            max_idx.saturating_sub(idx + 1)
        } else {
            idx
        }
    };
    // Mirrors the rows of commit dots and lines the same way `row` does for the text
    let graph_transform = rotate.then(|| {
        let (_, y) = commit_coord(max_idx, 0);
        format!("matrix(1 0 0 -1 0 {y})")
    });
    let mut widest_summary = 0.0;
    let mut widest_branch_names = 0.0;

    if settings.debug {
        for (branch_inx, branch) in enumerate(&tracks.all_branches) {
            if let (Some(start), Some(end)) = branch.range {
                let branch_visual = layout.track_visual(branch_inx.into()).unwrap();
                document = document.add(bold_line(
                    row(start),
                    branch_visual.column.unwrap(),
                    row(end),
                    branch_visual.column.unwrap(),
                    "cyan",
                ));
            }
        }
    }

    let max_column = find_max_column(graph);

    for (idx, info) in tracks.commits.iter().enumerate() {
        let mut commit_group = draw_commit(info, graph, idx);
        if let Some(transform) = &graph_transform {
            commit_group = commit_group.set("transform", transform.as_str());
        }
        document = document.add(commit_group);

        let commit = graph.repository.find_commit(info.oid).unwrap();
        let commit_summary = commit
            .summary()
            .map_err(|err| err.message().to_string())?
            .unwrap_or("");

        document = document.add(draw_summary(row(idx), max_column, commit_summary));

        if let Some(trace) = info.branch_trace {
            let branch_visual = layout
                .track_visual(trace)
                .expect("Branch should have a layout");

            if let Some((branches, width)) =
                draw_branches(row(idx), branch_visual.column.unwrap(), info, graph)
            {
                document = document.add(branches);

                widest_branch_names = f32::max(widest_branch_names, width);
            }
        }
        widest_summary = f32::max(widest_summary, text_bounding_box(commit_summary, 12.0).0);
    }

    document = set_document_size(
        document.clone(),
        widest_branch_names,
        widest_summary,
        max_idx,
        max_column,
    );

    let mut out: Vec<u8> = vec![];
    svg::write(&mut out, &document).map_err(|err| err.to_string())?;
    Ok(String::from_utf8(out).unwrap_or_else(|_| "Invalid UTF8 character.".to_string()))
}

fn set_document_size(
    document: Document,
    widest_branch_names: f32,
    widest_summary: f32,
    max_idx: usize,
    max_column: usize,
) -> Document {
    let (x_max, y_max) = commit_coord(max_idx + 1, max_column + 1);

    document
        .set(
            "viewBox",
            (
                -widest_branch_names,
                0,
                x_max + widest_branch_names + widest_summary,
                y_max,
            ),
        )
        .set("width", x_max + widest_branch_names + widest_summary + 15.0)
        .set("height", y_max)
        .set("style", "font-family:monospace;font-size:12px;")
}

fn find_max_column(graph: &GitGraph) -> usize {
    let tracks = &graph.tracks;
    let layout = &graph.layout;
    tracks
        .commits
        .iter()
        .filter_map(|info| {
            info.branch_trace
                .and_then(|trace| layout.track_visual(trace))
                .and_then(|visual| visual.column)
        })
        .max()
        .unwrap_or(0)
}

// index is graph.commits[index]
fn draw_commit(info: &CommitInfo, graph: &GitGraph, index: usize) -> Group {
    let tracks = &graph.tracks;
    let layout = &graph.layout;
    let mut group = Group::new();

    if let Some(trace) = info.branch_trace {
        let branch_visual = graph.layout.track_visual(trace).unwrap();
        let branch_color = &branch_visual.svg_color;

        for par_oid in &info.parents {
            let Some(par_idx) = tracks.indices.get(par_oid) else {
                // Parent is outside scope of tracks.indices
                // so draw a vertical line to the bottom
                let idx_bottom = tracks.commits.len();
                group = group.add(line(
                    index,
                    branch_visual.column.unwrap(),
                    idx_bottom,
                    branch_visual.column.unwrap(),
                    branch_color,
                ));
                continue;
            };
            let par_info = &tracks.commits[*par_idx];
            let par_branch_idx = par_info.branch_trace.unwrap();
            let par_branch_visual = layout.track_visual(par_branch_idx).unwrap();

            group = group.add(path(
                index,
                branch_visual.column.unwrap(),
                *par_idx,
                par_branch_visual.column.unwrap(),
                if branch_visual.column == par_branch_visual.column {
                    index
                } else {
                    get_deviate_index(tracks, layout, index, *par_idx)
                },
                if info.is_merge() {
                    &par_branch_visual.svg_color
                } else {
                    branch_color
                },
            ));
        }

        group = group.add(
            commit_dot(
                index,
                branch_visual.column.unwrap(),
                branch_color,
                !info.is_merge(),
            )
            .add(Title::new(info.oid.to_string())),
        );
    }
    group
}

fn commit_dot(index: usize, column: usize, color: &str, filled: bool) -> Circle {
    let (x, y) = commit_coord(index, column);
    Circle::new()
        .set("cx", x)
        .set("cy", y)
        .set("r", 4)
        .set("fill", if filled { color } else { "white" })
        .set("stroke", color)
        .set("stroke-width", 1)
}

fn draw_branches(
    index: usize,
    column: usize,
    info: &CommitInfo,
    graph: &GitGraph,
) -> Option<(Group, f32)> {
    let (x, y) = commit_coord(index, column);

    let mut branch_names = graph
        .labels
        .get_labels(&info.oid)
        .unwrap_or(&vec![])
        .iter()
        .map(|label| label.name.clone())
        .collect::<Vec<String>>();

    if graph.head.oid == info.oid {
        // Head is here
        match branch_names
            .iter()
            .position(|name| name == &graph.head.name)
        {
            Some(index) => {
                branch_names.insert(index + 1, "HEAD".to_string());
            }
            //Detached HEAD
            None => branch_names.push("HEAD".to_string()),
        }
    }

    if !branch_names.is_empty() {
        let mut g = Group::new();
        let mut start: f32 = 5.0;

        for branch_name in &branch_names {
            let gap = 9.0
                + if branch_name == "HEAD" && graph.head.is_branch {
                    0.0
                } else {
                    8.0
                };
            g = g.add(draw_branch(start - gap, 2.5, branch_name));

            start = start - text_bounding_box(branch_name, 12.0).0 - gap;
        }

        g = g.set("transform", format!("translate({x}, {y})"));

        Some((g.clone(), -(start + x)))
    } else {
        None
    }
}

fn draw_branch(x: f32, y: f32, branch_name: &String) -> Group {
    let width = text_bounding_box(branch_name, 12.0).0;

    Group::new()
        .add(Text::new(branch_name).set("x", x - width).set("y", y + 1.0))
        .add(
            Path::new()
                .set(
                    "d",
                    Data::new()
                        //Tip
                        .move_to((x + 2.0, y + 4.0))
                        .line_by((6.0, -7.0))
                        .line_by((-6.0, -7.0))
                        //Body
                        .horizontal_line_by(-width - 11.0)
                        //Rear
                        .line_by((6.0, 7.0))
                        .line_by((-6.0, 7.0))
                        .close(),
                )
                .set("stroke", "#00000000")
                .set("fill", "#00000030"),
        )
}

fn draw_summary(index: usize, max_column: usize, hash: &str) -> Text {
    let (x, y) = commit_coord(index, max_column);
    Text::new(hash)
        .set("x", x + 15.0)
        .set("y", y + 2.0)
        .set("style", "font-family:monospace;font-size:12px")
}

fn text_bounding_box(text: &str, size: f32) -> (f32, f32) {
    // Let's assume the font has a 60% width
    (text.len() as f32 * size * 0.6, size)
}

fn line(index1: usize, column1: usize, index2: usize, column2: usize, color: &str) -> Line {
    let (x1, y1) = commit_coord(index1, column1);
    let (x2, y2) = commit_coord(index2, column2);
    Line::new()
        .set("x1", x1)
        .set("y1", y1)
        .set("x2", x2)
        .set("y2", y2)
        .set("stroke", color)
        .set("stroke-width", 1)
}

fn bold_line(index1: usize, column1: usize, index2: usize, column2: usize, color: &str) -> Line {
    let (x1, y1) = commit_coord(index1, column1);
    let (x2, y2) = commit_coord(index2, column2);
    Line::new()
        .set("x1", x1)
        .set("y1", y1)
        .set("x2", x2)
        .set("y2", y2)
        .set("stroke", color)
        .set("stroke-width", 5)
}

fn path(
    index1: usize,
    column1: usize,
    index2: usize,
    column2: usize,
    split_idx: usize,
    color: &str,
) -> Path {
    let c0 = commit_coord(index1, column1);

    let c1 = commit_coord(split_idx, column1);
    let c2 = commit_coord(split_idx + 1, column2);

    let c3 = commit_coord(index2, column2);

    let m = (0.5 * (c1.0 + c2.0), 0.5 * (c1.1 + c2.1));

    let data = if column2 > column1 {
        Data::new()
            .move_to(c0)
            .line_to(c1)
            .line_to((c2.0, m.1))
            .line_to(c3)
    } else {
        Data::new()
            .move_to(c0)
            .line_to((c1.0, m.1))
            .line_to(c2)
            .line_to(c3)
    };

    Path::new()
        .set("d", data)
        .set("fill", "none")
        .set("stroke", color)
        .set("stroke-width", 1)
}

fn commit_coord(index: usize, column: usize) -> (f32, f32) {
    (15.0 * (column as f32 + 1.0), 15.0 * (index as f32 + 1.0))
}

#[cfg(test)]
mod tests {
    use super::print_svg;
    use git2::{Repository, Signature};
    use gleisbau::graph::Builder;
    use gleisbau::print::format::CommitFormat;
    use gleisbau::settings::{
        BranchOrder, BranchSettings, BranchSettingsDef, Characters, MergePatterns, Settings,
    };
    use std::rc::Rc;

    /// Renders a repository with the commits "first" and then "second".
    fn render(rotate: bool) -> String {
        let dir = std::env::temp_dir().join(format!(
            "git-graph-svg-test-{}-{rotate}",
            std::process::id()
        ));
        let repo = Repository::init(&dir).unwrap();
        let signature = Signature::now("Test", "test@example.com").unwrap();
        let tree_id = repo.treebuilder(None).unwrap().write().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        let first = repo
            .commit(Some("HEAD"), &signature, &signature, "first", &tree, &[])
            .unwrap();
        let first = repo.find_commit(first).unwrap();
        repo.commit(
            Some("HEAD"),
            &signature,
            &signature,
            "second",
            &tree,
            &[&first],
        )
        .unwrap();
        drop(first);
        drop(tree);

        let settings = Rc::new(Settings {
            reverse_commit_order: rotate,
            debug: false,
            compact: true,
            colored: false,
            include_remote: false,
            format: CommitFormat::OneLine,
            wrapping: None,
            characters: Characters::thin(),
            branch_order: BranchOrder::ShortestFirst(true),
            branches: BranchSettings::from(BranchSettingsDef::simple()).unwrap(),
            merge_patterns: MergePatterns::default(),
        });
        let graph = Builder::new()
            .with_repository(repo)
            .with_settings(settings.clone())
            .build()
            .unwrap();
        let svg = print_svg(&graph, &settings).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        svg
    }

    /// Returns the value of the attribute on the first element containing `marker`.
    fn attr_near(svg: &str, marker: &str, attr: &str) -> f32 {
        let end = svg.find(marker).unwrap();
        let start = svg[..end].rfind('<').unwrap();
        let element = &svg[start..end];
        let value = element.split(&format!("{attr}=\"")).nth(1).unwrap();
        value.split('"').next().unwrap().parse().unwrap()
    }

    #[test]
    fn svg_keeps_newest_commit_on_top_by_default() {
        let svg = render(false);
        assert!(attr_near(&svg, ">\nsecond", "y") < attr_near(&svg, ">\nfirst", "y"));
        assert!(!svg.contains("matrix(1 0 0 -1"));
    }

    #[test]
    fn svg_rotate_puts_oldest_commit_on_top() {
        let svg = render(true);
        assert!(attr_near(&svg, ">\nfirst", "y") < attr_near(&svg, ">\nsecond", "y"));
        // Commit dots and lines are mirrored around the same rows as the text.
        assert!(svg.contains("transform=\"matrix(1 0 0 -1 0 45)\""));
    }
}
