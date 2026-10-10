//! The knowledge graph drawn: every paper of a subject a dot at its place in the graph's layout (graph.rs), coloured
//! by its thread and sized by its influence, with the subject's citations faint behind them. The chosen paper's
//! citations are drawn gold and its neighbours ringed; a chosen thread is lit and the others dimmed. Under the
//! pointer a paper says what it is. The dots and faint lines are drawn once into a cache and kept until the graph or
//! the chosen thread changes.

use std::collections::HashMap;

use alelyon_research::graph::Role;
use alelyon_research::store::StoredGraph;
use iced::widget::canvas::{self, Cache, Frame, Path, Stroke, Text};
use iced::{Color, Point, Rectangle, Renderer, Size, Theme, mouse};

use crate::theme;

use super::Msg;

/// Thread colours, cycled; the unconnected bucket is grey.
const PALETTE: [Color; 10] = [
    Color::from_rgb(0.88, 0.70, 0.26),
    Color::from_rgb(0.36, 0.62, 0.80),
    Color::from_rgb(0.55, 0.78, 0.47),
    Color::from_rgb(0.84, 0.45, 0.42),
    Color::from_rgb(0.66, 0.52, 0.82),
    Color::from_rgb(0.42, 0.78, 0.74),
    Color::from_rgb(0.90, 0.58, 0.30),
    Color::from_rgb(0.80, 0.55, 0.70),
    Color::from_rgb(0.62, 0.68, 0.38),
    Color::from_rgb(0.55, 0.60, 0.70),
];
const UNCONNECTED: Color = Color::from_rgb(0.40, 0.40, 0.40);
const MARGIN: f32 = 18.0;
/// The most citations drawn behind the dots.
pub const BACKGROUND_LINES: usize = 6_000;

/// The graph with what drawing and picking need: each paper's index, and the citations as index pairs.
#[derive(Debug, Default)]
pub struct Drawn {
    pub graph: StoredGraph,
    pub index: HashMap<i64, usize>,
    pub cites: Vec<(usize, usize)>,
    /// The citations drawn behind the dots: those between the most influential papers, at most `BACKGROUND_LINES`.
    pub background: Vec<(usize, usize)>,
    /// Threads whose label is the unconnected bucket.
    pub unconnected: Option<usize>,
}

impl Drawn {
    pub fn new(graph: StoredGraph) -> Drawn {
        let index: HashMap<i64, usize> = graph.nodes.iter().enumerate().map(|(i, n)| (n.paper.work, i)).collect();
        let cites: Vec<(usize, usize)> = graph
            .edges
            .iter()
            .filter(|e| e.2 == "cites")
            .filter_map(|e| Some((*index.get(&e.0)?, *index.get(&e.1)?)))
            .collect();
        // Every citation of a big subject drawn faintly is noise to a reader and a large mesh for the graphics driver
        // on every frame; the strongest few thousand show the structure.
        let mut background = cites.clone();
        if background.len() > BACKGROUND_LINES {
            let rank = |&(a, b): &(usize, usize)| graph.nodes[a].pagerank + graph.nodes[b].pagerank;
            background.sort_by(|x, y| rank(y).total_cmp(&rank(x)).then(x.cmp(y)));
            background.truncate(BACKGROUND_LINES);
        }
        let unconnected = graph.communities.iter().find(|c| c.label == alelyon_research::graph::NOT_CONNECTED).map(|c| c.id);
        Drawn { graph, index, cites, background, unconnected }
    }

    pub fn colour(&self, thread: usize) -> Color {
        if Some(thread) == self.unconnected { UNCONNECTED } else { PALETTE[thread % PALETTE.len()] }
    }
}

/// Where a layout position (in the unit square) lands in `size`.
pub fn place(x: f32, y: f32, size: Size) -> Point {
    let side = (size.width - 2.0 * MARGIN).min(size.height - 2.0 * MARGIN).max(1.0);
    let ox = (size.width - side) / 2.0;
    let oy = (size.height - side) / 2.0;
    Point::new(ox + x * side, oy + y * side)
}

/// A paper's dot radius: by influence, relative to the subject's most influential.
fn radius(pagerank: f64, top: f64) -> f32 {
    2.0 + 7.0 * (pagerank / top.max(f64::MIN_POSITIVE)).sqrt() as f32
}

