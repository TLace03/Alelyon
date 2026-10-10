//! Sinai's face: the bust the Angel window draws, drawn here in the Sinai dock and moved by the loop.
//!
//! The shapes are the ones the Angel window draws: the bust (`body.rs`, with the baked MakeHuman mesh it carries),
//! the expressions (`expression.rs`) and the appearance a person gave it (`appearance.rs`), from the `sinai-face`
//! crate (../crates/sinai-face), which both windows link (no copies), so both
//! windows draw one Sinai.
//! What could not be linked is the Angel window's `main.rs`, which is egui throughout: the few numbers and formulas
//! of its frame (where Sinai stands, the blink, the breath, the mouth's spring, the uniform the shader reads) are
//! ported into this module, each saying where it came from.
//!
//! Sinai stands in its world, as in the Angel window: the valley it travels down and the sky over it, with the sun
//! where the sun really is (`world.rs`), drawn behind the bust in the same pass.
//!
//! The loop moves the face: `mouth` (the opening of the mouth, many times a second while Sinai speaks) and
//! `expression` (a tone a sentence, with a lid, a tilt and a warmth). Those never reach the window as messages: the
//! link thread writes them into `LIVE`, and the frame clock reads the latest whenever it draws, so a moving mouth
//! costs no rebuild of the window's widgets.
//!
//! The frame clock is the face's own and lives only while the face is on show: the shader widget asks for its next
//! frame a sixtieth of a second after this one (at most 60 frames a second), and
//! only once the graphics card has drawn a frame. A hidden face is not in the widget tree at all, so nothing asks.
//! Under the software renderer (`ICED_BACKEND=tiny-skia`) iced's shader widget draws nothing: the card never draws,
//! so after one look the clock stops, and a still placeholder stands where the face would be. The face is never a
//! reason the window needs a graphics card.
//!
//! Sinai's head can be glass with something inside it (`Face::set_interior`): the Lattice page puts the measured
//! model there as a stylised voxel brain. Then the cranium, above a cut over the brow, is drawn as glass (its back and
//! then its front, over the cubes, with no lattice on it and the sky's sun and stars shining in it); the face and body
//! are drawn as ever. With nothing inside, the head is drawn exactly as it always was.

pub mod creator;
pub mod render;
pub mod world;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use iced::widget::{column, container, shader, stack, text};
use iced::{Alignment, Element, Length, Rectangle, mouse, window};
use serde_json::Value;

use crate::theme::{self, fonts};

use crate::appearance::{Appearance, Catalog, Colours, Saved};
use crate::body::{self, Rig};
use crate::dock;
use crate::expression;
use crate::state_home;

/// The longest a frame is shown before the next: at most sixty a second.
pub const FRAME: Duration = Duration::from_nanos(16_666_667);

// ------------------------------------------------------------------- what the loop says

/// The expression the loop asked for last: a tone, and the lid, tilt and warmth sent with it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Tone {
    pub tone: String,
    pub lid: f32,
    pub tilt: f32,
    pub warmth: f32,
}

/// What the loop says to the face, as the link thread last heard it. Written by the link thread and read by the frame
/// clock; neither waits on the other for longer than a copy.
pub struct Live {
    /// The mouth's opening, 0 to 1, as `f32` bits.
    mouth: AtomicU32,
    connected: AtomicBool,
    tone: Mutex<Tone>,
}

/// The face's link to the loop, for the one link the window has.
pub static LIVE: Live = Live::new();

impl Live {
    pub const fn new() -> Live {
        Live {
            mouth: AtomicU32::new(0),
            connected: AtomicBool::new(false),
            tone: Mutex::new(Tone { tone: String::new(), lid: 0.0, tilt: 0.0, warmth: 0.0 }),
        }
    }

