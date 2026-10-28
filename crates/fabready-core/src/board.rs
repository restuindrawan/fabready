//! Load a .kicad_pcb file into a small geometry model (KiCad 6+ file format).

use std::collections::HashMap;
use std::f64::consts::PI;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

use crate::sexpr::{self, Sexp};

pub type Point = (f64, f64);

/// Rotate a local offset the way KiCad does (Y down, positive angle = CCW on screen).
pub fn rotate(lx: f64, ly: f64, angle_deg: f64) -> Point {
    if angle_deg == 0.0 {
        return (lx, ly);
    }
    let (s, c) = angle_deg.to_radians().sin_cos();
    (lx * c + ly * s, -lx * s + ly * c)
}

pub fn arc_points(start: Point, mid: Point, end: Point, steps: usize) -> Vec<Point> {
    let ((x1, y1), (x2, y2), (x3, y3)) = (start, mid, end);
    let d = 2.0 * (x1 * (y2 - y3) + x2 * (y3 - y1) + x3 * (y1 - y2));
    if d.abs() < 1e-9 {
        return vec![start, end];
    }
    let (s1, s2, s3) = (x1 * x1 + y1 * y1, x2 * x2 + y2 * y2, x3 * x3 + y3 * y3);
    let ux = (s1 * (y2 - y3) + s2 * (y3 - y1) + s3 * (y1 - y2)) / d;
    let uy = (s1 * (x3 - x2) + s2 * (x1 - x3) + s3 * (x2 - x1)) / d;
    let r = (x1 - ux).hypot(y1 - uy);
    let ang = |p: Point| (p.1 - uy).atan2(p.0 - ux);
    let (a1, am, a3) = (ang(start), ang(mid), ang(end));
    let tau = 2.0 * PI;
    let sweep = if (am - a1).rem_euclid(tau) <= (a3 - a1).rem_euclid(tau) {
        (a3 - a1).rem_euclid(tau)
    } else {
        -(a1 - a3).rem_euclid(tau)
    };
    let mut pts: Vec<Point> = (0..=steps)
        .map(|i| {
            let a = a1 + sweep * i as f64 / steps as f64;
            (ux + r * a.cos(), uy + r * a.sin())
        })
        .collect();
    pts[0] = start;
    pts[steps] = end;
    pts
}

pub fn circle_points(center: Point, radius: f64, steps: usize) -> Vec<Point> {
    (0..=steps)
        .map(|i| {
            let a = 2.0 * PI * i as f64 / steps as f64;
            (center.0 + radius * a.cos(), center.1 + radius * a.sin())
        })
        .collect()
}

pub fn point_segment_distance(p: Point, a: Point, b: Point) -> f64 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len2 = dx * dx + dy * dy;
    if len2 == 0.0 {
        return (p.0 - a.0).hypot(p.1 - a.1);
    }
    let t = (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / len2).clamp(0.0, 1.0);
    (p.0 - (a.0 + t * dx)).hypot(p.1 - (a.1 + t * dy))
}

fn segments_intersect(a: Point, b: Point, c: Point, d: Point) -> bool {
    let orient = |p: Point, q: Point, r: Point| (q.0 - p.0) * (r.1 - p.1) - (q.1 - p.1) * (r.0 - p.0);
    let (o1, o2, o3, o4) = (orient(a, b, c), orient(a, b, d), orient(c, d, a), orient(c, d, b));
    (o1 > 0.0) != (o2 > 0.0) && (o3 > 0.0) != (o4 > 0.0) && o1 * o2 != 0.0 && o3 * o4 != 0.0
}

pub fn segment_distance(a: Point, b: Point, c: Point, d: Point) -> f64 {
    if segments_intersect(a, b, c, d) {
        return 0.0;
    }
    point_segment_distance(a, c, d)
        .min(point_segment_distance(b, c, d))
        .min(point_segment_distance(c, a, b))
        .min(point_segment_distance(d, a, b))
}

pub fn point_in_polygon(p: Point, poly: &[Point]) -> bool {
    let (x, y) = p;
    let n = poly.len();
    let mut inside = false;
    for i in 0..n {
        let ((x1, y1), (x2, y2)) = (poly[i], poly[(i + 1) % n]);
        if (y1 > y) != (y2 > y) && x < (x2 - x1) * (y - y1) / (y2 - y1) + x1 {
            inside = !inside;
        }
    }
    inside
}

