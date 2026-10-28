//! Component suitability: is each part rated for what the board puts on it, and is it over-spec?
//!
//! Two stages:
//! 1. `collect`: group footprints into BOM lines, read ratings (value text, built-in library, package
//!    defaults) and work out the voltage/current/power each part sees from the board's nets.
//! 2. `evaluate`: turn ratings vs stress into under-rated findings and over-spec notes.
//!
//! Everything runs offline from the board file. Agents can fill gaps for unknown parts via the
//! `review_parts` MCP tool.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use regex::{Regex, RegexBuilder};
use serde::{Deserialize, Serialize};

use crate::board::{Board, Footprint, Pad};
use crate::config::PowerConfig;
use crate::findings::{Finding, Severity};
use crate::keyboard::CONTROLLER_RE;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Resistor,
    Capacitor,
    Inductor,
    Diode,
    Led,
    Transistor,
    Regulator,
    Ic,
    Module,
    Connector,
    Switch,
    Crystal,
    Fuse,
    Other,
}

impl Kind {
    /// Kinds fabready can judge; the rest (connectors, switches, ...) are listed but not rated.
    fn evaluable(self) -> bool {
        matches!(self, Kind::Resistor | Kind::Capacitor | Kind::Diode | Kind::Led | Kind::Transistor | Kind::Regulator | Kind::Ic)
    }
    fn passive(self) -> bool {
        matches!(self, Kind::Resistor | Kind::Capacitor | Kind::Inductor | Kind::Diode | Kind::Led)
    }
}

/// Ratings. Any field may be unknown.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Specs {
    pub resistance_ohm: Option<f64>,
    pub capacitance_f: Option<f64>,
    /// Capacitor rated voltage, diode reverse voltage, or MOSFET Vds.
    pub voltage_v: Option<f64>,
    pub power_w: Option<f64>,
    /// Diode/LED forward current, regulator output current, MOSFET drain current.
    pub current_a: Option<f64>,
    pub vin_max_v: Option<f64>,
    pub vout_v: Option<f64>,
    pub dropout_v: Option<f64>,
    pub supply_min_v: Option<f64>,
    pub supply_max_v: Option<f64>,
    pub vf_v: Option<f64>,
    pub dielectric: Option<String>,
    pub polarized: bool,
    pub tantalum: bool,
}