    fn tone_lock(&self) -> MutexGuard<'_, Tone> {
        self.tone.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Take a message that is the face's (`mouth`, `expression`) and say so; anything else is left to the
    /// conversation. An erased memory also puts the face at rest, and is left to the conversation too.
    ///
    /// Bounded as the Angel window bounds them (`link.rs`, `apply`): a number from a socket is not trusted to be in
    /// range, and an expression with nothing in it is rest, not the last face left on.
    pub fn absorb(&self, v: &Value) -> bool {
        let f = |k: &str| v.get(k).and_then(Value::as_f64).map(|x| x as f32);
        match v.get("type").and_then(Value::as_str).unwrap_or("") {
            "mouth" => {
                self.mouth.store(f("v").unwrap_or(0.0).clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
                true
            }
            "expression" => {
                *self.tone_lock() = Tone {
                    tone: v.get("tone").and_then(Value::as_str).unwrap_or("").to_string(),
                    lid: f("lid").unwrap_or(0.0).clamp(-0.5, 0.5),
                    tilt: f("tilt").unwrap_or(0.0).clamp(-0.2, 0.2),
                    warmth: f("warmth").unwrap_or(0.0).clamp(-1.0, 1.0),
                };
                true
            }
            "memory_erased" => {
                self.rest();
                false
            }
            _ => false,
        }
    }

    pub fn connected(&self, on: bool) {
        self.connected.store(on, Ordering::Relaxed);
    }

    /// The link is gone: nothing may go on claiming the loop does something now, not an open mouth and not an
    /// expression nobody is sending any more (the Angel window's `link_lost`).
    pub fn lost(&self) {
        self.connected(false);
        self.mouth.store(0f32.to_bits(), Ordering::Relaxed);
        self.rest();
    }

    fn rest(&self) {
        *self.tone_lock() = Tone::default();
    }

    /// The mouth's target, whether the loop is there, and the expression asked for.
    pub fn read(&self) -> (f32, bool, Tone) {
        (f32::from_bits(self.mouth.load(Ordering::Relaxed)), self.connected.load(Ordering::Relaxed), self.tone_lock().clone())
    }
}

// ------------------------------------------------------------------- the frame's arithmetic (ported from angel-native/src/main.rs)

/// Where Sinai stands relative to the camera, as the Angel window places it: it travels with the camera, a bust seen
/// whole and above the horizon, at three-quarters.
pub const CAM_Y: f32 = 1.15;
const SINAI_DIST: f32 = 5.5;
const SINAI_LIFT: f32 = 0.74;
const SINAI_YAW: f32 = -0.40;

/// The window's camera: level, at eye height, looking down the valley.
pub fn main_camera(aspect: f32) -> (glam::Mat4, glam::Mat4) {
    let proj = glam::Mat4::perspective_rh(55f32.to_radians(), aspect.max(0.1), 0.4, 900.0);
    let view = glam::Mat4::look_at_rh(glam::vec3(0.0, CAM_Y, 0.0), glam::vec3(0.0, CAM_Y, 1.0), glam::Vec3::Y);
    (proj, view)
}

/// Where Sinai stands: its face points along +z in its own coordinates and the camera looks that way too, so it is
/// turned half round, then by `yaw`; an expression's tilt rolls it.
pub fn sinai_model(yaw: f32, tilt: f32) -> glam::Mat4 {
    glam::Mat4::from_translation(glam::vec3(0.0, CAM_Y + SINAI_LIFT, SINAI_DIST))
        * glam::Mat4::from_rotation_z(tilt)
        * glam::Mat4::from_rotation_y(std::f32::consts::PI + yaw)
}

/// Where a breath is at time `t`, -1 out to 1 in: about thirteen a minute, in over two fifths of the cycle.
pub fn breath_at(t: f32) -> f32 {
    let phase = (t / 4.6).fract();
    if phase < 0.4 { -(phase / 0.4 * std::f32::consts::PI).cos() } else { ((phase - 0.4) / 0.6 * std::f32::consts::PI).cos() }
}

/// The lid's blink at time `t`, 0 open to 1 shut: a tenth of a second about every four.
pub fn blink_at(t: f32) -> f32 {
    let cycle = (t * 0.27).fract();
    if cycle > 0.97 { ((cycle - 0.97) / 0.03 * std::f32::consts::PI).sin() } else { 0.0 }
}

/// The mouth's opening, critically damped towards `target` over `dt` seconds, so it settles rather than chattering
/// at the frame rate: (opening, velocity). The Angel window's constant, the browser face's before it.
pub fn spring(mouth: f32, velocity: f32, target: f32, dt: f32) -> (f32, f32) {
    let omega = 12.5_f32;
    let e = (-omega * dt).exp();
    let change = mouth - target;
    let temp = (velocity + change * omega) * dt;
    let mut next = target + (change + temp) * e;
    let v = (velocity - temp * omega) * e;
    if next.abs() < 0.0005 && target == 0.0 {
        next = 0.0;
    }
    (next, v)
}

/// The share of the way an expression moves in `dt` seconds: about a third of a second to arrive, fast enough to
/// belong to its sentence and slow enough that four short sentences do not read as a twitch.
pub fn ease(dt: f32) -> f32 {
    1.0 - (-3.2 * dt).exp()
}

/// Everything `angel.wgsl` reads to draw Sinai, in its order.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Head {
    pub mvp: [[f32; 4]; 4],
    pub mv: [[f32; 4]; 4],
    /// Mouth open, lid, time, breath.
    pub params: [f32; 4],
    pub tint: [f32; 4],
    pub light: [f32; 4],
    pub eye_l: [f32; 4],
    pub eye_r: [f32; 4],
    /// Jaw hinge height and depth, the creator's highlight, spare.
    pub rig: [f32; 4],
    pub fill: [f32; 4],
    pub shadow: [f32; 4],
    pub glow: [f32; 4],
    pub iris: [f32; 4],
}

/// How Sinai moves this frame, beyond its shape.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Motion {
    pub mouth: f32,
    /// The lid's angle: the blink and the expression's lid together.
    pub lid: f32,
    pub t: f32,
    pub breath: f32,
    pub warmth: f32,
    /// The creator is lighting up what a control moves.
    pub highlighting: bool,
}

/// The uniform for one frame (the Angel window's `head_uniform`, unchanged in what it computes).
pub fn head_uniform(proj: glam::Mat4, view: glam::Mat4, model: glam::Mat4, m: &Motion, rig: &Rig, colours: &Colours) -> Head {
    let lit = |c: [f32; 3], w: f32| [c[0], c[1], c[2], w];
    Head {
        mvp: (proj * view * model).to_cols_array_2d(),
        mv: (view * model).to_cols_array_2d(),
        // Negative opens the eyes wider than rest; the link bounds an expression's lid to 0.5, well short of shut,
        // so no expression holds the eyes closed through a blink.
        params: [m.mouth, m.lid.clamp(-0.35, 1.0), m.t, m.breath],
        tint: lit(colours.lattice, 0.52),
        // Key from the front-upper-left in its coordinates, fill from below; warmth moves only the fill's weight.
        light: [-0.74, 0.44, 0.51, 0.34 + 0.10 * m.warmth],
        eye_l: rig.eyes[0],
        eye_r: rig.eyes[1],
        rig: [rig.hinge[0], rig.hinge[1], if m.highlighting { 1.0 } else { 0.0 }, 0.0],
        fill: lit(colours.fill, 1.0),
        shadow: lit(colours.shadow, 1.0),
        glow: lit(colours.glow, 0.85),
        iris: colours.iris,
    }
}

// ------------------------------------------------------------------- the face as it moves

/// The face between frames: the mouth's spring, the expression easing in, the shape posed for both, and the clock.
pub struct Animator {
    started: Instant,
    last: Option<Instant>,
    t: f32,
    connected: bool,
    mouth: f32,
    mouth_v: f32,
    lid: f32,
    tilt: f32,
    warmth: f32,
    /// The expression's units, made on the first frame drawn (the bust is read then, not at start).
    face: Option<expression::Face>,
    /// Sinai as the person shaped it.
    pub appearance: Appearance,
    /// The weights last posed, so a frame that changes nothing poses nothing.
    posed_for: Option<(Vec<(usize, f32)>, Option<String>)>,
    verts: Option<Arc<Vec<f32>>>,
    /// Counts the shapes posed, so the card uploads a shape once.
    version: u64,
    rig: Option<Rig>,
    /// The creator's view of Sinai, when this face is the creator's: its own camera, tone and jaw.
    pub preview: Option<Preview>,
    /// What is inside the glass head; with nothing, the head is opaque, as it always was.
    pub interior: Option<Interior>,
}

