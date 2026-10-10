//! The Appearance creator: where a person shapes their Sinai, rewritten in iced from the Angel window's
//! (`angel-native/src/creator.rs`, which is egui and cannot be linked; egui is not a dependency of Alelyon).
//!
//! It works as the Angel window's does. It takes the whole Sinai page: the face large in the middle, seen from the
//! creator's own camera against nothing so the shape is all there is to look at; the parts of Sinai down the left;
//! the controls down the right, every one the Angel window's control catalog offers, found by part or by searching.
//! Everything edits a working copy: Done keeps it, Cancel puts back what the creator opened with. Every change can
//! be undone, and a slider dragged across its travel is one change, not a hundred. Starting looks, random faces, the
//! looks a person keeps and share codes are the Angel window's own (`appearance.rs`, linked).
//!
//! Two differences, both on purpose. The file is written only on Done: a look kept
//! or forgotten here is part of the working copy, so Cancel takes it back too, where the Angel window writes it at
//! once. And iced has no colour wheel: a colour is set with its red, green and blue, beside a swatch.

use iced::widget::{Column, Row, button, column, container, mouse_area, row, scrollable, slider, space, stack, text, text_input};
use iced::{Alignment, Element, Length};

use crate::theme::{self, fonts};

use super::{Face, Preview};
use crate::appearance::{Appearance, Catalog, Control, History, Kind, Palette, Saved, Side};
use crate::expression::TONES;
use crate::ui;

/// The part of the creator that holds colours rather than shapes.
pub const COLOURS: &str = "colours";

/// Where the camera looks for each part of Sinai, and from how far, in Sinai's own units (the Angel window's).
pub fn focus_of(focus: &str) -> ([f32; 3], f32, Option<f32>) {
    match focus {
        "head" => ([0.0, 0.0, 0.2], 6.4, None),
        "brow" => ([0.0, 0.25, 0.8], 3.6, None),
        "eyes" => ([0.0, -0.14, 0.8], 3.4, None),
        "nose" => ([0.0, -0.45, 0.9], 3.0, None),
        "mouth" => ([0.0, -0.9, 0.9], 3.0, None),
        "jaw" => ([0.0, -0.95, 0.6], 4.0, None),
        "ear" => ([0.6, -0.3, 0.0], 3.6, Some(1.35)),
        "neck" => ([0.0, -1.5, 0.2], 5.4, None),
        "bust" => ([0.0, -1.25, 0.0], 12.0, None),
        _ => ([0.0, -0.3, 0.6], 5.0, None),
    }
}

/// The colour slots, with what each one colours.
pub const SLOTS: [(&str, &str, &str); 4] = [
    ("lattice", "Lattice", "the lines Sinai is drawn in"),
    ("fill", "Body", "the surface under the lines"),
    ("glow", "Glow", "the light the bust dissolves into at its edges"),
    ("iris", "Eyes", "the colour of the irises"),
];

/// The views the buttons under the face turn to.
pub const VIEWS: [(&str, f32); 4] = [("Front", 0.0), ("Three-quarter", -0.6), ("Profile", -1.5708), ("Back", 3.1416)];

/// A menu along the top, open below the bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Menu {
    Reset,
    Randomise,
    Looks,
    Share,
}

