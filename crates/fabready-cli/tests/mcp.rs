//! Drive the MCP server end-to-end over stdio, like an agent client would.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

use serde_json::{json, Value};

#[test]
fn mcp_initialize_list_and_call() {
    let board = concat!(env!("CARGO_MANIFEST_DIR"), "/../../samples/macropad-buggy");
    let mut child = Command::new(env!("CARGO_BIN_EXE_fabready"))
        .arg("mcp")
        .env("FABREADY_NO_AGENT_RECORD", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut out = BufReader::new(child.stdout.take().unwrap());
    let mut send = |msg: Value| {
        writeln!(stdin, "{msg}").unwrap();
        stdin.flush().unwrap();
    };
    let mut recv = || {
        let mut line = String::new();
        out.read_line(&mut line).unwrap();
        serde_json::from_str::<Value>(&line).unwrap()
    };

    send(json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"test","version":"1"}}}));
    let init = recv();
    assert_eq!(init["result"]["protocolVersion"], "2025-11-25");
    assert_eq!(init["result"]["serverInfo"]["name"], "fabready");

    send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}));
    send(json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}));
    let names: Vec<String> =
        recv()["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap().to_string()).collect();
    assert_eq!(names, ["check_board", "board_image", "get_matrix", "explain_check", "review_parts", "list_fixes", "apply_fix", "undo_fix"]);

    send(json!({"jsonrpc":"2.0","id":10,"method":"tools/call","params":{"name":"list_fixes","arguments":{"path":board}}}));
    let fixes = recv();
    let ids: Vec<String> = fixes["result"]["structuredContent"]["fixes"].as_array().unwrap().iter().map(|f| f["id"].as_str().unwrap().to_string()).collect();
    assert_eq!(ids.len(), 3, "{ids:?}");

    send(json!({"jsonrpc":"2.0","id":11,"method":"tools/call","params":{"name":"apply_fix","arguments":{"path":board,"fixes":["all"],"dry_run":true}}}));
    let dry = recv();
    let s = &dry["result"]["structuredContent"];
    assert_eq!(s["dry_run"], true);
    assert!(s["after"]["error"].as_u64() < s["before"]["error"].as_u64());

    send(json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"get_matrix","arguments":{"path":board}}}));
    let matrix = recv();
    assert_eq!(matrix["result"]["structuredContent"]["diode_direction"], "col2row");

    send(json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"board_image","arguments":{"path":board,"width":600}}}));
    let image = recv();
    assert_eq!(image["result"]["content"][0]["type"], "image");
    assert!(image["result"]["content"][1]["text"].as_str().unwrap().contains("D5"));

    send(json!({"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"get_matrix","arguments":{"path":"/nope"}}}));
    assert_eq!(recv()["result"]["isError"], true);

    send(json!({"jsonrpc":"2.0","id":6,"method":"bogus"}));
    assert_eq!(recv()["error"]["code"], -32601);

    drop(stdin);
    assert!(child.wait().unwrap().success());
}

#[test]
fn app_tools_relay_to_the_app_window() {
    // Stand-in for the desktop app's bridge: one JSON line in, one JSON line out.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let app = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut line = String::new();
        BufReader::new(&stream).read_line(&mut line).unwrap();
        let req: Value = serde_json::from_str(&line).unwrap();
        writeln!(&stream, "{}", json!({"result": {"shown": req["params"]["refs"]}})).unwrap();
        req
    });

    let mut child = Command::new(env!("CARGO_BIN_EXE_fabready"))
        .arg("mcp")
        .env("FABREADY_NO_AGENT_RECORD", "1")
        .env("FABREADY_APP_ADDR", &addr)
        .env("FABREADY_APP_TOKEN", "secret")
        .env("FABREADY_APP_PROJECT", "/boards/macropad")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut out = BufReader::new(child.stdout.take().unwrap());
    let mut call = |msg: Value| {
        writeln!(stdin, "{msg}").unwrap();
        stdin.flush().unwrap();
        let mut line = String::new();
        out.read_line(&mut line).unwrap();
        serde_json::from_str::<Value>(&line).unwrap()
    };

    let init = call(json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"test","version":"1"}}}));
    assert!(init["result"]["instructions"].as_str().unwrap().contains("show_on_board"));
    let tools = call(json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}));
    let names: Vec<&str> = tools["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert!(names.ends_with(&["app_state", "show_on_board", "propose_fix"]), "{names:?}");

    let shown = call(json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"show_on_board","arguments":{"refs":["SW5"]}}}));
    assert_eq!(shown["result"]["structuredContent"]["shown"], json!(["SW5"]));
    let req = app.join().unwrap();
    assert_eq!(req["token"], "secret");
    assert_eq!(req["method"], "show_on_board");
    assert_eq!(req["project"], "/boards/macropad");

    // Nobody listening any more: the tool reports the error to the model instead of failing the call.
    let gone = call(json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"app_state","arguments":{}}}));
    assert_eq!(gone["result"]["isError"], true);

    drop(stdin);
    assert!(child.wait().unwrap().success());
}
