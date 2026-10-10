//! The face on the graphics card: the Angel window's pipelines, rebuilt for iced's shader widget.
//!
//! The shaders are the ones the Angel window draws with, from the `sinai-face` crate (`angel.wgsl`, `blit.wgsl`,
//! and for the world `sky.wgsl` and `terrain.wgsl`). As there, the scene is
//! drawn into a texture of its own with a depth buffer, then put on the window: the pass iced hands a widget has no
//! depth attachment, and without one the far side of the bust draws over its near side and its teeth through its
//! lips. iced gives `prepare` no command encoder, so the offscreen pass is recorded on an encoder of its own and
//! submitted there, before iced submits the frame it is part of; `draw` then copies it into the widget's rectangle.
//!
//! Everything is made inside error scopes: a card or driver that refuses a pipeline leaves the face undrawn and says
//! why, and never takes the window down with it.
//!
//! The Angel window was built on wgpu 30; this is wgpu 27, iced 0.14's: the same calls under their older names.

use std::sync::Arc;

use iced::Rectangle;
use iced::wgpu;
use iced::widget::shader::{self, Viewport};
use wgpu::util::DeviceExt;

use super::world::{self, Camera, SkyU};
use super::{Card, Cube, Frame, GlassU, Head};
use crate::body;

pub const ANGEL_WGSL: &str = sinai_face::shaders::ANGEL;
pub const BLIT_WGSL: &str = sinai_face::shaders::BLIT;

/// The Angel window's blit, for a window whose colours are stored as they are written (egui's, and iced's on a card
/// that offers no sRGB surface). iced asks for an sRGB surface where there is one, which would encode the scene's
/// colours a second time and wash Sinai out; there the scene is drawn as the Angel window draws it, into a plain
/// texture, and this blit decodes it on the way out, so the window shows the Angel window's very values.
pub const BLIT_SRGB_WGSL: &str = r#"
@group(0) @binding(0) var scene : texture_2d<f32>;
@group(0) @binding(1) var samp  : sampler;

struct VsOut {
    @builtin(position) pos : vec4<f32>,
    @location(0) uv        : vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> VsOut {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var out : VsOut;
    out.pos = vec4<f32>(uv * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0), 0.0, 1.0);
    out.uv = uv;
    return out;
}

fn decoded(c: vec3<f32>) -> vec3<f32> {
    let low = c / 12.92;
    let high = pow((c + 0.055) / 1.055, vec3<f32>(2.4));
    return select(high, low, c <= vec3<f32>(0.04045));
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    // Premultiplied: undo the alpha, decode, and put it back.
    let c = textureSample(scene, samp, in.uv);
    let a = max(c.a, 1e-5);
    return vec4<f32>(decoded(c.rgb / a) * c.a, c.a);
}
"#;

/// What the glass head adds to `angel.wgsl`: the glass's own uniform and the cranium it covers, the cubes inside, and
/// the glass itself, which shines with the sky's lights.
const GLASS_EXTRA: &str = r#"

// ---- CENTCOM: the glass head and what is inside it

struct GlassU {
    inv_mvp  : mat4x4<f32>,   // the target's pixels back to Sinai's coordinates
    viewport : vec4<f32>,     // the target's width and height in pixels
    cut      : vec4<f32>,     // the cranium: height at the brow, its fall per unit back, the brow's depth
    light    : vec4<f32>,     // the sun or the moon as the sky draws it: direction in view space, strength
    colour   : vec4<f32>,     // its colour, and how much of the night there is
};

@group(1) @binding(0) var<uniform> glass : GlassU;

/// Whether a point of the head (its own coordinates) is in the cranium: above a cut over the brow that falls towards
/// the back of the head, so the crown and the back of the skull are glass and the face is not.
fn in_cranium(p: vec3<f32>) -> bool {
    return p.y > glass.cut.x + (p.z - glass.cut.z) * glass.cut.y;
}

/// The same for a fragment, from where it is in the target and its depth.
fn cranium_at(frag: vec4<f32>) -> bool {
    let ndc = vec4<f32>(frag.x / glass.viewport.x * 2.0 - 1.0, 1.0 - frag.y / glass.viewport.y * 2.0, frag.z, 1.0);
    let q = glass.inv_mvp * ndc;
    return in_cranium(q.xyz / q.w);
}

struct CubeOut {
    @builtin(position) pos : vec4<f32>,
    @location(0) colour    : vec3<f32>,
};