#[derive(Clone, Debug)]
pub enum Msg {
    /// Show one part of Sinai (a category of the catalog, or `COLOURS`).
    Part(String),
    Search(String),
    /// Move a control (by id), on one side or both.
    Set(String, Option<Side>, f32),
    /// The pointer let go of a slider: the gesture is one change.
    Released,
    /// A control back to Sinai's own.
    Reset(String),
    /// Set a sided control's two sides together or apart.
    Link(String),
    /// One channel (0 red, 1 green, 2 blue) of a colour slot.
    Colour(&'static str, usize, f32),
    /// A colour slot back to the brand's, or all of them.
    BrandColour(&'static str),
    BrandColours,
    Menu(Option<Menu>),
    ResetPart,
    ResetAll,
    Randomise,
    RandomisePart,
    /// Sinai as it ships, a starting look from the catalog, or a look the person kept.
    AsShipped,
    ShippedLook(usize),
    KeptLook(usize),
    Forget(usize),
    LookName(String),
    Keep,
    CopyCode,
    Code(String),
    UseCode,
    Tone(String),
    Speaking(bool),
    Undo,
    Redo,
    /// Turn the camera to one of the views.
    View(f32),
    /// The control under the pointer, lit up on the face.
    Hover(Option<String>),
    Done,
    Cancel,
}

/// What a message asks of the window.
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    Open,
    /// Keep the working copy and the looks, and write the file.
    Done(Appearance, Vec<(String, Appearance)>),
    Cancelled,
    /// Put this text on the clipboard.
    Copy(String),
}

pub struct Creator {
    pub working: Appearance,
    entry: Appearance,
    /// The looks the person keeps, as they will be written on Done.
    pub looks: Vec<(String, Appearance)>,
    history: History,
    pub part: String,
    pub search: String,
    pub tone: String,
    pub speaking: bool,
    pub hovered: Option<String>,
    pub menu: Option<Menu>,
    pub code: String,
    pub code_status: String,
    pub look_name: String,
    /// What reading the saved file said, shown in the bar.
    pub notice: String,
    seed: u64,
    /// A slider is being dragged and its change is already recorded for undo.
    gesture_open: bool,
    /// The creator's own view of Sinai: its animation and camera.
    pub face: Face,
}

impl Creator {
    pub fn open(saved: &Saved, catalog: &Catalog) -> Creator {
        let part = catalog.categories.first().map(|c| c.id.clone()).unwrap_or_default();
        let face = Face::new(saved.current.clone());
        let mut creator = Creator {
            working: saved.current.clone(),
            entry: saved.current.clone(),
            looks: saved.looks.clone(),
            history: History::default(),
            part,
            search: String::new(),
            tone: String::new(),
            speaking: false,
            hovered: None,
            menu: None,
            code: String::new(),
            code_status: String::new(),
            look_name: String::new(),
            notice: saved.notice.clone(),
            seed: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.as_nanos() as u64),
            gesture_open: false,
            face,
        };
        let (target, dist, _) = focus_of("head");
        creator.face.lock().preview = Some(Preview::new(target, dist));
        creator.focus(catalog);
        creator.sync();
        creator
    }

    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    /// The camera goes to the part on show.
    fn focus(&mut self, catalog: &Catalog) {
        let focus = catalog.categories.iter().find(|c| c.id == self.part).map_or("head", |c| c.focus.as_str());
        self.look_at(focus);
    }

    fn look_at(&mut self, focus: &str) {
        let (target, dist, yaw) = focus_of(focus);
        if let Some(p) = self.face.lock().preview.as_mut() {
            p.goal = (target, dist);
            if let Some(y) = yaw {
                p.yaw = y;
            }
        }
    }

    /// Hand the face what it shows: the working copy, the tone, the speaking jaw and the lit-up control. The light shows
    /// what a control moves while the pointer rests on it, and goes out while its slider is dragged, so the change
    /// itself is what shows.
    fn sync(&mut self) {
        let mut a = self.face.lock();
        a.appearance = self.working.clone();
        if let Some(p) = a.preview.as_mut() {
            p.tone = self.tone.clone();
            p.speaking = self.speaking;
            p.hovered = if self.gesture_open { None } else { self.hovered.clone() };
        }
    }