/// What the board does to the part (worst case across the BOM line).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Stress {
    /// Voltage across a 2-terminal part (or reverse voltage for diodes, Vds for MOSFETs).
    pub voltage_v: Option<f64>,
    /// True when `voltage_v` is an upper bound (one side's net voltage unknown).
    pub voltage_assumed: bool,
    pub power_w: Option<f64>,
    pub current_a: Option<f64>,
    pub vin_v: Option<f64>,
    pub supply_v: Option<f64>,
    /// Expected load (from fabready.json `power.current_ma`) for regulators.
    pub load_a: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Under,
    Over,
    Info,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Note {
    pub level: Level,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Part {
    pub refs: Vec<String>,
    pub value: String,
    pub footprint: String,
    pub kind: Kind,
    pub package: Option<String>,
    pub through_hole: bool,
    pub lcsc: Option<String>,
    pub mpn: Option<String>,
    pub description: Option<String>,
    pub specs: Specs,
    /// Where each spec came from, e.g. `voltage_v: "value text"`.
    pub sources: BTreeMap<String, String>,
    pub stress: Stress,
    /// Nets on each pin of the first part in the line: `pad -> net (volts)`.
    pub pins: Vec<String>,
    /// `ok`, `under`, `over`, `unknown`, or `na` (not something fabready rates, e.g. connectors).
    pub verdict: String,
    pub notes: Vec<Note>,
    #[serde(skip)]
    pub at: Option<(f64, f64)>,
}

impl Part {
    pub fn label(&self) -> String {
        let refs = if self.refs.len() > 3 { format!("{}…{}", self.refs[0], self.refs[self.refs.len() - 1]) } else { self.refs.join(", ") };
        format!("{refs} ({})", self.value)
    }
    fn src(&mut self, key: &str, source: &str) {
        self.sources.insert(key.into(), source.into());
    }
}

// ------------------------------------------------------------------------------------------ parsing

fn re(p: &str) -> Regex {
    RegexBuilder::new(p).case_insensitive(true).build().expect("valid regex")
}

fn si_prefix(p: &str) -> f64 {
    match p {
        "p" | "P" => 1e-12,
        "n" | "N" => 1e-9,
        "u" | "U" | "µ" | "μ" => 1e-6,
        "m" => 1e-3,
        "k" | "K" => 1e3,
        "M" => 1e6,
        "G" => 1e9,
        _ => 1.0,
    }
}

/// "4k7" / "2R2" / "4u7" style values.
fn infix_value(s: &str) -> Option<f64> {
    let caps = Regex::new(r"^(\d+)([pnuµμmkKMGR])(\d+)$").unwrap().captures(s)?;
    let p = &caps[2];
    let mult = if p == "R" { 1.0 } else { si_prefix(p) };
    format!("{}.{}", &caps[1], &caps[3]).parse::<f64>().ok().map(|v| v * mult)
}

pub fn parse_resistance(value: &str) -> Option<f64> {
    let v = value.trim().replace(['Ω', 'Ω'], "").replace("ohms", "").replace("ohm", "");
    let token = v.split_whitespace().next()?;
    if let Some(x) = infix_value(token) {
        return Some(x);
    }
    let caps = Regex::new(r"^(\d+(?:\.\d+)?)\s*([kKMGmR]?)$").unwrap().captures(token)?;
    let n: f64 = caps[1].parse().ok()?;
    Some(if &caps[2] == "R" { n } else { n * si_prefix(&caps[2]) })
}

pub fn parse_capacitance(value: &str) -> Option<f64> {
    let cap_re = Regex::new(r"^(\d+(?:\.\d+)?)([pnuµμm])$").unwrap();
    for token in value.split(|c: char| c.is_whitespace() || c == '/' || c == ',') {
        let t = token.trim_end_matches(['F', 'f']);
        if let Some(x) = infix_value(t) {
            return Some(x);
        }
        if let Some(c) = cap_re.captures(t) {
            return c[1].parse::<f64>().ok().map(|n| n * si_prefix(&c[2]));
        }
    }
    None
}

/// First "<number><prefix><unit>" in a string, e.g. ("1.1V@(800mA)", "V") -> 1.1.
pub fn parse_quantity(s: &str, unit: &str) -> Option<f64> {
    if unit == "W" {
        if let Some(c) = Regex::new(r"(\d+)\s*/\s*(\d+)\s*W").unwrap().captures(s) {
            return Some(c[1].parse::<f64>().ok()? / c[2].parse::<f64>().ok()?);
        }
    }
    let pat = format!(r"(\d+(?:\.\d+)?)\s*([pnuµμmkKM]?){unit}(?:[^a-zA-Z]|$)");
    let c = Regex::new(&pat).unwrap().captures(s)?;
    c[1].parse::<f64>().ok().map(|n| n * si_prefix(&c[2]))
}

fn dielectric(s: &str) -> Option<String> {
    re(r"\b(X5R|X7R|X6S|X7S|X8R|C0G|NP0|Y5V|Z5U)\b").captures(s).map(|c| c[1].to_uppercase())
}

/// Typical LED forward voltage by colour.
fn led_vf(text: &str) -> f64 {
    let t = text.to_lowercase();
    if ["blue", "white", "green", "_bl", "_wh", "warm", "uv", "rgb"].iter().any(|k| t.contains(k)) {
        3.0
    } else if t.contains("ir") && !t.contains("dir") {
        1.3
    } else {
        2.0
    }
}

pub fn package_of(lib_id: &str) -> Option<String> {
    let name = lib_id.rsplit(':').next().unwrap_or(lib_id);
    if let Some(c) = re(r"(?:^|[_\-])(01005|0201|0402|0603|0805|1206|1210|1812|2010|2512)(?:[_\-]|$)").captures(name) {
        return Some(c[1].to_string());
    }
    let c = re(r"(SOD-?\d{3}F?|SOT-?\d{2,3}(?:-\d)?|SMA|SMB|SMC|DO-?\d{2,3}[A-Z]*|TO-?\d{2,3}|MiniMELF|SOIC-\d+|QFN-\d+|TSSOP-\d+|PLCC-?\d|DIP-\d+|Axial|Radial)").captures(name)?;
    Some(c[1].to_uppercase().replace("SOD123", "SOD-123").replace("SOT23", "SOT-23"))
}

/// Typical power rating of chip resistors by size.
fn resistor_power(package: &str) -> Option<f64> {
    Some(match package {
        "01005" => 0.031,
        "0201" => 0.05,
        "0402" => 0.0625,
        "0603" => 0.1,
        "0805" => 0.125,
        "1206" => 0.25,
        "1210" => 0.5,
        "1812" | "2010" => 0.75,
        "2512" => 1.0,
        "AXIAL" => 0.25,
        _ => return None,
    })
}

fn package_rank(package: &str) -> u8 {
    match package {
        "01005" | "0201" => 0,
        "0402" => 1,
        "0603" => 2,
        "0805" => 3,
        "1206" => 4,
        "1210" | "1812" | "2010" | "2512" => 5,
        _ => 3,
    }
}

// ------------------------------------------------------------------------------------------ built-in library

struct Known {
    pattern: &'static str,
    kind: Kind,
    voltage_v: Option<f64>,
    current_a: Option<f64>,
    vin_max_v: Option<f64>,
    dropout_v: Option<f64>,
    supply: Option<(f64, f64)>,
}

const fn k(pattern: &'static str, kind: Kind, voltage_v: Option<f64>, current_a: Option<f64>) -> Known {
    Known { pattern, kind, voltage_v, current_a, vin_max_v: None, dropout_v: None, supply: None }
}
const fn reg(pattern: &'static str, vin_max: f64, iout: f64, dropout: f64) -> Known {
    Known { pattern, kind: Kind::Regulator, voltage_v: None, current_a: Some(iout), vin_max_v: Some(vin_max), dropout_v: Some(dropout), supply: None }
}
const fn sup(pattern: &'static str, kind: Kind, lo: f64, hi: f64) -> Known {
    Known { pattern, kind, voltage_v: None, current_a: None, vin_max_v: None, dropout_v: None, supply: Some((lo, hi)) }
}

/// Typical datasheet values for common hobby parts. Values in the part's own fields take priority where parsed.
const LIBRARY: &[Known] = &[
    k(r"^1n4148", Kind::Diode, Some(75.0), Some(0.15)),
    k(r"^1n4001", Kind::Diode, Some(50.0), Some(1.0)),
    k(r"^1n4002", Kind::Diode, Some(100.0), Some(1.0)),
    k(r"^1n4003", Kind::Diode, Some(200.0), Some(1.0)),
    k(r"^1n4004", Kind::Diode, Some(400.0), Some(1.0)),
    k(r"^1n4005", Kind::Diode, Some(600.0), Some(1.0)),
    k(r"^1n4006", Kind::Diode, Some(800.0), Some(1.0)),
    k(r"^1n4007|^m7$", Kind::Diode, Some(1000.0), Some(1.0)),
    k(r"^1n5817", Kind::Diode, Some(20.0), Some(1.0)),
    k(r"^1n5818", Kind::Diode, Some(30.0), Some(1.0)),
    k(r"^1n5819|^b5819", Kind::Diode, Some(40.0), Some(1.0)),
    k(r"^1n5822", Kind::Diode, Some(40.0), Some(3.0)),
    k(r"^ss14", Kind::Diode, Some(40.0), Some(1.0)),
    k(r"^ss34", Kind::Diode, Some(40.0), Some(3.0)),
    k(r"^ss54", Kind::Diode, Some(40.0), Some(5.0)),
    k(r"^bat54", Kind::Diode, Some(30.0), Some(0.2)),
    k(r"^ao3400", Kind::Transistor, Some(30.0), Some(5.7)),
    k(r"^ao3401", Kind::Transistor, Some(30.0), Some(4.0)),
    k(r"^si2302", Kind::Transistor, Some(20.0), Some(2.6)),
    k(r"^2n7002", Kind::Transistor, Some(60.0), Some(0.2)),
    k(r"^bss138", Kind::Transistor, Some(50.0), Some(0.2)),
    reg(r"^ams1117", 15.0, 1.0, 1.1),
    reg(r"^lm1117", 15.0, 0.8, 1.2),
    reg(r"^ld1117", 15.0, 0.8, 1.1),
    reg(r"^mcp1700", 6.0, 0.25, 0.18),
    reg(r"^mcp1825", 6.0, 0.5, 0.21),
    reg(r"^xc6206", 6.0, 0.2, 0.25),
    reg(r"^ht7[35]3", 12.0, 0.25, 0.1),
    reg(r"^ap2112", 6.0, 0.6, 0.25),
    reg(r"^me6211", 6.5, 0.5, 0.1),
    reg(r"^rt9013", 5.5, 0.5, 0.25),
    reg(r"^tlv755", 5.5, 0.5, 0.24),
    reg(r"^ap7361", 6.0, 1.0, 0.36),
    reg(r"^mic5219", 12.0, 0.5, 0.35),
    reg(r"^(lm|l|ua|mc)?78(0[5-9]|1[0258]|24)", 35.0, 1.5, 2.0),
    sup(r"^ws2812b", Kind::Led, 3.5, 5.3),
    sup(r"^sk6812", Kind::Led, 3.5, 5.5),
    sup(r"^esp32", Kind::Ic, 3.0, 3.6),
    sup(r"^atmega32u4", Kind::Ic, 2.7, 5.5),
    sup(r"^stm32", Kind::Ic, 2.0, 3.6),
    sup(r"^rp2040", Kind::Ic, 1.8, 3.63),
];

fn known(text: &str) -> Option<&'static Known> {
    let t = text.trim().to_lowercase();
    LIBRARY.iter().find(|e| Regex::new(e.pattern).map(|r| r.is_match(&t)).unwrap_or(false))
}

