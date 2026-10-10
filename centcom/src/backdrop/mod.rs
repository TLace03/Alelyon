//! The sign-in screen's backdrop: an endless carbon-fibre plain with a gold fog rolling over it, which the cursor stirs.
//!
//! It replaces the old diagonal gradient behind the sign-in screen with a dynamic shader: an infinite carbon-fibre
//! plain with a rolling gold fog or mist, where the cursor affects the background through a realistic fluid sim. The
//! gradient's diagonal is kept: the light glows from the upper left and the plain falls into darkness towards the
//! lower right.
//!
//! What is drawn (`scene.wgsl`, `sim.wgsl`, put on the card by `render.rs`):
//!
//! * the plain, seen from a low eye: a 2x2 twill of dark tows, each with a sheen along its fibres (so the checker of
//!   sheen flips between warp and weft), a bevel across it, a dip where it goes under, and a clear coat over all of
//!   it that mirrors the glow. Far off, where a pixel covers several tows, the weave fades to its average by the
//!   pixel's footprint, so it never shimmers into moire, and the haze takes over towards the horizon;
//! * the fog: drifting fractal noise, densest at the ground, lit gold by a lamp low over the horizon on the upper
//!   left, marched at a quarter of the window's resolution;
//! * the fluid: Stable Fluids (advection, vorticity confinement, a Jacobi pressure solve, the gradient taken off) at
//!   about an eighth of the window's resolution, in render passes only. A slow curl-noise force keeps it rolling; the
//!   cursor pushes it and lays a little gold along its path. The fluid's dye thickens the fog and its velocity shoves
//!   the fog's pattern about, so stirring moves the mist.
//!
//! The clock (it uses only as much of the card as it needs). It builds on Sinai's
//! face's (`face::pace`: frames only once the card has drawn one, only while the backdrop is in the widget tree, which
//! is while the sign-in screen shows; after signing in nothing of it runs) and decides how often to draw, by
//! [`rate`]:
//!
//! * **stirred** ([`Rate::Full`]): sixty frames a second while the cursor moved in the last [`SETTLE`], or while the
//!   swirl it left (an estimate kept here, fading as the fluid's velocity fades) is still fast;
//! * **calm** ([`Rate::Calm`]): twenty frames a second otherwise: the fog rolls slowly;
//! * **paused** ([`Rate::Paused`]): no frames at all while the window is not the focused one (minimised included: a
//!   minimised window loses the focus) and nothing stirs it, and in still mode. Focus coming back, or a cursor moving
//!   over the window, starts the clock again. iced 0.14 does not report a window covered by others, so a focused
//!   window hidden behind another keeps its calm rate.
//!
//! The scene's time runs with the clock (never more than a quarter of a second a frame), so a pause leaves the fog
//! where it was. The fog is marched less often than frames are shown: ahead of time, a thirtieth of a second ahead when
//! stirred and a fifth when calm, and each frame shows the two latest marches blended by where it falls between
//! them ([`FogPlan`]); the fluid is stepped up to each march (a sixtieth of a second a step when stirred, a tenth when
//! calm). A frame drawn again for the form's sake, at the same time and size, is the one already prepared: nothing is
//! stepped or marched again.
//!
//! Motion on or off (a static shader for people who do not want the dynamic background, and a small button to
//! toggle it): with motion off the backdrop draws one frame of the same scene and asks
//! for no more, until the window is resized (which redraws that frame at the new size) or motion is switched back on.
//! The choice is the sign-in screen's Motion button or Settings' Appearance line, kept in `preferences.json`
//! (`crate::prefs`); with nothing kept, Windows' "Show animations in Windows" decides ([`system`]).
//! `CENTCOM_STILL_BACKDROP=1` (or any value but `0`, `false`, `off` or empty) forces motion off, whatever is kept.
//!
//! Under the software renderer (`ICED_BACKEND=tiny-skia`) the shader widget draws nothing, so the clock stops after one
//! look, and the old diagonal gradient, which lies under the shader, is what shows: the window never needs a graphics
//! card. A card that refuses a pipeline leaves the gradient showing too; a card that cannot render to half-float
//! textures gets the fog without the fluid.
//!
//! Two debug aids: `CENTCOM_BACKDROP_TIMING=1` waits on the card after each piece of work and prints, once a second,
//! the frames shown, the fluid's steps and the fog's marches and what they took on the card (to stderr), and the
//! window's pass; `CENTCOM_BACKDROP_DEMO=1` stirs the fog with a cursor of its own tracing a figure of eight over the
//! right of the window, so a `--screenshot` run, which has no hand on the mouse, shows the fluid at work.

