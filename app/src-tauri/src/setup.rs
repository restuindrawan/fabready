//! Setup: find the apps fabready works with (for now KiCad, whose kicad-cli adds KiCad's DRC and ERC to every
//! check) and, when the user clicks Install, install a missing one with the platform's package manager: Homebrew
//! on macOS, winget on Windows. Without one, the official download page opens instead. The installer's output
//! streams to the window as `setup-progress` events, then `setup-done`.

use std::io::Read;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use serde_json::json;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::agent;

const KICAD_DOWNLOAD: &str = "https://www.kicad.org/download/";

#[derive(Serialize)]
pub struct Prereq {
    id: &'static str,
    name: &'static str,
    found: bool,
    path: Option<String>,
    version: Option<String>,
    /// The command Install runs, shown to the user; None when there's no package manager to run it.
    install: Option<String>,
    download: &'static str,
}

/// The installer currently running, if any (one at a time).
#[derive(Default)]
pub struct Installs {
    running: Mutex<Option<Arc<Mutex<Child>>>>,
}

/// How to install KiCad here: the package manager and its arguments.
fn kicad_installer() -> Option<(PathBuf, Vec<&'static str>)> {
    if cfg!(target_os = "macos") {
        return agent::find("brew").map(|b| (b, vec!["install", "--cask", "kicad"]));
    }
    if cfg!(windows) {
        return agent::find("winget").map(|w| {
            (w, vec!["install", "--id", "KiCad.KiCad", "-e", "--accept-package-agreements", "--accept-source-agreements", "--disable-interactivity"])
        });
    }
    None
}

fn display(program: &std::path::Path, args: &[&str]) -> String {
    let name = program.file_stem().and_then(|s| s.to_str()).unwrap_or("installer");
    std::iter::once(name).chain(args.iter().copied()).collect::<Vec<_>>().join(" ")
}

fn kicad_status() -> Prereq {
    let cli = fabready_core::kicad::find_cli();
    Prereq {
        id: "kicad",
        name: "KiCad",
        found: cli.is_some(),
        version: cli.as_ref().map(|c| fabready_core::kicad::version(c)),
        path: cli.map(|c| c.display().to_string()),
        install: kicad_installer().map(|(p, a)| display(&p, &a)),
        download: KICAD_DOWNLOAD,
    }
}

/// What's installed, what's missing, and how each missing one would be installed.
#[tauri::command]
pub async fn setup_status() -> Result<Vec<Prereq>, String> {
    tauri::async_runtime::spawn_blocking(|| vec![kicad_status()]).await.map_err(|e| e.to_string())
}

/// macOS: Homebrew asks sudo for the admin password when a package writes outside /Applications (KiCad's demos
/// go to /Library). With no terminal, sudo runs this helper instead, which shows the system password dialog.
/// The password goes straight to sudo.
#[cfg(target_os = "macos")]
fn askpass(app: &AppHandle) -> Option<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    let dir = app.path().app_cache_dir().ok()?;
    std::fs::create_dir_all(&dir).ok()?;
    let file = dir.join("askpass.sh");
    let script = "#!/bin/sh\nexec /usr/bin/osascript -e 'text returned of (display dialog \"Installing KiCad needs your Mac password.\" default answer \"\" with hidden answer with title \"Install KiCad\" with icon caution)'\n";
    std::fs::write(&file, script).ok()?;
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o700)).ok()?;
    Some(file)
}

/// Send the installer's output to the window line by line (progress bars redraw with \r, so split on that too).
fn stream(app: AppHandle, id: &'static str, mut from: impl Read + Send + 'static, tail: Arc<Mutex<Vec<String>>>) {
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        let mut line = Vec::new();
        let flush = |line: &mut Vec<u8>| {
            let text = String::from_utf8_lossy(line).trim().to_string();
            line.clear();
            if text.is_empty() {
                return;
            }
            let _ = app.emit("setup-progress", json!({"id": id, "line": text}));
            let mut t = tail.lock().unwrap();
            t.push(text);
            if t.len() > 8 {
                t.remove(0);
            }
        };
        while let Ok(n) = from.read(&mut buf) {
            if n == 0 {
                break;
            }
            for &b in &buf[..n] {
                if b == b'\n' || b == b'\r' {
                    flush(&mut line);
                } else {
                    line.push(b);
                }
            }
        }
        flush(&mut line);
    });
}

