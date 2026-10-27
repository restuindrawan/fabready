//! Keyboard-specific checks: switch/diode matrix, controller pins, spacing and wireless basics.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use regex::{Regex, RegexBuilder};
use serde::{Deserialize, Serialize};

use crate::board::{point_in_polygon, Board, Footprint, Pad};
use crate::config::KeyboardConfig;
use crate::findings::{Finding, Severity};

pub const SWITCH_RE: &str =
    r"(cherry|\bmx|_mx|mx_|choc|kailh|gateron|alps|hotswap|pg1350|pg1316|pg1232|keyswitch|switch_keyboard|keyboard)";
const TACT_RE: &str = r"(push|tact|spst|reset|tl33|evqp|skqg)";
pub const CONTROLLER_RE: &str = r"(nice.?nano|xiao|seeed|nrf52|nrf5|pro.?micro|promicro|e73|holyiot|mdbt|rp2040|\bpico|elite.?c|blackpill|esp32|supermini|puchi|bluemicro|kb2040|mcu_module)";
const WIRELESS_RE: &str = r"(nice.?nano|nrf5|e73|holyiot|mdbt|esp32|supermini|puchi|bluemicro|xiao.?ble|xiao.?nrf|ble)";
const POWER_PIN_RE: &str = r"^(gnd|vss|vcc|vdd|3v3|3\.3v|5v|vbus|vin|raw|bat\+?|b\+|batt|vbat|rst|reset|nrst|nc|ext_?vcc|aref)$";
const BATTERY_NET_RE: &str = r"(^|[^a-z])(bat|batt|vbat|b\+|bat\+|raw)([^a-z]|$)";
const POWER_SWITCH_RE: &str = r"(slide|spdt|msk12|ss12|pcm12|js102|power.?switch)";
const CHOC_RE: &str = r"(choc|pg1350|pg1232)";

