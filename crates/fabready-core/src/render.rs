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

fn pad_shape(p: &Pad, fill: &str, class: &str) -> String {
    let title = esc(&format!(
        "{}.{} {} — {}",
        p.fp_ref,
        p.number,
        p.pinfunction,
        if p.net.is_empty() { "no net" } else { &p.net }
    ));
    let net = esc(&p.net);
    if p.shape == "circle" {
        format!(r#"<circle class="{class}" cx="{:.2}" cy="{:.2}" r="{:.2}" fill="{fill}" data-net="{net}"><title>{title}</title></circle>"#, p.x, p.y, p.w / 2.0)
    } else {
        format!(r#"<polygon class="{class}" points="{}" fill="{fill}" data-net="{net}"><title>{title}</title></polygon>"#, pts(&p.polygon()))
    }
}

/// SVG of the board in KiCad coordinates (mm, Y down), with layer groups the viewer can toggle.
/// Colours are presentation attributes (used for PNG export); every element also has a class so
/// the viewer's CSS can re-theme it (light/dark).
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

    for (layer, lid, col) in [("B.Cu", "B", B_CU), ("F.Cu", "F", F_CU)] {
        let side = lid.to_lowercase();
        let _ = writeln!(o, r#"<g id="layer-{lid}" class="copper">"#);
        for z in &board.zones {
            if z.layers.iter().any(|l| l == layer || l == "F&B.Cu" || l == "*.Cu") && z.outline.len() >= 3 {
                let style = if z.keepout {
                    r##"class="keepout" fill="none" stroke="#ffb020" stroke-dasharray="1 0.6" stroke-width="0.2""##.to_string()
                } else {
                    format!(r#"class="zone-{side}" fill="{col}" fill-opacity="0.18" stroke="{col}" stroke-width="0.15""#)
                };
                let _ = writeln!(o, r#"<polygon points="{}" {style}><title>zone {} {layer}</title></polygon>"#, pts(&z.outline), esc(&z.net));
            }
        }
        for t in board.tracks.iter().filter(|t| t.layer == layer) {
            let net = esc(&t.net);
            let _ = writeln!(
                o,
                r#"<polyline class="trk-{side}" points="{}" fill="none" stroke="{col}" stroke-width="{:.3}" stroke-linecap="round" stroke-linejoin="round" data-net="{net}"><title>{net} · {} mm</title></polyline>"#,
                pts(&t.points),
                t.width,
                t.width
            );
        }
        for p in board.pads().filter(|p| p.kind == "smd" && p.on_layer(layer)) {
            o.push_str(&pad_shape(p, col, &format!("pad-{side}")));
            o.push('\n');
        }
        o.push_str("</g>\n");
    }

    o.push_str("<g id=\"layer-T\" class=\"tht\">\n");
    for p in board.pads().filter(|p| p.kind == "thru_hole" || p.kind == "np_thru_hole") {
        if p.kind == "thru_hole" {
            o.push_str(&pad_shape(p, PAD, "pad"));
            o.push('\n');
        }
        if let Some(d) = p.drill {
            let _ = writeln!(o, r#"<circle class="hole" cx="{:.2}" cy="{:.2}" r="{:.2}" fill="{BG}"/>"#, p.x, p.y, d / 2.0);
        }
    }
    for v in &board.vias {
        let net = esc(&v.net);
        let _ = writeln!(
            o,
            r##"<circle class="via" cx="{:.2}" cy="{:.2}" r="{:.2}" fill="#bbb" data-net="{net}"><title>via {net}</title></circle><circle class="hole" cx="{:.2}" cy="{:.2}" r="{:.2}" fill="{BG}"/>"##,
            v.x,
            v.y,
            v.size / 2.0,
            v.x,
            v.y,
            v.drill / 2.0
        );
    }
    o.push_str("</g>\n");

    o.push_str("<g id=\"layer-E\">\n");
    for (poly, _) in &board.edges {
        let _ = writeln!(o, r#"<polyline class="edge" points="{}" fill="none" stroke="{EDGE}" stroke-width="0.25"/>"#, pts(poly));
    }
    o.push_str("</g>\n");

    o.push_str("<g id=\"layer-R\">\n");
    for fp in &board.footprints {
        for shape in &fp.courtyard {
            let dash = if fp.layer.starts_with("B.") { r#" stroke-dasharray="0.6 0.4""# } else { "" };
            let _ = writeln!(o, r#"<polyline class="crt" points="{}" fill="none" stroke="{COURTYARD}" stroke-width="0.08"{dash}/>"#, pts(shape));
        }
        if !fp.reference.is_empty() {
            let (x1, _, x2, _) = fp.bbox();
            let size = ((x2 - x1) / 6.0).clamp(0.8, 2.0);
            let _ = writeln!(
                o,
                r#"<text x="{:.3}" y="{:.3}" font-size="{size:.2}" text-anchor="middle" fill="{REF}" font-family="Menlo, ui-monospace, monospace" opacity="0.85" class="ref" data-ref="{r}">{r}<title>{r} · {} · {} · {}</title></text>"#,
                fp.x,
                fp.y + size * 0.35,
                esc(&fp.value),
                esc(&fp.lib_id),
                fp.layer,
                r = esc(&fp.reference),
            );
        }
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
