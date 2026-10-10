//! Sinai's world: the valley it travels down, the sky over it and the sun where the sun really is.
//!
//! Ported from the Angel window's `main.rs` (egui throughout, so it cannot be linked): the valley's grid and its
//! constants, the sky's four palettes and the sun's position from the local clock and a latitude. The shaders that
//! draw them are the Angel window's, from the `sinai-face` crate (`terrain.wgsl`, `sky.wgsl`). On the card the whole
//! valley is one vertex buffer uploaded once and a uniform a frame; the sky is one triangle.
//!
//! The latitude is a setting (`ANGEL_LATITUDE`, 40 by default, as in the Angel window), never a lookup: asking the
//! network where somebody lives to draw a gradient is not a trade worth making quietly. `ANGEL_HOUR` pins the clock,
//! as there, so every sky can be photographed.

use std::time::{SystemTime, UNIX_EPOCH};

pub const TERRAIN_WGSL: &str = sinai_face::shaders::TERRAIN;
pub const SKY_WGSL: &str = sinai_face::shaders::SKY;

/// Columns either side of the road, and rows down the valley.
pub const NX: i32 = 128;
pub const NROWS: i32 = 193;
const ROAD_HALF: f32 = 2.45;
const DZ: f32 = 1.10;
const COL_DX: f32 = 0.42;
const SPEED: f32 = 5.4;
const THEME_HOLD_S: f32 = 24.0;
const THEME_FADE_S: f32 = 9.0;
const THEME_COUNT: f32 = 4.0;

/// One point of the valley's grid; the shader makes the ground from it.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pub grid: [f32; 2],
}

/// What `terrain.wgsl` reads.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Camera {
    pub view_proj: [[f32; 4]; 4],
    /// Travel, the themes' mix, theme a, theme b.
    pub motion: [f32; 4],
    pub params: [f32; 4],
    pub tint: [f32; 4],
    pub fade: [f32; 4],
}

/// What `sky.wgsl` reads.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SkyU {
    pub top: [f32; 4],
    /// rgb, and how much of the night is left.
    pub mid: [f32; 4],
    /// rgb, and the sun's elevation.
    pub hor: [f32; 4],
    /// The sun's x and y in clip space, the time, the cloud.
    pub sun: [f32; 4],
}

/// The valley's grid and its lines: every row across, every other column down.
pub fn build_terrain() -> (Vec<Vertex>, Vec<u32>) {
    let cols = (NX * 2 + 1) as usize;
    let rows = NROWS as usize;
    let mut verts = Vec::with_capacity(cols * rows);
    for r in 0..rows {
        for c in -NX..=NX {
            verts.push(Vertex { grid: [c as f32, r as f32] });
        }
    }
    let mut idx: Vec<u32> = Vec::with_capacity(cols * rows * 4);
    let at = |r: usize, c: usize| (r * cols + c) as u32;
    for r in 0..rows {
        for c in 0..cols - 1 {
            idx.push(at(r, c));
            idx.push(at(r, c + 1));
        }
    }
    for c in (0..cols).step_by(2) {
        for r in 0..rows - 1 {
            idx.push(at(r, c));
            idx.push(at(r + 1, c));
        }
    }
    (verts, idx)
}

/// The sun's elevation (-1 to 1) and hour angle, at `local` seconds since 1970 in local time and `latitude` degrees.
pub fn sun_at(local: f64, latitude: f32, pinned_hour: Option<f32>) -> (f32, f32) {
    let days = (local / 86400.0).floor();
    let doy = (days % 365.2425) as f32;
    let hours = pinned_hour.unwrap_or(((local - days * 86400.0) / 3600.0) as f32);
    let decl = 0.40928 * (2.0 * std::f32::consts::PI * (doy - 81.0) / 365.0).sin();
    let ha = (hours - 12.0) * 15.0 * std::f32::consts::PI / 180.0;
    let lat = latitude * std::f32::consts::PI / 180.0;
    let sin_e = lat.sin() * decl.sin() + lat.cos() * decl.cos() * ha.cos();
    let elev = sin_e.clamp(-1.0, 1.0).asin() / std::f32::consts::FRAC_PI_2;
    (elev, ha)
}

/// The sun now, on this machine's clock and offset, at the configured latitude.
pub fn sun_now() -> (f32, f32) {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0);
    let hour = std::env::var("ANGEL_HOUR").ok().and_then(|v| v.parse::<f32>().ok());
    sun_at(secs + f64::from(local_offset_seconds()), latitude(), hour)
}

/// `ANGEL_LATITUDE`, or 40 degrees.
pub fn latitude() -> f32 {
    std::env::var("ANGEL_LATITUDE").ok().and_then(|v| v.parse::<f32>().ok()).filter(|l| l.is_finite()).unwrap_or(40.0)
}

