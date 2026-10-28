//! Manufacturing (DFM) checks against a fab capability profile.

use std::collections::BTreeMap;

use crate::board::{point_segment_distance, segment_distance, Board, Point, Track};
use crate::config::Rules;
use crate::findings::{Finding, Severity};

fn rule(rules: &Rules, key: &str) -> f64 {
    rules.get(key).copied().unwrap_or(0.0)
}

pub fn check(board: &Board, rules: &Rules) -> Vec<Finding> {
    let mut findings = Vec::new();

    if board.edges.is_empty() {
        findings.push(Finding::new("board.no-outline", Severity::Error, "No shapes on the Edge.Cuts layer."));
    } else if !board.outline_is_closed() {
        findings.push(Finding::new("board.outline-open", Severity::Error, "Edge.Cuts has loose ends; the outline is not closed."));
    }

    if let Some(b) = board.outline_bbox() {
        let (w, h) = (b.2 - b.0, b.3 - b.1);
        let cheap = rules.get("cheap_size").copied().unwrap_or(100.0);
        let note = if w > cheap || h > cheap {
            format!(" Larger than {cheap} x {cheap} mm, which usually costs more at budget fabs.")
        } else {
            String::new()
        };
        findings.push(Finding::new("board.size", Severity::Info, format!("Board is {w:.1} x {h:.1} mm.{note}")));
    }

    let min_track = rule(rules, "min_track");
    let mut thin: BTreeMap<(String, String, String), Vec<&Track>> = BTreeMap::new();
    for t in &board.tracks {
        if t.width < min_track - 1e-6 {
            thin.entry((t.net.clone(), t.layer.clone(), format!("{}", t.width))).or_default().push(t);
        }
    }
    // Report groups in the order their first track appears in the file.
    let mut thin: Vec<_> = thin.into_iter().collect();
    thin.sort_by_key(|(_, ts)| board.tracks.iter().position(|t| std::ptr::eq(t, ts[0])));
    for ((net, layer, width), ts) in thin {
        let count = if ts.len() > 1 { format!(" ({} segments)", ts.len()) } else { String::new() };
        findings.push(
            Finding::new(
                "fab.track-too-thin",
                Severity::Error,
                format!("{width} mm track on {} ({layer}){count}; minimum is {min_track} mm.", or_no_net(&net)),
            )
            .at(ts[0].points[0])
            .fixable(format!("widen-track:{layer}:{width}:{net}")),
        );
    }

    let (min_drill, min_ring) = (rule(rules, "min_via_drill"), rule(rules, "min_annular_ring"));
    for v in &board.vias {
        let fix = format!("resize-via:{}:{}", v.x, v.y);
        if v.drill < min_drill - 1e-6 {
            findings.push(
                Finding::new(
                    "fab.via-drill-too-small",
                    Severity::Error,
                    format!("Via on {} has a {} mm drill; minimum is {min_drill} mm.", or_no_net(&v.net), v.drill),
                )
                .at((v.x, v.y))
                .fixable(fix.clone()),
            );
        }
        let ring = (v.size - v.drill) / 2.0;
        if ring < min_ring - 1e-6 {
            findings.push(
                Finding::new(
                    "fab.via-annular-ring",
                    Severity::Error,
                    format!("Via on {} has a {ring:.3} mm ring; minimum is {min_ring} mm.", or_no_net(&v.net)),
                )
                .at((v.x, v.y))
                .fixable(fix),
            );
        }
    }

    for fp in &board.footprints {
        for p in &fp.pads {
            let Some(drill) = p.drill else { continue };
            let limit = if p.kind == "np_thru_hole" { rule(rules, "min_npth_drill") } else { rule(rules, "min_pth_drill") };
            if drill < limit - 1e-6 {
                findings.push(
                    Finding::new(
                        "fab.hole-too-small",
                        Severity::Error,
                        format!("{} pad {} hole is {drill} mm; minimum is {limit} mm.", fp.reference, p.number),
                    )
                    .refs([fp.reference.clone()])
                    .at(p.center()),
                );
            }
        }
    }

    let edges = board.edge_segments();
    if !edges.is_empty() {
        let limit = rule(rules, "min_edge_clearance");
        let mut near: Vec<(String, f64, Point)> = Vec::new();
        for t in &board.tracks {
            for (a, b) in t.segments() {
                for &(c, d) in &edges {
                    let gap = segment_distance(a, b, c, d) - t.width / 2.0;
                    if gap < limit - 1e-6 {
                        let p = if point_segment_distance(a, c, d) <= point_segment_distance(b, c, d) { a } else { b };
                        match near.iter_mut().find(|n| n.0 == t.net) {
                            Some(n) if gap < n.1 => *n = (t.net.clone(), gap, p),
                            Some(_) => {}
                            None => near.push((t.net.clone(), gap, p)),
                        }
                    }
                }
            }
        }
        for (net, gap, p) in near {
            findings.push(
                Finding::new(
                    "fab.copper-near-edge",
                    Severity::Warning,
                    format!("Track on {} is {:.2} mm from the edge; keep at least {limit} mm.", or_no_net(&net), gap.max(0.0)),
                )
                .at(p),
            );
        }
        for v in &board.vias {
            let gap = edges.iter().map(|&(c, d)| point_segment_distance((v.x, v.y), c, d)).fold(f64::MAX, f64::min) - v.size / 2.0;
            if gap < limit - 1e-6 {
                findings.push(
                    Finding::new(
                        "fab.copper-near-edge",
                        Severity::Warning,
                        format!("Via on {} is {:.2} mm from the edge.", or_no_net(&v.net), gap.max(0.0)),
                    )
                    .at((v.x, v.y)),
                );
            }
        }
    }

    if !board.footprints.is_empty() && !board.footprints.iter().any(|f| f.is_mounting_hole() || f.reference.starts_with('H')) {
        findings.push(Finding::new("board.no-mounting-holes", Severity::Info, "No MountingHole footprints found."));
    }
    findings
}

fn or_no_net(net: &str) -> &str {
    if net.is_empty() {
        "no net"
    } else {
        net
    }
}
