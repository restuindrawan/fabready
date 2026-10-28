//! Manual edits from the app's properties card: change one track, via or footprint, or delete a track or
//! via. They are planned like automatic fixes (byte-range edits of the `.kicad_pcb`), so they share preview,
//! backup, undo and the "file changed since" guard. Edit ids:
//!
//! - `edit:track:<id>:width=0.3`, `edit:track:<id>:layer=B.Cu`
//! - `edit:via:<id>:size=0.6`, `edit:via:<id>:drill=0.3`, `edit:via:<id>:at=120.5,80`
//! - `edit:fp:<id>:reference=SW1`, `edit:fp:<id>:value=10k`, `edit:fp:<id>:at=120,80,90`
//! - relative, for dragging and nudging: `edit:fp:<id>:move=1.5,-2`, `edit:fp:<id>:rotate=90`, `edit:via:<id>:move=dx,dy`
//! - `delete:track:<id>`, `delete:via:<id>`
//! - new items: `add:track:<layer>:<width>:<x,y;x,y;...>:<net>`, `add:via:<x,y>:<size>:<drill>:<net>`
//!
//! Moving a footprint or via drags the ends of tracks that meet its pads (same net) along, like KiCad's Drag.
//!
//! `<id>` is the item's uuid, or `#<n>` for files without one (see `board::item_id`).

use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};

use crate::board::{arc_points, item_id, layer_of, net_name, net_table, point_in_polygon, point_segment_distance, rotate, segment_distance, Board, Pad, Point};
use crate::fixes::{atom_edit, mm, uuid_for, Change, Edit, Highlight, Plan};
use crate::sexpr::{self, Sexp};

pub fn is_edit(id: &str) -> bool {
    id.starts_with("edit:") || id.starts_with("delete:") || id.starts_with("add:")
}

/// Copper layers this board defines, front first.
pub fn copper_layers(board: &Board) -> Vec<String> {
    let mut out: Vec<String> = board.layers.iter().filter(|l| l.name.ends_with(".Cu")).map(|l| l.name.clone()).collect();
    for must in ["F.Cu", "B.Cu"] {
        if !out.iter().any(|l| l == must) {
            out.push(must.into());
        }
    }
    let rank = |l: &String| match l.as_str() {
        "F.Cu" => 0,
        "B.Cu" => 1000,
        other => other.trim_start_matches("In").trim_end_matches(".Cu").parse::<i32>().unwrap_or(500),
    };
    out.sort_by_key(rank);
    out
}

fn find<'a>(root: &'a Sexp, kind: &str, id: &str) -> Result<&'a Sexp> {
    let hit = match kind {
        "track" => {
            let segments: Vec<&Sexp> = root.children("segment").collect();
            let n = segments.len();
            segments
                .into_iter()
                .enumerate()
                .chain(root.children("arc").enumerate().map(|(i, a)| (n + i, a)))
                .find(|(i, node)| item_id(node, *i) == id)
                .map(|(_, node)| node)
        }
        "via" => root.children("via").enumerate().find(|(i, node)| item_id(node, *i) == id).map(|(_, node)| node),
        "fp" => root.children("footprint").chain(root.children("module")).enumerate().find(|(i, node)| item_id(node, *i) == id).map(|(_, node)| node),
        other => bail!("unknown item kind '{other}'"),
    };
    hit.ok_or_else(|| anyhow!("that {} isn't on the board any more", if kind == "fp" { "footprint" } else { kind }))
}

fn number(v: &str, what: &str) -> Result<f64> {
    v.trim().replace(',', ".").parse::<f64>().ok().filter(|n| n.is_finite()).ok_or_else(|| anyhow!("{what}: '{v}' isn't a number"))
}

fn track_points(node: &Sexp) -> Vec<Point> {
    if node.head() == Some("arc") {
        arc_points(node.child_xy("start"), node.child_xy("mid"), node.child_xy("end"), 8)
    } else {
        vec![node.child_xy("start"), node.child_xy("end")]
    }
}

fn connection_points(node: &Sexp) -> Vec<Point> {
    if node.head() == Some("arc") {
        arc_points(node.child_xy("start"), node.child_xy("mid"), node.child_xy("end"), 64)
    } else {
        track_points(node)
    }
}

