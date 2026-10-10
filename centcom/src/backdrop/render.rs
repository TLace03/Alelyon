//! The backdrop on the graphics card.
//!
//! The fog is kept in two textures, each holding one march of it (at a scene time the clock planned, `FogPlan`).
//! When a frame needs a march neither holds, `prepare` steps the fluid to that time (Stable Fluids steps of at most a
//! tenth of a second: curl, forces, divergence, a Jacobi pressure solve warm-started from the last step's pressure,
//! the gradient taken off, advection) on small half-float textures, then marches the fog at a quarter of the window's
//! resolution into the texture the frame no longer needs; on encoders of the backdrop's own, submitted before iced
//! submits its frame, as Sinai's face does. `draw` then paints the window's rectangle in iced's own pass: the plain, the
//! sky and the two marches of the fog over them, blended by the frame's place between them, tone-mapped and dithered.
//! The plain and the sky never move, so they are drawn once for the window's size into a texture of their own (the
//! kept plain, ten bits a channel) and the window's pass reads them back; a card that refuses that texture, or a window
//! larger than 2560 x 1600, works them out every frame instead. A frame with nothing new (the same time and size: the
//! window redrawn for the form's sake) costs only the window's pass.
//!
//! Everything is made inside error scopes (`scoped`, as `face::render` makes its own): a card or driver that refuses a pipeline
//! leaves the backdrop undrawn and says why on the card, and the gradient under it shows. A card that cannot render
//! into half-float textures gets the fog without the fluid, its fog kept in eight bits, square-root encoded.
//!
//! What it keeps on the card, at most: five fluid textures of 256 x 160 half-float RGBA (8 bytes a cell: 320 KiB
//! each, 1.6 MiB), two of the fog's 480 x 300 (1.1 MiB each) and the kept plain (4 bytes a pixel, at most 2560 x 1600:
//! 16 MiB), with three small uniform buffers. At the default 1440 x 900 window at scale 1: 5 x 184 x 112 x 8 B =
//! 805 KiB for the fluid, 2 x 360 x 226 x 8 B = 1271 KiB for the fog and 1440 x 900 x 4 B = 5063 KiB for the plain.

use std::sync::Arc;
use std::time::{Duration, Instant};

use iced::Rectangle;
use iced::wgpu;
use iced::widget::shader::{self, Viewport};

use super::{Frame, Splat};
use crate::face::Card;
use crate::face::render::pixels;

pub const SIM_WGSL: &str = include_str!("sim.wgsl");
pub const SCENE_WGSL: &str = include_str!("scene.wgsl");

/// The fluid's textures: half floats, so velocities and the faint dye keep their precision.
pub const SIM_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
/// The fog's texture where half floats cannot be rendered into.
pub const FOG_FALLBACK: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
/// What lies under the fog (the plain and the sky), kept once for the window's size: ten bits a channel, cube-root
/// encoded (`scene.wgsl`'s `keep`), four bytes a pixel.
pub const PLAIN_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgb10a2Unorm;

/// Jacobi iterations of the pressure solve a step: even, so the pressure ends where it began.
pub const JACOBI: usize = 20;
/// Steps of the fluid's ambient rolling taken before the first frame shows, so it opens mid-roll.
pub const WARM_STEPS: usize = 48;
/// The same after the window is resized enough to remake the fluid.
pub const RESIZE_WARM_STEPS: usize = 8;

/// The fluid: an eighth of the window, at most 256 x 160 cells, in whole multiples of 8 (so resizing the window
/// remakes it only now and then).
pub const SIM_MAX: (u32, u32) = (256, 160);
/// The fog: a quarter of the window, at most 480 x 300.
pub const FOG_MAX: (u32, u32) = (480, 300);

/// `px` scaled down by `divisor`, and further if it would exceed `max`, keeping its shape, in multiples of `quantum`.
pub fn scaled(px: (u32, u32), divisor: f32, max: (u32, u32), quantum: u32) -> (u32, u32) {
    let (w, h) = (px.0.max(1) as f32, px.1.max(1) as f32);
    let k = (1.0 / divisor).min(max.0 as f32 / w).min(max.1 as f32 / h);
    let q = |v: f32| (((v * k) / quantum as f32).round() as u32 * quantum).max(quantum);
    (q(w), q(h))
}

pub fn sim_size(px: (u32, u32)) -> (u32, u32) {
    scaled(px, 8.0, SIM_MAX, 8)
}

pub fn fog_size(px: (u32, u32)) -> (u32, u32) {
    scaled(px, 4.0, FOG_MAX, 2)
}

/// Bytes on the card for the fluid (five textures) and the fog (two) at a window of `px` device pixels.
pub fn texture_bytes(px: (u32, u32), fluid: bool) -> (u64, u64) {
    let (sw, sh) = sim_size(px);
    let (fw, fh) = fog_size(px);
    let fluid_bytes = if fluid { 5 * sw as u64 * sh as u64 * 8 } else { 0 };
    let fog_bytes = 2 * fw as u64 * fh as u64 * if fluid { 8 } else { 4 };
    (fluid_bytes, fog_bytes)
}

/// The largest window whose plain is kept (2560 x 1600, 16 MiB of it); a larger one works its plain out each frame,
/// trading the card's time for its memory.
pub const PLAIN_MAX_PIXELS: u64 = 2560 * 1600;

pub fn keeps_plain(px: (u32, u32)) -> bool {
    px.0 as u64 * px.1 as u64 <= PLAIN_MAX_PIXELS
}

/// Bytes on the card for the kept plain at a window of `px` device pixels (none when it is not kept).
pub fn plain_bytes(px: (u32, u32)) -> u64 {
    if keeps_plain(px) { px.0 as u64 * px.1 as u64 * 4 } else { 0 }
}

/// The longest step the fluid takes: a sixtieth of a second while stirred, as it always did (the stirring looks as it
/// did), a tenth while calm (the slow rolling needs no finer); and the shortest (a march that does not move time on
/// still takes the stroke).
pub const STIRRED_STEP: f32 = 1.0 / 60.0;
pub const CALM_STEP: f32 = 0.1;
pub const MIN_STEP: f32 = 1.0 / 60.0;
/// The most fluid time one march makes up: a longer gap (a pause) is skipped, not caught up.
pub const MAX_GAP: f32 = 0.2;