fn copper_layer_match(pad_layers: &[String], layer: &str) -> bool {
    pad_layers.iter().any(|l| l == layer || l == "*.Cu" || (l == "F&B.Cu" && (layer == "F.Cu" || layer == "B.Cu")))
}

#[derive(Debug, Clone)]
pub struct Pad {
    pub fp_ref: String,
    pub number: String,
    /// thru_hole | smd | np_thru_hole | connect
    pub kind: String,
    pub shape: String,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub angle: f64,
    pub layers: Vec<String>,
    pub net: String,
    pub pinfunction: String,
    pub drill: Option<f64>,
}

impl Pad {
    pub fn electrical(&self) -> bool {
        self.kind != "np_thru_hole" && self.layers.iter().any(|l| l.ends_with(".Cu"))
    }

    pub fn on_layer(&self, layer: &str) -> bool {
        copper_layer_match(&self.layers, layer)
    }

    pub fn center(&self) -> Point {
        (self.x, self.y)
    }

    pub fn contains(&self, p: Point) -> bool {
        let (dx, dy) = (p.0 - self.x, p.1 - self.y);
        if self.shape == "circle" {
            return dx.hypot(dy) <= self.w / 2.0;
        }
        let (lx, ly) = rotate(dx, dy, -self.angle);
        lx.abs() <= self.w / 2.0 && ly.abs() <= self.h / 2.0
    }

    pub fn polygon(&self) -> Vec<Point> {
        if self.shape == "circle" {
            return circle_points((self.x, self.y), self.w / 2.0, 20);
        }
        let (hw, hh) = (self.w / 2.0, self.h / 2.0);
        [(-hw, -hh), (hw, -hh), (hw, hh), (-hw, hh)]
            .iter()
            .map(|&(lx, ly)| {
                let (rx, ry) = rotate(lx, ly, self.angle);
                (self.x + rx, self.y + ry)
            })
            .collect()
    }
}

#[derive(Debug, Clone)]
pub struct Footprint {
    /// `uuid` (or `tstamp`) from the file, or `#<n>` (order in the file) when there is none.
    pub id: String,
    pub reference: String,
    pub value: String,
    pub lib_id: String,
    pub x: f64,
    pub y: f64,
    pub angle: f64,
    pub layer: String,
    pub pads: Vec<Pad>,
    pub courtyard: Vec<Vec<Point>>,
    /// All footprint properties (KiCad 8+ copies symbol fields here: Datasheet, Description, LCSC, MPN, ...).
    pub props: std::collections::BTreeMap<String, String>,
    /// `(attr ...)` flags such as `smd`, `through_hole`, `exclude_from_bom`, `dnp`, `board_only`.
    pub attrs: Vec<String>,
}

impl Footprint {
    pub fn bbox(&self) -> (f64, f64, f64, f64) {
        let mut pts: Vec<Point> = self.courtyard.iter().flatten().copied().collect();
        if pts.is_empty() {
            pts = self.pads.iter().flat_map(Pad::polygon).collect();
        }
        bbox_of(&pts).unwrap_or((self.x, self.y, self.x, self.y))
    }

    pub fn text(&self) -> String {
        format!("{} {}", self.lib_id, self.value)
    }

    pub fn is_mounting_hole(&self) -> bool {
        self.lib_id.to_lowercase().replace('_', "").contains("mountinghole")
    }
}

#[derive(Debug, Clone)]
pub struct Track {
    /// See `Footprint::id`; the order counts segments first, then arcs.
    pub id: String,
    pub points: Vec<Point>,
    pub width: f64,
    pub layer: String,
    pub net: String,
}

impl Track {
    pub fn segments(&self) -> impl Iterator<Item = (Point, Point)> + '_ {
        self.points.windows(2).map(|w| (w[0], w[1]))
    }

    pub fn length(&self) -> f64 {
        self.segments().map(|(a, b)| (b.0 - a.0).hypot(b.1 - a.1)).sum()
    }
}

#[derive(Debug, Clone)]
pub struct Via {
    pub id: String,
    pub x: f64,
    pub y: f64,
    pub size: f64,
    pub drill: f64,
    pub net: String,
}

#[derive(Debug, Clone)]
pub struct Zone {
    pub net: String,
    pub layers: Vec<String>,
    pub keepout: bool,
    pub outline: Vec<Point>,
    /// Copper KiCad poured when the zones were last filled: (layer, polygon).
    pub fills: Vec<(String, Vec<Point>)>,
}

