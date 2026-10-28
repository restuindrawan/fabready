# Desktop app

The app checks a KiCad project and shows the problems on the board. It re-checks quietly whenever you save
the board, the schematic or `fabready.json`.

## Install

```bash
cd app && npm install
npx tauri build --bundles app      # fabready.app in target/release/bundle/macos/
```

Open a project with **Open project** (⌘O), by dropping a project folder or board file on the window, or from
the command line: `fabready-app ~/pcbs/my-board`.

## Layout

- **Sidebar:** recent projects, each with a status dot and its error count. At the bottom, **Connect agents**
  and whether KiCad's DRC is available.
- **Board:** click a marker to select its finding, hover a pad to light up its net, and toggle layers from the
  floating bar. The app follows the macOS light/dark setting.
- **Header:** the fab profile (saved to `fabready.json`), re-check, and export. Export writes `report.json`,
  `report.md`, `viewer.html` and `board.png` to `fabready-out/`.
- **Inspector** (resizable), with four tabs:
  - **Findings**, filterable by severity. Click one to zoom to it on the board.
  - **Parts**: the component review (see [parts.md](parts.md)). Click a part to highlight it.
  - **Board**: size, counts and the fab limits in use.
  - **Settings**: the project's `fabready.json` (see below).

## Fixing problems

Findings marked with a wrench can be fixed automatically: tracks thinner than the fab allows, undersized
vias, and copper pour over a wireless antenna.

1. **Preview fix** (or ⌘⇧F for all of them) shows the board after the change, with the edits highlighted and
   the error and warning counts before and after. Nothing is written yet.
2. **Apply** (⌘↩) writes the `.kicad_pcb`, keeps a backup in `fabready-out/backups/`, and offers Undo.
3. If the board is open in KiCad, reload it (File → Revert).

Everything else needs a design change. **Ask agent** copies a prompt for your coding agent (Claude Code,
Codex, Cursor…). With fabready connected over MCP, the agent can apply the automatic fixes and edit the KiCad
files for the rest, while the app re-checks after every save. **Connect agents** shows the setup command and
JSON config, and the sidebar shows which agent connected last.

## Waiving findings

If a finding is deliberate, **Waive** stops reporting it and offers Undo. When the finding names exactly one
part, only that part is waived (for example `conn.unrouted:SW9`). Otherwise the whole check is. Waivers are
saved in `fabready.json` and listed in the Settings tab, where you can remove them.

## Settings

The Settings tab edits the project's `fabready.json`, so the command line and connected agents use the same
settings. It covers:

- **Fab rules:** the profile and each limit. Empty boxes show the profile's value; typing a value overrides it
  and marks the row with a dot.
- **Power rails** and **expected loads**, with net names suggested from the board.
- **Keyboard:** key pitch and the patterns that recognise switch and controller footprints. The section is
  collapsed on boards without key switches.
- **Waived findings.**

Save with **Save** or ⌘S, or discard with **Revert**. Values are checked before saving, and keys the app
doesn't know about are kept. If you edit `fabready.json` elsewhere, the tab picks up the change unless you
have unsaved edits. Every setting is described in [configuration.md](configuration.md).

## Shortcuts

| Keys | Action |
|---|---|
| ⌘O | Open project |
| ⌘R | Re-check |
| ⌘B | Show or hide the sidebar |
| ⌘I | Show or hide the inspector |
| ⌘0 | Fit the board |
| ⌘⇧F | Preview all automatic fixes |
| ⌘↩ | Apply the previewed fixes |
| ⌘S | Save settings |
| Esc | Clear the selection, or cancel a preview |
