//! Draw the board as SVG (with findings overlaid), build the standalone HTML viewer, and rasterise to PNG.

use std::fmt::Write;

use crate::board::{bbox_of, Board, Pad, Point};
use crate::findings::{Finding, Severity};

pub const VIEWER_CSS: &str = include_str!("../assets/viewer.css");
pub const VIEWER_JS: &str = include_str!("../assets/viewer.js");

const BG: &str = "#0b1220";
const BOARD: &str = "#10301f";
const EDGE: &str = "#e6d34a";
const F_CU: &str = "#d9534f";
const B_CU: &str = "#4f8fe0";
const PAD: &str = "#c9a640";
const COURTYARD: &str = "#8a8fa3";
const REF: &str = "#e8e8ee";

fn color(sev: Severity) -> &'static str {
    match sev {
        Severity::Error => "#ff4d4d",
        Severity::Warning => "#ffb020",
        Severity::Info => "#4db8ff",
    }
}

fn pts(points: &[Point]) -> String {
    let mut s = String::with_capacity(points.len() * 14);
    for (i, (x, y)) in points.iter().enumerate() {
        if i > 0 {
            s.push(' ');
        }
        let _ = write!(s, "{x:.2},{y:.2}");
    }
    s
}

pub fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&#x27;")
}

fn pad_shape(p: &Pad, fill: &str, class: &str, item: &str) -> String {
    let title = esc(&format!(
        "{}.{} {} — {}",
        p.fp_ref,
        p.number,
        p.pinfunction,
        if p.net.is_empty() { "no net" } else { &p.net }
    ));
    let net = esc(&p.net);
    if p.shape == "circle" {
        format!(r#"<circle class="{class}" cx="{:.2}" cy="{:.2}" r="{:.2}" fill="{fill}" data-net="{net}"{item}><title>{title}</title></circle>"#, p.x, p.y, p.w / 2.0)
    } else {
        format!(r#"<polygon class="{class}" points="{}" fill="{fill}" data-net="{net}"{item}><title>{title}</title></polygon>"#, pts(&p.polygon()))
    }
}

/// Display defaults for a layer: colour (dark theme; the viewer's CSS re-themes), friendly label, and
/// whether it shows before the user turns it on.
pub fn layer_style(name: &str) -> (&'static str, String, bool) {
    const INNER: [&str; 6] = ["#c2b23a", "#c25bd1", "#3fb8a8", "#d9874a", "#8bb53f", "#7f8fd9"];
    let side = |s: &str| if s.starts_with("F.") { "Front" } else { "Back" };
    match name {
        "F.Cu" => (F_CU, "Front copper".into(), true),
        "B.Cu" => (B_CU, "Back copper".into(), true),
        "Edge.Cuts" => (EDGE, "Board outline".into(), true),
        _ if name.starts_with("In") && name.ends_with(".Cu") => {
            let n: usize = name[2..name.len() - 3].parse().unwrap_or(1);
            (INNER[(n.max(1) - 1) % INNER.len()], format!("Inner copper {n}"), true)
        }
        _ if name.ends_with(".SilkS") => (if name.starts_with("F.") { "#e8e4b8" } else { "#c99bd6" }, format!("{} silkscreen", side(name)), true),
        _ if name.ends_with(".CrtYd") => (COURTYARD, format!("{} courtyard", side(name)), true),
        _ if name.ends_with(".Mask") => (if name.starts_with("F.") { "#b05bd8" } else { "#2fb3a0" }, format!("{} solder mask", side(name)), false),
        _ if name.ends_with(".Paste") => (if name.starts_with("F.") { "#9aa0a6" } else { "#6f8aa0" }, format!("{} paste", side(name)), false),
        _ if name.ends_with(".Fab") => (if name.starts_with("F.") { "#a7a7b0" } else { "#6b70a8" }, format!("{} fabrication", side(name)), false),
        _ if name.ends_with(".Adhes") => ("#8a6dd8", format!("{} adhesive", side(name)), false),
        _ => ("#8f8f8f", name.replace(".User", " (user)").replace("User.", "User "), false),
    }
}

/// Bottom-to-top drawing order, as seen from the front.
fn layer_rank(name: &str) -> i32 {
    let tech = ["Fab", "CrtYd", "Adhes", "Paste", "Mask", "SilkS"];
    let pos = |suffix: &str| tech.iter().position(|t| *t == suffix).map(|i| i as i32);
    match name.split_once('.') {
        Some(("B", "Cu")) => 100,
        Some(("F", "Cu")) => 300,
        Some((inner, "Cu")) if inner.starts_with("In") => 200 - inner[2..].parse::<i32>().unwrap_or(1),
        Some(("B", t)) => pos(t).unwrap_or(500),
        Some(("F", t)) => pos(t).map_or(500, |i| 400 - i),
        _ => 500,
    }
}

/// Does a pad appear on this concrete layer? Handles `*.Cu`, `*.Mask`, `F&B.Cu` and the like.
fn pad_on(p: &Pad, layer: &str) -> bool {
    let (side, kind) = layer.split_once('.').unwrap_or(("", layer));
    p.layers.iter().any(|l| {
        l == layer
            || l.strip_prefix("*.").is_some_and(|k| k == kind)
            || (l.starts_with("F&B.") && l.ends_with(kind) && (side == "F" || side == "B"))
    })
}

fn zone_on(z: &crate::board::Zone, layer: &str) -> bool {
    z.layers.iter().any(|l| l == layer || (l == "*.Cu" && layer.ends_with(".Cu")) || (l == "F&B.Cu" && (layer == "F.Cu" || layer == "B.Cu")))
}

fn text_svg(t: &crate::board::Text, item: &str) -> String {
    // KiCad keeps text readable: anything turned past 90° reads the other way round.
    let mut a = t.angle.rem_euclid(360.0);
    if a > 90.0 && a <= 270.0 {
        a -= 180.0;
    }
    let size = t.size * 1.35;
    let anchor = match t.justify.as_str() {
        "left" => "start",
        "right" => "end",
        _ => "middle",
    };
    let (x, y) = (t.x, t.y);
    let mirror = if t.mirrored { format!(" translate({:.3} 0) scale(-1 1) translate({:.3} 0)", x, -x) } else { String::new() };
    format!(
        r#"<text x="{x:.3}" y="{:.3}" font-size="{size:.2}" text-anchor="{anchor}" fill="currentColor" font-family="Menlo, ui-monospace, monospace" transform="rotate({:.1} {x:.3} {y:.3}){mirror}"{item}>{}</text>"#,
        y + size * 0.36,
        -a,
        esc(&t.text)
    )
}

/// SVG of the board in KiCad coordinates (mm, Y down). Every board layer is a `<g data-layer>` group the
/// viewer can show, hide or reorder; layers that are off by default carry `display="none"` (the PNG export
/// honours it too). Pads & vias (`#layer-T`), labels (`#layer-R`) and finding markers (`#layer-M`) are
/// separate groups. Clickable items carry `data-item="track:<id>"`, `via:<id>` or `fp:<id>`.
/// Colours are presentation attributes (for PNG export) and `currentColor`, so the viewer's CSS can re-theme.
pub fn svg(board: &Board, findings: &[Finding]) -> String {
    let bbox = board.outline_bbox().or_else(|| bbox_of(&board.pads().flat_map(Pad::polygon).collect::<Vec<_>>())).unwrap_or((0.0, 0.0, 100.0, 100.0));
    let m = 4.0;
    let (x0, y0, w, h) = (bbox.0 - m, bbox.1 - m, bbox.2 - bbox.0 + 2.0 * m, bbox.3 - bbox.1 + 2.0 * m);
    let mut o = String::new();
    let _ = writeln!(
        o,
        r#"<svg xmlns="http://www.w3.org/2000/svg" id="board" viewBox="{x0:.3} {y0:.3} {w:.3} {h:.3}" data-home="{x0:.3} {y0:.3} {w:.3} {h:.3}">"#
    );
    // Solid backdrop so rasterised PNGs aren't transparent. Oversized so panning never shows an edge.
    let _ = writeln!(o, r#"<rect class="bg" x="{:.3}" y="{:.3}" width="{:.3}" height="{:.3}" fill="{BG}"/>"#, x0 - 10.0 * w, y0 - 10.0 * h, 21.0 * w, 21.0 * h);

    o.push_str("<g id=\"layer-board\">\n");
    for (poly, closed) in &board.edges {
        if *closed {
            let _ = writeln!(o, r#"<polygon class="board-fill" points="{}" fill="{BOARD}" stroke="none"/>"#, pts(poly));
        }
    }
    o.push_str("</g>\n");

    let fp_item = |reference: &str| -> String {
        board.footprint(reference).map(|f| format!(r#" data-item="fp:{}""#, esc(&f.id))).unwrap_or_default()
    };

    // Every layer that is defined or has something on it.
    let mut names: Vec<String> = board.layers.iter().filter(|l| l.kind != "user" || l.name.ends_with(".Cu")).map(|l| l.name.clone()).collect();
    let used = board
        .tracks
        .iter()
        .map(|t| t.layer.clone())
        .chain(board.graphics.iter().map(|g| g.layer.clone()))
        .chain(board.texts.iter().map(|t| t.layer.clone()))
        .chain(board.zones.iter().flat_map(|z| z.fills.iter().map(|f| f.0.clone())))
        .chain(board.layers.iter().map(|l| l.name.clone()).filter(|n| board.pads().any(|p| p.kind != "np_thru_hole" && pad_on(p, n) && !n.ends_with(".Cu"))));
    for n in used {
        if !n.is_empty() && n != "Edge.Cuts" && !names.contains(&n) {
            names.push(n);
        }
    }
    for must in ["F.Cu", "B.Cu"] {
        if !names.iter().any(|n| n == must) {
            names.push(must.into());
        }
    }
    names.retain(|n| n != "Edge.Cuts");
    names.sort_by_key(|n| layer_rank(n));

    o.push_str("<g id=\"layers\">\n");
    for layer in &names {
        let (col, label, visible) = layer_style(layer);
        let id = match layer.as_str() {
            "F.Cu" => "layer-F".to_string(),
            "B.Cu" => "layer-B".to_string(),
            other => format!("L-{}", other.replace('.', "_")),
        };
        let copper = layer.ends_with(".Cu");
        let side = if layer == "F.Cu" { "f" } else { "b" };
        let _ = writeln!(
            o,
            r#"<g id="{}" class="L{}" data-layer="{}" data-label="{}" color="{col}"{}>"#,
            esc(&id),
            if copper { " copper" } else { "" },
            esc(layer),
            esc(&label),
            if visible { "" } else { r#" display="none""# }
        );
        if copper {
            for z in board.zones.iter().filter(|z| zone_on(z, layer) && z.outline.len() >= 3) {
                if z.keepout {
                    let _ = writeln!(
                        o,
                        r##"<polygon class="zone keepout" points="{}" fill="none" stroke="#ffb020" stroke-dasharray="1 0.6" stroke-width="0.2"><title>keep-out {}</title></polygon>"##,
                        pts(&z.outline),
                        esc(layer)
                    );
                    continue;
                }
                let fills: Vec<&Vec<Point>> = z.fills.iter().filter(|(l, _)| l == layer).map(|(_, p)| p).collect();
                let net = esc(&z.net);
                for f in &fills {
                    let _ = writeln!(o, r#"<polygon class="zone zone-{side}" points="{}" fill="currentColor" fill-opacity="0.38" data-net="{net}"><title>zone {net} {}</title></polygon>"#, pts(f), esc(layer));
                }
                let fill = r#"fill="none""#;
                let _ = writeln!(
                    o,
                    r#"<polygon class="zone zone-{side}" points="{}" {fill} stroke="currentColor" stroke-width="0.15" stroke-dasharray="{}"><title>zone {net} {}</title></polygon>"#,
                    pts(&z.outline),
                    if fills.is_empty() { "none" } else { "0.8 0.5" },
                    esc(layer)
                );
            }
            for t in board.tracks.iter().filter(|t| &t.layer == layer) {
                let net = esc(&t.net);
                let _ = writeln!(
                    o,
                    r#"<polyline class="trk-{side}" points="{}" fill="none" stroke="currentColor" stroke-width="{:.3}" stroke-linecap="round" stroke-linejoin="round" data-net="{net}" data-item="track:{}"><title>{net} · {} mm</title></polyline>"#,
                    pts(&t.points),
                    t.width,
                    esc(&t.id),
                    t.width
                );
            }
        }
        for g in board.graphics.iter().filter(|g| &g.layer == layer) {
            let item = if g.owner.is_empty() { String::new() } else { fp_item(&g.owner) };
            let crt = if layer.ends_with("CrtYd") { r#" class="crt""# } else { "" };
            let dash = if layer.starts_with("B.") && layer.ends_with("CrtYd") { r#" stroke-dasharray="0.6 0.4""# } else { "" };
            let fill = if g.filled { "currentColor" } else { "none" };
            let tag = if g.closed { "polygon" } else { "polyline" };
            let _ = writeln!(
                o,
                r#"<{tag}{crt} points="{}" fill="{fill}" stroke="currentColor" stroke-width="{:.3}" stroke-linecap="round" stroke-linejoin="round"{dash}{item}/>"#,
                pts(&g.points),
                g.width.max(0.02)
            );
        }
        for fp in &board.footprints {
            let item = format!(r#" data-item="fp:{}""#, esc(&fp.id));
            for p in fp.pads.iter().filter(|p| pad_on(p, layer) && (!copper || p.kind == "smd" || p.kind == "connect")) {
                let class = if copper { format!("pad-{side}") } else { "pad-tech".into() };
                o.push_str(&pad_shape(p, "currentColor", &class, &item));
                o.push('\n');
            }
        }
        for t in board.texts.iter().filter(|t| &t.layer == layer) {
            let item = if t.owner.is_empty() { String::new() } else { fp_item(&t.owner) };
            o.push_str(&text_svg(t, &item));
            o.push('\n');
        }
        o.push_str("</g>\n");
    }
    o.push_str("</g>\n");

    o.push_str("<g id=\"layer-T\" class=\"tht\">\n");
    for fp in &board.footprints {
        let item = format!(r#" data-item="fp:{}""#, esc(&fp.id));
        for p in fp.pads.iter().filter(|p| p.kind == "thru_hole" || p.kind == "np_thru_hole") {
            if p.kind == "thru_hole" {
                o.push_str(&pad_shape(p, PAD, "pad", &item));
                o.push('\n');
            }
            if let Some(d) = p.drill {
                let _ = writeln!(o, r#"<circle class="hole" cx="{:.2}" cy="{:.2}" r="{:.2}" fill="{BG}"/>"#, p.x, p.y, d / 2.0);
            }
        }
    }
    for v in &board.vias {
        let net = esc(&v.net);
        let stack: Vec<&str> = board.layers.iter().filter(|l| l.name.ends_with(".Cu")).map(|l| l.name.as_str()).collect();
        let via_layers = if v.layers.len() >= 2 {
            match (stack.iter().position(|l| *l == v.layers[0]), stack.iter().position(|l| *l == v.layers[1])) {
                (Some(a), Some(b)) => stack[a.min(b)..=a.max(b)].join(" "),
                _ => v.layers.join(" "),
            }
        } else if v.layers.is_empty() {
            if stack.is_empty() { "F.Cu B.Cu".to_string() } else { stack.join(" ") }
        } else {
            v.layers.join(" ")
        };
        let _ = writeln!(
            o,
            r##"<circle class="via" cx="{:.2}" cy="{:.2}" r="{:.2}" fill="#bbb" data-net="{net}" data-layers="{}" data-item="via:{}"><title>via {net} · {}/{} mm</title></circle><circle class="hole" cx="{:.2}" cy="{:.2}" r="{:.2}" fill="{BG}"/>"##,
            v.x,
            v.y,
            v.size / 2.0,
            esc(&via_layers),
            esc(&v.id),
            v.size,
            v.drill,
            v.x,
            v.y,
            v.drill / 2.0
        );
    }
    o.push_str("</g>\n");

    let (col, label, _) = layer_style("Edge.Cuts");
    let _ = writeln!(o, r#"<g id="layer-E" class="L" data-layer="Edge.Cuts" data-label="{label}" color="{col}">"#);
    for (poly, _) in &board.edges {
        let _ = writeln!(o, r#"<polyline class="edge" points="{}" fill="none" stroke="currentColor" stroke-width="0.25"/>"#, pts(poly));
    }
    o.push_str("</g>\n");

    o.push_str("<g id=\"layer-R\">\n");
    for fp in board.footprints.iter().filter(|f| !f.reference.is_empty()) {
        let (x1, _, x2, _) = fp.bbox();
        let size = ((x2 - x1) / 6.0).clamp(0.8, 2.0);
        let _ = writeln!(
            o,
            r#"<text x="{:.3}" y="{:.3}" font-size="{size:.2}" text-anchor="middle" fill="{REF}" font-family="Menlo, ui-monospace, monospace" opacity="0.85" class="ref" data-ref="{r}" data-item="fp:{}">{r}<title>{r} · {} · {} · {}</title></text>"#,
            fp.x,
            fp.y + size * 0.35,
            esc(&fp.id),
            esc(&fp.value),
            esc(&fp.lib_id),
            fp.layer,
            r = esc(&fp.reference),
        );
    }
    o.push_str("</g>\n");

    // One marker per location; findings at the same spot share it ("3,4").
    o.push_str("<g id=\"layer-M\">\n");
    let mut spots: Vec<((i64, i64), Vec<usize>)> = Vec::new();
    for (i, f) in findings.iter().enumerate() {
        let (Some(p), false) = (f.point(), f.severity == Severity::Info) else { continue };
        let key = ((p.0 * 10.0).round() as i64, (p.1 * 10.0).round() as i64);
        match spots.iter_mut().find(|s| s.0 == key) {
            Some(s) => s.1.push(i),
            None => spots.push((key, vec![i])),
        }
    }
    for ((kx, ky), idxs) in spots {
        let (x, y) = (kx as f64 / 10.0, ky as f64 / 10.0);
        let worst = idxs.iter().map(|&i| findings[i].severity).min().unwrap_or(Severity::Info);
        let c = color(worst);
        let label = idxs.iter().map(|i| (i + 1).to_string()).collect::<Vec<_>>().join(",");
        let tip = idxs.iter().map(|&i| format!("{}: {}", findings[i].title, findings[i].message)).collect::<Vec<_>>().join(" | ");
        let data = idxs.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(" ");
        let _ = writeln!(
            o,
            r#"<g class="marker" data-idx="{data}"><circle cx="{x:.3}" cy="{y:.3}" r="2.2" fill="{c}" fill-opacity="0.15" stroke="{c}" stroke-width="0.35"/><text x="{:.3}" y="{:.3}" font-size="2" fill="{c}" font-family="Helvetica, Arial, sans-serif" font-weight="700">{label}</text><title>{}</title></g>"#,
            x + 2.6,
            y - 1.6,
            esc(&tip)
        );
    }
    o.push_str("</g></svg>\n");
    o
}

/// Self-contained HTML page: viewer.css + viewer.js + the report and SVG embedded as JSON.
pub fn viewer_html(title: &str, data_json: &str) -> String {
    // Prevent "</script>" inside the data from closing the script tag.
    let data = data_json.replace("</", "<\\/");
    format!(
        "<!doctype html>\n<html lang=\"en\"><head><meta charset=\"utf-8\"><title>fabready · {}</title>\n\
         <meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\n<style>{VIEWER_CSS}</style></head>\n\
         <body class=\"standalone\"><div id=\"viewer\"></div>\n<script>{VIEWER_JS}</script>\n\
         <script>PcbViewer.mount(document.getElementById('viewer'), {data});</script>\n</body></html>\n",
        esc(title)
    )
}

#[cfg(feature = "png")]
pub fn png(svg: &str, width: u32) -> anyhow::Result<Vec<u8>> {
    use resvg::{tiny_skia, usvg};
    let mut opt = usvg::Options::default();
    opt.fontdb_mut().load_system_fonts();
    let tree = usvg::Tree::from_str(svg, &opt)?;
    let size = tree.size();
    let scale = width as f32 / size.width();
    let height = (size.height() * scale).ceil() as u32;
    let mut pixmap = tiny_skia::Pixmap::new(width, height).ok_or_else(|| anyhow::anyhow!("invalid image size"))?;
    resvg::render(&tree, tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    Ok(pixmap.encode_png()?)
}
