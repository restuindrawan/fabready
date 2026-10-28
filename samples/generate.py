"""Generate sample 3x3 nice!nano macropad boards (KiCad 8 format) for trying fabready.

    python3 samples/generate.py

Writes samples/macropad/ (clean) and samples/macropad-buggy/ (with planted mistakes).
The footprints are simplified stand-ins, not production footprints.
"""

from __future__ import annotations

import json
import uuid
from pathlib import Path

HERE = Path(__file__).parent
PITCH = 19.05
KX = [110 + c * PITCH for c in range(3)]
KY = [110 + r * PITCH for r in range(3)]
MOD_X, MOD_Y = 175.0, 118.0
LEFT_X, RIGHT_X = MOD_X - 7.62, MOD_X + 7.62
PIN_Y = [MOD_Y - 13.97 + i * 2.54 for i in range(12)]
LEFT_FUNCS = ["D3", "D2", "GND", "GND", "D1", "D0", "D4", "C6", "D7", "E6", "B4", "B5"]
RIGHT_FUNCS = ["RAW", "GND", "RST", "VCC", "F4", "F5", "F6", "F7", "B1", "B3", "B2", "B6"]
COL_PINS = [4, 5, 6]  # left-side pin index used by COL0..2
ROW_PINS = [7, 8, 9]
COL_LANE_Y = [101.5, 102.2, 102.9]
COL_LANE_X = [164.0, 163.3, 162.6]
ROW_LANE_X = [161.7, 160.3, 161.0]

LAYERS = """  (layers
    (0 "F.Cu" signal) (31 "B.Cu" signal)
    (32 "B.Adhes" user "B.Adhesive") (33 "F.Adhes" user "F.Adhesive")
    (34 "B.Paste" user) (35 "F.Paste" user)
    (36 "B.SilkS" user "B.Silkscreen") (37 "F.SilkS" user "F.Silkscreen")
    (38 "B.Mask" user) (39 "F.Mask" user)
    (40 "Dwgs.User" user "User.Drawings") (41 "Cmts.User" user "User.Comments")
    (42 "Eco1.User" user "User.Eco1") (43 "Eco2.User" user "User.Eco2")
    (44 "Edge.Cuts" user) (45 "Margin" user)
    (46 "B.CrtYd" user "B.Courtyard") (47 "F.CrtYd" user "F.Courtyard")
    (48 "B.Fab" user) (49 "F.Fab" user)
  )"""


