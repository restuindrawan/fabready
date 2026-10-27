# AGENTS.md

pcbcheck reviews KiCad PCBs before ordering, with extra checks that switch on for keyboard boards
(see `docs/keyboards.md`). One Rust engine (`crates/pcbcheck-core`) powers the CLI, the MCP server and the
Tauri desktop app.

## Using pcbcheck on a board

Prefer the MCP tools if they are connected (`check_board`, `board_image`, `explain_check`, `review_parts`,
`list_fixes`, `apply_fix`, `undo_fix`; `get_matrix` only for keyboard firmware work). Otherwise use the CLI:

```bash
cargo run -q -p pcbcheck-cli -- check <project-dir|board.kicad_pcb> --json
cargo run -q -p pcbcheck-cli -- matrix <project> --json
cargo run -q -p pcbcheck-cli -- explain <check-id>
```

Exit codes: `0` pass/warn, `1` errors (or warnings with `--strict`), `2` could not read the input.

- Each finding has `id`, `severity` (`error` > `warning` > `info`), `title`, `message`, `fix`, `refs`
  (footprint references like `SW5`) and `at` (board mm, same coordinates as KiCad).
- Fix errors first, then warnings. `info` is context.
- Findings with a `fix_id` have an automatic fix: `list_fixes`, then `apply_fix` with `dry_run: true`, then apply.
  Keep the returned backup path so `undo_fix` can restore it. Fix everything else by editing the design; for wiring
  problems, change the schematic first, then update the PCB. Re-run `check_board` after every change.
- Look at `pcbcheck-out/board.png` (or `board_image`) to sanity-check placement. Marker numbers match finding order.
- `review_parts` (or `pcbcheck parts`) gives each BOM line its ratings, what the board puts on it, and a verdict.
  For `unknown`/`na` parts, judge from datasheet knowledge and say which conclusions are your own judgement.
- Edit the user's KiCad files only when asked. Otherwise report findings with their `fix` text.
- After editing a board that's open in KiCad, tell the user to reload it (File → Revert).
- Waive deliberate exceptions in `pcbcheck.json` next to the board (`"ignore": ["id", "id:REF", "kb.*"]`),
  never by changing pcbcheck.
- Connectivity and zone checks are approximations. When kicad-cli is installed, its DRC is authoritative.

## Developing

```bash
cargo test --workspace                     # unit, sample-board and MCP end-to-end tests
cargo clippy --workspace --all-targets     # keep warning-free
python3 samples/generate.py                # regenerate sample boards (deterministic)
cd app && npx tauri dev                    # run the desktop app
```

- New checks go in `crates/pcbcheck-core/src/{keyboard,fab,connectivity}.rs`. Register the id in `findings::CATALOG`.
  Add a planted bug to the buggy variant in `samples/generate.py` and an assertion in `crates/pcbcheck-core/tests/samples.rs`.
- Automatic fixes live in `crates/pcbcheck-core/src/fixes.rs`: the check attaches `.fixable("<kind>:...")` to its
  finding, and `fixes::plan` turns that id into byte-range edits of the `.kicad_pcb` (S-expression nodes carry
  spans). Only add fixes that need no design judgement, and keep edits surgical so the rest of the file is untouched.
- Component checks live in `crates/pcbcheck-core/src/parts.rs`: `collect` (ratings + board stress) then `evaluate`
  (findings for under-rated parts, notes for over-spec). Common parts go in `LIBRARY` with typical datasheet values.
  pcbcheck makes no network requests; don't add calls to unofficial or scraped endpoints (respect vendors' terms).
- License hygiene: dependencies must be under a license listed in `about.toml`; run
  `cargo about generate about.hbs -o THIRD_PARTY_LICENSES.html` before shipping binaries (see `THIRD-PARTY-NOTICES.md`).
- The board viewer lives in `crates/pcbcheck-core/assets/viewer.{js,css}`. It is embedded into `viewer.html`
  and served to the app through the `viewer_assets` command, so change it in one place only.
- The app frontend (`app/ui/`) is plain HTML/CSS/JS with no build step. It uses `window.__TAURI__` (`withGlobalTauri`).
  Debug builds embed `app/ui`, so rebuild after editing it unless you're running `tauri dev`.