/// One entry of the board's `(layers ...)` table.
#[derive(Debug, Clone, serde::Serialize)]
pub struct LayerInfo {
    pub name: String,
    /// signal | power | mixed | jumper | user
    pub kind: String,
    /// The name the user gave it in KiCad, if any.
    pub user_name: String,
}

/// A line, rectangle, circle, arc or polygon on any layer (silkscreen, fab, courtyard, copper…).
#[derive(Debug, Clone)]
pub struct Graphic {
    pub layer: String,
    pub points: Vec<Point>,
    pub closed: bool,
    pub width: f64,
    pub filled: bool,
    /// Footprint reference for footprint graphics, empty for board graphics.
    pub owner: String,
}

/// Visible text (silkscreen references, fab values, board text).
#[derive(Debug, Clone)]
pub struct Text {
    pub layer: String,
    pub text: String,
    pub x: f64,
    pub y: f64,
    /// Degrees, counter-clockwise as in KiCad.
    pub angle: f64,
    /// Character height in mm.
    pub size: f64,
    pub mirrored: bool,
    /// left | right | "" (centred)
    pub justify: String,
    pub owner: String,
}

#[derive(Debug, Clone)]
pub struct Board {
    pub path: PathBuf,
    pub version: String,
    /// The board's layer table, in file order.
    pub layers: Vec<LayerInfo>,
    pub footprints: Vec<Footprint>,
    pub tracks: Vec<Track>,
    pub vias: Vec<Via>,
    pub zones: Vec<Zone>,
    /// Edge.Cuts polylines and whether each one is a closed shape by itself.
    pub edges: Vec<(Vec<Point>, bool)>,
    /// Every other graphic, from the board and its footprints.
    pub graphics: Vec<Graphic>,
    pub texts: Vec<Text>,
}

pub fn bbox_of(pts: &[Point]) -> Option<(f64, f64, f64, f64)> {
    if pts.is_empty() {
        return None;
    }
    let mut b = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for &(x, y) in pts {
        b = (b.0.min(x), b.1.min(y), b.2.max(x), b.3.max(y));
    }
    Some(b)
}

impl Board {
    pub fn footprint(&self, reference: &str) -> Option<&Footprint> {
        self.footprints.iter().find(|f| f.reference == reference)
    }

    pub fn pads(&self) -> impl Iterator<Item = &Pad> {
        self.footprints.iter().flat_map(|f| f.pads.iter())
    }

    /// Electrical pads grouped by net name (sorted for stable output).
    pub fn net_pads(&self) -> std::collections::BTreeMap<String, Vec<&Pad>> {
        let mut out: std::collections::BTreeMap<String, Vec<&Pad>> = Default::default();
        for p in self.pads() {
            if !p.net.is_empty() && p.electrical() {
                out.entry(p.net.clone()).or_default().push(p);
            }
        }
        out
    }

    pub fn edge_segments(&self) -> Vec<(Point, Point)> {
        self.edges.iter().flat_map(|(poly, _)| poly.windows(2).map(|w| (w[0], w[1]))).collect()
    }

    pub fn outline_bbox(&self) -> Option<(f64, f64, f64, f64)> {
        let pts: Vec<Point> = self.edges.iter().flat_map(|(p, _)| p.iter().copied()).collect();
        bbox_of(&pts)
    }

    pub fn outline_is_closed(&self) -> bool {
        if self.edges.is_empty() {
            return false;
        }
        let mut degree: HashMap<(i64, i64), u32> = HashMap::new();
        for (poly, closed) in &self.edges {
            if *closed || poly.is_empty() {
                continue;
            }
            for p in [poly[0], poly[poly.len() - 1]] {
                *degree.entry(((p.0 * 100.0).round() as i64, (p.1 * 100.0).round() as i64)).or_default() += 1;
            }
        }
        degree.values().all(|d| d % 2 == 0)
    }
}

/// Net number -> name table from the board's top-level `(net N "name")` entries.
pub fn net_table(root: &Sexp) -> HashMap<i64, String> {
    let mut nets = HashMap::new();
    for n in root.children("net") {
        if let Some(id) = n.arg(1).and_then(|v| v.parse::<i64>().ok()) {
            nets.insert(id, n.arg(2).unwrap_or_default().to_string());
        }
    }
    nets
}

