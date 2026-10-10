//! A model's measured anatomy laid out as a STYLISED voxel brain (the morphometry voxel model shaped
//! into a brain that is properly separated into regions, drawn as stylised cubes; shown in the Morphometry
//! tab and, with the face session, inside Sinai's glass head).
//!
//! This is a DISPLAY MAPPING, a metaphor drawn on top of the measurement. The canonical voxel field and the cells the
//! engine measured are unchanged (the space commitment covers that frame, not this picture), and a transformer has no
//! anatomy that matches a brain region for region. The mapping:
//! - token embedding -> brainstem and thalamus, where input enters;
//! - normalisation -> the white-matter core;
//! - attention (query, key, value, output) -> the cortex shell, its blocks laid from the back of the head to the front;
//! - feed-forward (gate, up, down) -> the association lobes, below and beside the cortex;
//! - the expert bank (mixture of experts) -> the cerebellum, many small specialised folds;
//! - the output projection -> the frontal pole.
//!
//! One function maps a model location to a place in the brain: a regular grid over the brain's shape mask, each grid
//! point given to a region by where it lies, and each region's points shared among that region's cells in order (block
//! by block) and in proportion to their parameters. `resolution` sets the grid: coarse for the stylised cubes now; the
//! same mapping sampled finely is the dense, realistic picture a per-parameter view would need. A region the model
//! does not use keeps its points, drawn faint and flagged empty, so the brain keeps its shape without implying data.

use model_anatomy::Cell;

/// The brain's regions, in the order they are listed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Region {
    Brainstem,
    WhiteMatter,
    Cortex,
    Association,
    Cerebellum,
    Frontal,
}

impl Region {
    pub const ALL: [Region; 6] =
        [Region::Brainstem, Region::WhiteMatter, Region::Cortex, Region::Association, Region::Cerebellum, Region::Frontal];

    pub fn name(self) -> &'static str {
        match self {
            Region::Brainstem => "Brainstem and thalamus",
            Region::WhiteMatter => "White matter",
            Region::Cortex => "Cortex",
            Region::Association => "Association lobes",
            Region::Cerebellum => "Cerebellum",
            Region::Frontal => "Frontal pole",
        }
    }

    /// What part of the model it stands for.
    pub fn stands_for(self) -> &'static str {
        match self {
            Region::Brainstem => "the token embedding, where input enters",
            Region::WhiteMatter => "the normalisations",
            Region::Cortex => "attention, its blocks from the back of the head to the front",
            Region::Association => "the feed-forward gate, up and down projections",
            Region::Cerebellum => "the expert bank of a mixture of experts",
            Region::Frontal => "the output projection",
        }
    }

    /// The region's colour (linear RGB), before a cell's share brightens it.
    pub fn colour(self) -> [f32; 3] {
        match self {
            Region::Brainstem => [0.55, 0.42, 0.85],
            Region::WhiteMatter => [0.78, 0.76, 0.70],
            Region::Cortex => [0.90, 0.72, 0.30],
            Region::Association => [0.33, 0.70, 0.62],
            Region::Cerebellum => [0.85, 0.42, 0.50],
            Region::Frontal => [0.95, 0.55, 0.25],
        }
    }
}

/// The region a module's cells go to (by the engine's module ids).
pub fn region_of(module: &str) -> Region {
    match module {
        "token_embd" => Region::Brainstem,
        "attn_norm" | "attn_q_norm" | "attn_k_norm" | "ffn_norm" | "output_norm" => Region::WhiteMatter,
        "attn_q" | "attn_k" | "attn_v" | "attn_output" => Region::Cortex,
        "ffn_gate" | "ffn_up" | "ffn_down" => Region::Association,
        "ffn_expert" => Region::Cerebellum,
        // "output" and the engine's "other" (unclassified) go to the front: what leaves the model.
        _ => Region::Frontal,
    }
}

/// An ellipsoid: centre and radii, in brain space.
#[derive(Clone, Copy)]
struct Ellipsoid {
    c: [f32; 3],
    r: [f32; 3],
}

impl Ellipsoid {
    /// The normalised radius of `p`: below 1 inside.
    fn rho(&self, p: [f32; 3]) -> f32 {
        let d = |i: usize| (p[i] - self.c[i]) / self.r[i];
        (d(0) * d(0) + d(1) * d(1) + d(2) * d(2)).sqrt()
    }
}