/// What the Appearance creator shows instead of what the loop says: the tone being previewed, a speaking jaw, the
/// control under the pointer lit up on the face, and its own camera (the Angel window's creator's), which eases to the
/// part on show and is turned by dragging over the face.
#[derive(Clone, Debug, PartialEq)]
pub struct Preview {
    pub tone: String,
    pub speaking: bool,
    pub hovered: Option<String>,
    pub yaw: f32,
    pub pitch: f32,
    /// Where the camera looks and how far it is, easing towards `goal`.
    pub target: [f32; 3],
    pub dist: f32,
    pub goal: ([f32; 3], f32),
}

impl Preview {
    pub fn new(target: [f32; 3], dist: f32) -> Preview {
        Preview { tone: String::new(), speaking: false, hovered: None, yaw: 0.0, pitch: 0.05, target, dist, goal: (target, dist) }
    }

    /// The projection and view for this camera, for a view `aspect` wide over high.
    pub fn matrices(&self, aspect: f32) -> (glam::Mat4, glam::Mat4) {
        let target = glam::Vec3::from(self.target);
        let dir = glam::vec3(self.yaw.sin() * self.pitch.cos(), self.pitch.sin(), self.yaw.cos() * self.pitch.cos());
        let proj = glam::Mat4::perspective_rh(30f32.to_radians(), aspect.max(0.1), 0.1, 100.0);
        (proj, glam::Mat4::look_at_rh(target + dir * self.dist, target, glam::Vec3::Y))
    }

    /// A drag of `dx`, `dy` pixels over the face turns it.
    pub fn turn(&mut self, dx: f32, dy: f32) {
        self.yaw -= dx * 0.01;
        self.pitch = (self.pitch + dy * 0.006).clamp(-0.9, 0.9);
    }

    /// A scroll of `lines` brings the camera closer or takes it away, within reach of the face.
    pub fn zoom(&mut self, lines: f32) {
        self.goal.1 = (self.goal.1 * (1.0 - lines * 0.08)).clamp(1.6, 14.0);
    }

    /// The jaw of a speaking mouth at `t`: a vowel's worth, at a syllable's pace (the Angel window's preview).
    pub fn speaking_mouth(t: f32) -> f32 {
        0.45 * (0.5 + 0.5 * (t * 9.0).sin()) * (0.6 + 0.4 * (t * 2.3).sin())
    }
}

impl Animator {
    pub fn new(appearance: Appearance, now: Instant) -> Animator {
        Animator {
            started: now,
            last: None,
            t: 0.0,
            connected: false,
            mouth: 0.0,
            mouth_v: 0.0,
            lid: 0.0,
            tilt: 0.0,
            warmth: 0.0,
            face: None,
            appearance,
            posed_for: None,
            verts: None,
            version: 0,
            rig: None,
            preview: None,
            interior: None,
        }
    }

    /// Move to `now`: the mouth towards the loop's, the expression towards the loop's tone, and the shape posed again
    /// only when its weights changed.
    pub fn step(&mut self, now: Instant, live: &Live) {
        let dt = self.last.map_or(FRAME.as_secs_f32(), |l| now.saturating_duration_since(l).as_secs_f32()).clamp(0.001, 0.05);
        self.last = Some(now);
        self.t = now.saturating_duration_since(self.started).as_secs_f32();
        let (target, connected, mut tone) = live.read();
        self.connected = connected;
        // In the creator the face wears the tone being previewed, with the lid, tilt and warmth the loop would send.
        if let Some(p) = self.preview.as_mut() {
            let (lid, tilt, warmth) = expression::tone_pose(&p.tone);
            tone = Tone { tone: p.tone.clone(), lid, tilt, warmth };
            let k = 1.0 - (-8.0 * dt).exp();
            for i in 0..3 {
                p.target[i] += (p.goal.0[i] - p.target[i]) * k;
            }
            p.dist += (p.goal.1 - p.dist) * k;
        }
        (self.mouth, self.mouth_v) = spring(self.mouth, self.mouth_v, target, dt);
        let k = ease(dt);
        self.lid += (tone.lid - self.lid) * k;
        self.tilt += (tone.tilt - self.tilt) * k;
        self.warmth += (tone.warmth - self.warmth) * k;
        let bust = body::shared();
        let face = self.face.get_or_insert_with(|| expression::Face::new(bust));
        face.ease_toward(bust, &tone.tone, k);
        let catalog = Catalog::builtin();
        let mut weights = self.appearance.weights(catalog, |n| bust.target(n));
        weights.extend(face.weights());
        let hovered = self.preview.as_ref().and_then(|p| p.hovered.clone());
        let key = (weights, hovered);
        if self.posed_for.as_ref() != Some(&key) {
            let posed = bust.pose(&key.0);
            // While a control is under the pointer, the creator lights up what it moves.
            let highlight = key.1.as_deref().and_then(|id| catalog.control(id)).map(|c| {
                let targets: Vec<usize> = c.all_targets().iter().filter_map(|n| bust.target(n)).collect();
                bust.influence(&targets)
            });
            self.verts = Some(Arc::new(bust.vertex_data(&posed, highlight.as_deref())));
            self.rig = Some(posed.rig);
            self.version = POSES.fetch_add(1, Ordering::Relaxed) + 1;
            self.posed_for = Some(key);
        }
    }

    /// What one frame draws, for a view `aspect` wide over high; nothing until the first step has posed a shape.
    pub fn frame(&self, aspect: f32) -> Option<Frame> {
        let (verts, rig) = (self.verts.clone()?, self.rig?);
        let colours = self.appearance.palette.resolve();
        if let Some(p) = &self.preview {
            // The creator looks at Sinai in Sinai's own coordinates, from its own camera, against nothing.
            let (proj, view) = p.matrices(aspect);
            let mouth = if p.speaking { Preview::speaking_mouth(self.t) } else { 0.0 };
            let motion = Motion {
                mouth,
                lid: blink_at(self.t) + self.lid,
                t: self.t,
                breath: breath_at(self.t),
                warmth: self.warmth,
                highlighting: p.hovered.is_some(),
            };
            let head = head_uniform(proj, view, glam::Mat4::from_rotation_z(self.tilt), &motion, &rig, &colours);
            // The creator shapes how Sinai looks, so it shows the head as it is, never glass.
            return Some(Frame { head, verts, version: self.version, world: None, inside: None });
        }
        let (proj, view) = main_camera(aspect);
        // The sky from the real sun, recomputed each frame: four transcendentals, and a value cached for a minute jumps
        // when the minute turns.
        let (elev, ha) = world::sun_now();
        let scenery = world::scenery_uniforms(self.t, proj, view, elev, ha, 0.0);
        // With no loop there is no voice to move to: Sinai breathes, its mouth a little with the breath.
        let mouth = if self.connected { self.mouth } else { 0.06 * (0.5 + 0.5 * (self.t * 0.9).sin()) };
        let motion = Motion {
            mouth,
            lid: blink_at(self.t) + self.lid,
            t: self.t,
            breath: breath_at(self.t),
            warmth: self.warmth,
            highlighting: false,
        };
        let head = head_uniform(proj, view, sinai_model(SINAI_YAW, self.tilt), &motion, &rig, &colours);
        let inside = self.interior.clone().map(|i| (i, glass_uniform(&head, proj, &scenery.1, &rig)));
        Some(Frame { head, verts, version: self.version, world: Some(scenery), inside })
    }
}

