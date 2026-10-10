//! Watching Sinai's simulator on the processor: a scene from the simulator's MJCF fixtures, stepped in real time
//! by its own physics (`sim-physics`, the CPU reference of the physics core, in `f32`), and drawn a few times a
//! second by its own renderer's host reference (`sim-raycast`, the CPU ray caster that checks the GPU renderer).
//! Nothing here opens a graphics device: the GPU paths of both crates run only in card windows.
//!
//! One thread loads the scene, steps it and draws it while the page shows it running; the window takes the newest
//! frame when it ticks. Pausing, choosing another scene or leaving the page stops that thread within one frame.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use sim_physics::{Data, Model, step_world};
use sim_raycast::camera::{Intrinsics, look_at};
use sim_raycast::from_scene::{Tessellation, scene_desc};
use sim_raycast::math::{Pose, normalize};
use sim_raycast::reference::{pose_instances, render_pixel};
use sim_raycast::scene::{Lighting, PackedScene};
use sim_world::{HostWorld, ResetNoise};

/// One scene the page offers: a fixture of the simulator's own tests, read from the repository's main checkout.
pub struct SceneFile {
    pub key: &'static str,
    pub title: &'static str,
    pub what: &'static str,
    /// Under the main checkout.
    pub path: &'static str,
}

pub const SCENES: [SceneFile; 6] = [
    SceneFile {
        key: "humanoid",
        title: "MuJoCo's humanoid",
        what: "A free root and 21 hinges, its motors at rest, so it falls as a body without tone would.",
        path: "sim/crates/sim-scene/tests/fixtures/mujoco/humanoid.xml",
    },
    SceneFile {
        key: "pile",
        title: "A pile of boxes",
        what: "Eight boxes dropped from a lattice onto a plane: contacts, friction and the Newton solver.",
        path: "sim/crates/sim-scene/tests/fixtures/mujoco/contact_pile.xml",
    },
    SceneFile {
        key: "stack",
        title: "A stack of boxes",
        what: "Three boxes resting on each other.",
        path: "sim/crates/sim-scene/tests/fixtures/mujoco/contact_stack.xml",
    },
    SceneFile {
        key: "capsules",
        title: "Capsules",
        what: "Five capsules falling onto a plane.",
        path: "sim/crates/sim-scene/tests/fixtures/mujoco/contact_capsules.xml",
    },
    SceneFile {
        key: "zoo",
        title: "Every collider",
        what: "One of each shape the physics collides, dropped together onto a plane.",
        path: "sim/crates/sim-scene/tests/fixtures/mujoco/contact_zoo.xml",
    },
    SceneFile {
        key: "constrained",
        title: "Joint limits and tendons",
        what: "A small articulated model with every kind of limit and a tendon, and no contacts.",
        path: "sim/crates/sim-scene/tests/fixtures/mujoco/constrained.xml",
    },
];

/// What a frame shows beside the picture.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stats {
    /// Simulated seconds.
    pub sim_time: f64,
    pub steps: u64,
    pub contacts: usize,
    pub constraint_rows: usize,
    pub solver_iterations: usize,
    /// Mean wall time of one physics step over the latest steps, microseconds.
    pub step_us: f64,
    /// Wall time of drawing this frame, milliseconds.
    pub frame_ms: f64,
    /// The physics fell behind real time and skipped ahead.
    pub behind: bool,
}

/// One drawn frame: RGBA, rows top to bottom.
#[derive(Clone, Debug)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    pub stats: Stats,
}

/// What loading the scene said, once: its size, its timestep, what the physics does not do that the scene asks
/// for (a solver it replaces, shape pairs that make no contact, a tendon's spring), and what the importer set aside
/// (mostly the settings of MuJoCo's own viewer), each in the simulator's own words.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Loaded {
    pub bodies: usize,
    pub geoms: usize,
    pub timestep: f64,
    pub physics_gaps: Vec<String>,
    pub set_aside: Vec<String>,
}

/// The light every scene is drawn under (a scene carries none): a sun high to one side, a blue sky. The values of
/// the renderer's own scene test (`sim-render/tests/scene_interface.rs`).
fn lighting() -> Lighting {
    Lighting {
        sun_direction: normalize([0.3, -0.2, 0.9]),
        sun_irradiance: [3.0; 3],
        sky_zenith: [0.25, 0.35, 0.55],
        horizon_boost: 1.0,
        ground: [0.2; 3],
        exposure: 1.0,
        shadows: true,
    }
}