/// Replace a whole child list such as `(at 1 2 90)`.
fn replace_child(node: &Sexp, key: &str, text: String) -> Result<Edit> {
    let c = node.child(key).ok_or_else(|| anyhow!("item has no ({key} ...)"))?;
    Ok(Edit { start: c.span().start, end: c.span().end, text })
}

/// `(at x y angle ...)` with a new angle, keeping its position and any trailing flags (e.g. `unlocked`).
fn at_with_angle(at: &Sexp, angle: f64) -> String {
    let rest: Vec<&str> = at.items().iter().skip(3).filter_map(Sexp::atom).filter(|a| a.parse::<f64>().is_err()).collect();
    let angle = angle.rem_euclid(360.0);
    let mut s = format!("(at {} {}", at.arg(1).unwrap_or("0"), at.arg(2).unwrap_or("0"));
    if angle.abs() > 1e-9 {
        s.push(' ');
        s.push_str(&mm(angle));
    }
    for r in rest {
        s.push(' ');
        s.push_str(r);
    }
    s.push(')');
    s
}

/// Remove a node, and its line too when it sits on a line of its own.
pub(crate) fn delete_node(text: &str, node: &Sexp) -> Edit {
    let (mut start, mut end) = (node.span().start, node.span().end);
    let line_start = text[..start].rfind('\n').map_or(0, |i| i + 1);
    if text[line_start..start].trim().is_empty() && text[end..].starts_with('\n') {
        start = line_start;
        end += 1;
    }
    Edit { start, end, text: String::new() }
}

/// The text atom of a footprint's Reference/Value (KiCad 8 `property`, older `fp_text`).
fn field_atom<'a>(fp: &'a Sexp, field: &str) -> Option<&'a Sexp> {
    let lower = field.to_lowercase();
    fp.children("property")
        .find(|p| p.arg(1) == Some(field))
        .or_else(|| fp.children("fp_text").find(|t| t.arg(1) == Some(lower.as_str())))
        .and_then(|n| n.items().get(2))
}

