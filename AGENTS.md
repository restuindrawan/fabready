# AGENTS.md

fabready reviews KiCad PCBs before ordering, with extra checks that switch on for keyboard boards
(see `docs/keyboards.md`). One Rust engine (`crates/fabready-core`) powers the CLI, the MCP server and the
Tauri desktop app. User docs are in `docs/`: `app.md`, `command-line.md`, `configuration.md`, `parts.md` and
`keyboards.md`.

## Using fabready on a board

Prefer the MCP tools if they are connected (`check_board`, `board_image`, `explain_check`, `review_parts`,
`list_fixes`, `apply_fix`, `undo_fix`; `get_matrix` only for keyboard firmware work). Otherwise use the CLI:

```bash
cargo run -q -p fabready-cli -- check <project-dir|board.kicad_pcb> --json
cargo run -q -p fabready-cli -- matrix <project> --json
cargo run -q -p fabready-cli -- explain <check-id>
```

Exit codes: `0` pass/warn, `1` errors (or warnings with `--strict`), `2` could not read the input.

- Each finding has `id`, `severity` (`error` > `warning` > `info`), `title`, `message`, `fix`, `refs`
  (footprint references like `SW5`) and `at` (board mm, same coordinates as KiCad).
- Fix errors first, then warnings. `info` is context.
- Findings with a `fix_id` have an automatic fix: `list_fixes`, then `apply_fix` with `dry_run: true`, then apply.
  Keep the returned backup path so `undo_fix` can restore it. Fix everything else by editing the design; for wiring
  problems, change the schematic first, then update the PCB. Re-run `check_board` after every change.
- Look at `fabready-out/board.png` (or `board_image`) to sanity-check placement. Marker numbers match finding order.
- `review_parts` (or `fabready parts`) gives each BOM line its ratings, what the board puts on it, and a verdict.
  For `unknown`/`na` parts, judge from datasheet knowledge and say which conclusions are your own judgement.
- Edit the user's KiCad files only when asked. Otherwise report findings with their `fix` text.
- After editing a board that's open in KiCad, tell the user to reload it (File → Revert).
- Waive deliberate exceptions in `fabready.json` next to the board (`"ignore": ["id", "id:REF", "kb.*"]`),
  never by changing fabready. The same file holds the fab profile, limit overrides and power rails
  (see `docs/configuration.md`). The desktop app may have it open in its Settings tab and re-reads it on save.
- Connectivity and zone checks are approximations. When kicad-cli is installed, its DRC is authoritative.
- Inside the desktop app's Agent tab you also get `app_state`, `show_on_board` and `propose_fix`. Each message
  starts with a `<fabready_app>` block saying what the user has open and selected (a finding, or the tracks, vias
  and footprints they selected), which layers are on screen, and edits they made by hand. Point at the findings, parts
  and layers you discuss, and prefer `propose_fix` over `apply_fix` so the user reviews the change on the board.

## Developing

```bash
cargo test --workspace                     # unit, sample-board and MCP end-to-end tests
cargo clippy --workspace --all-targets     # keep warning-free
python3 samples/generate.py                # regenerate sample boards (deterministic)
cd app && npm install && npx tauri dev     # run the desktop app
cd app && npx tauri build --bundles app    # build fabready.app (target/release/bundle/macos/)
```

| Path | What |
|---|---|
| `crates/fabready-core` | Engine: KiCad parser, checks, fixes, parts review, SVG/PNG renderer, report, MCP server, new-project scaffold (`scaffold.rs`), shared viewer (`assets/`) |
| `crates/fabready-cli` | `fabready` binary |
| `app/` | Tauri desktop app (`src-tauri/` Rust commands, `ui/` HTML/CSS/JS, no bundler) |
| `samples/` | Generated boards from `samples/generate.py`: keyboard macropads (clean and buggy) and a sensor board with component problems. The app embeds them for "Try a sample" (`SAMPLES` in `app/src-tauri/src/lib.rs`) |
| `docs/` | User docs. Keep `README.md` short and put details here |

- New checks go in `crates/fabready-core/src/{keyboard,fab,connectivity}.rs`. Register the id in `findings::CATALOG`.
  Add a planted bug to the buggy variant in `samples/generate.py` and an assertion in `crates/fabready-core/tests/samples.rs`.
