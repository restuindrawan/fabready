//! Agent chat over the Agent Client Protocol (ACP, https://agentclientprotocol.com): newline-delimited
//! JSON-RPC with an agent process on stdio. OpenCode speaks it natively; Claude Code and Codex speak it
//! through their official adapters. One process per agent, one session per project. Every session gets this
//! app's MCP server, which includes the app tools (see bridge.rs).

use std::collections::{HashMap, VecDeque};
use std::ffi::OsString;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use fabready_core::mcp::{APP_ADDR_ENV, APP_PROJECT_ENV, APP_TOKEN_ENV};
use serde::Serialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager};

use crate::bridge::Bridge;

/// ACP's "authentication required" error code.
const AUTH_REQUIRED: i64 = -32000;

struct Spec {
    id: &'static str,
    name: &'static str,
    /// Installed adapter binary and its arguments.
    bin: (&'static str, &'static [&'static str]),
    /// npm package to run through npx when the binary isn't installed.
    npx: Option<&'static str>,
    login: &'static str,
}

const SPECS: &[Spec] = &[
    Spec {
        id: "claude",
        name: "Claude Code",
        bin: ("claude-agent-acp", &[]),
        npx: Some("@agentclientprotocol/claude-agent-acp"),
        login: "Run `claude` in Terminal and sign in (or set ANTHROPIC_API_KEY), then try again.",
    },
    Spec {
        id: "codex",
        name: "Codex",
        bin: ("codex-acp", &[]),
        npx: Some("@agentclientprotocol/codex-acp"),
        login: "Run `codex login` in Terminal (or set OPENAI_API_KEY), then try again.",
    },
    Spec {
        id: "opencode",
        name: "OpenCode",
        bin: ("opencode", &["acp"]),
        npx: None,
        login: "Run `opencode auth login` in Terminal, then try again.",
    },
];

fn spec(id: &str) -> Result<&'static Spec, String> {
    SPECS.iter().find(|s| s.id == id).ok_or_else(|| format!("unknown agent '{id}'"))
}

// ------------------------------------------------------------------ environment

/// The user's login-shell environment. Apps opened from the Finder don't get the PATH or API keys set in
/// shell profiles, and agents need both (node from nvm, ~/.opencode/bin, ANTHROPIC_API_KEY…).
pub(crate) fn user_env() -> &'static Vec<(OsString, OsString)> {
    static ENV: OnceLock<Vec<(OsString, OsString)>> = OnceLock::new();
    ENV.get_or_init(|| {
        let mut vars: HashMap<OsString, OsString> = std::env::vars_os().collect();
        for (k, v) in login_shell_env() {
            if !matches!(k.to_str(), Some("PWD" | "OLDPWD" | "SHLVL" | "_")) {
                vars.insert(k, v);
            }
        }
        let mut path: Vec<PathBuf> = vars.get(&OsString::from("PATH")).map(std::env::split_paths).map(Iterator::collect).unwrap_or_default();
        if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
            for dir in [".opencode/bin", ".local/bin", ".bun/bin", ".npm-global/bin", ".claude/local"] {
                path.push(home.join(dir));
            }
        }
        path.extend(["/opt/homebrew/bin", "/usr/local/bin"].map(PathBuf::from));
        let mut seen = std::collections::HashSet::new();
        path.retain(|p| seen.insert(p.clone()));
        if let Ok(joined) = std::env::join_paths(path) {
            vars.insert("PATH".into(), joined);
        }
        vars.into_iter().collect()
    })
}

