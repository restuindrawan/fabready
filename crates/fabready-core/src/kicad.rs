//! Optional integration with KiCad's own command-line tool (kicad-cli).

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

use crate::findings::{Finding, Severity};

const CANDIDATES: &[&str] = &[
    "/Applications/KiCad/KiCad.app/Contents/MacOS/kicad-cli",
    "/usr/bin/kicad-cli",
    "/usr/local/bin/kicad-cli",
    r"C:\Program Files\KiCad\10.0\bin\kicad-cli.exe",
    r"C:\Program Files\KiCad\9.0\bin\kicad-cli.exe",
    r"C:\Program Files\KiCad\8.0\bin\kicad-cli.exe",
];

pub fn find_cli() -> Option<PathBuf> {
    if let Some(paths) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&paths) {
            for name in ["kicad-cli", "kicad-cli.exe"] {
                let p = dir.join(name);
                if p.is_file() {
                    return Some(p);
                }
            }
        }
    }
    CANDIDATES.iter().map(PathBuf::from).find(|p| p.exists())
}

pub fn version(cli: &Path) -> String {
    Command::new(cli)
        .arg("version")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| "unknown".into())
}

fn run(cmd: &mut Command) -> Option<String> {
    match cmd.output() {
        Ok(o) if matches!(o.status.code(), Some(0) | Some(5)) => None,
        Ok(o) => {
            let text = if o.stderr.is_empty() { o.stdout } else { o.stderr };
            let msg = String::from_utf8_lossy(&text).trim().to_string();
            Some(if msg.is_empty() { format!("exit code {:?}", o.status.code()) } else { msg })
        }
        Err(e) => Some(e.to_string()),
    }
}

fn violations(items: Option<&Value>, source: &str, prefix: &str) -> Vec<Finding> {
    let mut out = Vec::new();
    for v in items.and_then(Value::as_array).into_iter().flatten() {
        if v.get("excluded").and_then(Value::as_bool).unwrap_or(false) {
            continue;
        }
        let severity = match v.get("severity").and_then(Value::as_str).unwrap_or("error") {
            "error" => Severity::Error,
            "warning" => Severity::Warning,
            _ => Severity::Info,
        };
        let parts = v.get("items").and_then(Value::as_array).cloned().unwrap_or_default();
        let detail = parts.iter().take(2).filter_map(|p| p.get("description")?.as_str()).collect::<Vec<_>>().join("; ");
        let desc = v.get("description").and_then(Value::as_str).unwrap_or_default();
        let kind = v.get("type").and_then(Value::as_str).unwrap_or("violation");
        let message = if detail.is_empty() { desc.to_string() } else { format!("{desc} — {detail}") };
        let mut f = Finding::new(&format!("{prefix}.{kind}"), severity, message).source(source);
        if f.title == f.id {
            f.title = desc.to_string();
        }
        if let Some(pos) = parts.first().and_then(|p| p.get("pos")) {
            if let (Some(x), Some(y)) = (pos.get("x").and_then(Value::as_f64), pos.get("y").and_then(Value::as_f64)) {
                f = f.at((x, y));
            }
        }
        out.push(f);
    }
    out
}

fn command_line(cli: &Path, args: &[String]) -> String {
    std::iter::once(cli.display().to_string()).chain(args.iter().cloned()).collect::<Vec<_>>().join(" ")
}

pub fn run_drc(cli: &Path, pcb: &Path, out_dir: &Path) -> Vec<Finding> {
    let report = out_dir.join("kicad-drc.json");
    let mut args: Vec<String> = ["pcb", "drc", "--format", "json", "--severity-all", "--units", "mm", "--output"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    args.push(report.display().to_string());
    if pcb.with_extension("kicad_sch").exists() {
        args.insert(2, "--schematic-parity".into());
    }
    args.push(pcb.display().to_string());
    let err = run(Command::new(cli).args(&args));
    let data = std::fs::read_to_string(&report).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok());
    let (None, Some(data)) = (err.as_ref(), data) else {
        return vec![Finding::new(
            "kicad.failed",
            Severity::Warning,
            format!("DRC failed: {}. Command: {}", err.unwrap_or_default(), command_line(cli, &args)),
        )];
    };
    let mut findings = violations(data.get("violations"), "kicad-drc", "drc");
    findings.extend(violations(data.get("unconnected_items"), "kicad-drc", "drc"));
    findings.extend(violations(data.get("schematic_parity"), "kicad-drc", "drc.parity"));
    findings
}

pub fn run_erc(cli: &Path, sch: &Path, out_dir: &Path) -> Vec<Finding> {
    let report = out_dir.join("kicad-erc.json");
    let args: Vec<String> = vec![
        "sch".into(),
        "erc".into(),
        "--format".into(),
        "json".into(),
        "--severity-all".into(),
        "--units".into(),
        "mm".into(),
        "--output".into(),
        report.display().to_string(),
        sch.display().to_string(),
    ];
    let err = run(Command::new(cli).args(&args));
    let data = std::fs::read_to_string(&report).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok());
    let (None, Some(data)) = (err.as_ref(), data) else {
        return vec![Finding::new(
            "kicad.failed",
            Severity::Warning,
            format!("ERC failed: {}. Command: {}", err.unwrap_or_default(), command_line(cli, &args)),
        )];
    };
    let mut findings = Vec::new();
    for sheet in data.get("sheets").and_then(Value::as_array).into_iter().flatten() {
        for mut f in violations(sheet.get("violations"), "kicad-erc", "erc") {
            f.at = None; // schematic coordinates don't map onto the board view
            findings.push(f);
        }
    }
    findings
}

/// KiCad 9+ ray-traced renders. Silently skipped on older versions.
pub fn render_3d(cli: &Path, pcb: &Path, out_dir: &Path) -> Vec<PathBuf> {
    let mut made = Vec::new();
    for side in ["top", "bottom"] {
        let out = out_dir.join(format!("kicad-3d-{side}.png"));
        let ok = run(Command::new(cli).args(["pcb", "render", "--side", side, "--width", "1600", "--height", "1000", "--output"]).arg(&out).arg(pcb))
            .is_none();
        if ok && out.exists() {
            made.push(out);
        }
    }
    made
}
