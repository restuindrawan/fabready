use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

mod agent;
mod bridge;

use notify_debouncer_mini::{new_debouncer, notify::RecursiveMode, DebounceEventResult, Debouncer};
use fabready_core::config::{self, Config, Rules};
use fabready_core::report::{self, default_out_dir, Options};
use fabready_core::render::{VIEWER_CSS, VIEWER_JS};
use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager, State};

type Watcher = Debouncer<notify_debouncer_mini::notify::RecommendedWatcher>;

#[derive(Default)]
struct AppState {
    watcher: Mutex<Option<Watcher>>,
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

fn run_check(path: &Path) -> Result<CheckPayload, String> {
    let result = report::run(path, &Options::default()).map_err(|e| format!("{e:#}"))?;
    Ok(CheckPayload { report: serde_json::to_value(&result.report).map_err(|e| e.to_string())?, svg: result.svg })
}

#[tauri::command]
fn viewer_assets() -> ViewerAssets {
    ViewerAssets { css: VIEWER_CSS, js: VIEWER_JS }
}

#[tauri::command]
async fn check_path(path: String) -> Result<CheckPayload, String> {
    tauri::async_runtime::spawn_blocking(move || run_check(Path::new(&path))).await.map_err(|e| e.to_string())?
}

/// Write report.json/.md, viewer.html, board.svg/.png next to the project. Returns the output folder.
#[tauri::command]
async fn export_report(path: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let (pcb, _) = report::resolve(Path::new(&path)).map_err(|e| format!("{e:#}"))?;
        let out_dir = default_out_dir(&pcb);
        let opts = Options { out_dir: Some(out_dir.clone()), ..Default::default() };
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
    name == "fabready.json"
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
        let _ = handle.emit("check-started", target.display().to_string());
        match run_check(&target) {
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
async fn preview_fixes(path: String, fixes: Vec<String>) -> Result<FixPreview, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let opts = Options { use_kicad: false, ..Default::default() };
        let p = fabready_core::fixes::preview(Path::new(&path), &opts, &fixes).map_err(|e| format!("{e:#}"))?;
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
async fn apply_fixes(path: String, fixes: Vec<String>, token: String) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let opts = Options { use_kicad: false, ..Default::default() };
        let applied = fabready_core::fixes::apply(Path::new(&path), &opts, &fixes, Some(&token)).map_err(|e| format!("{e:#}"))?;
        serde_json::to_value(applied).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[derive(Serialize)]
struct CheckInfo {
    id: &'static str,
    title: &'static str,
}

#[derive(Serialize)]
struct ConfigView {
    /// Where the project's fabready.json lives (or would be created).
    file: String,
    exists: bool,
    /// None when the file can't be read; see `error`.
    config: Option<Config>,
    error: Option<String>,
    /// Built-in profile limits, shown as the defaults the overrides replace.
    profiles: BTreeMap<String, Rules>,
    checks: Vec<CheckInfo>,
    /// Net names on the board, to suggest for power rails.
    nets: Vec<String>,
}

/// The project's settings, plus what the settings form needs to explain them.
#[tauri::command]
async fn load_config(path: String) -> Result<ConfigView, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let (pcb, _) = report::resolve(Path::new(&path)).map_err(|e| format!("{e:#}"))?;
        let file = config::path_for(&pcb);
        let (config, error) = if !file.exists() {
            (Some(Config::default()), None)
        } else {
            match Config::read(&file) {
                Ok(c) => {
                    let invalid = c.validate().err().map(|e| format!("{e:#}"));
                    (Some(c), invalid)
                }
                Err(e) => (None, Some(format!("{e:#}"))),
            }
        };
        let nets: BTreeSet<String> = fabready_core::board::load(&pcb)
            .map(|b| b.pads().map(|p| p.net.clone()).filter(|n| !n.is_empty()).collect())
            .unwrap_or_default();
        Ok(ConfigView {
            exists: file.exists(),
            file: file.display().to_string(),
            config,
            error,
            profiles: config::PROFILE_NAMES.iter().filter_map(|n| Some((n.to_string(), config::profile(n)?))).collect(),
            checks: fabready_core::CATALOG.iter().map(|e| CheckInfo { id: e.0, title: e.1 }).collect(),
            nets: nets.into_iter().collect(),
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Write the project's fabready.json (the file watcher then re-checks).
#[tauri::command]
fn save_config(path: String, config: Config) -> Result<String, String> {
    let (pcb, _) = report::resolve(Path::new(&path)).map_err(|e| format!("{e:#}"))?;
    let file = config::path_for(&pcb);
    config.save(&file).map_err(|e| format!("{e:#}"))?;
    Ok(file.display().to_string())
}

#[derive(Serialize)]
struct ItemsView {
    /// One entry per item asked for, in order (null for items no longer on the board).
    items: Vec<Value>,
    /// Hash of the board file, so an edit from the card is refused if the file changed since.
    token: String,
}

/// Board items (`track:<id>`, `via:<id>`, `fp:<id>`) as the properties card shows them.
#[tauri::command]
async fn inspect_items(path: String, items: Vec<String>) -> Result<ItemsView, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let (pcb, _) = report::resolve(Path::new(&path)).map_err(|e| format!("{e:#}"))?;
        let text = std::fs::read_to_string(&pcb).map_err(|e| e.to_string())?;
        let board = fabready_core::board::parse_board(&text).map_err(|e| format!("{e:#}"))?;
        let items = items.iter().map(|i| fabready_core::edit::inspect(&board, i).unwrap_or(Value::Null)).collect();
        Ok(ItemsView { items, token: fabready_core::fixes::token(&text) })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Write edits (`edit:`, `delete:`, `add:` ids) with a backup. With `token`, refuses if the file changed since.
#[tauri::command]
async fn apply_edits(path: String, edits: Vec<String>, token: Option<String>) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let opts = Options { use_kicad: false, ..Default::default() };
        let applied = fabready_core::fixes::apply(Path::new(&path), &opts, &edits, token.as_deref()).map_err(|e| format!("{e:#}"))?;
        serde_json::to_value(applied).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Undo or redo: put `backup` in place, keeping the current board as a new backup (returned with its token).
#[tauri::command]
async fn swap_backup(path: String, backup: String, token: String) -> Result<fabready_core::fixes::Swapped, String> {
    tauri::async_runtime::spawn_blocking(move || fabready_core::fixes::swap(Path::new(&path), Path::new(&backup), &token).map_err(|e| format!("{e:#}")))
        .await
        .map_err(|e| e.to_string())?
}

/// The window's answer to an `app-request` from an agent's app tool.
#[tauri::command]
fn app_reply(bridge: State<'_, Arc<bridge::Bridge>>, id: u64, result: Option<Value>, error: Option<String>) {
    bridge.reply(id, match error {
        Some(e) => Err(e),
        None => Ok(result.unwrap_or(Value::Null)),
    });
}

/// Last agent that connected over MCP (name + unix time), if any.
#[tauri::command]
fn agent_status() -> Option<fabready_core::mcp::AgentSeen> {
    fabready_core::mcp::last_agent()
}

/// Project passed on the command line (`fabready-app ~/kb/my-board`), if any.
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
        .manage(agent::Agents::default())
        .setup(|app| {
            app.manage(bridge::start(app.handle().clone())?);
            Ok(())
        })
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
            inspect_items,
            apply_edits,
            swap_backup,
            load_config,
            save_config,
            agent_status,
            app_reply,
            agent::agent_list,
            agent::agent_session,
            agent::agent_authenticate,
            agent::agent_prompt,
            agent::agent_cancel,
            agent::agent_permission,
            agent::agent_set_config,
            agent::agent_set_mode,
            agent::agent_stop
        ])
        .build(tauri::generate_context!())
        .expect("error while running fabready")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                app.state::<agent::Agents>().stop_all();
            }
        });
}