#[cfg(unix)]
fn login_shell_env() -> Vec<(OsString, OsString)> {
    use std::os::unix::ffi::OsStrExt;
    const MARK: &[u8] = b"\n__FABREADY_ENV__\n";
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let out = Command::new(shell)
            .args(["-ilc", "printf '\\n__FABREADY_ENV__\\n'; env -0"])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output();
        let _ = tx.send(out);
    });
    // A slow or broken shell profile shouldn't hold up the chat; fall back to the app's own environment.
    let Ok(Ok(out)) = rx.recv_timeout(Duration::from_secs(6)) else { return Vec::new() };
    let Some(start) = out.stdout.windows(MARK.len()).rposition(|w| w == MARK) else { return Vec::new() };
    out.stdout[start + MARK.len()..]
        .split(|&b| b == 0)
        .filter_map(|kv| {
            let eq = kv.iter().position(|&b| b == b'=')?;
            Some((OsString::from(std::ffi::OsStr::from_bytes(&kv[..eq])), OsString::from(std::ffi::OsStr::from_bytes(&kv[eq + 1..]))))
        })
        .collect()
}

#[cfg(not(unix))]
fn login_shell_env() -> Vec<(OsString, OsString)> {
    Vec::new()
}

/// A program on the user's shell PATH (see `user_env`).
pub(crate) fn find(program: &str) -> Option<PathBuf> {
    let path = user_env().iter().find(|(k, _)| k == "PATH")?.1.clone();
    let exts: &[&str] = if cfg!(windows) { &[".cmd", ".exe", ""] } else { &[""] };
    std::env::split_paths(&path).find_map(|dir| exts.iter().map(|e| dir.join(format!("{program}{e}"))).find(|p| p.is_file()))
}

struct Launch {
    program: PathBuf,
    args: Vec<String>,
    /// How it's started, for the UI.
    via: String,
    /// Runs through npx, which downloads the adapter the first time.
    npx: bool,
}

fn launch(s: &Spec) -> Option<Launch> {
    if let Some(program) = find(s.bin.0) {
        let args: Vec<String> = s.bin.1.iter().map(|a| a.to_string()).collect();
        return Some(Launch { via: [s.bin.0.to_string()].into_iter().chain(args.iter().cloned()).collect::<Vec<_>>().join(" "), program, args, npx: false });
    }
    let pkg = s.npx?;
    Some(Launch { program: find("npx")?, args: vec!["-y".into(), pkg.into()], via: format!("npx {pkg}"), npx: true })
}

#[derive(Serialize)]
pub struct AgentOption {
    id: &'static str,
    name: &'static str,
    ready: bool,
    via: Option<String>,
    note: String,
}

// ------------------------------------------------------------------ connection

#[derive(Debug)]
struct RpcError {
    code: i64,
    message: String,
}

type Pending = Sender<Result<Value, RpcError>>;

/// Where agent events go: the window in the app, a channel in tests.
type Sink = Arc<dyn Fn(&str, Value) + Send + Sync>;

fn window_sink(app: &AppHandle) -> Sink {
    let app = app.clone();
    Arc::new(move |event, payload| {
        let _ = app.emit(event, payload);
    })
}

struct Conn {
    kind: &'static str,
    child: Mutex<Child>,
    stdin: Mutex<ChildStdin>,
    pending: Mutex<HashMap<u64, Pending>>,
    next: AtomicU64,
    /// Open permission requests from the agent: our handle -> (their JSON-RPC id, session id).
    perms: Mutex<HashMap<u64, (Value, String)>>,
    stderr: Mutex<VecDeque<String>>,
    alive: AtomicBool,
    init: Mutex<Value>,
}

impl Conn {
    fn send(&self, msg: &Value) -> Result<(), String> {
        let mut stdin = self.stdin.lock().unwrap();
        writeln!(stdin, "{msg}").and_then(|_| stdin.flush()).map_err(|e| format!("{} isn't running ({e})", self.kind))
    }