    pub fn update(&mut self, msg: Msg, catalog: &Catalog) -> Outcome {
        let before = self.working.clone();
        let gesture = matches!(msg, Msg::Set(..) | Msg::Colour(..));
        let mut outcome = Outcome::Open;
        let mut undoing = false;
        match msg {
            Msg::Part(part) => {
                self.part = part;
                self.search.clear();
                self.focus(catalog);
            }
            Msg::Search(s) => self.search = s,
            Msg::Set(id, side, v) => {
                if let Some(c) = catalog.control(&id) {
                    // A control found by searching, or one that acts outside its part's view, takes the camera to
                    // where it acts as it starts to move.
                    if !self.gesture_open {
                        let focus = c.focus.clone().unwrap_or_else(|| {
                            catalog.categories.iter().find(|k| k.id == c.category).map_or("face".into(), |k| k.focus.clone())
                        });
                        self.look_at(&focus);
                    }
                    self.working.set(c, side, v);
                }
            }
            Msg::Released => self.gesture_open = false,
            Msg::Reset(id) => {
                if let Some(c) = catalog.control(&id) {
                    self.working.reset(c);
                }
            }
            Msg::Link(id) => {
                if let Some(c) = catalog.control(&id) {
                    if self.working.is_linked(c) {
                        self.working.unlink(c);
                    } else {
                        self.working.link(c);
                    }
                }
            }
            Msg::Colour(slot, channel, v) => {
                let mut c = self.working.palette.shown(slot);
                c[channel.min(2)] = v.round().clamp(0.0, 255.0) as u8;
                self.working.palette.set(slot, Some(c));
            }
            Msg::BrandColour(slot) => self.working.palette.set(slot, None),
            Msg::BrandColours => self.working.palette = Palette::default(),
            Msg::Menu(menu) => self.menu = menu,
            Msg::ResetPart => {
                if self.part == COLOURS {
                    self.working.palette = Palette::default();
                } else {
                    self.working.reset_category(catalog, &self.part);
                }
                self.menu = None;
            }
            Msg::ResetAll => {
                self.working = Appearance::default();
                self.menu = None;
            }
            Msg::Randomise => {
                self.seed = self.seed.wrapping_add(1);
                let palette = self.working.palette.clone();
                self.working = Appearance::random(catalog, self.seed, None);
                self.working.palette = palette;
            }
            Msg::RandomisePart => {
                if self.part != COLOURS && !self.part.is_empty() {
                    self.seed = self.seed.wrapping_add(1);
                    let random = Appearance::random(catalog, self.seed, Some(&self.part));
                    self.working = self.working.with_category_from(&random, catalog, &self.part);
                }
            }
            // Sinai as it ships, colours included, as the Angel window's Looks menu does; the starting points below
            // are shapes only, so the colours a person chose stay.
            Msg::AsShipped => self.working = Appearance::default(),
            Msg::ShippedLook(i) => {
                if let Some((_, look)) = catalog.looks.get(i) {
                    let palette = self.working.palette.clone();
                    self.working = look.clone();
                    self.working.palette = palette;
                }
            }
            Msg::KeptLook(i) => {
                if let Some((_, look)) = self.looks.get(i) {
                    self.working = look.clone();
                }
            }
            Msg::Forget(i) => {
                if i < self.looks.len() {
                    self.looks.remove(i);
                }
            }
            Msg::LookName(name) => self.look_name = name,
            Msg::Keep => {
                let name = self.look_name.trim().to_string();
                if !name.is_empty() {
                    match self.looks.iter_mut().find(|(n, _)| *n == name) {
                        Some(slot) => slot.1 = self.working.clone(),
                        None => self.looks.push((name, self.working.clone())),
                    }
                    self.look_name.clear();
                }
            }
            Msg::CopyCode => {
                let code = self.working.share_code();
                self.code_status = format!("Copied: {} characters", code.len());
                outcome = Outcome::Copy(code);
            }
            Msg::Code(code) => self.code = code,
            Msg::UseCode => match Appearance::from_share_code(&self.code, catalog) {
                Ok((a, notes)) => {
                    self.working = a;
                    self.code_status = if notes.is_empty() { "Applied".into() } else { notes.join("; ") };
                    self.code.clear();
                }
                Err(why) => self.code_status = why,
            },
            Msg::Tone(tone) => self.tone = tone,
            Msg::Speaking(on) => self.speaking = on,
            Msg::Undo => {
                undoing = true;
                if let Some(a) = self.history.undo(&self.working) {
                    self.working = a;
                }
            }
            Msg::Redo => {
                undoing = true;
                if let Some(a) = self.history.redo(&self.working) {
                    self.working = a;
                }
            }
            Msg::View(yaw) => {
                if let Some(p) = self.face.lock().preview.as_mut() {
                    p.yaw = yaw;
                    p.pitch = 0.05;
                }
            }
            Msg::Hover(id) => self.hovered = id,
            Msg::Done => outcome = Outcome::Done(self.working.clone(), self.looks.clone()),
            Msg::Cancel => {
                self.working = self.entry.clone();
                outcome = Outcome::Cancelled;
            }
        }
        // One undo step a change, and one a gesture: a slider dragged is recorded when it starts.
        if self.working != before && !undoing {
            if !gesture {
                self.history.record(&before);
            } else if !self.gesture_open {
                self.history.record(&before);
                self.gesture_open = true;
            }
        }
        self.sync();
        outcome
    }

    /// The controls on show: the part's, or every control whose words match the search.
    pub fn shown<'a>(&self, catalog: &'a Catalog) -> Vec<&'a Control> {
        let needle = self.search.trim().to_lowercase();
        if needle.is_empty() {
            return catalog.controls.iter().filter(|c| c.category == self.part).collect();
        }
        catalog
            .controls
            .iter()
            .filter(|c| {
                let ends = match &c.kind {
                    Kind::Slider { ends, .. } => ends.join(" "),
                    Kind::Shape { .. } => String::new(),
                };
                let part = catalog.categories.iter().find(|k| k.id == c.category).map_or("", |k| k.label.as_str());
                format!("{} {} {}", c.label, ends, part).to_lowercase().contains(&needle)
            })
            .collect()
    }
}

