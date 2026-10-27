//! Minimal MCP server over stdio (newline-delimited JSON-RPC 2.0), exposing pcbcheck as agent tools.

use std::io::{BufRead, Write};
use std::path::PathBuf;

use anyhow::Result;
use base64::Engine;
use crate::report::{self, default_out_dir, Options};
use crate::{fixes, keyboard, render, CATALOG};
use serde_json::{json, Value};

const SUPPORTED: &[&str] = &["2026-07-28", "2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

const INSTRUCTIONS: &str = "pcbcheck reviews KiCad PCBs before ordering. Call check_board with a project \
directory or .kicad_pcb path. Fix errors first, then warnings; info findings are context. Findings with a fix_id can \
be fixed automatically: call list_fixes, apply_fix with dry_run to preview, then apply_fix (undo_fix restores the \
backup). Everything else needs a real design edit; prefer changing the schematic for wiring problems, then update the \
PCB. Re-run check_board after each change. Use board_image to look at the layout with numbered markers matching the \
findings, and explain_check for why a finding matters. review_parts lists every component with its ratings vs what \
the board puts on it; use it when the user asks whether parts fit or are over-spec, and fill gaps from your own \
datasheet knowledge. Keyboard boards get extra matrix/diode/controller checks \
automatically; call get_matrix only when the user is working on keyboard firmware (ZMK/QMK pins, diode direction). \
Only edit the user's KiCad files when they've asked you to fix something.";

fn tools() -> Value {
    let path = json!({"type": "string", "description": "KiCad project directory, .kicad_pro, or .kicad_pcb path (absolute)."});
    json!([
        {
            "name": "check_board",
            "title": "Check KiCad PCB",
            "description": "Run all pcbcheck checks (routing completeness, fab rules, board outline, keyboard checks when switches are present, \
plus KiCad DRC/ERC when kicad-cli is installed). Writes report.json, report.md, viewer.html and board.png to <project>/pcbcheck-out and returns the report.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": path,
                    "profile": {"type": "string", "enum": ["jlcpcb", "safe"], "description": "Fab rules profile (default jlcpcb or pcbcheck.json)."},
                    "use_kicad": {"type": "boolean", "description": "Run kicad-cli DRC/ERC if installed (default true)."}
                },
                "required": ["path"]
            },
            "annotations": {"readOnlyHint": false, "destructiveHint": false, "idempotentHint": true}
        },
        {
            "name": "board_image",
            "title": "Render board image",
            "description": "PNG of the board (front+back copper, pads, outline, references) with numbered markers for each error/warning, matching the order of check_board findings.",
            "inputSchema": {
                "type": "object",
                "properties": {"path": path, "width": {"type": "integer", "minimum": 400, "maximum": 3000, "description": "Image width in pixels (default 1400)."}},
                "required": ["path"]
            },
            "annotations": {"readOnlyHint": true}
        },
        {
            "name": "get_matrix",
            "title": "Get keyboard matrix",
            "description": "Keyboard boards only, on demand: derived switch matrix (keys, row/column nets, controller pin for each line, diode direction col2row/row2col, GPIO budget) for writing ZMK/QMK firmware config.",
            "inputSchema": {"type": "object", "properties": {"path": path}, "required": ["path"]},
            "annotations": {"readOnlyHint": true}
        },
        {
            "name": "explain_check",
            "title": "Explain a check",
            "description": "Why a pcbcheck finding id matters and how to fix it. Omit id to list every check.",
            "inputSchema": {"type": "object", "properties": {"id": {"type": "string", "description": "Finding id, e.g. conn.unrouted"}}},
            "annotations": {"readOnlyHint": true}
        },
        {
            "name": "review_parts",
            "title": "Review components",
            "description": "Bill of materials with each part's ratings, what the board puts on it (rail voltages, LED current, \
resistor power, regulator input) and a verdict: under-rated, over-spec (bigger/costlier/higher-rated than needed), ok or unknown. \
Ratings come from values/descriptions, package defaults and a built-in library of common parts. Use it to judge whether \
components fit the board's job; fill gaps for unknown parts from your own datasheet knowledge.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": path,
                    "profile": {"type": "string", "enum": ["jlcpcb", "safe"]}
                },
                "required": ["path"]
            },
            "annotations": {"readOnlyHint": true}
        },
        {
            "name": "list_fixes",
            "title": "List automatic fixes",
            "description": "Fixes pcbcheck can apply by itself (track widths, via sizes, antenna keep-outs), each with the findings it resolves. \
Anything not listed needs a real design edit by you or the user.",
            "inputSchema": {"type": "object", "properties": {"path": path, "profile": {"type": "string", "enum": ["jlcpcb", "safe"]}}, "required": ["path"]},
            "annotations": {"readOnlyHint": true}
        },
        {
            "name": "apply_fix",
            "title": "Apply automatic fixes",
            "description": "Apply fixes from list_fixes to the .kicad_pcb. Pass fix ids, or [\"all\"]. Use dry_run first to see what changes and \
the error count before/after. A backup is saved to pcbcheck-out/backups and returned so the change can be undone with undo_fix. \
Tell the user to reload the board in KiCad (File > Revert) if it is open.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": path,
                    "fixes": {"type": "array", "items": {"type": "string"}, "minItems": 1, "description": "Fix ids from list_fixes, or [\"all\"]."},
                    "dry_run": {"type": "boolean", "description": "Only report what would change (default false)."},
                    "profile": {"type": "string", "enum": ["jlcpcb", "safe"]}
                },
                "required": ["path", "fixes"]
            },
            "annotations": {"readOnlyHint": false, "destructiveHint": false, "idempotentHint": false}
        },
        {
            "name": "undo_fix",
            "title": "Undo a fix",
            "description": "Restore the board from a backup path returned by apply_fix.",
            "inputSchema": {"type": "object", "properties": {"path": path, "backup": {"type": "string"}}, "required": ["path", "backup"]},
            "annotations": {"readOnlyHint": false, "destructiveHint": true}
        }
    ])
}

