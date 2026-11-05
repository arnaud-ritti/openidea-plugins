//! Directed graphs drawn as an `Html` view body: a layered left-to-right
//! layout in inline SVG, themed with the IDE's CSS variables. Clicking a
//! node posts its id to the plugin (`ViewEvent::Message(id)`).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Node {
    pub id: String,
    pub label: String,
    /// A second, muted line (a type, a stereotype).
    pub detail: Option<String>,
    /// Drawn with the accent colour.
    pub accent: bool,
}

impl Node {
    pub fn new(id: &str, label: &str) -> Self {
        Self { id: id.into(), label: label.into(), ..Self::default() }
    }

    pub fn detail(mut self, detail: &str) -> Self {
        self.detail = Some(detail.into());
        self
    }

    pub fn accent(mut self, accent: bool) -> Self {
        self.accent = accent;
        self
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Edge {
    pub from: String,
    pub to: String,
    pub label: Option<String>,
    /// Drawn in the warning colour (a violation, a broken link).
    pub warning: bool,
}

impl Edge {
    pub fn new(from: &str, to: &str) -> Self {
        Self { from: from.into(), to: to.into(), ..Self::default() }
    }

    pub fn label(mut self, label: &str) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn warning(mut self, warning: bool) -> Self {
        self.warning = warning;
        self
    }
}

const CHAR_W: f64 = 7.0;
const NODE_H: f64 = 40.0;
const MAX_NODE_W: f64 = 320.0;
const GAP_X: f64 = 70.0;
const GAP_Y: f64 = 18.0;
const PAD: f64 = 16.0;
/// Room above the first row for edges that jump back over several columns.
const LANE: f64 = 16.0;

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&#39;")
}

const EDGE_LABEL_MAX: usize = 28;

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.into();
    }
    let mut short: String = s.chars().take(max - 1).collect();
    short.push('…');
    short
}

/// An SVG path along axis-aligned `points`, corners rounded.
fn orthogonal(points: &[(f64, f64)]) -> String {
    const R: f64 = 6.0;
    let step = |a: f64, b: f64| if b > a { 1.0 } else if b < a { -1.0 } else { 0.0 };
    let line = |d: &mut String, from: (f64, f64), (x, y): (f64, f64)| {
        let _ = if from.1 == y { write!(d, "H{x:.1}") } else { write!(d, "V{y:.1}") };
    };
    let mut d = format!("M{:.1},{:.1}", points[0].0, points[0].1);
    for w in points.windows(3) {
        let (p, (cx, cy), n) = (w[0], w[1], w[2]);
        line(&mut d, p, (cx - R * step(p.0, cx), cy - R * step(p.1, cy)));
        let _ = write!(d, "Q{cx:.1},{cy:.1} {:.1},{:.1}", cx + R * step(cx, n.0), cy + R * step(cy, n.1));
    }
    if let [.., p, last] = points {
        line(&mut d, *p, *last);
    }
    d
}

