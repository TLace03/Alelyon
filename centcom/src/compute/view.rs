//! What the Compute and simulation page draws.

use iced::widget::canvas::{self, Canvas, Frame as Drawing, Path, Stroke};
use iced::widget::{Column, Row, button, column, container, image, row, space, text_input};
use iced::{Alignment, Element, Length, Point, Rectangle, Renderer, Size, Theme, mouse};

use crate::theme;

use super::sim::SCENES;
use super::{FRAME_H, FRAME_W, Msg, State, Tab, train};
use crate::ui::{self, card, chip, fact, label, mono, note, strong};
use crate::utc;

type El<'a> = Element<'a, Msg>;

const PURPOSE: &str = "Sinai's simulator, watched as it runs on this PC's processor, and a trainer's metrics as they are written. Nothing \
                       here uses the graphics card.";

pub fn view(state: &State, phase: f32) -> El<'_> {
    let mut page = Column::new().spacing(18).push(ui::heading("Compute and simulation", PURPOSE));
    page = page.push(ui::tabs(&state.tabs(), state.tab, |t| state.title(t), Msg::Tab));
    page = page.push(match state.tab {
        Tab::Simulator => simulator(state, phase),
        Tab::Plugin(i) => match state.plugins.get(i) {
            Some(p) => p.page.view(phase).map(move |m| Msg::Plugin(i, m)),
            None => ui::notice("This build has no such tab.", theme::CAUTION),
        },
        Tab::Training => train_tab(state),
    });
    page = page.push(ui::subheading("The section's features"));
    for feature in crate::catalogue::Section::Compute.features_with(&state.features) {
        page = page.push(ui::feature_row(feature));
    }
    page.into()
}

// ------------------------------------------------------------------- the simulator

fn simulator(state: &State, phase: f32) -> El<'_> {
    match &state.repo {
        Err(why) if why == super::NO_CHECKOUT => {
            return ui::absent(crate::capability::absent(crate::capability::Capability::SimulatorScenes));
        }
        Err(why) => return ui::notice(why.as_str(), theme::CAUTION),
        Ok(_) => {}
    }
    let mut scenes = Row::new().spacing(6);
    for (i, scene) in SCENES.iter().enumerate() {
        let on = i == state.scene;
        scenes = scenes.push(
            button(label(scene.title, 12.5, if on { theme::GOLD } else { theme::TEXT_DIM }))
                .padding([6.0, 12.0])
                .on_press(Msg::Scene(i))
                .style(theme::segment_button(on)),
        );
    }
    let scene = &SCENES[state.scene];
    let picture: El<'_> = match (&state.picture, &state.sim_failed) {
        (_, Some(why)) => container(ui::notice(format!("The simulator stopped: {why}"), theme::CAUTION)).width(FRAME_W as f32).into(),
        (Some(handle), None) => image(handle.clone()).width(FRAME_W as f32).height(FRAME_H as f32).into(),
        (None, None) if state.playing => container(ui::working(phase, "Loading the scene and drawing its first frame…"))
            .width(FRAME_W as f32)
            .height(FRAME_H as f32)
            .center_x(FRAME_W as f32)
            .center_y(FRAME_H as f32)
            .into(),
        (None, None) => container(label("Paused.", 13.5, theme::TEXT_DIM)).width(FRAME_W as f32).height(FRAME_H as f32).into(),
    };
    let mut facts = Column::new().spacing(8);
    if let Some(frame) = &state.frame {
        let s = &frame.stats;
        facts = facts
            .push(fact("Simulated time", label(format!("{:.2} s", s.sim_time), 13.0, theme::TEXT)))
            .push(fact("Physics steps", label(format!("{}", s.steps), 13.0, theme::TEXT)))
            .push(fact("Contacts now", label(format!("{}", s.contacts), 13.0, theme::TEXT)))
            .push(fact("Constraint rows", label(format!("{}", s.constraint_rows), 13.0, theme::TEXT)))
            .push(fact("Solver iterations", label(format!("{}", s.solver_iterations), 13.0, theme::TEXT)))
            .push(fact("One step takes", label(format!("{:.1} µs (mean of the latest)", s.step_us), 13.0, theme::TEXT)))
            .push(fact(
                "One frame takes",
                label(format!("{:.0} ms ({FRAME_W}×{FRAME_H}, {} threads)", s.frame_ms, super::draw_threads()), 13.0, theme::TEXT),
            ));
        if s.behind {
            facts = facts.push(chip("Behind real time: the physics skipped ahead", theme::CAUTION));
        }
    }
    if let Some(loaded) = &state.loaded {
        facts = facts
            .push(fact("Bodies, shapes", label(format!("{}, {}", loaded.bodies, loaded.geoms), 13.0, theme::TEXT)))
            .push(fact("Timestep", label(format!("{:.4} s", loaded.timestep), 13.0, theme::TEXT)));
    }
    let controls = if state.playing {
        row![ui::secondary("Pause", Some(Msg::Pause)), ui::secondary("Start again", Some(Msg::Restart))]
    } else {
        row![ui::secondary("Run", Some(Msg::Play)), ui::secondary("Start again", Some(Msg::Restart))]
    }
    .spacing(8);
    let mut body = column![
        scenes.wrap(),
        label(scene.what, 13.0, theme::TEXT_DIM),
        row![container(picture).style(theme::panel).padding(4), column![facts, controls].spacing(14)].spacing(18),
    ]
    .spacing(12);
    if let Some(loaded) = &state.loaded {
        let mut list = Column::new().spacing(5);
        if loaded.physics_gaps.is_empty() {
            list = list.push(label("Nothing: the physics does all this scene asks of it.", 13.0, theme::POSITIVE));
        }
        for gap in &loaded.physics_gaps {
            list = list.push(label(gap.as_str(), 12.5, theme::TEXT));
        }
        if !loaded.set_aside.is_empty() {
            list = list.push(note(format!(
                "The importer also set aside {} entries of the file that are not physics (mostly the settings of MuJoCo's own \
                 viewer: its colours, fog, camera and textures), the first of them:",
                loaded.set_aside.len()
            )));
            for item in loaded.set_aside.iter().take(4) {
                list = list.push(mono(ui::cut(item, 150), 11.0, theme::TEXT_FAINT));
            }
        }
        body = body.push(card("What this scene asks for that the simulator does not do", list));
    }
    body.push(note(
        "Physics: sim-physics, the CPU reference of the simulator's physics core (a port of MuJoCo 3.14.0), in f32, paced to \
         real time. Frames: sim-raycast, the host reference ray caster that checks the GPU renderer (shadows on, its scene test's \
         light). The scenes are the simulator's own test fixtures, read from this checkout.",
    ))
    .into()
}