/// A scene ready to run: its physics, its world state, its packed render scene and a camera.
pub struct Sim {
    model: Model<f32>,
    datas: Vec<Data<f32>>,
    world: HostWorld,
    packed: PackedScene,
    bodies: usize,
    camera: Pose,
    /// Where the camera looks, following the bodies' middle, and where it stands from there.
    aim: [f32; 3],
    offset: [f32; 3],
    pub loaded: Loaded,
    steps: u64,
}

impl Sim {
    pub fn load(path: &Path) -> Result<Sim, String> {
        let xml = std::fs::read_to_string(path).map_err(|e| format!("the scene {} cannot be read: {e}", path.display()))?;
        let dir = path.parent().unwrap_or(Path::new("."));
        let scene = sim_scene::mjcf::load(&xml, dir).map_err(|e| format!("the scene does not import: {e}"))?;
        let (model, not_modelled) = Model::<f32>::compile(&scene).map_err(|e| format!("the physics refuses it: {e}"))?;
        let mut world = HostWorld::new(&scene, 1).map_err(|e| format!("the world refuses it: {e}"))?;
        world.reset_with(0, &ResetNoise::NONE);
        let (desc, _) = scene_desc(&scene, lighting(), &Tessellation::default()).map_err(|e| format!("the renderer refuses it: {e}"))?;
        let packed = desc.pack().map_err(|e| format!("the renderer cannot pack it: {e}"))?;
        let datas = vec![Data::new(&model)];
        let mut sim = Sim {
            loaded: Loaded {
                bodies: scene.bodies.len(),
                geoms: scene.geoms.len(),
                timestep: scene.timestep_s,
                physics_gaps: not_modelled
                    .iter()
                    .filter(|n| !matches!(n, sim_physics::NotModelled::Unsupported { .. }))
                    .map(|n| n.to_string())
                    .collect(),
                set_aside: not_modelled
                    .iter()
                    .filter(|n| matches!(n, sim_physics::NotModelled::Unsupported { .. }))
                    .map(|n| n.to_string())
                    .collect(),
            },
            bodies: scene.bodies.len(),
            model,
            datas,
            world,
            packed,
            camera: Pose::IDENTITY,
            aim: [0.0; 3],
            offset: [0.0; 3],
            steps: 0,
        };
        sim.frame_the_bodies();
        Ok(sim)
    }

    fn poses(&self) -> Vec<Pose> {
        (0..self.bodies)
            .map(|b| {
                let (p, q) = self.world.body_pose(0, b);
                Pose::new(p, q)
            })
            .collect()
    }

    /// The bodies' middle.
    fn middle(&self) -> [f32; 3] {
        let poses = self.poses();
        let n = poses.len().max(1) as f32;
        let mut mid = [0.0f32; 3];
        for p in &poses {
            for (m, x) in mid.iter_mut().zip(p.position) {
                *m += x / n;
            }
        }
        mid
    }

    /// A camera that sees every body: aimed at their middle, from above and to one side, as far back as they spread.
    fn frame_the_bodies(&mut self) {
        let poses = self.poses();
        let mid = self.middle();
        let spread =
            poses.iter().map(|p| p.position.iter().zip(mid).map(|(x, m)| (x - m) * (x - m)).sum::<f32>().sqrt()).fold(0.0f32, f32::max);
        let back = (spread * 1.5 + 0.7).clamp(1.0, 8.0);
        // From about 15 degrees above, so the top of the frame keeps a strip of sky above the horizon.
        self.aim = [mid[0], mid[1], mid[2].max(0.25)];
        self.offset = [back * 0.78, -back * 0.98, back * 0.33];
        self.aim_camera();
    }

    fn aim_camera(&mut self) {
        let eye = [self.aim[0] + self.offset[0], self.aim[1] + self.offset[1], self.aim[2] + self.offset[2]];
        self.camera = look_at(eye, self.aim, [0.0, 0.0, 1.0]).unwrap_or(Pose::IDENTITY);
    }

    /// Move the camera a fifth of the way toward the bodies' middle, keeping its distance and angle: it follows a
    /// body that falls or rolls away without jumping.
    pub fn follow(&mut self) {
        let mid = self.middle();
        let goal = [mid[0], mid[1], mid[2].max(0.25)];
        for (a, g) in self.aim.iter_mut().zip(goal) {
            *a += 0.2 * (g - *a);
        }
        self.aim_camera();
    }

    /// Step once; the wall time it took, in microseconds.
    pub fn step(&mut self) -> Result<f64, String> {
        let start = Instant::now();
        step_world(&self.model, &mut self.datas, &mut self.world).map_err(|e| format!("the physics stopped: {e}"))?;
        self.steps += 1;
        Ok(start.elapsed().as_secs_f64() * 1e6)
    }

    pub fn timestep(&self) -> f64 {
        self.loaded.timestep
    }