/// Layers by longest path from the sources, back edges of cycles ignored;
/// then a few barycentre sweeps to reduce crossings.
fn layout(nodes: &[Node], edges: &[(usize, usize)]) -> Vec<(usize, usize)> {
    let n = nodes.len();
    let mut out_edges = vec![Vec::new(); n];
    for &(a, b) in edges {
        out_edges[a].push(b);
    }
    // Back edges by DFS (iterative): an edge to a node on the stack.
    let mut state = vec![0u8; n]; // 0 new, 1 on stack, 2 done
    let mut forward: BTreeSet<(usize, usize)> = BTreeSet::new();
    for root in 0..n {
        if state[root] != 0 {
            continue;
        }
        let mut stack = vec![(root, 0usize)];
        state[root] = 1;
        while let Some(&mut (v, ref mut k)) = stack.last_mut() {
            if let Some(&w) = out_edges[v].get(*k) {
                *k += 1;
                match state[w] {
                    0 => {
                        forward.insert((v, w));
                        state[w] = 1;
                        stack.push((w, 0));
                    }
                    2 => {
                        forward.insert((v, w));
                    }
                    _ => {}
                }
            } else {
                state[v] = 2;
                stack.pop();
            }
        }
    }
    // Longest path layering over the DAG of forward edges (Kahn order).
    let mut indegree = vec![0; n];
    for &(_, b) in &forward {
        indegree[b] += 1;
    }
    let mut layer = vec![0usize; n];
    let mut queue: Vec<usize> = (0..n).filter(|&v| indegree[v] == 0).collect();
    while let Some(v) = queue.pop() {
        for &(a, b) in forward.range((v, 0)..(v + 1, 0)) {
            debug_assert_eq!(a, v);
            layer[b] = layer[b].max(layer[v] + 1);
            indegree[b] -= 1;
            if indegree[b] == 0 {
                queue.push(b);
            }
        }
    }
    let layers = layer.iter().max().map_or(0, |m| m + 1);
    let mut columns: Vec<Vec<usize>> = vec![Vec::new(); layers];
    for v in 0..n {
        columns[layer[v]].push(v);
    }
    let mut position = vec![0usize; n];
    let renumber = |columns: &Vec<Vec<usize>>, position: &mut Vec<usize>| {
        for col in columns {
            for (i, &v) in col.iter().enumerate() {
                position[v] = i;
            }
        }
    };
    renumber(&columns, &mut position);
    let mut neighbours = vec![Vec::new(); n];
    for &(a, b) in &forward {
        neighbours[a].push(b);
        neighbours[b].push(a);
    }
    for sweep in 0..4 {
        let order: Vec<usize> = if sweep % 2 == 0 { (1..layers).collect() } else { (0..layers.saturating_sub(1)).rev().collect() };
        for l in order {
            let side = if sweep % 2 == 0 { l.wrapping_sub(1) } else { l + 1 };
            let mut keyed: Vec<(f64, usize)> = columns[l]
                .iter()
                .map(|&v| {
                    let adj: Vec<usize> = neighbours[v].iter().copied().filter(|&w| layer[w] == side).collect();
                    let key = if adj.is_empty() {
                        position[v] as f64
                    } else {
                        adj.iter().map(|&w| position[w] as f64).sum::<f64>() / adj.len() as f64
                    };
                    (key, v)
                })
                .collect();
            keyed.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
            columns[l] = keyed.into_iter().map(|(_, v)| v).collect();
            renumber(&columns, &mut position);
        }
    }
    (0..n).map(|v| (layer[v], position[v])).collect()
}

/// The IDE's theme variables are set by the frame's script, which the desktop IDE blocks (LIMITATIONS.md): these system-colour defaults follow light/dark through color-scheme, and the IDE's values override them when the script runs.
pub const THEME_DEFAULTS: &str = ":root{--openidea-bg:Canvas;--openidea-bg-elevated:color-mix(in srgb,CanvasText 6%,Canvas);--openidea-bg-hover:color-mix(in srgb,CanvasText 12%,Canvas);--openidea-bg-selected:color-mix(in srgb,Highlight 35%,Canvas);--openidea-bg-input:Field;--openidea-fg:CanvasText;--openidea-fg-strong:CanvasText;--openidea-fg-muted:GrayText;--openidea-border:color-mix(in srgb,CanvasText 25%,Canvas);--openidea-focus-ring:Highlight;--openidea-link:LinkText;--openidea-accent:Highlight;--openidea-fg-on-accent:HighlightText;--openidea-danger:color-mix(in srgb,#db5c5c 75%,CanvasText);--openidea-warning:color-mix(in srgb,#d6ae58 75%,CanvasText);--openidea-success:color-mix(in srgb,#5fad65 75%,CanvasText);--openidea-info:color-mix(in srgb,#548af7 75%,CanvasText);--openidea-font-ui:system-ui,sans-serif;--openidea-font-mono:ui-monospace,Menlo,monospace;--openidea-font-size:13px}";