pub mod render;
// Reads Windows' "Show animations in Windows" (SystemParametersInfoW), so it may use unsafe code.
#[allow(unsafe_code)]
pub mod system;

use std::sync::Arc;
use std::time::{Duration, Instant};

use iced::widget::{container, shader, stack};
use iced::{Background, Color, Element, Length, Point, Rectangle, Vector, mouse, window};

use crate::face::{self, Card, Pace};

/// The switch that forces motion off.
pub const STILL_VAR: &str = "CENTCOM_STILL_BACKDROP";

/// The switch for the debug aid that times the card's work.
pub const TIMING_VAR: &str = "CENTCOM_BACKDROP_TIMING";

/// The switch for the debug aid that stirs the fog by itself.
pub const DEMO_VAR: &str = "CENTCOM_BACKDROP_DEMO";

/// Where the demonstration's cursor is `t` seconds in, over a widget at `bounds`: a figure of eight over its right.
pub fn demo_cursor(t: f32, bounds: Rectangle) -> Point {
    Point::new(bounds.x + bounds.width * (0.66 + 0.2 * (t * 1.1).sin()), bounds.y + bounds.height * (0.62 + 0.16 * (t * 2.2).sin()))
}

/// Where the scene's clock starts: the fog has been rolling a while already, so the first frame is mid-roll.
pub const T0: f32 = 37.0;

/// Whether a switch's value turns it on: anything but nothing, `0`, `false` or `off`.
pub fn switched_on(value: Option<&str>) -> bool {
    match value.map(str::trim) {
        None | Some("") | Some("0") => false,
        Some(v) => !v.eq_ignore_ascii_case("false") && !v.eq_ignore_ascii_case("off"),
    }
}

/// Whether this process was told to keep the backdrop still, whatever the person chose.
pub fn forced_still() -> bool {
    switched_on(std::env::var(STILL_VAR).ok().as_deref())
}

/// Whether the backdrop moves at start: never when forced still; else as the person last chose; else as Windows'
/// "Show animations" says; else it moves.
pub fn motion_at_start(forced_still: bool, kept: Option<bool>, windows_animates: Option<bool>) -> bool {
    !forced_still && kept.or(windows_animates).unwrap_or(true)
}

/// The person's choice of motion as the window holds it: on or off, whether `CENTCOM_STILL_BACKDROP` forces it off,
/// the preferences file it is kept in, and why it could not be kept, when it could not.
#[derive(Clone, Debug, PartialEq)]
pub struct Motion {
    pub on: bool,
    pub forced_still: bool,
    pub file: Option<std::path::PathBuf>,
    pub note: Option<String>,
}

impl Motion {
    /// The choice at start, from the environment, the preferences file and Windows (asked only when nothing is kept).
    pub fn at_start() -> Motion {
        let file = crate::prefs::path();
        let kept = file.as_deref().map(crate::prefs::load).unwrap_or_default().motion;
        let forced_still = forced_still();
        let windows = if kept.is_none() && !forced_still { system::animations_on() } else { None };
        Motion { on: motion_at_start(forced_still, kept, windows), forced_still, file, note: None }
    }

    /// Switch motion on or off and keep the choice; forced still, nothing changes.
    pub fn set(&mut self, on: bool) {
        if self.forced_still {
            return;
        }
        self.on = on;
        self.note = match &self.file {
            Some(file) => crate::prefs::save(file, &crate::prefs::Prefs { motion: Some(on) }).err(),
            None => Some("USERPROFILE is not set, so the choice lasts until Alelyon closes".into()),
        };
    }
}

// ------------------------------------------------------------------- how often