// One face of one cube: the unit cube's corner and normal, and the instance's centre, edge and colour, all in the
// head's own coordinates, so the head's matrices place it.
@vertex
fn vs_cube(@location(0) corner: vec3<f32>,
           @location(1) nrm:    vec3<f32>,
           @location(2) centre: vec3<f32>,
           @location(3) size:   f32,
           @location(4) colour: vec4<f32>) -> CubeOut {
    let p = centre + corner * (0.5 * size);
    let key = max(dot(nrm, normalize(head.light.xyz)), 0.0);
    // A little light of its own, so the brain glows inside the glass rather than sitting in its shadow.
    let lit = 0.42 + 0.58 * key;
    var out : CubeOut;
    out.pos = head.mvp * vec4<f32>(p, 1.0);
    out.colour = colour.rgb * lit;
    return out;
}

@fragment
fn fs_cube(in: CubeOut) -> @location(0) vec4<f32> {
    return vec4<f32>(in.colour, 1.0);
}

struct GlassOut {
    @builtin(position) pos : vec4<f32>,
    @location(0) local     : vec3<f32>,
    @location(1) vpos      : vec3<f32>,
    @location(2) vn        : vec3<f32>,
    @location(3) fade      : f32,
};

// The cranium moves only with the head: the jaw, the lids and the breath that vs_main adds all lie below the cut, so
// the glass is the shape as posed.
@vertex
fn vs_glass(@location(0) pos: vec3<f32>,
            @location(1) nrm: vec3<f32>,
            @location(2) w:   vec4<f32>,
            @location(3) w2:  vec4<f32>) -> GlassOut {
    var out : GlassOut;
    out.pos = head.mvp * vec4<f32>(pos, 1.0);
    out.local = pos;
    out.vpos = (head.mv * vec4<f32>(pos, 1.0)).xyz;
    out.vn = (head.mv * vec4<f32>(nrm, 0.0)).xyz;
    out.fade = w2.x;
    return out;
}

fn glint(k: vec2<f32>) -> f32 {
    return fract(sin(dot(k, vec2<f32>(12.9898, 78.233))) * 43758.5453);
}

// The cranium as glass: nearly clear where it faces the eye and brighter towards its edges (a Fresnel rim), tinted
// with the body's colours, with the sun or the moon shining in it and, at night, the stars glinting. No lattice lies
// on it. Drawn back faces first, then front faces, over what is inside.
@fragment
fn fs_glass(in: GlassOut) -> @location(0) vec4<f32> {
    if (!in_cranium(in.local)) { discard; }
    if (in.fade < dither(in.pos.xy) * 0.999) { discard; }
    let v = normalize(-in.vpos);
    var n = normalize(in.vn);
    // The far wall is seen from inside the head, and its glass faces that way.
    let near = dot(n, v) > 0.0;
    if (!near) { n = -n; }
    let facing = clamp(dot(n, v), 0.0, 1.0);
    let rim = pow(1.0 - facing, 3.0);
    var colour = mix(head.shadow.rgb, head.fill.rgb, 0.5 + 0.5 * facing);
    colour = mix(colour, head.tint.rgb, 0.75 * rim);
    var a = 0.16 + 0.62 * rim;
    // A seam glows where the glass meets the skin, so the cranium reads as set into the head.
    let over = in.local.y - (glass.cut.x + (in.local.z - glass.cut.z) * glass.cut.y);
    let seam = 1.0 - smoothstep(0.0, 0.018, over);
    colour = mix(colour, head.tint.rgb * 1.3, seam);
    a = max(a, 0.85 * seam);
    // The sky's light: a tight highlight and a broad sheen about the half vector.
    let h = normalize(normalize(glass.light.xyz) + v);
    let nh = max(dot(n, h), 0.0);
    let shine = (pow(nh, 90.0) * 2.0 + pow(nh, 14.0) * 0.18) * glass.light.w;
    // The stars, reflected: where the sky above is mirrored, a cell of the reflection may hold a glint.
    let r = reflect(-v, n);
    var stars = 0.0;
    if (glass.colour.w > 0.01 && r.y > 0.0) {
        let g = floor(r.xy * 60.0);
        let f = fract(r.xy * 60.0) - vec2<f32>(0.5);
        let k = glint(g);
        if (k > 0.96) {
            stars = exp(-dot(f, f) * 60.0) * glass.colour.w * (0.6 + 0.4 * sin(head.params.z * (1.0 + 3.0 * k) + 40.0 * k));
        }
    }
    let spec = (glass.colour.rgb * shine + vec3<f32>(0.85, 0.87, 1.0) * stars) * select(0.35, 1.0, near);
    return vec4<f32>(colour * a + spec, min(a + max(spec.r, max(spec.g, spec.b)) * 0.5, 1.0));
}
"#;