fn server_info(version: &str) -> Value {
    json!({
        "protocolVersion": version,
        "capabilities": {"tools": {"listChanged": false}},
        "serverInfo": {"name": "pcbcheck", "title": "pcbcheck", "version": report::VERSION},
        "instructions": INSTRUCTIONS
    })
}

fn text_result(text: String, structured: Option<Value>, is_error: bool) -> Value {
    let mut r = json!({"content": [{"type": "text", "text": text}], "isError": is_error});
    if let Some(s) = structured {
        r["structuredContent"] = s;
    }
    r
}

fn arg_path(args: &Value) -> Result<PathBuf> {
    args.get("path").and_then(Value::as_str).map(PathBuf::from).ok_or_else(|| anyhow::anyhow!("missing required argument 'path'"))
}

fn call_tool(name: &str, args: &Value) -> Result<Value> {
    match name {
        "check_board" => {
            let path = arg_path(args)?;
            let (pcb, _) = report::resolve(&path)?;
            let out_dir = default_out_dir(&pcb);
            let opts = Options {
                profile: args.get("profile").and_then(Value::as_str).map(str::to_string),
                config: None,
                use_kicad: args.get("use_kicad").and_then(Value::as_bool).unwrap_or(true),
                out_dir: Some(out_dir.clone()),
            };
            let mut result = report::run(&pcb, &opts)?;
            report::write_outputs(&mut result, &out_dir, true)?;
            let value = serde_json::to_value(&result.report)?;
            Ok(text_result(serde_json::to_string_pretty(&value)?, Some(value), false))
        }
        "review_parts" => {
            let path = arg_path(args)?;
            let opts = Options { profile: arg_profile(args), use_kicad: false, ..Default::default() };
            let r = report::run(&path, &opts)?.report;
            let value = json!({
                "board": r.board,
                "how_to_read": "verdict: under = rated below what the board puts on it (also a finding), over = more part than needed \
(size/cost/rating), ok, unknown = ratings or board voltages missing, na = not rated (connectors, switches, modules). \
stress.voltage_assumed means one side's voltage is unknown so the max rail was assumed. For unknown/na parts, judge from \
your own datasheet knowledge using value, footprint, mpn and pins; say which conclusions are your judgement.",
                "parts": r.parts,
            });
            Ok(text_result(serde_json::to_string_pretty(&value)?, Some(value), false))
        }
        "board_image" => {
            let path = arg_path(args)?;
            let width = args.get("width").and_then(Value::as_u64).unwrap_or(1400).clamp(400, 3000) as u32;
            let opts = Options { use_kicad: false, ..Default::default() };
            let result = report::run(&path, &opts)?;
            let png = render::png(&result.svg, width)?;
            let r = &result.report;
            let legend: Vec<String> = r
                .findings
                .iter()
                .enumerate()
                .filter(|(_, f)| f.at.is_some() && f.severity != crate::Severity::Info)
                .map(|(i, f)| format!("{}. [{}] {}", i + 1, f.severity.as_str(), f.message))
                .collect();
            let caption = format!(
                "{} — status {} ({} errors, {} warnings). Red = front copper, blue = back copper, gold = through-hole pads.\nMarkers (built-in checks only):\n{}",
                r.board,
                r.status,
                r.summary.error,
                r.summary.warning,
                if legend.is_empty() { "none".into() } else { legend.join("\n") }
            );
            Ok(json!({
                "content": [
                    {"type": "image", "data": base64::engine::general_purpose::STANDARD.encode(png), "mimeType": "image/png"},
                    {"type": "text", "text": caption}
                ],
                "isError": false
            }))
        }
        "get_matrix" => {
            let path = arg_path(args)?;
            let (pcb, _) = report::resolve(&path)?;
            let cfg = crate::config::Config::load(pcb.parent().unwrap(), None)?;
            let board = crate::board::load(&pcb)?;
            let (_, matrix) = keyboard::check(&board, &cfg.keyboard);
            match matrix {
                Some(m) => {
                    let value = serde_json::to_value(&m)?;
                    Ok(text_result(serde_json::to_string_pretty(&value)?, Some(value), false))
                }
                None => Ok(text_result("No key switches found on this board.".into(), None, true)),
            }
        }
        "explain_check" => {
            let id = args.get("id").and_then(Value::as_str);
            let entries: Vec<Value> = CATALOG
                .iter()
                .filter(|e| id.is_none_or(|id| e.0 == id))
                .map(|(id, title, why, fix)| json!({"id": id, "title": title, "why": why, "fix": fix}))
                .collect();
            if entries.is_empty() {
                return Ok(text_result(format!("Unknown check id '{}'. Call explain_check without id to list all.", id.unwrap_or("")), None, true));
            }
            let value = json!({"checks": entries});
            Ok(text_result(serde_json::to_string_pretty(&value)?, Some(value), false))
        }
        "list_fixes" => {
            let path = arg_path(args)?;
            let opts = Options { profile: arg_profile(args), use_kicad: false, ..Default::default() };
            let r = report::run(&path, &opts)?.report;
            let fixes: Vec<Value> = r
                .fixes
                .iter()
                .map(|f| {
                    let resolves: Vec<String> = f.findings.iter().map(|&i| format!("{}: {}", r.findings[i].id, r.findings[i].message)).collect();
                    json!({"id": f.id, "title": f.title, "resolves": resolves})
                })
                .collect();
            let manual = r.findings.iter().filter(|f| f.fix_id.is_none() && f.severity != crate::Severity::Info).count();
            let value = json!({"board": r.board, "fixes": fixes, "findings_needing_manual_fix": manual});
            Ok(text_result(serde_json::to_string_pretty(&value)?, Some(value), false))
        }
        "apply_fix" => {
            let path = arg_path(args)?;
            let ids: Vec<String> = args
                .get("fixes")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
                .unwrap_or_default();
            if ids.is_empty() {
                anyhow::bail!("pass at least one fix id from list_fixes, or [\"all\"]");
            }
            let opts = Options { profile: arg_profile(args), use_kicad: false, ..Default::default() };
            let value = if args.get("dry_run").and_then(Value::as_bool).unwrap_or(false) {
                let p = fixes::preview(&path, &opts, &ids)?;
                json!({"dry_run": true, "changes": p.changes, "before": p.before, "after": p.after.report.summary})
            } else {
                let applied = fixes::apply(&path, &opts, &ids, None)?;
                let mut v = serde_json::to_value(&applied)?;
                v["note"] = json!("If the board is open in KiCad, reload it (File > Revert) before editing, or KiCad will overwrite this fix on save.");
                v
            };
            Ok(text_result(serde_json::to_string_pretty(&value)?, Some(value), false))
        }
        "undo_fix" => {
            let path = arg_path(args)?;
            let backup = args.get("backup").and_then(Value::as_str).ok_or_else(|| anyhow::anyhow!("missing 'backup'"))?;
            fixes::restore(&path, std::path::Path::new(backup))?;
            Ok(text_result(format!("Restored {backup}. Reload the board in KiCad if it is open."), None, false))
        }
        other => anyhow::bail!("unknown tool '{other}'"),
    }
}