pub fn plan(text: &str, root: &Sexp, board: &Board, id: &str) -> Result<Plan> {
    let nets = net_table(root);
    if id.starts_with("add:") {
        return plan_add(text, root, board, id);
    }

    let mut parts = id.splitn(4, ':');
    let (op, kind, item) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""), parts.next().unwrap_or(""));
    let node = find(root, kind, item)?;
    let change = |title: String, detail: String| Change { id: id.to_string(), title, detail };

    if op == "delete" {
        let net = net_name(node, &nets);
        let (title, highlight) = match kind {
            "track" => (format!("Delete track on {}", if net.is_empty() { "no net" } else { &net }), Highlight::Line { points: track_points(node), width: node.value_f64("width").unwrap_or(0.25) }),
            "via" => {
                let (x, y) = node.child_xy("at");
                (format!("Delete via on {}", if net.is_empty() { "no net" } else { &net }), Highlight::Circle { x, y, r: node.value_f64("size").unwrap_or(0.6) / 2.0 })
            }
            _ => bail!("only tracks and vias can be deleted here; remove footprints in the schematic"),
        };
        return Ok(Plan { change: change(title, layer_of(node)), edits: vec![delete_node(text, node)], highlights: vec![highlight] });
    }
    if op != "edit" {
        bail!("unknown edit '{id}'");
    }
    let (prop, value) = parts.next().and_then(|p| p.split_once('=')).ok_or_else(|| anyhow!("bad edit id {id}"))?;

    match (kind, prop) {
        ("track", "width") => {
            let w = number(value, "Width")?;
            if !(0.05..=10.0).contains(&w) {
                bail!("Width must be between 0.05 and 10 mm");
            }
            let old = node.value_f64("width").unwrap_or(0.25);
            let edit = atom_edit(node, "width", mm(w)).ok_or_else(|| anyhow!("track has no width"))?;
            Ok(Plan {
                change: change(format!("Track width {} → {} mm", mm(old), mm(w)), format!("{} on {}", net_name(node, &nets), layer_of(node))),
                edits: vec![edit],
                highlights: vec![Highlight::Line { points: track_points(node), width: w }],
            })
        }
        ("track", "layer") => {
            if !copper_layers(board).iter().any(|l| l == value) {
                bail!("{value} isn't a copper layer on this board");
            }
            let edit = atom_edit(node, "layer", sexpr::quote(value)).ok_or_else(|| anyhow!("track has no layer"))?;
            Ok(Plan {
                change: change(format!("Move track to {value}"), format!("{} was on {}. Add vias where it meets the other layer.", net_name(node, &nets), layer_of(node))),
                edits: vec![edit],
                highlights: vec![Highlight::Line { points: track_points(node), width: node.value_f64("width").unwrap_or(0.25) }],
            })
        }
        ("via", "size" | "drill") => {
            let v = number(value, if prop == "size" { "Diameter" } else { "Drill" })?;
            let (size, drill) = (node.value_f64("size").unwrap_or(0.6), node.value_f64("drill").unwrap_or(0.3));
            let (new_size, new_drill) = if prop == "size" { (v, drill) } else { (size, v) };
            if v <= 0.0 || v > 10.0 || new_size > 10.0 || new_drill > 10.0 {
                bail!("Via sizes must be positive and under 10 mm");
            }
            let edit = atom_edit(node, prop, mm(v)).ok_or_else(|| anyhow!("via has no {prop}"))?;
            let (x, y) = node.child_xy("at");
            Ok(Plan {
                change: change(
                    format!("Via {} → {} mm", if prop == "size" { "diameter" } else { "drill" }, mm(v)),
                    format!("{}: {}/{} mm → {}/{} mm (diameter/drill)", net_name(node, &nets), mm(size), mm(drill), mm(new_size), mm(new_drill)),
                ),
                edits: vec![edit],
                highlights: vec![Highlight::Circle { x, y, r: new_size / 2.0 }],
            })
        }
        ("via", "at" | "move") => {
            let (a, b) = value.split_once(',').ok_or_else(|| anyhow!("position needs x,y"))?;
            let (a, b) = (number(a, "X")?, number(b, "Y")?);
            let old = node.child_xy("at");
            let (x, y) = if prop == "move" { (old.0 + a, old.1 + b) } else { (a, b) };
            let mut edits = vec![replace_child(node, "at", format!("(at {} {})", mm(x), mm(y)))?];
            let via = board.vias.iter().find(|v| v.id == item).ok_or_else(|| anyhow!("that via isn't on the board any more"))?;
            let follow = follow_via_edits(root, &nets, board, via, (x, y))?;
            let n = follow.len();
            edits.extend(follow);
            Ok(Plan {
                change: change("Move via".into(), format!("({}, {}) → ({}, {}) mm{}", mm(old.0), mm(old.1), mm(x), mm(y), followed(n))),
                edits,
                highlights: vec![Highlight::Circle { x, y, r: node.value_f64("size").unwrap_or(0.6) / 2.0 }],
            })
        }
        ("fp", "reference" | "value") => {
            let field = if prop == "reference" { "Reference" } else { "Value" };
            let v = value.trim();
            if v.is_empty() || v.contains('\n') {
                bail!("{field} can't be empty");
            }
            let atom = field_atom(node, field).ok_or_else(|| anyhow!("footprint has no {field}"))?;
            let old = atom.atom().unwrap_or_default().to_string();
            if field == "Reference" && board.footprints.iter().any(|f| f.reference == v && f.reference != old) {
                bail!("{v} is already used by another footprint");
            }
            let fp = board.footprints.iter().find(|f| f.id == item);
            Ok(Plan {
                change: change(
                    format!("{field} {old} → {v}"),
                    "Change it in the schematic too, or Update PCB from Schematic will put it back.".into(),
                ),
                edits: vec![Edit { start: atom.span().start, end: atom.span().end, text: sexpr::quote(v) }],
                highlights: fp.map(|f| fp_highlight(f.bbox())).into_iter().collect(),
            })
        }
        ("fp", "at" | "move" | "rotate") => {
            let at = node.child("at").ok_or_else(|| anyhow!("footprint has no position"))?;
            let old = (at.num(1).unwrap_or(0.0), at.num(2).unwrap_or(0.0), at.num(3).unwrap_or(0.0));
            let (x, y, angle) = match prop {
                "rotate" => (old.0, old.1, old.2 + number(value, "Rotation")?),
                _ => {
                    let nums: Vec<&str> = value.split(',').collect();
                    if nums.len() < 2 {
                        bail!("position needs x,y[,angle]");
                    }
                    let (a, b) = (number(nums[0], "X")?, number(nums[1], "Y")?);
                    let angle = match nums.get(2) {
                        Some(r) if !r.trim().is_empty() => number(r, "Rotation")?,
                        _ => old.2,
                    };
                    if prop == "move" { (old.0 + a, old.1 + b, old.2) } else { (a, b, angle) }
                }
            };
            plan_fp_move(root, board, &nets, node, item, old, (x, y, angle), change)
        }
        _ => bail!("can't change {prop} of a {kind}"),
    }
}