- Automatic fixes live in `crates/fabready-core/src/fixes.rs`: the check attaches `.fixable("<kind>:...")` to its
  finding, and `fixes::plan` turns that id into byte-range edits of the `.kicad_pcb` (S-expression nodes carry
  spans). Only add fixes that need no design judgement, and keep edits surgical so the rest of the file is untouched.
- Manual edits from the app live in `crates/fabready-core/src/edit.rs` (`edit:<kind>:<id>:<prop>=<value>` incl. relative
  `move=`/`rotate=`, `delete:<kind>:<id>`, `add:track:...`, `add:via:...`). `fixes::plan` hands them over, so they share
  preview, backup and the stale-file guard; `fixes::swap` is undo/redo. Moves drag connected track ends along.
  Item ids are uuids (or `#<n>` file order, see `board::item_id`); the renderer puts them on SVG elements as `data-item`.
- The app's editor (`app/ui/editor.js`: toolbar, tools, multi-select, properties card, ⌘Z) drives the shared canvas
  through hooks in `viewer.js` (`onTool`, `onMarquee`, `canDrag`/`onDrag*`, `snap`, `ghost`, `layer`). The standalone
  viewer passes none of them and stays read-only.
- The renderer draws every board layer as a `<g data-layer>` group (colour via `currentColor`, `display="none"` for layers
  off by default); `render::layer_style` holds the defaults. The viewer's layers panel, bottom view and item picking work
  off those attributes, so keep them when changing `render.rs`.
- Component checks live in `crates/fabready-core/src/parts.rs`: `collect` (ratings + board stress) then `evaluate`
  (findings for under-rated parts, notes for over-spec). Common parts go in `LIBRARY` with typical datasheet values.
  fabready makes no network requests; don't add calls to unofficial or scraped endpoints (respect vendors' terms).
- License hygiene: dependencies must be under a license listed in `about.toml`; run
  `cargo about generate about.hbs -o THIRD_PARTY_LICENSES.html` before shipping binaries (see `THIRD-PARTY-NOTICES.md`).
- The board viewer lives in `crates/fabready-core/assets/viewer.{js,css}`. It is embedded into `viewer.html`
  and served to the app through the `viewer_assets` command, so change it in one place only.
- The app frontend (`app/ui/`) is plain HTML/CSS/JS with no build step. It uses `window.__TAURI__` (`withGlobalTauri`).
  Debug builds embed `app/ui`, so rebuild after editing it unless you're running `tauri dev`.
  Follow `DESIGN.md` for UI changes (labels, text, dialogs, empty states).
- Agent chat: `app/src-tauri/src/agent.rs` is an [ACP](https://agentclientprotocol.com) client (one process per
  agent, one session per project; `opencode acp`, or the official Claude Code and Codex adapters, through `npx`
  when they aren't installed). Each session gets `<app> mcp` with `FABREADY_APP_*` set, which turns on the app tools
  in `mcp.rs`; they reach the window through `bridge.rs` (localhost socket + token) and `app-request` events handled
  in `app/ui/agent.js`. `FABREADY_TEST_PROMPT=1 cargo test -p fabready-app` also sends a real prompt through
  OpenCode when it's installed.
- Project settings: `config::Config` is the `fabready.json` schema. `Config::validate` runs on load and save, and
  `Config::save` keeps unknown keys. The app's Settings tab (`load_config` / `save_config` commands) and Waive
  button write through it; the app passes no profile override, so the file is the single source of truth.
  When adding a setting, add it to the Settings form in `app/ui/app.js` and to `docs/configuration.md`.
- App-wide preferences (not per project) live in `app/ui/settings.js`: one entry in `GROUPS` (menu, with search words)
  and `PAGE_HTML` per page, values in the app's local storage. Document them under "App settings" in `docs/app.md`.
- Setup (apps fabready works with, now just KiCad): detection and installs in `app/src-tauri/src/setup.rs`, the list
  in `app/ui/setup.js`, shown in the first-launch setup window (`app/ui/onboard.js`: Welcome → KiCad → Agent →
  Start) and in Settings → Setup. Installs only run when the user clicks Install, through the platform's package
  manager.
- When a change affects users, update the matching page in `docs/`. Keep `README.md` to the overview,
  quick start, privacy, disclaimer and license.