/// Net name of a pad/segment/via/zone node, whichever way the KiCad version wrote it.
pub fn net_name(node: &Sexp, table: &HashMap<i64, String>) -> String {
    if let Some(name) = node.value("net_name") {
        return name.to_string();
    }
    let Some(n) = node.child("net") else { return String::new() };
    if let Some(name) = n.arg(2) {
        return name.to_string();
    }
    match n.arg(1) {
        Some(v) => match v.parse::<i64>() {
            Ok(id) => table.get(&id).cloned().unwrap_or_default(),
            Err(_) => v.to_string(),
        },
        None => String::new(),
    }
}

pub fn layer_of(node: &Sexp) -> String {
    node.value("layer").unwrap_or_default().to_string()
}

/// A node's `uuid` (KiCad 7+) or `tstamp` (older), or `#<ordinal>` when it has neither.
pub fn item_id(node: &Sexp, ordinal: usize) -> String {
    node.value("uuid").or_else(|| node.value("tstamp")).map(str::to_string).unwrap_or_else(|| format!("#{ordinal}"))
}

fn stroke_width(node: &Sexp) -> f64 {
    node.child("stroke").and_then(|s| s.value_f64("width")).or_else(|| node.value_f64("width")).unwrap_or(0.12)
}

fn is_filled(node: &Sexp) -> bool {
    node.child("fill").and_then(|f| f.arg(1)).is_some_and(|v| v == "solid" || v == "yes")
}

/// Hidden text: `(hide yes)` (KiCad 8+) or a bare `hide` flag on the node or its effects (older files).
fn is_hidden(node: &Sexp) -> bool {
    let flagged = |n: &Sexp| n.items().iter().any(|i| i.atom() == Some("hide")) || n.value("hide") == Some("yes");
    flagged(node) || node.child("effects").is_some_and(flagged)
}

/// Visible text from `property`, `fp_text` or `gr_text`, placed on the board.
fn text_of(node: &Sexp, transform: &dyn Fn(Point) -> Point, owner: &str, value: &str) -> Option<Text> {
    let raw = match node.head()? {
        "property" | "fp_text" => node.arg(2)?,
        "gr_text" => node.arg(1)?,
        _ => return None,
    };
    let layer = layer_of(node);
    if raw.is_empty() || layer.is_empty() || is_hidden(node) {
        return None;
    }
    let at = node.child("at");
    let (x, y) = transform(at.map(Sexp::xy).unwrap_or((0.0, 0.0)));
    let effects = node.child("effects");
    let justify = effects.and_then(|e| e.child("justify"));
    let has = |flag: &str| justify.is_some_and(|j| j.items().iter().any(|i| i.atom() == Some(flag)));
    let text = raw.replace("${REFERENCE}", owner).replace("%R", owner).replace("${VALUE}", value).replace("%V", value);
    Some(Text {
        layer,
        text,
        x,
        y,
        angle: at.and_then(|a| a.num(3)).unwrap_or(0.0),
        size: effects.and_then(|e| e.child("font")).and_then(|f| f.child("size")).and_then(|s| s.num(1)).unwrap_or(1.0),
        mirrored: has("mirror"),
        justify: if has("left") { "left".into() } else if has("right") { "right".into() } else { String::new() },
        owner: owner.to_string(),
    })
}

/// Points (board coordinates) and closedness of a gr_*/fp_* graphic.
fn shape_points(node: &Sexp, transform: &dyn Fn(Point) -> Point) -> (Vec<Point>, bool) {
    let head = node.head().unwrap_or_default();
    let kind = head.split_once('_').map(|(_, k)| k).unwrap_or_default();
    match kind {
        "line" => (vec![transform(node.child_xy("start")), transform(node.child_xy("end"))], false),
        "rect" => {
            let ((x1, y1), (x2, y2)) = (node.child_xy("start"), node.child_xy("end"));
            (vec![(x1, y1), (x2, y1), (x2, y2), (x1, y2), (x1, y1)].into_iter().map(transform).collect(), true)
        }
        "circle" => {
            let (c, e) = (node.child_xy("center"), node.child_xy("end"));
            (circle_points(c, (e.0 - c.0).hypot(e.1 - c.1), 32).into_iter().map(transform).collect(), true)
        }
        "arc" => {
            let (s, e) = (node.child_xy("start"), node.child_xy("end"));
            match node.child("mid") {
                Some(m) => (arc_points(s, m.xy(), e, 16).into_iter().map(transform).collect(), false),
                None => (vec![transform(s), transform(e)], false),
            }
        }
        "poly" => {
            let mut pts: Vec<Point> = node
                .child("pts")
                .map(|p| p.children("xy").map(|xy| transform(xy.xy())).collect())
                .unwrap_or_default();
            if let Some(&first) = pts.first() {
                pts.push(first);
            }
            (pts, true)
        }
        _ => (Vec::new(), false),
    }
}