/// The fluid's steps to reach a march at `at` from where it stands (`from`, none for a fluid just made), with or
/// without a stroke, in steps of at most `longest`: their count and length. None when it is there already and nothing
/// stirs it.
pub fn fluid_steps(from: Option<f32>, at: f32, stroke: bool, longest: f32) -> Option<(u32, f32)> {
    let gap = match from {
        Some(f) if at > f => (at - f).clamp(MIN_STEP, MAX_GAP),
        _ if stroke => MIN_STEP,
        _ => return None,
    };
    // (a hair under, so a fifth of a second in float is two steps, not three)
    let n = (gap / longest.max(MIN_STEP) - 1e-3).ceil().max(1.0);
    Some((n as u32, gap / n))
}

// ------------------------------------------------------------------- the camera and the light

/// The eye's height over the plain.
pub const EYE: f32 = 0.55;
/// Looking down a little, so the horizon sits in the upper part of the window.
pub const PITCH: f32 = -0.20;
/// Rolled a touch, so the horizon runs on a slant, rising to the right: along the old gradient's lines of equal colour
/// (it ran at 135 degrees, warm upper left to black lower right), so the composition keeps its diagonal.
pub const ROLL: f32 = -0.06;
/// The vertical field of view.
pub const FOV_Y: f32 = 50.0;
/// Where the light shows in the window (0..1 across and down): the upper left of what the form leaves visible.
pub const LIGHT_AT: [f32; 2] = [0.36, 0.215];
/// How far off it is, along that line of sight.
pub const LIGHT_DIST: f32 = 22.0;
/// Its colour, linear: the brand gold (#e6c46a), a shade deeper so the brightest fog lands on it.
pub const GOLD: [f32; 3] = [0.79, 0.50, 0.13];
pub const LIGHT_POWER: f32 = 2.1;
pub const EXPOSURE: f32 = 1.0;

/// The camera: where it is and its right, up and forward, with the tangent of half its vertical field.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    pub pos: glam::Vec3,
    pub right: glam::Vec3,
    pub up: glam::Vec3,
    pub fwd: glam::Vec3,
    pub tan_half: f32,
}

pub fn camera() -> Camera {
    let fwd = glam::vec3(0.0, PITCH.sin(), PITCH.cos());
    let right = fwd.cross(glam::Vec3::Y).normalize();
    let up = right.cross(fwd);
    let (s, c) = ROLL.sin_cos();
    Camera {
        pos: glam::vec3(0.0, EYE, 0.0),
        right: right * c + up * s,
        up: up * c - right * s,
        fwd,
        tan_half: (FOV_Y.to_radians() * 0.5).tan(),
    }
}

impl Camera {
    /// The line of sight through a point of the window (0..1 across and down), as `scene.wgsl`'s `ray_dir`.
    pub fn ray(&self, uv: [f32; 2], aspect: f32) -> glam::Vec3 {
        let (x, y) = (uv[0] * 2.0 - 1.0, 1.0 - uv[1] * 2.0);
        (self.fwd + self.right * (x * self.tan_half * aspect) + self.up * (y * self.tan_half)).normalize()
    }

    /// Where a point of the scene shows in the window (0..1 across and down), if in front of the eye.
    #[cfg(test)]
    pub fn project(&self, p: glam::Vec3, aspect: f32) -> Option<[f32; 2]> {
        let d = p - self.pos;
        let z = d.dot(self.fwd);
        (z > 0.0).then(|| {
            let x = d.dot(self.right) / z / (self.tan_half * aspect);
            let y = d.dot(self.up) / z / self.tan_half;
            [(x + 1.0) * 0.5, (1.0 - y) * 0.5]
        })
    }

    /// Where the light is for a window `aspect` wide over high.
    pub fn light(&self, aspect: f32) -> glam::Vec3 {
        self.pos + self.ray(LIGHT_AT, aspect) * LIGHT_DIST
    }
}

// ------------------------------------------------------------------- the uniforms

/// Everything `scene.wgsl` reads (its `SceneU`, in its order).
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SceneU {
    pub cam_pos: [f32; 4],
    pub cam_right: [f32; 4],
    pub cam_up: [f32; 4],
    pub cam_fwd: [f32; 4],
    pub light_pos: [f32; 4],
    pub light_col: [f32; 4],
    pub view: [f32; 4],
    pub flags: [f32; 4],
}

/// How the scene is drawn this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Look {
    /// The target's size in device pixels, and the fog's.
    pub px: (u32, u32),
    pub fog: (u32, u32),
    /// The fluid is there to read.
    pub fluid: bool,
    /// The window's surface is not sRGB, so the shader encodes.
    pub encode: bool,
    /// The fog is kept in eight bits, square-root encoded.
    pub fog_encoded: bool,
    /// Changes the grain each frame.
    pub seed: u32,
    /// How much of the newer march of the fog the window shows (0..1; the window's pass only).
    pub blend: f32,
}

pub fn scene_uniform(t: f32, look: &Look) -> SceneU {
    let cam = camera();
    let aspect = look.px.0.max(1) as f32 / look.px.1.max(1) as f32;
    let light = cam.light(aspect);
    let four = |v: glam::Vec3, w: f32| [v.x, v.y, v.z, w];
    let flag = |b: bool| if b { 1.0 } else { 0.0 };
    SceneU {
        cam_pos: four(cam.pos, cam.tan_half),
        cam_right: four(cam.right, aspect),
        cam_up: four(cam.up, t),
        cam_fwd: four(cam.fwd, flag(look.fluid)),
        light_pos: four(light, LIGHT_POWER),
        light_col: [GOLD[0], GOLD[1], GOLD[2], EXPOSURE],
        view: [look.px.0 as f32, look.px.1 as f32, look.fog.0 as f32, look.fog.1 as f32],
        flags: [flag(look.encode), flag(look.fog_encoded), (look.seed % 251) as f32, look.blend],
    }
}

/// Everything `sim.wgsl` reads (its `SimU`, in its order).
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SimU {
    pub texel: [f32; 4],
    pub step: [f32; 4],
    pub stroke: [f32; 4],
    pub force: [f32; 4],
    pub ambient: [f32; 4],
}

