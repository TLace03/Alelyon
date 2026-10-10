// The gold fog's fluid: Stable Fluids (Stam 1999) at a low resolution, in render passes only (no compute shaders:
// the OpenGL backend CENTCOM prefers may have none). One fragment entry per stage; every stage reads at most two
// textures and writes one, and the card steps through them in order (render.rs, `Made::step`).
//
// The field lives in screen space over the widget, y down. Velocity is in cells per second; `a`'s xy is the
// velocity, z the dye (how much gold mist the stirring has added), w unused. Pressure and the scratch field
// (curl, then divergence) use x only.

struct SimU {
    texel   : vec4<f32>,   // 1/width, 1/height, width, height (cells)
    step    : vec4<f32>,   // dt (s), time (s), velocity dissipation (1/s), dye dissipation (1/s)
    stroke  : vec4<f32>,   // the cursor's path this frame: from.xy, to.xy (uv)
    force   : vec4<f32>,   // the cursor's velocity (uv/s) xy, the dye it lays down, its radius (in heights)
    ambient : vec4<f32>,   // the rolling force, vorticity confinement, the ambient dye, width over height
};

@group(0) @binding(0) var<uniform> u : SimU;
@group(0) @binding(1) var a : texture_2d<f32>;
@group(0) @binding(2) var b : texture_2d<f32>;
@group(0) @binding(3) var samp : sampler;

struct VsOut {
    @builtin(position) pos : vec4<f32>,
    @location(0) uv        : vec2<f32>,
};

// One triangle over the target; uv runs 0..1 over it, y down.
@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> VsOut {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var out : VsOut;
    out.pos = vec4<f32>(uv * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0), 0.0, 1.0);
    out.uv = uv;
    return out;
}

fn cells() -> vec2<i32> {
    return vec2<i32>(u.texel.zw);
}

fn ld_a(ij: vec2<i32>) -> vec4<f32> {
    return textureLoad(a, clamp(ij, vec2<i32>(0), cells() - vec2<i32>(1)), 0);
}

fn ld_b(ij: vec2<i32>) -> vec4<f32> {
    return textureLoad(b, clamp(ij, vec2<i32>(0), cells() - vec2<i32>(1)), 0);
}

fn inside(ij: vec2<i32>) -> bool {
    return all(ij >= vec2<i32>(0)) && all(ij < cells());
}

// The velocity at a neighbour, mirrored at the walls so no fog flows out through the window's edge.
fn wall_v(ij: vec2<i32>, here: vec2<f32>) -> vec2<f32> {
    if (inside(ij)) {
        return ld_a(ij).xy;
    }
    return -here;
}

// ---- noise for the rolling force and the ambient mist

fn hash3(p: vec3<i32>) -> f32 {
    var h = (u32(p.x) * 73856093u) ^ (u32(p.y) * 19349663u) ^ (u32(p.z) * 83492791u);
    h = (h ^ (h >> 13u)) * 1274126177u;
    h = h ^ (h >> 16u);
    return f32(h) * (1.0 / 4294967295.0);
}

fn vnoise(p: vec3<f32>) -> f32 {
    let i = vec3<i32>(floor(p));
    let f = fract(p);
    let w = f * f * (3.0 - 2.0 * f);
    let x00 = mix(hash3(i), hash3(i + vec3<i32>(1, 0, 0)), w.x);
    let x10 = mix(hash3(i + vec3<i32>(0, 1, 0)), hash3(i + vec3<i32>(1, 1, 0)), w.x);
    let x01 = mix(hash3(i + vec3<i32>(0, 0, 1)), hash3(i + vec3<i32>(1, 0, 1)), w.x);
    let x11 = mix(hash3(i + vec3<i32>(0, 1, 1)), hash3(i + vec3<i32>(1, 1, 1)), w.x);
    return mix(mix(x00, x10, w.y), mix(x01, x11, w.y), w.z);
}

fn fbm2(p: vec3<f32>) -> f32 {
    return 0.65 * vnoise(p) + 0.35 * vnoise(p * 2.03 + vec3<f32>(17.1, 3.7, 9.2));
}

// ---- the stages

// The curl of the velocity, for vorticity confinement.
@fragment
fn fs_curl(in: VsOut) -> @location(0) vec4<f32> {
    let ij = vec2<i32>(in.pos.xy);
    let l = ld_a(ij - vec2<i32>(1, 0)).xy;
    let r = ld_a(ij + vec2<i32>(1, 0)).xy;
    let up = ld_a(ij - vec2<i32>(0, 1)).xy;
    let dn = ld_a(ij + vec2<i32>(0, 1)).xy;
    let curl = 0.5 * ((r.y - l.y) - (dn.x - up.x));
    return vec4<f32>(curl, 0.0, 0.0, 1.0);
}

