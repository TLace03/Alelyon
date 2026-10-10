// The sign-in backdrop: an endless carbon-fibre plain under a rolling gold fog, lit from the upper left.
//
// The passes this file holds. `fs_fog` marches the fog at a quarter of the window's resolution into a texture of its
// own (in-scattered light, and how much of what lies behind still shows), at the scene time it is given. `fs_plain`
// draws what lies under the fog, which never moves, once for the window's size: the plain (a 2x2 twill of dark tows,
// each with a sheen along its fibres, under a clear coat) and the sky's glow. `fs_cached` draws the window from it:
// the fog over it (two marches, blended), tone-mapped, encoded and dithered. `fs_main` does the same working the plain
// out each frame, for a card that cannot keep it.
//
// Camera: a low eye over the plain looking a little down, rolled a touch so the horizon runs on a slant; the light
// sits low over the horizon towards the upper left, a long way off, so the fog nearest it glows and the foreground
// on the lower right falls to black.

struct SceneU {
    cam_pos   : vec4<f32>,   // the eye; tan of half the vertical field of view
    cam_right : vec4<f32>,   // the camera's right; width over height
    cam_up    : vec4<f32>,   // its up; time (s)
    cam_fwd   : vec4<f32>,   // its forward; how strongly the fluid shows (0 without one)
    light_pos : vec4<f32>,   // where the light is; its intensity
    light_col : vec4<f32>,   // its colour (linear); exposure
    view      : vec4<f32>,   // the target's width and height in device pixels; the fog texture's
    flags     : vec4<f32>,   // 1 to encode sRGB here; 1 when the fog is stored encoded (8-bit); grain seed; how much of the newer fog march shows
};

@group(0) @binding(0) var<uniform> s : SceneU;
// fs_fog: the fluid (xy velocity, z dye) and a spare. fs_main: the fog's older march, then its newer one.
@group(0) @binding(1) var tex_a : texture_2d<f32>;
@group(0) @binding(2) var tex_b : texture_2d<f32>;
@group(0) @binding(3) var samp : sampler;
// fs_cached: the plain, kept (`fs_plain` draws it).
@group(0) @binding(4) var tex_c : texture_2d<f32>;

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

// ---- noise

fn hash3(p: vec3<i32>) -> f32 {
    var h = (u32(p.x) * 73856093u) ^ (u32(p.y) * 19349663u) ^ (u32(p.z) * 83492791u);
    h = (h ^ (h >> 13u)) * 1274126177u;
    h = h ^ (h >> 16u);
    return f32(h) * (1.0 / 4294967295.0);
}

fn hash2(p: vec2<i32>) -> f32 {
    return hash3(vec3<i32>(p, 7));
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

fn fbm(p0: vec3<f32>, octaves: i32) -> f32 {
    var p = p0;
    var sum = 0.0;
    var amp = 0.5;
    var norm = 0.0;
    for (var o = 0; o < octaves; o = o + 1) {
        sum = sum + amp * vnoise(p);
        norm = norm + amp;
        // rotate between octaves so the lattice of the noise never lines up
        p = vec3<f32>(0.80 * p.x - 0.60 * p.z, p.y, 0.60 * p.x + 0.80 * p.z) * 2.02 + vec3<f32>(3.1, 1.7, 5.3);
        amp = amp * 0.5;
    }
    return sum / norm;
}

// ---- the camera

fn ray_dir(uv: vec2<f32>) -> vec3<f32> {
    let p = vec2<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0);
    let th = s.cam_pos.w;
    return normalize(s.cam_fwd.xyz + s.cam_right.xyz * (p.x * th * s.cam_right.w) + s.cam_up.xyz * (p.y * th));
}

fn time() -> f32 {
    return s.cam_up.w;
}

// How brightly the light reaches a point: a lamp a long way off, falling with distance, so the fog and the plain
// nearest it glow.
fn light_at(p: vec3<f32>) -> vec3<f32> {
    let d = s.light_pos.xyz - p;
    return s.light_col.rgb * s.light_pos.w / (1.0 + 0.06 * dot(d, d));
}

// Henyey-Greenstein: fog throws light forwards, so it glows hardest looking towards the light.
fn phase(cos_t: f32, g: f32) -> f32 {
    let gg = g * g;
    return (1.0 - gg) / (12.566 * pow(1.0 + gg - 2.0 * g * cos_t, 1.5));
}

// ---- the fog

