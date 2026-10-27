//! `pcbcheck.json` project configuration and fab capability profiles.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub profile: Option<String>,
    /// Overrides for individual fab rules, e.g. `{"min_track": 0.15}`.
    pub fab: BTreeMap<String, f64>,
    pub keyboard: KeyboardConfig,
    pub power: PowerConfig,
    /// `id`, `id:REF`, or `prefix.*` patterns to waive.
    pub ignore: Vec<String>,
}

/// Board context for part checks. Rails are otherwise guessed from net names (+3V3, VBUS, BAT+...).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PowerConfig {
    /// Net name -> volts, e.g. `{"VCC": 3.3, "VIN": 12}`.
    pub rails: BTreeMap<String, f64>,
    /// Net name -> expected load in mA, used to size regulators, e.g. `{"+3V3": 50}`.
    pub current_ma: BTreeMap<String, f64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct KeyboardConfig {
    pub switch_regex: Option<String>,
    pub controller_regex: Option<String>,
    pub key_pitch: Option<f64>,
    pub key_pitch_x: Option<f64>,
    pub key_pitch_y: Option<f64>,
}

impl Config {
    /// Load `explicit` if given, otherwise `pcbcheck.json` next to the board (if present).
    pub fn load(board_dir: &Path, explicit: Option<&Path>) -> Result<Config> {
        let path = match explicit {
            Some(p) => p.to_path_buf(),
            None => board_dir.join("pcbcheck.json"),
        };
        if !path.exists() {
            if explicit.is_some() {
                anyhow::bail!("config {} not found", path.display());
            }
            return Ok(Config::default());
        }
        let text = std::fs::read_to_string(&path)?;
        serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))
    }
}

pub type Rules = BTreeMap<String, f64>;

pub const PROFILE_NAMES: &[&str] = &["jlcpcb", "safe"];

/// Conservative "standard price" limits for 2-layer boards. Verify against the fab's current capability page.
pub fn profile(name: &str) -> Option<Rules> {
    let values: &[(&str, f64)] = match name {
        "jlcpcb" => &[
            ("min_track", 0.127),
            ("min_via_drill", 0.3),
            ("min_annular_ring", 0.13),
            ("min_pth_drill", 0.3),
            ("min_npth_drill", 0.5),
            ("min_edge_clearance", 0.3),
            ("cheap_size", 100.0),
        ],
        "safe" => &[
            ("min_track", 0.2),
            ("min_via_drill", 0.4),
            ("min_annular_ring", 0.15),
            ("min_pth_drill", 0.4),
            ("min_npth_drill", 0.8),
            ("min_edge_clearance", 0.5),
            ("cheap_size", 100.0),
        ],
        _ => return None,
    };
    Some(values.iter().map(|(k, v)| (k.to_string(), *v)).collect())
}