// Brain space: x to the right, y up, z to the front (the face looks along +z), within [-1, 1].
const HEMI_L: Ellipsoid = Ellipsoid { c: [-0.40, 0.15, 0.05], r: [0.52, 0.62, 0.92] };
const HEMI_R: Ellipsoid = Ellipsoid { c: [0.40, 0.15, 0.05], r: [0.52, 0.62, 0.92] };
const CEREBELLUM: Ellipsoid = Ellipsoid { c: [0.0, -0.45, -0.66], r: [0.62, 0.28, 0.30] };
const STEM: Ellipsoid = Ellipsoid { c: [0.0, -0.62, -0.25], r: [0.16, 0.40, 0.17] };
const THALAMUS: Ellipsoid = Ellipsoid { c: [0.0, -0.05, -0.10], r: [0.22, 0.18, 0.26] };
/// The gap between the hemispheres (the longitudinal fissure), as a half-width.
const FISSURE: f32 = 0.035;
/// Inside this normalised radius of a hemisphere is white matter; outside it, the cortex and the lobes.
const CORE: f32 = 0.62;
/// In front of this z the shell is the frontal pole.
const FRONT: f32 = 0.62;

/// Which region a point of the grid is in, or `None` outside the brain.
pub fn region_at(p: [f32; 3]) -> Option<Region> {
    if STEM.rho(p) < 1.0 || THALAMUS.rho(p) < 1.0 {
        return Some(Region::Brainstem);
    }
    if CEREBELLUM.rho(p) < 1.0 {
        return Some(Region::Cerebellum);
    }
    if p[0].abs() < FISSURE {
        return None;
    }
    let hemi = if p[0] < 0.0 { HEMI_L } else { HEMI_R };
    let rho = hemi.rho(p);
    if rho >= 1.0 {
        return None;
    }
    if rho < CORE {
        return Some(Region::WhiteMatter);
    }
    if p[2] > FRONT {
        return Some(Region::Frontal);
    }
    Some(if p[1] > 0.05 { Region::Cortex } else { Region::Association })
}

/// One cube of the brain, in brain space ([-1, 1] on each axis).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cube {
    pub centre: [f32; 3],
    /// Edge length.
    pub size: f32,
    /// Linear RGBA (premultiplied alpha is the renderer's to apply).
    pub colour: [f32; 4],
    pub region: Region,
    /// The index (into the cells given) of the cell this cube stands for; `None` for a region the model leaves empty.
    pub cell: Option<usize>,
}