/// The `Html` body drawing the graph; `empty` is shown when there are no nodes.
/// Edges naming unknown nodes are dropped.
pub fn html(nodes: &[Node], edges: &[Edge], empty: &str) -> String {
    let style = ["<style>", THEME_DEFAULTS, "
body{margin:0}
.wrap{overflow:auto;width:100%;height:100vh}
.n{cursor:pointer}
.n:focus{outline:none}
.n rect{fill:var(--openidea-bg-elevated);stroke:var(--openidea-border);rx:6}
.n.a rect{stroke:var(--openidea-accent);stroke-width:1.5}
.n:hover rect{fill:var(--openidea-bg-hover)}
.n:focus-visible rect{stroke:var(--openidea-focus-ring);stroke-width:2}
.n text{fill:var(--openidea-fg);font:12px var(--openidea-font-ui);pointer-events:none}
.n text.d{fill:var(--openidea-fg-muted);font-size:10px}
.e{fill:none;stroke:var(--openidea-fg-muted);stroke-width:1.2}
.e.w{stroke:var(--openidea-warning)}
.ah{fill:var(--openidea-fg-muted)}
.ah.w{fill:var(--openidea-warning)}
.l{fill:var(--openidea-fg-muted);font:10px var(--openidea-font-ui);paint-order:stroke;stroke:var(--openidea-bg);stroke-width:3px}
.empty{padding:16px;color:var(--openidea-fg-muted);font-family:var(--openidea-font-ui)}
</style>"].concat();
    if nodes.is_empty() {
        return format!("{style}<div class=\"empty\">{}</div>", escape(empty));
    }
    let index: BTreeMap<&str, usize> = nodes.iter().enumerate().map(|(i, n)| (n.id.as_str(), i)).collect();
    let pairs: Vec<(usize, usize, &Edge)> = edges
        .iter()
        .filter_map(|e| Some((*index.get(e.from.as_str())?, *index.get(e.to.as_str())?, e)))
        .filter(|(a, b, _)| a != b)
        .collect();
    let places = layout(nodes, &pairs.iter().map(|&(a, b, _)| (a, b)).collect::<Vec<_>>());
    let max_chars = ((MAX_NODE_W - 20.0) / CHAR_W) as usize;
    let shown: Vec<(String, Option<String>)> = nodes
        .iter()
        .map(|n| (truncate(&n.label, max_chars), n.detail.as_deref().map(|d| truncate(d, max_chars * 12 / 10))))
        .collect();
    let widths: Vec<f64> = shown
        .iter()
        .map(|(label, detail)| {
            let chars = label.chars().count().max(detail.as_ref().map_or(0, |d| d.chars().count() * 10 / 12));
            (chars as f64 * CHAR_W + 20.0).clamp(60.0, MAX_NODE_W)
        })
        .collect();
    let layers = places.iter().map(|p| p.0).max().unwrap_or(0) + 1;
    let mut column_w = vec![0.0f64; layers];
    for i in 0..nodes.len() {
        column_w[places[i].0] = column_w[places[i].0].max(widths[i]);
    }
    let mut column_x = vec![PAD; layers];
    for l in 1..layers {
        column_x[l] = column_x[l - 1] + column_w[l - 1] + GAP_X;
    }
    let lane = pairs.iter().any(|&(a, b, _)| places[b].0 + 1 < places[a].0);
    let top = if lane { PAD + LANE } else { PAD };
    let xy = |i: usize| (column_x[places[i].0], top + places[i].1 as f64 * (NODE_H + GAP_Y));
    let total_w = column_x[layers - 1] + column_w[layers - 1] + PAD;
    let rows = places.iter().map(|p| p.1).max().unwrap_or(0) + 1;
    let total_h = top + PAD + rows as f64 * (NODE_H + GAP_Y);
    let mut svg = String::new();
    let _ = write!(
        svg,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{total_w:.0}\" height=\"{total_h:.0}\"><defs>\
<marker id=\"arrow\" viewBox=\"0 0 10 10\" refX=\"9\" refY=\"5\" markerWidth=\"7\" markerHeight=\"7\" orient=\"auto\"><path class=\"ah\" d=\"M0,0L10,5L0,10z\"/></marker>\
<marker id=\"arrow-w\" viewBox=\"0 0 10 10\" refX=\"9\" refY=\"5\" markerWidth=\"7\" markerHeight=\"7\" orient=\"auto\"><path class=\"ah w\" d=\"M0,0L10,5L0,10z\"/></marker></defs>"
    );
    for &(a, b, e) in &pairs {
        let (la, lb) = (places[a].0, places[b].0);
        let ((ax, ay), (bx, by)) = (xy(a), xy(b));
        let (ya, yb) = (ay + NODE_H / 2.0, by + NODE_H / 2.0);
        let (a_right, b_right) = (ax + widths[a], bx + widths[b]);
        // Back edges (always to an earlier column, see `layout`) enter b from
        // the right, through the empty gap after its column, so they never
        // cross a box.
        let b_column_right = bx + column_w[lb];
        let into_b = if b_right < b_column_right { format!("H{b_right:.1}") } else { String::new() };
        let (path, (lx, ly)) = if lb == la + 1 {
            let dx = (bx - a_right) / 2.0;
            let path = format!("M{a_right:.1},{ya:.1}C{:.1},{ya:.1} {:.1},{yb:.1} {bx:.1},{yb:.1}", a_right + dx, bx - dx);
            (path, ((a_right + bx) / 2.0, (ya + yb) / 2.0 - 4.0))
        } else if lb > la {
            // Forward over several columns: down the gap after a's column,
            // along the free row gap (or the slack below the last row) nearest
            // both ends, up the gap before b. Off the gap middles, where back
            // edges run.
            // ponytail: edges sharing a gap or channel overlap; spread them if that gets confusing.
            let (out, into) = (column_x[la] + column_w[la] + GAP_X / 2.0 - 8.0, column_x[lb] - GAP_X / 2.0 + 8.0);
            let cost = |y: f64| (y - ya).abs() + (y - yb).abs();
            let channel = (1..=rows)
                .map(|k| top + k as f64 * (NODE_H + GAP_Y) - GAP_Y / 2.0)
                .min_by(|p, q| cost(*p).total_cmp(&cost(*q)))
                .unwrap_or(top);
            let path = orthogonal(&[(a_right, ya), (out, ya), (out, channel), (into, channel), (into, yb), (bx, yb)]);
            (path, ((out + into) / 2.0, channel + 3.5))
        } else if lb + 1 == la {
            let k = GAP_X * 0.8;
            let path = format!("M{ax:.1},{ya:.1}C{:.1},{ya:.1} {:.1},{yb:.1} {b_column_right:.1},{yb:.1}{into_b}", ax - k, b_column_right + k);
            (path, ((ax + b_column_right) / 2.0, (ya + yb) / 2.0))
        } else {
            // Back over several columns: up the gap before a, along the lane
            // above the rows, down the gap after b's column.
            let lane_y = PAD;
            let (up, down) = (ax - GAP_X / 2.0, b_column_right + GAP_X / 2.0);
            let path = orthogonal(&[(ax, ya), (up, ya), (up, lane_y), (down, lane_y), (down, yb), (b_right, yb)]);
            (path, ((up + down) / 2.0, lane_y - 3.0))
        };
        let (class, marker) = if e.warning { ("e w", "arrow-w") } else { ("e", "arrow") };
        let _ = write!(svg, "<path class=\"{class}\" d=\"{path}\" marker-end=\"url(#{marker})\"/>");
        if let Some(label) = &e.label {
            // A long label (an expression) is cut short; the full text is its tooltip.
            let shown = truncate(label, EDGE_LABEL_MAX);
            let title = if shown == *label { String::new() } else { format!("<title>{}</title>", escape(label)) };
            let _ = write!(svg, "<text class=\"l\" x=\"{lx:.1}\" y=\"{ly:.1}\" text-anchor=\"middle\">{title}{}</text>", escape(&shown));
        }
    }
    for (i, n) in nodes.iter().enumerate() {
        let (x, y) = xy(i);
        let w = widths[i];
        let (label, detail) = &shown[i];
        let class = if n.accent { "n a" } else { "n" };
        let title = match &n.detail {
            Some(d) => format!("{} ({d})", n.label),
            None => n.label.clone(),
        };
        let text_y = if detail.is_some() { y + 17.0 } else { y + 24.0 };
        let _ = write!(
            svg,
            "<g class=\"{class}\" data-id=\"{}\" tabindex=\"0\" role=\"button\"><title>{}</title><rect x=\"{x:.1}\" y=\"{y:.1}\" width=\"{w:.1}\" height=\"{NODE_H}\"/>\
<text x=\"{:.1}\" y=\"{text_y:.1}\">{}</text>",
            escape(&n.id),
            escape(&title),
            x + 10.0,
            escape(label)
        );
        if let Some(detail) = detail {
            let _ = write!(svg, "<text class=\"d\" x=\"{:.1}\" y=\"{:.1}\">{}</text>", x + 10.0, y + 31.0, escape(detail));
        }
        svg.push_str("</g>");
    }
    svg.push_str("</svg>");
    format!(
        "{style}<div class=\"wrap\">{svg}</div><script>\
document.querySelectorAll('.n').forEach(function(g){{\
var open=function(){{openidea.postMessage(g.getAttribute('data-id'));}};\
g.addEventListener('click',open);\
g.addEventListener('keydown',function(e){{if(e.key==='Enter'||e.key===' '){{e.preventDefault();open();}}}});}});\
</script>"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layers_follow_edges_and_survive_cycles() {
        let nodes: Vec<Node> = ["a", "b", "c", "d"].iter().map(|id| Node::new(id, id)).collect();
        let places = layout(&nodes, &[(0, 1), (1, 2), (2, 0), (0, 3)]);
        assert_eq!(places[0].0, 0);
        assert_eq!(places[1].0, 1);
        assert_eq!(places[2].0, 2);
        assert_eq!(places[3].0, 1);
        assert_ne!(places[1].1, places[3].1, "same column, different rows");
    }

    #[test]
    fn draws_clickable_escaped_nodes() {
        let nodes = [Node::new("x", "<Order>").detail("@Service").accent(true), Node::new("y", "Repo")];
        let edges = [Edge::new("x", "y").label("uses"), Edge::new("x", "missing")];
        let page = html(&nodes, &edges, "nothing");
        assert!(page.contains("&lt;Order&gt;"));
        assert!(page.contains("data-id=\"x\""));
        assert_eq!(page.matches("marker-end").count(), 1, "the edge to an unknown node is dropped");
        assert!(page.contains("openidea.postMessage") && page.contains("keydown"));
        assert!(page.contains("tabindex=\"0\" role=\"button\""));
        assert!(html(&[], &[], "No beans").contains("No beans"));
    }

    #[test]
    fn truncates_long_edge_labels_but_keeps_them_in_the_title() {
        let long = "bookingService.isAvailable(booking, currentUser)";
        let page = html(&[Node::new("a", "A"), Node::new("b", "B")], &[Edge::new("a", "b").label(long), Edge::new("b", "a").label("else")], "");

        assert!(page.contains(&format!("<title>{long}</title>bookingService.isAvailable(…</text>")), "{page}");
        assert!(page.contains(">else</text>"));
    }

    #[test]
    fn truncates_long_labels_but_keeps_them_in_the_title() {
        let long = "x".repeat(100);
        let page = html(&[Node::new("x", &long).detail("@Bean")], &[], "");
        assert!(page.contains(&format!("<title>{long} (@Bean)</title>")));
        assert!(page.contains(&format!(">{}…<", "x".repeat(41))));
        assert!(page.contains("width=\"314.0\""), "{page}");
    }

    #[test]
    fn back_edges_leave_through_the_side_with_their_own_marker() {
        let nodes: Vec<Node> = ["a", "b", "c"].iter().map(|id| Node::new(id, id)).collect();
        let edges = [Edge::new("a", "b"), Edge::new("b", "c"), Edge::new("c", "a").warning(true), Edge::new("b", "a").label("back")];
        let page = html(&nodes, &edges, "");
        let paths: Vec<&str> = page.match_indices("<path class=\"e").map(|(i, _)| &page[i..i + page[i..].find("/>").unwrap()]).collect();
        assert_eq!(paths.len(), 4);
        assert!(paths[2].contains("e w") && paths[2].contains("url(#arrow-w)") && paths[2].contains('V'), "over the lane: {}", paths[2]);
        assert!(paths[3].contains("url(#arrow)") && !paths[3].contains('V'), "through the gap: {}", paths[3]);
        assert!(page.contains("id=\"arrow-w\"") && page.contains(">back<"));
    }

    fn attr(tag: &str, name: &str) -> f64 {
        let start = tag.find(&format!(" {name}=\"")).unwrap() + name.len() + 3;
        tag[start..start + tag[start..].find('"').unwrap()].parse().unwrap()
    }

    /// The path's segments; a curve contributes the legs to its control point, which bound it.
    fn segments(d: &str) -> Vec<((f64, f64), (f64, f64))> {
        let mut out = Vec::new();
        let mut at = (0.0, 0.0);
        let starts: Vec<usize> = d.match_indices(|c: char| c.is_ascii_alphabetic()).map(|(i, _)| i).collect();
        for (k, &i) in starts.iter().enumerate() {
            let end = starts.get(k + 1).copied().unwrap_or(d.len());
            let v: Vec<f64> = d[i + 1..end].split([',', ' ']).filter(|s| !s.is_empty()).map(|s| s.parse().unwrap()).collect();
            let points = match &d[i..i + 1] {
                "M" => {
                    at = (v[0], v[1]);
                    continue;
                }
                "H" => vec![(v[0], at.1)],
                "V" => vec![(at.0, v[0])],
                "Q" => vec![(v[0], v[1]), (v[2], v[3])],
                other => panic!("not orthogonal: {other} in {d}"),
            };
            for p in points {
                out.push((at, p));
                at = p;
            }
        }
        out
    }

    #[test]
    fn forward_edges_skipping_columns_go_around_the_boxes_between() {
        let nodes: Vec<Node> = ["a", "b", "c", "d", "e"].iter().map(|id| Node::new(id, id)).collect();
        let edges =
            [Edge::new("a", "b"), Edge::new("b", "c"), Edge::new("c", "d"), Edge::new("a", "e"), Edge::new("a", "d").label("skip").warning(true)];
        let page = html(&nodes, &edges, "");
        let rects: Vec<(f64, f64, f64, f64)> = page
            .match_indices("<rect ")
            .map(|(i, _)| {
                let tag = &page[i..i + page[i..].find("/>").unwrap()];
                let (x, y) = (attr(tag, "x"), attr(tag, "y"));
                (x, y, x + attr(tag, "width"), y + attr(tag, "height"))
            })
            .collect();
        let paths: Vec<&str> = page.match_indices("<path class=\"e").map(|(i, _)| &page[i..i + page[i..].find("/>").unwrap()]).collect();
        let skip = paths[4];
        assert!(skip.contains("e w") && skip.contains("url(#arrow-w)"), "{skip}");
        let d = &skip[skip.find(" d=\"").unwrap() + 4..];
        let d = &d[..d.find('"').unwrap()];
        let segments = segments(d);
        let (first, last) = (segments[0].0, segments[segments.len() - 1].1);
        assert_eq!((first.0, last.0), (rects[0].2, rects[3].0), "leaves a on the right, enters d on the left: {d}");
        assert!(last.0 > segments[segments.len() - 1].0 .0, "the arrowhead points right, into d");
        for ((x1, y1), (x2, y2)) in segments {
            assert!(x1 == x2 || y1 == y2, "orthogonal: {d}");
            for &(left, top, right, bottom) in &rects {
                let crosses = x1.max(x2) > left && x1.min(x2) < right && y1.max(y2) > top && y1.min(y2) < bottom;
                assert!(!crosses, "segment ({x1},{y1})-({x2},{y2}) crosses box {left},{top}: {d}");
            }
        }
        assert!(page.contains(">skip<"));
    }
}