/// The machine's own offset from UTC now, in seconds, from what Windows already knows (as the Angel window reads it).
#[cfg(windows)]
#[allow(unsafe_code)]
fn local_offset_seconds() -> i32 {
    #[repr(C)]
    struct Tzi {
        bias: i32,
        standard_name: [u16; 32],
        standard_date: [u16; 8],
        standard_bias: i32,
        daylight_name: [u16; 32],
        daylight_date: [u16; 8],
        daylight_bias: i32,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetTimeZoneInformation(info: *mut Tzi) -> u32;
    }
    let mut z = Tzi {
        bias: 0,
        standard_name: [0; 32],
        standard_date: [0; 8],
        standard_bias: 0,
        daylight_name: [0; 32],
        daylight_date: [0; 8],
        daylight_bias: 0,
    };
    // SAFETY: the call writes one TIME_ZONE_INFORMATION into the struct it is given, laid out as Windows declares it.
    let r = unsafe { GetTimeZoneInformation(&mut z) };
    // The bias is in minutes WEST of UTC, plus whichever daylight bias is in force.
    let extra = match r {
        1 => z.standard_bias,
        2 => z.daylight_bias,
        _ => 0,
    };
    -(z.bias + extra) * 60
}

#[cfg(not(windows))]
fn local_offset_seconds() -> i32 {
    0
}

fn mix3(a: [f32; 3], b: [f32; 3], m: f32) -> [f32; 3] {
    [a[0] + (b[0] - a[0]) * m, a[1] + (b[1] - a[1]) * m, a[2] + (b[2] - a[2]) * m]
}

/// The sky's top, middle and horizon, and how much night is left, for an elevation in -1..1: muted by day, the drama
/// in the two twilights.
pub fn sky_for(e: f32) -> ([f32; 3], [f32; 3], [f32; 3], f32) {
    const NIGHT: [[f32; 3]; 3] = [[0.016, 0.016, 0.027], [0.027, 0.027, 0.043], [0.067, 0.051, 0.039]];
    const TWILIGHT: [[f32; 3]; 3] = [[0.031, 0.027, 0.059], [0.094, 0.063, 0.078], [0.282, 0.157, 0.094]];
    const GOLDEN: [[f32; 3]; 3] = [[0.059, 0.051, 0.067], [0.212, 0.137, 0.090], [0.596, 0.369, 0.149]];
    const DAY: [[f32; 3]; 3] = [[0.102, 0.110, 0.145], [0.204, 0.196, 0.192], [0.384, 0.337, 0.259]];
    if e < -0.20 {
        return (NIGHT[0], NIGHT[1], NIGHT[2], 1.0);
    }
    if e < 0.0 {
        let m = (e + 0.20) / 0.20;
        return (mix3(NIGHT[0], TWILIGHT[0], m), mix3(NIGHT[1], TWILIGHT[1], m), mix3(NIGHT[2], TWILIGHT[2], m), 1.0 - m * 0.75);
    }
    if e < 0.16 {
        let m = e / 0.16;
        return (mix3(TWILIGHT[0], GOLDEN[0], m), mix3(TWILIGHT[1], GOLDEN[1], m), mix3(TWILIGHT[2], GOLDEN[2], m), 0.25 * (1.0 - m));
    }
    let m = ((e - 0.16) / 0.42).min(1.0);
    (mix3(GOLDEN[0], DAY[0], m), mix3(GOLDEN[1], DAY[1], m), mix3(GOLDEN[2], DAY[2], m), 0.0)
}

/// Which of the valley's four themes is showing at `t`, the next, and how far the fade between them has gone.
pub fn theme_at(t: f32) -> (f32, f32, f32) {
    let span = THEME_HOLD_S + THEME_FADE_S;
    let p = t / span;
    let idx = p.floor();
    let into = (p - idx) * span;
    let a = idx.rem_euclid(THEME_COUNT);
    let b = (idx + 1.0).rem_euclid(THEME_COUNT);
    let m = if into <= THEME_HOLD_S { 0.0 } else { (into - THEME_HOLD_S) / THEME_FADE_S };
    (a, b, m * m * (3.0 - 2.0 * m))
}

