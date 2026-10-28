# Keyboard PCBs

fabready switches on extra checks when it finds key switch footprints on a board. Boards without key switches
never see these findings.

## What gets checked

| Check | Severity | What it catches |
|---|---|---|
| `kb.missing-diode` | error | A switch wired straight to the matrix with no diode, which causes ghost key presses |
| `kb.diode-direction-mixed` | error | A diode facing the opposite way to the rest, so that key never registers |
| `kb.duplicate-matrix-position` | error | Two keys on the same row and column |
| `kb.switch-unconnected` | error | A switch (or its diode) with a pin wired to nothing |
| `kb.line-not-at-controller` | error | A row or column that never reaches the controller |
| `kb.line-on-power-pin` | error | A row or column wired to a power or reset pin |
| `kb.too-many-lines` | error | More rows plus columns than the controller has GPIO pins |
| `kb.switch-overlap` | error | Switch centres closer than the key pitch (19.05 mm MX, 18 × 17 mm Choc) |
| `kb.no-controller` | warning | No recognised MCU module (nice!nano, XIAO, Pro Micro, RP2040, ESP32, …) |
| `kb.copper-under-antenna` | warning | A copper pour overlapping a wireless module's antenna |
| `kb.antenna-not-at-edge` | warning | Wireless module more than 5 mm from the board edge |
| `kb.no-battery` | warning | Wireless board with no battery net or connector |
| `kb.no-power-switch` | info | Battery present but no power slide switch |
| `kb.no-reset-button` | info | Controller RST pin not wired to a button |
| `kb.matrix-summary` | info | Key count, matrix size, diode direction and GPIO usage |

Run `fabready explain <id>` for the reasoning and fix behind each one.

## How boards are recognised

- **Switches:** footprint library names containing MX, Choc, Kailh, Gateron, Alps, Hotswap, PG1350 or Keyboard.
  Tactile/reset buttons (Push, Tact, SPST, TL33xx, EVQP) are excluded.
- **Diodes:** references starting with `D` that have two pads and aren't LEDs. The cathode is the pad with
  pin function `K`, or pad 1 (KiCad's convention).
- **Controller:** footprint or value matching nice!nano, XIAO, Pro Micro, nRF52, E73, Holyiot, MDBT, RP2040,
  Pico, Elite-C, ESP32 and similar. Wireless modules also get the antenna, battery and power-switch checks.
- **Rows vs columns:** net names containing `ROW`/`COL` let fabready name the firmware diode direction
  (`col2row` or `row2col`).

If your footprints use other names, override the patterns in `fabready.json` (or in the app's Settings tab;
see [configuration.md](configuration.md#keyboard)):

```json
{
  "keyboard": {
    "switch_regex": "(my_switch_lib|SW_Hotswap)",
    "controller_regex": "(my_mcu_module)",
    "key_pitch": 19.05
  },
  "ignore": ["kb.no-reset-button"]
}
```

`key_pitch_x` / `key_pitch_y` set different horizontal and vertical spacing.

## Matrix and firmware pins

Run these when you're writing ZMK or QMK config, not as part of every check:

```bash
fabready matrix path/to/project          # grid of keys, diode direction, controller pin per row/column
fabready matrix path/to/project --json
```

Agents get the same data from the `get_matrix` MCP tool: keys, row/column nets, the controller pin for
each line, `diode_direction`, and the GPIO budget.

## Samples

`samples/macropad` (clean) and `samples/macropad-buggy` (planted mistakes) are 3×3 nice!nano macropads made by
`python3 samples/generate.py`. The buggy one plants a missing diode, a backwards diode, a column that never
reaches the controller, and a ground pour under the antenna, along with general routing and fab mistakes.