    fn request(&self, method: &str, params: Value, timeout: Option<Duration>) -> Result<Value, RpcError> {
        if !self.alive.load(Ordering::SeqCst) {
            return Err(RpcError { code: -1, message: self.exit_message() });
        }
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = channel();
        self.pending.lock().unwrap().insert(id, tx);
        if let Err(message) = self.send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})) {
            self.pending.lock().unwrap().remove(&id);
            return Err(RpcError { code: -1, message });
        }
        let reply = match timeout {
            Some(t) => rx.recv_timeout(t).map_err(|_| RpcError { code: -1, message: format!("{method} timed out") }),
            None => rx.recv().map_err(|_| RpcError { code: -1, message: self.exit_message() }),
        };
        self.pending.lock().unwrap().remove(&id);
        reply?
    }

    fn notify(&self, method: &str, params: Value) -> Result<(), String> {
        self.send(&json!({"jsonrpc": "2.0", "method": method, "params": params}))
    }

    /// The last few lines the agent wrote to stderr, which usually say why it stopped.
    fn stderr_tail(&self) -> Option<String> {
        let tail: Vec<String> = self.stderr.lock().unwrap().iter().rev().take(6).rev().cloned().collect();
        (!tail.is_empty()).then(|| tail.join("\n"))
    }

    fn exit_message(&self) -> String {
        match self.stderr_tail() {
            Some(tail) => format!("The agent stopped:\n{tail}"),
            None => "The agent stopped.".into(),
        }
    }

    fn kill(&self) {
        self.alive.store(false, Ordering::SeqCst);
        let mut child = self.child.lock().unwrap();
        let _ = child.kill();
        let _ = child.wait();
    }
}

fn spawn(sink: Sink, s: &'static Spec) -> Result<Arc<Conn>, String> {
    let l = launch(s).ok_or_else(|| missing_note(s))?;
    let mut cmd = Command::new(&l.program);
    cmd.args(&l.args)
        .env_clear()
        .envs(user_env().iter().cloned())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(home) = std::env::var_os("HOME") {
        cmd.current_dir(home);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let mut child = cmd.spawn().map_err(|e| format!("Couldn't start {} ({}): {e}", s.name, l.via))?;
    let (stdin, stdout, stderr) = (child.stdin.take().unwrap(), child.stdout.take().unwrap(), child.stderr.take().unwrap());
    let conn = Arc::new(Conn {
        kind: s.id,
        child: Mutex::new(child),
        stdin: Mutex::new(stdin),
        pending: Mutex::new(HashMap::new()),
        next: AtomicU64::new(1),
        perms: Mutex::new(HashMap::new()),
        stderr: Mutex::new(VecDeque::new()),
        alive: AtomicBool::new(true),
        init: Mutex::new(Value::Null),
    });

    let c = conn.clone();
    std::thread::spawn(move || {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            let mut tail = c.stderr.lock().unwrap();
            tail.push_back(line);
            if tail.len() > 40 {
                tail.pop_front();
            }
        }
    });

    let c = conn.clone();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if let Ok(msg) = serde_json::from_str::<Value>(&line) {
                on_message(&c, &sink, msg);
            }
        }
        c.alive.store(false, Ordering::SeqCst);
        let message = c.exit_message();
        for (_, tx) in c.pending.lock().unwrap().drain() {
            let _ = tx.send(Err(RpcError { code: -1, message: message.clone() }));
        }
        c.perms.lock().unwrap().clear();
        sink("agent-exit", json!({"kind": c.kind, "message": message}));
    });

    // npx downloads the adapter on first use, which can take a while.
    let timeout = Duration::from_secs(if l.npx { 300 } else { 60 });
    let params = json!({
        "protocolVersion": 1,
        "clientCapabilities": {"session": {"notices": {}}},
        "clientInfo": {"name": "fabready", "title": "fabready", "version": fabready_core::report::VERSION},
    });
    match conn.request("initialize", params, Some(timeout)) {
        Ok(init) => {
            *conn.init.lock().unwrap() = init;
            Ok(conn)
        }
        Err(e) => {
            conn.kill();
            let tail = conn.stderr_tail().map(|t| format!("\n{t}")).unwrap_or_default();
            Err(format!("{} didn't start: {}{tail}", s.name, e.message))
        }
    }
}

