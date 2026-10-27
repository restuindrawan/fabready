//! pcbcheck: pre-order checks for KiCad PCBs, with extra checks for keyboards.
//!
//! The same engine powers the `pcbcheck` CLI, its MCP server, and the desktop app.

pub mod board;
pub mod config;
pub mod connectivity;
pub mod fab;
pub mod findings;
pub mod fixes;
pub mod keyboard;
pub mod kicad;
#[cfg(feature = "mcp")]
pub mod mcp;
pub mod parts;
pub mod render;
pub mod report;
pub mod sexpr;

pub use findings::{Finding, Severity, CATALOG};
pub use report::{run, write_outputs, CheckResult, Options, Report};
