# pcbcheck

Pre-order checks for KiCad PCBs. It ships as a desktop app, a command-line tool, and an MCP server
for AI agents, and all three share one Rust engine.

It checks routing completeness, fab limits (JLCPCB or a "safe" profile), drill and via sizes,
copper-to-edge clearance, board outline and size, and mounting holes. It also reviews components: whether
each part is rated for what the board puts on it, and whether it's bigger or more than the job needs.
If KiCad is installed, its own DRC/ERC results are merged in.

## Desktop app

```bash
cd app && npm install
npx tauri dev                      # run in development
npx tauri build --bundles app      # build pcbcheck.app (target/release/bundle/macos/)
```

The window is laid out like an agent app. The left sidebar lists recent projects, each with a status dot and its
error count. The board sits in the middle, and a resizable inspector on the right has Findings (filterable),
Parts and Board tabs. The app re-checks quietly whenever you save in KiCad. Click a finding to fly to it,
hover a pad to light up its net, and toggle layers from the floating bar. The app follows the macOS light/dark
setting. You can also pass a project on the command line: `pcbcheck-app ~/pcbs/my-board`.

Shortcuts: ⌘O open · ⌘R re-check · ⌘B sidebar · ⌘I inspector · ⌘0 fit board · Esc clear selection.

### Fixing problems

Findings marked with a wrench can be fixed automatically: tracks thinner than the fab allows, undersized vias,
and copper pour over a wireless antenna. **Preview fix** (or ⌘⇧F for all of them) shows the board after the
change, with the edits highlighted and the error count before and after. **Apply** (⌘↩) writes the
`.kicad_pcb`, keeps a backup in `pcbcheck-out/backups/`, and offers Undo. If the board is open in KiCad,
reload it afterwards (File → Revert).

Everything else needs a design change. **Ask agent** copies a prompt for your coding agent (Claude Code, Codex,
Cursor…). With pcbcheck connected over MCP, the agent can apply the automatic fixes itself and edit the KiCad
files for the rest, while the app re-checks after every save.

### Reviewing parts

The Parts tab lists every component line with a verdict:

- **Under-rated:** the part is rated below what the board puts on it. Examples are a 6.3 V capacitor on a
  5 V rail, an LED driven too hard by its resistor, a regulator below its dropout, or a part powered outside
  its supply range. These also appear as findings.
- **Over-spec:** more part than the job needs, such as a 1206 pull-up, a 50 V 1206 capacitor on 3.3 V, a
  through-hole rectifier on a signal line, or a 1 A regulator for a 30 mA load.
- **OK** or **Unknown**, plus connectors and switches, which aren't rated.

Ratings come from the part's value and description ("10uF 16V"), typical package limits, and a built-in
library of common parts. Rail voltages are read from net names (`+3V3`, `VBUS`, `BAT+`…) and regulator
outputs. Clicking a part highlights it on the board.

## Command line

```bash
cargo run -p pcbcheck-cli -- check path/to/project        # report + viewer in path/to/project/pcbcheck-out/
cargo run -p pcbcheck-cli -- parts path/to/project        # component review (under-rated / over-spec)
cargo run -p pcbcheck-cli -- explain conn.unrouted        # what a finding means and how to fix it
cargo install --path crates/pcbcheck-cli                  # install `pcbcheck` on your PATH
```

`check` writes `report.json`, `report.md`, `viewer.html` (interactive, self-contained) and `board.png`.
It exits with 1 when there are errors, or on warnings too with `--strict`.

## AI agents (MCP)

Both `pcbcheck mcp` and `pcbcheck-app mcp` run an MCP server over stdio. Its main tools are `check_board`,
`board_image` (a PNG with markers), `explain_check`, `review_parts` (the component review, for the agent to
judge further), and `list_fixes` / `apply_fix` / `undo_fix` for the automatic fixes. The app's **Connect agents** button shows ready-to-copy config, and the sidebar shows which
agent connected last. For example:

```bash
claude mcp add pcbcheck -- /Applications/pcbcheck.app/Contents/MacOS/pcbcheck-app mcp
```

## Project settings

Put a `pcbcheck.json` next to your board to pick a fab profile, override limits, waive findings, or describe
power rails the net names don't reveal (`"power": {"rails": {"VCC": 3.3}, "current_ma": {"+3V3": 50}}`).
See `pcbcheck.example.json`.

## Docs

- [`docs/keyboards.md`](docs/keyboards.md): extra checks and tools for keyboard PCBs
- [`AGENTS.md`](AGENTS.md): how agents should use pcbcheck, and how to develop it

## Layout

| Path | What |
|---|---|
| `crates/pcbcheck-core` | Engine: KiCad parser, checks, SVG/PNG renderer, report, MCP server, shared viewer (`assets/`) |
| `crates/pcbcheck-cli` | `pcbcheck` binary |
| `app/` | Tauri desktop app (`src-tauri/` Rust, `ui/` HTML/CSS/JS, no bundler) |
| `samples/` | Generated sample boards (`samples/generate.py`): two keyboard macropads (clean and buggy) and a sensor board with component problems |

## Privacy

pcbcheck runs entirely on your machine and makes no network requests. It writes only these files:

- `pcbcheck-out/` next to your board: reports, images, and backups made before automatic fixes.
- `~/.pcbcheck/agent.json`: the name of the last agent that connected over MCP, shown in the app.

## Disclaimer

pcbcheck catches common mistakes, but it doesn't replace your fab's DRC, the parts' datasheets, or your
own review. The fab profiles and part ratings are typical values and may be out of date. Always confirm
with your manufacturer before ordering. KiCad, JLCPCB and other names are trademarks of their owners, and
pcbcheck isn't affiliated with them (see [`THIRD-PARTY-NOTICES.md`](THIRD-PARTY-NOTICES.md)).

## License

[MIT](LICENSE). Third-party components are listed in [`THIRD-PARTY-NOTICES.md`](THIRD-PARTY-NOTICES.md).