class Pcb:
    def __init__(self, name: str):
        self.name = name
        self.nets = [""]
        self.items = []
        self._n = 0

    def uid(self) -> str:
        self._n += 1
        return str(uuid.uuid5(uuid.NAMESPACE_URL, f"{self.name}/{self._n}"))

    def net(self, name: str) -> str:
        if not name:
            return ""
        if name not in self.nets:
            self.nets.append(name)
        return f'(net {self.nets.index(name)} "{name}")'

    def footprint(self, ref, value, lib, x, y, pads, courtyard, layer="F.Cu", rot=0, props=None, attr=None):
        side = layer[0]
        mirror = " (justify mirror)" if side == "B" else ""
        at = f"(at {x:.3f} {y:.3f}{f' {rot}' if rot else ''})"
        body = [f'  (footprint "{lib}" (layer "{layer}") (uuid "{self.uid()}") {at}',
                f'    (property "Reference" "{ref}" (at 0 {courtyard[1] - 1:.2f} {rot}) (layer "{side}.SilkS") (uuid "{self.uid()}") '
                f'(effects (font (size 1 1) (thickness 0.15)){mirror}))',
                f'    (property "Value" "{value}" (at 0 {courtyard[3] + 1:.2f} {rot}) (layer "{side}.Fab") (uuid "{self.uid()}") '
                f'(effects (font (size 1 1) (thickness 0.15)){mirror}))']
        for key, val in (props or {}).items():
            body.append(f'    (property "{key}" "{val}" (at 0 0 0) (layer "{side}.Fab") (hide yes) (uuid "{self.uid()}") '
                        f'(effects (font (size 1 1) (thickness 0.15))))')
        if attr:
            body.append(f"    (attr {attr})")
        body.append(f'    (fp_rect (start {courtyard[0]} {courtyard[1]}) (end {courtyard[2]} {courtyard[3]}) '
                    f'(stroke (width 0.05) (type default)) (fill none) (layer "{side}.CrtYd") (uuid "{self.uid()}"))')
        for num, kind, shape, lx, ly, w, h, drill, net, func in pads:
            if kind == "smd":
                layers = f'"{side}.Cu" "{side}.Paste" "{side}.Mask"'
            else:
                layers = '"*.Cu" "*.Mask"'
            drill_s = f" (drill {drill})" if drill else ""
            net_s = f" {self.net(net)}" if net else ""
            func_s = f' (pinfunction "{func}") (pintype "passive")' if func else ""
            body.append(f'    (pad "{num}" {kind} {shape} (at {lx} {ly}{f" {rot}" if rot else ""}) (size {w} {h}){drill_s} '
                        f'(layers {layers}){net_s}{func_s} (uuid "{self.uid()}"))')
        body.append("  )")
        self.items.append("\n".join(body))

    def track(self, net, layer, *pts, width=0.25):
        for (x1, y1), (x2, y2) in zip(pts, pts[1:]):
            self.items.append(f'  (segment (start {x1:.3f} {y1:.3f}) (end {x2:.3f} {y2:.3f}) (width {width}) '
                              f'(layer "{layer}") {self.net(net)} (uuid "{self.uid()}"))')

    def via(self, net, x, y, size=0.6, drill=0.3):
        self.items.append(f'  (via (at {x} {y}) (size {size}) (drill {drill}) (layers "F.Cu" "B.Cu") {self.net(net)} (uuid "{self.uid()}"))')

    def zone(self, net, layer, pts):
        xy = " ".join(f"(xy {x} {y})" for x, y in pts)
        self.items.append(f'  (zone {self.net(net)} (net_name "{net}") (layer "{layer}") (uuid "{self.uid()}") (hatch edge 0.5) '
                          f'(connect_pads (clearance 0.5)) (min_thickness 0.25) (filled_areas_thickness no) '
                          f'(fill (thermal_gap 0.5) (thermal_bridge_width 0.5)) (polygon (pts {xy})))')

    def rect(self, x1, y1, x2, y2):
        self.items.append(f'  (gr_rect (start {x1} {y1}) (end {x2} {y2}) (stroke (width 0.1) (type default)) (fill none) '
                          f'(layer "Edge.Cuts") (uuid "{self.uid()}"))')

    def text(self) -> str:
        nets = "\n".join(f'  (net {i} "{n}")' for i, n in enumerate(self.nets))
        return (f'(kicad_pcb (version 20240108) (generator "fabready_sample") (generator_version "8.0")\n'
                f'  (general (thickness 1.6) (legacy_teardrops no))\n  (paper "A4")\n{LAYERS}\n'
                f'  (setup (pad_to_mask_clearance 0))\n{nets}\n' + "\n".join(self.items) + "\n)\n")