    /// Draw the bodies as they are now, `width` by `height`, its rows split between `threads` threads; the wall
    /// time it took, in milliseconds.
    pub fn draw(&self, width: u32, height: u32, threads: usize) -> (Vec<u8>, f64) {
        let start = Instant::now();
        let posed = pose_instances(&self.packed, &self.poses());
        let k = Intrinsics::from_hfov(width, height, 60f32.to_radians(), 0.05, 50.0);
        let mut rgba = vec![0u8; (width * height * 4) as usize];
        let rows_each = height.div_ceil(threads.max(1) as u32).max(1);
        std::thread::scope(|scope| {
            for (band, chunk) in rgba.chunks_mut((rows_each * width * 4) as usize).enumerate() {
                let (packed, posed, cam, k) = (&self.packed, &posed, &self.camera, &k);
                scope.spawn(move || {
                    let first = band as u32 * rows_each;
                    for (i, px) in chunk.chunks_exact_mut(4).enumerate() {
                        let i = i as u32;
                        let s = render_pixel(packed, posed, cam, k, i % width, first + i / width);
                        px.copy_from_slice(&[s.rgb[0], s.rgb[1], s.rgb[2], 255]);
                    }
                });
            }
        });
        (rgba, start.elapsed().as_secs_f64() * 1e3)
    }

    pub fn stats(&self) -> Stats {
        let d = &self.datas[0];
        Stats {
            sim_time: f64::from(d.time),
            steps: self.steps,
            contacts: d.ncon,
            constraint_rows: d.nefc,
            solver_iterations: d.solver_niter,
            ..Stats::default()
        }
    }
}

/// How often a frame is drawn, at most.
pub const FRAME_EVERY: Duration = Duration::from_millis(150);
/// The most physics time one round may catch up; beyond it the run skips ahead and the frame says so.
const MAX_CATCH_UP: Duration = Duration::from_millis(250);

/// What the thread shares with the window.
#[derive(Default)]
struct Shared {
    loaded: Option<Loaded>,
    latest: Option<Arc<Frame>>,
    failed: Option<String>,
}

/// The thread that runs a scene: loads it, steps it in real time and draws a frame every `FRAME_EVERY`.
pub struct Runner {
    stop: Arc<AtomicBool>,
    shared: Arc<Mutex<Shared>>,
}