/// One frame for the card: the uniform, the shape with its version (uploaded only when it changed), and the valley
/// and sky behind Sinai when it stands in its world.
#[derive(Clone, Debug)]
pub struct Frame {
    pub head: Head,
    pub verts: Arc<Vec<f32>>,
    pub version: u64,
    pub world: Option<(world::Camera, world::SkyU)>,
    /// What the glass head holds and the glass's uniform; `None` draws the head opaque.
    pub inside: Option<(Interior, GlassU)>,
}

// ------------------------------------------------------------------- inside the glass head

/// One cube inside the head, in Sinai's own coordinates (head units: up `y`, the face towards `z`), as the card reads
/// it per instance.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Cube {
    pub centre: [f32; 3],
    /// The edge's length.
    pub size: f32,
    /// Its colour; the cubes are opaque, so the alpha is not read.
    pub colour: [f32; 4],
}

/// What the glass head holds, with a number the card uploads it under once.
#[derive(Clone, Debug)]
pub struct Interior {
    pub cubes: Arc<Vec<Cube>>,
    pub version: u64,
}

impl Interior {
    pub fn new(cubes: Vec<Cube>) -> Interior {
        Interior { cubes: Arc::new(cubes), version: POSES.fetch_add(1, Ordering::Relaxed) + 1 }
    }
}

/// What the glass reads besides the head's own uniform (`GlassU` in the glass shader, in its order).
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct GlassU {
    /// From the target's pixels back to Sinai's coordinates: the inverse of the head's `mvp`.
    pub inv_mvp: [[f32; 4]; 4],
    /// The target's width and height in pixels, set where it is drawn.
    pub viewport: [f32; 4],
    /// The cranium's cut: its height at the brow, how far it falls per unit towards the back, the brow's depth.
    pub cut: [f32; 4],
    /// The sun or the moon as the sky draws it: its direction in view space, and how strongly it shines in the glass.
    pub light: [f32; 4],
    /// Its colour, and how much of the night there is (the stars' glints).
    pub colour: [f32; 4],
}

/// How far above the eyes' centres the glass begins at the brow, in eye radii, and how far the cut falls for each unit
/// towards the back of the head: the forehead's top, the crown and the back of the skull are glass, the face is not.
const CUT_ABOVE_EYES: f32 = 3.4;
const CUT_FALL: f32 = 0.55;

/// The glass's uniform for a frame in Sinai's world: where the cranium is, and the light the sky shows where the sky
/// shows it (`sky.wgsl`: one disc at the sun's place, warm by day and cool low down, drawn only above the horizon).
pub fn glass_uniform(head: &Head, proj: glam::Mat4, sky: &world::SkyU, rig: &Rig) -> GlassU {
    let [_, eye_y, eye_z, radius] = rig.eyes[0];
    let far = proj.inverse() * glam::vec4(sky.sun[0], sky.sun[1], 1.0, 1.0);
    let dir = (far.truncate() / far.w).normalize_or_zero();
    let elev = sky.hor[3];
    let warm = ((elev + 0.25) / 0.5).clamp(0.0, 1.0);
    let moon = glam::vec3(0.78, 0.80, 0.92);
    let colour = moon.lerp(glam::vec3(1.0, 0.86, 0.60), warm);
    let shown = if sky.sun[1] >= sky.top[3] - 0.06 { 1.0 } else { 0.0 };
    let strength = shown * (0.55 + 0.45 * warm) * (1.0 - 0.6 * sky.sun[3]);
    GlassU {
        inv_mvp: glam::Mat4::from_cols_array_2d(&head.mvp).inverse().to_cols_array_2d(),
        viewport: [1.0, 1.0, 0.0, 0.0],
        cut: [eye_y + CUT_ABOVE_EYES * radius, CUT_FALL, eye_z, 0.0],
        light: [dir.x, dir.y, dir.z, strength],
        colour: [colour.x, colour.y, colour.z, sky.mid[3]],
    }
}

/// The room inside Sinai's skull: a box, centre and half-extents in head units.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Cavity {
    pub centre: [f32; 3],
    pub half: [f32; 3],
}

/// The skin's part number (`angel.wgsl`'s `SKIN`; `body.rs` keeps its own name for it to its tests).
const SKIN: f32 = 0.0;

/// How far inside the skin the cavity stops, as a share of the skin's own half-extent: what is put there clears the
/// glass, which the breath and a tilt move by a hair.
const CAVITY_INSET: f32 = 0.70;

/// The room inside Sinai's glass cranium, measured once from the bust at rest: the box the skin above the glass's cut
/// spans (`glass_uniform`'s), pulled in from the skin.
pub fn cavity() -> Cavity {
    static CAVITY: std::sync::OnceLock<Cavity> = std::sync::OnceLock::new();
    *CAVITY.get_or_init(|| {
        let bust = body::shared();
        let rest = bust.pose(&[]);
        let [_, eye_y, eye_z, radius] = rest.rig.eyes[0];
        let above_cut = |p: &[f32; 3]| p[1] > eye_y + CUT_ABOVE_EYES * radius + (p[2] - eye_z) * CUT_FALL;
        cavity_of(&rest.positions, |v| bust.attrs(v)[3] == SKIN, above_cut)
    })
}