def build(name: str, buggy: bool) -> str:
    b = Pcb(name)
    b.rect(100, 100, 187, 158)

    # Controller: nice!nano / Pro Micro footprint
    pads = [(str(i + 1), "thru_hole", "circle", -7.62, round(PIN_Y[i] - MOD_Y, 2), 1.7, 1.7, 1.0, "", f) for i, f in enumerate(LEFT_FUNCS)]
    pads += [(str(24 - i), "thru_hole", "circle", 7.62, round(PIN_Y[i] - MOD_Y, 2), 1.7, 1.7, 1.0, "", f) for i, f in enumerate(RIGHT_FUNCS)]
    nets_by_pin = {}
    for c, i in enumerate(COL_PINS):
        if not (buggy and c == 2):
            nets_by_pin[str(i + 1)] = f"COL{c}"
    for r, i in enumerate(ROW_PINS):
        nets_by_pin[str(i + 1)] = f"ROW{r}"
    nets_by_pin.update({"24": "RAW", "23": "GND", "3": "GND", "4": "GND"})
    pads = [(n, k, s, x, y, w, h, d, nets_by_pin.get(n, ""), f) for n, k, s, x, y, w, h, d, _, f in pads]
    b.footprint("U1", "nice!nano", "fabready_samples:nice_nano", MOD_X, MOD_Y, pads, (-9, -16.5, 9, 16.5))

    # Switches + diodes
    n = 0
    for r, ky in enumerate(KY):
        for c, kx in enumerate(KX):
            n += 1
            junction = f"Net-(D{n}-A)"
            no_diode = buggy and n == 9
            sw_pad2_net = f"ROW{r}" if no_diode else junction
            b.footprint(f"SW{n}", "SW_Push", "fabready_samples:SW_MX_1u", kx, ky, [
                ("1", "thru_hole", "circle", -3.81, -2.54, 2.2, 2.2, 1.5, f"COL{c}", "1"),
                ("2", "thru_hole", "circle", 2.54, -5.08, 2.2, 2.2, 1.5, sw_pad2_net, "2"),
                ("", "np_thru_hole", "circle", 0, 0, 4, 4, 4, "", ""),
            ], (-7, -7, 7, 7))
            b.track(sw_pad2_net, "B.Cu", (kx + 2.54, ky - 5.08), (kx + 6, ky - 5.08), (kx + 6, ky + 5), (kx + 1.65, ky + 5))
            if not no_diode:
                reversed_diode = buggy and n == 5
                k_net, a_net = (junction, f"ROW{r}") if reversed_diode else (f"ROW{r}", junction)
                b.footprint(f"D{n}", "1N4148W", "Diode_SMD:D_SOD-123", kx, ky + 5, [
                    ("1", "smd", "roundrect", -1.65, 0, 0.9, 1.2, None, k_net, "K"),
                    ("2", "smd", "roundrect", 1.65, 0, 0.9, 1.2, None, a_net, "A"),
                ], (-2.35, -1, 2.35, 1), layer="B.Cu", rot=180 if reversed_diode else 0)
            b.track(f"ROW{r}", "B.Cu", (kx - 1.65, ky + 5), (kx - 1.65, ky + 7))

    # Rows (back copper) to the controller
    for r, ky in enumerate(KY):
        y = ky + 7
        lane = ROW_LANE_X[r]
        pin_y = PIN_Y[ROW_PINS[r]]
        width = 0.1 if buggy and r == 0 else 0.25
        b.track(f"ROW{r}", "B.Cu", (KX[0] - 1.65, y), (lane, y), (lane, pin_y), (LEFT_X, pin_y), width=width)

    # Columns (front copper) to the controller
    for c, kx in enumerate(KX):
        x = kx - 3.81
        b.track(f"COL{c}", "F.Cu", (x, KY[0] - 2.54), (x, KY[2] - 2.54))
        if buggy and c == 2:
            continue
        lane_y = 100.2 if buggy and c == 0 else COL_LANE_Y[c]
        lane_x, pin_y = COL_LANE_X[c], PIN_Y[COL_PINS[c]]
        b.track(f"COL{c}", "F.Cu", (x, KY[0] - 2.54), (x, lane_y), (lane_x, lane_y), (lane_x, pin_y), (LEFT_X, pin_y))

    # Battery connector, power switch, mounting holes
    b.footprint("J1", "Battery", "fabready_samples:JST_PH_2pin", 176, 150, [
        ("1", "thru_hole", "circle", -1, 0, 1.4, 1.4, 0.8, "BAT+", "+"),
        ("2", "thru_hole", "circle", 1, 0, 1.4, 1.4, 0.8, "GND", "-"),
    ], (-3, -2.5, 3, 2.5))
    b.footprint("SW10", "Power", "fabready_samples:SW_Slide_SPDT", 176, 142, [
        ("1", "thru_hole", "circle", -1.5, 0, 1.2, 1.2, 0.7, "RAW", ""),
        ("2", "thru_hole", "circle", 0, 0, 1.2, 1.2, 0.7, "BAT+", ""),
        ("3", "thru_hole", "circle", 1.5, 0, 1.2, 1.2, 0.7, "", ""),
    ], (-3.5, -1.5, 3.5, 1.5))
    b.track("BAT+", "F.Cu", (175, 150), (175, 146), (176, 146), (176, 142))
    b.track("RAW", "F.Cu", (174.5, 142), (174.5, 139), (178, 139), (178, PIN_Y[0]), (RIGHT_X, PIN_Y[0]))
    b.track("GND", "F.Cu", (177, 150), (180, 150), (180, PIN_Y[1]), (RIGHT_X, PIN_Y[1]))
    for i, y in enumerate((138, 153)):
        b.footprint(f"H{i + 1}", "MountingHole", "MountingHole:MountingHole_2.2mm_M2", 184, y, [
            ("", "np_thru_hole", "circle", 0, 0, 2.2, 2.2, 2.2, "", ""),
        ], (-1.35, -1.35, 1.35, 1.35))

    if buggy:
        b.via("ROW0", ROW_LANE_X[0], 119.5, size=0.45, drill=0.2)
        b.zone("GND", "F.Cu", [(100.5, 100.5), (186.5, 100.5), (186.5, 157.5), (100.5, 157.5)])
    return b.text()


