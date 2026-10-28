// Prevents an extra console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // `fabready-app mcp` runs the agent (MCP) server over stdio instead of opening the window,
    // so one installed app serves both people and agents.
    if std::env::args().nth(1).as_deref() == Some("mcp") {
        if let Err(e) = fabready_core::mcp::serve() {
            eprintln!("error: {e:#}");
            std::process::exit(2);
        }
        return;
    }
    fabready_app_lib::run();
}