/// The box the skin vertices `inside` span, pulled in by `CAVITY_INSET` about its centre.
fn cavity_of(positions: &[[f32; 3]], skin: impl Fn(usize) -> bool, inside: impl Fn(&[f32; 3]) -> bool) -> Cavity {
    let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
    for (_, p) in positions.iter().enumerate().filter(|&(v, p)| inside(p) && skin(v)) {
        for i in 0..3 {
            lo[i] = lo[i].min(p[i]);
            hi[i] = hi[i].max(p[i]);
        }
    }
    let centre = std::array::from_fn(|i| (lo[i] + hi[i]) * 0.5);
    let half = std::array::from_fn(|i| (hi[i] - lo[i]) * 0.5 * CAVITY_INSET);
    Cavity { centre, half }
}

/// Every shape posed in this process has its own number, so a card never mistakes one face's shape for another's.
static POSES: AtomicU64 = AtomicU64::new(0);

// ------------------------------------------------------------------- when to draw

/// Whether the face is on show: the Sinai page is open, not editing grants, and a dock shows Sinai's tab.
pub fn on_show(sinai_page: bool, grants_open: bool, showing: impl IntoIterator<Item = dock::View>) -> bool {
    sinai_page && !grants_open && showing.into_iter().any(|v| v == dock::View::Angel)
}

/// What the frame clock does on a redraw.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pace {
    /// Step the face and draw again one frame later.
    Draw,
    /// Nothing has reached the card yet: look once more, in case this redraw was the first the card saw.
    Look,
    /// The card never drew (the software renderer, or the face failed): no clock at all.
    Rest,
}

/// Decide the clock: frames run only once the card has drawn one, and a renderer that never draws gets one more look.
pub fn pace(card_drew: bool, looked: bool) -> Pace {
    match (card_drew, looked) {
        (true, _) => Pace::Draw,
        (false, false) => Pace::Look,
        (false, true) => Pace::Rest,
    }
}

/// When the next frame is due after one drawn at `now`: never sooner than a sixtieth of a second.
pub fn next_frame(now: Instant) -> Instant {
    now + FRAME
}

// ------------------------------------------------------------------- the face in the window

/// What the card has done with the face, shared with the renderer.
#[derive(Debug, Default)]
pub struct Card {
    /// Frames the card has drawn.
    frames: AtomicU64,
    /// Why the face could not be drawn on this card, if it could not.
    failed: Mutex<Option<String>>,
}

impl Card {
    pub fn drew(&self) {
        self.frames.fetch_add(1, Ordering::Relaxed);
    }

    pub fn frames(&self) -> u64 {
        self.frames.load(Ordering::Relaxed)
    }

    pub fn fail(&self, why: String) {
        *self.failed.lock().unwrap_or_else(|p| p.into_inner()) = Some(why);
    }

    pub fn failed(&self) -> Option<String> {
        self.failed.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }
}

/// The face as the window holds it: the animation, and what the card made of it.
#[derive(Clone)]
pub struct Face {
    pub animator: Arc<Mutex<Animator>>,
    pub card: Arc<Card>,
}

impl Face {
    pub fn new(appearance: Appearance) -> Face {
        Face { animator: Arc::new(Mutex::new(Animator::new(appearance, Instant::now()))), card: Arc::new(Card::default()) }
    }

    pub fn lock(&self) -> MutexGuard<'_, Animator> {
        self.animator.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Put something inside Sinai's head, which then draws as glass; `None` makes it opaque again.
    pub fn set_interior(&self, interior: Option<Interior>) {
        self.lock().interior = interior;
    }
}

/// The shader widget's side of the face: steps the animation on each redraw and hands the card a frame.
pub struct Program {
    face: Face,
}

/// The clock's memory while the widget is on show, and a drag under way over the creator's face.
#[derive(Default)]
pub struct Clock {
    looked: bool,
    dragging: Option<iced::Point>,
}

impl<Message> shader::Program<Message> for Program {
    type State = Clock;
    type Primitive = render::Primitive;

    fn update(&self, clock: &mut Clock, event: &iced::Event, bounds: Rectangle, cursor: mouse::Cursor) -> Option<shader::Action<Message>> {
        let now = match event {
            iced::Event::Window(window::Event::RedrawRequested(now)) => now,
            // The creator's face turns under a drag and comes closer under a scroll; the dock's does neither.
            iced::Event::Mouse(m) => return self.handle(clock, m, bounds, cursor),
            _ => return None,
        };
        match pace(self.face.card.frames() > 0, clock.looked) {
            Pace::Draw => {
                self.face.lock().step(*now, &LIVE);
                Some(shader::Action::request_redraw_at(next_frame(*now)))
            }
            Pace::Look => {
                clock.looked = true;
                Some(shader::Action::request_redraw())
            }
            Pace::Rest => None,
        }
    }

    fn mouse_interaction(&self, clock: &Clock, bounds: Rectangle, cursor: mouse::Cursor) -> mouse::Interaction {
        match (self.face.lock().preview.is_some(), clock.dragging.is_some(), cursor.is_over(bounds)) {
            (true, true, _) => mouse::Interaction::Grabbing,
            (true, false, true) => mouse::Interaction::Grab,
            _ => mouse::Interaction::default(),
        }
    }

    fn draw(&self, _clock: &Clock, _cursor: mouse::Cursor, bounds: Rectangle) -> render::Primitive {
        let frame = self.face.lock().frame(bounds.width / bounds.height.max(1.0));
        render::Primitive::new(frame, self.face.card.clone())
    }
}

impl Program {
    /// A mouse event over the creator's face: drag to turn, scroll to zoom.
    fn handle<Message>(
        &self,
        clock: &mut Clock,
        event: &mouse::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<shader::Action<Message>> {
        let mut a = self.face.lock();
        let p = a.preview.as_mut()?;
        match event {
            mouse::Event::ButtonPressed(mouse::Button::Left) => {
                clock.dragging = Some(cursor.position_over(bounds)?);
                Some(shader::Action::capture())
            }
            mouse::Event::CursorMoved { position } => {
                let (dx, dy) = drag_moved(&mut clock.dragging, *position, crate::pointer::left_held())?;
                p.turn(dx, dy);
                Some(shader::Action::request_redraw())
            }
            mouse::Event::ButtonReleased(mouse::Button::Left) => clock.dragging.take().map(|_| shader::Action::capture()),
            mouse::Event::CursorLeft => {
                clock.dragging = None;
                None
            }
            mouse::Event::WheelScrolled { delta } if cursor.is_over(bounds) => {
                let lines = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => *y,
                    mouse::ScrollDelta::Pixels { y, .. } => *y / 40.0,
                };
                p.zoom(lines);
                Some(shader::Action::capture())
            }
            _ => None,
        }
    }
}

