use std::path::PathBuf;

use fabready_core::report::{self, Options};
use fabready_core::Severity;

fn sample(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../samples").join(name)
}

fn run(name: &str) -> fabready_core::CheckResult {
    let opts = Options { use_kicad: false, ..Default::default() };
    report::run(&sample(name), &opts).expect("check runs")
}

#[test]
fn clean_board_passes() {
    let r = run("macropad").report;
    let problems: Vec<_> = r.findings.iter().filter(|f| f.severity != Severity::Info).map(|f| &f.message).collect();
    assert_eq!(r.status, "pass", "{problems:?}");
    let m = r.matrix.expect("matrix");
    assert_eq!(m.diode_direction.as_deref(), Some("col2row"));
    assert_eq!(m.pins["ROW0"], "U1 pad 8 (C6)");
    assert_eq!(m.gpio_available, Some(18));
    assert_eq!(m.keys.len(), 9);
}

#[test]
fn buggy_board_catches_planted_mistakes() {
    let r = run("macropad-buggy").report;
    assert_eq!(r.status, "fail");
    let has = |id: &str, refs: &[&str]| r.findings.iter().any(|f| f.id == id && f.refs == refs);
    assert!(has("kb.diode-direction-mixed", &["D5", "SW5"]));
    assert!(has("kb.missing-diode", &["SW9"]));
    for id in [
        "kb.line-not-at-controller",
        "kb.copper-under-antenna",
        "fab.track-too-thin",
        "fab.via-drill-too-small",
        "fab.via-annular-ring",
        "fab.copper-near-edge",
        "conn.unrouted",
    ] {
        assert!(r.findings.iter().any(|f| f.id == id), "missing {id}");
    }
    assert_eq!(r.findings.iter().filter(|f| f.id == "kb.diode-direction-mixed").count(), 1);
    assert_eq!((r.summary.error, r.summary.warning), (7, 2));
}