const FOG_TOP : f32 = 1.6;      // the fog's ceiling (the eye is at 0.55)
const FOG_FAR : f32 = 70.0;     // how far the march goes
const FOG_STEPS : i32 = 16;
const STIR_GLOW : f32 = 0.10;   // how much more glow stirred mist catches
const AMBIENT : vec3<f32> = vec3<f32>(0.011, 0.0065, 0.0022);   // the glow of the fog all round, lighting every bank a little

// The rolling mist: drifting fbm, densest at the ground, thinning upwards, in banks that roll in from the light.
// How the fog thickens towards the ground: exp(-y / h), averaged over a stretch of the path that runs from height
// `ya` to `yb` (a long step far off, taken at one point, would catch the dense layer on some pixels and miss it on
// their neighbours, and the horizon would speckle).
fn layer(ya: f32, yb: f32, h: f32) -> f32 {
    let a = max(ya, 0.0);
    let b = max(yb, 0.0);
    if (abs(b - a) < 1e-3) {
        return exp(-0.5 * (a + b) / h);
    }
    return h * (exp(-a / h) - exp(-b / h)) / (b - a);
}

// The same over a stretch of the folded path (down to the plain at `t_g` and back up its reflection) from `ta` to `tb`.
fn layer_on_path(ta: f32, tb: f32, ya: f32, yb: f32, t_g: f32, h: f32) -> f32 {
    if (ta < t_g && tb > t_g) {
        let la = t_g - ta;
        let lb = tb - t_g;
        return (la * layer(ya, 0.0, h) + lb * layer(0.0, yb, h)) / max(la + lb, 1e-5);
    }
    return layer(ya, yb, h);
}

// The fog's heights for a stretch: the banks', the wisps' on the plain, and the stirred mist's.
struct Heights {
    banks : f32,
    wisps : f32,
    dye   : f32,
};

const BANK_H : f32 = 0.38;
const WISP_H : f32 = 0.06;
const DYE_H : f32 = 0.25;

// The rolling mist at `p`: drifting fbm in banks, densest at the ground and thinning upwards, rolling in from the
// light; and a thin mist lying on the plain itself, in wisps that drift a little faster (`wisps` fades it out far
// off, where, seen edge-on, it would stack into a dark wall along the horizon).
fn fog_density(p: vec3<f32>, stir: vec2<f32>, octaves: i32, wisps: f32, h: Heights, smear: f32) -> f32 {
    let t = time();
    let wind = vec3<f32>(0.11, 0.0, -0.16) * t;   // from the far upper left towards the eye
    let q = vec3<f32>(p.x * 0.42, p.y * 1.1, p.z * 0.30) + wind + vec3<f32>(stir.x, 0.0, stir.y);
    // a step that spans several of the noise's waves sees their mean, not whichever one its sample lands on
    let bank = mix(fbm(q, octaves), 0.5, smear);
    let banks = smoothstep(0.40, 0.74, bank + 0.10 * h.banks);
    var wisp = 0.0;
    if (wisps > 0.01) {
        wisp = smoothstep(0.42, 0.78, fbm(q * vec3<f32>(2.3, 1.0, 2.3) + vec3<f32>(0.0, 0.0, -0.05 * t), max(octaves - 1, 1)));
    }
    return (0.004 + 0.16 * banks) * h.banks + 0.0012 + 0.9 * wisps * wisp * h.wisps;
}

fn heights_at(y: f32) -> Heights {
    return Heights(layer(y, y, BANK_H), layer(y, y, WISP_H), layer(y, y, DYE_H));
}

// The fog along the path so far: the light it has scattered towards the eye, and how much it lets through.
struct Scatter {
    light : vec3<f32>,
    trans : f32,
};

fn march_step(acc: Scatter, p: vec3<f32>, h: Heights, along: vec3<f32>, dt: f32, t: f32, weight: f32, stir: vec2<f32>, dye: f32) -> Scatter {
    // far off a step spans more than the noise's finest detail: leave it out rather than alias it
    let octaves = select(select(2, 3, t < 10.0), 4, t < 3.5);
    let smear = smoothstep(0.6, 3.0, dt * 0.42);
    let base = fog_density(p, stir, octaves, exp(-t / 6.0), h, smear);
    let stirred = dye * 0.55 * h.dye;
    let d = base + stirred;
    let sigma = d * dt * 1.35;
    // a cheap shadow: how much fog lies a step towards the light (with a floor: light scattered more than once still
    // reaches the heart of a bank)
    let towards = normalize(s.light_pos.xyz - p);
    let probe = p + towards * 0.6;
    let shade = 0.4 + 0.6 * exp(-fog_density(probe, stir, 2, 0.0, heights_at(probe.y), smear) * 1.4);
    let ph = phase(dot(along, towards), 0.6) * 4.0 + 0.08;
    // mist the cursor has stirred up is lifted into the glow: it shows as brighter swirls, not only thicker ones
    let lift = s.light_col.rgb * STIR_GLOW * stirred / max(d, 1e-4);
    let lit = light_at(p) * (ph * shade + 0.03) + AMBIENT + lift;
    let absorbed = 1.0 - exp(-sigma);
    return Scatter(acc.light + acc.trans * absorbed * lit * weight, acc.trans * exp(-sigma));
}