/// How often the backdrop draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rate {
    /// Stirred, or still settling: sixty frames a second, the fog marched thirty times a second.
    Full,
    /// Nothing stirs it: twenty frames a second, the fog marched five times a second.
    Calm,
    /// Unfocused, or still: no frames.
    Paused,
}

/// A frame every sixtieth of a second, stirred.
pub const FULL: Duration = face::FRAME;
/// A frame every twentieth of a second, calm.
pub const CALM: Duration = Duration::from_millis(50);
/// How long after the cursor last moved the backdrop stays at its full rate whatever the swirl.
pub const SETTLE: Duration = Duration::from_millis(1500);
/// The swirl (widths a second) below which the fluid counts as settled.
pub const SETTLED: f32 = 0.15;
/// The most the scene's time moves in one frame: after a pause it goes on from where it stopped.
pub const MAX_ADVANCE: f32 = 0.25;
/// A frame due this close is drawn now rather than asked for again (the system's timers are coarse).
pub const SLACK: Duration = Duration::from_millis(4);

impl Rate {
    /// The time between frames; none when paused.
    pub fn every(self) -> Option<Duration> {
        match self {
            Rate::Full => Some(FULL),
            Rate::Calm => Some(CALM),
            Rate::Paused => None,
        }
    }

    /// The scene time between two marches of the fog.
    pub fn fog_every(self) -> f32 {
        match self {
            Rate::Full => 1.0 / 30.0,
            Rate::Calm | Rate::Paused => 0.2,
        }
    }
}

/// The policy: full while stirred or settling (`since_moved`: how long since the cursor last moved over the window;
/// `swirl`: the estimate of how fast the stirring still flows, widths a second), calm while focused, else paused;
/// a still backdrop is always paused (its one frame is drawn by the first look, see [`Program`]).
pub fn rate(focused: bool, still: bool, since_moved: Option<Duration>, swirl: f32) -> Rate {
    if still {
        Rate::Paused
    } else if since_moved.is_some_and(|d| d < SETTLE) || swirl > SETTLED {
        Rate::Full
    } else if focused {
        Rate::Calm
    } else {
        Rate::Paused
    }
}

// ------------------------------------------------------------------- the cursor

/// The cursor's stroke over one frame, in the widget's own terms: where it went from and to (0..1 across and down)
/// and how fast (widths and heights a second).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Splat {
    pub from: [f32; 2],
    pub to: [f32; 2],
    pub velocity: [f32; 2],
}

impl Splat {
    pub fn speed(&self) -> f32 {
        (self.velocity[0] * self.velocity[0] + self.velocity[1] * self.velocity[1]).sqrt()
    }
}

/// The fastest a stroke is taken to move, in widths a second: a flick across the window stirs hard but never
/// blows the fluid up.
pub const MAX_SPEED: f32 = 6.0;

/// The cursor as the backdrop follows it: where it was last seen, and the path it has taken since the last frame.
/// It is told every move the window hears, over the form and its buttons too, because a move carries its own
/// position (the stack above may hide the cursor from the widgets under it, but not the event).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Stir {
    last: Option<Point>,
    /// The stroke since the last frame: where it began and where it is now.
    stroke: Option<(Point, Point)>,
}

impl Stir {
    /// The cursor moved to `p` (window coordinates).
    pub fn moved_to(&mut self, p: Point) {
        if let Some(last) = self.last {
            let from = self.stroke.map_or(last, |s| s.0);
            self.stroke = Some((from, p));
        }
        self.last = Some(p);
    }

    /// The cursor left the window: the next move it makes starts afresh, not a stroke from where it went out.
    pub fn left(&mut self) {
        self.last = None;
    }

