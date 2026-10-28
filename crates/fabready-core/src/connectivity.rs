//! Approximate copper connectivity: are all pads of each net joined by tracks, vias and zones?

use std::collections::{BTreeMap, BTreeSet, HashMap};

use regex::RegexBuilder;

use crate::board::{point_in_polygon, point_segment_distance, Board, Pad, Track, Via, Zone};
use crate::findings::{Finding, Severity};
use crate::keyboard::CONTROLLER_RE;

struct UnionFind(Vec<usize>);

impl UnionFind {
    fn new(n: usize) -> Self {
        UnionFind((0..n).collect())
    }
    fn find(&mut self, mut i: usize) -> usize {
        while self.0[i] != i {
            self.0[i] = self.0[self.0[i]];
            i = self.0[i];
        }
        i
    }
    fn union(&mut self, a: usize, b: usize) {
        let (ra, rb) = (self.find(a), self.find(b));
        self.0[ra] = rb;
    }
}

enum Item<'a> {
    Pad(&'a Pad),
    Track(&'a Track),
    Via(&'a Via),
    Zone(&'a Zone),
}

fn track_touches_pad(t: &Track, pad: &Pad) -> bool {
    pad.on_layer(&t.layer)
        && t.segments().any(|(a, b)| pad.contains(a) || pad.contains(b) || point_segment_distance(pad.center(), a, b) <= t.width / 2.0)
}

fn tracks_touch(t1: &Track, t2: &Track) -> bool {
    if t1.layer != t2.layer {
        return false;
    }
    for (a, b) in t1.segments() {
        for (c, d) in t2.segments() {
            if point_segment_distance(a, c, d) <= t2.width / 2.0 + 1e-3
                || point_segment_distance(b, c, d) <= t2.width / 2.0 + 1e-3
                || point_segment_distance(c, a, b) <= t1.width / 2.0 + 1e-3
                || point_segment_distance(d, a, b) <= t1.width / 2.0 + 1e-3
            {
                return true;
            }
        }
    }
    false
}

fn zone_touches(z: &Zone, item: &Item) -> bool {
    match item {
        Item::Pad(p) => z.layers.iter().any(|l| p.on_layer(l)) && point_in_polygon(p.center(), &z.outline),
        Item::Via(v) => point_in_polygon((v.x, v.y), &z.outline),
        Item::Track(t) => z.layers.contains(&t.layer) && t.points.iter().any(|&p| point_in_polygon(p, &z.outline)),
        Item::Zone(_) => false,
    }
}

fn touching(a: &Item, b: &Item, module_refs: &BTreeSet<String>) -> bool {
    match (a, b) {
        (Item::Pad(a), Item::Pad(b)) => {
            let module = module_refs.contains(&a.fp_ref);
            a.fp_ref == b.fp_ref && (a.number == b.number || module) && (module || a.contains(b.center()) || b.contains(a.center()))
        }
        (Item::Pad(p), Item::Track(t)) | (Item::Track(t), Item::Pad(p)) => track_touches_pad(t, p),
        (Item::Pad(p), Item::Via(v)) | (Item::Via(v), Item::Pad(p)) => p.contains((v.x, v.y)),
        (Item::Track(a), Item::Track(b)) => tracks_touch(a, b),
        (Item::Track(t), Item::Via(v)) | (Item::Via(v), Item::Track(t)) => {
            t.segments().any(|(a, b)| point_segment_distance((v.x, v.y), a, b) <= v.size / 2.0)
        }
        (Item::Via(a), Item::Via(b)) => (a.x - b.x).hypot(a.y - b.y) <= (a.size + b.size) / 2.0,
        (Item::Zone(z), other) | (other, Item::Zone(z)) => zone_touches(z, other),
    }
}

pub fn check(board: &Board) -> Vec<Finding> {
    let mut findings = Vec::new();
    let net_pads = board.net_pads();
    let copper_zones: Vec<&Zone> = board.zones.iter().filter(|z| !z.keepout && !z.net.is_empty() && z.outline.len() >= 3).collect();

    if board.tracks.is_empty() && copper_zones.is_empty() && net_pads.values().any(|p| p.len() > 1) {
        return vec![Finding::new("conn.not-routed", Severity::Error, "The board has no tracks or copper zones yet.")];
    }

    let mut tracks_by_net: HashMap<&str, Vec<&Track>> = HashMap::new();
    for t in &board.tracks {
        tracks_by_net.entry(t.net.as_str()).or_default().push(t);
    }
    let mut vias_by_net: HashMap<&str, Vec<&Via>> = HashMap::new();
    for v in &board.vias {
        vias_by_net.entry(v.net.as_str()).or_default().push(v);
    }
    // Plug-in controller modules (nice!nano, XIAO, ...) join same-net pins (e.g. several GND pins) internally.
    let controller_re = RegexBuilder::new(CONTROLLER_RE).case_insensitive(true).build().unwrap();
    let module_refs: BTreeSet<String> =
        board.footprints.iter().filter(|f| controller_re.is_match(&f.text())).map(|f| f.reference.clone()).collect();

    let mut zone_nets = BTreeSet::new();
    for (net, pads) in &net_pads {
        if pads.len() < 2 || net.starts_with("unconnected-") {
            continue;
        }
        if pads.iter().map(|p| &p.fp_ref).collect::<BTreeSet<_>>().len() < 2 {
            continue; // all pads on one footprint, usually joined internally
        }
        let zones: Vec<&Zone> = copper_zones.iter().copied().filter(|z| &z.net == net).collect();
        if !zones.is_empty() {
            zone_nets.insert(net.clone());
        }
        let mut items: Vec<Item> = pads.iter().map(|p| Item::Pad(p)).collect();
        items.extend(tracks_by_net.get(net.as_str()).into_iter().flatten().map(|t| Item::Track(t)));
        items.extend(vias_by_net.get(net.as_str()).into_iter().flatten().map(|v| Item::Via(v)));
        items.extend(zones.iter().map(|z| Item::Zone(z)));

        let mut uf = UnionFind::new(items.len());
        for i in 0..items.len() {
            for j in i + 1..items.len() {
                if touching(&items[i], &items[j], &module_refs) {
                    uf.union(i, j);
                }
            }
        }
        let mut groups: BTreeMap<usize, Vec<&Pad>> = BTreeMap::new();
        for (i, p) in pads.iter().enumerate() {
            groups.entry(uf.find(i)).or_default().push(p);
        }
        if groups.len() > 1 {
            let mut pieces: Vec<Vec<&Pad>> = groups.into_values().collect();
            pieces.sort_by_key(|g| std::cmp::Reverse(g.len()));
            let stray: Vec<&Pad> = pieces[1..].iter().flatten().copied().collect();
            let names = stray.iter().take(6).map(|p| format!("{}.{}", p.fp_ref, p.number)).collect::<Vec<_>>().join(", ");
            let refs: BTreeSet<&str> = stray.iter().map(|p| p.fp_ref.as_str()).collect();
            findings.push(
                Finding::new(
                    "conn.unrouted",
                    Severity::Error,
                    format!("Net {net} is split into {} unconnected pieces; not joined to the rest: {names}.", pieces.len()),
                )
                .refs(refs)
                .at(stray[0].center()),
            );
        }
    }

    if !zone_nets.is_empty() {
        findings.push(Finding::new(
            "conn.zone-approx",
            Severity::Info,
            format!("Zone outlines were treated as solid copper for: {}.", zone_nets.into_iter().collect::<Vec<_>>().join(", ")),
        ));
    }

    for fp in &board.footprints {
        let elec: Vec<&Pad> = fp.pads.iter().filter(|p| p.electrical()).collect();
        if !elec.is_empty() && elec.iter().all(|p| p.net.is_empty()) && !fp.is_mounting_hole() {
            findings.push(
                Finding::new(
                    "conn.footprint-no-nets",
                    Severity::Warning,
                    format!("{} ({}) has {} pads and none are connected.", fp.reference, fp.value, elec.len()),
                )
                .refs([fp.reference.clone()])
                .at((fp.x, fp.y)),
            );
        }
    }
    findings
}