pub(crate) fn validate_batch(root: &Sexp, ids: &[String]) -> Result<()> {
    for id in ids {
        let Some(rest) = id.strip_prefix("edit:via:") else { continue };
        let Some((item, property)) = rest.split_once(':') else { continue };
        if !property.starts_with("size=") && !property.starts_with("drill=") {
            continue;
        }
        let node = find(root, "via", item)?;
        let size = node.value_f64("size").unwrap_or(0.6);
        let drill = node.value_f64("drill").unwrap_or(0.3);
        if size <= 0.0 || drill <= 0.0 || size > 10.0 || drill > 10.0 || drill >= size {
            bail!("The final via dimensions ({}/{}) mm are invalid; the drill must be smaller than the via", mm(size), mm(drill));
        }
    }
    Ok(())
}

fn followed(n: usize) -> String {
    match n {
        0 => String::new(),
        1 => ". 1 track end follows.".into(),
        n => format!(". {n} track ends follow."),
    }
}

struct PadMove {
    pad: Pad,
    old_fp: (f64, f64, f64),
    new_fp: (f64, f64, f64),
}

fn transformed(p: Point, old: (f64, f64, f64), new: (f64, f64, f64)) -> Point {
    let local = rotate(p.0 - old.0, p.1 - old.1, -old.2);
    let moved = rotate(local.0, local.1, new.2);
    (new.0 + moved.0, new.1 + moved.1)
}

fn near_pad(p: Point, pad: &Pad, margin: f64) -> bool {
    let poly = pad.polygon();
    point_in_polygon(p, &poly)
        || (0..poly.len()).any(|i| point_segment_distance(p, poly[i], poly[(i + 1) % poly.len()]) <= margin)
}

fn line_near_pad(a: Point, b: Point, pad: &Pad, margin: f64) -> bool {
    let poly = pad.polygon();
    point_in_polygon(a, &poly)
        || point_in_polygon(b, &poly)
        || (0..poly.len()).any(|i| segment_distance(a, b, poly[i], poly[(i + 1) % poly.len()]) <= margin)
}

fn follow_pad_edits(root: &Sexp, nets: &std::collections::HashMap<i64, String>, moves: &[PadMove]) -> Result<Vec<Edit>> {
    let mut out = Vec::new();
    for node in root.children("segment").chain(root.children("arc")) {
        let net = net_name(node, nets);
        let layer = layer_of(node);
        let points = connection_points(node);
        let margin = node.value_f64("width").unwrap_or(0.25) / 2.0;
        let relevant: Vec<&PadMove> = moves
            .iter()
            .filter(|m| m.pad.net == net && m.pad.on_layer(&layer) && points.windows(2).any(|p| line_near_pad(p[0], p[1], &m.pad, margin)))
            .collect();
        if relevant.is_empty() {
            continue;
        }
        if node.head() == Some("arc") {
            bail!("can't move a footprint connected to an arc; reroute that connection first");
        }
        let connected_endpoints: Vec<(&str, &Sexp, &PadMove)> = ["start", "end"]
            .into_iter()
            .filter_map(|key| node.child(key).map(|coord| (key, coord)))
            .filter_map(|(key, coord)| {
                relevant.iter().find(|m| near_pad(coord.xy(), &m.pad, margin)).map(|m| (key, coord, *m))
            })
            .collect();
        for pad in &relevant {
            if !connected_endpoints.iter().any(|(_, _, m)| std::ptr::eq(*m, *pad)) {
                bail!("can't move a footprint connected to the middle of a track ({} on {}, pad at {},{}, track {:?}); reroute that connection first",
                    pad.pad.number, layer, mm(pad.pad.x), mm(pad.pad.y), points);
            }
        }
        for (key, coord, pad) in connected_endpoints {
            let p = transformed(coord.xy(), pad.old_fp, pad.new_fp);
            out.push(Edit { start: coord.span().start, end: coord.span().end, text: format!("({key} {} {})", mm(p.0), mm(p.1)) });
        }
    }
    Ok(out)
}