@fragment
fn fs_fog(in: VsOut) -> @location(0) vec4<f32> {
    let uv = in.uv;
    let dir = ray_dir(uv);
    let eye = s.cam_pos.xyz;
    // the fluid at this point of the window: its dye thickens the mist, and its velocity shoves the mist's pattern
    // the way it flows (the window's right is the scene's -x, its down the scene's -z, and moving a noise's lookup
    // by +o moves its pattern by -o)
    let fluid = textureSampleLevel(tex_a, samp, uv, 0.0);
    let strength = s.cam_fwd.w;
    let stir = fluid.xy * 0.008 * strength;
    let dye = fluid.z * strength;

    // The path: to the fog's ceiling, or to the plain and on along its reflection in the clear coat (which mirrors the
    // fog above it, almost wholly at a grazing angle), so the fog runs on unbroken across the horizon.
    let ground = dir.y < -1e-4;
    let t_ground = select(1e9, -eye.y / min(dir.y, -1e-4), ground);
    var t_end = FOG_FAR;
    if (!ground && dir.y > 1e-4) {
        t_end = min(t_end, (FOG_TOP - eye.y) / dir.y);
    }
    let rdir = vec3<f32>(dir.x, -dir.y, dir.z);
    let mirror = 0.04 + 0.96 * pow(1.0 - clamp(-dir.y, 0.0, 1.0), 5.0);
    if (ground) {
        t_end = min(FOG_FAR, t_ground + min(FOG_FAR * 0.6, FOG_TOP / max(-dir.y, 1e-4)));
        t_end = max(t_end, t_ground + 0.05);
    }
    // steps spread exponentially (fine near the eye, coarse far off), offset per pixel so no rings show; each step
    // stands for the stretch since the last, sampled at its middle
    let jitter = fract(52.9829189 * fract(dot(in.pos.xy, vec2<f32>(0.06711056, 0.00583715))));
    let t0 = 0.08;
    let ratio = max(t_end / t0, 1.0001);
    var acc = Scatter(vec3<f32>(0.0), 1.0);
    var at_ground = 1.0;
    var prev = t0;
    var prev_y = eye.y + dir.y * t0;
    for (var i = 0; i < FOG_STEPS; i = i + 1) {
        let f = (f32(i) + jitter) / f32(FOG_STEPS);
        let t = t0 * pow(ratio, f);
        let dt = t - prev;
        let mid = 0.5 * (t + prev);
        var y = eye.y + dir.y * t;
        if (t >= t_ground) {
            y = -dir.y * (t - t_ground);
        }
        let h = Heights(
            layer_on_path(prev, t, prev_y, y, t_ground, BANK_H),
            layer_on_path(prev, t, prev_y, y, t_ground, WISP_H),
            layer_on_path(prev, t, prev_y, y, t_ground, DYE_H),
        );
        // what still shows of the plain: the light let through up to the step that reaches it
        if (prev < t_ground) {
            at_ground = acc.trans;
        }
        if (mid < t_ground) {
            acc = march_step(acc, eye + dir * mid, h, dir, dt, mid, 1.0, stir, dye);
        } else {
            let p = eye + dir * t_ground + rdir * (mid - t_ground);
            acc = march_step(acc, p, h, rdir, dt, mid, mirror, stir, dye);
        }
        prev = t;
        prev_y = y;
    }
    if (prev < t_ground) {
        at_ground = acc.trans;
    }
    let light = acc.light;
    let trans = select(acc.trans, at_ground, ground);
    if (s.flags.y > 0.5) {
        return vec4<f32>(sqrt(max(light, vec3<f32>(0.0))), trans);
    }
    return vec4<f32>(light, trans);
}

// ---- the plain

const TOW : f32 = 0.016;       // one tow's width, in the scene's units (the eye stands 0.55 above the plain)
const FILL : f32 = 0.030;      // the fog's glow overhead, lighting the plain everywhere