fn on_message(conn: &Arc<Conn>, sink: &Sink, msg: Value) {
    let method = msg.get("method").and_then(Value::as_str);
    match (method, msg.get("id")) {
        (Some("session/request_permission"), Some(id)) => {
            let handle = conn.next.fetch_add(1, Ordering::Relaxed);
            let params = &msg["params"];
            let session = params["sessionId"].as_str().unwrap_or_default().to_string();
            conn.perms.lock().unwrap().insert(handle, (id.clone(), session.clone()));
            sink(
                "agent-permission",
                json!({"kind": conn.kind, "handle": handle, "sessionId": session, "toolCall": params["toolCall"], "options": params["options"]}),
            );
        }
        (Some(other), Some(id)) => {
            // We advertise no client file system or terminal; the agents use their own tools for those.
            let _ = conn.send(&json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32601, "message": format!("Method not supported: {other}")}}));
        }
        (Some("session/update"), None) => {
            let p = &msg["params"];
            sink("agent-update", json!({"kind": conn.kind, "sessionId": p["sessionId"], "update": p["update"]}));
        }
        (Some(_), None) => {}
        (None, Some(id)) => {
            let Some(tx) = id.as_u64().and_then(|id| conn.pending.lock().unwrap().remove(&id)) else { return };
            let reply = match msg.get("error") {
                Some(e) => Err(RpcError {
                    code: e["code"].as_i64().unwrap_or(0),
                    message: e["message"].as_str().unwrap_or("agent error").to_string()
                        + &e.get("data").and_then(|d| d.as_str().or_else(|| d["message"].as_str())).map(|d| format!(": {d}")).unwrap_or_default(),
                }),
                None => Ok(msg.get("result").cloned().unwrap_or(Value::Null)),
            };
            let _ = tx.send(reply);
        }
        (None, None) => {}
    }
}

fn missing_note(s: &Spec) -> String {
    match s.npx {
        Some(pkg) => format!("{} needs Node.js for its ACP adapter. Install Node.js, or run `npm install -g {pkg}`.", s.name),
        None => format!("{} isn't installed. Get it from https://opencode.ai.", s.name),
    }
}

// ------------------------------------------------------------------ state + commands

#[derive(Default)]
pub struct Agents {
    conns: Mutex<HashMap<&'static str, Arc<Conn>>>,
    /// Serializes starting each agent, so two quick sends don't start the same one twice.
    starting: Mutex<HashMap<&'static str, Arc<Mutex<()>>>>,
}

impl Agents {
    fn get(&self, kind: &str) -> Result<Arc<Conn>, String> {
        self.conns.lock().unwrap().get(kind).filter(|c| c.alive.load(Ordering::SeqCst)).cloned().ok_or_else(|| "The agent isn't running.".to_string())
    }

    fn ensure(&self, sink: Sink, kind: &str) -> Result<Arc<Conn>, String> {
        let s = spec(kind)?;
        let lock = self.starting.lock().unwrap().entry(s.id).or_default().clone();
        let _guard = lock.lock().unwrap();
        if let Ok(c) = self.get(kind) {
            return Ok(c);
        }
        let conn = spawn(sink, s)?;
        self.conns.lock().unwrap().insert(s.id, conn.clone());
        Ok(conn)
    }

    pub fn stop_all(&self) {
        for (_, c) in self.conns.lock().unwrap().drain() {
            c.kill();
        }
    }
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T, String> + Send + 'static) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| e.to_string())?
}

/// The agents the chat can use, and whether each can start on this machine.
#[tauri::command]
pub async fn agent_list() -> Result<Vec<AgentOption>, String> {
    blocking(|| {
        Ok(SPECS
            .iter()
            .map(|s| {
                let l = launch(s);
                let note = match &l {
                    Some(l) if l.npx => format!("Runs the official ACP adapter with npx (downloaded the first time). {}", login_source(s)),
                    Some(_) => login_source(s).to_string(),
                    None => missing_note(s),
                };
                AgentOption { id: s.id, name: s.name, ready: l.is_some(), via: l.map(|l| l.via), note }
            })
            .collect())
    })
    .await
}

fn login_source(s: &Spec) -> &'static str {
    match s.id {
        "claude" => "Uses your Claude Code sign-in.",
        "codex" => "Uses your Codex sign-in.",
        _ => "Uses your OpenCode providers.",
    }
}

