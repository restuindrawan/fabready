use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Result;
use clap::{Parser, Subcommand};
use pcbcheck_core::config::PROFILE_NAMES;
use pcbcheck_core::keyboard;
use pcbcheck_core::report::{self, default_out_dir, Options};
use pcbcheck_core::{kicad, mcp, Severity, CATALOG};

#[derive(Parser)]
#[command(name = "pcbcheck", version, about = "Pre-order checks for KiCad PCBs (with extra checks for keyboards).")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run all checks and write report + viewer
    Check {
        /// .kicad_pcb, .kicad_pro, or project directory
        path: PathBuf,
        /// Fab rules profile (jlcpcb, safe)
        #[arg(long)]
        profile: Option<String>,
        /// Path to pcbcheck.json (default: next to the board)
        #[arg(long)]
        config: Option<PathBuf>,
        /// Output directory (default: <project>/pcbcheck-out)
        #[arg(long)]
        out: Option<PathBuf>,
        /// Print the full JSON result to stdout
        #[arg(long)]
        json: bool,
        /// Exit 1 on warnings too
        #[arg(long)]
        strict: bool,
        /// Skip kicad-cli even if installed
        #[arg(long)]
        no_kicad: bool,
        /// Skip PNG / 3D image generation
        #[arg(long)]
        no_png: bool,
        /// Also print info findings
        #[arg(short, long)]
        verbose: bool,
    },
    /// Review components: ratings vs what the board puts on them, and over-spec parts
    Parts {
        path: PathBuf,
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        config: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    /// Keyboard boards: print the switch matrix and controller pins (for ZMK/QMK)
    Matrix {
        path: PathBuf,
        #[arg(long)]
        config: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    /// Explain a check id (or list all)
    Explain { id: Option<String> },
    /// Show detected tools
    Doctor,
    /// Run as an MCP server over stdio (for Claude, Codex, Cursor, ...)
    Mcp,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.cmd {
        Cmd::Check { path, profile, config, out, json, strict, no_kicad, no_png, verbose } => {
            check(path, profile, config, out, json, strict, no_kicad, no_png, verbose)
        }
        Cmd::Parts { path, profile, config, json } => parts(path, profile, config, json),
        Cmd::Matrix { path, config, json } => matrix(path, config, json),
        Cmd::Explain { id } => explain(id),
        Cmd::Doctor => doctor(),
        Cmd::Mcp => mcp::serve().map(|_| ExitCode::SUCCESS),
    };
    result.unwrap_or_else(|e| {
        eprintln!("error: {e:#}");
        ExitCode::from(2)
    })
}

#[allow(clippy::too_many_arguments)]
fn check(
    path: PathBuf,
    profile: Option<String>,
    config: Option<PathBuf>,
    out: Option<PathBuf>,
    json: bool,
    strict: bool,
    no_kicad: bool,
    no_png: bool,
    verbose: bool,
) -> Result<ExitCode> {
    let (pcb, _) = report::resolve(&path)?;
    let out_dir = out.unwrap_or_else(|| default_out_dir(&pcb));
    let opts = Options { profile, config, use_kicad: !no_kicad, out_dir: Some(out_dir.clone()) };
    let mut result = report::run(&pcb, &opts)?;
    report::write_outputs(&mut result, &out_dir, !no_png)?;
    let r = &result.report;

    if json {
        println!("{}", serde_json::to_string_pretty(r)?);
    } else {
        let name = pcb.file_name().unwrap_or_default().to_string_lossy();
        println!(
            "{name}: {}  ({} errors, {} warnings, {} info)",
            r.status.to_uppercase(),
            r.summary.error,
            r.summary.warning,
            r.summary.info
        );
        for f in &r.findings {
            if f.severity != Severity::Info || verbose {
                let mark = match f.severity {
                    Severity::Error => "✗",
                    Severity::Warning => "!",
                    Severity::Info => "·",
                };
                println!("  {mark} [{}] {}", f.id, f.message);
            }
        }
        println!("\nViewer: {}\nReport: {}", r.files["viewer"], r.files["report_md"]);
        if let Some(png) = r.files.get("board_png") {
            println!("Image:  {png}");
        }
    }
    Ok(if r.status == "fail" || (strict && r.status == "warn") { ExitCode::from(1) } else { ExitCode::SUCCESS })
}

fn parts(path: PathBuf, profile: Option<String>, config: Option<PathBuf>, json: bool) -> Result<ExitCode> {
    let opts = Options { profile, config, use_kicad: false, ..Default::default() };
    let r = report::run(&path, &opts)?.report;
    if json {
        println!("{}", serde_json::to_string_pretty(&serde_json::json!({"parts": r.parts}))?);
        return Ok(ExitCode::SUCCESS);
    }
    let mark = |v: &str| match v {
        "under" => "✗ under",
        "over" => "↓ over ",
        "ok" => "✓ ok   ",
        "unknown" => "? ----",
        _ => "  n/a  ",
    };
    for p in &r.parts {
        let refs = if p.refs.len() > 4 { format!("{}…{} ×{}", p.refs[0], p.refs[p.refs.len() - 1], p.refs.len()) } else { p.refs.join(",") };
        println!("{}  {:<14} {:<22} {}", mark(&p.verdict), refs, p.value, p.package.as_deref().unwrap_or(""));
        for n in &p.notes {
            let tag = match n.level {
                pcbcheck_core::parts::Level::Under => "!",
                pcbcheck_core::parts::Level::Over => "↓",
                pcbcheck_core::parts::Level::Info => "·",
            };
            println!("           {tag} {}", n.text);
        }
    }
    let count = |v: &str| r.parts.iter().filter(|p| p.verdict == v).count();
    println!(
        "\n{} part types: {} under-rated, {} over-spec, {} ok, {} unknown",
        r.parts.len(),
        count("under"),
        count("over"),
        count("ok"),
        count("unknown"),
    );
    Ok(if count("under") > 0 { ExitCode::from(1) } else { ExitCode::SUCCESS })
}

fn matrix(path: PathBuf, config: Option<PathBuf>, json: bool) -> Result<ExitCode> {
    let (pcb, _) = report::resolve(&path)?;
    let cfg = pcbcheck_core::config::Config::load(pcb.parent().unwrap(), config.as_deref())?;
    let board = pcbcheck_core::board::load(&pcb)?;
    let (_, matrix) = keyboard::check(&board, &cfg.keyboard);
    let Some(m) = matrix else {
        eprintln!("No key switches found.");
        return Ok(ExitCode::from(1));
    };
    if json {
        println!("{}", serde_json::to_string_pretty(&m)?);
        return Ok(ExitCode::SUCCESS);
    }
    let (sl, dl) = (&m.switch_side_lines, &m.diode_side_lines);
    let width = sl.iter().chain(dl).map(String::len).max().unwrap_or(0).max(5) + 2;
    print!("{:width$}", "");
    for s in sl {
        print!("{s:width$}");
    }
    println!();
    for d in dl {
        print!("{d:width$}");
        for s in sl {
            let key = m.keys.iter().find(|k| k.switch_line.as_ref() == Some(s) && k.diode_line.as_ref() == Some(d));
            print!("{:width$}", key.map(|k| k.reference.as_str()).unwrap_or("-"));
        }
        println!();
    }
    println!("\ndiode-direction: {}", m.diode_direction.as_deref().unwrap_or("unknown (name nets ROWx/COLx to infer)"));
    for (line, pin) in &m.pins {
        println!("  {line:<10} -> {pin}");
    }
    Ok(ExitCode::SUCCESS)
}

fn explain(id: Option<String>) -> Result<ExitCode> {
    let entries: Vec<_> = match &id {
        Some(id) => CATALOG.iter().filter(|e| e.0 == id).collect(),
        None => CATALOG.iter().collect(),
    };
    if entries.is_empty() {
        eprintln!("unknown check id: {}", id.unwrap_or_default());
        return Ok(ExitCode::from(2));
    }
    for (id, title, why, fix) in entries {
        println!("{id}\n  {title}\n  why: {why}");
        if !fix.is_empty() {
            println!("  fix: {fix}");
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn doctor() -> Result<ExitCode> {
    println!("pcbcheck {}", report::VERSION);
    match kicad::find_cli() {
        Some(cli) => println!("kicad-cli: {}  ({})", cli.display(), kicad::version(&cli)),
        None => println!("kicad-cli: not found  -> brew install --cask kicad"),
    }
    println!("profiles: {}", PROFILE_NAMES.join(", "));
    Ok(ExitCode::SUCCESS)
}
