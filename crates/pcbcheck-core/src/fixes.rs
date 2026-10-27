//! Automatic fixes: deterministic text edits to a `.kicad_pcb`, with preview, backup and undo.
//!
//! Only changes that are safe to make without design judgement live here. Everything else (routing,
//! schematic changes, part moves) is left to the user or their agent.
//!
//! Fix ids are attached to findings by the checks:
//! - `widen-track:<layer>:<width>:<net>`: widen every track of that net/layer/width to a fab-safe width
//! - `resize-via:<x>:<y>`: enlarge the via's drill and/or diameter to meet the fab's minimums
//! - `antenna-keepout:<ref>`: add a "no copper pour" rule area over a wireless module

use std::collections::hash_map::DefaultHasher;
use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::board::{self, arc_points, layer_of, net_name, Board, Point};
use crate::config::Rules;
use crate::findings::Finding;
use crate::report::{self, CheckResult, Options, Summary};
use crate::sexpr::{self, Sexp};

/// One automatic fix offered by a report, and which findings it resolves.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FixSummary {
    pub id: String,
    pub title: String,
    /// Indexes into `Report::findings`.
    pub findings: Vec<usize>,
}

/// Replace `source[start..end]` with `text` (an insert when start == end).
#[derive(Debug, Clone, PartialEq)]
pub struct Edit {
    pub start: usize,
    pub end: usize,
    pub text: String,
}