/// How fast the fluid's velocity and dye fade (per second), the rolling force, vorticity confinement and the ambient
/// dye; the brush's radius (in heights), how much of the cursor's speed it passes on and how much gold it lays down.
pub const VELOCITY_FADE: f32 = 0.35;
pub const DYE_FADE: f32 = 0.30;
pub const ROLLING: f32 = 3.0;
pub const VORTICITY: f32 = 18.0;
pub const AMBIENT_DYE: f32 = 0.35;
pub const BRUSH: f32 = 0.045;
pub const PUSH: f32 = 0.7;
pub const GOLD_DYE: f32 = 0.6;

pub fn sim_uniform(size: (u32, u32), t: f32, dt: f32, splat: Option<&Splat>) -> SimU {
    let (w, h) = (size.0.max(1) as f32, size.1.max(1) as f32);
    let (stroke, force) = match splat {
        Some(s) => {
            let speed = (s.velocity[0].powi(2) + s.velocity[1].powi(2)).sqrt();
            // the brush pushes and lays gold once a step, as much as a sixtieth of a second's steps would: a step
            // that stands for two of them stirs twice as hard
            let k = (dt * 60.0).clamp(1.0, 6.0);
            (
                [s.from[0], s.from[1], s.to[0], s.to[1]],
                [s.velocity[0] * PUSH * k, s.velocity[1] * PUSH * k, GOLD_DYE * (speed / 2.0).min(1.0) * k, BRUSH],
            )
        }
        // no stroke: a brush far outside the window, with nothing on it
        None => ([-10.0, -10.0, -10.0, -10.0], [0.0, 0.0, 0.0, BRUSH]),
    };
    SimU {
        texel: [1.0 / w, 1.0 / h, w, h],
        step: [dt, t, VELOCITY_FADE, DYE_FADE],
        stroke,
        force,
        ambient: [ROLLING, VORTICITY, AMBIENT_DYE, w / h],
    }
}

// ------------------------------------------------------------------- the card's objects

/// One frame for the card.
#[derive(Debug)]
pub struct Primitive {
    frame: Frame,
    card: Arc<Card>,
}

impl Primitive {
    pub fn new(frame: Frame, card: Arc<Card>) -> Primitive {
        Primitive { frame, card }
    }
}

struct SimPipes {
    curl: wgpu::RenderPipeline,
    force: wgpu::RenderPipeline,
    divergence: wgpu::RenderPipeline,
    jacobi: wgpu::RenderPipeline,
    gradient: wgpu::RenderPipeline,
    advect: wgpu::RenderPipeline,
}

/// The fluid's textures and the bind groups its stages read them through. `cur` is the velocity texture that holds
/// the field now; the other is the next step's.
struct Fluid {
    size: (u32, u32),
    v: [wgpu::TextureView; 2],
    p: [wgpu::TextureView; 2],
    x: wgpu::TextureView,
    /// (V[i], scratch), (V[i], P0), (P[i], scratch).
    v_x: [wgpu::BindGroup; 2],
    v_p: [wgpu::BindGroup; 2],
    p_x: [wgpu::BindGroup; 2],
    cur: usize,
}

/// What is sized to the window: the fog's two textures, the fluid, and the bind groups the fog's march (one for each
/// of the fluid's two velocity textures) and the window's pass (one for each fog texture being the newer) read them
/// through.
struct Slot {
    px: (u32, u32),
    fog_size: (u32, u32),
    fog_views: [wgpu::TextureView; 2],
    /// The scene time of the march each fog texture holds.
    fog_t: [Option<f32>; 2],
    fluid: Option<Fluid>,
    fog_binds: [wgpu::BindGroup; 2],
    composite_binds: [wgpu::BindGroup; 2],
    /// The kept plain, the window's pass's bind groups that read it with the fog, and whether it is drawn.
    plain: Option<(wgpu::TextureView, [wgpu::BindGroup; 2])>,
    plain_drawn: bool,
    /// The scene time the fluid was last stepped to.
    fluid_t: Option<f32>,
    /// Which fog texture the window's pass takes as the newer.
    newer: usize,
    /// The frame last prepared, to tell a redraw for the form's sake.
    last: Option<Frame>,
    frames: u32,
    ready: bool,
}

impl Slot {
    fn cur(&self) -> usize {
        self.fluid.as_ref().map_or(0, |f| f.cur)
    }
}

/// Which fog texture a march at `at` goes into: never the one holding `keep` (the other march the frame shows); of
/// the two left, an empty one, else the one with the older march.
pub fn fog_target(fog_t: [Option<f32>; 2], keep: Option<f32>) -> usize {
    let free = |i: usize| keep.is_none() || fog_t[i] != keep;
    match (free(0), free(1)) {
        (true, false) => 0,
        (false, true) => 1,
        _ => match (fog_t[0], fog_t[1]) {
            (None, _) => 0,
            (_, None) => 1,
            (Some(a), Some(b)) => usize::from(b < a),
        },
    }
}

/// The debug aid's tally (`CENTCOM_BACKDROP_TIMING`): frames prepared, fluid steps and fog marches made, and what
/// they took, waited on the card.
struct Timing {
    since: Instant,
    frames: u32,
    steps: u32,
    batches: u32,
    marches: u32,
    fluid: Duration,
    fog: Duration,
    composite: Duration,
    probe: Option<((u32, u32), wgpu::TextureView)>,
}

struct KeptPipes {
    plain: wgpu::RenderPipeline,
    cached: wgpu::RenderPipeline,
}

struct Made {
    format: wgpu::TextureFormat,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    sim: Option<SimPipes>,
    fog_format: wgpu::TextureFormat,
    fog: wgpu::RenderPipeline,
    /// The window's pass working the plain out each frame, and, where the card keeps the plain, the pass that draws
    /// it once and the window's pass from it.
    composite: wgpu::RenderPipeline,
    kept: Option<KeptPipes>,
    sim_u: wgpu::Buffer,
    /// The fog's march's uniform, and the window's pass's.
    scene_u: wgpu::Buffer,
    composite_u: wgpu::Buffer,
    blank: wgpu::TextureView,
    slot: Option<Slot>,
    timing: Option<Timing>,
}

pub struct Pipeline {
    made: Result<Made, String>,
}