    /// The stroke since the last frame, `dt` seconds ago, over a widget at `bounds`; none if the cursor did not move.
    pub fn take(&mut self, dt: f32, bounds: Rectangle) -> Option<Splat> {
        let (from, to) = self.stroke.take()?;
        let (w, h) = (bounds.width.max(1.0), bounds.height.max(1.0));
        let at = |p: Point| [(p.x - bounds.x) / w, (p.y - bounds.y) / h];
        let moved: Vector = to - from;
        let dt = dt.max(1.0 / 240.0);
        let mut velocity = [moved.x / w / dt, moved.y / h / dt];
        let speed = (velocity[0] * velocity[0] + velocity[1] * velocity[1]).sqrt();
        if speed > MAX_SPEED {
            velocity = [velocity[0] * MAX_SPEED / speed, velocity[1] * MAX_SPEED / speed];
        }
        (speed > 0.0).then_some(Splat { from: at(from), to: at(to), velocity })
    }
}

// ------------------------------------------------------------------- the frame

/// Which marches of the fog a frame shows: the older and the newer (scene times; the same when there is one), and
/// how much of the newer (0..1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FogPlan {
    pub older: f32,
    pub newer: f32,
    pub blend: f32,
}

impl FogPlan {
    /// One march, shown alone.
    pub fn at(t: f32) -> FogPlan {
        FogPlan { older: t, newer: t, blend: 1.0 }
    }

    /// The plan for a frame at `t` after `last` (none before the first), marching every `every` seconds of scene time:
    /// the same marches while `t` lies between them; once `t` passes the newer, a new one `every` ahead of it; and
    /// after a gap longer than that (a pause, a change of rate), a fresh pair from `t`.
    pub fn next(last: Option<FogPlan>, t: f32, every: f32) -> FogPlan {
        let (older, newer) = match last {
            None => return FogPlan::at(t),
            Some(p) if t <= p.newer => (p.older, p.newer),
            Some(p) if t - p.newer < every => (p.newer, p.newer + every),
            Some(_) => (t, t + every),
        };
        let blend = if newer > older { ((t - older) / (newer - older)).clamp(0.0, 1.0) } else { 1.0 };
        FogPlan { older, newer, blend }
    }
}

/// One frame for the card: the scene's time, the fog's marches, the stroke to stir in when the newer march is
/// made, and whether the backdrop stands still.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub t: f32,
    pub fog: FogPlan,
    pub splat: Option<Splat>,
    /// Stirred (the full rate): the fluid steps a sixtieth of a second at a time, as it always did.
    pub stirred: bool,
    pub still: bool,
}

impl Frame {
    pub fn first() -> Frame {
        Frame { t: T0, fog: FogPlan::at(T0), splat: None, stirred: false, still: false }
    }
}

/// The shader widget's memory while the backdrop is on show.
#[derive(Debug)]
pub struct Clock {
    looked: bool,
    /// The scene's time, and when the clock last moved it.
    t: f32,
    last: Option<Instant>,
    /// When the next frame is due; a redraw before then (the form's) shows the frame already made.
    due: Option<Instant>,
    focused: bool,
    /// When the cursor last moved over the window, and how fast the swirl it left still flows (widths a second).
    moved_at: Option<Instant>,
    swirl: f32,
    /// When the stroke was last taken.
    taken: Option<Instant>,
    planned: bool,
    frame: Frame,
    stir: Stir,
    card: Arc<Card>,
}

impl Default for Clock {
    fn default() -> Clock {
        Clock {
            looked: false,
            t: T0,
            last: None,
            due: None,
            focused: true,
            moved_at: None,
            swirl: 0.0,
            taken: None,
            planned: false,
            frame: Frame::first(),
            stir: Stir::default(),
            card: Arc::new(Card::default()),
        }
    }
}

impl Clock {
    /// The rate at `now`.
    pub fn rate(&self, now: Instant, still: bool) -> Rate {
        rate(self.focused, still, self.moved_at.map(|m| now.saturating_duration_since(m)), self.swirl)
    }

