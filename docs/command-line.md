# Command line and MCP

## Install

```bash
cargo install --path crates/fabready-cli      # puts `fabready` on your PATH
```

Or run it from the repository without installing: `cargo run -p fabready-cli -- <command>`.

Every command that takes a path accepts a project directory, a `.kicad_pro`, a `.kicad_sch` or a `.kicad_pcb`.

## Commands

| Command | What it does |
|---|---|
| `fabready check <path>` | Run every check and write the report files |
| `fabready parts <path>` | Review components: ratings against what the board puts on them (see [parts.md](parts.md)) |
| `fabready matrix <path>` | Keyboard boards: the switch matrix and controller pins, for ZMK/QMK (see [keyboards.md](keyboards.md)) |
| `fabready explain [id]` | Why a check matters and how to fix it. Without an id, lists every check |
| `fabready doctor` | Show which tools fabready found (kicad-cli) |
| `fabready mcp` | Run as an MCP server over stdio |

### `check` options

| Option | Effect |
|---|---|
| `--profile <jlcpcb\|safe>` | Use this fab profile instead of the one in `fabready.json` |
| `--config <file>` | Read settings from this file instead of `fabready.json` next to the board |
| `--out <dir>` | Write output files here (default: `fabready-out/` next to the board) |
| `--json` | Print the full report as JSON |
| `--strict` | Exit with 1 on warnings too |
| `--no-kicad` | Skip KiCad's DRC/ERC even if kicad-cli is installed |
| `--no-png` | Skip the PNG and 3D renders |
| `-v`, `--verbose` | Also print info findings |

`parts` takes `--profile`, `--config` and `--json`. `matrix` takes `--config` and `--json`.

## Output files

`check` writes these to `fabready-out/`:

- `report.json`: the full report (the same data as `--json`)
- `report.md`: a readable summary
- `viewer.html`: an interactive, self-contained board viewer
- `board.svg` and `board.png`: the board with numbered markers that match the finding order
- KiCad's DRC/ERC reports and 3D renders, when kicad-cli is installed

## Findings

Each finding has an `id`, a `severity` (`error`, `warning` or `info`), a `title`, a `message`, a `fix`,
`refs` (footprint references such as `SW5`) and `at` (board coordinates in mm, the same as KiCad's). Findings
with a `fix_id` can be fixed automatically. To stop reporting a finding, waive it in `fabready.json` (see
[configuration.md](configuration.md#waiving-findings)).

## Exit codes

| Code | Meaning |
|---|---|
| `0` | Passed, or warnings only |
| `1` | Errors found (or warnings, with `--strict`). For `parts`, an under-rated part |
| `2` | The input couldn't be read |

## MCP server

Both `fabready mcp` and the desktop app's `fabready-app mcp` run an MCP server over stdio.

| Tool | What it does |
|---|---|
| `check_board` | Run every check and return the findings |
| `board_image` | A PNG of the board with numbered markers that match the findings |
| `explain_check` | Why a check matters and how to fix it |
| `review_parts` | The component review, for the agent to judge further |
| `list_fixes` | Automatic fixes available, and which findings each resolves |
| `apply_fix` | Apply fixes to the `.kicad_pcb` (use `dry_run` first). Returns a backup path |
| `undo_fix` | Restore a backup made by `apply_fix` |
| `get_matrix` | Keyboard boards: the switch matrix and controller pins, for firmware work |

When the desktop app starts the server for its own agent chat, it adds three tools that act in the app
window: `app_state` (what the user has open and selected, the layers on screen, and edits they made by hand),
`show_on_board` (select a finding, highlight parts, show only some layers, or view from the bottom) and
`propose_fix` (open a fix preview for the user to apply). They talk to the window over a
localhost socket with a per-launch token, set through `FABREADY_APP_ADDR`, `FABREADY_APP_TOKEN` and
`FABREADY_APP_PROJECT`. Other MCP clients don't get them.

### Setup

Claude Code:

```bash
claude mcp add fabready -- fabready mcp
# or, using the desktop app:
claude mcp add fabready -- /Applications/fabready.app/Contents/MacOS/fabready-app mcp
```

Codex:

```bash
codex mcp add fabready -- fabready mcp
```

Claude Desktop, Cursor and other MCP clients:

```json
{
  "mcpServers": {
    "fabready": { "command": "fabready", "args": ["mcp"] }
  }
}
```

The app's **Settings** (Agents page) shows these, filled in with the app's own path. How agents should use
these tools is described in [AGENTS.md](../AGENTS.md).