// The forces: vorticity confinement, a slow divergence-free rolling (the curl of a drifting noise), a breath of
// drift from the light towards the viewer's right, the cursor's stroke, and the mist that wells up near the light.
@fragment
fn fs_force(in: VsOut) -> @location(0) vec4<f32> {
    let ij = vec2<i32>(in.pos.xy);
    let uv = (vec2<f32>(ij) + vec2<f32>(0.5)) * u.texel.xy;
    let here = ld_a(ij);
    var v = here.xy;
    var dye = here.z;
    let dt = u.step.x;
    let t = u.step.y;
    let aspect = u.ambient.w;

    // vorticity confinement: give the small swirls back the spin the coarse grid smears away
    let cl = abs(ld_b(ij - vec2<i32>(1, 0)).x);
    let cr = abs(ld_b(ij + vec2<i32>(1, 0)).x);
    let cu = abs(ld_b(ij - vec2<i32>(0, 1)).x);
    let cd = abs(ld_b(ij + vec2<i32>(0, 1)).x);
    let c = ld_b(ij).x;
    var n = 0.5 * vec2<f32>(cr - cl, cd - cu);
    n = n / (length(n) + 1e-4);
    v = v + dt * u.ambient.y * c * vec2<f32>(n.y, -n.x);

    // the rolling: the curl of a slowly drifting stream function, so it adds no divergence
    let q = vec3<f32>(uv.x * aspect * 2.2, uv.y * 2.2, t * 0.045);
    let e = 0.02;
    let ndx = fbm2(q + vec3<f32>(e, 0.0, 0.0)) - fbm2(q - vec3<f32>(e, 0.0, 0.0));
    let ndy = fbm2(q + vec3<f32>(0.0, e, 0.0)) - fbm2(q - vec3<f32>(0.0, e, 0.0));
    let roll = vec2<f32>(ndy, -ndx) / (2.0 * e);
    v = v + dt * u.ambient.x * (roll * 6.0 + vec2<f32>(1.2, 0.35));

    // the cursor: a soft brush along its path this frame
    let p0 = u.stroke.xy;
    let p1 = u.stroke.zw;
    let k = vec2<f32>(aspect, 1.0);
    let seg = (p1 - p0) * k;
    let rel = (uv - p0) * k;
    let h = clamp(dot(rel, seg) / max(dot(seg, seg), 1e-8), 0.0, 1.0);
    let d = rel - seg * h;
    let radius = max(u.force.w, 1e-4);
    let g = exp(-dot(d, d) / (radius * radius));
    v = v + g * u.force.xy * u.texel.zw;
    dye = dye + g * u.force.z;

    // mist welling up towards the light (upper left), in slow patches
    let well = smoothstep(0.5, 0.82, fbm2(vec3<f32>(uv.x * aspect * 3.0 - t * 0.05, uv.y * 3.0 + t * 0.02, t * 0.03)));
    let near_light = 1.0 - smoothstep(0.15, 0.95, length((uv - vec2<f32>(0.32, 0.2)) * k) / aspect);
    dye = dye + dt * u.ambient.z * well * near_light;

    return vec4<f32>(v, min(dye, 4.0), 1.0);
}

// The divergence of the velocity, with the walls closed.
@fragment
fn fs_divergence(in: VsOut) -> @location(0) vec4<f32> {
    let ij = vec2<i32>(in.pos.xy);
    let here = ld_a(ij).xy;
    let l = wall_v(ij - vec2<i32>(1, 0), here);
    let r = wall_v(ij + vec2<i32>(1, 0), here);
    let up = wall_v(ij - vec2<i32>(0, 1), here);
    let dn = wall_v(ij + vec2<i32>(0, 1), here);
    let div = 0.5 * ((r.x - l.x) + (dn.y - up.y));
    return vec4<f32>(div, 0.0, 0.0, 1.0);
}

// One Jacobi iteration of the pressure's Poisson equation: `a` the pressure so far, `b` the divergence.
@fragment
fn fs_jacobi(in: VsOut) -> @location(0) vec4<f32> {
    let ij = vec2<i32>(in.pos.xy);
    let l = ld_a(ij - vec2<i32>(1, 0)).x;
    let r = ld_a(ij + vec2<i32>(1, 0)).x;
    let up = ld_a(ij - vec2<i32>(0, 1)).x;
    let dn = ld_a(ij + vec2<i32>(0, 1)).x;
    let div = ld_b(ij).x;
    return vec4<f32>((l + r + up + dn - div) * 0.25, 0.0, 0.0, 1.0);
}

// Subtract the pressure's gradient (`b`) from the velocity (`a`): what is left flows without compressing.
@fragment
fn fs_gradient(in: VsOut) -> @location(0) vec4<f32> {
    let ij = vec2<i32>(in.pos.xy);
    let here = ld_a(ij);
    let l = ld_b(ij - vec2<i32>(1, 0)).x;
    let r = ld_b(ij + vec2<i32>(1, 0)).x;
    let up = ld_b(ij - vec2<i32>(0, 1)).x;
    let dn = ld_b(ij + vec2<i32>(0, 1)).x;
    let v = here.xy - 0.5 * vec2<f32>(r - l, dn - up);
    return vec4<f32>(v, here.z, 1.0);
}

// Semi-Lagrangian advection of the velocity and the dye by the velocity, both fading gently.
@fragment
fn fs_advect(in: VsOut) -> @location(0) vec4<f32> {
    let ij = vec2<i32>(in.pos.xy);
    let uv = (vec2<f32>(ij) + vec2<f32>(0.5)) * u.texel.xy;
    let dt = u.step.x;
    let v = ld_a(ij).xy;
    let back = uv - dt * v * u.texel.xy;
    let s = textureSampleLevel(a, samp, back, 0.0);
    let vel = s.xy / (1.0 + dt * u.step.z);
    let dye = s.z / (1.0 + dt * u.step.w);
    return vec4<f32>(vel, dye, 1.0);
}
