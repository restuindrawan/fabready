//! Run every check on a project and produce the report (JSON/Markdown/HTML/SVG/PNG).

use std::collections::BTreeMap;
use std::fmt::Write;
use std::path::{Path, PathBuf};

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

use crate::board::{self, Board};
use crate::config::{self, Config, Rules};
use crate::findings::{Finding, Severity};
use crate::keyboard::{self, Matrix};
use crate::{connectivity, fab, kicad, render};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Summary {
    pub error: usize,
    pub warning: usize,
    pub info: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tools {
    pub kicad_cli: Option<String>,
    pub kicad_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Stats {
    pub width_mm: Option<f64>,
    pub height_mm: Option<f64>,
    pub footprints: usize,
    pub tracks: usize,
    pub vias: usize,
    pub zones: usize,
    pub kicad_file_version: String,
}

impl Stats {
    fn of(board: &Board) -> Self {
        let size = board.outline_bbox().map(|b| (b.2 - b.0, b.3 - b.1));
        let round = |v: f64| (v * 100.0).round() / 100.0;
        Stats {
            width_mm: size.map(|s| round(s.0)),
            height_mm: size.map(|s| round(s.1)),
            footprints: board.footprints.len(),
            tracks: board.tracks.len(),
            vias: board.vias.len(),
            zones: board.zones.len(),
            kicad_file_version: board.version.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Report {
    pub tool: String,
    pub version: String,
    pub board: String,
    pub schematic: Option<String>,
    /// pass | warn | fail
    pub status: String,
    pub profile: String,
    pub rules: Rules,
    pub tools: Tools,
    pub stats: Stats,
    pub summary: Summary,
    pub findings: Vec<Finding>,
    /// Automatic fixes available for this report (see `fixes`), with indexes into `findings`.
    pub fixes: Vec<crate::fixes::FixSummary>,
    /// BOM lines with ratings, what the board puts on them, and a verdict (see `parts`).
    pub parts: Vec<crate::parts::Part>,
    pub matrix: Option<Matrix>,
    pub files: BTreeMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct Options {
    pub profile: Option<String>,
    pub config: Option<PathBuf>,
    pub use_kicad: bool,
    /// Where kicad-cli writes its JSON reports (and where `write_outputs` puts files).
    pub out_dir: Option<PathBuf>,
}

impl Default for Options {
    fn default() -> Self {
        Options { profile: None, config: None, use_kicad: true, out_dir: None }
    }
}

pub struct CheckResult {
    pub report: Report,
    pub svg: String,
    pub board: Board,
}

/// Accept a .kicad_pcb, .kicad_pro, .kicad_sch, or a project directory.
pub fn resolve(path: &Path) -> Result<(PathBuf, Option<PathBuf>)> {
    let p = std::fs::canonicalize(path).map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?;
    let pcb = if p.is_dir() {
        let mut boards: Vec<PathBuf> = std::fs::read_dir(&p)?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|f| f.extension().is_some_and(|x| x == "kicad_pcb"))
            .collect();
        boards.sort();
        match boards.into_iter().next() {
            Some(b) => b,
            None => bail!("no .kicad_pcb file in {}", p.display()),
        }
    } else if p.extension().is_some_and(|x| x == "kicad_pro" || x == "kicad_sch") {
        p.with_extension("kicad_pcb")
    } else {
        p
    };
    if !pcb.exists() {
        bail!("{} not found", pcb.display());
    }
    let sch = pcb.with_extension("kicad_sch");
    Ok((pcb, sch.exists().then_some(sch)))
}

pub fn default_out_dir(pcb: &Path) -> PathBuf {
    pcb.parent().unwrap_or(Path::new(".")).join("fabready-out")
}

fn ignored(f: &Finding, ignore: &[String]) -> bool {
    ignore.iter().any(|rule| {
        let (id, reference) = rule.split_once(':').unwrap_or((rule, ""));
        let id_match = f.id == id || id.strip_suffix('*').is_some_and(|prefix| f.id.starts_with(prefix));
        id_match && (reference.is_empty() || f.refs.iter().any(|r| r == reference))
    })
}

pub fn status_of(findings: &[Finding]) -> &'static str {
    if findings.iter().any(|f| f.severity == Severity::Error) {
        "fail"
    } else if findings.iter().any(|f| f.severity == Severity::Warning) {
        "warn"
    } else {
        "pass"
    }
}

pub fn run(path: &Path, opts: &Options) -> Result<CheckResult> {
    let (pcb, sch) = resolve(path)?;
    let board = board::load(&pcb)?;
    check_board(board, &pcb, sch, opts)
}

/// Run every check on an already-loaded board (also used to check fix previews held in memory).
pub fn check_board(board: Board, pcb: &Path, sch: Option<PathBuf>, opts: &Options) -> Result<CheckResult> {
    let pcb = pcb.to_path_buf();
    let (cfg, profile, rules) = settings(&pcb, opts)?;
    let (mut findings, matrix) = keyboard::check(&board, &cfg.keyboard);
    findings.extend(connectivity::check(&board));
    findings.extend(fab::check(&board, &rules));

    let mut parts = crate::parts::collect(&board, &cfg.power);
    findings.extend(crate::parts::evaluate(&mut parts));
    // Problems first, then over-spec, unknown, fine, and unrated parts; by reference within each.
    let rank = |v: &str| ["under", "over", "unknown", "ok", "na"].iter().position(|x| *x == v).unwrap_or(5);
    parts.sort_by(|a, b| {
        (rank(&a.verdict), keyboard::natural_key(&a.refs[0])).cmp(&(rank(&b.verdict), keyboard::natural_key(&b.refs[0])))
    });

    let cli = if opts.use_kicad { kicad::find_cli() } else { None };
    let tools = Tools {
        kicad_cli: cli.as_ref().map(|c| c.display().to_string()),
        kicad_version: cli.as_ref().map(|c| kicad::version(c)),
    };
    if opts.use_kicad && cli.is_none() {
        findings.push(Finding::new("kicad.missing", Severity::Info, "kicad-cli not found; ran fabready's built-in checks only."));
    }
    if let Some(cli) = &cli {
        let out_dir = opts.out_dir.clone().unwrap_or_else(|| default_out_dir(&pcb));
        std::fs::create_dir_all(&out_dir)?;
        findings.extend(kicad::run_drc(cli, &pcb, &out_dir));
        if let Some(sch) = &sch {
            findings.extend(kicad::run_erc(cli, sch, &out_dir));
        }
    }

    findings.retain(|f| !ignored(f, &cfg.ignore));
    findings.sort_by(|a, b| (a.severity, &a.id).cmp(&(b.severity, &b.id)));

    let count = |s| findings.iter().filter(|f| f.severity == s).count();
    let summary = Summary { error: count(Severity::Error), warning: count(Severity::Warning), info: count(Severity::Info) };
    let svg = render::svg(&board, &findings);
    let fixes = crate::fixes::summarize(&findings);
    let report = Report {
        tool: "fabready".into(),
        version: VERSION.into(),
        board: pcb.display().to_string(),
        schematic: sch.map(|s| s.display().to_string()),
        status: status_of(&findings).into(),
        profile,
        rules,
        tools,
        stats: Stats::of(&board),
        summary,
        findings,
        fixes,
        parts,
        matrix,
        files: BTreeMap::new(),
    };
    Ok(CheckResult { report, svg, board })
}

/// Project config, chosen profile name and the resulting fab rules.
pub fn settings(pcb: &Path, opts: &Options) -> Result<(Config, String, Rules)> {
    let cfg = Config::load(pcb.parent().unwrap_or(Path::new(".")), opts.config.as_deref())?;
    let profile = opts.profile.clone().or(cfg.profile.clone()).unwrap_or_else(|| "jlcpcb".into());
    let Some(mut rules) = config::profile(&profile) else {
        bail!("unknown profile '{profile}' (choose from {})", config::PROFILE_NAMES.join(", "));
    };
    rules.extend(cfg.fab.clone());
    Ok((cfg, profile, rules))
}

/// JSON the viewer consumes: `{report, svg}`.
pub fn viewer_data(report: &Report, svg: &str) -> String {
    serde_json::json!({ "report": report, "svg": svg }).to_string()
}

/// Write report.json, report.md, board.svg, viewer.html and (optionally) board.png + KiCad 3D renders.
pub fn write_outputs(result: &mut CheckResult, out_dir: &Path, images: bool) -> Result<()> {
    std::fs::create_dir_all(out_dir)?;
    let files = &mut result.report.files;
    let svg_path = out_dir.join("board.svg");
    std::fs::write(&svg_path, &result.svg)?;
    files.insert("board_svg".into(), svg_path.display().to_string());
    #[cfg(feature = "png")]
    if images {
        let png_path = out_dir.join("board.png");
        if let Ok(bytes) = render::png(&result.svg, 1800) {
            std::fs::write(&png_path, bytes)?;
            files.insert("board_png".into(), png_path.display().to_string());
        }
    }
    if images {
        if let Some(cli) = result.report.tools.kicad_cli.clone() {
            for (i, p) in kicad::render_3d(Path::new(&cli), Path::new(&result.report.board), out_dir).into_iter().enumerate() {
                files.insert(format!("render_3d_{i}"), p.display().to_string());
            }
        }
    }
    for (key, name) in [("viewer", "viewer.html"), ("report_json", "report.json"), ("report_md", "report.md")] {
        files.insert(key.into(), out_dir.join(name).display().to_string());
    }

    let report = &result.report;
    let title = Path::new(&report.board).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    std::fs::write(out_dir.join("viewer.html"), render::viewer_html(&title, &viewer_data(report, &result.svg)))?;
    std::fs::write(out_dir.join("report.json"), serde_json::to_string_pretty(report)?)?;
    std::fs::write(out_dir.join("report.md"), markdown(report))?;
    Ok(())
}

pub fn markdown(r: &Report) -> String {
    let name = Path::new(&r.board).file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let mut o = String::new();
    let _ = writeln!(
        o,
        "# fabready: {name}\n\n**Status: {}** · profile `{}` · {} errors · {} warnings · {} infos\n",
        r.status.to_uppercase(),
        r.profile,
        r.summary.error,
        r.summary.warning,
        r.summary.info
    );
    for (sev, heading) in [(Severity::Error, "Errors"), (Severity::Warning, "Warnings"), (Severity::Info, "Infos")] {
        let group: Vec<&Finding> = r.findings.iter().filter(|f| f.severity == sev).collect();
        if group.is_empty() {
            continue;
        }
        let _ = writeln!(o, "## {heading}\n");
        for f in group {
            let at = f.at.map(|a| format!(" @ ({:.1}, {:.1}) mm", a.x, a.y)).unwrap_or_default();
            let _ = writeln!(o, "- **{}** (`{}`{at}): {}", f.title, f.id, f.message);
            if !f.fix.is_empty() && sev != Severity::Info {
                let _ = writeln!(o, "  - Fix: {}", f.fix);
            }
        }
        o.push('\n');
    }
    if let Some(m) = r.matrix.as_ref().filter(|m| !m.pins.is_empty()) {
        let _ = writeln!(o, "## Matrix pins\n\nDiode direction: `{}`\n\n| Line | Controller pin |\n|---|---|", m.diode_direction.as_deref().unwrap_or("unknown"));
        for (line, pin) in &m.pins {
            let _ = writeln!(o, "| {line} | {pin} |");
        }
        o.push('\n');
    }
    if !r.files.is_empty() {
        o.push_str("## Files\n\n");
        for (k, v) in &r.files {
            let _ = writeln!(o, "- {k}: `{v}`");
        }
    }
    o
}