# Chip sizes: (pad offset, pad w, pad h, courtyard half-width)
CHIP = {"0603": (0.8, 0.9, 0.95, 1.5), "0805": (1.0, 1.0, 1.3, 1.7), "1206": (1.5, 1.2, 1.8, 2.3)}


def chip(b, ref, value, lib, size, x, y, net1, net2, funcs=("", ""), props=None):
    off, w, h, cy = CHIP[size]
    b.footprint(ref, value, lib, x, y, [
        ("1", "smd", "roundrect", -off, 0, w, h, None, net1, funcs[0]),
        ("2", "smd", "roundrect", off, 0, w, h, None, net2, funcs[1]),
    ], (-cy, -1.0, cy, 1.0), props=props, attr="smd")


def build_sensor() -> str:
    """USB-powered 3.3 V sensor board with deliberate component problems (wiring is left unrouted)."""
    b = Pcb("sensor-board")
    b.rect(100, 100, 160, 140)
    b.footprint("J1", "Conn_01x02", "Connector_PinHeader_2.54mm:PinHeader_1x02_P2.54mm_Vertical", 105, 110, [
        ("1", "thru_hole", "rect", 0, 0, 1.7, 1.7, 1.0, "VBUS", "Pin_1"),
        ("2", "thru_hole", "circle", 0, 2.54, 1.7, 1.7, 1.0, "GND", "Pin_2"),
    ], (-1.3, -1.3, 1.3, 3.8), attr="through_hole")
    b.footprint("U1", "AMS1117-3.3", "Package_TO_SOT_SMD:SOT-223-3_TabPin2", 118, 111, [
        ("1", "smd", "rect", -2.3, 3.15, 0.95, 2.0, None, "GND", "GND"),
        ("2", "smd", "rect", 0, 3.15, 0.95, 2.0, None, "+3V3", "VO"),
        ("3", "smd", "rect", 2.3, 3.15, 0.95, 2.0, None, "VBUS", "VI"),
        ("2", "smd", "rect", 0, -3.15, 3.25, 2.0, None, "+3V3", "VO"),
    ], (-4.0, -4.6, 4.0, 4.6), props={"LCSC": "C6186"}, attr="smd")
    chip(b, "C1", "10uF 6.3V", "Capacitor_SMD:C_0805_2012Metric", "0805", 110, 120, "VBUS", "GND")
    chip(b, "C2", "22uF 10V", "Capacitor_SMD:C_0805_2012Metric", "0805", 126, 120, "+3V3", "GND")
    chip(b, "C3", "100nF 50V", "Capacitor_SMD:C_1206_3216Metric", "1206", 132, 120, "+3V3", "GND")
    chip(b, "R1", "1k", "Resistor_SMD:R_0603_1608Metric", "0603", 138, 110, "+3V3", "Net-(D1-A)")
    chip(b, "D1", "RED", "LED_SMD:LED_0603_1608Metric", "0603", 144, 110, "GND", "Net-(D1-A)", ("K", "A"))
    chip(b, "R2", "47", "Resistor_SMD:R_0603_1608Metric", "0603", 138, 116, "VBUS", "Net-(D2-A)")
    chip(b, "D2", "BLUE", "LED_SMD:LED_0603_1608Metric", "0603", 144, 116, "GND", "Net-(D2-A)", ("K", "A"))
    chip(b, "R3", "10k", "Resistor_SMD:R_1206_3216Metric", "1206", 138, 124, "+3V3", "SDA")
    b.footprint("U2", "WS2812B", "LED_SMD:LED_WS2812B_PLCC4_5.0x5.0mm_P3.2mm", 120, 130, [
        ("1", "smd", "rect", -2.45, -1.6, 1.5, 1.0, None, "+3V3", "VDD"),
        ("2", "smd", "rect", -2.45, 1.6, 1.5, 1.0, None, "", "DOUT"),
        ("3", "smd", "rect", 2.45, 1.6, 1.5, 1.0, None, "GND", "VSS"),
        ("4", "smd", "rect", 2.45, -1.6, 1.5, 1.0, None, "LED_DIN", "DIN"),
    ], (-3.5, -3.0, 3.5, 3.0), attr="smd")
    b.footprint("D3", "1N4007", "Diode_THT:D_DO-41_SOD81_P10.16mm_Horizontal", 135, 132, [
        ("1", "thru_hole", "rect", 0, 0, 2.2, 2.2, 1.1, "SDA", "K"),
        ("2", "thru_hole", "oval", 10.16, 0, 2.2, 2.2, 1.1, "INT", "A"),
    ], (-1.5, -1.8, 11.7, 1.8), attr="through_hole")
    b.footprint("H1", "MountingHole", "MountingHole:MountingHole_3.2mm_M3", 155, 135, [
        ("", "np_thru_hole", "circle", 0, 0, 3.2, 3.2, 3.2, "", ""),
    ], (-1.85, -1.85, 1.85, 1.85))
    return b.text()


def main() -> None:
    for name, buggy in (("macropad", False), ("macropad-buggy", True)):
        d = HERE / name
        d.mkdir(exist_ok=True)
        (d / f"{name}.kicad_pcb").write_text(build(name, buggy))
        (d / f"{name}.kicad_pro").write_text(json.dumps({"meta": {"filename": f"{name}.kicad_pro", "version": 1}}, indent=2) + "\n")
        print(f"wrote {d / (name + '.kicad_pcb')}")
    d = HERE / "sensor-board"
    d.mkdir(exist_ok=True)
    (d / "sensor-board.kicad_pcb").write_text(build_sensor())
    (d / "sensor-board.kicad_pro").write_text(json.dumps({"meta": {"filename": "sensor-board.kicad_pro", "version": 1}}, indent=2) + "\n")
    # The sample is about components, so routing findings are waived; the 3.3 V load sizes the regulator.
    (d / "fabready.json").write_text(json.dumps({"power": {"current_ma": {"+3V3": 30}}, "ignore": ["conn.*"]}, indent=2) + "\n")
    print(f"wrote {d / 'sensor-board.kicad_pcb'}")


if __name__ == "__main__":
    main()