// ------------------------------------------------------------------- training

fn train_tab(state: &State) -> El<'_> {
    let mut page = Column::new().spacing(14);
    let mut choose = Column::new().spacing(6);
    if state.train_offered.is_empty() {
        let none = match &state.training {
            Some(t) => format!("No stream in {}.", t.folder),
            None => "No folder of streams is set in this build: type the path of one below.".to_string(),
        };
        choose = choose.push(label(none, 13.0, theme::TEXT_DIM));
    }
    for o in &state.train_offered {
        let on = state.stream.as_ref().is_some_and(|s| s.path == o.path);
        let name = o.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        choose = choose.push(
            button(
                row![
                    strong(name, 13.0, if on { theme::GOLD } else { theme::TEXT }),
                    label(o.what.as_str(), 12.5, theme::TEXT_DIM),
                    space().width(Length::Fill),
                    label(format!("{:.0} KB · written {}", o.bytes as f64 / 1024.0, utc::ago(o.modified, utc::now())), 12.0, theme::TEXT_FAINT),
                ]
                .spacing(10)
                .align_y(Alignment::Center),
            )
            .width(Length::Fill)
            .padding([6.0, 10.0])
            .on_press(Msg::TrainChoose(o.path.clone()))
            .style(theme::list_row(on)),
        );
    }
    choose = choose.push(
        row![
            text_input("Or the full path of a metrics stream (.jsonl)", &state.train_typed)
                .on_input(Msg::TrainTyped)
                .on_submit(Msg::TrainOpenTyped)
                .padding(8)
                .size(13.0),
            ui::secondary("Watch", (!state.train_typed.trim().is_empty()).then_some(Msg::TrainOpenTyped)),
        ]
        .spacing(8),
    );
    page = page.push(card("Metrics streams", choose));
    let Some(s) = &state.stream else {
        let trainer = state.training.map(|t| format!(" ({})", t.trainer)).unwrap_or_default();
        return page
            .push(note(format!(
                "A trainer started with --out <file>.jsonl{trainer} writes a line a step. Choose one to watch it grow. Nothing \
                 here starts or stops a trainer."
            )))
            .into();
    };
    let colour = match s.status {
        train::Status::Completed => theme::GOLD,
        train::Status::Invalid | train::Status::Aborted => theme::CAUTION,
        _ => theme::TEXT_DIM,
    };
    let mut facts = Column::new().spacing(6);
    facts = facts.push(fact("Stream", chip(s.status.words(), colour)));
    facts = facts.push(fact(
        "Steps seen",
        label(format!("{} of {}", s.seen(), s.budget().map(|b| b.to_string()).unwrap_or("?".into())), 13.0, theme::TEXT),
    ));
    if let Some(last) = s.rows.last() {
        if let Some(l) = last.loss {
            facts = facts.push(fact("Loss now", label(format!("{l:.4}"), 13.0, theme::TEXT)));
        }
        if let Some(t) = last.tokens_per_s {
            facts = facts.push(fact("Tokens a second", label(format!("{t:.1}"), 13.0, theme::TEXT)));
        }
        if let Some(g) = last.grad_norm {
            facts = facts.push(fact("Gradient norm", label(format!("{g:.3}"), 13.0, theme::TEXT)));
        }
    }
    let mut body = column![facts, label(s.message.as_str(), 12.5, colour)].spacing(10);
    if let Some(h) = &s.header {
        body = body.push(label(train::describe(h), 12.5, theme::TEXT_DIM));
    }
    let losses: Vec<(f32, f32)> = s.rows.iter().filter_map(|r| r.loss.map(|l| (r.step as f32, l as f32))).collect();
    if losses.len() >= 2 {
        body = body.push(Canvas::new(Curve { points: losses }).width(Length::Fill).height(220));
        body = body.push(label(
            format!("Loss by step, the newest {} steps kept (scale from the lowest to the highest shown).", s.rows.len()),
            12.0,
            theme::TEXT_FAINT,
        ));
    }
    if let Some(u) = s.rows.last().map(|r| &r.utilisation).filter(|u| !u.is_empty()) {
        body = body.push(Canvas::new(Bars { values: u.clone() }).width(Length::Fill).height(70));
        body = body.push(label("Each expert's share of the last step's tokens.", 12.0, theme::TEXT_FAINT));
    }
    page = page.push(card(s.path.to_string_lossy().into_owned(), body));
    page.push(note(
        "Read by the Training Studio's rules, every two seconds while this tab is on show. The numbers are what the file \
         says; whether the trainer still runs is not known, and checkpoints, GPU memory and held-out evaluation are not in \
         the stream.",
    ))
    .into()
}