// ------------------------------------------------------------------- what it looks like

fn button_small<'a>(words: impl text::IntoFragment<'a>, message: Option<Msg>) -> Element<'a, Msg> {
    button(text(words).size(12.0).font(fonts().ui)).padding([4.0, 9.0]).on_press_maybe(message).style(theme::secondary_button).into()
}

fn segment<'a>(words: impl text::IntoFragment<'a>, selected: bool, message: Msg) -> Element<'a, Msg> {
    button(text(words).size(12.5).font(fonts().ui).color(if selected { theme::GOLD } else { theme::TEXT_DIM }))
        .padding([4.0, 10.0])
        .on_press(message)
        .style(theme::segment_button(selected))
        .into()
}

/// The creator, filling the Sinai page.
pub fn view<'a>(c: &'a Creator, catalog: &'a Catalog) -> Element<'a, Msg> {
    let menu_button = |label: &'static str, which: Menu| {
        let open = c.menu == Some(which);
        segment(label, open, Msg::Menu(if open { None } else { Some(which) }))
    };
    let mut bar = Row::new()
        .spacing(8)
        .align_y(Alignment::Center)
        .push(ui::strong("Sinai's appearance", 15.0, theme::GOLD))
        .push(button_small("Undo", c.can_undo().then_some(Msg::Undo)))
        .push(button_small("Redo", c.can_redo().then_some(Msg::Redo)))
        .push(menu_button("Reset", Menu::Reset))
        .push(menu_button("Randomise", Menu::Randomise))
        .push(menu_button("Looks", Menu::Looks))
        .push(menu_button("Share", Menu::Share))
        .push(space().width(12))
        .push(ui::label("Wearing", 12.5, theme::TEXT_DIM))
        .push(segment("At rest", c.tone.is_empty(), Msg::Tone(String::new())));
    for tone in TONES {
        bar = bar.push(segment(tone, c.tone == tone, Msg::Tone(tone.to_string())));
    }
    bar = bar.push(segment("Speaking", c.speaking, Msg::Speaking(!c.speaking)));
    let ends = row![space().width(Length::Fill), ui::secondary("Cancel", Some(Msg::Cancel)), ui::primary("Done", Some(Msg::Done)),]
        .spacing(8)
        .align_y(Alignment::Center);
    let mut top = Column::new().spacing(8).push(
        scrollable(bar)
            .direction(scrollable::Direction::Horizontal(scrollable::Scrollbar::new().width(4).scroller_width(4)))
            .style(theme::scrollbars),
    );
    let mut status = Row::new().spacing(10).align_y(Alignment::Center);
    if !c.notice.is_empty() {
        status = status.push(ui::label(c.notice.as_str(), 12.5, theme::CAUTION).width(Length::Fill));
    } else {
        status = status.push(
            ui::label(
                "Done keeps this appearance and writes it where the Angel window keeps it; Cancel puts back the one from before.",
                12.5,
                theme::TEXT_FAINT,
            )
            .width(Length::Fill),
        );
    }
    top = top.push(row![status, ends].spacing(10).align_y(Alignment::Center));
    if let Some(menu) = c.menu {
        top = top.push(container(menu_panel(c, catalog, menu)).padding(10).width(Length::Fill).style(theme::notice(theme::GOLD_DIM)));
    }

    // The parts of Sinai, down the left: a dot after a part a person has moved.
    let mut parts = Column::new().spacing(4);
    for k in &catalog.categories {
        let moved = catalog.in_category(&k.id).any(|ctl| ctl.keys().iter().any(|key| c.working.value(key) != 0.0));
        let words = if moved { format!("{}  \u{2022}", k.label) } else { k.label.clone() };
        let selected = c.part == k.id && c.search.trim().is_empty();
        parts = parts.push(
            button(text(words).size(13.0).font(fonts().ui))
                .width(Length::Fill)
                .padding([5.0, 10.0])
                .on_press(Msg::Part(k.id.clone()))
                .style(theme::list_row(selected)),
        );
    }
    parts = parts.push(container(space().height(1)).width(Length::Fill).style(theme::line));
    let colours_on = c.part == COLOURS && c.search.trim().is_empty();
    parts = parts.push(
        button(text("Colours").size(13.0).font(fonts().ui))
            .width(Length::Fill)
            .padding([5.0, 10.0])
            .on_press(Msg::Part(COLOURS.into()))
            .style(theme::list_row(colours_on)),
    );
    let left = container(scrollable(parts).style(theme::scrollbars).height(Length::Fill))
        .padding(8)
        .width(184)
        .height(Length::Fill)
        .style(theme::panel);

    // The face, with the views to turn it to.
    let mut views = Row::new().spacing(6);
    for (label, yaw) in VIEWS {
        views = views.push(button_small(label, Some(Msg::View(yaw))));
    }
    let middle = stack![
        super::view(&c.face),
        container(ui::label("Drag to turn, scroll to zoom", 11.5, theme::TEXT_FAINT)).padding(8),
        container(views).align_bottom(Length::Fill).center_x(Length::Fill).padding(10),
    ]
    .width(Length::Fill)
    .height(Length::Fill);

    // The controls, down the right.
    let mut controls = Column::new().spacing(12);
    if colours_on {
        controls = controls.push(colours(c));
    } else {
        let shown = c.shown(catalog);
        if shown.is_empty() {
            controls = controls.push(ui::label("Nothing matches.", 13.0, theme::TEXT_FAINT));
        }
        let searching = !c.search.trim().is_empty();
        for ctl in shown {
            let part = if searching { catalog.categories.iter().find(|k| k.id == ctl.category).map(|k| k.label.as_str()) } else { None };
            controls =
                controls.push(mouse_area(control_row(c, ctl, part)).on_enter(Msg::Hover(Some(ctl.id.clone()))).on_exit(Msg::Hover(None)));
        }
    }
    let right = container(
        column![
            text_input("Search every control…", &c.search)
                .on_input(Msg::Search)
                .padding(8)
                .size(13.0)
                .font(fonts().ui)
                .style(ui::input_style),
            scrollable(container(controls).padding(iced::padding::right(10))).style(theme::scrollbars).height(Length::Fill),
        ]
        .spacing(8),
    )
    .padding(10)
    .width(380)
    .height(Length::Fill)
    .style(theme::panel);

    column![top, row![left, container(middle).width(Length::Fill).height(Length::Fill), right].spacing(10).height(Length::Fill)]
        .spacing(10)
        .height(Length::Fill)
        .into()
}