/// `angel.wgsl`, copies of its body's and its lattice's fragment entries that leave the cranium to the glass, and what
/// the glass head adds. The copies are cut from the file as it is when built, so they cannot drift from it; nothing
/// in the `sinai-face` crate is edited for the glass.
pub fn glass_source() -> String {
    let guard = "if (cranium_at(in.pos)) { discard; }";
    format!(
        "{ANGEL_WGSL}\n{}\n{}\n{GLASS_EXTRA}",
        guarded(ANGEL_WGSL, "fs_fill", "fs_fill_open", guard),
        guarded(ANGEL_WGSL, "fs_main", "fs_main_open", guard)
    )
}

/// The fragment entry `name` of `source`, copied as `renamed` with `guard` as its first statement.
fn guarded(source: &str, name: &str, renamed: &str, guard: &str) -> String {
    let opening = format!("fn {name}(in: VsOut) -> @location(0) vec4<f32> {{");
    let start = source.find(&opening).unwrap_or_else(|| panic!("angel.wgsl has {name}"));
    // Through its closing brace, whichever line endings the checkout gave the file.
    let end = start + source[start..].find("\n}").unwrap_or_else(|| panic!("{name} ends")) + 2;
    format!("@fragment\nfn {renamed}(in: VsOut) -> @location(0) vec4<f32> {{\n    {guard}{}\n", &source[start + opening.len()..end])
}

/// The unit cube's 36 corners, each with its face's normal, three floats each: corners at ±1.
fn unit_cube() -> Vec<[f32; 6]> {
    let mut out = Vec::with_capacity(36);
    for axis in 0..3 {
        for sign in [-1.0f32, 1.0] {
            let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
            let corner = |a: f32, b: f32| {
                let mut c = [0.0f32; 6];
                c[axis] = sign;
                c[u] = a;
                c[v] = b;
                c[3 + axis] = sign;
                c
            };
            // Wound counter-clockwise seen from outside: swap the diagonal's order on the negative face.
            let quad = [corner(-1.0, -1.0), corner(1.0, -1.0), corner(1.0, 1.0), corner(-1.0, 1.0)];
            let order: [usize; 6] = if sign > 0.0 { [0, 1, 2, 0, 2, 3] } else { [0, 2, 1, 0, 3, 2] };
            out.extend(order.map(|i| quad[i]));
        }
    }
    out
}

/// The unit cube's corner and normal.
const CUBE_ATTRS: [wgpu::VertexAttribute; 2] = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3];

/// Each cube's centre, edge and colour (`Cube`).
const INSTANCE_ATTRS: [wgpu::VertexAttribute; 3] = wgpu::vertex_attr_array![2 => Float32x3, 3 => Float32, 4 => Float32x4];

const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// Position, normal, then occlusion, jaw, lid and part, then fade, chest, lift and highlight (`body.rs`).
const HEAD_ATTRS: [wgpu::VertexAttribute; 4] = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x4, 3 => Float32x4];

/// The valley's grid points.
const GRID_ATTRS: [wgpu::VertexAttribute; 1] = wgpu::vertex_attr_array![0 => Float32x2];

/// What a mesh does about depth, as in the Angel window.
#[derive(Copy, Clone, PartialEq)]
enum Depth {
    /// Drawn wherever it is drawn: the sky, and the valley's lines, which show through each other and lie behind Sinai.
    Ignore,
    /// Lays down the surface everything after it is hidden behind.
    Write,
    /// Tested against that surface but not written: the lattice sits exactly on the body.
    OnSurface,
}

/// One frame for the card.
#[derive(Debug)]
pub struct Primitive {
    frame: Option<Frame>,
    card: Arc<Card>,
}

impl Primitive {
    pub fn new(frame: Option<Frame>, card: Arc<Card>) -> Primitive {
        Primitive { frame, card }
    }
}

/// The colour and depth the scene is drawn into, sized to the widget in device pixels.
struct Target {
    size: (u32, u32),
    color: wgpu::TextureView,
    depth: wgpu::TextureView,
    bind: wgpu::BindGroup,
}

/// The face's own buffers: its shape, its uniform and its target.
struct Slot {
    vertices: wgpu::Buffer,
    version: u64,
    uniform: wgpu::Buffer,
    bind: wgpu::BindGroup,
    camera: wgpu::Buffer,
    camera_bind: wgpu::BindGroup,
    sky: wgpu::Buffer,
    sky_bind: wgpu::BindGroup,
    target: Option<Target>,
    /// The cubes inside the glass head, the interior's version they were uploaded for, and how many.
    cubes: Option<(wgpu::Buffer, u64, u32)>,
    glass: wgpu::Buffer,
    glass_bind: wgpu::BindGroup,
    /// A frame is in the target, ready to be put on the window.
    ready: bool,
}

