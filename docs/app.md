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
- **Board:** click a marker to select its finding, click a track, via or footprint to edit it, and hover a pad
  to light up its net. Scroll with two fingers to pan, and pinch or ⌘-scroll to zoom. The toolbar at the bottom
  holds the layers, the view side, the editing tools, undo and zoom (see below). The app follows the macOS
  light/dark setting.
- **Header:** the fab profile (saved to `fabready.json`), re-check, and export. Export writes `report.json`,
  `report.md`, `viewer.html` and `board.png` to `fabready-out/`.
- **Inspector** (resizable), with five tabs:
  - **Agent**: a chat with your coding agent (see below).
  - **Findings**, filterable by severity. Click one to zoom to it on the board.
  - **Parts**: the component review (see [parts.md](parts.md)). Click a part to highlight it.
  - **Board**: size, counts and the fab limits in use.
  - **Settings**: the project's `fabready.json` (see below).

## Layers

**Layers** on the toolbar (or L) opens a list of every layer the board defines: all copper layers including inner ones,
silkscreen, solder mask, paste, courtyard, fabrication, the board outline and user layers. Pads and vias,
zones, labels and issue markers can be turned off the same way. Copper pours show as KiCad last filled them.

- Click a layer to show or hide it, or **Only** (⌥-click) to show just that layer and the outline.
- Hover a layer to fade the others and see what's on it.
- **All**, **Copper**, **Front** and **Back** switch between common sets.
- **Top / Bottom** on the toolbar flips the view. Bottom views the board from underneath: mirrored, with the back layers on top. Labels and issue numbers
  stay readable.

Mask, paste, fabrication and user layers start hidden. The app remembers your choices. The exported
`board.png` and the image agents get show the default set.

## Editing the board

Editing works like Figma: pick a tool from the toolbar at the bottom of the board, change things directly, and
undo with ⌘Z. Every change is written to the `.kicad_pcb` straight away, with a backup in
`fabready-out/backups/`. Only the values that change are touched in the file. The toolbar briefly shows what
changed and the error count afterwards, and the board re-checks.

| Tool | Key | What it does |
|---|---|---|
| Select | V | Click an item to select it, ⇧-click to add or remove, or drag a box on empty board to select everything it touches. Drag footprints and vias to move them. |
| Hand | H | Drag to pan. Holding Space does the same with any tool. |
| Route | X | Click a pad, via or track end to start. Each click adds a 45° bend toward the pointer, and clicking a pad or via of the same net finishes. Double-click or ↩ also finishes, ⌫ removes the last bend, Esc cancels. Press V to drop a via and continue on the other layer, or `/` to flip the bend. The toolbar sets the layer and width. |
| Via | I | Click to place a via. It joins the net of the pad or track it lands on. The toolbar sets its diameter and drill. |
| Measure | M | Click two points. Snaps to pads, vias and track ends, and shows the distance and Δx/Δy. |

Moves snap to a 0.1 mm grid. Hold ⇧ to keep a drag to one axis, or ⌥ to turn snapping off. Tracks that end
on a moved pad or via stretch to follow, like KiCad's Drag. With items selected:

- Arrow keys nudge them 0.1 mm (⇧: 1 mm).
- R rotates footprints 90° (⇧R the other way).
- ⌫ deletes tracks and vias. Footprints can't be deleted here; remove them in the schematic.

The card at the top right shows the selected item's properties. A change saves when you press ↩ or leave the
field, and Esc reverts the field.

| Item | You can change |
|---|---|
| Track | width, copper layer, or delete it |
| Via | diameter, drill, position, or delete it |
| Footprint | reference, value, position, rotation |

With several items selected, the card offers Rotate, Delete and Ask agent for all of them.

⌘Z undoes and ⇧⌘Z redoes, for your edits and for applied automatic fixes alike. Undo refuses when the board
changed elsewhere since (in KiCad or by an agent), so it never throws that work away.