fn menu_panel<'a>(c: &'a Creator, catalog: &'a Catalog, menu: Menu) -> Element<'a, Msg> {
    match menu {
        Menu::Reset => {
            let shaped = !c.working.is_default_shape() || c.working.palette != Palette::default();
            row![
                button_small("Reset this part", Some(Msg::ResetPart)),
                button_small("Reset everything to Sinai as it ships", shaped.then_some(Msg::ResetAll))
            ]
            .spacing(8)
            .into()
        }
        Menu::Randomise => {
            let part = c.part != COLOURS && !c.part.is_empty();
            row![
                button_small("Randomise the whole face and body", Some(Msg::Randomise)),
                button_small("Randomise this part only", part.then_some(Msg::RandomisePart)),
                ui::label("Colours stay as they are.", 12.0, theme::TEXT_FAINT),
            ]
            .spacing(8)
            .align_y(Alignment::Center)
            .into()
        }
        Menu::Looks => {
            let mut starting = Row::new().spacing(6).push(button_small("Sinai as it ships", Some(Msg::AsShipped)));
            for (i, (label, _)) in catalog.looks.iter().enumerate() {
                starting = starting.push(button_small(label.as_str(), Some(Msg::ShippedLook(i))));
            }
            let mut panel = Column::new()
                .spacing(8)
                .push(ui::label("Starting points (shapes only; your colours stay)", 12.0, theme::TEXT_DIM))
                .push(starting.wrap());
            if !c.looks.is_empty() {
                let mut kept = Row::new().spacing(6);
                for (i, (name, _)) in c.looks.iter().enumerate() {
                    kept = kept.push(
                        row![button_small(name.as_str(), Some(Msg::KeptLook(i))), button_small("Forget", Some(Msg::Forget(i)))].spacing(2),
                    );
                }
                panel = panel.push(ui::label("Kept (written on Done)", 12.0, theme::TEXT_DIM)).push(kept.wrap());
            }
            let name = c.look_name.trim();
            panel
                .push(
                    row![
                        text_input("Name", &c.look_name)
                            .on_input(Msg::LookName)
                            .on_submit(Msg::Keep)
                            .padding(6)
                            .size(12.5)
                            .width(180)
                            .font(fonts().ui)
                            .style(ui::input_style),
                        button_small("Keep this look", (!name.is_empty()).then_some(Msg::Keep)),
                    ]
                    .spacing(8)
                    .align_y(Alignment::Center),
                )
                .into()
        }
        Menu::Share => {
            let mut panel = row![
                button_small("Copy this look as a code", Some(Msg::CopyCode)),
                text_input("SINAI1:…", &c.code)
                    .on_input(Msg::Code)
                    .on_submit(Msg::UseCode)
                    .padding(6)
                    .size(12.5)
                    .width(300)
                    .font(fonts().mono)
                    .style(ui::input_style),
                button_small("Use this code", (!c.code.trim().is_empty()).then_some(Msg::UseCode)),
            ]
            .spacing(8)
            .align_y(Alignment::Center);
            if !c.code_status.is_empty() {
                panel = panel.push(ui::label(c.code_status.as_str(), 12.5, theme::TEXT_DIM));
            }
            panel.into()
        }
    }
}