/// Start (or resume) the chat session for a project. Returns the session, or `{auth}` when the agent wants
/// the user to sign in first.
#[tauri::command]
pub async fn agent_session(app: AppHandle, kind: String, path: String, resume: Option<String>) -> Result<Value, String> {
    blocking(move || {
        let agents = app.state::<Agents>();
        let conn = agents.ensure(window_sink(&app), &kind)?;
        let (pcb, _) = fabready_core::report::resolve(Path::new(&path)).map_err(|e| format!("{e:#}"))?;
        let cwd = pcb.parent().ok_or("board has no parent folder")?.display().to_string();
        let bridge = app.state::<Arc<Bridge>>();
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let env = [(APP_ADDR_ENV, bridge.addr.as_str()), (APP_TOKEN_ENV, bridge.token.as_str()), (APP_PROJECT_ENV, path.as_str()), ("FABREADY_NO_AGENT_RECORD", "1")];
        let mcp = json!([{
            "name": "fabready",
            "command": exe.display().to_string(),
            "args": ["mcp"],
            "env": env.iter().map(|(name, value)| json!({"name": name, "value": value})).collect::<Vec<_>>(),
        }]);
        let init = conn.init.lock().unwrap().clone();
        let info = json!({"agent": init["agentInfo"]});

        let can_resume = !init["agentCapabilities"]["sessionCapabilities"]["resume"].is_null();
        if let (Some(id), true) = (resume, can_resume) {
            if let Ok(r) = conn.request("session/resume", json!({"sessionId": id, "cwd": cwd, "mcpServers": mcp}), Some(Duration::from_secs(120))) {
                return Ok(merge(info, r, json!({"sessionId": id, "resumed": true})));
            }
        }
        match conn.request("session/new", json!({"cwd": cwd, "mcpServers": mcp}), Some(Duration::from_secs(120))) {
            Ok(r) => Ok(merge(info, r, json!({}))),
            Err(e) if e.code == AUTH_REQUIRED => {
                // Only methods the agent runs itself; we don't offer terminal sign-in.
                let methods: Vec<Value> = init["authMethods"]
                    .as_array()
                    .map(|a| a.iter().filter(|m| m["type"].as_str().is_none_or(|t| t == "agent")).cloned().collect())
                    .unwrap_or_default();
                Ok(json!({"auth": {"message": e.message, "methods": methods, "hint": spec(&kind)?.login}}))
            }
            Err(e) => Err(e.message),
        }
    })
    .await
}

fn merge(mut a: Value, b: Value, c: Value) -> Value {
    for extra in [b, c] {
        if let (Some(a), Value::Object(extra)) = (a.as_object_mut(), extra) {
            a.extend(extra);
        }
    }
    a
}

#[tauri::command]
pub async fn agent_authenticate(app: AppHandle, kind: String, method: String) -> Result<(), String> {
    blocking(move || {
        let conn = app.state::<Agents>().get(&kind)?;
        conn.request("authenticate", json!({"methodId": method}), Some(Duration::from_secs(600))).map(|_| ()).map_err(|e| e.message)
    })
    .await
}

/// Send a message and wait for the turn to end. Progress arrives as `agent-update` events. Returns the stop reason.
#[tauri::command]
pub async fn agent_prompt(app: AppHandle, kind: String, session: String, prompt: Vec<Value>) -> Result<String, String> {
    blocking(move || {
        let conn = app.state::<Agents>().get(&kind)?;
        let r = conn.request("session/prompt", json!({"sessionId": session, "prompt": prompt}), None).map_err(|e| e.message)?;
        Ok(r["stopReason"].as_str().unwrap_or("end_turn").to_string())
    })
    .await
}

#[tauri::command]
pub fn agent_cancel(agents: tauri::State<'_, Agents>, kind: String, session: String) -> Result<(), String> {
    let conn = agents.get(&kind)?;
    let open: Vec<Value> = {
        let mut perms = conn.perms.lock().unwrap();
        let handles: Vec<u64> = perms.iter().filter(|(_, (_, s))| *s == session).map(|(h, _)| *h).collect();
        handles.iter().filter_map(|h| perms.remove(h)).map(|(id, _)| id).collect()
    };
    for id in open {
        conn.send(&json!({"jsonrpc": "2.0", "id": id, "result": {"outcome": {"outcome": "cancelled"}}}))?;
    }
    conn.notify("session/cancel", json!({"sessionId": session}))
}