fn arg_profile(args: &Value) -> Option<String> {
    args.get("profile").and_then(Value::as_str).map(str::to_string)
}

/// The last agent that connected to this MCP server, so the app can say who will act on a prompt.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AgentSeen {
    pub client: String,
    pub version: String,
    /// Unix seconds.
    pub at: u64,
}

fn agents_file() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
    Some(PathBuf::from(home).join(".pcbcheck").join("agent.json"))
}

fn record_agent(params: &Value) {
    let Some(file) = agents_file() else { return };
    let info = params.get("clientInfo");
    let seen = AgentSeen {
        client: info.and_then(|i| i.get("title").or_else(|| i.get("name"))).and_then(Value::as_str).unwrap_or("MCP client").to_string(),
        version: info.and_then(|i| i.get("version")).and_then(Value::as_str).unwrap_or_default().to_string(),
        at: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0),
    };
    if let (Some(dir), Ok(json)) = (file.parent(), serde_json::to_string(&seen)) {
        let _ = std::fs::create_dir_all(dir).and_then(|_| std::fs::write(&file, json));
    }
}

pub fn last_agent() -> Option<AgentSeen> {
    serde_json::from_str(&std::fs::read_to_string(agents_file()?).ok()?).ok()
}

/// Handle one JSON-RPC message; returns the response (None for notifications).
pub fn handle(msg: &Value) -> Option<Value> {
    let id = msg.get("id").cloned();
    let method = msg.get("method").and_then(Value::as_str).unwrap_or_default();
    let params = msg.get("params").cloned().unwrap_or(Value::Null);
    let id = id?; // notifications (no id) get no response

    let result = match method {
        "initialize" => {
            let requested = params.get("protocolVersion").and_then(Value::as_str).unwrap_or(SUPPORTED[1]);
            let version = if SUPPORTED.contains(&requested) { requested } else { SUPPORTED[1] };
            if std::env::var_os("PCBCHECK_NO_AGENT_RECORD").is_none() {
                record_agent(&params);
            }
            Ok(server_info(version))
        }
        "server/discover" => Ok(server_info(SUPPORTED[0])),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({"tools": tools()})),
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or_default();
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            if !tools().as_array().unwrap().iter().any(|t| t["name"] == name) {
                Err((-32602, format!("Unknown tool: {name}")))
            } else {
                // Tool failures are reported in-band so the model can read and react to them.
                Ok(call_tool(name, &args).unwrap_or_else(|e| text_result(format!("Error: {e:#}"), None, true)))
            }
        }
        _ => Err((-32601, format!("Method not found: {method}"))),
    };
    Some(match result {
        Ok(r) => json!({"jsonrpc": "2.0", "id": id, "result": r}),
        Err((code, message)) => json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}}),
    })
}

pub fn serve() -> Result<()> {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let response = match serde_json::from_str::<Value>(&line) {
            Ok(Value::Array(batch)) => {
                let out: Vec<Value> = batch.iter().filter_map(handle).collect();
                (!out.is_empty()).then_some(Value::Array(out))
            }
            Ok(msg) => handle(&msg),
            Err(e) => Some(json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700, "message": format!("Parse error: {e}")}})),
        };
        if let Some(r) = response {
            writeln!(stdout, "{r}")?;
            stdout.flush()?;
        }
    }
    Ok(())
}