impl shader::Pipeline for Pipeline {
    fn new(device: &wgpu::Device, _queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Pipeline {
        Pipeline { made: scoped(device, || make(device, format)) }
    }
}

/// Run `work` inside validation and internal error scopes: `Err` with the card's words when it refused anything
/// (as `face::render` does).
fn scoped<T>(device: &wgpu::Device, work: impl FnOnce() -> T) -> Result<T, String> {
    device.push_error_scope(wgpu::ErrorFilter::Validation);
    device.push_error_scope(wgpu::ErrorFilter::Internal);
    let out = work();
    let internal = futures::executor::block_on(device.pop_error_scope());
    let validation = futures::executor::block_on(device.pop_error_scope());
    match internal.or(validation) {
        Some(error) => Err(error.to_string()),
        None => Ok(out),
    }
}

/// Which fog format a card gets, and whether its fog is kept encoded: half floats with the fluid, eight bits
/// without.
pub fn fog_format(fluid: bool) -> (wgpu::TextureFormat, bool) {
    if fluid { (SIM_FORMAT, false) } else { (FOG_FALLBACK, true) }
}

fn texture(device: &wgpu::Device, label: &str, size: (u32, u32), format: wgpu::TextureFormat) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d { width: size.0.max(1), height: size.1.max(1), depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
        .create_view(&Default::default())
}

fn full_screen(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    module: &wgpu::ShaderModule,
    entry: &str,
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("centcom.backdrop"),
        layout: Some(layout),
        vertex: wgpu::VertexState { module, entry_point: Some("vs_main"), compilation_options: Default::default(), buffers: &[] },
        fragment: Some(wgpu::FragmentState {
            module,
            entry_point: Some(entry),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })],
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
        cache: None,
    })
}

fn uniform(device: &wgpu::Device, label: &str, size: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: size as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn make(device: &wgpu::Device, format: wgpu::TextureFormat) -> Made {
    let texture_entry = |binding| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    };
    // One layout for every pass: a uniform, three textures (the third only for the window's pass from the kept plain)
    // and a sampler.
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("centcom.backdrop"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                count: None,
            },
            texture_entry(1),
            texture_entry(2),
            texture_entry(4),
            wgpu::BindGroupLayoutEntry {
                binding: 3,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    });
    let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("centcom.backdrop"),
        bind_group_layouts: &[&layout],
        push_constant_ranges: &[],
    });
    // The fluid, only where the card renders into half floats: a refusal here costs the fluid, not the fog.
    let sim = scoped(device, || {
        let _probe = texture(device, "centcom.backdrop.probe", (1, 1), SIM_FORMAT);
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("centcom.backdrop.sim"),
            source: wgpu::ShaderSource::Wgsl(SIM_WGSL.into()),
        });
        let stage = |entry| full_screen(device, &pl, &module, entry, SIM_FORMAT);
        SimPipes {
            curl: stage("fs_curl"),
            force: stage("fs_force"),
            divergence: stage("fs_divergence"),
            jacobi: stage("fs_jacobi"),
            gradient: stage("fs_gradient"),
            advect: stage("fs_advect"),
        }
    })
    .inspect_err(|why| eprintln!("centcom: the sign-in backdrop's fog is drawn without its fluid: {why}"))
    .ok();
    let (fog_format, _) = fog_format(sim.is_some());
    let scene = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("centcom.backdrop.scene"),
        source: wgpu::ShaderSource::Wgsl(SCENE_WGSL.into()),
    });
    Made {
        format,
        sampler: device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("centcom.backdrop"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        }),
        sim,
        fog_format,
        fog: full_screen(device, &pl, &scene, "fs_fog", fog_format),
        composite: full_screen(device, &pl, &scene, "fs_main", format),
        // The plain kept in ten bits, where the card renders into them: a refusal here costs only the saving.
        kept: scoped(device, || {
            let _probe = texture(device, "centcom.backdrop.probe", (1, 1), PLAIN_FORMAT);
            KeptPipes {
                plain: full_screen(device, &pl, &scene, "fs_plain", PLAIN_FORMAT),
                cached: full_screen(device, &pl, &scene, "fs_cached", format),
            }
        })
        .inspect_err(|why| eprintln!("centcom: the sign-in backdrop works its plain out every frame: {why}"))
        .ok(),
        sim_u: uniform(device, "centcom.backdrop.sim", std::mem::size_of::<SimU>()),
        scene_u: uniform(device, "centcom.backdrop.scene", std::mem::size_of::<SceneU>()),
        composite_u: uniform(device, "centcom.backdrop.composite", std::mem::size_of::<SceneU>()),
        blank: texture(device, "centcom.backdrop.blank", (1, 1), FOG_FALLBACK),
        layout,
        slot: None,
        timing: super::switched_on(std::env::var(super::TIMING_VAR).ok().as_deref()).then(|| Timing {
            since: Instant::now(),
            frames: 0,
            steps: 0,
            batches: 0,
            marches: 0,
            fluid: Duration::ZERO,
            fog: Duration::ZERO,
            composite: Duration::ZERO,
            probe: None,
        }),
    }
}

/// Wait for the card to finish `index` (the debug aid only).
fn wait(device: &wgpu::Device, index: wgpu::SubmissionIndex) -> Duration {
    let begun = Instant::now();
    let _ = device.poll(wgpu::PollType::Wait { submission_index: Some(index), timeout: Some(Duration::from_secs(2)) });
    begun.elapsed()
}

/// The window's pass for `slot`: from the kept plain once it is drawn, else working the plain out.
fn window_pass<'a>(
    kept: &'a Option<KeptPipes>,
    composite: &'a wgpu::RenderPipeline,
    slot: &'a Slot,
) -> (&'a wgpu::RenderPipeline, &'a wgpu::BindGroup) {
    match (kept, &slot.plain, slot.plain_drawn) {
        (Some(k), Some((_, binds)), true) => (&k.cached, &binds[slot.newer]),
        _ => (composite, &slot.composite_binds[slot.newer]),
    }
}

/// One full-screen pass of `pipeline` into `target`.
fn pass(encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView, pipeline: &wgpu::RenderPipeline, bind: &wgpu::BindGroup) {
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("centcom.backdrop"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: target,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
    });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, bind, &[]);
    pass.draw(0..3, 0..1);
}

impl Made {
    fn bind(&self, device: &wgpu::Device, uniform: &wgpu::Buffer, a: &wgpu::TextureView, b: &wgpu::TextureView) -> wgpu::BindGroup {
        self.bind3(device, uniform, a, b, None)
    }