/// The valley and the sky at `t`, seen through `proj` and `view`, under a sun at `elev` and hour angle `ha`.
pub fn scenery_uniforms(t: f32, proj: glam::Mat4, view: glam::Mat4, elev: f32, ha: f32, cloud: f32) -> (Camera, SkyU) {
    let (top, mid, hor, night) = sky_for(elev);
    // The camera looks level, so the horizon is the middle of the view; the sun rises from there with its elevation
    // and moves sideways with its hour angle.
    let sun_y = elev * 1.15;
    let sun_x = (ha.sin() * 0.85).clamp(-1.2, 1.2);
    let sky = SkyU {
        top: [top[0], top[1], top[2], 0.0],
        mid: [mid[0], mid[1], mid[2], night],
        hor: [hor[0], hor[1], hor[2], elev],
        sun: [sun_x, sun_y, t, cloud],
    };
    let (ta, tb, mix) = theme_at(t);
    let camera = Camera {
        view_proj: (proj * view).to_cols_array_2d(),
        motion: [t * SPEED, mix, ta, tb],
        params: [ROAD_HALF, DZ, COL_DX, NX as f32],
        tint: [0.851, 0.706, 0.357, 0.42],
        fade: [40.0, (NROWS as f32) * DZ, super::CAM_Y, 0.0],
    };
    (camera, sky)
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::wgpu::naga;

    #[test]
    fn the_valley_is_the_angel_windows_grid() {
        let (verts, idx) = build_terrain();
        assert_eq!(verts.len(), 257 * 193);
        assert_eq!(idx.len(), 2 * (193 * 256 + 129 * 192), "every row across and every other column down, as lines");
        assert!(idx.iter().all(|&i| (i as usize) < verts.len()));
        assert_eq!(verts[0].grid, [-128.0, 0.0]);
    }

    #[test]
    fn the_sun_is_up_at_noon_and_down_at_midnight_and_the_sky_follows_it() {
        // 2026-06-21 in local time, at latitude 40
        let midsummer = 20_625.0 * 86_400.0;
        let (noon, ha) = sun_at(midsummer, 40.0, Some(12.0));
        assert!(noon > 0.7 && ha == 0.0, "{noon}");
        let (midnight, _) = sun_at(midsummer, 40.0, Some(0.0));
        assert!(midnight < -0.2, "{midnight}");
        let (morning, ha) = sun_at(midsummer, 40.0, Some(6.0));
        assert!(ha < 0.0 && morning.abs() < 0.2, "near the horizon at six ({morning})");
        assert_eq!(sky_for(midnight).3, 1.0, "all night at midnight");
        assert_eq!(sky_for(noon).3, 0.0, "none at noon");
        // the palettes meet where they change hands: no jump at the horizon or the golden hour
        for e in [-0.20f32, 0.0, 0.16] {
            let (a, b) = (sky_for(e - 1e-4), sky_for(e + 1e-4));
            assert!((a.2[0] - b.2[0]).abs() < 0.01, "a jump at {e}");
        }
    }

    #[test]
    fn the_themes_hold_then_fade_into_the_next() {
        assert_eq!(theme_at(0.0), (0.0, 1.0, 0.0));
        assert_eq!(theme_at(20.0).2, 0.0, "held");
        let (a, b, m) = theme_at(24.0 + 4.5);
        assert_eq!((a, b), (0.0, 1.0));
        assert!((m - 0.5).abs() < 1e-5, "half way through the fade ({m})");
        assert_eq!(theme_at(4.0 * 33.0).0, 0.0, "four themes round");
    }

    #[test]
    fn the_scenery_uniforms_carry_the_sky_and_the_travel() {
        let (proj, view) = super::super::main_camera(1.6);
        let (camera, sky) = scenery_uniforms(10.0, proj, view, 0.3, 0.5, 0.0);
        assert_eq!(camera.motion[0], 54.0, "the valley moves past at its speed");
        assert_eq!(camera.params, [ROAD_HALF, DZ, COL_DX, 128.0]);
        assert_eq!(sky.hor[3], 0.3);
        assert!((sky.sun[1] - 0.345).abs() < 1e-6);
        assert_eq!(sky.sun[2], 10.0);
    }

    /// The uniforms are laid out as the shaders read them, and both shaders build for this wgpu and for OpenGL.
    #[test]
    fn the_world_shaders_build_and_read_the_uniforms_written() {
        for (name, source, uniform, size) in
            [("terrain.wgsl", TERRAIN_WGSL, "Camera", size_of::<Camera>()), ("sky.wgsl", SKY_WGSL, "Sky", size_of::<SkyU>())]
        {
            let module = naga::front::wgsl::parse_str(source).unwrap_or_else(|e| panic!("{name}: {e}"));
            let info = naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty())
                .validate(&module)
                .unwrap_or_else(|e| panic!("{name}: {e:?}"));
            let ty = module.types.iter().find(|(_, t)| t.name.as_deref() == Some(uniform)).expect("the uniform's struct");
            assert_eq!(ty.1.inner.size(module.to_ctx()) as usize, size, "{name}");
            for version in [naga::back::glsl::Version::Desktop(330), naga::back::glsl::Version::Embedded { version: 300, is_webgl: false }]
            {
                for (entry, stage) in [("vs_main", naga::ShaderStage::Vertex), ("fs_main", naga::ShaderStage::Fragment)] {
                    let options = naga::back::glsl::Options { version, ..Default::default() };
                    let pipeline = naga::back::glsl::PipelineOptions { shader_stage: stage, entry_point: entry.into(), multiview: None };
                    let mut out = String::new();
                    naga::back::glsl::Writer::new(
                        &mut out,
                        &module,
                        &info,
                        &options,
                        &pipeline,
                        naga::proc::BoundsCheckPolicies::default(),
                    )
                    .and_then(|mut w| w.write().map(|_| ()))
                    .unwrap_or_else(|e| panic!("{name} {entry} {version:?}: {e}"));
                }
            }
        }
    }
}
