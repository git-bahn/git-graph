//! Create graphs in SVG format (Scalable Vector Graphics).

use crate::graph::CommitInfo;
use crate::graph::GitGraph;
use crate::settings::Settings;
use svg::node::element::path::Data;
use svg::node::element::{Circle, Group, Line, Path, Text, Title};
use svg::Document;

/// Creates a SVG visual representation of a graph.
pub fn print_svg(graph: &GitGraph, settings: &Settings) -> Result<String, String> {
    let mut document = Document::new();

    let max_idx = graph.commits.len();
    let mut widest_summary = 0.0;
    let mut widest_branch_names = 0.0;

    if settings.debug {
        for branch in &graph.all_branches {
            if let (Some(start), Some(end)) = branch.range {
                document = document.add(bold_line(
                    start,
                    branch.visual.column.unwrap(),
                    end,
                    branch.visual.column.unwrap(),
                    "cyan",
                ));
            }
        }
    }

    let max_column = graph
        .commits
        .iter()
        .filter_map(|info| {
            info.branch_trace
                .and_then(|trace| graph.all_branches[trace].visual.column)
        })
        .max()
        .unwrap_or(0);

    for (idx, info) in graph.commits.iter().enumerate() {
        if let Some(trace) = info.branch_trace {
            let branch = &graph.all_branches[trace];
            let branch_color = &branch.visual.svg_color;

            for p in 0..2 {
                let parent = info.parents[p];
                let Some(par_oid) = parent else {
                    continue;
                };
                let Some(par_idx) = graph.indices.get(&par_oid) else {
                    // Parent is outside scope of graph.indices
                    // so draw a vertical line to the bottom
                    let idx_bottom = max_idx;
                    document = document.add(line(
                        idx,
                        branch.visual.column.unwrap(),
                        idx_bottom,
                        branch.visual.column.unwrap(),
                        branch_color,
                    ));
                    continue;
                };
                let par_info = &graph.commits[*par_idx];
                let par_branch = &graph.all_branches[par_info.branch_trace.unwrap()];

                let color = if info.is_merge {
                    &par_branch.visual.svg_color
                } else {
                    branch_color
                };

                if branch.visual.column == par_branch.visual.column {
                    document = document.add(line(
                        idx,
                        branch.visual.column.unwrap(),
                        *par_idx,
                        par_branch.visual.column.unwrap(),
                        color,
                    ));
                } else {
                    let split_index = super::get_deviate_index(graph, idx, *par_idx);
                    document = document.add(path(
                        idx,
                        branch.visual.column.unwrap(),
                        *par_idx,
                        par_branch.visual.column.unwrap(),
                        split_index,
                        color,
                    ));
                }
            }

            document = document.add(
                commit_dot(
                    idx,
                    branch.visual.column.unwrap(),
                    branch_color,
                    !info.is_merge,
                )
                .add(Title::new(&info.oid.to_string())),
            );

            let commit = graph
                .repository
                .find_commit(info.oid)
                .map_err(|err| err.message().to_string())?;

            let commit_str = commit.summary().unwrap_or("");

            document = document.add(draw_summary(idx, max_column, &commit_str));

            match draw_branches(idx, branch.visual.column.unwrap(), info, graph) {
                Some((branches, width)) => {
                    document = document.add(branches);

                    widest_branch_names = f32::max(widest_branch_names, width);
                }
                None => {}
            }

            widest_summary = f32::max(widest_summary, text_bounding_box(&commit_str, 12.0).0);
        }
    }

    let (x_max, y_max) = commit_coord(max_idx + 1, max_column + 1);

    document = document
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
        .set("style", "font-family:monospace;font-size:12px;");

    let mut out: Vec<u8> = vec![];
    svg::write(&mut out, &document).map_err(|err| err.to_string())?;
    Ok(String::from_utf8(out).unwrap_or_else(|_| "Invalid UTF8 character.".to_string()))
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

    let mut branch_names = info
        .branches
        .iter()
        .map(|b| graph.all_branches[*b].name.clone())
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

    if branch_names.len() > 0 {
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

            start = start - text_bounding_box(&branch_name, 12.0).0 - gap;
        }

        g = g.set("transform", format!("translate({x}, {y})"));

        Some((g.clone(), -(start + x)))
    } else {
        None
    }
}

fn draw_branch(x: f32, y: f32, branch_name: &String) -> Group {
    let width = text_bounding_box(&branch_name, 12.0).0;

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