/// One region's share of the picture.
#[derive(Clone, Debug, PartialEq)]
pub struct RegionShare {
    pub region: Region,
    /// Cells of the model in this region, and their parameters.
    pub cells: usize,
    pub parameters: i64,
    /// Cubes the region has, and cells too small to get one at this resolution.
    pub cubes: usize,
    pub unshown_cells: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Brain {
    pub cubes: Vec<Cube>,
    pub regions: Vec<RegionShare>,
    pub resolution: usize,
}

/// The order a region's points are handed out in: blocks run from the back of the head to the front (and up the stem).
fn order_key(region: Region, p: [f32; 3]) -> (i64, i64, i64) {
    let q = |v: f32| (v * 10_000.0).round() as i64;
    match region {
        Region::Brainstem => (q(p[1]), q(p[2]), q(p[0])),
        Region::Cerebellum => (q(p[0]), q(p[2]), q(p[1])),
        _ => (q(p[2]), q(p[1]), q(p[0])),
    }
}

/// Lay `cells` out as a brain on a `resolution`-cube grid (at least 4).
pub fn build(cells: &[Cell], resolution: usize) -> Brain {
    let n = resolution.max(4);
    let step = 2.0 / n as f32;
    // The grid's points in the brain, by region.
    let mut points: Vec<Vec<[f32; 3]>> = vec![Vec::new(); Region::ALL.len()];
    for i in 0..n {
        for j in 0..n {
            for k in 0..n {
                let p = [-1.0 + (i as f32 + 0.5) * step, -1.0 + (j as f32 + 0.5) * step, -1.0 + (k as f32 + 0.5) * step];
                if let Some(region) = region_at(p) {
                    points[region as usize].push(p);
                }
            }
        }
    }
    let biggest = cells.iter().map(|c| c.parameters).max().unwrap_or(0).max(1);
    let mut cubes = Vec::new();
    let mut regions = Vec::new();
    for region in Region::ALL {
        let mut pts = std::mem::take(&mut points[region as usize]);
        pts.sort_by_key(|p| order_key(region, *p));
        // The region's cells, block by block, in the engine's order within a block.
        let mut members: Vec<usize> =
            (0..cells.len()).filter(|&i| region_of(&cells[i].module) == region && cells[i].parameters > 0).collect();
        members.sort_by_key(|&i| (cells[i].block.unwrap_or(-1), i));
        let total: i64 = members.iter().map(|&i| cells[i].parameters).sum();
        let base = region.colour();
        let mut shown = std::collections::BTreeSet::new();
        if members.is_empty() || total <= 0 {
            // A region the model does not use: its shape, faint, standing for nothing.
            for p in &pts {
                cubes.push(Cube { centre: *p, size: step * 0.55, colour: [base[0], base[1], base[2], 0.10], region, cell: None });
            }
        } else {
            // Point k goes to the cell whose cumulative share of the region's parameters holds (k + 0.5) / points.
            let mut cumulative = Vec::with_capacity(members.len());
            let mut running = 0i64;
            for &i in &members {
                running += cells[i].parameters;
                cumulative.push(running);
            }
            let count = pts.len().max(1) as f64;
            let mut m = 0usize;
            for (k, p) in pts.iter().enumerate() {
                let at = ((k as f64 + 0.5) / count) * total as f64;
                while m + 1 < cumulative.len() && (cumulative[m] as f64) < at {
                    m += 1;
                }
                let cell = members[m];
                shown.insert(cell);
                let share = (cells[cell].parameters as f64 / biggest as f64).sqrt() as f32;
                let light = 0.35 + 0.65 * share;
                cubes.push(Cube {
                    centre: *p,
                    size: step * 0.82,
                    colour: [base[0] * light, base[1] * light, base[2] * light, 1.0],
                    region,
                    cell: Some(cell),
                });
            }
        }
        regions.push(RegionShare {
            region,
            cells: members.len(),
            parameters: total,
            cubes: pts.len(),
            unshown_cells: members.len().saturating_sub(shown.len()),
        });
    }
    Brain { cubes, regions, resolution: n }
}

// ------------------------------------------------------------------ the picture

/// The resolution the Morphometry tab draws at: coarse enough to read as cubes, fine enough to read as a brain.
pub const PREVIEW_RESOLUTION: usize = 22;

/// The stylised brain drawn from the front left and above, back to front, each cube's three visible faces shaded.
pub struct BrainView<'a> {
    pub brain: &'a Brain,
    pub title: String,
}