    /// Move the clock to `now` at `rate`: the scene's time, the fog's plan, and the stroke when a new march is due.
    /// A still backdrop keeps the frame it has, marked still (the first, before any tick: the scene at [`T0`]).
    pub fn tick(&mut self, now: Instant, bounds: Rectangle, rate: Rate, still: bool, demo: bool) {
        if still {
            self.frame.still = true;
            self.frame.splat = None;
            self.last = None;
            return;
        }
        let dt = self.last.map_or(0.0, |l| now.saturating_duration_since(l).as_secs_f32().min(MAX_ADVANCE));
        self.last = Some(now);
        self.t += dt;
        self.swirl *= (-render::VELOCITY_FADE * dt).exp();
        if demo {
            self.stir.moved_to(demo_cursor(self.t - T0, bounds));
            self.moved_at = Some(now);
        }
        let last = self.planned.then_some(self.frame.fog);
        let fog = FogPlan::next(last, self.t, rate.fog_every());
        self.planned = true;
        let marches = last.is_none_or(|l| l.newer != fog.newer);
        let mut splat = None;
        if marches {
            let since = self.taken.map_or(rate.fog_every(), |k| now.saturating_duration_since(k).as_secs_f32());
            self.taken = Some(now);
            splat = self.stir.take(since, bounds);
            if let Some(s) = splat {
                self.swirl = self.swirl.max(s.speed() * render::PUSH);
            }
        }
        self.frame = Frame { t: self.t, fog, splat, stirred: rate == Rate::Full, still: false };
    }

    pub fn frame(&self) -> Frame {
        self.frame
    }
}

/// The shader widget's side of the backdrop.
pub struct Program {
    still: bool,
    demo: bool,
}

impl<Message> shader::Program<Message> for Program {
    type State = Clock;
    type Primitive = render::Primitive;

    fn update(&self, clock: &mut Clock, event: &iced::Event, bounds: Rectangle, _cursor: mouse::Cursor) -> Option<shader::Action<Message>> {
        match event {
            iced::Event::Window(window::Event::RedrawRequested(now)) => match face::pace(clock.card.frames() > 0, clock.looked) {
                Pace::Draw => {
                    let rate = clock.rate(*now, self.still);
                    let Some(every) = rate.every() else {
                        // paused: the frame already made is shown again; still mode marks it still, once
                        if self.still && !clock.frame.still {
                            clock.tick(*now, bounds, rate, true, false);
                        }
                        clock.due = None;
                        return None;
                    };
                    if let Some(due) = clock.due.filter(|d| *now + SLACK < *d) {
                        // a redraw for the form's sake, before the backdrop's own: nothing new for the card
                        return Some(shader::Action::request_redraw_at(due));
                    }
                    clock.tick(*now, bounds, rate, false, self.demo);
                    let due = *now + every;
                    clock.due = Some(due);
                    Some(shader::Action::request_redraw_at(due))
                }
                Pace::Look => {
                    clock.looked = true;
                    clock.tick(*now, bounds, Rate::Full, self.still, self.demo);
                    Some(shader::Action::request_redraw())
                }
                Pace::Rest => None,
            },
            iced::Event::Window(window::Event::Focused) => {
                clock.focused = true;
                (!self.still).then(shader::Action::request_redraw)
            }
            iced::Event::Window(window::Event::Unfocused) => {
                clock.focused = false;
                None
            }
            // Never captured: the form above still gets every move and click.
            iced::Event::Mouse(mouse::Event::CursorMoved { position }) if !self.still => {
                clock.stir.moved_to(*position);
                let now = Instant::now();
                let was = clock.rate(now, false);
                clock.moved_at = Some(now);
                // calm or paused: the stirring starts now, not at the next calm frame
                (was != Rate::Full).then(|| {
                    clock.due = None;
                    shader::Action::request_redraw()
                })
            }
            iced::Event::Mouse(mouse::Event::CursorLeft) => {
                clock.stir.left();
                None
            }
            _ => None,
        }
    }

    fn draw(&self, clock: &Clock, _cursor: mouse::Cursor, _bounds: Rectangle) -> render::Primitive {
        render::Primitive::new(clock.frame(), clock.card.clone())
    }
}

/// The old diagonal gradient (the sign-in screen's art panel before the backdrop): a warm light in the upper left
/// falling to black. It lies under the shader and is what shows wherever the card draws nothing.
pub fn gradient(_: &iced::Theme) -> container::Style {
    container::Style {
        background: Some(Background::Gradient(iced::Gradient::Linear(
            iced::gradient::Linear::new(iced::Radians(std::f32::consts::PI * 0.75))
                .add_stop(0.0, Color::from_rgb8(0x2a, 0x22, 0x10))
                .add_stop(0.55, Color::from_rgb8(0x0c, 0x0a, 0x06))
                .add_stop(1.0, Color::BLACK),
        ))),
        ..container::Style::default()
    }
}