These are quick touch-ups, not a replacement for KiCad. A stretched track can end up running across other pads;
the re-check flags anything that breaks. A track moved to another layer needs vias. A renamed reference or
value goes back to the schematic's on the next Update PCB from Schematic unless you change it there too. If the
board is open in KiCad, reload it (File → Revert) after editing here.

## Fixing problems

Findings marked with a wrench can be fixed automatically: tracks thinner than the fab allows, undersized
vias, and copper pour over a wireless antenna.

1. **Preview fix** (or ⌘⇧F for all of them) shows the board after the change, with the edits highlighted and
   the error and warning counts before and after. Nothing is written yet.
2. **Apply** (⌘↩) writes the `.kicad_pcb`, keeps a backup in `fabready-out/backups/`, and offers Undo.
3. If the board is open in KiCad, reload it (File → Revert).

Everything else needs a design change. **Ask agent** sends the finding to the Agent tab (below), where your
coding agent can fix it while the app re-checks after every save.

## Agent chat

The **Agent** tab (⌘L) chats with Claude Code, Codex or OpenCode beside the board. Pick the agent under
the message box. The agent knows:

- which project is open, its check status, and the finding you've selected (click a marker on the board
  to change it, without leaving the tab)
- the board item you've selected, and which layers you're looking at and from which side
- findings and board items you attach with **Ask agent**, shown as chips above the message
- what you did since its last reply: edited the board by hand, applied or undid a fix, cancelled its
  proposed fix, waived a finding, or changed the settings

It can also act in the app. It can select a finding, highlight parts, switch the visible layers or view the
board from the bottom, and open a fix preview that you apply or cancel as usual. In its replies, part references like `SW5` and phrases like
"finding 3" are links that point at the board. File edits and commands appear as approval cards with the
agent's own choices (allow once, always allow, reject). Model, effort and mode pickers appear once the
agent has started, if it offers them.

Each project keeps its own conversation, and it keeps working while you look at another project. The
compose button next to Send starts a new conversation.

How the app starts each agent:

| Agent | Runs | Signs in with |
|---|---|---|
| Claude Code | `claude-agent-acp` if installed, otherwise `npx @agentclientprotocol/claude-agent-acp` | your Claude Code login or `ANTHROPIC_API_KEY` |
| Codex | `codex-acp` if installed, otherwise `npx @agentclientprotocol/codex-acp` | your Codex login or `OPENAI_API_KEY` |
| OpenCode | `opencode acp` | your OpenCode providers |

The adapters are the official [Agent Client Protocol](https://agentclientprotocol.com) adapters. With `npx`,
the first start downloads the adapter, which needs Node.js. The app looks for the agents on the `PATH` from
your login shell, plus the usual install folders. If an agent needs you to sign in, the chat says how; sign
in from Terminal and press **Try again**.

Every session gets the fabready MCP server, with three extra tools that talk to the open window:
`app_state`, `show_on_board` and `propose_fix`. If you'd rather use your agent in its own terminal or editor,
**Connect agents** shows the MCP setup. **Copy as prompt**, next to attached findings, copies a ready-made
prompt for it.

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
| ⌘L | Open the agent chat |
| ⌘R | Re-check |
| ⌘B | Show or hide the sidebar |
| ⌘I | Show or hide the inspector |
| ⌘0 | Fit the board |
| ⌘+ / ⌘− | Zoom in / out |
| ⌘Z / ⇧⌘Z | Undo / redo board edits |
| V, H, X, I, M | Select, hand, route, via, measure |
| Space (hold) | Pan |
| ⌘⇧F | Preview all automatic fixes |
| ⌘↩ | Apply the previewed fixes |
| ⌘S | Save settings |
| L | Show or hide the layers panel |
| Arrows, R, ⌫ | Nudge, rotate or delete the selected items |
| Esc | Cancel a preview or route, leave a tool, deselect, or clear the selected finding |