// The sky's light where a ray leaves the plain: dark, warmed into a gold glow about the lamp.
fn sky(dir: vec3<f32>) -> vec3<f32> {
    let toward = normalize(s.light_pos.xyz - s.cam_pos.xyz);
    let c = max(dot(dir, toward), 0.0);
    let glow = pow(c, 220.0) * 1.6 + pow(c, 24.0) * 0.30 + pow(c, 4.0) * 0.05;
    let base = vec3<f32>(0.0045, 0.0034, 0.0022) * (1.0 - 0.5 * clamp(dir.y * 3.0, 0.0, 1.0));
    return base + s.light_col.rgb * glow * s.light_pos.w * 0.03;
}

// GGX's distribution and Schlick's Fresnel, for the clear coat.
fn ggx(nh: f32, rough: f32) -> f32 {
    let a2 = rough * rough * rough * rough;
    let d = nh * nh * (a2 - 1.0) + 1.0;
    return a2 / (3.14159 * d * d);
}

fn fresnel(c: f32) -> f32 {
    return 0.04 + 0.96 * pow(1.0 - c, 5.0);
}

// The sheen along a tow: Kajiya-Kay, bright where the half vector lies across the fibres.
fn sheen(tangent: vec3<f32>, h: vec3<f32>, power: f32) -> f32 {
    let th = dot(tangent, h);
    return pow(sqrt(max(1.0 - th * th, 0.0)), power);
}

struct Weave {
    normal  : vec3<f32>,
    tangent : vec3<f32>,
    resin   : f32,    // 1 on a tow, darker in the resin between tows
    fibre   : f32,    // the fibres' streaks along the tow
};

// The 2x2 twill at a point of the plain (in tows): which tow is on top, its bevel across, its dip where it goes
// under at the ends of its float, and its fibres.
fn weave(c: vec2<f32>) -> Weave {
    let cell = vec2<i32>(floor(c));
    let f = fract(c);
    let k = (cell.x + cell.y) & 3;
    let warp = k < 2;
    var across = f.x;
    var along = f.y;
    var phase_along = f32(k) + f.y;
    var tangent = vec3<f32>(0.0, 0.0, 1.0);
    if (!warp) {
        across = f.y;
        along = f.x;
        phase_along = f32(k - 2) + f.x;
        tangent = vec3<f32>(1.0, 0.0, 0.0);
    }
    // the bevel across the tow: a rounded crown
    let x = across * 2.0 - 1.0;
    let slope_across = x * 0.55;
    // the dip at each end of the two-cell float
    let e = phase_along * 0.5;
    let slope_along = 0.5 * (1.0 - smoothstep(0.0, 0.16, e)) - 0.5 * (1.0 - smoothstep(0.0, 0.16, 1.0 - e));
    var n = vec3<f32>(0.0, 1.0, 0.0);
    if (warp) {
        n = normalize(vec3<f32>(-slope_across, 1.0, -slope_along));
    } else {
        n = normalize(vec3<f32>(-slope_along, 1.0, -slope_across));
    }
    let resin = smoothstep(0.0, 0.10, across) * smoothstep(1.0, 0.90, across);
    // the tow's own fibres: fine streaks running its whole length, different in every tow
    let tow_id = select(vec2<i32>(999, cell.y), vec2<i32>(cell.x, 0), warp);
    let run = select(c.x, c.y, warp);
    let fibre = 0.75 + 0.5 * vnoise(vec3<f32>(across * 38.0, run * 0.8, hash2(tow_id) * 50.0));
    return Weave(n, tangent, resin, fibre);
}