/// The backdrop, filling what it is given: the gradient, and the scene over it wherever the card draws one; moving
/// when `motion`, else one frame of it.
pub fn view<'a, Message: 'a>(motion: bool) -> Element<'a, Message> {
    stack![
        container(iced::widget::Space::new()).width(Length::Fill).height(Length::Fill).style(gradient),
        shader(Program { still: !motion, demo: switched_on(std::env::var(DEMO_VAR).ok().as_deref()) })
            .width(Length::Fill)
            .height(Length::Fill),
    ]
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOUNDS: Rectangle = Rectangle { x: 0.0, y: 0.0, width: 1000.0, height: 500.0 };

    #[test]
    fn the_still_switch_reads_as_a_switch() {
        assert!(!switched_on(None));
        for off in ["", " ", "0", "false", "FALSE", "off", "Off"] {
            assert!(!switched_on(Some(off)), "{off:?}");
        }
        for on in ["1", "true", "yes", "on", " 1 "] {
            assert!(switched_on(Some(on)), "{on:?}");
        }
    }

    #[test]
    fn motion_at_start_is_forced_off_then_kept_then_windows_then_on() {
        assert!(motion_at_start(false, None, None), "nothing says otherwise: it moves");
        assert!(!motion_at_start(true, Some(true), Some(true)), "CENTCOM_STILL_BACKDROP wins");
        assert!(!motion_at_start(false, Some(false), Some(true)), "the person's choice over Windows'");
        assert!(motion_at_start(false, Some(true), Some(false)));
        assert!(!motion_at_start(false, None, Some(false)), "Windows' Show animations off: still");
        assert!(motion_at_start(false, None, Some(true)));
    }

    #[test]
    fn stirred_is_sixty_calm_is_twenty_and_unfocused_or_still_is_none() {
        let just = Some(Duration::from_millis(100));
        let long_ago = Some(Duration::from_secs(10));
        assert_eq!(rate(true, false, just, 0.0), Rate::Full, "the cursor moved just now");
        assert_eq!(rate(true, false, long_ago, 1.0), Rate::Full, "the swirl it left is still fast");
        assert_eq!(rate(true, false, long_ago, 0.1), Rate::Calm, "settled");
        assert_eq!(rate(true, false, None, 0.0), Rate::Calm, "never stirred");
        assert_eq!(rate(false, false, long_ago, 0.0), Rate::Paused, "another window has the focus");
        assert_eq!(rate(false, false, just, 0.0), Rate::Full, "but a cursor moving over it stirs it");
        assert_eq!(rate(true, true, just, 5.0), Rate::Paused, "still: never a frame of its own");
        assert_eq!(Rate::Full.every(), Some(face::FRAME));
        assert_eq!(Rate::Calm.every(), Some(Duration::from_millis(50)));
        assert_eq!(Rate::Paused.every(), None);
        assert!(Rate::Calm.fog_every() > Rate::Full.fog_every());
        assert_eq!(rate(true, false, Some(SETTLE), 0.0), Rate::Calm, "SETTLE after the last move, calm");
    }

    #[test]
    fn the_fog_is_marched_ahead_and_blended_between_marches() {
        let every = 0.2;
        let first = FogPlan::next(None, 40.0, every);
        assert_eq!(first, FogPlan::at(40.0), "the first frame: one march, now");
        let a = FogPlan::next(Some(first), 40.05, every);
        assert_eq!((a.older, a.newer), (40.0, 40.2), "passed it: the next one ahead");
        assert!((a.blend - 0.25).abs() < 1e-4, "{a:?}");
        let b = FogPlan::next(Some(a), 40.15, every);
        assert_eq!((b.older, b.newer), (40.0, 40.2), "between them: the same two");
        assert!((b.blend - 0.75).abs() < 1e-4);
        let c = FogPlan::next(Some(b), 40.25, every);
        assert_eq!((c.older, c.newer), (40.2, 40.4));
        // a gap longer than a march: a fresh pair from now
        let d = FogPlan::next(Some(c), 41.0, every);
        assert_eq!((d.older, d.newer, d.blend), (41.0, 41.2, 0.0));
    }

    #[test]
    fn the_clock_runs_from_mid_roll_and_a_pause_leaves_the_fog_where_it_was() {
        let start = Instant::now();
        let mut clock = Clock::default();
        clock.tick(start, BOUNDS, Rate::Calm, false, false);
        assert_eq!(clock.frame().t, T0);
        assert_eq!(clock.frame().fog, FogPlan::at(T0));
        clock.tick(start + Duration::from_millis(50), BOUNDS, Rate::Calm, false, false);
        assert!((clock.frame().t - (T0 + 0.05)).abs() < 1e-4);
        assert!((clock.frame().fog.newer - (T0 + 0.2)).abs() < 1e-4, "{:?}", clock.frame().fog);
        // ten seconds unfocused: the scene goes on from where it stopped, a quarter of a second at most
        clock.tick(start + Duration::from_secs(10), BOUNDS, Rate::Calm, false, false);
        assert!((clock.frame().t - (T0 + 0.05 + MAX_ADVANCE)).abs() < 1e-4);
    }

    #[test]
    fn a_still_backdrop_keeps_its_one_frame() {
        let start = Instant::now();
        let mut still = Clock::default();
        still.stir.moved_to(Point::new(10.0, 10.0));
        still.stir.moved_to(Point::new(50.0, 10.0));
        for i in 0..3 {
            still.tick(start + Duration::from_secs(i), BOUNDS, Rate::Paused, true, true);
            assert_eq!(still.frame(), Frame { still: true, ..Frame::first() }, "the scene at T0, unstirred");
        }
        // switched to still after running: the frame it had, marked still
        let mut ran = Clock::default();
        ran.tick(start, BOUNDS, Rate::Calm, false, false);
        ran.tick(start + Duration::from_millis(120), BOUNDS, Rate::Calm, false, false);
        let before = ran.frame();
        ran.tick(start + Duration::from_secs(5), BOUNDS, Rate::Paused, true, false);
        assert_eq!(ran.frame(), Frame { still: true, ..before });
    }

    #[test]
    fn a_still_backdrop_asks_for_no_frames_after_its_first() {
        let program = Program { still: true, demo: false };
        let mut clock = Clock::default();
        let redraw = |clock: &mut Clock, at: Instant| {
            shader::Program::<()>::update(
                &program,
                clock,
                &iced::Event::Window(window::Event::RedrawRequested(at)),
                BOUNDS,
                mouse::Cursor::Unavailable,
            )
        };
        let start = Instant::now();
        assert!(redraw(&mut clock, start).is_some(), "the first look asks once more");
        clock.card.drew();
        for i in 1..5 {
            assert!(redraw(&mut clock, start + Duration::from_millis(100 * i)).is_none(), "then nothing");
        }
        let moved = iced::Event::Mouse(mouse::Event::CursorMoved { position: Point::new(5.0, 5.0) });
        assert!(shader::Program::<()>::update(&program, &mut clock, &moved, BOUNDS, mouse::Cursor::Unavailable).is_none());
        assert!(redraw(&mut clock, start + Duration::from_secs(2)).is_none(), "the cursor does not wake it");
        assert!(clock.frame().still);
    }

    #[test]
    fn the_moving_backdrop_paces_itself_and_rests_unfocused() {
        let program = Program { still: false, demo: false };
        let mut clock = Clock::default();
        let send =
            |clock: &mut Clock, e: iced::Event| shader::Program::<()>::update(&program, clock, &e, BOUNDS, mouse::Cursor::Unavailable);
        let redraw = |at: Instant| iced::Event::Window(window::Event::RedrawRequested(at));
        let start = Instant::now();
        assert!(send(&mut clock, redraw(start)).is_some());
        clock.card.drew();
        let t1 = start + Duration::from_millis(10);
        assert!(send(&mut clock, redraw(t1)).is_some());
        let frame = clock.frame();
        // the form redrawn 20 ms later: before the calm frame is due, nothing new
        assert!(send(&mut clock, redraw(t1 + Duration::from_millis(20))).is_some());
        assert_eq!(clock.frame(), frame);
        assert!(send(&mut clock, redraw(t1 + Duration::from_millis(50))).is_some());
        assert_ne!(clock.frame(), frame, "the calm frame");
        // another window takes the focus: no more frames
        assert!(send(&mut clock, iced::Event::Window(window::Event::Unfocused)).is_none());
        let paused = clock.frame();
        assert!(send(&mut clock, redraw(t1 + Duration::from_millis(200))).is_none());
        assert_eq!(clock.frame(), paused);
        // the focus comes back: a frame is asked for at once
        assert!(send(&mut clock, iced::Event::Window(window::Event::Focused)).is_some());
        assert!(send(&mut clock, redraw(t1 + Duration::from_millis(300))).is_some());
        assert_ne!(clock.frame(), paused);
    }

    #[test]
    fn a_stroke_raises_the_rate_and_its_swirl_fades() {
        let start = Instant::now();
        let mut clock = Clock::default();
        clock.tick(start, BOUNDS, Rate::Calm, false, false);
        assert_eq!(clock.rate(start, false), Rate::Calm);
        clock.stir.moved_to(Point::new(100.0, 100.0));
        clock.stir.moved_to(Point::new(300.0, 100.0));
        clock.moved_at = Some(start);
        assert_eq!(clock.rate(start, false), Rate::Full);
        clock.tick(start + Duration::from_millis(100), BOUNDS, Rate::Full, false, false);
        let splat = clock.frame().splat.expect("the stroke goes in with the next march");
        assert!(clock.swirl >= splat.speed() * render::PUSH - 1e-4);
        // long after the cursor stopped, the swirl has faded and the rate is calm again
        let mut at = start + Duration::from_millis(100);
        for _ in 0..80 {
            at += Duration::from_millis(250);
            clock.tick(at, BOUNDS, Rate::Calm, false, false);
        }
        assert_eq!(clock.rate(at, false), Rate::Calm, "swirl {}", clock.swirl);
    }

    #[test]
    fn the_cursor_stirs_along_its_path_at_its_speed() {
        let bounds = Rectangle { x: 100.0, y: 50.0, width: 1000.0, height: 500.0 };
        let mut stir = Stir::default();
        assert_eq!(stir.take(0.016, bounds), None, "no cursor, no stroke");
        stir.moved_to(Point::new(200.0, 300.0));
        assert_eq!(stir.take(0.016, bounds), None, "the first sight of the cursor is no stroke");
        stir.moved_to(Point::new(250.0, 300.0));
        stir.moved_to(Point::new(300.0, 275.0));
        let s = stir.take(0.02, bounds).expect("a stroke");
        assert_eq!(s.from, [0.1, 0.5]);
        assert_eq!(s.to, [0.2, 0.45]);
        assert!((s.velocity[0] - 5.0).abs() < 1e-4 && (s.velocity[1] + 2.5).abs() < 1e-4, "{:?}", s.velocity);
        assert_eq!(stir.take(0.02, bounds), None, "each stroke is taken once");
        // the next stroke begins where the last ended
        stir.moved_to(Point::new(310.0, 275.0));
        assert_eq!(stir.take(0.02, bounds).unwrap().from, [0.2, 0.45]);
        // a flick is capped
        stir.moved_to(Point::new(1100.0, 50.0));
        let flick = stir.take(0.001, bounds).unwrap();
        assert!((flick.speed() - MAX_SPEED).abs() < 1e-3, "{}", flick.speed());
        // leaving the window ends the path: coming back elsewhere is no stroke across the window
        stir.left();
        stir.moved_to(Point::new(900.0, 500.0));
        assert_eq!(stir.take(0.02, bounds), None);
        // standing still is no stroke
        stir.moved_to(Point::new(900.0, 500.0));
        assert_eq!(stir.take(0.02, bounds), None);
    }
}