/// The paper under `cursor`: the nearest within reach of its dot.
pub fn under(drawn: &Drawn, size: Size, cursor: Point) -> Option<usize> {
    let top = drawn.graph.nodes.iter().map(|n| n.pagerank).fold(0.0, f64::max);
    drawn
        .graph
        .nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (i, place(n.x, n.y, size).distance(cursor), radius(n.pagerank, top) + 3.0))
        .filter(|(_, d, reach)| d <= reach)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _, _)| i)
}

pub struct Map<'a> {
    pub drawn: &'a Drawn,
    pub cache: &'a Cache,
    pub selected: Option<usize>,
    pub thread: Option<usize>,
}

impl canvas::Program<Msg> for Map<'_> {
    type State = ();

    fn update(&self, _: &mut (), event: &iced::Event, bounds: Rectangle, cursor: mouse::Cursor) -> Option<canvas::Action<Msg>> {
        if let iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) = event {
            let at = cursor.position_in(bounds)?;
            let hit = under(self.drawn, bounds.size(), at).map(|i| self.drawn.graph.nodes[i].paper.work);
            return Some(canvas::Action::publish(Msg::Select(hit)).and_capture());
        }
        None
    }

    fn draw(&self, _: &(), renderer: &Renderer, _: &Theme, bounds: Rectangle, cursor: mouse::Cursor) -> Vec<canvas::Geometry> {
        let size = bounds.size();
        let nodes = &self.drawn.graph.nodes;
        let top = nodes.iter().map(|n| n.pagerank).fold(0.0, f64::max);
        let lit = |c: usize| self.thread.is_none_or(|t| t == c);
        let base = self.cache.draw(renderer, size, |frame: &mut Frame| {
            let faint = Stroke::default().with_color(theme::with_alpha(theme::TEXT, 0.035)).with_width(0.6);
            for &(a, b) in &self.drawn.background {
                if lit(nodes[a].community) || lit(nodes[b].community) {
                    frame.stroke(&Path::line(place(nodes[a].x, nodes[a].y, size), place(nodes[b].x, nodes[b].y, size)), faint);
                }
            }
            // Least influential first, so the foundations sit on top.
            let mut order: Vec<usize> = (0..nodes.len()).collect();
            order.sort_by(|&a, &b| nodes[a].pagerank.total_cmp(&nodes[b].pagerank));
            for i in order {
                let n = &nodes[i];
                let colour = self.drawn.colour(n.community);
                let colour = if lit(n.community) { colour } else { theme::with_alpha(colour, 0.12) };
                let p = place(n.x, n.y, size);
                frame.fill(&Path::circle(p, radius(n.pagerank, top)), colour);
                if n.role == Role::Foundation && lit(n.community) {
                    frame.stroke(&Path::circle(p, radius(n.pagerank, top) + 1.5), Stroke::default().with_color(theme::TEXT).with_width(1.0));
                }
            }
            // The largest threads' names at their centres.
            for c in self.drawn.graph.communities.iter().filter(|c| Some(c.id) != self.drawn.unconnected).take(8) {
                if !lit(c.id) {
                    continue;
                }
                let members: Vec<&_> = nodes.iter().filter(|n| n.community == c.id).collect();
                // Small threads' names would cover each other; the legend beside the map names every thread.
                if members.len() < 40 {
                    continue;
                }
                let (sx, sy) = members.iter().fold((0.0, 0.0), |acc, n| (acc.0 + n.x, acc.1 + n.y));
                let at = place(sx / members.len() as f32, sy / members.len() as f32, size);
                frame.fill_text(Text {
                    content: c.label.clone(),
                    position: Point::new(at.x, at.y),
                    color: theme::TEXT,
                    size: 12.5.into(),
                    align_x: iced::widget::text::Alignment::Center,
                    ..Text::default()
                });
            }
        });

        let mut over = Frame::new(renderer, size);
        if let Some(s) = self.selected.filter(|&s| s < nodes.len()) {
            let from = place(nodes[s].x, nodes[s].y, size);
            for &(a, b) in &self.drawn.cites {
                if a == s || b == s {
                    let other = if a == s { b } else { a };
                    let to = place(nodes[other].x, nodes[other].y, size);
                    // What it cites in gold, what cites it in blue.
                    let colour = if a == s { theme::GOLD } else { Color::from_rgb(0.36, 0.62, 0.80) };
                    over.stroke(&Path::line(from, to), Stroke::default().with_color(theme::with_alpha(colour, 0.75)).with_width(1.2));
                    over.stroke(&Path::circle(to, radius(nodes[other].pagerank, top) + 2.0), Stroke::default().with_color(colour).with_width(1.2));
                }
            }
            over.stroke(&Path::circle(from, radius(nodes[s].pagerank, top) + 3.0), Stroke::default().with_color(theme::GOLD).with_width(2.0));
        }
        if let Some(h) = cursor.position_in(bounds).and_then(|p| under(self.drawn, size, p)) {
            let n = &nodes[h];
            let thread = self.drawn.graph.communities.iter().find(|c| c.id == n.community).map(|c| c.label.as_str()).unwrap_or("");
            let lines = [
                crate::ui::cut(&n.paper.title, 110),
                format!(
                    "{} · {} · thread: {thread}",
                    n.paper.year.map_or("year unknown".into(), |y| y.to_string()),
                    n.role.name()
                ),
                format!("cites {} and is cited by {} papers of this subject", n.cites, n.cited_by),
            ];
            let mut y = 6.0;
            for (k, line) in lines.iter().enumerate() {
                over.fill_text(Text {
                    content: line.clone(),
                    position: Point::new(8.0, y),
                    color: if k == 0 { theme::TEXT } else { theme::TEXT_DIM },
                    size: (if k == 0 { 13.0 } else { 11.5 }).into(),
                    ..Text::default()
                });
                y += 16.0;
            }
        }
        vec![base, over.into_geometry()]
    }

    fn mouse_interaction(&self, _: &(), bounds: Rectangle, cursor: mouse::Cursor) -> mouse::Interaction {
        match cursor.position_in(bounds).and_then(|p| under(self.drawn, bounds.size(), p)) {
            Some(_) => mouse::Interaction::Pointer,
            None => mouse::Interaction::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alelyon_research::graph::Community;
    use alelyon_research::store::{GraphNode, PaperRef};

    fn node(work: i64, x: f32, y: f32, pagerank: f64) -> GraphNode {
        GraphNode {
            paper: PaperRef { work, title: format!("p{work}"), year: Some(2000), ids: vec![] },
            community: 0,
            pagerank,
            role: Role::Member,
            cites: 0,
            cited_by: 0,
            x,
            y,
        }
    }

    #[test]
    fn the_background_keeps_the_strongest_citations_only() {
        let n = 200;
        let nodes: Vec<GraphNode> = (0..n).map(|i| node(i as i64, 0.5, 0.5, i as f64)).collect();
        let mut edges = Vec::new();
        for a in 0..n as i64 {
            for b in 0..n as i64 {
                if a != b {
                    edges.push((a, b, "cites".to_string(), 1.0));
                }
            }
        }
        let d = Drawn::new(StoredGraph { nodes, edges, communities: vec![] });
        assert_eq!(d.cites.len(), 200 * 199);
        assert_eq!(d.background.len(), BACKGROUND_LINES);
        // The strongest pair (the two most influential papers) is in; a pair of the two weakest is not.
        assert!(d.background.contains(&(199, 198)));
        assert!(!d.background.contains(&(0, 1)));
    }

    #[test]
    fn every_paper_lands_inside_and_the_pointer_finds_it() {
        let g = StoredGraph {
            nodes: vec![node(10, 0.0, 0.0, 0.1), node(11, 1.0, 1.0, 0.5), node(12, 0.5, 0.5, 0.2)],
            edges: vec![(10, 11, "cites".into(), 1.0), (11, 12, "similar".into(), 0.3), (10, 99, "cites".into(), 1.0)],
            communities: vec![Community { id: 0, label: "t".into(), size: 3, years: None }],
        };
        let d = Drawn::new(g);
        assert_eq!(d.cites, vec![(0, 1)], "only citations between drawn papers");
        assert_eq!(d.background, d.cites);
        let size = Size::new(600.0, 400.0);
        for n in &d.graph.nodes {
            let p = place(n.x, n.y, size);
            assert!((0.0..=size.width).contains(&p.x) && (0.0..=size.height).contains(&p.y));
        }
        assert_eq!(under(&d, size, place(1.0, 1.0, size)), Some(1));
        assert_eq!(under(&d, size, Point::new(300.0 + 40.0, 200.0 + 40.0)), None);
    }
}