    /// The same with a third texture (the kept plain, for the window's pass).
    fn bind3(
        &self,
        device: &wgpu::Device,
        uniform: &wgpu::Buffer,
        a: &wgpu::TextureView,
        b: &wgpu::TextureView,
        c: Option<&wgpu::TextureView>,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("centcom.backdrop"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: uniform.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(a) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(b) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(c.unwrap_or(&self.blank)) },
            ],
        })
    }

    fn fluid(&self, device: &wgpu::Device, size: (u32, u32)) -> Fluid {
        let tex = |label| texture(device, label, size, SIM_FORMAT);
        let v = [tex("centcom.backdrop.velocity"), tex("centcom.backdrop.velocity")];
        let p = [tex("centcom.backdrop.pressure"), tex("centcom.backdrop.pressure")];
        let x = tex("centcom.backdrop.scratch");
        let u = &self.sim_u;
        let v_x = [self.bind(device, u, &v[0], &x), self.bind(device, u, &v[1], &x)];
        let v_p = [self.bind(device, u, &v[0], &p[0]), self.bind(device, u, &v[1], &p[0])];
        let p_x = [self.bind(device, u, &p[0], &x), self.bind(device, u, &p[1], &x)];
        Fluid { size, v, p, x, v_x, v_p, p_x, cur: 0 }
    }

    /// Make the slot for a window of `px`, keeping the fluid of `old` where its size still fits. Says how many warm-up
    /// steps the fluid needs.
    fn slot(&self, device: &wgpu::Device, px: (u32, u32), old: Option<Slot>) -> (Slot, usize) {
        let fog_size = fog_size(px);
        let fog_views = [0, 1].map(|_| texture(device, "centcom.backdrop.fog", fog_size, self.fog_format));
        let had = old.is_some();
        let (mut fluid, mut fluid_t, frames) = match old {
            Some(o) => (o.fluid, o.fluid_t, o.frames),
            None => (None, None, 0),
        };
        let mut warm = 0;
        if self.sim.is_some() {
            let size = sim_size(px);
            if fluid.as_ref().map(|f| f.size) != Some(size) {
                fluid = Some(self.fluid(device, size));
                fluid_t = None;
                warm = if had { RESIZE_WARM_STEPS } else { WARM_STEPS };
            }
        }
        let fog_binds = match &fluid {
            Some(f) => [0, 1].map(|i| self.bind(device, &self.scene_u, &f.v[i], &f.p[0])),
            None => [0, 1].map(|_| self.bind(device, &self.scene_u, &self.blank, &self.blank)),
        };
        // the window's pass reads (the older march, the newer one)
        let composite_binds = [0, 1].map(|n| self.bind(device, &self.composite_u, &fog_views[1 - n], &fog_views[n]));
        let plain = self.kept.as_ref().filter(|_| keeps_plain(px)).map(|_| {
            let view = texture(device, "centcom.backdrop.plain", px, PLAIN_FORMAT);
            let binds = [0, 1].map(|n| self.bind3(device, &self.composite_u, &fog_views[1 - n], &fog_views[n], Some(&view)));
            (view, binds)
        });
        let slot = Slot {
            px,
            fog_size,
            fog_views,
            fog_t: [None, None],
            fluid,
            fog_binds,
            composite_binds,
            plain,
            plain_drawn: false,
            fluid_t,
            newer: 0,
            last: None,
            frames,
            ready: false,
        };
        (slot, warm)
    }

    /// One step of the fluid, submitted on its own so its uniform is its own.
    fn step(&self, device: &wgpu::Device, queue: &wgpu::Queue, fluid: &mut Fluid, u: &SimU) -> Option<wgpu::SubmissionIndex> {
        let Some(sim) = &self.sim else { return None };
        queue.write_buffer(&self.sim_u, 0, bytemuck::bytes_of(u));
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("centcom.backdrop.fluid") });
        let (c, o) = (fluid.cur, 1 - fluid.cur);
        pass(&mut encoder, &fluid.x, &sim.curl, &fluid.v_p[c]);
        pass(&mut encoder, &fluid.v[o], &sim.force, &fluid.v_x[c]);
        pass(&mut encoder, &fluid.x, &sim.divergence, &fluid.v_p[o]);
        for i in 0..JACOBI {
            let from = i % 2;
            pass(&mut encoder, &fluid.p[1 - from], &sim.jacobi, &fluid.p_x[from]);
        }
        pass(&mut encoder, &fluid.v[c], &sim.gradient, &fluid.v_p[o]);
        pass(&mut encoder, &fluid.v[o], &sim.advect, &fluid.v_p[c]);
        fluid.cur = o;
        Some(queue.submit([encoder.finish()]))
    }

    /// Step the fluid to a march at `at`, taking `splat` in with the first step; a fluid just made first warms up.
    #[allow(clippy::too_many_arguments)]
    fn advance(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        slot: &mut Slot,
        at: f32,
        splat: Option<&Splat>,
        stirred: bool,
        warm: usize,
    ) {
        let Some(fluid) = slot.fluid.as_mut() else { return };
        let size = fluid.size;
        for i in 0..warm {
            let t = at - (warm - i) as f32 / 60.0;
            self.step(device, queue, fluid, &sim_uniform(size, t, 1.0 / 60.0, None));
        }
        let from = if warm > 0 { Some(at) } else { slot.fluid_t };
        let longest = if stirred { STIRRED_STEP } else { CALM_STEP };
        if let Some((n, dt)) = fluid_steps(from, at, splat.is_some(), longest) {
            let mut last = None;
            for i in 0..n {
                let t = at - (n - 1 - i) as f32 * dt;
                // the stroke is the cursor's path since the last march: each step brushes along it, as each frame's
                // step brushed along its own stretch of the path when there was a march a frame
                last = self.step(device, queue, fluid, &sim_uniform(size, t, dt, splat));
            }
            if let (Some(timing), Some(index)) = (self.timing.as_mut(), last) {
                timing.steps += n;
                timing.batches += 1;
                timing.fluid += wait(device, index);
            }
        }
        slot.fluid_t = Some(slot.fluid_t.map_or(at, |f| f.max(at)));
    }

    /// Everything before the window's pass: the slot sized; the marches the frame shows made where neither fog
    /// texture holds them (the fluid stepped to the newer first); the window's pass told how to blend them.
    fn prepare(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, px: (u32, u32), frame: &Frame) {
        let mut warm = 0;
        if self.slot.as_ref().map(|s| (s.px, s.fog_size)) != Some((px, fog_size(px))) {
            let old = self.slot.take();
            let (slot, w) = self.slot(device, px, old);
            self.slot = Some(slot);
            warm = w;
        }
        let mut slot = self.slot.take().expect("just made");
        // A frame drawn again at the same time and size (the window redrawn for the form's sake) is the one already
        // prepared: nothing is stepped, marched or written again.
        if slot.ready && slot.last == Some(*frame) {
            self.slot = Some(slot);
            return;
        }
        let (_, fog_encoded) = fog_format(self.sim.is_some());
        let (fog, fluid, encode) = (slot.fog_size, slot.fluid.is_some(), !self.format.is_srgb());
        let look = move |seed: u32, blend: f32| Look { px, fog, fluid, encode, fog_encoded, seed, blend };
        let plan = frame.fog;
        // the newer first, so a fluid just made has warmed up before either is marched
        let wanted: &[f32] = if plan.older == plan.newer { &[plan.newer] } else { &[plan.newer, plan.older] };
        for &at in wanted {
            if slot.fog_t.contains(&Some(at)) {
                continue;
            }
            let other = wanted.iter().copied().find(|w| *w != at);
            let target = fog_target(slot.fog_t, other);
            if at == plan.newer {
                self.advance(device, queue, &mut slot, at, frame.splat.as_ref(), frame.stirred, std::mem::take(&mut warm));
            }
            let u = scene_uniform(at, &look(0, 1.0));
            queue.write_buffer(&self.scene_u, 0, bytemuck::bytes_of(&u));
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("centcom.backdrop.fog") });
            pass(&mut encoder, &slot.fog_views[target], &self.fog, &slot.fog_binds[slot.cur()]);
            let index = queue.submit([encoder.finish()]);
            slot.fog_t[target] = Some(at);
            if let Some(timing) = self.timing.as_mut() {
                timing.marches += 1;
                timing.fog += wait(device, index);
            }
        }
        slot.newer = usize::from(slot.fog_t[1] == Some(plan.newer));
        slot.frames = slot.frames.wrapping_add(1);
        let u = scene_uniform(frame.t, &look(if frame.still { 0 } else { slot.frames }, if wanted.len() == 1 { 1.0 } else { plan.blend }));
        queue.write_buffer(&self.composite_u, 0, bytemuck::bytes_of(&u));
        // what lies under the fog, once for this size (it reads only the camera and the light from the uniform)
        if let (Some(kept), Some((view, _)), false) = (&self.kept, &slot.plain, slot.plain_drawn) {
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("centcom.backdrop.plain") });
            // (through the bind group without the plain: a texture is never read while it is drawn into)
            pass(&mut encoder, view, &kept.plain, &slot.composite_binds[0]);
            queue.submit([encoder.finish()]);
            slot.plain_drawn = true;
        }
        slot.last = Some(*frame);
        slot.ready = true;
        if self.timing.is_some() {
            self.time(device, queue, &slot);
        }
        self.slot = Some(slot);
    }

    /// The debug aid: add up, and once a second also time the window's pass (drawn into a texture of the window's size
    /// and format) and print it all.
    fn time(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, slot: &Slot) {
        let (format, px) = (self.format, slot.px);
        let (composite, bind) = window_pass(&self.kept, &self.composite, slot);
        let Some(timing) = self.timing.as_mut() else { return };
        timing.frames += 1;
        let secs = timing.since.elapsed().as_secs_f64();
        if secs < 1.0 {
            return;
        }
        if timing.probe.as_ref().map(|p| p.0) != Some(px) {
            timing.probe = Some((px, texture(device, "centcom.backdrop.timing", px, format)));
        }
        let probe = &timing.probe.as_ref().expect("just made").1;
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("centcom.backdrop.timing") });
        pass(&mut encoder, probe, composite, bind);
        timing.composite = wait(device, queue.submit([encoder.finish()]));
        let (fluid_bytes, fog_bytes) = texture_bytes(px, self.sim.is_some());
        let ms = |d: Duration, n: u32| if n == 0 { 0.0 } else { d.as_secs_f64() * 1000.0 / n as f64 };
        eprintln!(
            "centcom: backdrop {}x{} ({} KiB of fluid, {} KiB of fog, {} KiB of kept plain): {:.1} frames/s; {:.1} fluid steps/s, {:.2} ms a batch; {:.1} fog marches/s, {:.2} ms each; the window's pass {:.2} ms (waited on the card)",
            px.0,
            px.1,
            fluid_bytes / 1024,
            fog_bytes / 1024,
            if slot.plain.is_some() { plain_bytes(px) / 1024 } else { 0 },
            timing.frames as f64 / secs,
            timing.steps as f64 / secs,
            ms(timing.fluid, timing.batches),
            timing.marches as f64 / secs,
            ms(timing.fog, timing.marches),
            timing.composite.as_secs_f64() * 1000.0
        );
        timing.since = Instant::now();
        timing.frames = 0;
        timing.steps = 0;
        timing.batches = 0;
        timing.marches = 0;
        timing.fluid = Duration::ZERO;
        timing.fog = Duration::ZERO;
    }
}