/// Install a missing prerequisite with the package manager. Returns once it has started.
#[tauri::command]
pub fn setup_install(app: AppHandle, installs: State<'_, Installs>, id: String) -> Result<(), String> {
    if id != "kicad" {
        return Err(format!("unknown prerequisite '{id}'"));
    }
    let mut running = installs.running.lock().unwrap();
    if running.is_some() {
        return Err("An install is already running.".into());
    }
    let (program, args) = kicad_installer().ok_or("No package manager to install KiCad with. Use the download page instead.")?;
    let mut cmd = Command::new(&program);
    cmd.args(&args).env_clear().envs(agent::user_env().iter().cloned()).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(target_os = "macos")]
    {
        cmd.env("HOMEBREW_NO_AUTO_UPDATE", "1").env("HOMEBREW_NO_ENV_HINTS", "1");
        if let Some(helper) = askpass(&app) {
            cmd.env("SUDO_ASKPASS", helper);
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let mut child = cmd.spawn().map_err(|e| format!("Couldn't start {}: {e}", display(&program, &args)))?;
    let tail = Arc::new(Mutex::new(Vec::new()));
    stream(app.clone(), "kicad", child.stdout.take().unwrap(), tail.clone());
    stream(app.clone(), "kicad", child.stderr.take().unwrap(), tail.clone());
    let child = Arc::new(Mutex::new(child));
    *running = Some(child.clone());

    std::thread::spawn(move || {
        let status = loop {
            match child.lock().unwrap().try_wait() {
                Ok(Some(s)) => break Some(s),
                Ok(None) => {}
                Err(_) => break None,
            }
            std::thread::sleep(Duration::from_millis(250));
        };
        // Cancel takes the entry out before killing the process, so a missing entry means the user stopped it.
        let cancelled = app.state::<Installs>().running.lock().unwrap().take().is_none();
        let found = fabready_core::kicad::find_cli().is_some();
        let (ok, message) = if cancelled {
            (false, "Install cancelled.".to_string())
        } else if found {
            (true, "KiCad is installed.".to_string())
        } else {
            let tail = tail.lock().unwrap().join("\n");
            let code = status.and_then(|s| s.code()).map(|c| format!(" (exit code {c})")).unwrap_or_default();
            (false, format!("The install didn't finish{code}.{}", if tail.is_empty() { String::new() } else { format!("\n{tail}") }))
        };
        let _ = app.emit("setup-done", json!({"id": "kicad", "ok": ok, "message": message}));
    });
    Ok(())
}

/// Stop the running install.
#[tauri::command]
pub fn setup_cancel(installs: State<'_, Installs>) {
    if let Some(child) = installs.running.lock().unwrap().take() {
        let _ = child.lock().unwrap().kill();
    }
}

/// Open a prerequisite's official download page in the browser.
#[tauri::command]
pub fn setup_download(id: String) -> Result<(), String> {
    if id != "kicad" {
        return Err(format!("unknown prerequisite '{id}'"));
    }
    #[cfg(target_os = "macos")]
    let r = Command::new("open").arg(KICAD_DOWNLOAD).spawn();
    #[cfg(windows)]
    let r = Command::new("cmd").args(["/C", "start", "", KICAD_DOWNLOAD]).spawn();
    #[cfg(not(any(target_os = "macos", windows)))]
    let r = Command::new("xdg-open").arg(KICAD_DOWNLOAD).spawn();
    r.map(|_| ()).map_err(|e| format!("Couldn't open {KICAD_DOWNLOAD}: {e}"))
}
