//! Finding model and the catalog of every check pcbcheck can report.

use serde::{Deserialize, Serialize};

use crate::board::Point;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
    Info,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Info => "info",
        }
    }
}

/// (id, title, why it matters, how to fix)
pub const CATALOG: &[(&str, &str, &str, &str)] = &[
    ("kb.switch-unconnected", "Key switch is not wired",
     "A switch with fewer than two connected pins can never register a key press.",
     "Wire both switch pins in the schematic, then Update PCB from Schematic (F8)."),
    ("kb.missing-diode", "Key switch has no diode",
     "Without a diode per key, pressing 3+ keys at once causes ghost key presses.",
     "Add one diode in series with this switch (switch pin -> diode -> row/column)."),
    ("kb.diode-direction-mixed", "Diode is backwards compared to the others",
     "All matrix diodes must point the same way or those keys will never register.",
     "Rotate this diode 180 degrees in the schematic/PCB so its cathode (stripe) faces the same line as the rest."),
    ("kb.duplicate-matrix-position", "Two keys share the same row and column",
     "The firmware cannot tell these keys apart.",
     "Move one of the keys to an unused row/column combination."),
    ("kb.line-not-at-controller", "Row/column line never reaches the controller",
     "The firmware cannot scan a matrix line that is not connected to a GPIO pin.",
     "Connect this net to a free GPIO pin on the controller module."),
    ("kb.line-on-power-pin", "Row/column line is connected to a power/reset pin",
     "Matrix lines must go to GPIO pins; power or reset pins will short or reset the board.",
     "Move this net to a free GPIO pin."),
    ("kb.too-many-lines", "Not enough GPIO pins for this matrix",
     "The matrix needs more row+column lines than the controller has I/O pins.",
     "Rearrange the matrix (closer to square), or use a controller with more pins."),
    ("kb.no-controller", "No controller module found",
     "pcbcheck could not find an MCU module footprint (nice!nano, XIAO, Pro Micro, ...).",
     "Add the controller footprint, or set keyboard.controller_regex in pcbcheck.json."),
    ("kb.switch-overlap", "Key switches are too close together",
     "Keycaps will collide if switches are closer than the key pitch.",
     "Space switch centres at least 19.05 mm apart (MX) or 18 mm x 17 mm (Choc)."),
    ("kb.copper-under-antenna", "Copper pour overlaps the wireless module",
     "Ground pour under a Bluetooth antenna can cut range drastically.",
     "Keep copper (all layers) away from the module's antenna end; add a keep-out rule area there."),
    ("kb.antenna-not-at-edge", "Wireless module is far from the board edge",
     "Antennas surrounded by copper/switches/case have poor range.",
     "Place the module so its antenna end sits at a board edge."),
    ("kb.no-battery", "No battery connection found",
     "A wireless board needs a battery connector or pads.",
     "Add a JST-PH connector or battery pads wired to the module's B+/BAT pin."),
    ("kb.no-power-switch", "No power switch found",
     "Without a switch the battery drains whenever the keyboard is idle.",
     "Add a slide switch (e.g. MSK12C02) between the battery and the module's B+/RAW pin."),
    ("kb.no-reset-button", "No reset button found",
     "A reset button makes flashing firmware much easier.",
     "Add a small tactile switch between the module's RST and GND pins."),
    ("kb.matrix-summary", "Matrix summary", "Derived switch matrix information.", ""),
    ("conn.not-routed", "Board has no tracks yet", "Nothing is wired on the PCB.",
     "Route the board, or use the autorouter (Freerouting)."),
    ("conn.unrouted", "Net is not fully routed",
     "Some pads on this net are not connected by copper, so the circuit is incomplete.",
     "Finish routing this net (look for ratsnest lines in KiCad)."),
    ("conn.footprint-no-nets", "Footprint has no connections",
     "None of its pads are connected; usually the PCB was not updated from the schematic.",
     "In the PCB editor run Tools > Update PCB from Schematic (F8)."),
    ("conn.zone-approx", "Zone connectivity is approximate",
     "pcbcheck treats zone outlines as filled copper; real fills can be broken up by tracks.",
     "Install KiCad so `kicad-cli pcb drc` can verify zone fills."),
    ("fab.track-too-thin", "Track is thinner than the fab allows",
     "The factory cannot reliably make tracks this thin.", "Increase the track width."),
    ("fab.via-drill-too-small", "Via drill too small",
     "The factory cannot drill holes this small at standard pricing.", "Increase the via drill size."),
    ("fab.via-annular-ring", "Via annular ring too small",
     "The copper ring around the via hole is too thin and may break.", "Increase the via diameter or reduce the drill."),
    ("fab.hole-too-small", "Hole too small", "Pad drill is below the fab's minimum.", "Use a footprint with a larger hole."),
    ("fab.copper-near-edge", "Copper too close to the board edge",
     "Copper this close to the edge can be cut or exposed during routing.", "Move the track/via further from the board outline."),
    ("board.no-outline", "No board outline", "The factory needs a closed outline on the Edge.Cuts layer.",
     "Draw the board shape on Edge.Cuts."),
    ("board.outline-open", "Board outline is not closed", "An open outline makes the board shape ambiguous to the factory.",
     "Make sure every Edge.Cuts line connects end-to-end."),
    ("board.size", "Board size", "Board dimensions.", ""),
    ("board.no-mounting-holes", "No mounting holes", "Most boards need holes or standoffs to mount in a case or enclosure.",
     "Add MountingHole footprints matching your case."),
    ("kicad.missing", "KiCad CLI not found", "KiCad's own DRC/ERC were skipped.",
     "Install KiCad (brew install --cask kicad) for full checks."),
    ("kicad.failed", "KiCad CLI failed", "kicad-cli returned an error.", "Run the printed command manually to see the details."),
    // Parts
    ("part.voltage-exceeded", "Part rated below the voltage it sees",
     "Capacitors, diodes and transistors fail (sometimes violently) above their rated voltage.",
     "Use a part with a higher voltage rating."),
    ("part.voltage-margin", "Not enough voltage margin",
     "Ceramic capacitors lose capacitance near their rating, and spikes can exceed it; diodes and MOSFETs need headroom too.",
     "Pick the next voltage rating up (e.g. 10 V or 16 V for a 5 V rail)."),
    ("part.power-exceeded", "Resistor overloaded",
     "A resistor dissipating more than its rating overheats and drifts or burns.",
     "Use a larger package (0805/1206) or a higher resistance."),
    ("part.power-margin", "Resistor runs hot",
     "Running a resistor above half its rating shortens its life and heats nearby parts.",
     "Use the next package size up, or a higher resistance if the circuit allows."),
    ("part.led-current", "LED current too high",
     "Too little series resistance overdrives the LED, shortening its life or killing it.",
     "Increase the series resistor: R = (supply − LED forward voltage) / current (5–10 mA is plenty for indicators)."),
    ("part.regulator-input", "Regulator input above its maximum",
     "The regulator will be damaged by its own supply.",
     "Use a regulator rated for the input voltage."),
    ("part.regulator-dropout", "Regulator can't regulate at this input",
     "A regulator needs its dropout voltage of headroom; below that, the output sags with the input.",
     "Use a low-dropout (LDO) regulator, or raise the input voltage."),
    ("part.regulator-current", "Regulator too small for the load",
     "It will overheat or shut down under the expected load.",
     "Use a regulator rated above the load current (with margin), or a switching regulator."),
    ("part.supply-range", "Part powered outside its supply range",
     "The part may not work reliably (or at all) outside its supply range.",
     "Power it from a rail within its range, or choose a part rated for this rail."),
];