/// One control: its name, its slider or sliders, what its ends do, and its link and reset.
fn control_row<'a>(c: &'a Creator, ctl: &'a Control, part: Option<&'a str>) -> Element<'a, Msg> {
    let moved = ctl.keys().iter().any(|k| c.working.value(k) != 0.0);
    let mut head = Row::new().spacing(6).align_y(Alignment::Center).push(ui::strong(ctl.label.as_str(), 13.0, theme::TEXT));
    if ctl.why.is_some() {
        head = head.push(ui::label("(limited)", 11.0, theme::TEXT_FAINT));
    }
    head = head.push(space().width(Length::Fill));
    if ctl.sided {
        let linked = c.working.is_linked(ctl);
        head = head.push(button_small(if linked { "both sides" } else { "each side" }, Some(Msg::Link(ctl.id.clone()))));
    }
    head = head.push(button_small("\u{21ba}", moved.then(|| Msg::Reset(ctl.id.clone()))));
    let mut card = Column::new().spacing(4);
    if let Some(part) = part {
        card = card.push(ui::label(part, 11.0, theme::TEXT_FAINT));
    }
    card = card.push(head);
    if let Some(why) = &ctl.why {
        card = card.push(ui::label(why.as_str(), 11.0, theme::TEXT_FAINT));
    }
    let sides: Vec<(Option<Side>, &str)> = if ctl.sided && !c.working.is_linked(ctl) {
        vec![(Some(Side::Left), "Sinai's left"), (Some(Side::Right), "Sinai's right")]
    } else {
        vec![(None, "")]
    };
    let (lo, hi) = ctl.range();
    for (side, name) in sides {
        let v = c.working.get(ctl, side);
        let id = ctl.id.clone();
        let mut line = Row::new().spacing(8).align_y(Alignment::Center);
        if !name.is_empty() {
            line = line.push(ui::label(name, 11.5, theme::TEXT_DIM).width(84));
        }
        line = line
            .push(slider(lo..=hi, v, move |x| Msg::Set(id.clone(), side, x)).step(0.01_f32).on_release(Msg::Released).width(Length::Fill))
            .push(ui::mono(format!("{v:.2}"), 11.5, theme::TEXT_DIM).width(40));
        card = card.push(line);
    }
    if let Kind::Slider { ends, .. } = &ctl.kind {
        card = card.push(row![
            ui::label(ends[0].as_str(), 11.0, theme::TEXT_FAINT),
            space().width(Length::Fill),
            ui::label(ends[1].as_str(), 11.0, theme::TEXT_FAINT)
        ]);
    }
    card.into()
}