impl shader::Primitive for Primitive {
    type Pipeline = Pipeline;

    fn prepare(&self, pipeline: &mut Pipeline, device: &wgpu::Device, queue: &wgpu::Queue, bounds: &Rectangle, viewport: &Viewport) {
        let made = match &mut pipeline.made {
            Ok(made) => made,
            Err(why) => {
                self.card.fail(why.clone());
                return;
            }
        };
        let px = pixels(bounds, viewport.scale_factor());
        match scoped(device, || made.prepare(device, queue, px, &self.frame)) {
            Ok(()) => self.card.drew(),
            Err(why) => {
                if let Some(slot) = made.slot.as_mut() {
                    slot.ready = false;
                    slot.fog_t = [None, None];
                    slot.plain_drawn = false;
                    slot.last = None;
                }
                self.card.fail(why);
            }
        }
    }

    fn draw(&self, pipeline: &Pipeline, pass: &mut wgpu::RenderPass<'_>) -> bool {
        // iced has set the pass's viewport to the widget and its scissor to what of it is visible.
        if let Ok(made) = &pipeline.made
            && let Some(slot) = made.slot.as_ref().filter(|s| s.ready)
        {
            let (pipeline, bind) = window_pass(&made.kept, &made.composite, slot);
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, bind, &[]);
            pass.draw(0..3, 0..1);
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wgpu::naga;

    fn module(source: &str) -> (naga::Module, naga::valid::ModuleInfo) {
        let module = naga::front::wgsl::parse_str(source).unwrap_or_else(|e| panic!("the shader parses: {}", e.emit_to_string(source)));
        let info = naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty())
            .validate(&module)
            .unwrap_or_else(|e| panic!("the shader validates: {e:?}"));
        (module, info)
    }