/// Geometry to highlight in a preview.
#[derive(Debug, Clone)]
pub enum Highlight {
    Line { points: Vec<Point>, width: f64 },
    Circle { x: f64, y: f64, r: f64 },
    Area { points: Vec<Point> },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Change {
    pub id: String,
    pub title: String,
    pub detail: String,
}

#[derive(Debug, Clone)]
pub struct Plan {
    pub change: Change,
    pub edits: Vec<Edit>,
    pub highlights: Vec<Highlight>,
}

pub fn summarize(findings: &[Finding]) -> Vec<FixSummary> {
    let mut out: Vec<FixSummary> = Vec::new();
    for (i, f) in findings.iter().enumerate() {
        let Some(id) = &f.fix_id else { continue };
        match out.iter_mut().find(|s| &s.id == id) {
            Some(s) => s.findings.push(i),
            None => out.push(FixSummary { id: id.clone(), title: title(id), findings: vec![i] }),
        }
    }
    out
}

/// Short label for a fix id, without reading the board.
pub fn title(id: &str) -> String {
    let mut parts = id.splitn(4, ':');
    match parts.next() {
        Some("widen-track") => {
            let (layer, _, net) = (parts.next().unwrap_or(""), parts.next(), parts.next().unwrap_or(""));
            format!("Widen {} tracks on {layer}", if net.is_empty() { "unnamed" } else { net })
        }
        Some("resize-via") => "Enlarge via to fab minimums".into(),
        Some("antenna-keepout") => format!("Add copper keep-out over {}", parts.next().unwrap_or("module")),
        _ => id.to_string(),
    }
}

fn mm(v: f64) -> String {
    let s = format!("{v:.4}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-0" {
        "0".into()
    } else {
        s.to_string()
    }
}

fn round_up(v: f64, step: f64) -> f64 {
    ((v - 1e-9) / step).ceil() * step
}

fn rule(rules: &Rules, key: &str) -> f64 {
    rules.get(key).copied().unwrap_or(0.0)
}

fn atom_edit(node: &Sexp, key: &str, text: String) -> Option<Edit> {
    let atom = node.child(key)?.items().get(1)?;
    let span = atom.span();
    Some(Edit { start: span.start, end: span.end, text })
}

fn uuid_for(seed: &str) -> String {
    let mut h = DefaultHasher::new();
    seed.hash(&mut h);
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0).hash(&mut h);
    let a = h.finish();
    a.rotate_left(17).hash(&mut h);
    let b = h.finish();
    let hex = format!("{a:016x}{b:016x}");
    format!("{}-{}-4{}-8{}-{}", &hex[0..8], &hex[8..12], &hex[13..16], &hex[17..20], &hex[20..32])
}

/// Work out the edits for one fix id against the board source.
pub fn plan(text: &str, root: &Sexp, board: &Board, rules: &Rules, id: &str) -> Result<Plan> {
    let nets = board::net_table(root);
    let mut parts = id.splitn(4, ':');
    let kind = parts.next().unwrap_or_default();
    match kind {
        "widen-track" => {
            let layer = parts.next().ok_or_else(|| anyhow!("bad fix id {id}"))?;
            let width = parts.next().ok_or_else(|| anyhow!("bad fix id {id}"))?;
            let net = parts.next().unwrap_or("");
            let target = round_up(rule(rules, "min_track") + 0.01, 0.05).max(0.15);
            let mut edits = Vec::new();
            let mut highlights = Vec::new();
            for seg in root.children("segment").chain(root.children("arc")) {
                let w = seg.value_f64("width").unwrap_or(0.25);
                if layer_of(seg) != layer || net_name(seg, &nets) != net || format!("{w}") != width {
                    continue;
                }
                edits.extend(atom_edit(seg, "width", mm(target)));
                let points = if seg.head() == Some("arc") {
                    arc_points(seg.child_xy("start"), seg.child_xy("mid"), seg.child_xy("end"), 8)
                } else {
                    vec![seg.child_xy("start"), seg.child_xy("end")]
                };
                highlights.push(Highlight::Line { points, width: target });
            }
            if edits.is_empty() {
                bail!("no {width} mm tracks on {net} ({layer}) left to widen");
            }
            Ok(Plan {
                change: Change {
                    id: id.into(),
                    title: title(id),
                    detail: format!("{} segment{} from {width} mm to {} mm", edits.len(), if edits.len() == 1 { "" } else { "s" }, mm(target)),
                },
                edits,
                highlights,
            })
        }
        "resize-via" => {
            let x: f64 = parts.next().and_then(|v| v.parse().ok()).ok_or_else(|| anyhow!("bad fix id {id}"))?;
            let y: f64 = parts.next().and_then(|v| v.parse().ok()).ok_or_else(|| anyhow!("bad fix id {id}"))?;
            let via = root
                .children("via")
                .find(|v| {
                    let (vx, vy) = v.child_xy("at");
                    (vx - x).abs() < 1e-4 && (vy - y).abs() < 1e-4
                })
                .ok_or_else(|| anyhow!("no via at ({x}, {y}) any more"))?;
            let (size, drill) = (via.value_f64("size").unwrap_or(0.6), via.value_f64("drill").unwrap_or(0.3));
            let new_drill = drill.max(rule(rules, "min_via_drill"));
            let new_size = size.max(round_up(new_drill + 2.0 * rule(rules, "min_annular_ring"), 0.05));
            let mut edits = Vec::new();
            if (new_size - size).abs() > 1e-9 {
                edits.extend(atom_edit(via, "size", mm(new_size)));
            }
            if (new_drill - drill).abs() > 1e-9 {
                edits.extend(atom_edit(via, "drill", mm(new_drill)));
            }
            if edits.is_empty() {
                bail!("via at ({x}, {y}) already meets the fab minimums");
            }
            let net = net_name(via, &nets);
            Ok(Plan {
                change: Change {
                    id: id.into(),
                    title: title(id),
                    detail: format!(
                        "Via on {}: {}/{} mm → {}/{} mm (diameter/drill)",
                        if net.is_empty() { "no net" } else { &net },
                        mm(size),
                        mm(drill),
                        mm(new_size),
                        mm(new_drill)
                    ),
                },
                edits,
                highlights: vec![Highlight::Circle { x, y, r: new_size / 2.0 }],
            })
        }
        "antenna-keepout" => {
            let reference = parts.next().ok_or_else(|| anyhow!("bad fix id {id}"))?;
            let fp = board.footprint(reference).ok_or_else(|| anyhow!("no footprint {reference} on the board"))?;
            let (x1, y1, x2, y2) = fp.bbox();
            let m = 0.5;
            let pts = [(x1 - m, y1 - m), (x2 + m, y1 - m), (x2 + m, y2 + m), (x1 - m, y2 + m)];
            let mut zone = String::new();
            let xy = pts.iter().map(|(x, y)| format!("(xy {} {})", mm(*x), mm(*y))).collect::<Vec<_>>().join(" ");
            let _ = write!(
                zone,
                "\t(zone\n\t\t(net 0)\n\t\t(net_name \"\")\n\t\t(layers \"F&B.Cu\")\n\t\t(uuid \"{}\")\n\t\t(name {})\n\
                 \t\t(hatch edge 0.5)\n\t\t(connect_pads\n\t\t\t(clearance 0)\n\t\t)\n\t\t(min_thickness 0.25)\n\
                 \t\t(filled_areas_thickness no)\n\t\t(keepout\n\t\t\t(tracks allowed)\n\t\t\t(vias allowed)\n\
                 \t\t\t(pads allowed)\n\t\t\t(copperpour not_allowed)\n\t\t\t(footprints allowed)\n\t\t)\n\
                 \t\t(fill\n\t\t\t(thermal_gap 0.5)\n\t\t\t(thermal_bridge_width 0.5)\n\t\t)\n\
                 \t\t(polygon\n\t\t\t(pts\n\t\t\t\t{xy}\n\t\t\t)\n\t\t)\n\t)\n",
                uuid_for(id),
                sexpr::quote(&format!("pcbcheck keep-out {reference}"))
            );
            // Insert just before the board's closing parenthesis.
            let end = root.span().end - 1;
            let at = if text[..end].ends_with('\n') { end } else { zone.insert(0, '\n'); end };
            Ok(Plan {
                change: Change {
                    id: id.into(),
                    title: title(id),
                    detail: format!(
                        "Rule area over {reference} ({:.1} × {:.1} mm, both layers) that blocks copper pour. Refill zones in KiCad (B) afterwards.",
                        x2 - x1 + 2.0 * m,
                        y2 - y1 + 2.0 * m
                    ),
                },
                edits: vec![Edit { start: at, end: at, text: zone }],
                highlights: vec![Highlight::Area { points: pts.to_vec() }],
            })
        }
        _ => bail!("unknown fix '{id}'"),
    }
}

/// Apply non-overlapping edits (later offsets first so earlier spans stay valid).
pub fn apply_edits(text: &str, edits: &[Edit]) -> Result<String> {
    let mut sorted: Vec<&Edit> = edits.iter().collect();
    sorted.sort_by(|a, b| b.start.cmp(&a.start).then(b.end.cmp(&a.end)));
    sorted.dedup_by(|a, b| a == b);
    for pair in sorted.windows(2) {
        if pair[1].end > pair[0].start && pair[1].start != pair[1].end {
            bail!("conflicting edits at byte {}", pair[0].start);
        }
    }
    let mut out = text.to_string();
    for e in sorted {
        out.replace_range(e.start..e.end, &e.text);
    }
    Ok(out)
}

pub fn token(text: &str) -> String {
    let mut h = DefaultHasher::new();
    text.hash(&mut h);
    format!("{:016x}", h.finish())
}

/// SVG overlay group for preview highlights (green, drawn above everything else).
pub fn overlay_svg(highlights: &[Highlight]) -> String {
    const C: &str = "#3ecf8e";
    let pts = |p: &[Point]| p.iter().map(|(x, y)| format!("{x:.2},{y:.2}")).collect::<Vec<_>>().join(" ");
    let mut o = String::from("<g id=\"layer-P\" class=\"preview\">\n");
    for h in highlights {
        let _ = match h {
            Highlight::Line { points, width } => writeln!(
                o,
                r#"<polyline points="{}" fill="none" stroke="{C}" stroke-opacity="0.45" stroke-width="{:.2}" stroke-linecap="round" stroke-linejoin="round"/><polyline points="{}" fill="none" stroke="{C}" stroke-width="{:.3}" stroke-linecap="round"/>"#,
                pts(points),
                width + 0.9,
                pts(points),
                width
            ),
            Highlight::Circle { x, y, r } => writeln!(
                o,
                r#"<circle cx="{x:.2}" cy="{y:.2}" r="{:.2}" fill="{C}" fill-opacity="0.25" stroke="{C}" stroke-width="0.2"/>"#,
                r + 0.5
            ),
            Highlight::Area { points } => writeln!(
                o,
                r#"<polygon points="{}" fill="{C}" fill-opacity="0.12" stroke="{C}" stroke-width="0.25" stroke-dasharray="1 0.6"/>"#,
                pts(points)
            ),
        };
    }
    o.push_str("</g>\n");
    o
}

/// Expand `["all"]` to every fix the current report offers; dedupe the rest.
fn resolve_ids(ids: &[String], current: &CheckResult) -> Vec<String> {
    if ids.iter().any(|i| i == "all") {
        return current.report.fixes.iter().map(|f| f.id.clone()).collect();
    }
    let mut seen = BTreeSet::new();
    ids.iter().filter(|i| seen.insert((*i).clone())).cloned().collect()
}

struct Planned {
    pcb: PathBuf,
    sch: Option<PathBuf>,
    original: String,
    new_text: String,
    before: Summary,
    plans: Vec<Plan>,
}

fn plan_all(path: &Path, opts: &Options, ids: &[String]) -> Result<Planned> {
    let (pcb, sch) = report::resolve(path)?;
    let original = std::fs::read_to_string(&pcb).with_context(|| format!("reading {}", pcb.display()))?;
    let quick = Options { use_kicad: false, ..opts.clone() };
    let mut board = board::parse_board(&original)?;
    board.path = pcb.clone();
    let current = report::check_board(board, &pcb, sch.clone(), &quick)?;
    let ids = resolve_ids(ids, &current);
    if ids.is_empty() {
        bail!("nothing to fix: this board has no automatic fixes");
    }
    let (_, _, rules) = report::settings(&pcb, opts)?;
    let root = sexpr::parse(&original)?;
    let plans = ids.iter().map(|id| plan(&original, &root, &current.board, &rules, id)).collect::<Result<Vec<_>>>()?;
    let edits: Vec<Edit> = plans.iter().flat_map(|p| p.edits.iter().cloned()).collect();
    let new_text = apply_edits(&original, &edits)?;
    sexpr::parse(&new_text).context("fix produced an unreadable board (nothing was written)")?;
    Ok(Planned { pcb, sch, before: current.report.summary.clone(), original, new_text, plans })
}

/// The board as it would look after the fixes: re-checked report, SVG with highlights, and a token
/// that `apply` uses to make sure the file hasn't changed in between.
pub struct Preview {
    pub before: Summary,
    pub after: CheckResult,
    pub changes: Vec<Change>,
    pub token: String,
}

pub fn preview(path: &Path, opts: &Options, ids: &[String]) -> Result<Preview> {
    let p = plan_all(path, opts, ids)?;
    let mut board = board::parse_board(&p.new_text)?;
    board.path = p.pcb.clone();
    let quick = Options { use_kicad: false, ..opts.clone() };
    let mut after = report::check_board(board, &p.pcb, p.sch.clone(), &quick)?;
    let highlights: Vec<Highlight> = p.plans.iter().flat_map(|pl| pl.highlights.iter().cloned()).collect();
    if let Some(pos) = after.svg.rfind("</svg>") {
        after.svg.insert_str(pos, &overlay_svg(&highlights));
    }
    Ok(Preview { before: p.before, after, changes: p.plans.into_iter().map(|pl| pl.change).collect(), token: token(&p.original) })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Applied {
    pub board: String,
    pub backup: String,
    pub changes: Vec<Change>,
    pub before: Summary,
    pub after: Summary,
}

/// Write the fixes. A copy of the original goes to `pcbcheck-out/backups/` first.
/// Pass the preview's `token` to refuse writing if the file changed since it was previewed.
pub fn apply(path: &Path, opts: &Options, ids: &[String], expect_token: Option<&str>) -> Result<Applied> {
    let p = plan_all(path, opts, ids)?;
    if let Some(t) = expect_token {
        if t != token(&p.original) {
            bail!("the board changed since the preview; preview again before applying");
        }
    }
    let dir = report::default_out_dir(&p.pcb).join("backups");
    std::fs::create_dir_all(&dir)?;
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    let stem = p.pcb.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "board".into());
    let backup = dir.join(format!("{stem}-{stamp}.kicad_pcb"));
    std::fs::write(&backup, &p.original)?;
    std::fs::write(&p.pcb, &p.new_text)?;

    let mut board = board::parse_board(&p.new_text)?;
    board.path = p.pcb.clone();
    let quick = Options { use_kicad: false, ..opts.clone() };
    let after = report::check_board(board, &p.pcb, p.sch, &quick)?.report.summary;
    Ok(Applied {
        board: p.pcb.display().to_string(),
        backup: backup.display().to_string(),
        changes: p.plans.into_iter().map(|pl| pl.change).collect(),
        before: p.before,
        after,
    })
}

/// Undo: put a backup made by `apply` back in place.
pub fn restore(path: &Path, backup: &Path) -> Result<()> {
    let (pcb, _) = report::resolve(path)?;
    let backups = report::default_out_dir(&pcb).join("backups");
    let backup = std::fs::canonicalize(backup).with_context(|| format!("backup {} not found", backup.display()))?;
    if !backup.starts_with(std::fs::canonicalize(&backups).unwrap_or(backups)) {
        bail!("refusing to restore from outside pcbcheck-out/backups");
    }
    let text = std::fs::read_to_string(&backup)?;
    sexpr::parse(&text).context("backup is not a readable board")?;
    std::fs::write(&pcb, text)?;
    Ok(())
}
