//! New KiCad projects: an empty schematic and a board with its outline (plus optional mounting holes and a
//! key grid guide), ready to design in KiCad while fabready re-checks every save.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::Deserialize;

use crate::config::{self, Config};

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct NewProject {
    /// Folder and file name, e.g. `my-board`.
    pub name: String,
    /// Board outline in mm.
    pub width: f64,
    pub height: f64,
    /// 2 or 4.
    pub copper_layers: u8,
    /// M3 holes in the four corners.
    pub mounting_holes: bool,
    /// Fab profile saved to fabready.json (`None` keeps fabready's default).
    pub profile: Option<String>,
    /// Draw a key grid on User.Drawings and save the pitch to fabready.json.
    pub keyboard: Option<KeyGrid>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub struct KeyGrid {
    pub rows: u32,
    pub cols: u32,
    pub pitch_x: f64,
    pub pitch_y: f64,
}

impl Default for NewProject {
    fn default() -> Self {
        NewProject { name: "my-board".into(), width: 50.0, height: 50.0, copper_layers: 2, mounting_holes: true, profile: None, keyboard: None }
    }
}

/// Distance from the board edge to the centre of each corner mounting hole.
pub const HOLE_INSET: f64 = 4.0;
const HOLE_DRILL: f64 = 3.2;

/// Reject names that can't be a folder and file name on every platform (or would break the KiCad files).
pub fn check_name(name: &str) -> Result<()> {
    if name.is_empty() {
        bail!("give the project a name");
    }
    if name.len() > 80 {
        bail!("keep the project name under 80 characters");
    }
    if name.starts_with('.') || name.ends_with('.') || name.ends_with(' ') {
        bail!("the project name can't start or end with a dot or end with a space");
    }
    if let Some(c) = name.chars().find(|c| c.is_control() || r#"/\:*?"<>|"#.contains(*c)) {
        bail!("the project name can't contain {c:?}");
    }
    Ok(())
}

impl NewProject {
    pub fn validate(&self) -> Result<()> {
        check_name(&self.name)?;
        let ok = |v: f64| v.is_finite() && (5.0..=600.0).contains(&v);
        if !ok(self.width) || !ok(self.height) {
            bail!("the board must be between 5 and 600 mm on each side");
        }
        if self.copper_layers != 2 && self.copper_layers != 4 {
            bail!("choose 2 or 4 copper layers");
        }
        if let Some(p) = &self.profile {
            if config::profile(p).is_none() {
                bail!("unknown profile '{p}' (choose from {})", config::PROFILE_NAMES.join(", "));
            }
        }
        if let Some(k) = self.keyboard {
            if !(1..=30).contains(&k.rows) || !(1..=40).contains(&k.cols) {
                bail!("use 1 to 30 rows and 1 to 40 columns of keys");
            }
            if !(k.pitch_x.is_finite() && k.pitch_y.is_finite() && (10.0..=30.0).contains(&k.pitch_x) && (10.0..=30.0).contains(&k.pitch_y)) {
                bail!("the key pitch must be between 10 and 30 mm");
            }
            let (gw, gh) = (k.cols as f64 * k.pitch_x, k.rows as f64 * k.pitch_y);
            if gw > self.width + 1e-6 || gh > self.height + 1e-6 {
                bail!("{} × {} keys need at least {gw:.1} × {gh:.1} mm; make the board bigger", k.cols, k.rows);
            }
        }
        Ok(())
    }
}

/// Create `<parent>/<name>/` with `<name>.kicad_pro`, `.kicad_sch`, `.kicad_pcb` and `fabready.json`.
/// Refuses to touch a folder that already has files in it. Returns the project folder.
pub fn create(parent: &Path, p: &NewProject) -> Result<PathBuf> {
    p.validate()?;
    if !parent.is_dir() {
        bail!("{} isn't a folder", parent.display());
    }
    let dir = parent.join(&p.name);
    if dir.exists() {
        let busy = std::fs::read_dir(&dir)
            .with_context(|| format!("reading {}", dir.display()))?
            .filter_map(|e| e.ok())
            .any(|e| !e.file_name().to_string_lossy().starts_with('.'));
        if busy {
            bail!("{} already exists and has files in it; choose another name or location", dir.display());
        }
    }
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;

    let write = |ext: &str, text: String| {
        let f = dir.join(format!("{}.{ext}", p.name));
        std::fs::write(&f, text).with_context(|| format!("writing {}", f.display()))
    };
    write("kicad_pro", project_json(&p.name))?;
    write("kicad_sch", schematic(p))?;
    write("kicad_pcb", board(p))?;

    let mut cfg = Config { profile: p.profile.clone(), ..Default::default() };
    if let Some(k) = p.keyboard {
        if (k.pitch_x - k.pitch_y).abs() < 1e-9 {
            cfg.keyboard.key_pitch = Some(k.pitch_x);
        } else {
            cfg.keyboard.key_pitch_x = Some(k.pitch_x);
            cfg.keyboard.key_pitch_y = Some(k.pitch_y);
        }
    }
    if cfg.profile.is_some() || p.keyboard.is_some() {
        cfg.save(&dir.join(config::FILE_NAME))?;
    }
    Ok(dir)
}

fn project_json(name: &str) -> String {
    let v = serde_json::json!({ "meta": { "filename": format!("{name}.kicad_pro"), "version": 1 } });
    serde_json::to_string_pretty(&v).unwrap_or_default() + "\n"
}

fn schematic(p: &NewProject) -> String {
    format!(
        "(kicad_sch\n  (version 20231120)\n  (generator \"fabready\")\n  (generator_version \"{ver}\")\n  (uuid \"{uuid}\")\n  (paper \"A4\")\n  \
         (title_block\n    (title \"{name}\")\n  )\n  (lib_symbols)\n  (sheet_instances\n    (path \"/\"\n      (page \"1\")\n    )\n  )\n)\n",
        ver = env!("CARGO_PKG_VERSION"),
        uuid = crate::fixes::uuid_for(&format!("{}/sch", p.name)),
        name = p.name,
    )
}

fn board(p: &NewProject) -> String {
    let mut n = 0;
    let mut uid = || {
        n += 1;
        crate::fixes::uuid_for(&format!("{}/pcb/{n}", p.name))
    };
    let r = |v: f64| (v * 1000.0).round() / 1000.0;
    // Put the board in the middle of the A4 sheet, on whole millimetres.
    let x0 = ((297.0 - p.width) / 2.0).max(10.0).round();
    let y0 = ((210.0 - p.height) / 2.0).max(10.0).round();
    let (x1, y1) = (r(x0 + p.width), r(y0 + p.height));

    let inner = if p.copper_layers == 4 { "    (1 \"In1.Cu\" signal) (2 \"In2.Cu\" signal)\n" } else { "" };
    let mut s = format!(
        "(kicad_pcb (version 20240108) (generator \"fabready\") (generator_version \"{ver}\")\n  \
         (general (thickness 1.6) (legacy_teardrops no))\n  (paper \"A4\")\n  (title_block (title \"{name}\"))\n  \
         (layers\n    (0 \"F.Cu\" signal)\n{inner}    (31 \"B.Cu\" signal)\n{LAYERS}  )\n  (setup (pad_to_mask_clearance 0))\n  (net 0 \"\")\n",
        ver = env!("CARGO_PKG_VERSION"),
        name = p.name,
    );
    let _ = writeln!(
        s,
        "  (gr_rect (start {x0} {y0}) (end {x1} {y1}) (stroke (width 0.1) (type default)) (fill none) (layer \"Edge.Cuts\") (uuid \"{}\"))",
        uid()
    );

    if p.mounting_holes && p.width >= 3.0 * HOLE_INSET && p.height >= 3.0 * HOLE_INSET {
        let corners = [(x0 + HOLE_INSET, y0 + HOLE_INSET), (x1 - HOLE_INSET, y0 + HOLE_INSET), (x1 - HOLE_INSET, y1 - HOLE_INSET), (x0 + HOLE_INSET, y1 - HOLE_INSET)];
        for (i, (x, y)) in corners.into_iter().enumerate() {
            let (x, y) = (r(x), r(y));
            let crt = HOLE_DRILL / 2.0 + 1.85;
            let _ = write!(
                s,
                "  (footprint \"MountingHole:MountingHole_3.2mm_M3\" (layer \"F.Cu\") (uuid \"{u0}\") (at {x} {y})\n    \
                 (property \"Reference\" \"H{n}\" (at 0 -4.2 0) (layer \"F.SilkS\") (uuid \"{u1}\") (effects (font (size 1 1) (thickness 0.15))))\n    \
                 (property \"Value\" \"MountingHole_3.2mm_M3\" (at 0 4.2 0) (layer \"F.Fab\") (uuid \"{u2}\") (effects (font (size 1 1) (thickness 0.15))))\n    \
                 (attr board_only exclude_from_pos_files exclude_from_bom)\n    \
                 (fp_circle (center 0 0) (end {hole} 0) (stroke (width 0.15) (type solid)) (fill none) (layer \"Cmts.User\") (uuid \"{u3}\"))\n    \
                 (fp_circle (center 0 0) (end {crt} 0) (stroke (width 0.05) (type solid)) (fill none) (layer \"F.CrtYd\") (uuid \"{u4}\"))\n    \
                 (pad \"\" np_thru_hole circle (at 0 0) (size {d} {d}) (drill {d}) (layers \"*.Cu\" \"*.Mask\") (uuid \"{u5}\"))\n  )\n",
                n = i + 1,
                u0 = uid(),
                u1 = uid(),
                u2 = uid(),
                u3 = uid(),
                u4 = uid(),
                u5 = uid(),
                hole = HOLE_DRILL,
                d = HOLE_DRILL,
            );
        }
    }

    if let Some(k) = p.keyboard {
        // Centred across, with the same margin below as at the sides; the space left at the top is for the controller.
        let (gw, gh) = (k.cols as f64 * k.pitch_x, k.rows as f64 * k.pitch_y);
        let side = (p.width - gw) / 2.0;
        let top = if p.height - gh >= 2.0 * side { y1 - side - gh } else { y0 + (p.height - gh) / 2.0 };
        let left = x0 + side;
        for row in 0..k.rows {
            for col in 0..k.cols {
                let (ax, ay) = (r(left + col as f64 * k.pitch_x), r(top + row as f64 * k.pitch_y));
                let _ = writeln!(
                    s,
                    "  (gr_rect (start {ax} {ay}) (end {} {}) (stroke (width 0.1) (type default)) (fill none) (layer \"Dwgs.User\") (uuid \"{}\"))",
                    r(ax + k.pitch_x),
                    r(ay + k.pitch_y),
                    uid()
                );
            }
        }
        let _ = writeln!(
            s,
            "  (gr_text \"Key grid guide ({} × {} at {} × {} mm). Place a switch in each cell, then delete the guide.\" (at {} {}) (layer \"Dwgs.User\") (uuid \"{}\") (effects (font (size 1 1) (thickness 0.15)) (justify left bottom)))",
            k.cols,
            k.rows,
            k.pitch_x,
            k.pitch_y,
            r(left),
            r(top - 1.0),
            uid()
        );
    }
    s.push_str(")\n");
    s
}

const LAYERS: &str = r#"    (32 "B.Adhes" user "B.Adhesive") (33 "F.Adhes" user "F.Adhesive")
    (34 "B.Paste" user) (35 "F.Paste" user)
    (36 "B.SilkS" user "B.Silkscreen") (37 "F.SilkS" user "F.Silkscreen")
    (38 "B.Mask" user) (39 "F.Mask" user)
    (40 "Dwgs.User" user "User.Drawings") (41 "Cmts.User" user "User.Comments")
    (42 "Eco1.User" user "User.Eco1") (43 "Eco2.User" user "User.Eco2")
    (44 "Edge.Cuts" user) (45 "Margin" user)
    (46 "B.CrtYd" user "B.Courtyard") (47 "F.CrtYd" user "F.Courtyard")
    (48 "B.Fab" user) (49 "F.Fab" user)
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::{self, Options};

    fn tmp(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let d = std::env::temp_dir().join(format!("fabready-scaffold-{tag}-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn check(dir: &Path) -> report::Report {
        report::run(dir, &Options { use_kicad: false, ..Default::default() }).unwrap().report
    }

    #[test]
    fn blank_board_checks_clean() {
        let parent = tmp("blank");
        let p = NewProject { name: "blank".into(), width: 60.0, height: 40.0, profile: Some("safe".into()), ..Default::default() };
        let dir = create(&parent, &p).unwrap();
        for ext in ["kicad_pro", "kicad_sch", "kicad_pcb"] {
            assert!(dir.join(format!("blank.{ext}")).exists(), "{ext}");
        }
        let r = check(&dir);
        assert_eq!(r.summary.error, 0, "{:#?}", r.findings);
        assert_eq!(r.profile, "safe");
        assert_eq!((r.stats.width_mm, r.stats.height_mm), (Some(60.0), Some(40.0)));
        assert_eq!(r.stats.footprints, 4);
        assert!(r.parts.is_empty(), "mounting holes aren't BOM parts: {:?}", r.parts);
        assert!(!r.findings.iter().any(|f| f.id == "board.no-mounting-holes"));
        // A second create into the same folder is refused rather than overwriting the design.
        assert!(create(&parent, &p).unwrap_err().to_string().contains("already exists"));
        std::fs::remove_dir_all(parent).ok();
    }

    #[test]
    fn keyboard_board_gets_grid_and_pitch() {
        let parent = tmp("kb");
        let k = KeyGrid { rows: 2, cols: 3, pitch_x: 18.0, pitch_y: 17.0 };
        let p = NewProject { name: "pad".into(), width: 70.0, height: 60.0, copper_layers: 4, mounting_holes: false, keyboard: Some(k), ..Default::default() };
        let dir = create(&parent, &p).unwrap();
        let pcb = std::fs::read_to_string(dir.join("pad.kicad_pcb")).unwrap();
        assert_eq!(pcb.matches("(layer \"Dwgs.User\")").count(), 2 * 3 + 1);
        assert!(pcb.contains("\"In2.Cu\""));
        let cfg = Config::read(&dir.join(config::FILE_NAME)).unwrap();
        assert_eq!((cfg.keyboard.key_pitch_x, cfg.keyboard.key_pitch_y), (Some(18.0), Some(17.0)));
        assert_eq!(check(&dir).summary.error, 0);
        std::fs::remove_dir_all(parent).ok();
    }

    #[test]
    fn rejects_bad_input() {
        let parent = tmp("bad");
        let bad = |p: NewProject| create(&parent, &p).unwrap_err().to_string();
        assert!(bad(NewProject { name: "a/b".into(), ..Default::default() }).contains("can't contain"));
        assert!(bad(NewProject { width: 2.0, ..Default::default() }).contains("between 5 and 600"));
        let k = KeyGrid { rows: 4, cols: 4, pitch_x: 19.05, pitch_y: 19.05 };
        assert!(bad(NewProject { keyboard: Some(k), ..Default::default() }).contains("make the board bigger"));
        assert!(bad(NewProject { profile: Some("nope".into()), ..Default::default() }).contains("unknown profile"));
        std::fs::remove_dir_all(parent).ok();
    }
}