    /// Checked without a card, as the face's shaders are: they parse and validate as wgpu 27's naga reads them and
    /// translate to the GLSL the OpenGL backend (Alelyon's default) runs.
    #[test]
    fn the_shaders_build_for_this_wgpu_and_for_opengl() {
        let sim = ["fs_curl", "fs_force", "fs_divergence", "fs_jacobi", "fs_gradient", "fs_advect"];
        for (name, source, entries) in
            [("sim.wgsl", SIM_WGSL, &sim[..]), ("scene.wgsl", SCENE_WGSL, &["fs_fog", "fs_main", "fs_plain", "fs_cached"][..])]
        {
            let (module, info) = module(source);
            let stages =
                std::iter::once(("vs_main", naga::ShaderStage::Vertex)).chain(entries.iter().map(|e| (*e, naga::ShaderStage::Fragment)));
            for (entry, stage) in stages {
                for version in
                    [naga::back::glsl::Version::Desktop(330), naga::back::glsl::Version::Embedded { version: 300, is_webgl: false }]
                {
                    let options = naga::back::glsl::Options { version, ..Default::default() };
                    let pipeline =
                        naga::back::glsl::PipelineOptions { shader_stage: stage, entry_point: entry.to_string(), multiview: None };
                    let mut out = String::new();
                    let mut writer = naga::back::glsl::Writer::new(
                        &mut out,
                        &module,
                        &info,
                        &options,
                        &pipeline,
                        naga::proc::BoundsCheckPolicies::default(),
                    )
                    .unwrap_or_else(|e| panic!("{name} {entry} for {version:?}: {e}"));
                    writer.write().unwrap_or_else(|e| panic!("{name} {entry} for {version:?}: {e}"));
                    assert!(out.contains("void main"), "{name} {entry}");
                }
            }
        }
    }

    #[test]
    fn the_uniforms_are_laid_out_as_the_shaders_read_them() {
        for (source, name, size) in [(SCENE_WGSL, "SceneU", std::mem::size_of::<SceneU>()), (SIM_WGSL, "SimU", std::mem::size_of::<SimU>())]
        {
            let (module, _) = module(source);
            let ty = module.types.iter().find(|(_, t)| t.name.as_deref() == Some(name)).unwrap_or_else(|| panic!("{name}"));
            assert_eq!(ty.1.inner.size(module.to_ctx()) as usize, size, "{name}");
        }
        assert_eq!(std::mem::size_of::<SceneU>(), 128);
        assert_eq!(std::mem::size_of::<SimU>(), 80);
    }

    #[test]
    fn the_textures_stay_small() {
        // the default window at scale 1: an eighth and a quarter of it
        assert_eq!(sim_size((1440, 900)), (184, 112));
        assert_eq!(fog_size((1440, 900)), (360, 226));
        // a big screen at scale 2 is capped, keeping its shape
        assert_eq!(sim_size((3840, 2400)), (256, 160));
        assert_eq!(fog_size((3840, 2400)), (480, 300));
        let (w, h) = sim_size((5120, 1440));
        assert!(w <= SIM_MAX.0 && h <= SIM_MAX.1 && (w as f32 / h as f32 - 5120.0 / 1440.0).abs() < 0.3, "{w}x{h}");
        // nothing is ever empty
        assert_eq!(sim_size((1, 1)), (8, 8));
        assert_eq!(fog_size((0, 0)), (2, 2));
        // the fluid moves in steps of 8 cells, so a window dragged a little remakes nothing
        assert_eq!(sim_size((1440, 900)), sim_size((1450, 905)));
        // the most it ever keeps: 1.6 MiB of fluid and two marches of fog of 1.1 MiB each
        let (fluid, fog) = texture_bytes((3840, 2400), true);
        assert_eq!((fluid, fog), (5 * 256 * 160 * 8, 2 * 480 * 300 * 8));
        assert!(fluid + fog < 4 * 1024 * 1024);
        assert_eq!(texture_bytes((1440, 900), true), (5 * 184 * 112 * 8, 2 * 360 * 226 * 8));
        assert_eq!(texture_bytes((1440, 900), false), (0, 2 * 360 * 226 * 4), "no fluid: eight-bit fog");
        // the kept plain: four bytes a pixel, up to 2560 x 1600; a larger window works it out each frame
        assert_eq!(plain_bytes((1440, 900)), 1440 * 900 * 4);
        assert!(keeps_plain((2560, 1600)) && !keeps_plain((3840, 2160)));
        assert_eq!(plain_bytes((3840, 2160)), 0);
        assert!(plain_bytes((2560, 1600)) <= 16 * 1024 * 1024);
    }

    #[test]
    fn the_fluid_makes_up_the_time_to_each_march_in_short_steps_and_never_catches_up_a_pause() {
        let (n, dt) = fluid_steps(Some(10.0), 10.0 + 1.0 / 30.0, true, STIRRED_STEP).unwrap();
        assert!(n == 2 && (dt - 1.0 / 60.0).abs() < 1e-5, "stirred: a march every two frames, a step a frame: {n} x {dt}");
        let (n, dt) = fluid_steps(Some(10.0), 10.2, false, CALM_STEP).unwrap();
        assert_eq!(n, 2, "calm: a fifth of a second in two steps");
        assert!((dt - 0.1).abs() < 1e-5 && dt <= CALM_STEP + 1e-6);
        let (n, dt) = fluid_steps(Some(10.0), 70.0, false, CALM_STEP).unwrap();
        assert!((n as f32 * dt - MAX_GAP).abs() < 1e-5, "a minute's pause is not caught up");
        assert_eq!(fluid_steps(Some(10.0), 10.0, false, CALM_STEP), None, "there already, and nothing stirs it: no work");
        assert_eq!(fluid_steps(Some(10.5), 10.2, false, CALM_STEP), None, "ahead of the march (after a calm stretch)");
        assert_eq!(fluid_steps(Some(10.5), 10.2, true, STIRRED_STEP), Some((1, MIN_STEP)), "but a stroke is always taken in");
        assert_eq!(fluid_steps(None, 10.2, false, CALM_STEP), None, "a fluid just made has warmed up to the march");
    }

