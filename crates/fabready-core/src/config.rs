//! `fabready.json` project configuration and fab capability profiles.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const FILE_NAME: &str = "fabready.json";

/// Top-level keys fabready reads; anything else in the file is left alone when saving.
const KNOWN_KEYS: &[&str] = &["profile", "fab", "keyboard", "power", "ignore"];

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
    /// Load `explicit` if given, otherwise `fabready.json` next to the board (if present).
    pub fn load(board_dir: &Path, explicit: Option<&Path>) -> Result<Config> {
        let path = match explicit {
            Some(p) => p.to_path_buf(),
            None => board_dir.join(FILE_NAME),
        };
        if !path.exists() {
            if explicit.is_some() {
                anyhow::bail!("config {} not found", path.display());
            }
            return Ok(Config::default());
        }
        let cfg = Config::read(&path)?;
        cfg.validate().with_context(|| format!("in {}", path.display()))?;
        Ok(cfg)
    }

    /// Parse a config file without validating its values.
    pub fn read(path: &Path) -> Result<Config> {
        let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))
    }

    /// Reject values the checks can't use (unknown profile, bad numbers, invalid patterns).
    pub fn validate(&self) -> Result<()> {
        if let Some(p) = &self.profile {
            if profile(p).is_none() {
                bail!("unknown profile '{p}' (choose from {})", PROFILE_NAMES.join(", "));
            }
        }
        for (k, v) in &self.fab {
            if !v.is_finite() || *v <= 0.0 {
                bail!("fab.{k} must be a positive number of mm");
            }
        }
        for (net, v) in &self.power.rails {
            if !v.is_finite() {
                bail!("power.rails.{net} must be a number of volts");
            }
        }
        for (net, v) in &self.power.current_ma {
            if !v.is_finite() || *v < 0.0 {
                bail!("power.current_ma.{net} must be a number of mA, 0 or more");
            }
        }
        let kb = &self.keyboard;
        for (name, v) in [("key_pitch", kb.key_pitch), ("key_pitch_x", kb.key_pitch_x), ("key_pitch_y", kb.key_pitch_y)] {
            if v.is_some_and(|v| !v.is_finite() || v <= 0.0) {
                bail!("keyboard.{name} must be a positive number of mm");
            }
        }
        for (name, pattern) in [("switch_regex", &kb.switch_regex), ("controller_regex", &kb.controller_regex)] {
            if let Some(p) = pattern {
                regex::Regex::new(p).map_err(|e| anyhow::anyhow!("keyboard.{name} is not a valid pattern: {e}"))?;
            }
        }
        if self.ignore.iter().any(|r| r.trim().is_empty()) {
            bail!("ignore entries can't be empty");
        }
        Ok(())
    }

    /// Write to `path`, keeping keys fabready doesn't read and leaving out empty sections.
    pub fn save(&self, path: &Path) -> Result<()> {
        self.validate()?;
        let mut doc = match std::fs::read_to_string(path) {
            Ok(text) => match serde_json::from_str::<Value>(&text).with_context(|| format!("parsing {}", path.display()))? {
                Value::Object(map) => map,
                _ => bail!("{} is not a JSON object", path.display()),
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Default::default(),
            Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
        };
        let Value::Object(mine) = prune(serde_json::to_value(self)?) else { unreachable!("Config serializes to an object") };
        for key in KNOWN_KEYS {
            doc.remove(*key);
        }
        doc.extend(mine);
        let text = serde_json::to_string_pretty(&Value::Object(doc))? + "\n";
        std::fs::write(path, text).with_context(|| format!("writing {}", path.display()))
    }
}

/// `fabready.json` for a board file.
pub fn path_for(pcb: &Path) -> PathBuf {
    pcb.parent().unwrap_or(Path::new(".")).join(FILE_NAME)
}

/// Drop nulls and empty objects/arrays so saved files only hold what was set.
fn prune(v: Value) -> Value {
    match v {
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(k, v)| (k, prune(v)))
                .filter(|(_, v)| !(v.is_null() || v.as_object().is_some_and(|o| o.is_empty()) || v.as_array().is_some_and(|a| a.is_empty())))
                .collect(),
        ),
        other => other,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_keeps_unknown_keys_and_drops_empty_sections() {
        let dir = std::env::temp_dir().join(format!("fabready-config-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(FILE_NAME);
        std::fs::write(&path, r#"{"note": "keep me", "fab": {"min_track": 0.2}, "ignore": ["a"]}"#).unwrap();

        let mut cfg = Config::load(&dir, None).unwrap();
        cfg.fab.clear();
        cfg.profile = Some("safe".into());
        cfg.power.rails.insert("VCC".into(), 3.3);
        cfg.save(&path).unwrap();

        let saved: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(saved, serde_json::json!({"note": "keep me", "profile": "safe", "power": {"rails": {"VCC": 3.3}}, "ignore": ["a"]}));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn rejects_values_the_checks_cannot_use() {
        let bad = |f: fn(&mut Config)| {
            let mut c = Config::default();
            f(&mut c);
            c.validate().is_err()
        };
        assert!(bad(|c| c.profile = Some("cheapfab".into())));
        assert!(bad(|c| {
            c.fab.insert("min_track".into(), 0.0);
        }));
        assert!(bad(|c| c.keyboard.switch_regex = Some("(unclosed".into())));
        assert!(bad(|c| c.keyboard.key_pitch = Some(-1.0)));
        assert!(bad(|c| c.ignore.push(" ".into())));
        assert!(Config::default().validate().is_ok());
    }
}