/// Made once, the first time a face is drawn on this card.
struct Made {
    /// The format the scene is drawn in.
    scene: wgpu::TextureFormat,
    fill: wgpu::RenderPipeline,
    lattice: wgpu::RenderPipeline,
    sky: wgpu::RenderPipeline,
    valley: wgpu::RenderPipeline,
    valley_vertices: wgpu::Buffer,
    valley_lines: wgpu::Buffer,
    valley_count: u32,
    blit: wgpu::RenderPipeline,
    head_layout: wgpu::BindGroupLayout,
    world_layout: wgpu::BindGroupLayout,
    blit_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    tris: wgpu::Buffer,
    tri_count: u32,
    edges: wgpu::Buffer,
    edge_count: u32,
    /// The glass head: the body and the lattice leaving the cranium out, and the cranium's back faces, then its front
    /// faces, over what is inside.
    fill_open: wgpu::RenderPipeline,
    lattice_open: wgpu::RenderPipeline,
    glass_layout: wgpu::BindGroupLayout,
    glass_back: wgpu::RenderPipeline,
    glass_front: wgpu::RenderPipeline,
    /// The cubes inside it, and the unit cube each is drawn from.
    cube: wgpu::RenderPipeline,
    unit_cube: wgpu::Buffer,
    slot: Option<Slot>,
}

pub struct Pipeline {
    made: Result<Made, String>,
}

impl shader::Pipeline for Pipeline {
    fn new(device: &wgpu::Device, _queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Pipeline {
        Pipeline { made: scoped(device, || make(device, format)) }
    }
}

/// Run `work` inside validation and internal error scopes: `Err` with the card's words when it refused anything.
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

fn uniform_layout(device: &wgpu::Device, label: &str) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some(label),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
            count: None,
        }],
    })
}

/// The premultiplied blend the lattice and the blit use: lines glow rather than punching holes.
fn premultiplied() -> wgpu::BlendState {
    wgpu::BlendState {
        color: wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
            operation: wgpu::BlendOperation::Add,
        },
        alpha: wgpu::BlendComponent::OVER,
    }
}

/// One of the bust's two pipelines (the Angel window's `Lines::new`, for the head).
fn head_pipeline(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    layout: &wgpu::BindGroupLayout,
    shader: &wgpu::ShaderModule,
    topology: wgpu::PrimitiveTopology,
    fs_entry: &str,
    depth: Depth,
) -> wgpu::RenderPipeline {
    let vertices =
        [wgpu::VertexBufferLayout { array_stride: body::STRIDE, step_mode: wgpu::VertexStepMode::Vertex, attributes: &HEAD_ATTRS }];
    pipeline(device, format, &[layout], shader, &vertices, topology, ("vs_main", fs_entry), depth == Depth::Write, depth, None)
}

/// A pipeline into the scene's target with its depth, as the Angel window makes each of its meshes (`Lines::new`,
/// `SkyPass::new`): the body opaque, everything else premultiplied over what is there.
#[allow(clippy::too_many_arguments)]
fn pipeline(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    layouts: &[&wgpu::BindGroupLayout],
    shader: &wgpu::ShaderModule,
    vertices: &[wgpu::VertexBufferLayout<'_>],
    topology: wgpu::PrimitiveTopology,
    entries: (&str, &str),
    opaque: bool,
    depth: Depth,
    cull: Option<wgpu::Face>,
) -> wgpu::RenderPipeline {
    let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("centcom.face"),
        bind_group_layouts: layouts,
        push_constant_ranges: &[],
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("centcom.face"),
        layout: Some(&pl),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some(entries.0),
            compilation_options: Default::default(),
            buffers: vertices,
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some(entries.1),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                // The body and the sky replace what is behind them; the lattice and the valley lie on top.
                blend: if opaque { None } else { Some(premultiplied()) },
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState { topology, cull_mode: cull, ..Default::default() },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: depth == Depth::Write,
            depth_compare: match depth {
                Depth::Ignore => wgpu::CompareFunction::Always,
                Depth::Write => wgpu::CompareFunction::Less,
                Depth::OnSurface => wgpu::CompareFunction::LessEqual,
            },
            stencil: wgpu::StencilState::default(),
            // The bias goes on the surface, pushed back a hair, so the lattice lying on it passes (a line list cannot
            // take a bias at all).
            bias: if depth == Depth::Write {
                wgpu::DepthBiasState { constant: 32, slope_scale: 1.5, clamp: 0.0 }
            } else {
                wgpu::DepthBiasState::default()
            },
        }),
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
        cache: None,
    })
}