fn re(pattern: &str) -> Regex {
    RegexBuilder::new(pattern).case_insensitive(true).build().expect("valid built-in regex")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Key {
    #[serde(rename = "ref")]
    pub reference: String,
    pub x: f64,
    pub y: f64,
    pub diode: Option<String>,
    pub switch_line: Option<String>,
    pub diode_line: Option<String>,
    pub cathode_out: Option<bool>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Matrix {
    pub keys: Vec<Key>,
    pub switch_side_lines: Vec<String>,
    pub diode_side_lines: Vec<String>,
    /// `col2row` / `row2col` as ZMK and QMK name it.
    pub diode_direction: Option<String>,
    pub controller: Option<String>,
    /// Matrix line net -> "U1 pad 5 (D1)".
    pub pins: BTreeMap<String, String>,
    pub gpio_available: Option<usize>,
}

/// Sort key that orders "ROW2" before "ROW10".
pub fn natural_key(s: &str) -> Vec<(String, u64)> {
    let mut out = Vec::new();
    let mut text = String::new();
    let mut digits = String::new();
    for ch in s.chars() {
        if ch.is_ascii_digit() {
            digits.push(ch);
        } else {
            if !digits.is_empty() {
                out.push((std::mem::take(&mut text), digits.parse().unwrap_or(0)));
                digits.clear();
            }
            text.push(ch);
        }
    }
    out.push((text, digits.parse().unwrap_or(0)));
    out
}

fn sorted_natural(set: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut v: Vec<String> = set.into_iter().collect::<BTreeSet<_>>().into_iter().collect();
    v.sort_by_key(|s| natural_key(s));
    v
}

fn electrical_numbers(fp: &Footprint) -> usize {
    fp.pads.iter().filter(|p| p.electrical()).map(|p| &p.number).collect::<BTreeSet<_>>().len()
}

fn cathode(fp: &Footprint) -> String {
    for p in &fp.pads {
        let f = p.pinfunction.to_uppercase();
        if f == "K" || f == "CATHODE" || p.number.eq_ignore_ascii_case("K") {
            return p.number.clone();
        }
    }
    "1".into() // KiCad convention for Diode library footprints
}

fn pin_label(p: &Pad) -> String {
    if p.pinfunction.is_empty() {
        format!("{} pad {}", p.fp_ref, p.number)
    } else {
        format!("{} pad {} ({})", p.fp_ref, p.number, p.pinfunction)
    }
}

fn all_contain(lines: &[String], kind: &str) -> bool {
    !lines.is_empty() && lines.iter().all(|l| l.to_uppercase().contains(kind))
}

pub fn check(board: &Board, cfg: &KeyboardConfig) -> (Vec<Finding>, Option<Matrix>) {
    let mut findings = Vec::new();
    let switch_re = re(cfg.switch_regex.as_deref().unwrap_or(SWITCH_RE));
    let controller_re = re(cfg.controller_regex.as_deref().unwrap_or(CONTROLLER_RE));
    let tact_re = re(TACT_RE);
    let power_pin_re = re(POWER_PIN_RE);

    let switches: Vec<&Footprint> = board
        .footprints
        .iter()
        .filter(|f| switch_re.is_match(&f.lib_id) && !tact_re.is_match(&f.lib_id) && electrical_numbers(f) >= 2)
        .collect();
    if switches.is_empty() {
        // Not a keyboard: keyboard-specific checks don't apply, so stay quiet.
        return (Vec::new(), None);
    }
    let diode_ref = Regex::new(r"^D\d").unwrap();
    let diodes: BTreeMap<&str, &Footprint> = board
        .footprints
        .iter()
        .filter(|f| diode_ref.is_match(&f.reference) && !f.text().to_lowercase().contains("led") && electrical_numbers(f) == 2)
        .map(|f| (f.reference.as_str(), f))
        .collect();
    let controller = board.footprints.iter().find(|f| controller_re.is_match(&f.text()));

    let net_pads = board.net_pads();
    let switch_refs: BTreeSet<&str> = switches.iter().map(|s| s.reference.as_str()).collect();
    let mut matrix = Matrix { controller: controller.map(|c| c.reference.clone()), ..Default::default() };

    let mut sorted_switches = switches.clone();
    sorted_switches.sort_by_key(|f| natural_key(&f.reference));
    for sw in sorted_switches {
        let nets: Vec<String> =
            sw.pads.iter().filter(|p| p.electrical() && !p.net.is_empty()).map(|p| p.net.clone()).collect::<BTreeSet<_>>().into_iter().collect();
        let mut key = Key {
            reference: sw.reference.clone(),
            x: (sw.x * 1000.0).round() / 1000.0,
            y: (sw.y * 1000.0).round() / 1000.0,
            diode: None,
            switch_line: None,
            diode_line: None,
            cathode_out: None,
        };
        if nets.len() < 2 {
            findings.push(
                Finding::new("kb.switch-unconnected", Severity::Error, format!("{} has {} connected net(s); a key switch needs 2.", sw.reference, nets.len()))
                    .refs([sw.reference.clone()])
                    .at((sw.x, sw.y)),
            );
            matrix.keys.push(key);
            continue;
        }
        // The "junction" net is private to this switch and leads to exactly one diode.
        let mut junction: Option<&String> = None;
        for n in &nets {
            let others: BTreeSet<&str> =
                net_pads.get(n).into_iter().flatten().map(|p| p.fp_ref.as_str()).filter(|r| *r != sw.reference).collect();
            if others.iter().any(|r| switch_refs.contains(r)) {
                continue;
            }
            let ds: Vec<&str> = others.iter().copied().filter(|r| diodes.contains_key(r)).collect();
            if ds.len() == 1 && others.len() == 1 {
                junction = Some(n);
                key.diode = Some(ds[0].to_string());
                break;
            }
            if let Some(c) = controller {
                if others.len() == 1 && others.contains(c.reference.as_str()) && diodes.is_empty() {
                    junction = Some(n); // direct-pin wiring, no matrix
                    break;
                }
            }
        }
        let Some(junction) = junction else {
            findings.push(
                Finding::new("kb.missing-diode", Severity::Error, format!("{} connects straight to {} with no diode in between.", sw.reference, nets.join(" and ")))
                    .refs([sw.reference.clone()])
                    .at((sw.x, sw.y)),
            );
            matrix.keys.push(key);
            continue;
        };
        key.switch_line = nets.iter().find(|n| *n != junction).cloned();
        if let Some(d_ref) = key.diode.clone() {
            let d = diodes[d_ref.as_str()];
            let k_num = cathode(d);
            match d.pads.iter().find(|p| p.electrical() && !p.net.is_empty() && &p.net != junction) {
                None => findings.push(
                    Finding::new("kb.switch-unconnected", Severity::Error, format!("Diode {} for {} has nothing on its other side.", d.reference, sw.reference))
                        .refs([d.reference.clone(), sw.reference.clone()])
                        .at((d.x, d.y)),
                ),
                Some(out) => {
                    key.diode_line = Some(out.net.clone());
                    key.cathode_out = Some(out.number == k_num);
                }
            }
        }
        matrix.keys.push(key);
    }

    let wired: Vec<&Key> = matrix.keys.iter().filter(|k| k.diode.is_some() && k.diode_line.is_some() && k.switch_line.is_some()).collect();
    if !wired.is_empty() {
        let cathode_out = wired.iter().filter(|k| k.cathode_out == Some(true)).count();
        let majority = cathode_out * 2 >= wired.len();
        let odd: Vec<&&Key> = wired.iter().filter(|k| k.cathode_out != Some(majority)).collect();
        if !odd.is_empty() && odd.len() < wired.len() {
            for k in &odd {
                let d_ref = k.diode.as_deref().unwrap();
                let d = diodes[d_ref];
                findings.push(
                    Finding::new(
                        "kb.diode-direction-mixed",
                        Severity::Error,
                        format!("{} (key {}) points the opposite way to {} other diodes.", d_ref, k.reference, wired.len() - odd.len()),
                    )
                    .refs([d_ref.to_string(), k.reference.clone()])
                    .at((d.x, d.y)),
                );
            }
        }
        matrix.switch_side_lines = sorted_natural(wired.iter().filter_map(|k| k.switch_line.clone()));
        matrix.diode_side_lines = sorted_natural(wired.iter().filter_map(|k| k.diode_line.clone()));
        let rows_on_diode = all_contain(&matrix.diode_side_lines, "ROW") || all_contain(&matrix.switch_side_lines, "COL");
        let cols_on_diode = all_contain(&matrix.diode_side_lines, "COL") || all_contain(&matrix.switch_side_lines, "ROW");
        if rows_on_diode || cols_on_diode {
            // Current flows from the switch side to the diode side when the cathode faces the diode-side line.
            let towards_rows = if rows_on_diode { majority } else { !majority };
            matrix.diode_direction = Some(if towards_rows { "col2row" } else { "row2col" }.into());
        }

        let mut seen: HashMap<(&str, &str), &str> = HashMap::new();
        for k in &wired {
            let pos = (k.switch_line.as_deref().unwrap(), k.diode_line.as_deref().unwrap());
            match seen.get(&pos) {
                Some(other) => findings.push(
                    Finding::new("kb.duplicate-matrix-position", Severity::Error, format!("{} and {} are both on {} x {}.", k.reference, other, pos.0, pos.1))
                        .refs([k.reference.clone(), other.to_string()])
                        .at((k.x, k.y)),
                ),
                None => {
                    seen.insert(pos, &k.reference);
                }
            }
        }
    }

    let mut lines: Vec<String> = matrix.switch_side_lines.iter().chain(&matrix.diode_side_lines).cloned().collect();
    if wired.is_empty() && diodes.is_empty() {
        lines = sorted_natural(matrix.keys.iter().filter_map(|k| k.switch_line.clone()));
    }
    match controller {
        None => findings.push(Finding::new("kb.no-controller", Severity::Warning, "No controller module footprint found.")),
        Some(c) => {
            for line in &lines {
                let pads = net_pads.get(line).cloned().unwrap_or_default();
                let Some(p) = pads.iter().find(|p| p.fp_ref == c.reference) else {
                    let members = sorted_natural(pads.iter().map(|p| p.fp_ref.clone()));
                    let shown: Vec<String> = members.into_iter().take(5).collect();
                    let mut f = Finding::new(
                        "kb.line-not-at-controller",
                        Severity::Error,
                        format!("Matrix line {line} (used by {}) never reaches {}.", shown.join(", "), c.reference),
                    )
                    .refs(shown);
                    if let Some(first) = pads.first() {
                        f = f.at(first.center());
                    }
                    findings.push(f);
                    continue;
                };
                matrix.pins.insert(line.clone(), pin_label(p));
                if !p.pinfunction.is_empty() && power_pin_re.is_match(p.pinfunction.trim()) {
                    findings.push(
                        Finding::new("kb.line-on-power-pin", Severity::Error, format!("Matrix line {line} is on {}.", pin_label(p)))
                            .refs([c.reference.clone()])
                            .at(p.center()),
                    );
                }
            }
            let named: Vec<&Pad> = c.pads.iter().filter(|p| p.electrical() && !p.pinfunction.is_empty()).collect();
            if !named.is_empty() {
                let gpio = named.iter().filter(|p| !power_pin_re.is_match(p.pinfunction.trim())).map(|p| &p.number).collect::<BTreeSet<_>>().len();
                matrix.gpio_available = Some(gpio);
                if lines.len() > gpio {
                    findings.push(
                        Finding::new(
                            "kb.too-many-lines",
                            Severity::Error,
                            format!("Matrix needs {} lines but {} has about {gpio} GPIO pins.", lines.len(), c.reference),
                        )
                        .refs([c.reference.clone()]),
                    );
                }
            }
        }
    }

    findings.extend(spacing(&switches, cfg));
    if let Some(c) = controller {
        if re(WIRELESS_RE).is_match(&c.text()) {
            findings.extend(wireless(board, c));
        }
    }

    if !wired.is_empty() {
        let size = format!("{} switch-side x {} diode-side lines", matrix.switch_side_lines.len(), matrix.diode_side_lines.len());
        let direction = matrix.diode_direction.as_ref().map(|d| format!(", diode-direction = {d}")).unwrap_or_default();
        let gpio = matrix.gpio_available.map(|g| format!(", uses {} of ~{g} GPIOs", lines.len())).unwrap_or_default();
        findings.push(Finding::new("kb.matrix-summary", Severity::Info, format!("{} keys, {size}{direction}{gpio}.", matrix.keys.len())));
    }
    (findings, Some(matrix))
}

fn spacing(switches: &[&Footprint], cfg: &KeyboardConfig) -> Vec<Finding> {
    let choc_re = re(CHOC_RE);
    let mut findings = Vec::new();
    for a in switches {
        for b in switches {
            if a.reference >= b.reference {
                continue;
            }
            let (dx, dy) = ((a.x - b.x).abs(), (a.y - b.y).abs());
            let choc = choc_re.is_match(&a.lib_id) || choc_re.is_match(&b.lib_id);
            let (px, py) = if choc { (18.0, 17.0) } else { (19.05, 19.05) };
            let px = cfg.key_pitch_x.or(cfg.key_pitch).unwrap_or(px);
            let py = cfg.key_pitch_y.or(cfg.key_pitch).unwrap_or(py);
            if dx < px - 0.5 && dy < py - 0.5 {
                findings.push(
                    Finding::new(
                        "kb.switch-overlap",
                        Severity::Error,
                        format!("{} and {} centres are only {:.2} mm apart.", a.reference, b.reference, dx.hypot(dy)),
                    )
                    .refs([a.reference.clone(), b.reference.clone()])
                    .at(((a.x + b.x) / 2.0, (a.y + b.y) / 2.0)),
                );
            }
        }
    }
    findings
}

fn wireless(board: &Board, controller: &Footprint) -> Vec<Finding> {
    let mut findings = Vec::new();
    let (x1, y1, x2, y2) = controller.bbox();
    let corners = [(x1, y1), (x2, y1), (x2, y2), (x1, y2), ((x1 + x2) / 2.0, (y1 + y2) / 2.0)];
    // A keep-out rule area covering the whole module means the pour will be cut away there.
    let kept_out = board.zones.iter().any(|z| z.keepout && z.outline.len() >= 3 && corners.iter().all(|&c| point_in_polygon(c, &z.outline)));
    for z in &board.zones {
        if kept_out || z.keepout || z.outline.len() < 3 {
            continue;
        }
        let overlaps = corners.iter().any(|&c| point_in_polygon(c, &z.outline))
            || z.outline.iter().any(|p| x1 <= p.0 && p.0 <= x2 && y1 <= p.1 && p.1 <= y2);
        if overlaps {
            let net = if z.net.is_empty() { "Unnamed" } else { &z.net };
            findings.push(
                Finding::new(
                    "kb.copper-under-antenna",
                    Severity::Warning,
                    format!("{net} zone on {} overlaps wireless module {}.", z.layers.join(", "), controller.reference),
                )
                .refs([controller.reference.clone()])
                .at((controller.x, controller.y))
                .fixable(format!("antenna-keepout:{}", controller.reference)),
            );
        }
    }

    if let Some(b) = board.outline_bbox() {
        let gap = (x1 - b.0).min(y1 - b.1).min(b.2 - x2).min(b.3 - y2);
        if gap > 5.0 {
            findings.push(
                Finding::new("kb.antenna-not-at-edge", Severity::Warning, format!("{} is {gap:.1} mm from the nearest board edge.", controller.reference))
                    .refs([controller.reference.clone()])
                    .at((controller.x, controller.y)),
            );
        }
    }

    let net_pads = board.net_pads();
    let battery_net = re(BATTERY_NET_RE);
    let battery_fp = re(r"(battery|jst|bat)");
    let has_battery = net_pads.keys().any(|n| battery_net.is_match(n))
        || board.footprints.iter().any(|f| f.reference != controller.reference && battery_fp.is_match(&f.text()));
    if !has_battery {
        findings.push(Finding::new("kb.no-battery", Severity::Warning, "No battery net (BAT, B+, VBAT, RAW) or connector found."));
    } else if !board.footprints.iter().any(|f| re(POWER_SWITCH_RE).is_match(&f.text())) {
        findings.push(Finding::new("kb.no-power-switch", Severity::Info, "Battery found but no power slide switch."));
    }

    let reset_re = re(r"^/?(n?rst|reset)$");
    let wired_reset = net_pads
        .iter()
        .filter(|(n, _)| reset_re.is_match(n))
        .any(|(_, pads)| pads.iter().map(|p| &p.fp_ref).collect::<BTreeSet<_>>().len() > 1);
    if !wired_reset {
        findings.push(Finding::new("kb.no-reset-button", Severity::Info, format!("{}'s RST pin is not wired to a button.", controller.reference)));
    }
    findings
}