pub fn catalog_entry(id: &str) -> Option<(&'static str, &'static str, &'static str)> {
    CATALOG.iter().find(|e| e.0 == id).map(|e| (e.1, e.2, e.3))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finding {
    pub id: String,
    pub severity: Severity,
    pub title: String,
    pub message: String,
    pub fix: String,
    pub refs: Vec<String>,
    pub at: Option<At>,
    pub source: String,
    /// Set when pcbcheck can fix this automatically (see `fixes`). Shared by findings one fix resolves.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fix_id: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct At {
    pub x: f64,
    pub y: f64,
}

impl Finding {
    pub fn new(id: &str, severity: Severity, message: impl Into<String>) -> Self {
        let (title, fix) = match catalog_entry(id) {
            Some((t, _, f)) => (t.to_string(), f.to_string()),
            None => (id.to_string(), String::new()),
        };
        Finding {
            id: id.to_string(),
            severity,
            title,
            message: message.into(),
            fix,
            refs: Vec::new(),
            at: None,
            source: "pcbcheck".into(),
            fix_id: None,
        }
    }

    pub fn fixable(mut self, fix_id: impl Into<String>) -> Self {
        self.fix_id = Some(fix_id.into());
        self
    }

    pub fn refs<I: IntoIterator<Item = S>, S: Into<String>>(mut self, refs: I) -> Self {
        self.refs = refs.into_iter().map(Into::into).collect();
        self
    }

    pub fn at(mut self, p: Point) -> Self {
        self.at = Some(At { x: (p.0 * 1000.0).round() / 1000.0, y: (p.1 * 1000.0).round() / 1000.0 });
        self
    }

    pub fn source(mut self, s: &str) -> Self {
        self.source = s.into();
        self
    }

    pub fn point(&self) -> Option<Point> {
        self.at.map(|a| (a.x, a.y))
    }
}
