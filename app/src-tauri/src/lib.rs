use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use notify_debouncer_mini::{new_debouncer, notify::RecursiveMode, DebounceEventResult, Debouncer};
use pcbcheck_core::report::{self, default_out_dir, Options};
use pcbcheck_core::render::{VIEWER_CSS, VIEWER_JS};
use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager, State};

type Watcher = Debouncer<notify_debouncer_mini::notify::RecommendedWatcher>;

#[derive(Default)]
struct AppState {
    watcher: Mutex<Option<Watcher>>,
    /// Profile used by live re-checks; updated whenever the UI runs a check.
    profile: Mutex<Option<String>>,
}

#[derive(Serialize)]
struct ViewerAssets {
    css: &'static str,
    js: &'static str,
}

#[derive(Serialize, Clone)]
struct CheckPayload {
    report: Value,
    svg: String,
}

fn run_check(path: &Path, profile: Option<String>) -> Result<CheckPayload, String> {
    let opts = Options { profile, ..Default::default() };
    let result = report::run(path, &opts).map_err(|e| format!("{e:#}"))?;
    Ok(CheckPayload { report: serde_json::to_value(&result.report).map_err(|e| e.to_string())?, svg: result.svg })
}

#[tauri::command]
fn viewer_assets() -> ViewerAssets {
    ViewerAssets { css: VIEWER_CSS, js: VIEWER_JS }
}

#[tauri::command]
async fn check_path(state: State<'_, AppState>, path: String, profile: Option<String>) -> Result<CheckPayload, String> {
    *state.profile.lock().unwrap() = profile.clone();
    tauri::async_runtime::spawn_blocking(move || run_check(Path::new(&path), profile)).await.map_err(|e| e.to_string())?
}

/// Write report.json/.md, viewer.html, board.svg/.png next to the project. Returns the output folder.
#[tauri::command]
async fn export_report(path: String, profile: Option<String>) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let (pcb, _) = report::resolve(Path::new(&path)).map_err(|e| format!("{e:#}"))?;
        let out_dir = default_out_dir(&pcb);
        let opts = Options { profile, out_dir: Some(out_dir.clone()), ..Default::default() };
        let mut result = report::run(&pcb, &opts).map_err(|e| format!("{e:#}"))?;
        report::write_outputs(&mut result, &out_dir, true).map_err(|e| format!("{e:#}"))?;
        Ok(out_dir.display().to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

fn is_relevant(p: &Path) -> bool {
    let name = p.file_name().and_then(|n| n.to_str()).unwrap_or_default();
    // KiCad writes via temp files and backups; only react to the real design files and our config.
    name == "pcbcheck.json"
        || (p.extension().is_some_and(|e| e == "kicad_pcb" || e == "kicad_sch") && !name.starts_with('_') && !name.starts_with('.'))
}

/// Watch the project folder; re-check and emit `check-result` / `check-error` after each save.
#[tauri::command]
fn watch(app: AppHandle, state: State<'_, AppState>, path: String) -> Result<String, String> {
    let (pcb, _) = report::resolve(Path::new(&path)).map_err(|e| format!("{e:#}"))?;
    let dir: PathBuf = pcb.parent().map(Path::to_path_buf).ok_or("board has no parent folder")?;
    let handle = app.clone();
    let target = pcb.clone();
    let mut debouncer = new_debouncer(Duration::from_millis(400), move |res: DebounceEventResult| {
        let Ok(events) = res else { return };
        if !events.iter().any(|e| is_relevant(&e.path)) {
            return;
        }
        let profile = handle.state::<AppState>().profile.lock().unwrap().clone();
        let _ = handle.emit("check-started", target.display().to_string());
        match run_check(&target, profile) {
            Ok(payload) => {
                let _ = handle.emit("check-result", payload);
            }
            Err(e) => {
                let _ = handle.emit("check-error", e);
            }
        }
    })
    .map_err(|e| e.to_string())?;
    debouncer.watcher().watch(&dir, RecursiveMode::NonRecursive).map_err(|e| e.to_string())?;
    *state.watcher.lock().unwrap() = Some(debouncer);
    Ok(dir.display().to_string())
}

#[tauri::command]
fn unwatch(state: State<'_, AppState>) {
    *state.watcher.lock().unwrap() = None;
}

/// Command line agents can use to start this app as an MCP server.
#[tauri::command]
fn mcp_command() -> Result<String, String> {
    std::env::current_exe().map(|p| p.display().to_string()).map_err(|e| e.to_string())
}

#[derive(Serialize)]
struct FixPreview {
    report: Value,
    svg: String,
    before: Value,
    changes: Value,
    token: String,
}

/// Board as it would be after the fixes (nothing is written). `fixes` may be `["all"]`.
#[tauri::command]
async fn preview_fixes(path: String, profile: Option<String>, fixes: Vec<String>) -> Result<FixPreview, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let opts = Options { profile, use_kicad: false, ..Default::default() };
        let p = pcbcheck_core::fixes::preview(Path::new(&path), &opts, &fixes).map_err(|e| format!("{e:#}"))?;
        Ok(FixPreview {
            report: serde_json::to_value(&p.after.report).map_err(|e| e.to_string())?,
            svg: p.after.svg,
            before: serde_json::to_value(&p.before).map_err(|e| e.to_string())?,
            changes: serde_json::to_value(&p.changes).map_err(|e| e.to_string())?,
            token: p.token,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Write previewed fixes (refuses if the board changed since `token`). Returns the backup path etc.
#[tauri::command]
async fn apply_fixes(path: String, profile: Option<String>, fixes: Vec<String>, token: String) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let opts = Options { profile, use_kicad: false, ..Default::default() };
        let applied = pcbcheck_core::fixes::apply(Path::new(&path), &opts, &fixes, Some(&token)).map_err(|e| format!("{e:#}"))?;
        serde_json::to_value(applied).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
fn restore_backup(path: String, backup: String) -> Result<(), String> {
    pcbcheck_core::fixes::restore(Path::new(&path), Path::new(&backup)).map_err(|e| format!("{e:#}"))
}

/// Last agent that connected over MCP (name + unix time), if any.
#[tauri::command]
fn agent_status() -> Option<pcbcheck_core::mcp::AgentSeen> {
    pcbcheck_core::mcp::last_agent()
}

/// Project passed on the command line (`pcbcheck-app ~/kb/my-board`), if any.
#[tauri::command]
fn initial_path() -> Option<String> {
    let arg = std::env::args().skip(1).find(|a| !a.starts_with('-'))?;
    std::fs::canonicalize(arg).ok().map(|p| p.display().to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            viewer_assets,
            check_path,
            export_report,
            watch,
            unwatch,
            mcp_command,
            initial_path,
            preview_fixes,
            apply_fixes,
            restore_backup,
            agent_status
        ])
        .run(tauri::generate_context!())
        .expect("error while running pcbcheck");
}