/// A line through (step, loss), scaled to fill the space between the lowest and highest values shown.
struct Curve {
    points: Vec<(f32, f32)>,
}

impl canvas::Program<Msg> for Curve {
    /// Kept between frames, and drawn again only when a point arrives.
    type State = ui::Kept;

    fn draw(&self, kept: &ui::Kept, renderer: &Renderer, _: &Theme, bounds: Rectangle, _: mouse::Cursor) -> Vec<canvas::Geometry> {
        let key = ui::key_of(&self.points.iter().map(|(x, y)| (x.to_bits(), y.to_bits())).collect::<Vec<_>>());
        vec![kept.draw(renderer, bounds.size(), key, |frame| self.line_on(frame, bounds))]
    }
}

impl Curve {
    fn line_on(&self, frame: &mut Drawing, bounds: Rectangle) {
        let (x0, x1) = (self.points.first().map(|p| p.0).unwrap_or(0.0), self.points.last().map(|p| p.0).unwrap_or(1.0));
        let lo = self.points.iter().map(|p| p.1).fold(f32::INFINITY, f32::min);
        let hi = self.points.iter().map(|p| p.1).fold(f32::NEG_INFINITY, f32::max);
        let (w, h) = (bounds.width - 8.0, bounds.height - 8.0);
        let at = |(x, y): (f32, f32)| {
            Point::new(4.0 + (x - x0) / (x1 - x0).max(1e-6) * w, 4.0 + (1.0 - (y - lo) / (hi - lo).max(1e-6)) * h)
        };
        frame.stroke(
            &Path::rectangle(Point::ORIGIN, bounds.size()),
            Stroke::default().with_color(theme::with_alpha(theme::TEXT_FAINT, 0.4)).with_width(1.0),
        );
        let line = Path::new(|b| {
            for (i, p) in self.points.iter().enumerate() {
                if i == 0 { b.move_to(at(*p)) } else { b.line_to(at(*p)) }
            }
        });
        frame.stroke(&line, Stroke::default().with_color(theme::GOLD).with_width(1.5));
    }
}

/// One bar per expert, as tall as its share.
struct Bars {
    values: Vec<f64>,
}

impl canvas::Program<Msg> for Bars {
    /// Kept between frames, and drawn again only when the shares change.
    type State = ui::Kept;

    fn draw(&self, kept: &ui::Kept, renderer: &Renderer, _: &Theme, bounds: Rectangle, _: mouse::Cursor) -> Vec<canvas::Geometry> {
        let key = ui::key_of(&self.values.iter().map(|v| v.to_bits()).collect::<Vec<_>>());
        vec![kept.draw(renderer, bounds.size(), key, |frame| self.bars_on(frame, bounds))]
    }
}

impl Bars {
    fn bars_on(&self, frame: &mut Drawing, bounds: Rectangle) {
        let top = self.values.iter().cloned().fold(0.0f64, f64::max).max(1e-9);
        let w = bounds.width / self.values.len().max(1) as f32;
        for (i, v) in self.values.iter().enumerate() {
            let h = (v / top) as f32 * (bounds.height - 2.0);
            frame.fill_rectangle(
                Point::new(i as f32 * w + 1.0, bounds.height - h),
                Size::new((w - 2.0).max(1.0), h),
                theme::with_alpha(theme::GOLD, 0.75),
            );
        }
    }
}
