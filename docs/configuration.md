# Configuration

Each board's settings live in `fabready.json`, in the same folder as the `.kicad_pcb`. The file is optional:
without it, fabready uses the JLCPCB profile and its defaults. You can edit it in the desktop app's
**Settings** tab, or by hand. The command line, the MCP server and the app all read the same file.

```json
{
  "profile": "jlcpcb",
  "fab": {
    "min_track": 0.15
  },
  "power": {
    "rails": { "VCC": 3.3 },
    "current_ma": { "+3V3": 50 }
  },
  "keyboard": {
    "key_pitch": 19.05
  },
  "ignore": [
    "board.no-mounting-holes",
    "fab.copper-near-edge:J1"
  ]
}
```

Every key is optional. [`fabready.example.json`](../fabready.example.json) is a starting point.

## Fab profile and limits

`profile` picks the fab limits: `jlcpcb` (the default) or `safe`, a conservative set that suits most fabs.
`fab` overrides individual limits, in mm.

| Key | Meaning | `jlcpcb` | `safe` |
|---|---|---|---|
| `min_track` | Minimum track width | 0.127 | 0.2 |
| `min_via_drill` | Minimum via drill | 0.3 | 0.4 |
| `min_annular_ring` | Minimum via annular ring | 0.13 | 0.15 |
| `min_pth_drill` | Minimum plated hole | 0.3 | 0.4 |
| `min_npth_drill` | Minimum unplated hole | 0.5 | 0.8 |
| `min_edge_clearance` | Minimum copper-to-edge clearance | 0.3 | 0.5 |
| `cheap_size` | Width or height above which budget fabs usually charge more | 100 | 100 |

These are typical "standard price" limits for 2-layer boards. Check them against your fab's current
capabilities. `fabready check --profile safe` overrides the file's profile for one run.

## Power

The parts review needs to know each rail's voltage. Names like `+3V3`, `+5V`, `VBUS` and `BAT+` are read
automatically, as are regulator outputs. Set the rest here:

- `power.rails`: net name → volts, for nets whose names don't say (`"VCC": 3.3`, `"VIN": 12`)
- `power.current_ma`: net name → expected load in mA, used to check that the regulator feeding that rail is
  big enough (`"+3V3": 50`)

Net names must match the board's. In `rails`, the leading `/` of hierarchical nets can be left off.

## Keyboard

Used only on boards with key switches (see [keyboards.md](keyboards.md)).

| Key | Meaning |
|---|---|
| `switch_regex` | Pattern for switch footprints, if yours aren't recognised |
| `controller_regex` | Pattern for the controller module's footprint or value |
| `key_pitch` | Key spacing in mm (default 19.05 for MX, 18 × 17 for Choc) |
| `key_pitch_x`, `key_pitch_y` | Separate horizontal and vertical spacing |

Patterns are case-insensitive regular expressions.

## Waiving findings

`ignore` lists findings you've decided are fine. Each entry is one of:

- `id`: every finding from that check, for example `board.no-mounting-holes`
- `id:REF`: only findings that name that part, for example `fab.copper-near-edge:J1`
- `prefix.*`: every check in a group, for example `kb.*`

Run `fabready explain` for the list of check ids. In the app, a finding's **Waive** button adds the entry for
you.

## Validation

fabready refuses a file with an unknown profile, a limit or key pitch that isn't a positive number, a
negative load, an invalid pattern or an empty `ignore` entry, and reports which value is wrong. The app
checks values before saving. When it saves, it keeps any keys it doesn't know about and leaves out empty
sections.