fn via_reaches_layer(board: &Board, layers: &[String], layer: &str) -> bool {
    let stack: Vec<&str> = board.layers.iter().filter(|l| l.name.ends_with(".Cu")).map(|l| l.name.as_str()).collect();
    if layers.len() < 2 {
        return layers.iter().any(|l| l == layer);
    }
    match (stack.iter().position(|l| *l == layers[0]), stack.iter().position(|l| *l == layers[1])) {
        (Some(a), Some(b)) => stack[a.min(b)..=a.max(b)].contains(&layer),
        _ => layers.iter().any(|l| l == layer),
    }
}

fn follow_via_edits(
    root: &Sexp,
    nets: &std::collections::HashMap<i64, String>,
    board: &Board,
    via: &crate::board::Via,
    new: Point,
) -> Result<Vec<Edit>> {
    let mut out = Vec::new();
    let old = (via.x, via.y);
    for node in root.children("segment").chain(root.children("arc")) {
        let layer = layer_of(node);
        if net_name(node, nets) != via.net || !via_reaches_layer(board, &via.layers, &layer) {
            continue;
        }
        let radius = via.size / 2.0 + node.value_f64("width").unwrap_or(0.25) / 2.0;
        let points = connection_points(node);
        if !points.windows(2).any(|p| segment_distance(old, old, p[0], p[1]) <= radius) {
            continue;
        }
        if node.head() == Some("arc") {
            bail!("can't move a via connected to an arc; reroute that connection first");
        }
        let mut connected = false;
        for key in ["start", "end"] {
            let Some(coord) = node.child(key) else { continue };
            let p = coord.xy();
            if (p.0 - old.0).hypot(p.1 - old.1) <= radius {
                connected = true;
                let moved = (p.0 + new.0 - old.0, p.1 + new.1 - old.1);
                out.push(Edit { start: coord.span().start, end: coord.span().end, text: format!("({key} {} {})", mm(moved.0), mm(moved.1)) });
            }
        }
        if !connected {
            bail!("can't move a via where a track passes through it; reroute that connection first");
        }
    }
    Ok(out)
}

/// Move and/or rotate a footprint. Pads move with it (they're stored relative to it); pad and text angles in
/// the file include the footprint's rotation, so they turn with it; connected track ends follow.
#[allow(clippy::too_many_arguments)]
fn plan_fp_move(
    root: &Sexp,
    board: &Board,
    nets: &std::collections::HashMap<i64, String>,
    node: &Sexp,
    item: &str,
    old: (f64, f64, f64),
    new: (f64, f64, f64),
    change: impl Fn(String, String) -> Change,
) -> Result<Plan> {
    let (x, y, angle) = new;
    if !(x.is_finite() && y.is_finite() && angle.is_finite()) {
        bail!("position must be a number");
    }
    let at = node.child("at").ok_or_else(|| anyhow!("footprint has no position"))?;
    let delta = angle - old.2;
    let mut edits = vec![Edit { start: at.span().start, end: at.span().end, text: at_with_angle(&sexpr::parse(&format!("(at {} {})", mm(x), mm(y)))?, angle) }];
    let fp = board.footprints.iter().find(|f| f.id == item);
    let mut moves = Vec::new();
    let mut pad_index = 0;
    for child in node.items().iter().skip(1).filter(|c| matches!(c.head(), Some("pad" | "property" | "fp_text"))) {
        let Some(cat) = child.child("at") else { continue };
        if delta.abs() > 1e-9 {
            edits.push(Edit { start: cat.span().start, end: cat.span().end, text: at_with_angle(cat, cat.num(3).unwrap_or(0.0) + delta) });
        }
        if child.head() == Some("pad") {
            let pad = fp.and_then(|f| f.pads.get(pad_index));
            pad_index += 1;
            let Some(pad) = pad else { continue };
            moves.push(PadMove { pad: pad.clone(), old_fp: old, new_fp: (x, y, angle) });
        }
    }
    let follow = follow_pad_edits(root, nets, &moves)?;
    let n = follow.len();
    edits.extend(follow);
    let reference = fp.map(|f| f.reference.as_str()).unwrap_or("footprint");
    let moved = (x - old.0).abs() > 1e-9 || (y - old.1).abs() > 1e-9;
    let title = match (moved, delta.abs() > 1e-9) {
        (true, true) => format!("Move and rotate {reference}"),
        (false, true) => format!("Rotate {reference} to {}°", mm(angle.rem_euclid(360.0))),
        _ => format!("Move {reference}"),
    };
    let half = fp.map(|f| {
        let b = f.bbox();
        ((b.2 - b.0).hypot(b.3 - b.1)) / 2.0
    });
    Ok(Plan {
        change: change(
            title,
            format!("({}, {}) {}° → ({}, {}) {}° mm{}", mm(old.0), mm(old.1), mm(old.2.rem_euclid(360.0)), mm(x), mm(y), mm(angle.rem_euclid(360.0)), followed(n)),
        ),
        edits,
        highlights: vec![Highlight::Circle { x, y, r: half.unwrap_or(2.0) }],
    })
}

