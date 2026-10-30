# Design notes

How fabready's desktop app and board viewer should look and read. Colours, type and spacing come from the tokens
at the top of `crates/fabready-core/assets/viewer.css` (shared by the app and the standalone viewer).
Follow these when you add or change UI.

## Labels and headings

- **No all-caps eyebrows.** Section labels, group headings and step counters ("2 of 4") are sentence case, never
  `text-transform: uppercase` or letter-spaced small caps. Use the existing section label style: 12px, weight 600,
  `var(--dim)` (see `.pcbv-group`).
- Write headings and labels in sentence case ("Fab for new boards", not "Fab For New Boards").

## Text

- **Keep supporting text to a minimum.** If a title or button label says enough, leave the description out.
  Start buttons and cards are icon and title only, and the welcome screen has no intro paragraph or footnote.
- When a description is needed, keep it to one short line under the title, in `var(--dim)`.

## Windows and dialogs

- Prefer a centered modal over a full-window takeover.
- Close with a plain ✕ icon button in the top-right corner (`.icon-btn`), plus Esc. No circular close
  buttons or "ESC" captions.
- Settings-style windows: a search box and a grouped menu on the left, the page on the right. Each setting is a
  title with its control, an optional one-line description, and a divider.

## Empty states

- When nothing is open, hide chrome that has nothing to show (for example the header's title and divider), and
  let the empty state blend into the background.