/// Answer a permission request with one of its options, or `None` to decline.
#[tauri::command]
pub fn agent_permission(agents: tauri::State<'_, Agents>, kind: String, handle: u64, option: Option<String>) -> Result<(), String> {
    let conn = agents.get(&kind)?;
    let (id, _) = conn.perms.lock().unwrap().remove(&handle).ok_or("That request is no longer open.")?;
    let outcome = match option {
        Some(o) => json!({"outcome": "selected", "optionId": o}),
        None => json!({"outcome": "cancelled"}),
    };
    conn.send(&json!({"jsonrpc": "2.0", "id": id, "result": {"outcome": outcome}}))
}

/// Change a session config option (model, mode, effort…). Returns the full list of options.
#[tauri::command]
pub async fn agent_set_config(app: AppHandle, kind: String, session: String, config: String, value: Value) -> Result<Value, String> {
    blocking(move || {
        let conn = app.state::<Agents>().get(&kind)?;
        let r = conn
            .request("session/set_config_option", json!({"sessionId": session, "configId": config, "value": value}), Some(Duration::from_secs(30)))
            .map_err(|e| e.message)?;
        Ok(r["configOptions"].clone())
    })
    .await
}

/// Older agents expose modes instead of config options.
#[tauri::command]
pub async fn agent_set_mode(app: AppHandle, kind: String, session: String, mode: String) -> Result<(), String> {
    blocking(move || {
        let conn = app.state::<Agents>().get(&kind)?;
        conn.request("session/set_mode", json!({"sessionId": session, "modeId": mode}), Some(Duration::from_secs(30))).map(|_| ()).map_err(|e| e.message)
    })
    .await
}

#[tauri::command]
pub fn agent_stop(agents: tauri::State<'_, Agents>, kind: String) {
    if let Some(c) = agents.conns.lock().unwrap().remove(kind.as_str()) {
        c.kill();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Talks to a real `opencode acp` when it's installed. Sending a prompt spends the user's tokens, so that part
    /// only runs with FABREADY_TEST_PROMPT=1.
    #[test]
    fn opencode_over_acp() {
        if find("opencode").is_none() {
            eprintln!("opencode isn't installed; skipping");
            return;
        }
        let (tx, rx) = channel::<(String, Value)>();
        let sink: Sink = Arc::new(move |event, payload| {
            let _ = tx.send((event.to_string(), payload));
        });
        let conn = spawn(sink, spec("opencode").unwrap()).expect("opencode starts");
        assert_eq!(conn.init.lock().unwrap()["protocolVersion"], 1);

        let cwd = concat!(env!("CARGO_MANIFEST_DIR"), "/../../samples/macropad-buggy");
        let session = conn.request("session/new", json!({"cwd": cwd, "mcpServers": []}), Some(Duration::from_secs(60))).expect("session");
        let id = session["sessionId"].as_str().expect("session id").to_string();

        if std::env::var_os("FABREADY_TEST_PROMPT").is_some() {
            let prompt = json!({"sessionId": id, "prompt": [{"type": "text", "text": "Reply with exactly the word pong and nothing else."}]});
            let r = conn.request("session/prompt", prompt, Some(Duration::from_secs(180))).expect("prompt");
            assert_eq!(r["stopReason"], "end_turn");
            let text: String = rx
                .try_iter()
                .filter(|(e, v)| e == "agent-update" && v["update"]["sessionUpdate"] == "agent_message_chunk")
                .map(|(_, v)| v["update"]["content"]["text"].as_str().unwrap_or_default().to_string())
                .collect();
            assert!(text.to_lowercase().contains("pong"), "{text:?}");
        }
        conn.kill();
        assert!(conn.request("session/prompt", json!({}), None).is_err());
    }
}