/// How far a pointer move to `position` turns the creator's face: only while a drag that began on the face goes on.
/// A move with no drag under way turns nothing and starts none, wherever the pointer is and whatever button is held,
/// so a slider dragged or a tone pressed beside the face leaves it where it was (it once turned, 2026-10-07).
/// A drag whose release went elsewhere ends here.
pub fn drag_moved(dragging: &mut Option<iced::Point>, position: iced::Point, held: Option<bool>) -> Option<(f32, f32)> {
    let from = (*dragging)?;
    if !crate::pointer::drag_goes_on(held) {
        *dragging = None;
        return None;
    }
    *dragging = Some(position);
    Some((position.x - from.x, position.y - from.y))
}

/// The face, or, where the card does not draw it, a still placeholder that says why. The placeholder lies under the
/// face: the face covers it whenever the card draws, and it shows through when nothing does.
pub fn view<'a, Message: 'a>(face: &Face) -> Element<'a, Message> {
    let why = match face.card.failed() {
        Some(why) => format!("Sinai's face could not be drawn on this graphics card: {why}"),
        None => "Sinai's face is drawn by the graphics card. With the software renderer it is not drawn; everything else works as before."
            .to_string(),
    };
    let still = container(
        column![
            text("\u{2726}").size(30).color(theme::GOLD).font(fonts().ui),
            text(why).size(12.5).color(theme::TEXT_FAINT).font(fonts().ui).align_x(Alignment::Center),
        ]
        .spacing(8)
        .align_x(Alignment::Center)
        .max_width(320),
    )
    .center(Length::Fill)
    .style(theme::well);
    stack![still, shader(Program { face: face.clone() }).width(Length::Fill).height(Length::Fill)]
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

// ------------------------------------------------------------------- the saved appearance

/// Where the Angel window keeps the appearance, found without moving anything.
///
/// The Angel window's own lookup (`state_home::run`) first MOVES its settings out of `%APPDATA%` on a start no agent
/// made; Alelyon must never be that start. So this answers by the same rules from what is on disk: `ANGEL_APPEARANCE`
/// when it names a file; else, until the Angel window has moved its files in (no `MIGRATED-window.json` in
/// `~/.alelyon/angel`), the old file that move will copy in (an agent's start keeps its own folder, as Angel's does);
/// else `~/.alelyon/angel/sinai-appearance.json`.
pub fn appearance_at(env: state_home::Env, home: Option<&Path>, temp: &Path) -> Option<PathBuf> {
    let file = state_home::File::Appearance;
    if let Some(named) = env(file.variable()).filter(|v| !v.trim().is_empty()) {
        return Some(PathBuf::from(named));
    }
    let moved_in = state_home::home_dir(home?).join(state_home::MARKER).exists();
    if !moved_in && !state_home::agent_session(env) {
        return state_home::legacy_dir(env).map(|dir| dir.join(file.name()));
    }
    state_home::state_dir(env, home, temp).map(|dir| dir.join(file.name()))
}

/// This process's appearance file.
pub fn appearance_file() -> Option<PathBuf> {
    let env = |name: &str| std::env::var(name).ok();
    appearance_at(&env, std::env::home_dir().as_deref(), &std::env::temp_dir())
}

