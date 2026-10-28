//! Localhost socket for the app tools (app_state, show_on_board, propose_fix) of the MCP servers this app
//! starts for its agent chat. Each request is forwarded to the window as an `app-request` event, and the
//! window answers through the `app_reply` command.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};

type Reply = Result<Value, String>;

pub struct Bridge {
    pub addr: String,
    pub token: String,
    pending: Mutex<HashMap<u64, Sender<Reply>>>,
    next: AtomicU64,
}

/// 128 random bits from the standard library's randomly seeded hasher.
fn random_token() -> String {
    use std::hash::{BuildHasher, Hasher};
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    (0..2)
        .map(|i| {
            let mut h = std::collections::hash_map::RandomState::new().build_hasher();
            h.write_u128(nanos ^ i);
            h.write_u32(std::process::id());
            format!("{:016x}", h.finish())
        })
        .collect()
}

pub fn start(app: AppHandle) -> std::io::Result<Arc<Bridge>> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let bridge = Arc::new(Bridge {
        addr: listener.local_addr()?.to_string(),
        token: random_token(),
        pending: Mutex::new(HashMap::new()),
        next: AtomicU64::new(1),
    });
    let b = bridge.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let (b, app) = (b.clone(), app.clone());
            std::thread::spawn(move || {
                let _ = serve(&b, &app, stream);
            });
        }
    });
    Ok(bridge)
}

fn serve(bridge: &Bridge, app: &AppHandle, stream: TcpStream) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let req: Value = serde_json::from_str(&line).unwrap_or(Value::Null);
    let reply = if req.get("token").and_then(Value::as_str) != Some(bridge.token.as_str()) {
        Err("not authorized".to_string())
    } else {
        bridge.ask(app, &req)
    };
    let out = match reply {
        Ok(result) => json!({ "result": result }),
        Err(error) => json!({ "error": error }),
    };
    writeln!(&stream, "{out}")
}

impl Bridge {
    fn ask(&self, app: &AppHandle, req: &Value) -> Reply {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = channel();
        self.pending.lock().unwrap().insert(id, tx);
        let event = json!({"id": id, "method": req["method"], "params": req["params"], "project": req["project"]});
        let reply = match app.emit("app-request", event) {
            Ok(()) => rx.recv_timeout(Duration::from_secs(30)).unwrap_or_else(|_| Err("the fabready window didn't answer".into())),
            Err(e) => Err(e.to_string()),
        };
        self.pending.lock().unwrap().remove(&id);
        reply
    }

    pub fn reply(&self, id: u64, reply: Reply) {
        if let Some(tx) = self.pending.lock().unwrap().remove(&id) {
            let _ = tx.send(reply);
        }
    }
}
