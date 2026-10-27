use std::path::PathBuf;

use pcbcheck_core::report::{self, Options};
use pcbcheck_core::Severity;

fn sample(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../samples").join(name)
}

fn run(name: &str) -> pcbcheck_core::CheckResult {
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
}

#[test]
fn ignore_rules_waive_findings() {
    let dir = std::env::temp_dir().join(format!("pcbcheck-ignore-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::copy(sample("macropad-buggy/macropad-buggy.kicad_pcb"), dir.join("b.kicad_pcb")).unwrap();
    std::fs::write(dir.join("pcbcheck.json"), r#"{"ignore": ["fab.*", "kb.missing-diode:SW9"]}"#).unwrap();
    let r = report::run(&dir, &Options { use_kicad: false, ..Default::default() }).unwrap().report;
    assert!(!r.findings.iter().any(|f| f.id.starts_with("fab.") || f.id == "kb.missing-diode"));
    assert!(r.findings.iter().any(|f| f.id == "kb.diode-direction-mixed"));
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn non_keyboard_board_gets_no_keyboard_findings() {
    let dir = std::env::temp_dir().join(format!("pcbcheck-generic-{}", std::process::id()));
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
    use pcbcheck_core::fixes;
    let dir = std::env::temp_dir().join(format!("pcbcheck-fix-{}", std::process::id()));
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
    assert!(new_text.contains("(width 0.15)") && new_text.contains("pcbcheck keep-out U1"));
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
    let png = pcbcheck_core::render::png(&result.svg, 800).unwrap();
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
}