pub fn load(path: &Path) -> Result<Board> {
    let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let mut board = parse_board(&text)?;
    board.path = path.to_path_buf();
    Ok(board)
}

pub fn parse_board(text: &str) -> Result<Board> {
    let root = sexpr::parse(text)?;
    if root.head() != Some("kicad_pcb") {
        bail!("not a KiCad board file (found '{}')", root.head().unwrap_or("?"));
    }

    let nets = net_table(&root);

    let mut footprints = Vec::new();
    let mut edges = Vec::new();
    let mut graphics = Vec::new();
    let mut texts = Vec::new();

    let layers = root
        .child("layers")
        .map(|l| {
            l.items()[1..]
                .iter()
                .filter_map(|e| {
                    Some(LayerInfo {
                        name: e.arg(1)?.to_string(),
                        kind: e.arg(2).unwrap_or("user").to_string(),
                        user_name: e.arg(3).unwrap_or_default().to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    for (ordinal, node) in root.children("footprint").chain(root.children("module")).enumerate() {
        let at = node.child("at");
        let (fx, fy) = at.map(Sexp::xy).unwrap_or((0.0, 0.0));
        let fangle = at.and_then(|a| a.num(3)).unwrap_or(0.0);
        let (mut reference, mut value) = (String::new(), String::new());
        let mut props = std::collections::BTreeMap::new();
        for prop in node.children("property") {
            let (Some(k), Some(v)) = (prop.arg(1), prop.arg(2)) else { continue };
            match k {
                "Reference" => reference = v.to_string(),
                "Value" => value = v.to_string(),
                _ => {}
            }
            props.insert(k.to_string(), v.to_string());
        }
        let mut attrs: Vec<String> = node.child("attr").map(|a| a.items()[1..].iter().filter_map(Sexp::atom).map(str::to_string).collect()).unwrap_or_default();
        // KiCad 8 writes some flags as separate nodes: (dnp yes) (exclude_from_bom yes) ...
        for flag in ["dnp", "exclude_from_bom", "exclude_from_pos_files"] {
            if node.value(flag).is_some_and(|v| v == "yes") && !attrs.iter().any(|a| a == flag) {
                attrs.push(flag.to_string());
            }
        }
        for t in node.children("fp_text") {
            match t.arg(1) {
                Some("reference") if reference.is_empty() => reference = t.arg(2).unwrap_or_default().to_string(),
                Some("value") if value.is_empty() => value = t.arg(2).unwrap_or_default().to_string(),
                _ => {}
            }
        }
        let transform = move |p: Point| {
            let (rx, ry) = rotate(p.0, p.1, fangle);
            (fx + rx, fy + ry)
        };

        let mut fp = Footprint {
            id: item_id(node, ordinal),
            reference: reference.clone(),
            value,
            lib_id: node.arg(1).unwrap_or_default().to_string(),
            x: fx,
            y: fy,
            angle: fangle,
            layer: layer_of(node),
            pads: Vec::new(),
            courtyard: Vec::new(),
            props,
            attrs,
        };

        for pad in node.children("pad") {
            let pat = pad.child("at");
            let local = pat.map(Sexp::xy).unwrap_or((0.0, 0.0));
            let (px, py) = transform(local);
            let size = pad.child("size");
            let w = size.and_then(|s| s.num(1)).unwrap_or(0.0);
            let h = size.and_then(|s| s.num(2)).unwrap_or(w);
            let drill = pad.child("drill").and_then(|d| {
                d.items()[1..].iter().filter_map(|x| x.atom()).filter_map(|x| x.parse::<f64>().ok()).reduce(f64::min)
            });
            fp.pads.push(Pad {
                fp_ref: reference.clone(),
                number: pad.arg(1).unwrap_or_default().to_string(),
                kind: pad.arg(2).unwrap_or_default().to_string(),
                shape: pad.arg(3).unwrap_or_default().to_string(),
                x: px,
                y: py,
                w,
                h,
                angle: pat.and_then(|a| a.num(3)).unwrap_or(0.0),
                layers: pad
                    .child("layers")
                    .map(|l| l.items()[1..].iter().filter_map(Sexp::atom).map(str::to_string).collect())
                    .unwrap_or_default(),
                net: net_name(pad, &nets),
                pinfunction: pad.value("pinfunction").unwrap_or_default().to_string(),
                drill,
            });
        }

        for g in node.items().iter().skip(1) {
            let Some(head) = g.head() else { continue };
            if head == "fp_text" || head == "property" {
                texts.extend(text_of(g, &transform, &fp.reference, &fp.value));
                continue;
            }
            if !head.starts_with("fp_") {
                continue;
            }
            let layer = layer_of(g);
            let (pts, closed) = shape_points(g, &transform);
            if pts.is_empty() {
                continue;
            }
            if layer == "Edge.Cuts" {
                edges.push((pts, closed));
                continue;
            }
            if layer.ends_with("CrtYd") {
                fp.courtyard.push(pts.clone());
            }
            graphics.push(Graphic { layer, points: pts, closed, width: stroke_width(g), filled: is_filled(g), owner: fp.reference.clone() });
        }
        footprints.push(fp);
    }

    for g in root.items().iter().skip(1) {
        let Some(head) = g.head() else { continue };
        if head == "gr_text" {
            texts.extend(text_of(g, &|p| p, "", ""));
            continue;
        }
        if !head.starts_with("gr_") {
            continue;
        }
        let (pts, closed) = shape_points(g, &|p| p);
        if pts.is_empty() {
            continue;
        }
        match layer_of(g) {
            l if l == "Edge.Cuts" => edges.push((pts, closed)),
            layer => graphics.push(Graphic { layer, points: pts, closed, width: stroke_width(g), filled: is_filled(g), owner: String::new() }),
        }
    }

    let mut tracks = Vec::new();
    for (ordinal, seg) in root.children("segment").enumerate() {
        tracks.push(Track {
            id: item_id(seg, ordinal),
            points: vec![seg.child_xy("start"), seg.child_xy("end")],
            width: seg.value_f64("width").unwrap_or(0.25),
            layer: layer_of(seg),
            net: net_name(seg, &nets),
        });
    }
    let segments = tracks.len();
    for (ordinal, arc) in root.children("arc").enumerate() {
        tracks.push(Track {
            id: item_id(arc, segments + ordinal),
            points: arc_points(arc.child_xy("start"), arc.child_xy("mid"), arc.child_xy("end"), 8),
            width: arc.value_f64("width").unwrap_or(0.25),
            layer: layer_of(arc),
            net: net_name(arc, &nets),
        });
    }

    let vias = root
        .children("via")
        .enumerate()
        .map(|(ordinal, v)| {
            let (x, y) = v.child_xy("at");
            Via {
                id: item_id(v, ordinal),
                x,
                y,
                size: v.value_f64("size").unwrap_or(0.6),
                drill: v.value_f64("drill").unwrap_or(0.3),
                net: net_name(v, &nets),
            }
        })
        .collect();

    let zones = root
        .children("zone")
        .map(|z| {
            let layers = match z.child("layers") {
                Some(l) => l.items()[1..].iter().filter_map(Sexp::atom).map(str::to_string).collect(),
                None => vec![layer_of(z)],
            };
            let outline = z
                .child("polygon")
                .and_then(|p| p.child("pts"))
                .map(|pts| pts.children("xy").map(Sexp::xy).collect())
                .unwrap_or_default();
            let fills = z
                .children("filled_polygon")
                .filter_map(|f| Some((layer_of(f), f.child("pts")?.children("xy").map(Sexp::xy).collect::<Vec<_>>())))
                .filter(|(_, pts)| pts.len() >= 3)
                .collect();
            Zone { net: net_name(z, &nets), layers, keepout: z.child("keepout").is_some(), outline, fills }
        })
        .collect();

    Ok(Board {
        path: PathBuf::new(),
        version: root.value("version").unwrap_or_default().to_string(),
        layers,
        footprints,
        tracks,
        vias,
        zones,
        edges,
        graphics,
        texts,
    })
}