fn plain(hit: vec3<f32>, dir: vec3<f32>, detail: f32) -> vec3<f32> {
    let v = -dir;
    let l = normalize(s.light_pos.xyz - hit);
    let h = normalize(l + v);
    let radiance = light_at(hit);

    let w = weave(hit.xz / TOW);
    // far off, a pixel covers many tows: the weave averages out to a flat sheet with the mean of both sheens
    let n = normalize(mix(vec3<f32>(0.0, 1.0, 0.0), w.normal, detail));
    let nl = max(dot(n, l), 0.0);
    let s_here = sheen(w.tangent, h, 70.0) * w.fibre;
    let s_mean = 0.5 * (sheen(vec3<f32>(1.0, 0.0, 0.0), h, 70.0) + sheen(vec3<f32>(0.0, 0.0, 1.0), h, 70.0));
    let aniso = mix(s_mean, s_here, detail);
    let resin = mix(0.85, w.resin, detail);
    let base = vec3<f32>(0.0030, 0.0030, 0.0034) * resin;
    var colour = radiance * (base * nl + vec3<f32>(1.0, 0.92, 0.80) * aniso * 0.10 * resin * (0.3 + 0.7 * nl));
    // the fog's glow overhead lights the whole plain softly, so even the dark foreground shows its twill
    let fill_dir = normalize(vec3<f32>(l.x, 0.0, l.z) * 0.8 + vec3<f32>(0.0, 1.0, 0.0));
    let fh = normalize(fill_dir + v);
    let fill_here = sheen(w.tangent, fh, 24.0) * w.fibre;
    let fill_mean = 0.5 * (sheen(vec3<f32>(1.0, 0.0, 0.0), fh, 24.0) + sheen(vec3<f32>(0.0, 0.0, 1.0), fh, 24.0));
    let fill = s.light_col.rgb * FILL * (mix(fill_mean, fill_here, detail) * 0.8 + 0.05) * resin * max(dot(n, fill_dir), 0.0);
    colour = colour + fill;

    // the clear coat over it: a mirror with a little roughness, waved very gently, reflecting the glow and the lamp
    let wave = vec2<f32>(vnoise(vec3<f32>(hit.xz * 0.7, 1.0)), vnoise(vec3<f32>(hit.xz * 0.7, 9.0))) - vec2<f32>(0.5);
    let cn = normalize(vec3<f32>(wave.x * 0.05, 1.0, wave.y * 0.05));
    let nv = max(dot(cn, v), 1e-3);
    let fr = fresnel(nv);
    let r = reflect(dir, cn);
    colour = colour + fr * sky(r) * 0.8;
    let glint = ggx(max(dot(cn, h), 0.0), 0.32) * fr * max(dot(cn, l), 0.0) / (4.0 * nv);
    colour = colour + radiance * glint * 0.05;
    return colour;
}

// ---- the window

fn aces(x: vec3<f32>) -> vec3<f32> {
    return clamp((x * (2.51 * x + 0.03)) / (x * (2.43 * x + 0.59) + 0.14), vec3<f32>(0.0), vec3<f32>(1.0));
}

fn encode(c: vec3<f32>) -> vec3<f32> {
    let low = c * 12.92;
    let high = 1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(high, low, c <= vec3<f32>(0.0031308));
}

fn decode(c: vec3<f32>) -> vec3<f32> {
    let low = c / 12.92;
    let high = pow((c + 0.055) / 1.055, vec3<f32>(2.4));
    return select(high, low, c <= vec3<f32>(0.04045));
}


// What lies under the fog at a point of the window: the plain (its weave fading to its average by the pixel's
// footprint, and into the haze far off), or the sky's glow above the horizon. It does not move: `fs_plain` keeps it
// in a texture of the window's size, drawn once, and `fs_cached` reads it back; `fs_main` works it out every frame,
// where the card refuses that texture.
fn under_fog(uv: vec2<f32>) -> vec3<f32> {
    let dir = ray_dir(uv);
    let eye = s.cam_pos.xyz;
    // Where the ray meets the plain, worked out for every pixel so the footprint's derivatives are taken in
    // uniform control flow; above the horizon the point is meaningless and unused.
    let down = dir.y < -1e-4;
    let t_hit = select(1e4, -eye.y / min(dir.y, -1e-4), down);
    let hit = eye + dir * t_hit;
    let c = hit.xz / TOW;
    let footprint = max(length(dpdx(c)), length(dpdy(c)));
    let detail = 1.0 - smoothstep(0.10, 0.45, footprint);

    var colour = sky(dir);
    if (down) {
        colour = plain(hit, dir, detail);
        // past the fog's march, the far plain fades into the glow of the haze
        let haze = 1.0 - exp(-max(t_hit - FOG_FAR * 0.5, 0.0) * 0.03);
        colour = mix(colour, sky(normalize(vec3<f32>(dir.x, 0.0, dir.z))) * 0.9 + light_at(hit) * 0.004, haze);
    }
    return colour;
}

// The kept plain's encoding: a cube root of a quarter of it, so ten bits keep the dark weave's detail (a step is
// about 3% of the darkest tow's value) and a glint up to 4 still fits.
const KEPT_RANGE : f32 = 4.0;

fn keep(c: vec3<f32>) -> vec3<f32> {
    return pow(clamp(c / KEPT_RANGE, vec3<f32>(0.0), vec3<f32>(1.0)), vec3<f32>(1.0 / 3.0));
}

