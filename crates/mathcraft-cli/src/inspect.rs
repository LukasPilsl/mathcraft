//! `inspect`: the worksheet in evaluation order, one line per region, with Prime's stored result.

use std::collections::HashMap;
use std::path::Path;

use mathcraft_import::describe::describe_stored;
use mathcraft_import::evaluate::load_results;
use mathcraft_import::package::Package;
use mathcraft_import::regions::{RegionKind, RegionModel, Worksheet};
use mathcraft_import::results::ResultData;

use crate::common::file_name;

/// The text of `inspect`: the worksheet in evaluation order, one line per region.
pub fn inspect_file(path: &Path) -> Result<String, String> {
    let pkg = Package::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let ws = Worksheet::from_package(&pkg).map_err(|e| format!("{}: {e}", path.display()))?;
    let table = load_results(&pkg)?;
    let stored: HashMap<u32, &ResultData> = ws.attach_stored_results(&table).into_iter().collect();
    let members = solve_members(&ws);

    let mut lines = vec![format!(
        "{}: {} regions",
        file_name(path),
        ws.report.region_count
    )];
    lines.push(format!(
        "settings: ORIGIN={} convergence_tolerance={} constraint_tolerance={} unit_system={}",
        opt(ws.settings.array_origin),
        opt(ws.settings.convergence_tolerance),
        opt(ws.settings.constraint_tolerance),
        ws.settings.unit_system.as_deref().unwrap_or("-"),
    ));
    for note in &ws.report.notes {
        lines.push(format!("note: region #{}: {}", note.region_id, note.reason));
    }
    for unsupported in &ws.report.unsupported {
        lines.push(format!(
            "unsupported: region #{}: {}",
            unsupported.region_id, unsupported.reason
        ));
    }
    lines.push("evaluation order:".to_string());
    for region in ws.evaluation_order() {
        lines.push(inspect_line(
            region,
            stored.get(&region.id).copied(),
            &members,
        ));
    }
    Ok(lines.join("\n") + "\n")
}

fn inspect_line(
    region: &RegionModel,
    stored: Option<&ResultData>,
    members: &HashMap<u32, String>,
) -> String {
    let pos = match region.pos {
        Some((left, top)) => format!("@({left:.1},{top:.1})"),
        None => "@-".to_string(),
    };
    let member = match members.get(&region.id) {
        Some(keyword) => format!(
            " [solve {keyword}, {}]",
            region.solve_category.as_deref().unwrap_or("member")
        ),
        None => String::new(),
    };
    let stored_text = |data: Option<&ResultData>| {
        data.map_or_else(|| "n/a".to_string(), |d| describe_stored(&d.value))
    };
    let body = match &region.kind {
        RegionKind::Math(expr) => format!("math    {expr}   => stored: {}", stored_text(stored)),
        RegionKind::Text(text) => format!("text    {:?}", text.as_str()),
        RegionKind::Plot(plot) => format!(
            "plot    {} traces={:?} x={:?} y={:?} => stored: {}",
            plot.kind,
            plot.traces,
            plot.x_range,
            plot.y_range,
            stored_text(stored)
        ),
        RegionKind::SolveBlock(block) => format!(
            "solve   {}",
            block.keyword.as_deref().unwrap_or("(no keyword)")
        ),
        RegionKind::Table(table) => {
            let members: Vec<String> = table
                .members
                .iter()
                .map(|m| match &m.expr {
                    Ok(expr) => format!(
                        "{expr} [ref {}]",
                        m.result_ref.map_or("-".to_string(), |r| r.to_string())
                    ),
                    Err(reason) => format!("<{reason}>"),
                })
                .collect();
            format!("table   {}", members.join("  |  "))
        }
        RegionKind::Unsupported { reason } => format!("unsupported: {reason}"),
        RegionKind::Area(area) => format!(
            "area    collapsed={} locked={} hide-border={}",
            area.collapsed, area.locked, area.hide_border
        ),
        RegionKind::Image(image) => {
            format!("image   {} bytes, item {:?}", image.bytes.len(), image.item)
        }
        RegionKind::PageBreak => "page break".to_owned(),
    };
    format!("#{:<4} {:<22} {body}{member}", region.id, pos)
}

/// Maps the id of every solve-block member to its block's keyword.
fn solve_members(ws: &Worksheet) -> HashMap<u32, String> {
    fn walk(list: &[RegionModel], out: &mut HashMap<u32, String>) {
        for region in list {
            if let RegionKind::SolveBlock(block) = &region.kind {
                let keyword = block.keyword.clone().unwrap_or_else(|| "?".to_string());
                for child in &region.children {
                    out.insert(child.id, keyword.clone());
                }
            }
            walk(&region.children, out);
        }
    }
    let mut out = HashMap::new();
    walk(&ws.regions, &mut out);
    out
}

fn opt<T: std::fmt::Debug>(value: Option<T>) -> String {
    value.map_or_else(|| "-".to_string(), |v| format!("{v:?}"))
}