/// Output voltage from a regulator name: AMS1117-3.3, MCP1700-3302E, XC6206P332MR, LM7805.
fn regulator_vout(name: &str) -> Option<f64> {
    let n = name.to_uppercase();
    if let Some(c) = Regex::new(r"78(0[5-9]|1[0258]|24)").unwrap().captures(&n) {
        return c[1].parse().ok();
    }
    if n.contains("ADJ") {
        return None;
    }
    let tail = n.split_once(['-', '_']).map(|(_, t)| t.to_string()).or_else(|| n.get(6..).map(str::to_string))?;
    let c = Regex::new(r"(\d)[.V]?(\d)").unwrap().captures(&tail)?;
    let v: f64 = format!("{}.{}", &c[1], &c[2]).parse().ok()?;
    (0.8..=15.0).contains(&v).then_some(v)
}

// ------------------------------------------------------------------------------------------ board context

/// Guess a net's DC voltage from its name. `None` = unknown (signals etc.).
pub fn net_voltage(name: &str) -> Option<f64> {
    let n = name.trim_start_matches('/').rsplit('/').next().unwrap_or(name).to_uppercase();
    if re(r"^(A|D|P|C|S)?GND\w*$|^VSS$|^0V$").is_match(&n) {
        return Some(0.0);
    }
    if let Some(c) = Regex::new(r"^\+?(\d+)V(\d+)$").unwrap().captures(&n) {
        return format!("{}.{}", &c[1], &c[2]).parse().ok();
    }
    if let Some(c) = Regex::new(r"^\+?(\d+(?:\.\d+)?)V(?:[_\-]\w*)?$").unwrap().captures(&n) {
        return c[1].parse().ok();
    }
    if re(r"^(USB_)?VBUS|^VUSB$|^USB_?5V$").is_match(&n) {
        return Some(5.0);
    }
    if re(r"^(V?BATT?|BAT\+|B\+|VBATT?)$").is_match(&n) {
        return Some(4.2);
    }
    if n == "RAW" {
        return Some(5.0);
    }
    None
}

fn is_excluded(fp: &Footprint) -> bool {
    let lib = fp.lib_id.to_lowercase();
    fp.attrs.iter().any(|a| a == "exclude_from_bom" || a == "board_only")
        || fp.reference.starts_with('#')
        || ["mountinghole", "mounting_hole", "fiducial", "testpoint", "logo", "symbol:"].iter().any(|k| lib.replace('_', "").contains(&k.replace('_', "")))
        || fp.pads.iter().all(|p| !p.electrical())
}