fn make(device: &wgpu::Device, format: wgpu::TextureFormat) -> Made {
    let bust = body::shared();
    // The scene's own format: the window's without its sRGB encoding, as the Angel window's target stores colours.
    let scene = format.remove_srgb_suffix();
    let head_layout = uniform_layout(device, "centcom.face.head");
    let angel = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("centcom.face.angel"),
        source: wgpu::ShaderSource::Wgsl(ANGEL_WGSL.into()),
    });
    let fill = head_pipeline(device, scene, &head_layout, &angel, wgpu::PrimitiveTopology::TriangleList, "fs_fill", Depth::Write);
    let lattice = head_pipeline(device, scene, &head_layout, &angel, wgpu::PrimitiveTopology::LineList, "fs_main", Depth::OnSurface);

    // The glass head and what is inside it: the cubes lay down depth as the body does; the body and its lattice are
    // drawn as ever but for the cranium; and the cranium's glass is tested against all of it but writes no depth, so
    // its far wall shows through its near one.
    let glass_layout = uniform_layout(device, "centcom.face.glass");
    let glass_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("centcom.face.glass"),
        source: wgpu::ShaderSource::Wgsl(glass_source().into()),
    });
    let head_vertices =
        [wgpu::VertexBufferLayout { array_stride: body::STRIDE, step_mode: wgpu::VertexStepMode::Vertex, attributes: &HEAD_ATTRS }];
    let both = [&head_layout, &glass_layout];
    let open = |topology, fs, depth: Depth| {
        pipeline(device, scene, &both, &glass_shader, &head_vertices, topology, ("vs_main", fs), depth == Depth::Write, depth, None)
    };
    let fill_open = open(wgpu::PrimitiveTopology::TriangleList, "fs_fill_open", Depth::Write);
    let lattice_open = open(wgpu::PrimitiveTopology::LineList, "fs_main_open", Depth::OnSurface);
    let glass = |cull| {
        pipeline(
            device,
            scene,
            &both,
            &glass_shader,
            &head_vertices,
            wgpu::PrimitiveTopology::TriangleList,
            ("vs_glass", "fs_glass"),
            false,
            Depth::OnSurface,
            Some(cull),
        )
    };
    let (glass_back, glass_front) = (glass(wgpu::Face::Front), glass(wgpu::Face::Back));
    let cube_vertices = [
        wgpu::VertexBufferLayout { array_stride: 24, step_mode: wgpu::VertexStepMode::Vertex, attributes: &CUBE_ATTRS },
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Cube>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &INSTANCE_ATTRS,
        },
    ];
    let cube = pipeline(
        device,
        scene,
        &[&head_layout],
        &glass_shader,
        &cube_vertices,
        wgpu::PrimitiveTopology::TriangleList,
        ("vs_cube", "fs_cube"),
        true,
        Depth::Write,
        Some(wgpu::Face::Back),
    );

    // The world behind Sinai: the sky, a triangle over everything, and the valley, a grid uploaded once.
    let world_layout = uniform_layout(device, "centcom.face.world");
    let sky_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("centcom.face.sky"),
        source: wgpu::ShaderSource::Wgsl(world::SKY_WGSL.into()),
    });
    let sky =
        pipeline(device, scene, &[&world_layout], &sky_shader, &[], wgpu::PrimitiveTopology::TriangleList, ("vs_main", "fs_main"), true, Depth::Ignore, None);
    let terrain_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("centcom.face.valley"),
        source: wgpu::ShaderSource::Wgsl(world::TERRAIN_WGSL.into()),
    });
    let grid = [wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<world::Vertex>() as u64,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &GRID_ATTRS,
    }];
    let valley =
        pipeline(device, scene, &[&world_layout], &terrain_shader, &grid, wgpu::PrimitiveTopology::LineList, ("vs_main", "fs_main"), false, Depth::Ignore, None);
    let (grid_points, grid_lines) = world::build_terrain();

    let blit_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("centcom.face.blit"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    });
    let blit_source = if format.is_srgb() { BLIT_SRGB_WGSL } else { BLIT_WGSL };
    let blit_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("centcom.face.blit"),
        source: wgpu::ShaderSource::Wgsl(blit_source.into()),
    });
    let blit_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("centcom.face.blit"),
        bind_group_layouts: &[&blit_layout],
        push_constant_ranges: &[],
    });
    let blit = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("centcom.face.blit"),
        layout: Some(&blit_pl),
        vertex: wgpu::VertexState {
            module: &blit_shader,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        fragment: Some(wgpu::FragmentState {
            module: &blit_shader,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState { format, blend: Some(premultiplied()), write_mask: wgpu::ColorWrites::ALL })],
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
        cache: None,
    });
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("centcom.face.blit"),
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    });
    let index = |label: &str, idx: &[u32]| {
        device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(label),
            contents: bytemuck::cast_slice(idx),
            usage: wgpu::BufferUsages::INDEX,
        })
    };
    Made {
        scene,
        fill,
        lattice,
        sky,
        valley,
        valley_vertices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("centcom.face.valley"),
            contents: bytemuck::cast_slice(&grid_points),
            usage: wgpu::BufferUsages::VERTEX,
        }),
        valley_lines: index("centcom.face.valley", &grid_lines),
        valley_count: grid_lines.len() as u32,
        world_layout,
        blit,
        head_layout,
        blit_layout,
        sampler,
        tris: index("centcom.face.tris", &bust.tris),
        tri_count: bust.tris.len() as u32,
        edges: index("centcom.face.edges", &bust.edges),
        edge_count: bust.edges.len() as u32,
        fill_open,
        lattice_open,
        glass_layout,
        glass_back,
        glass_front,
        cube,
        unit_cube: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("centcom.face.cube"),
            contents: bytemuck::cast_slice(&unit_cube()),
            usage: wgpu::BufferUsages::VERTEX,
        }),
        slot: None,
    }
}