fn kept(k: vec3<f32>) -> vec3<f32> {
    return k * k * k * KEPT_RANGE;
}

// The fog's march in one of its textures at a point of the window, through a small tent filter over its quarter
// resolution (four bilinear taps half a fog texel off), decoded when it is kept in eight bits.
fn fog_newer(uv: vec2<f32>, ft: vec2<f32>) -> vec4<f32> {
    let f = 0.25 * (textureSampleLevel(tex_b, samp, uv + vec2<f32>(ft.x, ft.y), 0.0)
        + textureSampleLevel(tex_b, samp, uv + vec2<f32>(-ft.x, ft.y), 0.0)
        + textureSampleLevel(tex_b, samp, uv + vec2<f32>(ft.x, -ft.y), 0.0)
        + textureSampleLevel(tex_b, samp, uv + vec2<f32>(-ft.x, -ft.y), 0.0));
    return select(f, vec4<f32>(f.rgb * f.rgb, f.a), s.flags.y > 0.5);
}

fn fog_older(uv: vec2<f32>, ft: vec2<f32>) -> vec4<f32> {
    let f = 0.25 * (textureSampleLevel(tex_a, samp, uv + vec2<f32>(ft.x, ft.y), 0.0)
        + textureSampleLevel(tex_a, samp, uv + vec2<f32>(-ft.x, ft.y), 0.0)
        + textureSampleLevel(tex_a, samp, uv + vec2<f32>(ft.x, -ft.y), 0.0)
        + textureSampleLevel(tex_a, samp, uv + vec2<f32>(-ft.x, -ft.y), 0.0));
    return select(f, vec4<f32>(f.rgb * f.rgb, f.a), s.flags.y > 0.5);
}

// The window's pixel from what lies under the fog: the fog over it (the newer march, blended from the older by the
// frame's place between them), the lamp's bloom, the vignette, tone-mapped, encoded and dithered.
fn finish(under: vec3<f32>, uv: vec2<f32>, pos: vec2<f32>) -> vec4<f32> {
    let dir = ray_dir(uv);
    let eye = s.cam_pos.xyz;
    let ft = 0.5 / s.view.zw;
    var fog = fog_newer(uv, ft);
    if (s.flags.w < 0.999) {
        fog = mix(fog_older(uv, ft), fog, s.flags.w);
    }
    var colour = under * fog.a + fog.rgb;

    // the lamp's bloom, low over the horizon
    let toward = normalize(s.light_pos.xyz - eye);
    let cl = max(dot(dir, toward), 0.0);
    colour = colour + s.light_col.rgb * (pow(cl, 900.0) * 0.5 + pow(cl, 60.0) * 0.035) * s.light_pos.w * 0.05;

    // darker at the edges and in the lower right, where the plain runs into the dark
    let vig = smoothstep(1.45, 0.25, length((uv - vec2<f32>(0.42, 0.30)) * vec2<f32>(1.0, 1.25)));
    colour = colour * (0.25 + 0.75 * vig);

    var out = aces(colour * s.light_col.w);
    // a whisper of grain, in the encoded values, so the dark gradient never bands
    let g = hash3(vec3<i32>(vec2<i32>(pos), i32(s.flags.z))) + hash3(vec3<i32>(vec2<i32>(pos), i32(s.flags.z) + 1)) - 1.0;
    out = clamp(encode(out) + vec3<f32>(g * 1.4 / 255.0), vec3<f32>(0.0), vec3<f32>(1.0));
    if (s.flags.x < 0.5) {
        // the window encodes sRGB itself: hand it linear values
        out = decode(out);
    }
    return vec4<f32>(out, 1.0);
}

// The window, everything worked out this frame.
@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    return finish(under_fog(in.uv), in.uv, in.pos.xy);
}

// What lies under the fog, kept: drawn once for the window's size.
@fragment
fn fs_plain(in: VsOut) -> @location(0) vec4<f32> {
    return vec4<f32>(keep(under_fog(in.uv)), 1.0);
}

// The window from the kept plain (`tex_c`, one texel a pixel).
@fragment
fn fs_cached(in: VsOut) -> @location(0) vec4<f32> {
    let size = vec2<i32>(textureDimensions(tex_c));
    let at = clamp(vec2<i32>(in.uv * vec2<f32>(size)), vec2<i32>(0), size - vec2<i32>(1));
    return finish(kept(textureLoad(tex_c, at, 0).rgb), in.uv, in.pos.xy);
}