/// An orthographic camera looking from a direction at the origin.
struct Camera {
    right: [f32; 3],
    up: [f32; 3],
    forward: [f32; 3],
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn normalise(a: [f32; 3]) -> [f32; 3] {
    let n = dot(a, a).sqrt().max(1e-6);
    [a[0] / n, a[1] / n, a[2] / n]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

impl Camera {
    fn looking_from(eye: [f32; 3]) -> Camera {
        let forward = normalise([-eye[0], -eye[1], -eye[2]]);
        let right = normalise(cross(forward, [0.0, 1.0, 0.0]));
        let up = cross(right, forward);
        Camera { right, up, forward }
    }
}

/// From the front left, a little above: the frontal pole, a hemisphere's side and the cerebellum all show.
const EYE: [f32; 3] = [-0.75, 0.45, 0.85];

impl<Message> iced::widget::canvas::Program<Message> for BrainView<'_> {
    /// The picture, kept between frames: it changes only with a new measurement, its title or its size, and a spinner
    /// elsewhere on the page redraws the window thirty times a second while a measurement runs.
    type State = crate::ui::Kept;

    fn draw(
        &self,
        kept: &crate::ui::Kept,
        renderer: &iced::Renderer,
        _: &iced::Theme,
        bounds: iced::Rectangle,
        _: iced::mouse::Cursor,
    ) -> Vec<iced::widget::canvas::Geometry> {
        use iced::widget::canvas::{Path, Text};
        use iced::{Color, Point};
        use crate::theme::{self, fonts};

        let key = crate::ui::key_of(&(std::ptr::from_ref(self.brain) as usize, self.brain.cubes.len(), &self.title));
        let picture = kept.draw(renderer, bounds.size(), key, |frame| {
            frame.fill_rectangle(Point::ORIGIN, bounds.size(), theme::SURFACE);
            frame.fill_text(Text {
                content: self.title.clone(),
                position: Point::new(8.0, 6.0),
                color: theme::GOLD,
                size: 11.5.into(),
                font: fonts().ui,
                ..Text::default()
            });
            let cam = Camera::looking_from(EYE);
            let scale = (bounds.width.min(bounds.height - 40.0) * 0.36).max(10.0);
            let centre = Point::new(bounds.width / 2.0, bounds.height / 2.0 + 10.0);
            let screen = |p: [f32; 3]| Point::new(centre.x + dot(p, cam.right) * scale, centre.y - dot(p, cam.up) * scale);
            let mut order: Vec<&Cube> = self.brain.cubes.iter().collect();
            // Far first: a larger component along `forward` is farther from the eye.
            order.sort_by(|a, b| dot(b.centre, cam.forward).total_cmp(&dot(a.centre, cam.forward)));
            // The three faces turned toward the eye, with their light: top, left (x-) and front (z+).
            let faces: [([[f32; 3]; 4], f32); 3] = [
                ([[-1.0, 1.0, -1.0], [1.0, 1.0, -1.0], [1.0, 1.0, 1.0], [-1.0, 1.0, 1.0]], 1.22),
                ([[-1.0, -1.0, -1.0], [-1.0, 1.0, -1.0], [-1.0, 1.0, 1.0], [-1.0, -1.0, 1.0]], 0.78),
                ([[-1.0, -1.0, 1.0], [-1.0, 1.0, 1.0], [1.0, 1.0, 1.0], [1.0, -1.0, 1.0]], 1.0),
            ];
            for cube in order {
                let h = cube.size * 0.5;
                let [r, g, b, a] = cube.colour;
                for (corners, light) in &faces {
                    let pts: Vec<Point> = corners
                        .iter()
                        .map(|c| screen([cube.centre[0] + c[0] * h, cube.centre[1] + c[1] * h, cube.centre[2] + c[2] * h]))
                        .collect();
                    let path = Path::new(|p| {
                        p.move_to(pts[0]);
                        for q in &pts[1..] {
                            p.line_to(*q);
                        }
                        p.close();
                    });
                    let lit = |v: f32| (v * light).min(1.0);
                    frame.fill(&path, Color::from_rgba(lit(r), lit(g), lit(b), a));
                }
            }
            frame.fill_text(Text {
                content: "A picture of the model's measured cells, not anatomy: the regions are a display mapping.".into(),
                position: Point::new(8.0, bounds.height - 18.0),
                color: theme::TEXT_FAINT,
                size: 11.0.into(),
                font: fonts().ui,
                ..Text::default()
            });
        });
        vec![picture]
    }
}

// ------------------------------------------------------------------ inside Sinai's head

/// How much taller than the cavity the brain may be, the rest of it below the glass's seam.
const BELOW_THE_SEAM: f32 = 1.5;

/// The brain inside Sinai's glass head: its cubes moved from brain space into the room `cavity` leaves inside the
/// skull, scaled alike on every axis so it keeps its shape. Its width and depth fill the cavity's and its top sits
/// under the crown; below, the stem runs down past the eyes as a brain's does. The faint cubes of a region the model
/// leaves empty are left out: the head's cubes are opaque, and a solid placeholder would read as something measured.
pub fn in_head(brain: &Brain, cavity: crate::face::Cavity) -> Vec<crate::face::Cube> {
    let measured: Vec<&Cube> = brain.cubes.iter().filter(|c| c.cell.is_some()).collect();
    let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
    for c in &measured {
        for i in 0..3 {
            lo[i] = lo[i].min(c.centre[i] - c.size * 0.5);
            hi[i] = hi[i].max(c.centre[i] + c.size * 0.5);
        }
    }
    if measured.is_empty() {
        return Vec::new();
    }
    // Its height may run past the cavity's by half again: what goes below is the cerebellum and the stem, under the
    // glass's seam as a brain's are under the skull's floor.
    let room = [2.0 * cavity.half[0], 2.0 * cavity.half[1] * BELOW_THE_SEAM, 2.0 * cavity.half[2]];
    let scale = (0..3).map(|i| room[i] / (hi[i] - lo[i]).max(1e-6)).fold(f32::MAX, f32::min);
    // Where brain space's points land: x and z centred on the cavity, the top against the cavity's.
    let at = |i: usize, v: f32| match i {
        1 => cavity.centre[1] + cavity.half[1] - (hi[1] - v) * scale,
        _ => cavity.centre[i] + (v - (lo[i] + hi[i]) * 0.5) * scale,
    };
    measured
        .iter()
        .map(|c| crate::face::Cube { centre: std::array::from_fn(|i| at(i, c.centre[i])), size: c.size * scale, colour: c.colour })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell(block: Option<i64>, module: &str, parameters: i64) -> Cell {
        Cell {
            block,
            module: module.to_string(),
            parameters,
            tensors: 1,
            element_types: Vec::new(),
            nominal_bytes: None,
            routed_parameters: 0,
            family: String::new(),
            stage: 0,
            label: String::new(),
            mean_bits_per_weight: None,
            active_parameters: None,
        }
    }

    fn dense(blocks: i64) -> Vec<Cell> {
        let mut cells = vec![cell(None, "token_embd", 4096)];
        for b in 0..blocks {
            for m in ["attn_norm", "attn_q", "attn_k", "attn_v", "attn_output", "ffn_norm", "ffn_gate", "ffn_up", "ffn_down"] {
                cells.push(cell(Some(b), m, if m.ends_with("norm") { 64 } else { 1000 + b }));
            }
        }
        cells.push(cell(None, "output", 4096));
        cells
    }

    #[test]
    fn the_brain_fills_the_head_under_its_crown_and_keeps_its_front_to_the_face() {
        let brain = build(&dense(3), 16);
        let cavity = crate::face::Cavity { centre: [0.0, 0.9, 0.05], half: [0.5, 0.3, 0.6] };
        let inside = in_head(&brain, cavity);
        let measured: Vec<&Cube> = brain.cubes.iter().filter(|c| c.cell.is_some()).collect();
        assert_eq!(inside.len(), measured.len(), "only measured cubes go in");
        let reach = |i: usize, f: fn(f32, f32) -> f32, sign: f32| {
            inside.iter().map(|c| c.centre[i] + sign * c.size * 0.5).fold(if sign > 0.0 { f32::MIN } else { f32::MAX }, f)
        };
        // Across and front to back it stays inside the cavity; its top is the cavity's top.
        for i in [0, 2] {
            assert!(reach(i, f32::max, 1.0) <= cavity.centre[i] + cavity.half[i] + 1e-4, "axis {i}");
            assert!(reach(i, f32::min, -1.0) >= cavity.centre[i] - cavity.half[i] - 1e-4, "axis {i}");
        }
        // It fills the cavity one way: across, front to back, or in height with what is allowed below the seam.
        let room = [1.0, BELOW_THE_SEAM, 1.0];
        let fills: Vec<f32> =
            (0..3).map(|i| (reach(i, f32::max, 1.0) - reach(i, f32::min, -1.0)) / (2.0 * cavity.half[i] * room[i])).collect();
        assert!(fills.iter().any(|f| (f - 1.0).abs() < 1e-4) && fills.iter().all(|f| *f <= 1.0 + 1e-4), "{fills:?}");
        assert!((reach(1, f32::max, 1.0) - (cavity.centre[1] + cavity.half[1])).abs() < 1e-4);
        // One scale on every axis, so it keeps its shape, and order front to back is kept: the face is +z in both.
        let k = inside[0].size / measured[0].size;
        assert!(inside.iter().zip(&measured).all(|(h, b)| (h.size - b.size * k).abs() < 1e-5 && h.colour == b.colour));
        let pairs: Vec<(f32, f32)> = inside.iter().zip(&measured).map(|(h, b)| (h.centre[2], b.centre[2])).collect();
        assert!(pairs.windows(2).all(|w| (w[0].0 - w[1].0) * (w[0].1 - w[1].1) >= -1e-6));
        assert!(in_head(&build(&[], 16), cavity).is_empty(), "a model with no cells puts nothing in the head");
    }

    #[test]
    fn every_region_has_a_place_in_the_shape() {
        let brain = build(&[], 22);
        for share in &brain.regions {
            assert!(share.cubes > 0, "{:?} has no points at 22", share.region);
        }
        assert!(brain.cubes.iter().all(|c| c.cell.is_none()), "an empty model maps nothing");
    }

    #[test]
    fn every_point_is_drawn_once_and_the_layout_is_deterministic() {
        let cells = dense(6);
        let a = build(&cells, 20);
        let b = build(&cells, 20);
        assert_eq!(a, b);
        let total: usize = a.regions.iter().map(|r| r.cubes).sum();
        assert_eq!(total, a.cubes.len());
        let mut centres: Vec<[i64; 3]> =
            a.cubes.iter().map(|c| c.centre.map(|v| (v * 1000.0).round() as i64)).collect();
        centres.sort();
        centres.dedup();
        assert_eq!(centres.len(), a.cubes.len(), "two cubes at one point");
    }

    #[test]
    fn a_region_shares_its_points_in_proportion_to_parameters() {
        // Two attention cells, 3 : 1.
        let cells = vec![cell(Some(0), "attn_q", 3000), cell(Some(1), "attn_q", 1000)];
        let brain = build(&cells, 24);
        let count = |i| brain.cubes.iter().filter(|c| c.cell == Some(i)).count() as f64;
        let ratio = count(0) / count(1);
        assert!((ratio - 3.0).abs() < 0.2, "3:1 laid out as {ratio:.2}");
    }

    #[test]
    fn attention_blocks_run_from_the_back_of_the_head_to_the_front() {
        let cells = dense(8);
        let brain = build(&cells, 24);
        let mean_z = |block: i64| {
            let zs: Vec<f32> = brain
                .cubes
                .iter()
                .filter(|c| c.region == Region::Cortex && c.cell.is_some_and(|i| cells[i].block == Some(block)))
                .map(|c| c.centre[2])
                .collect();
            zs.iter().sum::<f32>() / zs.len() as f32
        };
        assert!(mean_z(0) < mean_z(7), "block 0 at z {} is not behind block 7 at z {}", mean_z(0), mean_z(7));
    }

    #[test]
    fn a_dense_model_leaves_the_cerebellum_empty_and_says_so() {
        let brain = build(&dense(4), 20);
        let cerebellum = brain.regions.iter().find(|r| r.region == Region::Cerebellum).unwrap();
        assert_eq!(cerebellum.cells, 0);
        assert!(
            brain.cubes.iter().filter(|c| c.region == Region::Cerebellum).all(|c| c.cell.is_none() && c.colour[3] < 0.2),
            "an unused region is drawn faint and stands for no cell"
        );
        // A mixture of experts fills it.
        let mut moe = dense(2);
        moe.push(cell(Some(0), "ffn_expert", 50_000));
        let brain = build(&moe, 20);
        assert!(brain.cubes.iter().any(|c| c.region == Region::Cerebellum && c.cell.is_some()));
    }

    #[test]
    fn cells_too_small_for_a_point_are_counted_not_lost() {
        let mut cells = vec![cell(Some(0), "attn_q", 1_000_000)];
        cells.extend((0..50).map(|b| cell(Some(b + 1), "attn_q", 1)));
        let brain = build(&cells, 12);
        let cortex = brain.regions.iter().find(|r| r.region == Region::Cortex).unwrap();
        assert_eq!(cortex.cells, 51);
        assert!(cortex.unshown_cells > 0);
    }

    #[test]
    fn the_regions_of_every_engine_module() {
        assert_eq!(region_of("token_embd"), Region::Brainstem);
        assert_eq!(region_of("ffn_norm"), Region::WhiteMatter);
        assert_eq!(region_of("attn_v"), Region::Cortex);
        assert_eq!(region_of("ffn_down"), Region::Association);
        assert_eq!(region_of("ffn_expert"), Region::Cerebellum);
        assert_eq!(region_of("output"), Region::Frontal);
        assert_eq!(region_of("other"), Region::Frontal);
    }
}