impl Runner {
    pub fn start(path: PathBuf, width: u32, height: u32, threads: usize) -> Result<Runner, String> {
        let stop = Arc::new(AtomicBool::new(false));
        let shared = Arc::new(Mutex::new(Shared::default()));
        let (s, sh) = (stop.clone(), shared.clone());
        std::thread::Builder::new()
            .name("centcom-sim".into())
            .spawn(move || run(&path, width, height, threads, &s, &sh))
            .map_err(|e| format!("the simulator's thread did not start: {e}"))?;
        Ok(Runner { stop, shared })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Shared> {
        self.shared.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn loaded(&self) -> Option<Loaded> {
        self.lock().loaded.clone()
    }

    pub fn latest(&self) -> Option<Arc<Frame>> {
        self.lock().latest.clone()
    }

    pub fn failed(&self) -> Option<String> {
        self.lock().failed.clone()
    }
}

impl Drop for Runner {
    /// The thread ends after the frame it is drawing; the window does not wait for it.
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn run(path: &Path, width: u32, height: u32, threads: usize, stop: &AtomicBool, shared: &Mutex<Shared>) {
    let put = |f: &dyn Fn(&mut Shared)| f(&mut shared.lock().unwrap_or_else(|p| p.into_inner()));
    let mut sim = match Sim::load(path) {
        Ok(sim) => sim,
        Err(why) => return put(&|s| s.failed = Some(why.clone())),
    };
    put(&|s| s.loaded = Some(sim.loaded.clone()));
    if stop.load(Ordering::Relaxed) {
        return;
    }
    let dt = Duration::from_secs_f64(sim.timestep().max(1e-5));
    let mut clock = Instant::now();
    let mut recent: Vec<f64> = Vec::new();
    while !stop.load(Ordering::Relaxed) {
        let mut owed = clock.elapsed();
        let behind = owed > MAX_CATCH_UP;
        if behind {
            owed = MAX_CATCH_UP;
        }
        let mut stepped = Duration::ZERO;
        while stepped + dt <= owed {
            match sim.step() {
                Ok(us) => recent.push(us),
                Err(why) => return put(&|s| s.failed = Some(why.clone())),
            }
            stepped += dt;
        }
        clock = if behind { Instant::now() } else { clock + stepped };
        if stop.load(Ordering::Relaxed) {
            return;
        }
        sim.follow();
        let (rgba, frame_ms) = sim.draw(width, height, threads);
        let keep = recent.len().saturating_sub(400);
        recent.drain(..keep);
        let mut stats = sim.stats();
        stats.step_us = if recent.is_empty() { 0.0 } else { recent.iter().sum::<f64>() / recent.len() as f64 };
        stats.frame_ms = frame_ms;
        stats.behind = behind;
        let frame = Arc::new(Frame { width, height, rgba, stats });
        put(&|s| s.latest = Some(frame.clone()));
        let spent = Duration::from_secs_f64(frame_ms / 1e3);
        if spent < FRAME_EVERY {
            std::thread::sleep(FRAME_EVERY - spent);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The checkout these tests run in (a worktree is its own checkout here: its scenes are the ones under test).
    fn repo() -> PathBuf {
        super::super::checkout_of(Path::new(env!("CARGO_MANIFEST_DIR"))).expect("a checkout")
    }

    #[test]
    fn every_scene_on_the_page_loads_steps_and_draws_on_the_processor() {
        let root = repo();
        for scene in &SCENES {
            let mut sim = Sim::load(&root.join(scene.path)).unwrap_or_else(|e| panic!("{}: {e}", scene.key));
            assert!(sim.loaded.bodies > 0 && sim.loaded.timestep > 0.0, "{}", scene.key);
            for _ in 0..20 {
                sim.step().unwrap();
            }
            let stats = sim.stats();
            assert_eq!(stats.steps, 20);
            assert!((stats.sim_time - 20.0 * sim.timestep()).abs() < 1e-3 * sim.timestep().max(1e-3) * 20.0 + 1e-4, "{}", scene.key);
            let (rgba, _) = sim.draw(64, 48, 2);
            assert_eq!(rgba.len(), 64 * 48 * 4);
            let lit = rgba.chunks_exact(4).filter(|p| p[3] == 255).count();
            assert_eq!(lit, 64 * 48, "every pixel drawn opaque");
            // What the camera sees, by the renderer's own segmentation: some of the scene's shapes, and the sky.
            let posed = pose_instances(&sim.packed, &sim.poses());
            let k = Intrinsics::from_hfov(64, 48, 60f32.to_radians(), 0.05, 50.0);
            let seen: std::collections::BTreeSet<u16> =
                (0..64 * 48).map(|i| render_pixel(&sim.packed, &posed, &sim.camera, &k, i % 64, i / 64).seg).collect();
            assert!(seen.iter().any(|s| *s != 0), "{}: no shape in view", scene.key);
            assert!(seen.contains(&0), "{}: no sky in view", scene.key);
        }
    }

    #[test]
    fn a_frame_split_between_threads_is_the_frame_drawn_on_one() {
        let sim = Sim::load(&repo().join(SCENES[1].path)).unwrap();
        let (one, _) = sim.draw(50, 37, 1);
        let (three, _) = sim.draw(50, 37, 3);
        let (many, _) = sim.draw(50, 37, 64);
        assert_eq!(one, three);
        assert_eq!(one, many);
    }

    #[test]
    fn a_runner_stops_when_dropped_and_says_why_a_scene_cannot_load() {
        let bad = Runner::start(PathBuf::from("no-such-scene.xml"), 32, 24, 1).unwrap();
        let start = Instant::now();
        while bad.failed().is_none() && start.elapsed() < Duration::from_secs(10) {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(bad.failed().unwrap().contains("cannot be read"));
        let good = Runner::start(repo().join(SCENES[2].path), 32, 24, 1).unwrap();
        let start = Instant::now();
        while good.latest().is_none() && start.elapsed() < Duration::from_secs(20) {
            std::thread::sleep(Duration::from_millis(20));
        }
        let frame = good.latest().expect("a frame within 20 s");
        assert_eq!((frame.width, frame.height, frame.rgba.len()), (32, 24, 32 * 24 * 4));
        assert!(good.loaded().is_some());
        let stop = good.stop.clone();
        drop(good);
        assert!(stop.load(Ordering::Relaxed));
    }

    #[test]
    #[ignore = "measures this PC's processor; run by hand"]
    fn how_long_a_step_and_a_frame_take_here() {
        for scene in &SCENES {
            let mut sim = Sim::load(&repo().join(scene.path)).unwrap();
            let steps = 200;
            let us: f64 = (0..steps).map(|_| sim.step().unwrap()).sum::<f64>() / f64::from(steps);
            let ms: Vec<f64> = (0..3).map(|_| sim.draw(448, 336, 4).1).collect();
            println!("{:<12} step {us:>8.1} us   frame 448x336 on 4 threads {:>7.1} {:>7.1} {:>7.1} ms", scene.key, ms[0], ms[1], ms[2]);
        }
    }
}
