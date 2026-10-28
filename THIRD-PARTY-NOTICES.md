# Third-party notices

fabready is MIT-licensed (see `LICENSE`). It depends on open-source Rust crates, all under permissive
licenses (MIT, Apache-2.0, BSD-2/3-Clause, ISC, Zlib, Unicode-3.0, CC0-1.0, Unlicense, 0BSD), plus a
few under the Mozilla Public License 2.0.

## MPL-2.0 components

These come in only through Tauri, so they're in the desktop app but not the `fabready` CLI. fabready
uses them unmodified. Their source code is available from crates.io and the links below.

| Crate | Source |
|---|---|
| cssparser, cssparser-macros | https://github.com/servo/rust-cssparser |
| selectors | https://github.com/servo/stylo |
| dtoa-short | https://github.com/upsuper/dtoa-short |
| option-ext | https://github.com/soc/option-ext |

## Binary releases

The MIT, Apache-2.0, BSD and Unicode licenses require their notices to ship with binaries. Before
publishing a release, generate the full license texts with
[cargo-about](https://github.com/EmbarkStudios/cargo-about) and include the file with the release
(for the app, as a bundled resource):

```bash
cargo install cargo-about --locked --features cli
cargo about generate about.hbs -o THIRD_PARTY_LICENSES.html
```

`about.toml` lists the licenses we accept. `cargo about` fails if a dependency uses anything else,
so it also works as a license check.

## Build tools (not distributed)

`app/node_modules` holds `@tauri-apps/cli` (MIT OR Apache-2.0). It's only used to build the app and
isn't included in it.

## Trademarks and data

KiCad, JLCPCB, LCSC, nice!nano, Seeed XIAO, Cherry MX, Kailh, ZMK, QMK and other product names are
trademarks of their respective owners. They're mentioned only to describe compatibility. fabready isn't
affiliated with or endorsed by any of them.

fabready makes no network requests. The fab capability profiles and the built-in part ratings are
typical values written from public datasheets and capability pages. They aren't copies of anyone's
database, and they can be out of date, so always confirm with your manufacturer and the part's datasheet.