/// The saved appearance at `path`, or Sinai as it ships, with what was set aside in words (the Angel window's reading
/// of the same file, `Saved::from_json`; a file that is not an appearance is left untouched until a save).
pub fn load(path: Option<&Path>) -> Saved {
    let catalog = Catalog::builtin();
    let Some(path) = path else {
        return Saved { notice: "No settings folder is available; Sinai is shown as it ships".into(), ..Saved::default() };
    };
    match std::fs::read_to_string(path) {
        Ok(text) => match Saved::from_json(&text, catalog) {
            Ok(saved) => saved,
            Err(why) => Saved {
                notice: format!("The saved appearance could not be read ({why}); it is untouched until you save"),
                ..Saved::default()
            },
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Saved::default(),
        Err(e) => Saved { notice: format!("The saved appearance could not be read: {e}"), ..Saved::default() },
    }
}

/// Keep `saved` at `path` as the Angel window's creator keeps it: written whole beside itself, then renamed into place.
pub fn save(saved: &Saved, path: Option<&Path>) -> Result<(), String> {
    saved.save_to(path.ok_or("No settings folder is available, so the appearance is not saved")?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_loops_face_messages_are_taken_bounded_and_rest_when_the_link_goes() {
        let live = Live::new();
        assert!(live.absorb(&json!({"type": "mouth", "v": 0.4})));
        assert!(live.absorb(&json!({"type": "expression", "tone": "thinking", "lid": 0.1, "tilt": 0.1, "warmth": -0.2})));
        let (mouth, _, tone) = live.read();
        assert_eq!(mouth, 0.4);
        assert_eq!(tone, Tone { tone: "thinking".into(), lid: 0.1, tilt: 0.1, warmth: -0.2 });
        // anything over the wire is bounded, as the Angel window bounds it
        live.absorb(&json!({"type": "mouth", "v": 7.0}));
        live.absorb(&json!({"type": "expression", "tone": "x", "lid": -99.0, "tilt": 40.0, "warmth": 1e9}));
        let (mouth, _, tone) = live.read();
        assert_eq!((mouth, tone.lid, tone.tilt, tone.warmth), (1.0, -0.5, 0.2, 1.0));
        // an expression with nothing in it is rest
        live.absorb(&json!({"type": "expression"}));
        assert_eq!(live.read().2, Tone::default());
        // an erased memory rests the face and is still the conversation's
        live.absorb(&json!({"type": "expression", "tone": "sad", "lid": 0.2}));
        assert!(!live.absorb(&json!({"type": "memory_erased", "epoch": 3})));
        assert_eq!(live.read().2, Tone::default());
        assert!(!live.absorb(&json!({"type": "state", "s": "idle"})), "the conversation's messages are left to it");
        // a lost link leaves no open mouth and no expression
        live.connected(true);
        live.absorb(&json!({"type": "mouth", "v": 0.8}));
        live.absorb(&json!({"type": "expression", "tone": "surprise", "lid": -0.3}));
        live.lost();
        assert_eq!(live.read(), (0.0, false, Tone::default()));
    }

    #[test]
    fn the_mouth_settles_on_its_target_without_ringing_and_closes_to_zero() {
        let (mut m, mut v) = (0.0f32, 0.0f32);
        let mut top = 0.0f32;
        for _ in 0..120 {
            (m, v) = spring(m, v, 0.6, 1.0 / 60.0);
            top = top.max(m);
        }
        assert!((m - 0.6).abs() < 1e-3, "{m}");
        assert!(top <= 0.6 + 1e-4, "critically damped: it never overshoots ({top})");
        for _ in 0..120 {
            (m, v) = spring(m, v, 0.0, 1.0 / 60.0);
        }
        assert_eq!(m, 0.0, "a closed mouth is exactly closed");
        // one step does not jump: the jaw does not chatter at the frame rate
        let (once, _) = spring(0.0, 0.0, 1.0, 1.0 / 60.0);
        assert!(once < 0.05, "{once}");
    }

    #[test]
    fn breath_and_blink_keep_their_rhythm() {
        assert_eq!(breath_at(0.0), -1.0, "out at the start of a breath");
        assert!((breath_at(4.6 * 0.4) - 1.0).abs() < 1e-5, "in two fifths of the way through");
        assert_eq!(blink_at(0.0), 0.0);
        let shut = (0..4000).map(|i| blink_at(i as f32 / 1000.0)).fold(0.0f32, f32::max);
        assert!(shut > 0.99, "a blink closes the lid within four seconds ({shut})");
        let open = (0..4000).filter(|i| blink_at(*i as f32 / 1000.0) == 0.0).count();
        assert!(open > 3800, "and the eyes are open nearly all the time ({open} of 4000)");
        assert!(ease(1.0 / 60.0) > 0.0 && ease(1.0 / 60.0) < 0.1);
    }

    #[test]
    fn the_uniform_carries_the_motion_rig_and_colours_the_shader_reads() {
        let rig = Rig { eyes: [[0.3, 0.1, 0.8, 0.12], [-0.3, 0.1, 0.8, 0.12]], hinge: [-0.4, 0.2] };
        let colours = crate::appearance::Palette::default().resolve();
        let (proj, view) = main_camera(1.5);
        let m = Motion { mouth: 0.5, lid: 2.0, t: 3.0, breath: -0.2, warmth: 1.0, highlighting: true };
        let h = head_uniform(proj, view, sinai_model(SINAI_YAW, 0.0), &m, &rig, &colours);
        assert_eq!(h.params, [0.5, 1.0, 3.0, -0.2], "the lid is bounded at shut");
        assert_eq!(h.eye_l, rig.eyes[0]);
        assert_eq!(h.rig, [-0.4, 0.2, 1.0, 0.0]);
        assert!((h.light[3] - 0.44).abs() < 1e-6, "warmth raises the fill");
        assert_eq!(&h.tint[..3], &crate::appearance::brand::LATTICE);
        let wide = head_uniform(proj, view, sinai_model(SINAI_YAW, 0.0), &Motion { lid: -1.0, ..m }, &rig, &colours);
        assert_eq!(wide.params[1], -0.35, "and wide open no further than the Angel window allows");
        // Sinai is in front of the camera and inside its view
        let centre = glam::Mat4::from_cols_array_2d(&h.mvp) * glam::vec4(0.0, 0.0, 0.0, 1.0);
        let ndc = centre.truncate() / centre.w;
        assert!(centre.w > 0.0 && ndc.x.abs() < 1.0 && ndc.y.abs() < 1.0, "{ndc:?}");
        assert_eq!(std::mem::size_of::<Head>(), 288);
    }

    #[test]
    fn the_animator_poses_once_and_again_only_when_the_shape_changes() {
        let live = Live::new();
        let start = Instant::now();
        let mut a = Animator::new(Appearance::default(), start);
        assert!(a.frame(1.0).is_none(), "nothing is drawn before the first step");
        a.step(start, &live);
        let first = a.frame(1.0).expect("posed");
        assert_eq!(first.verts.len(), body::shared().n_draw * body::FLOATS_PER_VERTEX);
        a.step(start + FRAME, &live);
        assert_eq!(a.frame(1.0).unwrap().version, first.version, "nothing changed, nothing posed");
        // the loop's tone eases in, and the shape changes with it
        live.absorb(&serde_json::json!({"type": "expression", "tone": "smile", "lid": 0.1, "tilt": -0.03, "warmth": 0.8}));
        a.step(start + FRAME * 2, &live);
        assert!(a.frame(1.0).unwrap().version > first.version);
        // and the mouth follows the loop's only while it is there
        live.absorb(&serde_json::json!({"type": "mouth", "v": 0.9}));
        for i in 3..60 {
            a.step(start + FRAME * i, &live);
        }
        assert!(a.frame(1.0).unwrap().head.params[0] < 0.07, "no loop: the mouth only breathes");
        live.connected(true);
        for i in 60..120 {
            a.step(start + FRAME * i, &live);
        }
        assert!(a.frame(1.0).unwrap().head.params[0] > 0.8, "the loop's mouth, smoothed");
    }

    #[test]
    fn the_creators_face_has_its_own_camera_tone_and_jaw() {
        let live = Live::new();
        live.connected(true);
        live.absorb(&serde_json::json!({"type": "expression", "tone": "sad", "lid": 0.2}));
        let start = Instant::now();
        let mut a = Animator::new(Appearance::default(), start);
        let mut p = Preview::new([0.0, 0.0, 0.2], 6.4);
        p.tone = "smile".into();
        p.speaking = true;
        p.hovered = Some("chin.width".into());
        p.goal = ([0.0, -0.9, 0.9], 3.0);
        a.preview = Some(p);
        for i in 0..90 {
            a.step(start + FRAME * i, &live);
        }
        let frame = a.frame(1.0).unwrap();
        assert!(frame.world.is_none(), "the creator shows Sinai against nothing");
        assert_eq!(frame.head.rig[2], 1.0, "the control under the pointer is lit");
        let lit = frame.verts.chunks_exact(body::FLOATS_PER_VERTEX).filter(|v| v[13] > 0.0).count();
        assert!(lit > 0, "and the vertices it moves carry the light");
        let p = a.preview.as_ref().unwrap();
        assert!((p.dist - 3.0).abs() < 0.01 && (p.target[1] + 0.9).abs() < 0.01, "the camera eased to the mouth");
        assert!((a.lid - 0.10).abs() < 0.01, "the previewed smile's lid, not the loop's sad one ({})", a.lid);
        // drag and scroll
        let mut p = Preview::new([0.0; 3], 5.0);
        p.turn(100.0, 1000.0);
        assert_eq!((p.yaw, p.pitch), (-1.0, 0.9));
        p.zoom(100.0);
        assert_eq!(p.goal.1, 1.6, "no closer than the face");
        assert!(Preview::speaking_mouth(0.3) >= 0.0 && Preview::speaking_mouth(0.3) <= 0.45);
    }

    #[test]
    fn only_a_drag_begun_on_the_face_turns_it() {
        use iced::Point;
        let at = |x: f32, y: f32| Point::new(x, y);
        // The pointer moves beside the face with the button held (a slider dragged, a tone pressed): nothing turns,
        // and no drag begins, however long it goes on.
        let mut dragging = None;
        for i in 0..5 {
            assert_eq!(drag_moved(&mut dragging, at(400.0 + 10.0 * i as f32, 50.0), Some(true)), None);
            assert_eq!(dragging, None);
        }
        assert_eq!(drag_moved(&mut dragging, at(500.0, 60.0), None), None, "nor where the system cannot be asked");
        // A press on the face begins one: each move turns by how far the pointer went since the last.
        let mut dragging = Some(at(100.0, 100.0));
        assert_eq!(drag_moved(&mut dragging, at(110.0, 95.0), Some(true)), Some((10.0, -5.0)));
        assert_eq!(drag_moved(&mut dragging, at(130.0, 95.0), Some(true)), Some((20.0, 0.0)));
        // Released where the face did not hear it: the next move ends the drag and turns nothing, nor does any after.
        assert_eq!(drag_moved(&mut dragging, at(200.0, 95.0), Some(false)), None);
        assert_eq!(dragging, None);
        assert_eq!(drag_moved(&mut dragging, at(220.0, 95.0), Some(true)), None);
    }

    #[test]
    fn the_face_draws_only_on_show_and_at_most_sixty_times_a_second() {
        use dock::View;
        assert!(on_show(true, false, [View::Angel, View::Stage]));
        assert!(!on_show(false, false, [View::Angel]), "another page");
        assert!(!on_show(true, true, [View::Angel]), "the grants editor covers the docks");
        assert!(!on_show(true, false, [View::Stage, View::Actions]), "another tab in Sinai's dock");
        assert_eq!(pace(true, false), Pace::Draw);
        assert_eq!(pace(false, false), Pace::Look, "one look for a card that has not drawn yet");
        assert_eq!(pace(false, true), Pace::Rest, "the software renderer: no clock at all");
        let now = Instant::now();
        assert!(next_frame(now) - now >= Duration::from_secs_f64(1.0 / 60.0) - Duration::from_nanos(1));
        assert_eq!(FRAME.as_nanos(), 16_666_667);
    }

    /// A folder of its own under the temporary directory, with a home, a roaming folder and a temp inside.
    fn scratch(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("centcom-face-{tag}-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_appearance_is_found_where_angel_keeps_it_without_moving_anything() {
        let root = scratch("where");
        let (home, roaming, temp) = (root.join("home"), root.join("roaming"), root.join("temp"));
        let roaming_text = roaming.display().to_string();
        let owner = |name: &str| (name == "APPDATA").then(|| roaming_text.clone());
        // before Angel has moved its files in: the old file, which that move copies in
        assert_eq!(appearance_at(&owner, Some(&home), &temp), Some(roaming.join("Alelyon").join("sinai-appearance.json")));
        assert!(!home.exists(), "nothing was created or moved");
        // an agent's start keeps its own folder until then, as Angel's does
        let agent = |name: &str| if name == "CLAUDECODE" { Some("1".to_string()) } else { owner(name) };
        assert_eq!(
            appearance_at(&agent, Some(&home), &temp),
            Some(temp.join("alelyon-agent-state").join("Angel").join("sinai-appearance.json"))
        );
        // after the move: the state home, for everyone
        let state = home.join(".alelyon").join("angel");
        std::fs::create_dir_all(&state).unwrap();
        std::fs::write(state.join(state_home::MARKER), "{}").unwrap();
        assert_eq!(appearance_at(&owner, Some(&home), &temp), Some(state.join("sinai-appearance.json")));
        assert_eq!(appearance_at(&agent, Some(&home), &temp), Some(state.join("sinai-appearance.json")));
        // ANGEL_APPEARANCE names a file, used as it is
        let named = |name: &str| if name == "ANGEL_APPEARANCE" { Some("D:/sinai.json".to_string()) } else { owner(name) };
        assert_eq!(appearance_at(&named, None, &temp), Some(PathBuf::from("D:/sinai.json")));
        assert_eq!(appearance_at(&owner, None, &temp), None, "no home, no file");
        assert!(!roaming.exists() && !temp.exists(), "no folder was made anywhere");
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn an_appearance_saved_reads_back_and_a_broken_one_is_left_alone() {
        let root = scratch("save");
        let path = root.join("sinai-appearance.json");
        assert_eq!(load(Some(&path)).current, Appearance::default(), "no file: Sinai as it ships");
        let catalog = Catalog::builtin();
        let chin = catalog.control("chin.width").unwrap();
        let mut saved = Saved::default();
        saved.current.set(chin, None, 0.25);
        saved.current.palette.set("lattice", Some([10, 200, 30]));
        saved.looks.push(("Mine".into(), saved.current.clone()));
        save(&saved, Some(&path)).unwrap();
        let back = load(Some(&path));
        assert_eq!(back.current, saved.current);
        assert_eq!(back.looks, saved.looks);
        assert!(back.notice.is_empty());
        let names: Vec<_> = std::fs::read_dir(&root).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(names.len(), 1, "written whole and renamed: nothing left beside it ({names:?})");
        std::fs::write(&path, "not an appearance").unwrap();
        let broken = load(Some(&path));
        assert_eq!(broken.current, Appearance::default());
        assert!(broken.notice.contains("untouched"), "{}", broken.notice);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "not an appearance", "reading changed nothing");
        assert!(save(&saved, None).is_err());
        std::fs::remove_dir_all(&root).unwrap();
    }
}