impl Made {
    fn slot(&mut self, device: &wgpu::Device) -> &mut Slot {
        let (layout, world_layout, glass_layout, n_draw) =
            (&self.head_layout, &self.world_layout, &self.glass_layout, body::shared().n_draw);
        self.slot.get_or_insert_with(|| {
            let vertices = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("centcom.face.vertices"),
                size: n_draw as u64 * body::STRIDE,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let uniform = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("centcom.face.head"),
                size: std::mem::size_of::<Head>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("centcom.face.head"),
                layout,
                entries: &[wgpu::BindGroupEntry { binding: 0, resource: uniform.as_entire_binding() }],
            });
            let uniform_in = |layout: &wgpu::BindGroupLayout, label: &str, size: usize| {
                let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some(label),
                    size: size as u64,
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some(label),
                    layout,
                    entries: &[wgpu::BindGroupEntry { binding: 0, resource: buffer.as_entire_binding() }],
                });
                (buffer, bind)
            };
            let (camera, camera_bind) = uniform_in(world_layout, "centcom.face.camera", std::mem::size_of::<Camera>());
            let (sky, sky_bind) = uniform_in(world_layout, "centcom.face.sky", std::mem::size_of::<SkyU>());
            let (glass, glass_bind) = uniform_in(glass_layout, "centcom.face.glass", std::mem::size_of::<GlassU>());
            Slot { vertices, version: 0, uniform, bind, camera, camera_bind, sky, sky_bind, target: None, cubes: None, glass, glass_bind, ready: false }
        })
    }

    fn target(&self, device: &wgpu::Device, size: (u32, u32)) -> Target {
        let extent = wgpu::Extent3d { width: size.0.max(1), height: size.1.max(1), depth_or_array_layers: 1 };
        let texture = |label: &str, format: wgpu::TextureFormat, usage: wgpu::TextureUsages| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: extent,
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage,
                    view_formats: &[],
                })
                .create_view(&Default::default())
        };
        let color =
            texture("centcom.face.scene", self.scene, wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING);
        let depth = texture("centcom.face.depth", DEPTH_FORMAT, wgpu::TextureUsages::RENDER_ATTACHMENT);
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("centcom.face.blit"),
            layout: &self.blit_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&color) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
            ],
        });
        Target { size, color, depth, bind }
    }
}