/// `(net ...)` for a new item, written the way this file writes nets.
fn net_node(root: &Sexp, nets: &std::collections::HashMap<i64, String>, name: &str) -> Result<String> {
    if nets.is_empty() {
        // Newer files without a net table name nets directly.
        return Ok(if name.is_empty() { String::new() } else { format!("(net {})", sexpr::quote(name)) });
    }
    let num = if name.is_empty() {
        0
    } else {
        *nets.iter().find(|(_, n)| n.as_str() == name).ok_or_else(|| anyhow!("there's no net {name} on this board"))?.0
    };
    let named = root.children("segment").chain(root.children("via")).next().and_then(|s| s.child("net")).is_some_and(|n| n.arg(2).is_some());
    Ok(if named { format!("(net {num} {})", sexpr::quote(name)) } else { format!("(net {num})") })
}

fn point(s: &str) -> Result<Point> {
    let (x, y) = s.split_once(',').ok_or_else(|| anyhow!("'{s}' isn't an x,y point"))?;
    Ok((number(x, "X")?, number(y, "Y")?))
}

/// New tracks and vias go just before the board's closing parenthesis.
fn plan_add(text: &str, root: &Sexp, board: &Board, id: &str) -> Result<Plan> {
    let nets = net_table(root);
    let end = root.span().end - 1;
    let lead = if text[..end].ends_with('\n') { "" } else { "\n" };
    let parts: Vec<&str> = id.splitn(6, ':').collect();
    match parts.get(1).copied() {
        Some("track") => {
            let [_, _, layer, width, pts, net] = parts[..] else { bail!("bad track '{id}'") };
            if !copper_layers(board).iter().any(|l| l == layer) {
                bail!("{layer} isn't a copper layer on this board");
            }
            let w = number(width, "Width")?;
            if !(0.05..=10.0).contains(&w) {
                bail!("Width must be between 0.05 and 10 mm");
            }
            let mut points: Vec<Point> = Vec::new();
            for p in pts.split(';') {
                let p = point(p)?;
                if points.last().is_none_or(|l: &Point| (l.0 - p.0).hypot(l.1 - p.1) > 1e-4) {
                    points.push(p);
                }
            }
            if points.len() < 2 {
                bail!("a track needs two different points");
            }
            let net_sx = net_node(root, &nets, net)?;
            let mut out = String::from(lead);
            for (i, w2) in points.windows(2).enumerate() {
                out.push_str(&format!(
                    "\t(segment\n\t\t(start {} {})\n\t\t(end {} {})\n\t\t(width {})\n\t\t(layer {})\n{}\t\t(uuid \"{}\")\n\t)\n",
                    mm(w2[0].0),
                    mm(w2[0].1),
                    mm(w2[1].0),
                    mm(w2[1].1),
                    mm(w),
                    sexpr::quote(layer),
                    if net_sx.is_empty() { String::new() } else { format!("\t\t{net_sx}\n") },
                    uuid_for(&format!("{id}#{i}"))
                ));
            }
            let length: f64 = points.windows(2).map(|p| (p[1].0 - p[0].0).hypot(p[1].1 - p[0].1)).sum();
            Ok(Plan {
                change: Change {
                    id: id.into(),
                    title: format!("Route {}", if net.is_empty() { "a track (no net)" } else { net }),
                    detail: format!("{} segment{} on {layer}, {} mm wide, {:.2} mm long", points.len() - 1, if points.len() == 2 { "" } else { "s" }, mm(w), length),
                },
                edits: vec![Edit { start: end, end, text: out }],
                highlights: vec![Highlight::Line { points, width: w }],
            })
        }
        Some("via") => {
            let [_, _, at, size, drill, net] = parts[..] else { bail!("bad via '{id}'") };
            let (x, y) = point(at)?;
            let (size, drill) = (number(size, "Diameter")?, number(drill, "Drill")?);
            if drill <= 0.0 || drill >= size || size > 10.0 {
                bail!("the drill must be smaller than the via, and both positive");
            }
            let net_sx = net_node(root, &nets, net)?;
            let (top, bottom) = {
                let cu = copper_layers(board);
                (cu.first().cloned().unwrap_or_else(|| "F.Cu".into()), cu.last().cloned().unwrap_or_else(|| "B.Cu".into()))
            };
            let out = format!(
                "{lead}\t(via\n\t\t(at {} {})\n\t\t(size {})\n\t\t(drill {})\n\t\t(layers {} {})\n{}\t\t(uuid \"{}\")\n\t)\n",
                mm(x),
                mm(y),
                mm(size),
                mm(drill),
                sexpr::quote(&top),
                sexpr::quote(&bottom),
                if net_sx.is_empty() { String::new() } else { format!("\t\t{net_sx}\n") },
                uuid_for(id)
            );
            Ok(Plan {
                change: Change {
                    id: id.into(),
                    title: format!("Add via on {}", if net.is_empty() { "no net" } else { net }),
                    detail: format!("({}, {}) mm, {}/{} mm diameter/drill", mm(x), mm(y), mm(size), mm(drill)),
                },
                edits: vec![Edit { start: end, end, text: out }],
                highlights: vec![Highlight::Circle { x, y, r: size / 2.0 }],
            })
        }
        _ => bail!("unknown item to add in '{id}'"),
    }
}