fn kind_of(fp: &Footprint) -> Kind {
    let prefix: String = fp.reference.chars().take_while(|c| c.is_ascii_alphabetic()).collect::<String>().to_uppercase();
    let text = format!("{} {}", fp.lib_id, fp.value).to_lowercase();
    if let Some(entry) = known(&fp.value).or_else(|| fp.props.get("MPN").and_then(|m| known(m))) {
        return entry.kind;
    }
    if re(CONTROLLER_RE).is_match(&text) {
        return Kind::Module;
    }
    let lib = fp.lib_id.split(':').next().unwrap_or("").to_lowercase();
    match prefix.as_str() {
        "R" | "RN" => Kind::Resistor,
        "C" => Kind::Capacitor,
        "L" | "FB" => Kind::Inductor,
        "D" | "LED" if text.contains("led") || text.contains("ws2812") || text.contains("sk6812") => Kind::Led,
        "D" => Kind::Diode,
        "LED" => Kind::Led,
        "Q" => Kind::Transistor,
        "U" | "IC" => {
            if text.contains("ldo") || text.contains("regulator") || lib.contains("regulator") { Kind::Regulator } else { Kind::Ic }
        }
        "J" | "P" | "CN" | "CON" | "USB" => Kind::Connector,
        "SW" | "S" | "K" | "BTN" => Kind::Switch,
        "Y" | "X" => Kind::Crystal,
        "F" | "PTC" => Kind::Fuse,
        _ => match lib.as_str() {
            l if l.starts_with("resistor") => Kind::Resistor,
            l if l.starts_with("capacitor") => Kind::Capacitor,
            l if l.starts_with("led") => Kind::Led,
            l if l.starts_with("diode") => Kind::Diode,
            _ => Kind::Other,
        },
    }
}

fn prop<'a>(fp: &'a Footprint, names: &[&str]) -> Option<&'a str> {
    fp.props.iter().find(|(k, v)| names.iter().any(|n| k.eq_ignore_ascii_case(n)) && !v.trim().is_empty() && *v != "~").map(|(_, v)| v.as_str())
}

/// Electrical pads grouped by pad number (first pad of each number).
fn pins(fp: &Footprint) -> Vec<&Pad> {
    let mut seen = BTreeSet::new();
    fp.pads.iter().filter(|p| p.electrical() && seen.insert(p.number.clone())).collect()
}

fn anode_cathode(fp: &Footprint) -> Option<(&Pad, &Pad)> {
    let ps = pins(fp);
    if ps.len() != 2 {
        return None;
    }
    let is = |p: &Pad, f: &str| p.pinfunction.eq_ignore_ascii_case(f) || p.number.eq_ignore_ascii_case(f);
    let k = ps.iter().find(|p| is(p, "K")).copied().unwrap_or(ps[0]); // KiCad diode/LED convention: pad 1 = K
    let a = ps.iter().find(|p| !std::ptr::eq(**p, k)).copied()?;
    Some((a, k))
}

// ------------------------------------------------------------------------------------------ collect