#[test]
fn svg_has_layers_and_markers() {
    let result = run("macropad-buggy");
    for id in ["layer-F", "layer-B", "layer-T", "layer-E", "layer-R", "layer-M"] {
        assert!(result.svg.contains(&format!("id=\"{id}\"")), "missing {id}");
    }
    // Two via findings share one marker.
    assert!(result.svg.contains(r#"data-idx="2 3""#));
    // Every layer is its own group: silkscreen shows by default, fabrication is off until turned on.
    assert!(result.svg.contains(r#"data-layer="F.SilkS""#));
    assert!(result.svg.contains(r##"data-layer="F.Fab" data-label="Front fabrication" color="#a7a7b0" display="none""##));
    assert!(result.svg.contains(r#"class="zone zone-f" points="#) && result.svg.contains(r#"fill="none" stroke="currentColor""#));
    assert!(!result.svg.contains(r#"fill-opacity="0.18""#), "unfilled zones must not be shown as copper");
    assert!(result.svg.contains(">SW1</text>"));
    // Board items can be picked in the app.
    assert!(result.svg.contains(r#"data-item="track:"#) && result.svg.contains(r#"data-item="via:"#) && result.svg.contains(r#"data-item="fp:"#));
}

#[test]
fn svg_escapes_layer_values_and_exposes_via_spans() {
    use fabready_core::{board, render};
    let mut b = board::load(&sample("macropad-buggy/macropad-buggy.kicad_pcb")).unwrap();
    let via = &b.vias[0];
    assert_eq!(via.layers, ["F.Cu", "B.Cu"]);
    let malicious = r#"F.Cu" onload="x.Cu"#;
    b.layers.push(board::LayerInfo { name: malicious.into(), kind: "signal".into(), user_name: String::new() });
    let back = b.layers.iter().position(|l| l.name == "B.Cu").unwrap();
    b.layers.splice(
        back..back,
        [
            board::LayerInfo { name: "In1.Cu".into(), kind: "signal".into(), user_name: String::new() },
            board::LayerInfo { name: "In2.Cu".into(), kind: "signal".into(), user_name: String::new() },
        ],
    );
    let mut through_via = b.vias[0].clone();
    through_via.id = "through-test".into();
    b.vias.push(through_via);
    b.vias[0].layers = vec!["In1.Cu".into(), "In2.Cu".into()];
    for zone in &mut b.zones {
        zone.layers = vec![malicious.into()];
        zone.fills = vec![(malicious.into(), vec![(0.0, 0.0), (1.0, 0.0), (1.0, 1.0)])];
    }
    let svg = render::svg(&b, &[]);
    let escaped_id = render::esc(&format!("L-{}", malicious.replace('.', "_")));
    assert!(svg.contains(&format!(r#"id="{escaped_id}""#)));
    assert!(svg.contains(&format!(r#"data-layer="{}""#, render::esc(malicious))));
    assert!(svg.contains(&format!("zone GND {}", render::esc(malicious))));
    assert!(svg.contains(r#"data-layers="In1.Cu In2.Cu""#));
    assert!(svg.contains(r#"data-layers="F.Cu In1.Cu In2.Cu B.Cu""#));
    assert!(!svg.contains(r#"id="L-F_Cu" onload="x_Cu""#));
}

#[test]
fn copper_geometry_edits_clear_saved_zone_fills() {
    use fabready_core::{board, fixes};
    let dir = std::env::temp_dir().join(format!("fabready-zone-fill-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let pcb = dir.join("b.kicad_pcb");
    let mut source = std::fs::read_to_string(sample("macropad-buggy/macropad-buggy.kicad_pcb")).unwrap();
    let zone = source.find("(zone ").unwrap();
    let polygon = source[zone..].find("(polygon (pts").unwrap() + zone;
    source.insert_str(polygon, r#"(filled_polygon (layer "F.Cu") (pts (xy 100.5 100.5) (xy 186.5 100.5) (xy 186.5 157.5) (xy 100.5 157.5))) "#);
    std::fs::write(&pcb, &source).unwrap();
    let b = board::load(&pcb).unwrap();
    assert!(!b.zones[0].fills.is_empty());
    let track = &b.tracks[0];
    let preview = fixes::preview(&dir, &Options { use_kicad: false, ..Default::default() }, &[format!("edit:track:{}:width=0.3", track.id)]).unwrap();
    assert!(!preview.after.svg.contains(r#"fill-opacity="0.38""#), "stale saved copper is not rendered");
    assert!(preview.changes[0].detail.contains("run KiCad DRC before fabrication"));
    assert!(std::fs::read_to_string(&pcb).unwrap().contains("(filled_polygon"), "preview does not write");
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn manual_edits_are_surgical_and_undoable() {
    use fabready_core::{board, edit, fixes};
    let dir = std::env::temp_dir().join(format!("fabready-edit-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let pcb = dir.join("b.kicad_pcb");
    std::fs::copy(sample("macropad-buggy/macropad-buggy.kicad_pcb"), &pcb).unwrap();
    let original = std::fs::read_to_string(&pcb).unwrap();
    let opts = Options { use_kicad: false, ..Default::default() };
    let b = board::load(&pcb).unwrap();
    let track = b.tracks.iter().find(|t| t.layer == "B.Cu" && t.width == 0.25).unwrap().clone();
    let via = b.vias[0].clone();
    let sw5 = b.footprint("SW5").unwrap().clone();

    let info = edit::inspect(&b, &format!("track:{}", track.id)).unwrap();
    assert_eq!(info["width"], 0.25);
    assert_eq!(info["copper_layers"], serde_json::json!(["F.Cu", "B.Cu"]));
    assert_eq!(edit::inspect(&b, &format!("fp:{}", sw5.id)).unwrap()["reference"], "SW5");

    let ids = |v: &[String]| v.to_vec();
    let edits = ids(&[
        format!("edit:track:{}:width=0.3", track.id),
        format!("edit:via:{}:size=0.8", via.id),
        format!("edit:via:{}:drill=0.7", via.id),
        format!("edit:fp:{}:reference=SW15", sw5.id),
    ]);
    let preview = fixes::preview(&dir, &opts, &edits).unwrap();
    assert_eq!(std::fs::read_to_string(&pcb).unwrap(), original, "preview must not write");
    assert_eq!(preview.changes[0].title, "Track width 0.25 → 0.3 mm");
    let applied = fixes::apply(&dir, &opts, &edits, Some(&preview.token)).unwrap();

    let text = std::fs::read_to_string(&pcb).unwrap();
    assert_eq!(text.lines().count(), original.lines().count(), "edits change values in place");
    let after = board::load(&pcb).unwrap();
    assert_eq!(after.tracks.iter().find(|t| t.id == track.id).unwrap().width, 0.3);
    let v = after.vias.iter().find(|v| v.id == via.id).unwrap();
    assert_eq!((v.size, v.drill), (0.8, 0.7));
    assert!(after.footprint("SW15").is_some(), "footprint rename applied");

    // Deleting a track removes its line and nothing else.
    let del = ids(&[format!("delete:track:{}", track.id)]);
    fixes::apply(&dir, &opts, &del, None).unwrap();
    assert_eq!(std::fs::read_to_string(&pcb).unwrap().lines().count(), original.lines().count() - 1);

    // Bad values are refused before anything is written.
    for bad in [
        format!("edit:via:{}:drill=0.9", via.id),
        format!("edit:track:{}:layer=F.SilkS", b.tracks[1].id),
        format!("edit:fp:{}:reference=SW1", after.footprint("SW15").unwrap().id),
        format!("edit:fp:{}:move=1,0", sw5.id),
        format!("edit:track:{}:width=0", b.tracks[1].id),
        "edit:track:nope:width=0.3".to_string(),
    ] {
        assert!(fixes::preview(&dir, &opts, &ids(std::slice::from_ref(&bad))).is_err(), "{bad}");
    }
    assert!(fixes::preview(
        &dir,
        &opts,
        &ids(&[format!("edit:via:{}:size=0.4", via.id), format!("edit:via:{}:drill=0.5", via.id)])
    )
    .is_err(), "combined via dimensions must be validated");

    fixes::restore(&dir, std::path::Path::new(&applied.backup)).unwrap();
    assert_eq!(std::fs::read_to_string(&pcb).unwrap(), original);
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn drag_route_and_undo_redo() {
    use fabready_core::{board, fixes};
    let dir = std::env::temp_dir().join(format!("fabready-drag-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let pcb = dir.join("b.kicad_pcb");
    std::fs::copy(sample("macropad/macropad.kicad_pcb"), &pcb).unwrap();
    let original = std::fs::read_to_string(&pcb).unwrap();
    let opts = Options { use_kicad: false, ..Default::default() };
    let b = board::load(&pcb).unwrap();
    let sw1 = b.footprint("SW1").unwrap().clone();
    let close = |a: (f64, f64), b: (f64, f64)| (a.0 - b.0).abs() < 1e-3 && (a.1 - b.1).abs() < 1e-3;
    let ends_at = |b: &board::Board, p: (f64, f64), net: &str| b.tracks.iter().filter(|t| t.net == net && t.points.iter().any(|q| close(*q, p))).count();
    let pad = sw1.pads.iter().find(|p| !p.net.is_empty() && ends_at(&b, (p.x, p.y), &p.net) > 0).expect("a routed pad").clone();
    let connected = ends_at(&b, (pad.x, pad.y), &pad.net);

    // Dragging a footprint drags the track ends on its pads along (up its column, so nothing else moves off a track).
    let drag = vec![format!("edit:fp:{}:move=0,-0.5", sw1.id)];
    let done = fixes::apply(&dir, &opts, &drag, None).unwrap();
    assert!(done.changes[0].detail.contains("follow"), "{}", done.changes[0].detail);
    let after = board::load(&pcb).unwrap();
    let moved = after.footprint("SW1").unwrap();
    assert_eq!((moved.x, moved.y), (sw1.x, sw1.y - 0.5));
    assert_eq!(ends_at(&after, (pad.x, pad.y - 0.5), &pad.net), connected);
    let errs: Vec<String> = report::run(&dir, &opts).unwrap().report.findings.iter().filter(|f| f.severity == Severity::Error).map(|f| f.message.clone()).collect();
    assert!(errs.is_empty(), "stretched tracks stay connected: {errs:?}");

    // Undo puts the board back and keeps the undone state for redo; a stale token is refused.
    assert!(fixes::swap(&dir, std::path::Path::new(&done.backup), "0000").is_err());
    let undone = fixes::swap(&dir, std::path::Path::new(&done.backup), &done.token).unwrap();
    assert_eq!(std::fs::read_to_string(&pcb).unwrap(), original);
    let redone = fixes::swap(&dir, std::path::Path::new(&undone.backup), &undone.token).unwrap();
    assert_eq!(fixes::token(&std::fs::read_to_string(&pcb).unwrap()), done.token);
    fixes::swap(&dir, std::path::Path::new(&redone.backup), &redone.token).unwrap();

    // Rotating turns the footprint relative to where it is.
    fixes::apply(&dir, &opts, &[format!("edit:fp:{}:rotate=90", sw1.id)], None).unwrap();
    assert_eq!(board::load(&pcb).unwrap().footprint("SW1").unwrap().angle, 90.0);
    std::fs::write(&pcb, &original).unwrap();

    // Route a new track with a bend and drop a via on its end; both join the net.
    let net = pad.net.clone();
    let (x, y) = (pad.x, pad.y);
    let add = vec![
        format!("add:track:F.Cu:0.3:{x},{y};{},{y};{},{}:{net}", x + 3.0, x + 5.0, y + 2.0),
        format!("add:via:{},{}:0.6:0.3:{net}", x + 5.0, y + 2.0),
    ];
    let done = fixes::apply(&dir, &opts, &add, None).unwrap();
    assert_eq!(done.changes[0].title, format!("Route {net}"));
    let after = board::load(&pcb).unwrap();
    assert_eq!(after.tracks.len(), b.tracks.len() + 2);
    assert!(after.tracks.iter().filter(|t| t.layer == "F.Cu" && t.width == 0.3).all(|t| t.net == net));
    assert!(after.vias.iter().any(|v| close((v.x, v.y), (x + 5.0, y + 2.0)) && v.net == net));
    assert!(fixes::preview(&dir, &opts, &["add:track:F.Cu:0.3:1,1;2,2:NO_SUCH_NET".to_string()]).is_err());
    assert!(fixes::preview(&dir, &opts, &["add:via:1,1:0.3:0.6:".to_string()]).is_err());
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn via_moves_refuse_tracks_that_pass_through_the_via() {
    use fabready_core::{board, edit, sexpr};
    let text = std::fs::read_to_string(sample("macropad-buggy/macropad-buggy.kicad_pcb")).unwrap();
    let root = sexpr::parse(&text).unwrap();
    let b = board::parse_board(&text).unwrap();
    let via = b.vias[0].clone();
    let node = root.children("via").next().unwrap();
    let net = node.child("net").and_then(|n| n.arg(1)).unwrap();
    let (x, y) = (via.x, via.y);
    let injected = format!(
        "\n(segment (start {} {}) (end {} {}) (width 0.25) (layer \"F.Cu\") (net {net}) (uuid \"move-test-track\"))\n",
        x - 1.0, y, x + 1.0, y
    );
    let mut changed = text.clone();
    changed.insert_str(root.span().end - 1, &injected);
    let changed_root = sexpr::parse(&changed).unwrap();
    let changed_board = board::parse_board(&changed).unwrap();
    assert!(edit::plan(&changed, &changed_root, &changed_board, &format!("edit:via:{}:move=1,0", via.id)).is_err());
}

#[test]
fn ignore_rules_waive_findings() {
    let dir = std::env::temp_dir().join(format!("fabready-ignore-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::copy(sample("macropad-buggy/macropad-buggy.kicad_pcb"), dir.join("b.kicad_pcb")).unwrap();
    std::fs::write(dir.join("fabready.json"), r#"{"ignore": ["fab.*", "kb.missing-diode:SW9"]}"#).unwrap();
    let r = report::run(&dir, &Options { use_kicad: false, ..Default::default() }).unwrap().report;
    assert!(!r.findings.iter().any(|f| f.id.starts_with("fab.") || f.id == "kb.missing-diode"));
    assert!(r.findings.iter().any(|f| f.id == "kb.diode-direction-mixed"));
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn non_keyboard_board_gets_no_keyboard_findings() {
    let dir = std::env::temp_dir().join(format!("fabready-generic-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let pad = |n: &str, x: f64, net: &str| format!(r#"(pad "{n}" thru_hole circle (at {x} 0) (size 1.6 1.6) (drill 0.8) (layers "*.Cu" "*.Mask") {net})"#);
    let board = format!(
        r#"(kicad_pcb (version 20240108) (net 0 "") (net 1 "SIG") (net 2 "VIN")
  (gr_rect (start 0 0) (end 30 20) (layer "Edge.Cuts"))
  (footprint "Resistor_THT:R_Axial" (layer "F.Cu") (at 8 10) (property "Reference" "R1") (property "Value" "10k") {} {})
  (footprint "LED_THT:LED_D3.0mm" (layer "F.Cu") (at 20 10) (property "Reference" "D1") (property "Value" "LED") {})
  (segment (start 12 10) (end 20 10) (width 0.25) (layer "F.Cu") (net 1)))"#,
        pad("1", 0.0, r#"(net 2 "VIN")"#),
        pad("2", 4.0, r#"(net 1 "SIG")"#),
        pad("1", 0.0, r#"(net 1 "SIG")"#)
    );
    std::fs::write(dir.join("led.kicad_pcb"), board).unwrap();
    let r = report::run(&dir, &Options { use_kicad: false, ..Default::default() }).unwrap().report;
    assert!(r.matrix.is_none());
    assert!(!r.findings.iter().any(|f| f.id.starts_with("kb.")), "{:?}", r.findings.iter().map(|f| &f.id).collect::<Vec<_>>());
    assert_eq!(r.status, "pass");
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn fixes_preview_apply_and_undo() {
    use fabready_core::fixes;
    let dir = std::env::temp_dir().join(format!("fabready-fix-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let pcb = dir.join("b.kicad_pcb");
    std::fs::copy(sample("macropad-buggy/macropad-buggy.kicad_pcb"), &pcb).unwrap();
    let original = std::fs::read_to_string(&pcb).unwrap();
    let opts = Options { use_kicad: false, ..Default::default() };

    let before = report::run(&dir, &opts).unwrap().report;
    let ids: Vec<&str> = before.fixes.iter().map(|f| f.id.as_str()).collect();
    assert!(ids.contains(&"widen-track:B.Cu:0.1:ROW0"), "{ids:?}");
    assert!(ids.contains(&"antenna-keepout:U1"), "{ids:?}");
    assert!(ids.iter().any(|i| i.starts_with("resize-via:")), "{ids:?}");
    // One via fix resolves both via findings.
    let via = before.fixes.iter().find(|f| f.id.starts_with("resize-via:")).unwrap();
    assert_eq!(via.findings.len(), 2);

    let all = vec!["all".to_string()];
    let preview = fixes::preview(&dir, &opts, &all).unwrap();
    assert_eq!(std::fs::read_to_string(&pcb).unwrap(), original, "preview must not write");
    assert!(preview.after.svg.contains("id=\"layer-P\""));
    assert_eq!(preview.changes.len(), 3);
    let fixed = ["fab.track-too-thin", "fab.via-drill-too-small", "fab.via-annular-ring", "kb.copper-under-antenna"];
    assert!(!preview.after.report.findings.iter().any(|f| fixed.contains(&f.id.as_str())));
    assert_eq!(preview.after.report.summary.error, before.summary.error - 3);

    // A stale token is refused.
    assert!(fixes::apply(&dir, &opts, &all, Some("0000")).is_err());
    let applied = fixes::apply(&dir, &opts, &all, Some(&preview.token)).unwrap();
    let after = report::run(&dir, &opts).unwrap().report;
    assert!(after.fixes.is_empty());
    assert!(!after.findings.iter().any(|f| fixed.contains(&f.id.as_str())));
    // Untouched findings survive, and the edit is surgical.
    assert!(after.findings.iter().any(|f| f.id == "kb.missing-diode"));
    let new_text = std::fs::read_to_string(&pcb).unwrap();
    assert!(new_text.contains("(width 0.15)") && new_text.contains("fabready keep-out U1"));
    assert_eq!(new_text.lines().count(), original.lines().count() + 29);

    fixes::restore(&dir, std::path::Path::new(&applied.backup)).unwrap();
    assert_eq!(std::fs::read_to_string(&pcb).unwrap(), original);
    assert!(fixes::restore(&dir, std::path::Path::new("/etc/hosts")).is_err());
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn parts_review_catches_misfit_components() {
    let r = run("sensor-board").report;
    let verdict = |reference: &str| r.parts.iter().find(|p| p.refs.iter().any(|x| x == reference)).map(|p| p.verdict.clone()).unwrap();
    for (reference, expected) in [("C1", "under"), ("D2", "under"), ("R2", "under"), ("U2", "under"), ("C3", "over"), ("R3", "over"), ("D3", "over"), ("U1", "over"), ("C2", "ok"), ("R1", "ok"), ("D1", "ok"), ("J1", "na")] {
        assert_eq!(verdict(reference), expected, "{reference}");
    }
    let ids: Vec<&str> = r.findings.iter().filter(|f| f.id.starts_with("part.")).map(|f| f.id.as_str()).collect();
    for id in ["part.voltage-margin", "part.led-current", "part.power-margin", "part.supply-range"] {
        assert!(ids.contains(&id), "missing {id}: {ids:?}");
    }
    // The regulator's output feeds the 3.3 V rail and its input comes from VBUS.
    let u1 = r.parts.iter().find(|p| p.refs == ["U1"]).unwrap();
    assert_eq!((u1.specs.vout_v, u1.stress.vin_v, u1.stress.load_a), (Some(3.3), Some(5.0), Some(0.03)));
    // The keyboard sample's diodes are fine; switches and the module aren't rated.
    let kb = run("macropad").report;
    assert!(kb.parts.iter().all(|p| p.verdict == "ok" || p.verdict == "na"));
}

#[cfg(feature = "png")]
#[test]
fn renders_png() {
    let result = run("macropad");
    let png = fabready_core::render::png(&result.svg, 800).unwrap();
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
}