    #[test]
    fn a_new_march_never_overwrites_the_other_march_the_frame_shows() {
        assert_eq!(fog_target([None, None], None), 0);
        assert_eq!(fog_target([Some(1.0), None], None), 1, "an empty texture first");
        assert_eq!(fog_target([Some(1.0), Some(2.0)], None), 0, "else the older march");
        assert_eq!(fog_target([Some(3.0), Some(2.0)], None), 1);
        assert_eq!(fog_target([Some(1.0), Some(2.0)], Some(1.0)), 1, "keeping the older: over the other");
        assert_eq!(fog_target([Some(1.0), Some(2.0)], Some(2.0)), 0);
        assert_eq!(fog_target([None, Some(2.0)], Some(2.0)), 0);
    }

    #[test]
    fn a_card_without_half_float_targets_gets_eight_bit_encoded_fog() {
        assert_eq!(fog_format(true), (SIM_FORMAT, false));
        assert_eq!(fog_format(false), (FOG_FALLBACK, true));
    }

    #[test]
    fn the_light_glows_upper_left_over_a_horizon_in_the_upper_part_of_the_window() {
        let cam = camera();
        let aspect = 1440.0 / 900.0;
        // the camera's frame is orthonormal
        for (a, b) in [(cam.right, cam.up), (cam.up, cam.fwd), (cam.fwd, cam.right)] {
            assert!(a.dot(b).abs() < 1e-5);
        }
        // the light shows where it was put, above the plain
        let light = cam.light(aspect);
        let at = cam.project(light, aspect).expect("in front");
        assert!((at[0] - LIGHT_AT[0]).abs() < 1e-4 && (at[1] - LIGHT_AT[1]).abs() < 1e-4, "{at:?}");
        assert!(light.y > 0.0 && at[0] < 0.5 && at[1] < 0.5, "upper left, off the plain");
        // the horizon: a point far off on the plain shows in the upper third, rising to the right like the old
        // gradient's lines of equal colour
        let horizon = |x: f32| cam.project(cam.pos + glam::vec3(x, -cam.pos.y, 1.0e5), aspect).expect("ahead")[1];
        let (centre, left, right) = (horizon(0.0), horizon(4.0e4), horizon(-4.0e4));
        assert!(centre > 0.15 && centre < 0.4, "{centre}");
        // which side is which: the camera's right is the screen's right
        let screen_x = |x: f32| cam.project(cam.pos + glam::vec3(x, -cam.pos.y, 1.0e5), aspect).unwrap()[0];
        let (on_left, on_right) = if screen_x(4.0e4) < screen_x(-4.0e4) { (left, right) } else { (right, left) };
        assert!(on_right < on_left, "the horizon rises to the right: {on_left} on the left, {on_right} on the right");
        // a ray through the lower right meets the plain near the eye: the dark foreground
        let down = cam.ray([0.9, 0.95], aspect);
        assert!(down.y < 0.0 && -cam.pos.y / down.y < 2.0);
    }

    #[test]
    fn the_uniforms_carry_the_frame() {
        let look = Look { px: (1440, 900), fog: (360, 226), fluid: true, encode: true, fog_encoded: false, seed: 300, blend: 0.25 };
        let u = scene_uniform(40.0, &look);
        assert_eq!(u.cam_up[3], 40.0, "the time");
        assert_eq!(u.cam_fwd[3], 1.0, "the fluid shows");
        assert_eq!(u.view, [1440.0, 900.0, 360.0, 226.0]);
        assert_eq!(u.flags, [1.0, 0.0, 49.0, 0.25], "encode, not encoded fog, the grain's seed, the newer march's share");
        assert!((u.cam_right[3] - 1.6).abs() < 1e-6);
        let none = scene_uniform(40.0, &Look { fluid: false, encode: false, fog_encoded: true, ..look });
        assert_eq!((none.cam_fwd[3], none.flags[0], none.flags[1]), (0.0, 0.0, 1.0));

        let still = sim_uniform((184, 112), 37.0, 1.0 / 60.0, None);
        assert_eq!(still.texel, [1.0 / 184.0, 1.0 / 112.0, 184.0, 112.0]);
        assert_eq!(still.force[..3], [0.0, 0.0, 0.0], "no stroke, no push and no gold");
        assert!(still.stroke.iter().all(|v| *v < -1.0), "the brush is far outside the window");
        let splat = Splat { from: [0.2, 0.3], to: [0.4, 0.3], velocity: [3.0, -4.0] };
        let stirred = sim_uniform((184, 112), 37.0, 1.0 / 60.0, Some(&splat));
        assert_eq!(stirred.stroke, [0.2, 0.3, 0.4, 0.3]);
        assert_eq!(stirred.force, [3.0 * PUSH, -4.0 * PUSH, GOLD_DYE, BRUSH], "a fast stroke lays down the most gold");
        let slow = sim_uniform((184, 112), 37.0, 1.0 / 60.0, Some(&Splat { velocity: [0.5, 0.0], ..splat }));
        assert!((slow.force[2] - GOLD_DYE * 0.25).abs() < 1e-6, "a slow one less");
        let long = sim_uniform((184, 112), 37.0, 1.0 / 30.0, Some(&splat));
        let want = [6.0 * PUSH, -8.0 * PUSH, 2.0 * GOLD_DYE, BRUSH];
        assert!(
            long.force.iter().zip(want).all(|(a, b)| (a - b).abs() < 1e-5),
            "a step of two frames stirs as two would: {:?}",
            long.force
        );
        assert!(JACOBI % 2 == 0 && (15..=25).contains(&JACOBI), "the pressure ends where it began");
    }
}