/// The widget's size in device pixels, at least one each way.
pub fn pixels(bounds: &Rectangle, scale: f32) -> (u32, u32) {
    ((bounds.width * scale).round().max(1.0) as u32, (bounds.height * scale).round().max(1.0) as u32)
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
        let Some(frame) = &self.frame else {
            // Nothing posed yet: the card has seen the face, so the clock may start.
            self.card.drew();
            return;
        };
        let size = pixels(bounds, viewport.scale_factor());
        if made.slot(device).target.as_ref().map(|t| t.size) != Some(size) {
            let target = made.target(device, size);
            made.slot(device).target = Some(target);
        }
        let slot = made.slot.as_mut().expect("just made");
        let target = slot.target.as_ref().expect("just made");
        let drawn = scoped(device, || {
            if slot.version != frame.version {
                queue.write_buffer(&slot.vertices, 0, bytemuck::cast_slice(&frame.verts));
            }
            queue.write_buffer(&slot.uniform, 0, bytemuck::bytes_of(&frame.head));
            // The cubes go up once per interior; a new interior may hold more or fewer, so it gets a buffer of its own.
            match frame.inside.as_ref().map(|(inside, _)| inside) {
                Some(inside) if slot.cubes.as_ref().map(|c| c.1) != Some(inside.version) => {
                    slot.cubes = (!inside.cubes.is_empty()).then(|| {
                        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                            label: Some("centcom.face.interior"),
                            contents: bytemuck::cast_slice(&inside.cubes),
                            usage: wgpu::BufferUsages::VERTEX,
                        });
                        (buffer, inside.version, inside.cubes.len() as u32)
                    });
                }
                Some(_) => {}
                None => slot.cubes = None,
            }
            if let Some((_, glass)) = &frame.inside {
                let glass = GlassU { viewport: [size.0 as f32, size.1 as f32, 0.0, 0.0], ..*glass };
                queue.write_buffer(&slot.glass, 0, bytemuck::bytes_of(&glass));
            }
            if let Some((camera, sky)) = &frame.world {
                queue.write_buffer(&slot.camera, 0, bytemuck::bytes_of(camera));
                queue.write_buffer(&slot.sky, 0, bytemuck::bytes_of(sky));
            }
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("centcom.face") });
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("centcom.face.scene"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &target.color,
                        depth_slice: None,
                        resolve_target: None,
                        // In its world the sky covers everything; alone (the creator's view) Sinai stands on a ground a
                        // little lighter than its carbon body, so its silhouette reads, as in the Angel window.
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(if frame.world.is_some() {
                                wgpu::Color::TRANSPARENT
                            } else {
                                wgpu::Color { r: 0.058, g: 0.058, b: 0.068, a: 1.0 }
                            }),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: &target.depth,
                        depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Discard }),
                        stencil_ops: None,
                    }),
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });
                if frame.world.is_some() {
                    pass.set_pipeline(&made.sky);
                    pass.set_bind_group(0, &slot.sky_bind, &[]);
                    pass.draw(0..3, 0..1);
                    pass.set_pipeline(&made.valley);
                    pass.set_bind_group(0, &slot.camera_bind, &[]);
                    pass.set_vertex_buffer(0, made.valley_vertices.slice(..));
                    pass.set_index_buffer(made.valley_lines.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..made.valley_count, 0, 0..1);
                }
                pass.set_bind_group(0, &slot.bind, &[]);
                pass.set_vertex_buffer(0, slot.vertices.slice(..));
                if frame.inside.is_some() {
                    // Glass: what is inside first, then the body and its lattice but for the cranium, then the
                    // cranium's far wall and its near one over all of it.
                    pass.set_bind_group(1, &slot.glass_bind, &[]);
                    if let Some((cubes, _, count)) = &slot.cubes {
                        pass.set_pipeline(&made.cube);
                        pass.set_vertex_buffer(0, made.unit_cube.slice(..));
                        pass.set_vertex_buffer(1, cubes.slice(..));
                        pass.draw(0..36, 0..*count);
                        pass.set_vertex_buffer(0, slot.vertices.slice(..));
                    }
                    pass.set_index_buffer(made.tris.slice(..), wgpu::IndexFormat::Uint32);
                    pass.set_pipeline(&made.fill_open);
                    pass.draw_indexed(0..made.tri_count, 0, 0..1);
                    pass.set_pipeline(&made.glass_back);
                    pass.draw_indexed(0..made.tri_count, 0, 0..1);
                    pass.set_pipeline(&made.glass_front);
                    pass.draw_indexed(0..made.tri_count, 0, 0..1);
                    pass.set_pipeline(&made.lattice_open);
                    pass.set_index_buffer(made.edges.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..made.edge_count, 0, 0..1);
                } else {
                    pass.set_pipeline(&made.fill);
                    pass.set_index_buffer(made.tris.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..made.tri_count, 0, 0..1);
                    pass.set_pipeline(&made.lattice);
                    pass.set_index_buffer(made.edges.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..made.edge_count, 0, 0..1);
                }
            }
            queue.submit([encoder.finish()]);
        });
        match drawn {
            Ok(()) => {
                slot.version = frame.version;
                slot.ready = true;
                self.card.drew();
            }
            Err(why) => {
                slot.ready = false;
                self.card.fail(why);
            }
        }
    }

    fn draw(&self, pipeline: &Pipeline, pass: &mut wgpu::RenderPass<'_>) -> bool {
        // iced has set the pass's viewport to the widget and its scissor to what of it is visible.
        if let Ok(made) = &pipeline.made
            && let Some(slot) = made.slot.as_ref().filter(|s| s.ready && self.frame.is_some())
            && let Some(target) = &slot.target
        {
            pass.set_pipeline(&made.blit);
            pass.set_bind_group(0, &target.bind, &[]);
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
        let module = naga::front::wgsl::parse_str(source).expect("the shader parses");
        let info = naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty())
            .validate(&module)
            .expect("the shader validates");
        (module, info)
    }

    /// The shaders are checked here without a card: they parse and validate as wgpu 27's naga reads them (the Angel
    /// window builds them with wgpu 30's), and they translate to the GLSL the OpenGL backend runs, which is the
    /// backend Alelyon asks for by default.
    #[test]
    fn the_angel_windows_shaders_build_for_this_wgpu_and_for_opengl() {
        let glass = glass_source();
        for (name, source, entries) in [
            (
                "angel.wgsl",
                ANGEL_WGSL,
                &[
                    ("vs_main", naga::ShaderStage::Vertex),
                    ("fs_main", naga::ShaderStage::Fragment),
                    ("fs_fill", naga::ShaderStage::Fragment),
                ][..],
            ),
            (
                "the glass head",
                glass.as_str(),
                &[
                    ("vs_main", naga::ShaderStage::Vertex),
                    ("fs_fill_open", naga::ShaderStage::Fragment),
                    ("fs_main_open", naga::ShaderStage::Fragment),
                    ("vs_glass", naga::ShaderStage::Vertex),
                    ("fs_glass", naga::ShaderStage::Fragment),
                    ("vs_cube", naga::ShaderStage::Vertex),
                    ("fs_cube", naga::ShaderStage::Fragment),
                ][..],
            ),
            ("blit.wgsl", BLIT_WGSL, &[("vs_main", naga::ShaderStage::Vertex), ("fs_main", naga::ShaderStage::Fragment)][..]),
            ("the sRGB blit", BLIT_SRGB_WGSL, &[("vs_main", naga::ShaderStage::Vertex), ("fs_main", naga::ShaderStage::Fragment)][..]),
        ] {
            let (module, info) = module(source);
            for version in [naga::back::glsl::Version::Desktop(330), naga::back::glsl::Version::Embedded { version: 300, is_webgl: false }]
            {
                for (entry, stage) in entries {
                    let options = naga::back::glsl::Options { version, ..Default::default() };
                    let pipeline =
                        naga::back::glsl::PipelineOptions { shader_stage: *stage, entry_point: entry.to_string(), multiview: None };
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
    fn the_uniform_is_laid_out_as_the_shader_reads_it() {
        let (module, _) = module(ANGEL_WGSL);
        let head = module.types.iter().find(|(_, t)| t.name.as_deref() == Some("Head")).expect("angel.wgsl has its Head");
        assert_eq!(head.1.inner.size(module.to_ctx()) as usize, std::mem::size_of::<Head>());
        assert_eq!(HEAD_ATTRS.iter().map(|a| a.format.size()).sum::<u64>(), body::STRIDE, "the vertex is read whole");
    }

    #[test]
    fn the_glass_heads_copies_are_angels_entries_with_the_cranium_left_out() {
        let source = glass_source();
        for (from, to) in [("fs_fill", "fs_fill_open"), ("fs_main", "fs_main_open")] {
            let copy = guarded(ANGEL_WGSL, from, to, "");
            let original = &ANGEL_WGSL[ANGEL_WGSL.find(&format!("fn {from}(")).unwrap()..];
            let body = |s: &str| s[s.find('{').unwrap()..s.find("\n}").unwrap()].to_string();
            assert_eq!(body(&copy[copy.find("fn ").unwrap()..]).replacen("{\n    ", "{", 1), body(original), "{to} is {from} as it is");
            assert!(source.contains(&format!("fn {to}(in: VsOut) -> @location(0) vec4<f32> {{\n    if (cranium_at(in.pos)) {{ discard; }}")));
        }
        let (module, _) = module(&source);
        let glass = module.types.iter().find(|(_, t)| t.name.as_deref() == Some("GlassU")).expect("the glass's uniform");
        assert_eq!(glass.1.inner.size(module.to_ctx()) as usize, std::mem::size_of::<GlassU>());
    }

    #[test]
    fn the_cube_is_closed_and_faces_out() {
        let cube = unit_cube();
        assert_eq!(cube.len(), 36);
        for tri in cube.chunks_exact(3) {
            let at = |i: usize| glam::Vec3::from_slice(&tri[i][..3]);
            let n = glam::Vec3::from_slice(&tri[0][3..]);
            // Counter-clockwise from outside: the winding's normal points the way the face does.
            assert!((at(1) - at(0)).cross(at(2) - at(0)).dot(n) > 0.0, "{tri:?}");
            assert!(tri.iter().all(|v| glam::Vec3::from_slice(&v[..3]).dot(n) == 1.0), "on its own face");
        }
        assert_eq!(std::mem::size_of::<Cube>() as u64, INSTANCE_ATTRS.iter().map(|a| a.format.size()).sum::<u64>());
    }

    #[test]
    fn the_target_is_the_widget_in_device_pixels() {
        let r = Rectangle { x: 10.0, y: 20.0, width: 333.4, height: 0.2 };
        assert_eq!(pixels(&r, 1.5), (500, 1));
        assert_eq!(pixels(&Rectangle { width: 0.0, height: 0.0, ..r }, 2.0), (1, 1));
    }
}