pub fn collect(board: &Board, power: &PowerConfig) -> Vec<Part> {
    let volt_re = Regex::new(r"(\d+(?:\.\d+)?)\s*V(?:DC)?(?:[^a-zA-Z]|$)").unwrap();
    let fps: Vec<&Footprint> = board.footprints.iter().filter(|f| !is_excluded(f)).collect();
    let kinds: HashMap<&str, Kind> = fps.iter().map(|f| (f.reference.as_str(), kind_of(f))).collect();
    let kind_for = |f: &Footprint| kinds.get(f.reference.as_str()).copied().unwrap_or(Kind::Other);
    // LED anode net -> (LED footprint, cathode net), for working out series-resistor currents.
    let led_by_anode: HashMap<&str, (&Footprint, &str)> = fps
        .iter()
        .filter(|f| kind_for(f) == Kind::Led)
        .filter_map(|f| anode_cathode(f).map(|(a, k)| (a.net.as_str(), (*f, k.net.as_str()))))
        .collect();

    // Net voltages: names, user overrides, then regulator outputs.
    let mut volts: HashMap<String, f64> = HashMap::new();
    for p in board.pads() {
        if p.net.is_empty() || volts.contains_key(&p.net) {
            continue;
        }
        let user = power.rails.get(&p.net).or_else(|| power.rails.get(p.net.trim_start_matches('/'))).copied();
        if let Some(v) = user.or_else(|| net_voltage(&p.net)) {
            volts.insert(p.net.clone(), v);
        }
    }
    for fp in &fps {
        if kind_for(fp) != Kind::Regulator {
            continue;
        }
        let Some(vout) = regulator_vout(&fp.value).or_else(|| prop(fp, &["MPN"]).and_then(regulator_vout)) else { continue };
        if let Some(out) = pins(fp).into_iter().find(|p| re(r"^(VOUT|OUT|VO|OUTPUT)$").is_match(p.pinfunction.trim())) {
            volts.entry(out.net.clone()).or_insert(vout);
        }
    }
    let max_rail = volts.values().copied().fold(None, |m: Option<f64>, v| Some(m.map_or(v, |m| m.max(v))));
    let v_of = |net: &str| volts.get(net).copied();

    // Group into BOM lines.
    let mut groups: BTreeMap<(String, String, String), Vec<&Footprint>> = BTreeMap::new();
    for fp in &fps {
        let key = (fp.value.clone(), fp.lib_id.clone(), prop(fp, &["LCSC", "LCSC Part", "LCSC Part #", "JLCPCB Part #"]).unwrap_or("").to_string());
        groups.entry(key).or_default().push(fp);
    }

    // LED currents set by series resistors: led ref -> amps.
    let mut led_current: HashMap<String, f64> = HashMap::new();
    let mut parts = Vec::new();
    for ((value, lib_id, lcsc), mut members) in groups {
        members.sort_by_key(|f| crate::keyboard::natural_key(&f.reference));
        let first = members[0];
        let kind = kind_for(first);
        let package = package_of(&lib_id);
        let through_hole = first.attrs.iter().any(|a| a == "through_hole")
            || (kind.passive() && pins(first).iter().all(|p| p.kind == "thru_hole"));
        let mpn = prop(first, &["MPN", "Manufacturer Part Number", "Part Number", "MFR.Part #"]).map(str::to_string);
        let description = prop(first, &["Description", "Desc"]).map(str::to_string);
        let mut part = Part {
            refs: members.iter().map(|f| f.reference.clone()).collect(),
            value: value.clone(),
            footprint: lib_id.clone(),
            kind,
            package: package.clone(),
            through_hole,
            lcsc: (!lcsc.is_empty()).then(|| lcsc.clone()),
            mpn: mpn.clone(),
            description: description.clone(),
            specs: Specs::default(),
            sources: BTreeMap::new(),
            stress: Stress::default(),
            pins: pins(first)
                .iter()
                .map(|p| {
                    let name = if p.pinfunction.is_empty() { p.number.clone() } else { format!("{} {}", p.number, p.pinfunction) };
                    match v_of(&p.net) {
                        Some(v) => format!("{name}: {} ({v} V)", p.net),
                        None => format!("{name}: {}", if p.net.is_empty() { "—" } else { &p.net }),
                    }
                })
                .collect(),
            verdict: String::new(),
            notes: Vec::new(),
            at: Some((first.x, first.y)),
        };

        // --- ratings from text
        let text = [Some(value.as_str()), description.as_deref(), mpn.as_deref(), prop(first, &["Voltage", "Voltage Rating", "Rated Voltage", "Power", "Tolerance", "Dielectric"])]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" ");
        match kind {
            Kind::Resistor => {
                part.specs.resistance_ohm = parse_resistance(&value);
                if let Some(p) = parse_quantity(&text, "W") {
                    part.specs.power_w = Some(p);
                    part.src("power_w", "value text");
                } else if let Some(p) = package.as_deref().map(str::to_uppercase).as_deref().and_then(resistor_power).or_else(|| through_hole.then_some(0.25)) {
                    part.specs.power_w = Some(p);
                    part.src("power_w", &format!("typical for {}", package.as_deref().unwrap_or("through-hole")));
                }
            }
            Kind::Capacitor => {
                part.specs.capacitance_f = parse_capacitance(&value);
                if let Some(v) = volt_re.captures_iter(&text).last().and_then(|c| c[1].parse().ok()) {
                    part.specs.voltage_v = Some(v);
                    part.src("voltage_v", "value text");
                }
                part.specs.dielectric = dielectric(&text);
                let t = format!("{lib_id} {text}").to_lowercase();
                part.specs.tantalum = t.contains("tantal");
                part.specs.polarized = part.specs.tantalum || t.contains("cp_") || t.contains("elec") || t.contains("polar");
            }
            Kind::Led => {
                part.specs.vf_v = Some(led_vf(&format!("{lib_id} {text}")));
                part.src("vf_v", "typical for colour");
                part.specs.current_a = Some(0.02);
                part.src("current_a", "typical indicator LED");
            }
            _ => {}
        }
        // --- built-in library
        if let Some(e) = known(&value).or_else(|| mpn.as_deref().and_then(known)) {
            let src = format!("built-in: {} (typical)", value);
            if e.voltage_v.is_some() {
                part.specs.voltage_v = e.voltage_v;
                part.src("voltage_v", &src);
            }
            if e.current_a.is_some() {
                part.specs.current_a = e.current_a;
                part.src("current_a", &src);
            }
            if e.vin_max_v.is_some() {
                part.specs.vin_max_v = e.vin_max_v;
                part.specs.dropout_v = e.dropout_v;
                part.src("vin_max_v", &src);
                part.src("dropout_v", &src);
            }
            if let Some((lo, hi)) = e.supply {
                part.specs.supply_min_v = Some(lo);
                part.specs.supply_max_v = Some(hi);
                part.src("supply", &src);
                part.sources.remove("current_a");
                if kind == Kind::Led {
                    part.specs.current_a = None;
                    part.specs.vf_v = None;
                }
            }
        }
        if kind == Kind::Regulator {
            part.specs.vout_v = regulator_vout(&value).or_else(|| mpn.as_deref().and_then(regulator_vout));
        }

        // --- stress, worst case over the line
        let mut stress = Stress::default();
        let bump = |slot: &mut Option<f64>, v: f64| *slot = Some(slot.map_or(v, |s: f64| s.max(v)));
        for fp in &members {
            let ps = pins(fp);
            match kind {
                Kind::Resistor | Kind::Capacitor | Kind::Inductor if ps.len() == 2 => {
                    let (na, nb) = (ps[0].net.as_str(), ps[1].net.as_str());
                    let (mut va, mut vb) = (v_of(na), v_of(nb));
                    // A resistor feeding an LED: the far node sits at V(cathode) + Vf.
                    if kind == Kind::Resistor {
                        let led_node = |rail: Option<f64>, far: &str| -> Option<f64> {
                            rail?;
                            let (led_fp, cathode) = led_by_anode.get(far)?;
                            let vf = led_vf(&format!("{} {}", led_fp.lib_id, led_fp.value));
                            Some(v_of(cathode)? + vf)
                        };
                        for (rail, far) in [(va, nb), (vb, na)] {
                            let (Some(rail_v), None, Some(node)) = (rail, v_of(far), led_node(rail, far)) else { continue };
                            if far == nb { vb = Some(node) } else { va = Some(node) }
                            if let Some(r) = part.specs.resistance_ohm.filter(|r| *r > 0.0) {
                                let led_ref = led_by_anode[far].0.reference.clone();
                                let e = led_current.entry(led_ref).or_insert(0.0);
                                *e = e.max(((rail_v - node) / r).max(0.0));
                            }
                        }
                    }
                    match (va, vb) {
                        (Some(a), Some(b)) => {
                            let v = (a - b).abs();
                            if stress.voltage_assumed || stress.voltage_v.is_none_or(|s| v > s) {
                                stress.voltage_v = Some(v);
                                stress.voltage_assumed = false;
                            }
                            if let Some(r) = part.specs.resistance_ohm.filter(|r| *r > 0.0) {
                                bump(&mut stress.power_w, v * v / r);
                                bump(&mut stress.current_a, v / r);
                            }
                        }
                        (a, b) => {
                            if let Some(bound) = max_rail.map(|m| m.max(a.or(b).unwrap_or(0.0))) {
                                if stress.voltage_v.is_none() {
                                    stress.voltage_v = Some(bound);
                                    stress.voltage_assumed = true;
                                }
                            }
                        }
                    }
                }
                Kind::Diode => {
                    if let Some((a, k)) = anode_cathode(fp) {
                        match (v_of(&a.net), v_of(&k.net)) {
                            (Some(va), Some(vk)) => {
                                bump(&mut stress.voltage_v, (vk - va).max(0.0));
                                stress.voltage_assumed = false;
                            }
                            _ => {
                                if stress.voltage_v.is_none() {
                                    stress.voltage_v = max_rail;
                                    stress.voltage_assumed = true;
                                }
                            }
                        }
                    }
                }
                Kind::Transistor => {
                    let d = ps.iter().find(|p| p.pinfunction.eq_ignore_ascii_case("D")).and_then(|p| v_of(&p.net));
                    let s = ps.iter().find(|p| p.pinfunction.eq_ignore_ascii_case("S")).and_then(|p| v_of(&p.net));
                    if let (Some(d), Some(s)) = (d, s) {
                        bump(&mut stress.voltage_v, (d - s).abs());
                    } else if stress.voltage_v.is_none() {
                        stress.voltage_v = max_rail;
                        stress.voltage_assumed = true;
                    }
                }
                Kind::Regulator => {
                    let vin = ps
                        .iter()
                        .find(|p| re(r"^(VIN|IN|VI|INPUT)$").is_match(p.pinfunction.trim()))
                        .and_then(|p| v_of(&p.net))
                        .or_else(|| {
                            // No pin names: the highest known voltage that isn't the output.
                            let vout = part.specs.vout_v.unwrap_or(f64::MAX);
                            ps.iter().filter_map(|p| v_of(&p.net)).filter(|v| (*v - vout).abs() > 1e-6).fold(None, |m: Option<f64>, v| Some(m.map_or(v, |m| m.max(v))))
                        });
                    if let Some(v) = vin {
                        bump(&mut stress.vin_v, v);
                    }
                    if let Some(out) = ps.iter().find(|p| re(r"^(VOUT|OUT|VO|OUTPUT)$").is_match(p.pinfunction.trim())) {
                        if let Some(ma) = power.current_ma.get(&out.net) {
                            bump(&mut stress.load_a, ma / 1000.0);
                        }
                    } else if let Some(ma) = part.specs.vout_v.and_then(|vo| volts.iter().find(|(_, v)| (**v - vo).abs() < 1e-6)).and_then(|(n, _)| power.current_ma.get(n)) {
                        bump(&mut stress.load_a, ma / 1000.0);
                    }
                }
                Kind::Led | Kind::Ic => {
                    if let Some(v) = ps
                        .iter()
                        .find(|p| re(r"^(VDD|VCC|V\+|VDDIO|IOVDD|DVDD|PWR)$").is_match(p.pinfunction.trim()))
                        .and_then(|p| v_of(&p.net))
                    {
                        bump(&mut stress.supply_v, v);
                    }
                }
                _ => {}
            }
        }
        part.stress = stress;
        parts.push(part);
    }

    // LED currents were found while visiting resistors; attach them to the LED lines.
    for part in parts.iter_mut().filter(|p| p.kind == Kind::Led) {
        if let Some(i) = part.refs.iter().filter_map(|r| led_current.get(r)).copied().fold(None, |m: Option<f64>, v| Some(m.map_or(v, |m| m.max(v)))) {
            part.stress.current_a = Some(i);
        }
    }
    parts
}