fn fp_highlight((x1, y1, x2, y2): (f64, f64, f64, f64)) -> Highlight {
    Highlight::Area { points: vec![(x1, y1), (x2, y1), (x2, y2), (x1, y2)] }
}

/// What the properties card (and the agent) shows for a board item: `track:<id>`, `via:<id>` or `fp:<id>`.
pub fn inspect(board: &Board, item: &str) -> Result<Value> {
    let (kind, id) = item.split_once(':').ok_or_else(|| anyhow!("bad item '{item}'"))?;
    let gone = || anyhow!("that item isn't on the board any more");
    let r = |v: f64| (v * 1000.0).round() / 1000.0;
    match kind {
        "track" => {
            let t = board.tracks.iter().find(|t| t.id == id).ok_or_else(gone)?;
            let (a, b) = (t.points[0], *t.points.last().unwrap_or(&t.points[0]));
            Ok(json!({
                "item": item, "kind": "track", "net": t.net, "layer": t.layer, "width": r(t.width), "length": r(t.length()),
                "start": [r(a.0), r(a.1)], "end": [r(b.0), r(b.1)], "arc": t.points.len() > 2, "copper_layers": copper_layers(board),
            }))
        }
        "via" => {
            let v = board.vias.iter().find(|v| v.id == id).ok_or_else(gone)?;
            Ok(json!({"item": item, "kind": "via", "net": v.net, "x": r(v.x), "y": r(v.y), "size": r(v.size), "drill": r(v.drill)}))
        }
        "fp" => {
            let f = board.footprints.iter().find(|f| f.id == id).ok_or_else(gone)?;
            let nets: std::collections::BTreeSet<&str> = f.pads.iter().map(|p| p.net.as_str()).filter(|n| !n.is_empty()).collect();
            Ok(json!({
                "item": item, "kind": "footprint", "reference": f.reference, "value": f.value, "lib_id": f.lib_id,
                "x": r(f.x), "y": r(f.y), "angle": r(f.angle.rem_euclid(360.0)), "side": if f.layer.starts_with("B.") { "back" } else { "front" },
                "pads": f.pads.len(), "nets": nets, "attrs": f.attrs,
            }))
        }
        other => bail!("unknown item kind '{other}'"),
    }
}