/// The colour slots: red, green and blue beside a swatch, and the way back to gold on carbon.
fn colours(c: &Creator) -> Element<'_, Msg> {
    let mut list = Column::new().spacing(14).push(ui::label(
        "Gold on carbon is Sinai's own look. Any colour here can go back to it.",
        12.5,
        theme::TEXT_DIM,
    ));
    for (slot, name, about) in SLOTS {
        let shown = c.working.palette.shown(slot);
        let swatch = container(space().width(28).height(28)).style(move |_: &iced::Theme| container::Style {
            background: Some(iced::Background::Color(iced::Color::from_rgb8(shown[0], shown[1], shown[2]))),
            border: iced::Border { color: theme::LINE, width: 1.0, radius: 6.0.into() },
            ..container::Style::default()
        });
        let own = c.working.palette.get(slot).is_some();
        let mut card = Column::new().spacing(4).push(
            row![
                swatch,
                column![ui::strong(name, 13.0, theme::TEXT), ui::label(about, 11.0, theme::TEXT_FAINT)].spacing(2).width(Length::Fill),
                button_small("\u{21ba}", own.then_some(Msg::BrandColour(slot)))
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        );
        for (channel, letter) in ["R", "G", "B"].into_iter().enumerate() {
            card = card.push(
                row![
                    ui::label(letter, 11.5, theme::TEXT_DIM).width(14),
                    slider(0.0..=255.0, shown[channel] as f32, move |x| Msg::Colour(slot, channel, x))
                        .step(1.0_f32)
                        .on_release(Msg::Released)
                        .width(Length::Fill),
                    ui::mono(format!("{}", shown[channel]), 11.5, theme::TEXT_DIM).width(30),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            );
        }
        list = list.push(card);
    }
    list.push(button_small("Back to gold on carbon", (c.working.palette != Palette::default()).then_some(Msg::BrandColours))).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog() -> &'static Catalog {
        Catalog::builtin()
    }

    fn open() -> Creator {
        Creator::open(&Saved::default(), catalog())
    }

    #[test]
    fn a_slider_dragged_is_one_change_to_undo() {
        let mut c = open();
        assert!(!c.can_undo());
        for v in [0.1, 0.2, 0.3, 0.4] {
            c.update(Msg::Set("chin.width".into(), None, v), catalog());
        }
        c.update(Msg::Released, catalog());
        assert_eq!(c.working.value("chin.width"), 0.4);
        c.update(Msg::Undo, catalog());
        assert_eq!(c.working.value("chin.width"), 0.0, "the whole drag came back in one step");
        assert!(!c.can_undo() && c.can_redo());
        c.update(Msg::Redo, catalog());
        assert_eq!(c.working.value("chin.width"), 0.4);
        // a new gesture after the release is a step of its own
        c.update(Msg::Set("chin.width".into(), None, -0.2), catalog());
        c.update(Msg::Released, catalog());
        c.update(Msg::Undo, catalog());
        assert_eq!(c.working.value("chin.width"), 0.4);
    }

    #[test]
    fn the_light_goes_out_while_a_slider_is_dragged() {
        let mut c = open();
        let lit = |c: &Creator| c.face.lock().preview.as_ref().and_then(|p| p.hovered.clone());
        c.update(Msg::Hover(Some("chin.width".into())), catalog());
        assert_eq!(lit(&c).as_deref(), Some("chin.width"), "resting on a control lights what it moves");
        c.update(Msg::Set("chin.width".into(), None, 0.3), catalog());
        assert_eq!(lit(&c), None, "dragging its slider shows the change, unlit");
        c.update(Msg::Set("chin.width".into(), None, 0.4), catalog());
        assert_eq!(lit(&c), None);
        c.update(Msg::Released, catalog());
        assert_eq!(lit(&c).as_deref(), Some("chin.width"), "let go, and the light comes back");
    }

    #[test]
    fn buttons_each_record_a_step_and_cancel_puts_back_what_it_opened_with() {
        let catalog = catalog();
        let mut saved = Saved::default();
        saved.current.set(catalog.control("chin.width").unwrap(), None, 0.25);
        let mut c = Creator::open(&saved, catalog);
        c.update(Msg::Randomise, catalog);
        assert_ne!(c.working, saved.current);
        c.update(Msg::ResetAll, catalog);
        assert!(c.working.is_default_shape());
        c.update(Msg::Undo, catalog);
        c.update(Msg::Undo, catalog);
        assert_eq!(c.working, saved.current, "two buttons, two steps back");
        c.update(Msg::ShippedLook(0), catalog);
        assert_eq!(c.update(Msg::Cancel, catalog), Outcome::Cancelled);
        assert_eq!(c.working, saved.current);
    }

    #[test]
    fn sides_link_and_unlink_and_reset_returns_to_sinais_own() {
        let catalog = catalog();
        let sided = catalog.controls.iter().find(|k| k.sided && matches!(k.kind, Kind::Slider { .. })).expect("a sided slider");
        let mut c = open();
        c.update(Msg::Set(sided.id.clone(), Some(Side::Left), 0.5), catalog);
        assert_eq!(c.working.get(sided, Some(Side::Right)), 0.5, "linked: both sides move");
        c.update(Msg::Link(sided.id.clone()), catalog);
        c.update(Msg::Set(sided.id.clone(), Some(Side::Right), -0.5), catalog);
        assert_eq!((c.working.get(sided, Some(Side::Left)), c.working.get(sided, Some(Side::Right))), (0.5, -0.5));
        c.update(Msg::Link(sided.id.clone()), catalog);
        assert_eq!(c.working.get(sided, Some(Side::Right)), 0.5, "linked again, the right takes the left's value");
        c.update(Msg::Reset(sided.id.clone()), catalog);
        assert!(c.working.is_default_shape());
    }

    #[test]
    fn colours_kept_looks_and_share_codes_are_the_working_copys() {
        let catalog = catalog();
        let mut c = open();
        c.update(Msg::Colour("iris", 0, 300.0), catalog);
        assert_eq!(c.working.palette.iris.unwrap()[0], 255, "a channel stays in range");
        c.update(Msg::Released, catalog);
        c.update(Msg::Set("chin.width".into(), None, 0.3), catalog);
        c.update(Msg::Released, catalog);
        c.update(Msg::LookName("  Mine ".into()), catalog);
        c.update(Msg::Keep, catalog);
        assert_eq!(c.looks.len(), 1);
        assert_eq!(c.looks[0].0, "Mine");
        let Outcome::Copy(code) = c.update(Msg::CopyCode, catalog) else { panic!("a code to copy") };
        c.update(Msg::ResetAll, catalog);
        c.update(Msg::Code(code), catalog);
        c.update(Msg::UseCode, catalog);
        assert_eq!(c.code_status, "Applied");
        assert!((c.working.value("chin.width") - 0.3).abs() < 0.005, "a code carries values to 1/127");
        assert_eq!(c.working.palette.iris.unwrap()[0], 255);
        c.update(Msg::Code("SINAI1:broken".into()), catalog);
        c.update(Msg::UseCode, catalog);
        assert!(c.code_status.contains("damaged"), "{}", c.code_status);
        c.update(Msg::BrandColours, catalog);
        assert_eq!(c.working.palette, Palette::default());
        c.update(Msg::Forget(0), catalog);
        assert!(c.looks.is_empty());
        let Outcome::Done(kept, looks) = c.update(Msg::Done, catalog) else { panic!("done") };
        assert_eq!(kept, c.working);
        assert!(looks.is_empty(), "a look forgotten here is forgotten in what is written");
    }

    #[test]
    fn a_search_finds_controls_in_every_part_and_a_part_shows_its_own() {
        let catalog = catalog();
        let mut c = open();
        let first = c.shown(catalog);
        assert!(!first.is_empty() && first.iter().all(|k| k.category == c.part));
        c.update(Msg::Search("chin".into()), catalog);
        let found = c.shown(catalog);
        assert!(found.iter().any(|k| k.id == "chin.width"));
        c.update(Msg::Search("no such control anywhere".into()), catalog);
        assert!(c.shown(catalog).is_empty());
        c.update(Msg::Part(COLOURS.into()), catalog);
        assert!(c.search.is_empty(), "choosing a part ends the search");
    }

    #[test]
    fn the_preview_wears_what_the_creator_shows() {
        let catalog = catalog();
        let mut c = open();
        c.update(Msg::Tone("smile".into()), catalog);
        c.update(Msg::Speaking(true), catalog);
        c.update(Msg::Hover(Some("chin.width".into())), catalog);
        c.update(Msg::Set("chin.width".into(), None, 0.5), catalog);
        c.update(Msg::Released, catalog);
        let a = c.face.lock();
        let p = a.preview.as_ref().expect("the creator's face has its own camera");
        assert_eq!((p.tone.as_str(), p.speaking, p.hovered.as_deref()), ("smile", true, Some("chin.width")));
        assert_eq!(a.appearance.value("chin.width"), 0.5);
        drop(a);
        c.update(Msg::Part("ears".into()), catalog);
        let goal = c.face.lock().preview.as_ref().unwrap().goal;
        let ear = catalog.categories.iter().find(|k| k.id == "ears").map(|k| focus_of(&k.focus));
        if let Some((target, dist, _)) = ear {
            assert_eq!(goal, (target, dist), "the camera goes to the part on show");
        }
    }
}