// ------------------------------------------------------------------------------------------ evaluate

fn fmt(v: f64, unit: &str) -> String {
    let (n, p) = if v != 0.0 && v.abs() < 1e-9 {
        (v * 1e12, "p")
    } else if v != 0.0 && v.abs() < 1e-6 {
        (v * 1e9, "n")
    } else if v != 0.0 && v.abs() < 1e-3 {
        (v * 1e6, "µ")
    } else if v != 0.0 && v.abs() < 1.0 && unit != "V" {
        (v * 1e3, "m")
    } else {
        (v, "")
    };
    let s = format!("{n:.2}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    format!("{s} {p}{unit}")
}

/// Compare ratings with stress; fill `notes`/`verdict` and return findings for under-rated parts.
pub fn evaluate(parts: &mut [Part]) -> Vec<Finding> {
    let mut findings = Vec::new();
    for part in parts.iter_mut() {
        part.notes.clear();
        let mut under: Vec<(&str, Severity, String)> = Vec::new();
        let mut over: Vec<String> = Vec::new();
        let mut info: Vec<String> = Vec::new();
        let (s, st) = (&part.specs, &part.stress);
        let assumed = if st.voltage_assumed { " (assumed: the other side's voltage is unknown)" } else { "" };

        match part.kind {
            Kind::Capacitor => {
                if let (Some(rated), Some(v)) = (s.voltage_v, st.voltage_v) {
                    let margin = if s.tantalum { 2.0 } else if s.polarized { 1.25 } else { 1.5 };
                    let kind = if s.tantalum { "tantalum" } else if s.polarized { "electrolytic" } else { "ceramic" };
                    if !st.voltage_assumed && v > rated {
                        under.push(("part.voltage-exceeded", Severity::Error, format!("{} is rated {} but sees {}.", part.label(), fmt(rated, "V"), fmt(v, "V"))));
                    } else if !st.voltage_assumed && v * margin > rated {
                        under.push((
                            "part.voltage-margin",
                            Severity::Warning,
                            format!("{} is rated {} on a {} rail; {kind} capacitors want at least {}× ({}).", part.label(), fmt(rated, "V"), fmt(v, "V"), margin, fmt(v * margin, "V")),
                        ));
                    } else if v > 0.0 && rated >= 5.0 * v.max(1.0) && (part.package.as_deref().is_some_and(|p| package_rank(p) >= 4) || part.through_hole) {
                        over.push(format!("Rated {} for {}{assumed}. A {} part in 0603/0805 would do and is smaller.", fmt(rated, "V"), fmt(v, "V"), fmt(nice_voltage(v * margin), "V")));
                    }
                }
            }
            Kind::Resistor => {
                if let (Some(rated), Some(p)) = (s.power_w, st.power_w) {
                    if p > rated {
                        under.push(("part.power-exceeded", Severity::Error, format!("{} dissipates {} but is rated {}.", part.label(), fmt(p, "W"), fmt(rated, "W"))));
                    } else if p > 0.5 * rated {
                        under.push(("part.power-margin", Severity::Warning, format!("{} dissipates {} of its {} rating; keep resistors under half their rating.", part.label(), fmt(p, "W"), fmt(rated, "W"))));
                    }
                }
                let need = st.power_w.or_else(|| match (st.voltage_v, s.resistance_ohm) {
                    (Some(v), Some(r)) if r > 0.0 => Some(v * v / r),
                    _ => None,
                });
                if let (Some(need), Some(pkg)) = (need, part.package.as_deref()) {
                    if package_rank(pkg) >= 4 && need < 0.1 * 0.1 {
                        over.push(format!("{pkg} package for about {} of heat; an 0603 (0.1 W) or 0402 is plenty.", fmt(need, "W")));
                    }
                }
            }
            Kind::Led => {
                if let Some(i) = st.current_a {
                    let max = s.current_a.unwrap_or(0.02);
                    if i > 1.5 * max {
                        under.push(("part.led-current", Severity::Error, format!("{} runs at about {} (rated {}). Increase the series resistor.", part.label(), fmt(i, "A"), fmt(max, "A"))));
                    } else if i > max {
                        under.push(("part.led-current", Severity::Warning, format!("{} runs at about {}, above the typical {}. Increase the series resistor.", part.label(), fmt(i, "A"), fmt(max, "A"))));
                    } else if i > 0.0 && i < 0.0005 {
                        info.push(format!("Only about {} through the LED; it may be dim.", fmt(i, "A")));
                    }
                }
            }
            Kind::Diode | Kind::Transistor => {
                let what = if part.kind == Kind::Diode { "reverse voltage" } else { "drain-source voltage" };
                if let (Some(rated), Some(v)) = (s.voltage_v, st.voltage_v) {
                    if !st.voltage_assumed && v > rated {
                        under.push(("part.voltage-exceeded", Severity::Error, format!("{} sees {} {what} but is rated {}.", part.label(), fmt(v, "V"), fmt(rated, "V"))));
                    } else if !st.voltage_assumed && v > 0.8 * rated {
                        under.push(("part.voltage-margin", Severity::Warning, format!("{} sees {} {what}, over 80% of its {} rating.", part.label(), fmt(v, "V"), fmt(rated, "V"))));
                    } else if v > 0.0 && rated >= 20.0 * v && (part.through_hole || part.package.as_deref().is_some_and(|p| p.starts_with("DO-") || p == "SMA" || p == "SMB" || p == "SMC")) {
                        over.push(format!(
                            "Rated {} / {} for {}{assumed}. A small-signal part (e.g. 1N4148W or BAT54 in SOD-123) is smaller and usually a JLCPCB basic part.",
                            fmt(rated, "V"),
                            fmt(s.current_a.unwrap_or(0.0), "A"),
                            fmt(v, "V")
                        ));
                    }
                }
            }
            Kind::Regulator => {
                if let (Some(max), Some(vin)) = (s.vin_max_v, st.vin_v) {
                    if vin > max {
                        under.push(("part.regulator-input", Severity::Error, format!("{} input is {} but its maximum is {}.", part.label(), fmt(vin, "V"), fmt(max, "V"))));
                    }
                }
                if let (Some(vo), Some(vin), Some(drop)) = (s.vout_v, st.vin_v, s.dropout_v) {
                    if vin < vo + drop {
                        under.push((
                            "part.regulator-dropout",
                            Severity::Warning,
                            format!("{} needs about {} in to hold {} out (dropout {}), but gets {}. Use a low-dropout regulator.", part.label(), fmt(vo + drop, "V"), fmt(vo, "V"), fmt(drop, "V"), fmt(vin, "V")),
                        ));
                    }
                }
                if let (Some(iout), Some(load)) = (s.current_a, st.load_a) {
                    if load > iout {
                        under.push(("part.regulator-current", Severity::Error, format!("{} supplies up to {} but the load is {}.", part.label(), fmt(iout, "A"), fmt(load, "A"))));
                    } else if iout >= 10.0 * load {
                        over.push(format!("Rated {} for a {} load. A smaller regulator (150–300 mA, SOT-23) is cheaper and smaller.", fmt(iout, "A"), fmt(load, "A")));
                    }
                } else if s.current_a.is_some() && st.load_a.is_none() {
                    info.push("Set the expected load in fabready.json (`power.current_ma`) to check whether this regulator is sized right.".into());
                }
            }
            _ => {}
        }
        if let (Some(lo), Some(hi), Some(v)) = (s.supply_min_v, s.supply_max_v, st.supply_v) {
            if v < lo || v > hi {
                under.push(("part.supply-range", Severity::Warning, format!("{} is powered from {} but needs {}–{}.", part.label(), fmt(v, "V"), fmt(lo, "V"), fmt(hi, "V"))));
            }
        }
        if part.through_hole && part.kind.passive() {
            over.push("Through-hole part. An SMD equivalent is cheaper for JLCPCB assembly (no hand-soldering fee) and smaller.".into());
        }

        let any_rating = [s.voltage_v, s.power_w, s.current_a, s.vin_max_v, s.supply_min_v].iter().any(Option::is_some);
        let any_stress = [st.voltage_v, st.power_w, st.current_a, st.vin_v, st.supply_v].iter().any(Option::is_some);
        part.verdict = if !under.is_empty() {
            "under"
        } else if !over.is_empty() {
            "over"
        } else if !part.kind.evaluable() {
            "na"
        } else if any_rating && any_stress {
            "ok"
        } else {
            "unknown"
        }
        .into();
        if part.verdict == "unknown" {
            info.push(
                if any_rating { "The board's voltages around this part are unknown; name rails like +3V3/+5V or set them in fabready.json." } else { "No ratings known. Put the rating in the value or Description field (e.g. \"10uF 16V\"), or ask your agent to review it." }.into(),
            );
        }
        for (id, sev, msg) in under {
            part.notes.push(Note { level: Level::Under, text: msg.clone() });
            let mut f = Finding::new(id, sev, msg).refs(part.refs.clone());
            if let Some(at) = part.at {
                f = f.at(at);
            }
            findings.push(f);
        }
        part.notes.extend(over.into_iter().map(|text| Note { level: Level::Over, text }));
        part.notes.extend(info.into_iter().map(|text| Note { level: Level::Info, text }));
    }
    findings
}

/// Next common capacitor voltage at or above `v`.
fn nice_voltage(v: f64) -> f64 {
    [6.3, 10.0, 16.0, 25.0, 35.0, 50.0, 63.0, 100.0].into_iter().find(|n| *n >= v).unwrap_or(v.ceil())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_values() {
        assert_eq!(parse_resistance("4k7"), Some(4700.0));
        assert_eq!(parse_resistance("10k"), Some(10_000.0));
        assert_eq!(parse_resistance("100R"), Some(100.0));
        assert_eq!(parse_resistance("2R2"), Some(2.2));
        assert_eq!(parse_resistance("1M"), Some(1e6));
        assert_eq!(parse_resistance("47"), Some(47.0));
        assert!((parse_capacitance("100nF 50V").unwrap() - 100e-9).abs() < 1e-15);
        assert!((parse_capacitance("4u7").unwrap() - 4.7e-6).abs() < 1e-12);
        assert_eq!(parse_quantity("1.1V@(800mA)", "V"), Some(1.1));
        assert_eq!(parse_quantity("1/10W", "W"), Some(0.1));
        assert_eq!(parse_quantity("125mW", "W"), Some(0.125));
    }

    #[test]
    fn infers_rails_and_regulators() {
        assert_eq!(net_voltage("+3V3"), Some(3.3));
        assert_eq!(net_voltage("/power/+5V"), Some(5.0));
        assert_eq!(net_voltage("3.3V"), Some(3.3));
        assert_eq!(net_voltage("VBUS"), Some(5.0));
        assert_eq!(net_voltage("BAT+"), Some(4.2));
        assert_eq!(net_voltage("GND"), Some(0.0));
        assert_eq!(net_voltage("ROW0"), None);
        assert_eq!(net_voltage("Net-(D1-A)"), None);
        assert_eq!(regulator_vout("AMS1117-3.3"), Some(3.3));
        assert_eq!(regulator_vout("MCP1700-3302E"), Some(3.3));
        assert_eq!(regulator_vout("XC6206P332MR"), Some(3.3));
        assert_eq!(regulator_vout("LM7805"), Some(5.0));
        assert_eq!(regulator_vout("AMS1117-ADJ"), None);
        assert_eq!(package_of("Resistor_SMD:R_0603_1608Metric").as_deref(), Some("0603"));
        assert_eq!(package_of("Diode_SMD:D_SOD-123").as_deref(), Some("SOD-123"));
    }
}
