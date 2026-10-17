//! Pages in windows of their own. A page of the rail leaves the main window for a window of its own by the pop-out
//! button beside the window's buttons, by dragging its place off the rail and letting go over the desktop or the page
//! (the window opens where it was let go), by a middle press on its place, or from the command palette. It
//! moves rather than copies: one page is in one window, so its work goes on wherever it shows. A page's tab (Lattice's
//! tabs) can have a window of its own the same ways, dragged off the page's bar: the page shows another of its tabs.
//! The main window shows
//! the page it showed before, and the page's place on the rail is marked: a press there brings its window forward. The
//! window's Back button returns it to the main window and shows it there; closing the window returns it to the rail.
//!
//! Every window is the same app, with one state. The keys a page listens for follow the window the keyboard is in
//! (`App::keyed`); the main window's own keys (the palette, the keyboard's sheet, the rail, Ctrl+1 to Ctrl+9) bring the
//! main window forward and act there. A panel a page opens (the Models panel, a part's picker, Auto configure) opens
//! over the window that page is in.

use std::time::Instant;

use iced::widget::{button, column, container, mouse_area, row, space, text};
use iced::{Alignment, Background, Border, Element, Length, Point, Size, Task, mouse, window};

use crate::app::{App, Message};
use crate::catalogue::Section;
use crate::chrome::{self, Act};
use crate::theme::{self, fonts};

/// A page's window opens this large, or as large as the main window when that is smaller.
const SIZE: Size = Size::new(1180.0, 800.0);
/// No smaller than this: a narrow page's heading and one column still fit.
const MIN: Size = Size::new(560.0, 420.0);
/// The point of a page's window that lies under the pointer where a drag let go: on its bar, over the page's name.
const GRAB: Point = Point::new(140.0, 16.0);
/// A page's window opened by a button sits this far down and right of the main window's corner.
const CASCADE: f32 = 48.0;
/// How far the pointer moves, pressed on the rail, before the press is a drag.
const DRAG: f32 = 6.0;
/// A drag let go nearer the window's left edge than this is back on the rail (84 wide), and opens no window.
const RAIL_REACH: f32 = 120.0;
/// A tab let go less than this above or below where it was pressed is back on its bar, and opens no window.
const STRIP_REACH: f32 = 40.0;

/// A page's tab that can have a window of its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Lattice(crate::lattice::Tab),
    /// Transcription's tabs (Live captions, Dictation, PC audio, Audio files, Library).
    Words(crate::app::Tab),
    /// Compute's tabs (the simulator, a plug-in's tab, Training).
    Compute(crate::compute::Tab),
}

impl Part {
    /// The page the tab is on.
    pub fn section(self) -> Section {
        match self {
            Part::Lattice(_) => Section::Lattice,
            Part::Words(_) => Section::Transcription,
            Part::Compute(_) => Section::Compute,
        }
    }
}

/// What can have a window of its own: a page, or a page's tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum What {
    Page(Section),
    Part(Part),
}

impl What {
    pub fn section(self) -> Section {
        match self {
            What::Page(section) => section,
            What::Part(part) => part.section(),
        }
    }
}

/// A page in a window of its own.
#[derive(Clone, Debug)]
pub struct Popout {
    pub id: window::Id,
    pub section: Section,
    pub size: Size,
    pub maximised: bool,
    /// The bar's last press (a quick second one maximises or restores).
    pub bar_pressed: Option<Instant>,
    /// Its handle in the window system, once said: the Stage never captures it, and Sinai is told when it shows what
    /// Sinai sees.
    pub raw: Option<u64>,
    /// The page's tab it shows, when it shows one tab rather than the page.
    pub part: Option<Part>,
}

impl Popout {
    pub fn what(&self) -> What {
        self.part.map_or(What::Page(self.section), What::Part)
    }
}

/// A press on a page's place on the rail, or on a tab on a page's bar, which a move farther than [`DRAG`] makes a
/// drag; `window` is the window it began in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Drag {
    pub what: What,
    pub window: window::Id,
    /// Where the pointer was first seen after the press, and where it is now, in its window's coordinates (outside the
    /// window, past its edges: the window has the pointer until the button is let go).
    pub from: Option<Point>,
    pub at: Option<Point>,
    pub moving: bool,
}

impl Drag {
    pub fn new(what: What, window: window::Id) -> Drag {
        Drag { what, window, from: None, at: None, moving: false }
    }

    pub fn moved(&mut self, to: Point) {
        let from = *self.from.get_or_insert(to);
        self.at = Some(to);
        if from.distance(to) > DRAG {
            self.moving = true;
        }
    }

    /// Where the drag let go, when that was away from where it began, so a window opens there: a page's place off the
    /// rail (over the page, or outside the window, of `size`, over the desktop; a maximised window has no desktop beside
    /// it, so letting go over the page counts too, as a tab torn off a browser's strip does), a tab off its bar. None
    /// when it was a press, or it came back.
    pub fn let_go(&self, size: Size) -> Option<Point> {
        let at = self.at.filter(|_| self.moving)?;
        let in_window = at.x >= 0.0 && at.y >= 0.0 && at.x < size.width && at.y < size.height;
        let back = in_window
            && match self.what {
                What::Page(_) => at.x < RAIL_REACH,
                What::Part(_) => self.from.is_some_and(|from| (at.y - from.y).abs() < STRIP_REACH),
            };
        (!back).then_some(at)
    }
}

/// The pages in windows of their own, and what the windows are doing.
#[derive(Debug, Default)]
pub struct Windows {
    /// In the order they opened.
    pub open: Vec<Popout>,
    /// The window the keyboard is in, as the window system last said; None while no window of the app's has it.
    pub focus: Option<window::Id>,
    /// The window the message being handled came from (`Message::In`); None for the main window.
    pub acting: Option<window::Id>,
    /// The page's window a panel (the Models panel, a part's picker, Auto configure) opened over; None: the main window.
    pub panel_in: Option<window::Id>,
    /// The page's window files are dragged over; None: the main window.
    pub hovered: Option<window::Id>,
    /// The pages the main window showed, latest first: when the one on show leaves, the one before it shows.
    pub recent: Vec<Section>,
    /// The page's place on the rail or the tab under the pointer, and the window it is in; and a press on one.
    pub hover: Option<(What, window::Id)>,
    pub drag: Option<Drag>,
}

impl Windows {
    pub fn get(&self, id: window::Id) -> Option<&Popout> {
        self.open.iter().find(|p| p.id == id)
    }

    fn get_mut(&mut self, id: window::Id) -> Option<&mut Popout> {
        self.open.iter_mut().find(|p| p.id == id)
    }

    /// The window the page `section` is in, when it is in one of its own.
    pub fn of(&self, section: Section) -> Option<&Popout> {
        self.open.iter().find(|p| p.section == section && p.part.is_none())
    }

    /// The window a page's tab is in, when it is in one of its own.
    pub fn of_part(&self, part: Part) -> Option<&Popout> {
        self.open.iter().find(|p| p.part == Some(part))
    }

    /// Whether the page `section`, or one of its tabs, is in a window of its own.
    pub fn shows(&self, section: Section) -> bool {
        self.open.iter().any(|p| p.section == section)
    }

    pub fn is_out(&self, section: Section) -> bool {
        self.of(section).is_some()
    }

    fn take(&mut self, id: window::Id) -> Option<Popout> {
        let at = self.open.iter().position(|p| p.id == id)?;
        if self.panel_in == Some(id) {
            self.panel_in = None;
        }
        if self.hovered == Some(id) {
            self.hovered = None;
        }
        Some(self.open.remove(at))
    }

    /// The main window showed `section`.
    pub fn shown(&mut self, section: Section) {
        self.recent.retain(|s| *s != section);
        self.recent.insert(0, section);
        self.recent.truncate(Section::ALL.len());
    }

    /// The page the main window shows when `leaving` goes to a window of its own: the latest it showed that is still in
    /// it, else the first such on the rail. None when `leaving` is the last page in the main window (it stays).
    pub fn instead_of(&self, leaving: Section) -> Option<Section> {
        let stays = |s: &Section| *s != leaving && !self.is_out(*s);
        self.recent.iter().copied().find(stays).or_else(|| Section::ALL.into_iter().find(stays))
    }
}

#[derive(Clone, Debug)]
pub enum Msg {
    /// Open the page in a window of its own, a little down and right of the main window.
    PopOut(Section),
    /// A page's tab to a window of its own, a little down and right of the window its page is in.
    PopOutPart(Part),
    /// ... with the window's top left corner at this point of the screen (None: where the window system puts it).
    PopOutAt(What, Option<Point>),
    /// A tab in a window of its own: that window forward.
    RaisePart(Part),
    /// Return the page to the main window and show it there.
    Back(window::Id),
    /// The window system asked a window to close (Alt+F4), or a window closed.
    CloseRequested(window::Id),
    Closed(window::Id),
    /// A window gained (true) or lost the keyboard.
    Focused(window::Id, bool),
    /// A page's window's bar, buttons and edges.
    Chrome(window::Id, Act),
    Resized(window::Id, Size),
    Maximised(window::Id, bool),
    /// A page's window's handle in the window system.
    Raw(window::Id, u64),
    /// The pointer came onto (true) or left a page's place on the rail, or a tab on a page's bar.
    Hover(What, bool),
    /// The left button pressed or let go, or the pointer moved while a place on the rail was pressed, in a window.
    Pointer(window::Id, Pointer),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Pointer {
    Pressed,
    Moved(Point),
    Released,
}

/// A page's window: no Windows frame (its bar and edges are drawn, as the main window's are), at `at` when given.
fn settings(size: Size, at: Option<Point>) -> window::Settings {
    window::Settings {
        size,
        position: at.map(window::Position::Specific).unwrap_or_default(),
        min_size: Some(MIN),
        icon: crate::brand::window_icon(),
        decorations: false,
        // its close asks first (`Msg::CloseRequested`): the page returns to the rail as the window goes
        exit_on_close_request: false,
        #[cfg(windows)]
        platform_specific: window::settings::PlatformSpecific { undecorated_shadow: true, ..Default::default() },
        ..window::Settings::default()
    }
}

/// `id` forward: shown again when minimised, and given the keyboard.
pub fn raise<T: Send + 'static>(id: window::Id) -> Task<T> {
    window::minimize::<T>(id, false).chain(window::gain_focus(id))
}

impl App {
    /// The page window `id` shows: a page's own window's, or (for the main window, or none) the main window's.
    pub fn section_in(&self, id: Option<window::Id>) -> Section {
        id.and_then(|id| self.windows.get(id)).map_or(self.section, |p| p.section)
    }

    /// Whether the page `section` itself is on show (not only a tab of it in a window of its own).
    pub fn page_on_show(&self, section: Section) -> bool {
        self.section == section || self.windows.is_out(section)
    }

    /// Whether `section` is on show: in the main window, or in a window of its own.
    pub fn on_show(&self, section: Section) -> bool {
        self.section == section || self.windows.shows(section)
    }

    /// Lattice's tab in window `id` (None: the main window): a tab's own window's tab, the page's tab where the page
    /// is, or None where Lattice is not.
    pub fn lattice_tab_in(&self, id: Option<window::Id>) -> Option<crate::lattice::Tab> {
        match id.and_then(|id| self.windows.get(id)) {
            Some(Popout { part: Some(Part::Lattice(tab)), .. }) => Some(*tab),
            Some(p) if p.section == Section::Lattice => Some(self.lattice.tab),
            Some(_) => None,
            None => (self.section == Section::Lattice).then_some(self.lattice.tab),
        }
    }

    /// Whether Transcription's `tab` is on show: the page's tab where the page is, or in a window of its own.
    pub fn words_shown(&self, tab: crate::app::Tab) -> bool {
        let page = self.section == Section::Transcription || self.windows.is_out(Section::Transcription);
        (page && self.tab == tab) || self.words_out.contains(&tab)
    }

    /// The page in the window the keyboard is in (the main window's while no window of the app's has it): its keys
    /// listen, and no other page's do.
    pub fn keyed(&self) -> Section {
        self.section_in(self.windows.focus)
    }

    /// Whether the keyboard is in the main window (or in none of the app's windows).
    pub fn main_keyed(&self) -> bool {
        !self.windows.focus.is_some_and(|id| self.windows.get(id).is_some())
    }

    /// The page the message being handled came from: its window's, or the main window's.
    pub fn acting_section(&self) -> Section {
        self.section_in(self.windows.acting)
    }

    /// The width of the window `section` is drawn in (`ui::columns_for` lays a page's columns out by it).
    pub fn width_for(&self, section: Section) -> f32 {
        self.windows.of(section).map_or_else(|| self.window_width(), |p| p.size.width)
    }

    /// A message from a page's own window (`Message::In`): handled as from that window, and a panel the page opens
    /// opens over it.
    pub(crate) fn handle_in(&mut self, id: window::Id, message: Message) -> Task<Message> {
        let page_window = self.windows.get(id).is_some();
        // a page's window asking for another page (Ctrl+1 to Ctrl+9, or a link to it) that is in the main window: the
        // main window shows it, and comes forward
        let docked = |s: Option<Section>| s.is_some_and(|s| !self.windows.is_out(s));
        let to_main = page_window
            && match &message {
                Message::Shortcut(n) => docked(Section::ALL.get(*n).copied()),
                Message::Go(s) => docked(Some(*s)),
                _ => false,
            };
        let panel_was_open = self.panel_open();
        let main_was_wanted = self.main_window_wanted();
        let outer = self.windows.acting.replace(id);
        let task = self.update(message);
        self.windows.acting = outer;
        if page_window && !panel_was_open && self.panel_open() {
            self.windows.panel_in = Some(id);
        }
        // the main window's own keys, pressed in a page's window, act in the main window: it comes forward
        if page_window && (to_main || (!main_was_wanted && self.main_window_wanted())) {
            return Task::batch([task, self.raise_main()]);
        }
        task
    }

    /// One of the main window's own things opened (the palette, the keyboard's sheet, the rail's keyboard place).
    fn main_window_wanted(&self) -> bool {
        self.palette.is_some() || self.sheet || self.rail_focus.is_some()
    }

    fn raise_main(&self) -> Task<Message> {
        self.window_id.map_or_else(Task::none, raise)
    }

    /// A page's panel is open over the page (`view::page_panel`).
    pub fn panel_open(&self) -> bool {
        self.models.open || self.parts.open.is_some() || self.auto.open
    }

    pub(crate) fn windows_update(&mut self, msg: Msg) -> Task<Message> {
        match msg {
            Msg::PopOut(section) => {
                let Some(main) = self.window_id else { return Task::none() };
                window::position(main)
                    .map(move |at| Message::Windows(Msg::PopOutAt(What::Page(section), at.map(|p| Point::new(p.x + CASCADE, p.y + CASCADE)))))
            }
            Msg::PopOutPart(part) => {
                // beside the window its page is in, or the main window
                let Some(near) = self.windows.of(part.section()).map(|p| p.id).or(self.window_id) else { return Task::none() };
                window::position(near)
                    .map(move |at| Message::Windows(Msg::PopOutAt(What::Part(part), at.map(|p| Point::new(p.x + CASCADE, p.y + CASCADE)))))
            }
            Msg::PopOutAt(What::Page(section), at) => self.pop_out(section, at),
            Msg::PopOutAt(What::Part(part), at) => self.pop_out_part(part, at),
            Msg::RaisePart(part) => self.windows.of_part(part).map_or_else(Task::none, |p| raise(p.id)),
            Msg::Back(id) => {
                let Some(p) = self.windows.take(id) else { return Task::none() };
                if let Some(part) = p.part {
                    return Task::batch([window::close(id), self.part_back(part)]);
                }
                // the page shows in the main window (it never stopped showing: nothing is closed and nothing read)
                let was = self.section;
                self.section = p.section;
                self.windows.shown(p.section);
                if was != p.section {
                    self.leave(was);
                }
                self.tell_loop_which_window();
                Task::batch([window::close(id), self.raise_main()])
            }
            Msg::CloseRequested(id) if Some(id) == self.window_id => {
                // the main window closes as its close button closes it: friends are told first, and every window goes
                self.update(Message::Chrome(Act::Close))
            }
            Msg::CloseRequested(id) => self.close(id),
            // the main window gone some other way: the app goes with it (a daemon would run on without a window)
            Msg::Closed(id) if Some(id) == self.window_id => iced::exit(),
            Msg::Closed(id) => {
                // closed some other way: the page returns to the rail, a tab to its page's bar
                if let Some(p) = self.windows.take(id) {
                    self.returned(&p);
                }
                Task::none()
            }
            Msg::Focused(id, true) => {
                self.windows.focus = Some(id);
                Task::none()
            }
            Msg::Focused(id, false) => {
                if self.windows.focus == Some(id) {
                    self.windows.focus = None;
                }
                Task::none()
            }
            Msg::Chrome(id, act) => self.popout_chrome(id, act),
            Msg::Resized(id, size) => {
                if Some(id) == self.window_id {
                    return self.update(Message::Resized(size));
                }
                let Some(p) = self.windows.get_mut(id) else { return Task::none() };
                p.size = size;
                if p.section == Section::Pages {
                    self.pages.width = Some(size.width);
                }
                window::is_maximized(id).map(move |m| Message::Windows(Msg::Maximised(id, m)))
            }
            Msg::Maximised(id, m) => {
                if let Some(p) = self.windows.get_mut(id) {
                    p.maximised = m;
                }
                Task::none()
            }
            Msg::Raw(id, raw) => {
                if let Some(p) = self.windows.get_mut(id) {
                    p.raw = Some(raw);
                }
                // Sinai's page in this window: the loop is told it shows what Sinai sees
                self.tell_loop_which_window();
                Task::none()
            }
            Msg::Hover(what, true) => {
                // in the window the message came from: a page's own, or the main window
                if let Some(window) = self.windows.acting.or(self.window_id) {
                    self.windows.hover = Some((what, window));
                }
                Task::none()
            }
            Msg::Hover(what, false) => {
                if self.windows.hover.is_some_and(|(on, _)| on == what) {
                    self.windows.hover = None;
                }
                Task::none()
            }
            Msg::Pointer(id, pointer) => self.rail_pointer(id, pointer),
        }
    }

    /// `section` into a window of its own, its top left corner at `at`. A page already in one: that window comes
    /// forward. The main window's last page stays.
    fn pop_out(&mut self, section: Section, at: Option<Point>) -> Task<Message> {
        if !self.signin.through() {
            return Task::none();
        }
        if let Some(p) = self.windows.of(section) {
            return raise(p.id);
        }
        let instead = if self.section == section {
            match self.windows.instead_of(section) {
                Some(next) => Some(next),
                None => return Task::none(),
            }
        } else {
            None
        };
        let main = self.main_size();
        let size = Size::new(SIZE.width.min(main.width), SIZE.height.min(main.height));
        let (id, opened) = window::open(settings(size, at));
        self.windows.open.push(Popout { id, section, size, maximised: false, bar_pressed: None, raw: None, part: None });
        if section == Section::Pages {
            self.pages.width = Some(size.width);
        }
        let opened = opened.then(move |id| window::raw_id::<Message>(id).map(move |raw| Message::Windows(Msg::Raw(id, raw))));
        match instead {
            // the main window shows the page it showed before; the page that left goes on showing, so nothing of it
            // closes and nothing is read again
            Some(next) => Task::batch([opened, self.update(Message::Go(next))]),
            // a page not on show comes on show: it reads what opening it reads
            None => Task::batch([opened, self.enter(section)]),
        }
    }

    /// A page's tab into a window of its own, its top left corner at `at`. A tab already in one: that window comes
    /// forward. The page shows another of its tabs when it showed this one, and keeps one.
    fn pop_out_part(&mut self, part: Part, at: Option<Point>) -> Task<Message> {
        if !self.signin.through() {
            return Task::none();
        }
        if let Some(p) = self.windows.of_part(part) {
            return raise(p.id);
        }
        let section = part.section();
        let was_shown = self.on_show(section);
        let mut tasks = Vec::new();
        match part {
            Part::Lattice(tab) => {
                use crate::lattice::{Msg as L, Tab};
                let next = Tab::ALL.into_iter().find(|t| *t != tab && !self.lattice.out.contains(t));
                if self.lattice.tab == tab {
                    let Some(next) = next else { return Task::none() };
                    self.lattice.out.push(tab);
                    tasks.push(self.lattice.update(L::Tab(next)).map(Message::Lattice));
                } else {
                    self.lattice.out.push(tab);
                }
            }
            Part::Compute(tab) => {
                use crate::compute::Msg as C;
                let tabs = self.compute.tabs();
                let next = tabs.into_iter().find(|t| *t != tab && !self.compute.out.contains(t));
                if self.compute.tab == tab {
                    let Some(next) = next else { return Task::none() };
                    // marked out first, so the page's move to another tab neither stops it nor closes it
                    self.compute.out.push(tab);
                    tasks.push(self.compute.update(C::Tab(next)).map(Message::Compute));
                }
            }
            Part::Words(tab) => {
                use crate::app::Tab;
                let next = Tab::ALL.into_iter().find(|t| *t != tab && !self.words_out.contains(t));
                if self.tab == tab {
                    let Some(next) = next else { return Task::none() };
                    self.words_out.push(tab);
                    tasks.push(self.update(Message::Tab(next)));
                } else {
                    self.words_out.push(tab);
                }
            }
        }
        let main = self.main_size();
        let size = Size::new(SIZE.width.min(main.width), SIZE.height.min(main.height));
        let (id, opened) = window::open(settings(size, at));
        self.windows.open.push(Popout { id, section, size, maximised: false, bar_pressed: None, raw: None, part: Some(part) });
        tasks.push(opened.then(move |id| window::raw_id::<Message>(id).map(move |raw| Message::Windows(Msg::Raw(id, raw)))));
        // what the tab reads: the page opened first when it was on show nowhere (Lattice's opening reads every tab on
        // show; the Library reads its transcripts)
        let entered = if was_shown || matches!(part, Part::Compute(_)) { Task::none() } else { self.enter(section) };
        tasks.push(match part {
            Part::Lattice(tab) if was_shown => self.lattice.refresh_tab(tab).map(Message::Lattice),
            Part::Lattice(_) => entered,
            Part::Words(tab) => Task::batch([entered, if tab == crate::app::Tab::Library { self.load_library() } else { Task::none() }]),
            // a Compute tab runs and reads on its own; opening the page is not asked for (its tab on show would run)
            Part::Compute(tab) => self.compute.tab_out(tab).map(Message::Compute),
        });
        Task::batch(tasks)
    }

    /// Back from a tab's own window: the page shows the tab, wherever the page is, and that window comes forward.
    fn part_back(&mut self, part: Part) -> Task<Message> {
        self.part_returned(part);
        let shown = match part {
            Part::Lattice(tab) => self.lattice.update(crate::lattice::Msg::Tab(tab)).map(Message::Lattice),
            Part::Words(tab) => self.update(Message::Tab(tab)),
            Part::Compute(tab) => self.compute.update(crate::compute::Msg::Tab(tab)).map(Message::Compute),
        };
        let section = part.section();
        let forward = match self.windows.of(section) {
            Some(p) => raise(p.id),
            None if self.section == section => self.raise_main(),
            None => Task::batch([self.update(Message::Go(section)), self.raise_main()]),
        };
        Task::batch([shown, forward])
    }

    /// A tab out of its own window: its page's bar has it again.
    fn part_returned(&mut self, part: Part) {
        match part {
            Part::Lattice(tab) => self.lattice.out.retain(|t| *t != tab),
            Part::Words(tab) => self.words_out.retain(|t| *t != tab),
            Part::Compute(tab) => self.compute.tab_back(tab),
        }
    }

    /// A window of its own gone: a tab back on its page's bar, the page stopped if it shows nowhere now, and Sinai told
    /// where its page is.
    fn returned(&mut self, p: &Popout) {
        if let Some(part) = p.part {
            self.part_returned(part);
        }
        self.leave(p.section);
        self.tell_loop_which_window();
    }

    /// A page's window closes: its page returns to the rail (a tab to its page's bar).
    fn close(&mut self, id: window::Id) -> Task<Message> {
        if let Some(p) = self.windows.take(id) {
            self.returned(&p);
        }
        if !self.windows.is_out(Section::Pages) {
            self.pages.width = Some(self.window_width());
        }
        window::close(id)
    }

    fn popout_chrome(&mut self, id: window::Id, act: Act) -> Task<Message> {
        let Some(p) = self.windows.get_mut(id) else { return Task::none() };
        let act = match act {
            Act::Press => {
                let now = Instant::now();
                let act = chrome::press(p.bar_pressed, now);
                p.bar_pressed = matches!(act, Act::Press).then_some(now);
                act
            }
            other => other,
        };
        let maximised = p.maximised;
        match act {
            Act::Press => window::drag(id),
            Act::Minimise => window::minimize(id, true),
            Act::ToggleMaximise => {
                window::toggle_maximize(id).chain(window::is_maximized(id).map(move |m| Message::Windows(Msg::Maximised(id, m))))
            }
            Act::Close => self.close(id),
            Act::Back => self.windows_update(Msg::Back(id)),
            Act::Resize(direction) if !maximised => window::drag_resize(id, direction),
            Act::Resize(_) | Act::PopOut => Task::none(),
        }
    }

    /// A press on a page's place on the rail or on a tab, followed: let go away from where it began, what it carries
    /// opens in a window of its own there.
    fn rail_pointer(&mut self, id: window::Id, pointer: Pointer) -> Task<Message> {
        match pointer {
            Pointer::Pressed => {
                let through = self.signin.through();
                self.windows.drag = self.windows.hover.filter(|(_, on)| *on == id && through).map(|(what, on)| Drag::new(what, on));
                Task::none()
            }
            Pointer::Moved(to) => {
                if let Some(drag) = self.windows.drag.as_mut().filter(|d| d.window == id) {
                    drag.moved(to);
                }
                Task::none()
            }
            Pointer::Released => {
                let Some(drag) = self.windows.drag.take_if(|d| d.window == id) else { return Task::none() };
                let Some(at) = drag.let_go(self.size_of(id)) else { return Task::none() };
                let what = drag.what;
                window::position(id).map(move |corner| {
                    let at = corner.map(|c| Point::new(c.x + at.x - GRAB.x, c.y + at.y - GRAB.y));
                    Message::Windows(Msg::PopOutAt(what, at))
                })
            }
        }
    }

    /// The size of window `id`: a page's own, or the main window's.
    fn size_of(&self, id: window::Id) -> Size {
        self.windows.get(id).map_or_else(|| self.main_size(), |p| p.size)
    }

    /// Alelyon's own windows in the window system: the main window and each page's own. The Stage captures none of them.
    pub fn own_windows(&self) -> Vec<i64> {
        self.own_window.into_iter().chain(self.windows.open.iter().filter_map(|p| p.raw)).map(|h| h as i64).collect()
    }

    /// The window that shows what Sinai sees: the one Sinai's page is in (None until the window system has said).
    pub fn stage_window(&self) -> Option<u64> {
        match self.windows.of(Section::Sinai) {
            Some(p) => p.raw,
            None => self.own_window,
        }
    }

    /// The name of what a window shows: a page, or a page's tab after its page.
    pub fn what_title(&self, what: What) -> String {
        let tab = |section: Section, tab: String| format!("{} \u{00B7} {tab}", section.title());
        match what {
            What::Page(section) => section.title().to_string(),
            What::Part(Part::Lattice(t)) => tab(Section::Lattice, t.title().to_string()),
            What::Part(Part::Words(t)) => tab(Section::Transcription, t.title().to_string()),
            What::Part(Part::Compute(t)) => tab(Section::Compute, self.compute.title(t)),
        }
    }

    /// The title each window has (the taskbar shows it): a page's window is named for its page.
    pub fn title(&self, id: window::Id) -> String {
        match self.windows.get(id) {
            Some(p) => format!("{} \u{00B7} Alelyon", self.what_title(p.what())),
            None => "Alelyon".to_string(),
        }
    }
}

/// The windows' events the app follows: which has the keyboard, and closing.
pub fn window_events(event: iced::Event, _status: iced::event::Status, id: window::Id) -> Option<Message> {
    let iced::Event::Window(event) = event else { return None };
    let msg = match event {
        window::Event::Focused => Msg::Focused(id, true),
        window::Event::Unfocused => Msg::Focused(id, false),
        window::Event::CloseRequested => Msg::CloseRequested(id),
        window::Event::Closed => Msg::Closed(id),
        _ => return None,
    };
    Some(Message::Windows(msg))
}

/// The left button, while the pointer is over a place on the rail or one is pressed.
pub fn pointer_buttons(event: iced::Event, _status: iced::event::Status, id: window::Id) -> Option<Message> {
    match event {
        iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => Some(Message::Windows(Msg::Pointer(id, Pointer::Pressed))),
        iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => Some(Message::Windows(Msg::Pointer(id, Pointer::Released))),
        _ => None,
    }
}

/// The pointer's moves, while a place on the rail is pressed.
pub fn pointer_moves(event: iced::Event, _status: iced::event::Status, id: window::Id) -> Option<Message> {
    match event {
        iced::Event::Mouse(mouse::Event::CursorMoved { position }) => Some(Message::Windows(Msg::Pointer(id, Pointer::Moved(position)))),
        _ => None,
    }
}

/// A page's window's bar: the page's mark and name (a press drags the window, a quick second one maximises), then Back
/// to the main window, minimise, maximise and close.
pub fn bar<'a>(p: &Popout, title: String, act: impl Fn(Act) -> Message + Copy + 'a) -> Element<'a, Message> {
    let name = row![
        text(p.section.glyph()).size(15).font(fonts().ui).color(theme::GOLD),
        crate::ui::caps(title, 10.5, theme::GOLD),
    ]
    .spacing(9)
    .align_y(Alignment::Center);
    let grip = chrome::drag(container(name).padding(iced::padding::left(16)).center_y(chrome::BAR_H).width(Length::Fill), act);
    container(
        row![grip, container(chrome::compact(Some(Act::Back), p.maximised, act)).padding(iced::padding::right(8))]
            .align_y(Alignment::Center)
            .height(chrome::BAR_H),
    )
    .width(Length::Fill)
    .into()
}

/// Over the main window while a place on the rail is dragged: what letting go outside it does, by the pointer, and the
/// grabbing hand.
pub fn ghost<'a>(drag: &Drag, title: String) -> Option<Element<'a, Message>> {
    let at = drag.at.filter(|_| drag.moving)?;
    let chip = container(
        row![
            text(drag.what.section().glyph()).size(14).font(fonts().ui).color(theme::GOLD),
            text(match drag.what {
                What::Page(_) => format!("{title}: let go off the rail for a window of its own"),
                What::Part(_) => format!("{title}: let go off the bar for a window of its own"),
            })
                .size(12.5)
                .font(fonts().ui)
                .color(theme::TEXT),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    )
    .padding([6.0, 10.0])
    .style(|_| container::Style {
        background: Some(Background::Color(theme::SURFACE)),
        border: Border { color: theme::GOLD, width: 1.0, radius: 6.0.into() },
        ..container::Style::default()
    });
    let placed = column![space().height(Length::Fixed((at.y + 14.0).max(0.0))), row![space().width(Length::Fixed((at.x + 14.0).max(0.0))), chip]];
    Some(mouse_area(container(placed).width(Length::Fill).height(Length::Fill)).interaction(mouse::Interaction::Grabbing).into())
}

/// The pop-out mark beside a page's place on the rail while the page is in a window of its own.
pub const OUT_MARK: &str = "\u{29C9}";

/// The rail's button styled for a page in a window of its own: no wash, its words faint.
pub fn rail_out_style(_: &iced::Theme, status: button::Status) -> button::Style {
    button::Style {
        background: matches!(status, button::Status::Hovered | button::Status::Pressed).then_some(Background::Color(theme::WARM_HOVER)),
        text_color: theme::TEXT_FAINT,
        border: Border { color: theme::with_alpha(theme::GOLD, 0.30), width: 1.0, radius: 10.0.into() },
        ..button::Style::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Options;

    /// The app past signing in (a photograph's options skip it), its main window open.
    fn app() -> App {
        let (mut a, _) = App::boot(Options { screenshot: Some("x.png".into()), ..Options::default() });
        a.window_id = Some(window::Id::unique());
        a
    }

    /// `section` in a window of its own, as a drag out of the rail or the pop-out button ends; its window's id.
    fn out(a: &mut App, section: Section) -> window::Id {
        let _ = a.update(Message::Windows(Msg::PopOutAt(What::Page(section), None)));
        a.windows.of(section).expect("in a window of its own").id
    }

    #[test]
    fn a_page_popped_out_moves_and_the_main_window_shows_the_page_it_showed_before() {
        let mut a = app();
        let _ = a.update(Message::Go(Section::Fleet));
        let _ = a.update(Message::Go(Section::Data));
        let id = out(&mut a, Section::Data);
        assert_eq!(a.section, Section::Fleet, "the main window shows the page before");
        assert!(a.on_show(Section::Data), "the page goes on showing, in its own window");
        assert_eq!(a.section_in(Some(id)), Section::Data);
        assert_eq!(a.title(id), "Data \u{00B7} Alelyon", "the taskbar names it for its page");
        assert_eq!(a.title(a.window_id.unwrap()), "Alelyon");
        // a press on its place on the rail brings its window forward and moves nothing
        let _ = a.update(Message::Go(Section::Data));
        assert_eq!(a.section, Section::Fleet);
        assert_eq!(a.windows.open.len(), 1, "one page, one window: popping it out again only brings it forward");
        let _ = a.update(Message::Windows(Msg::PopOutAt(What::Page(Section::Data), None)));
        assert_eq!(a.windows.open.len(), 1);
    }

    #[test]
    fn back_shows_the_page_in_the_main_window_and_closing_its_window_returns_it_to_the_rail() {
        let mut a = app();
        let _ = a.update(Message::Go(Section::Fleet));
        let id = out(&mut a, Section::Research);
        let _ = a.update(Message::Windows(Msg::Back(id)));
        assert_eq!(a.section, Section::Research, "Back shows it in the main window");
        assert!(!a.windows.is_out(Section::Research));
        let id = out(&mut a, Section::Data);
        let _ = a.update(Message::Windows(Msg::CloseRequested(id)));
        assert!(!a.windows.is_out(Section::Data), "closed: back on the rail");
        assert_eq!(a.section, Section::Research, "closing a page's window leaves the main window where it is");
        assert!(!a.on_show(Section::Data));
        // a window closed some other way says so afterwards: its page returns all the same
        let id = out(&mut a, Section::Trust);
        let _ = a.update(Message::Windows(Msg::Closed(id)));
        assert!(!a.windows.is_out(Section::Trust));
    }

    #[test]
    fn the_main_window_keeps_one_page() {
        let mut a = app();
        let main = a.section;
        for section in Section::ALL.into_iter().filter(|s| *s != main) {
            assert!(a.windows.instead_of(main).is_some(), "{section:?} is still in the main window");
            let size = Size::new(800.0, 600.0);
            a.windows.open.push(Popout { id: window::Id::unique(), section, size, maximised: false, bar_pressed: None, raw: None, part: None });
        }
        assert!(a.windows.instead_of(main).is_none(), "every other page is out");
        let _ = a.update(Message::Windows(Msg::PopOutAt(What::Page(main), None)));
        assert!(!a.windows.is_out(main), "the last page stays in the main window");
        assert_eq!(a.section, main);
    }

    #[test]
    fn a_drag_off_the_rail_opens_its_page_where_it_is_let_go_and_a_press_opens_none() {
        let size = Size::new(1000.0, 700.0);
        let mut d = Drag::new(What::Page(Section::Fleet), window::Id::unique());
        d.moved(Point::new(40.0, 300.0));
        d.moved(Point::new(43.0, 302.0));
        assert_eq!(d.let_go(size), None, "a press that wobbled: not a drag");
        d.moved(Point::new(-120.0, 280.0));
        assert_eq!(d.let_go(size), Some(Point::new(-120.0, 280.0)), "over the desktop, left of the window");
        d.moved(Point::new(500.0, 300.0));
        assert_eq!(d.let_go(size), Some(Point::new(500.0, 300.0)), "over the page: a maximised window has no desktop beside it");
        d.moved(Point::new(60.0, 420.0));
        assert_eq!(d.let_go(size), None, "back on the rail: no window opens");
        d.moved(Point::new(60.0, 900.0));
        assert!(d.let_go(size).is_some(), "below the window, under the rail");
    }

    #[test]
    fn only_a_press_on_a_place_on_the_rail_is_followed() {
        let mut a = app();
        let main = a.window_id.unwrap();
        let press = |a: &mut App, p: Pointer| {
            let _ = a.update(Message::Windows(Msg::Pointer(main, p)));
        };
        press(&mut a, Pointer::Pressed);
        assert!(a.windows.drag.is_none(), "a press off the rail");
        let _ = a.update(Message::Windows(Msg::Hover(What::Page(Section::Data), true)));
        press(&mut a, Pointer::Pressed);
        assert_eq!(a.windows.drag.map(|d| (d.what, d.window)), Some((What::Page(Section::Data), main)));
        press(&mut a, Pointer::Moved(Point::new(40.0, 200.0)));
        press(&mut a, Pointer::Moved(Point::new(-60.0, 210.0)));
        assert!(a.windows.drag.is_some_and(|d| d.moving));
        press(&mut a, Pointer::Released);
        assert!(a.windows.drag.is_none(), "let go: the drag is over (its window opens once the main window's place is known)");
        // the pointer leaving one place after coming onto the next leaves the next under it
        let _ = a.update(Message::Windows(Msg::Hover(What::Page(Section::Fleet), true)));
        let _ = a.update(Message::Windows(Msg::Hover(What::Page(Section::Data), false)));
        assert_eq!(a.windows.hover, Some((What::Page(Section::Fleet), main)));
        // a press in another window is not a press on what the pointer is over here
        let other = window::Id::unique();
        let _ = a.update(Message::Windows(Msg::Pointer(other, Pointer::Pressed)));
        assert!(a.windows.drag.is_none());
    }

    #[test]
    fn a_pages_keys_listen_only_while_the_keyboard_is_in_its_window() {
        let mut a = app();
        let main = a.window_id.unwrap();
        let _ = a.update(Message::Go(Section::Fleet));
        let id = out(&mut a, Section::Data);
        let _ = a.update(Message::Windows(Msg::Focused(main, true)));
        assert_eq!(a.keyed(), Section::Fleet);
        assert!(a.main_keyed());
        let _ = a.update(Message::Windows(Msg::Focused(main, false)));
        let _ = a.update(Message::Windows(Msg::Focused(id, true)));
        assert_eq!(a.keyed(), Section::Data, "the keyboard in the page's window: its page's keys");
        assert!(!a.main_keyed());
        let _ = a.update(Message::Windows(Msg::Focused(id, false)));
        assert_eq!(a.keyed(), Section::Fleet, "no window of the app's has the keyboard: the main window's page");
        // a window that lost the keyboard after another gained it does not take it back
        let _ = a.update(Message::Windows(Msg::Focused(id, true)));
        let _ = a.update(Message::Windows(Msg::Focused(main, true)));
        let _ = a.update(Message::Windows(Msg::Focused(id, false)));
        assert!(a.main_keyed());
    }

    #[test]
    fn a_panel_a_page_opens_lies_over_the_window_it_was_opened_in() {
        let mut a = app();
        let main = a.window_id.unwrap();
        let _ = a.update(Message::Go(Section::Fleet));
        let id = out(&mut a, Section::Overview);
        let _ = a.update(Message::In(id, Box::new(Message::Models(crate::models::Msg::Open))));
        assert!(a.models.open);
        assert_eq!(a.windows.panel_in, Some(id), "over the page's own window");
        let _ = a.update(Message::Models(crate::models::Msg::Close));
        assert_eq!(a.windows.panel_in, None, "closed, it is no window's");
        let _ = a.update(Message::In(main, Box::new(Message::Models(crate::models::Msg::Open))));
        assert_eq!(a.windows.panel_in, None, "opened from the main window: over the main window");
        let _ = a.update(Message::Models(crate::models::Msg::Close));
        // its window closing takes the panel to the main window
        let _ = a.update(Message::In(id, Box::new(Message::Models(crate::models::Msg::Open))));
        let _ = a.update(Message::Windows(Msg::CloseRequested(id)));
        assert_eq!(a.windows.panel_in, None);
    }

    #[test]
    fn the_stage_leaves_out_every_window_of_alelyons_and_sinai_is_told_where_its_page_is() {
        let mut a = app();
        a.own_window = Some(7);
        assert_eq!(a.stage_window(), Some(7), "Sinai's page in the main window");
        let _ = a.update(Message::Go(Section::Fleet));
        let data = out(&mut a, Section::Data);
        let _ = a.update(Message::Windows(Msg::Raw(data, 11)));
        a.windows.open.push(Popout { id: window::Id::unique(), section: Section::Sinai, size: SIZE, maximised: false, bar_pressed: None, raw: None, part: None });
        assert_eq!(a.stage_window(), None, "its own window has not said its handle yet: nothing is named");
        let sinai = a.windows.of(Section::Sinai).unwrap().id;
        let _ = a.update(Message::Windows(Msg::Raw(sinai, 13)));
        assert_eq!(a.stage_window(), Some(13));
        assert_eq!(a.own_windows(), [7, 11, 13], "the Stage captures none of them");
        assert_eq!(crate::stage::on_show(None, &[11, 5, 13], &a.own_windows()), [5]);
    }

    /// Lattice's `tab` in a window of its own, as a drag off its bar or a middle press ends; its window's id.
    fn tab_out(a: &mut App, tab: crate::lattice::Tab) -> window::Id {
        let _ = a.update(Message::Windows(Msg::PopOutAt(What::Part(Part::Lattice(tab)), None)));
        a.windows.of_part(Part::Lattice(tab)).expect("in a window of its own").id
    }

    #[test]
    fn a_lattice_tab_popped_out_moves_and_its_page_shows_another_tab() {
        use crate::lattice::Tab;
        let mut a = app();
        let _ = a.update(Message::Go(Section::Lattice));
        assert_eq!(a.lattice.tab, Tab::Chat);
        let id = tab_out(&mut a, Tab::Chat);
        assert_eq!(a.lattice.out, [Tab::Chat]);
        assert_ne!(a.lattice.tab, Tab::Chat, "the page shows another tab, not the same one twice");
        assert_eq!(a.title(id), "Lattice \u{00B7} Chat and IDE \u{00B7} Alelyon");
        assert_eq!(a.section, Section::Lattice, "the page stays in the main window");
        assert!(!a.windows.is_out(Section::Lattice), "its tab is out, not the page");
        // asking the page for that tab brings its window forward and moves nothing
        let before = a.lattice.tab;
        let _ = a.update(Message::Lattice(crate::lattice::Msg::Tab(Tab::Chat)));
        assert_eq!(a.lattice.tab, before);
        assert!(a.lattice.raise_out.is_none(), "the window took the ask, and brought the tab's window forward");
        // Back: the page shows it
        let _ = a.update(Message::Windows(Msg::Back(id)));
        assert_eq!(a.lattice.tab, Tab::Chat);
        assert!(a.lattice.out.is_empty() && a.windows.of_part(Part::Lattice(Tab::Chat)).is_none());
    }

    #[test]
    fn a_tabs_window_closed_returns_it_and_its_page_keeps_one_tab() {
        use crate::lattice::Tab;
        let mut a = app();
        let _ = a.update(Message::Go(Section::Fleet));
        let id = tab_out(&mut a, Tab::Runs);
        assert!(a.on_show(Section::Lattice), "a tab out: Lattice is on show, so its work goes on");
        assert!(a.lattice.tab_shown(Tab::Runs), "its runs are followed while its window shows them");
        let _ = a.update(Message::Windows(Msg::CloseRequested(id)));
        assert!(a.lattice.out.is_empty() && !a.on_show(Section::Lattice));
        let shown = a.lattice.tab;
        for tab in Tab::ALL.into_iter().filter(|t| *t != shown) {
            tab_out(&mut a, tab);
        }
        let _ = a.update(Message::Windows(Msg::PopOutAt(What::Part(Part::Lattice(shown)), None)));
        assert!(a.windows.of_part(Part::Lattice(shown)).is_none(), "the page's last tab stays");
        assert_eq!(a.lattice.out.len(), Tab::ALL.len() - 1);
    }

    #[test]
    fn a_tab_dragged_off_its_bar_opens_where_let_go_and_one_let_go_along_the_bar_opens_none() {
        let size = Size::new(1000.0, 700.0);
        let mut d = Drag::new(What::Part(Part::Lattice(crate::lattice::Tab::Runs)), window::Id::unique());
        d.moved(Point::new(300.0, 20.0));
        d.moved(Point::new(340.0, 24.0));
        assert_eq!(d.let_go(size), None, "along the bar: back on it");
        d.moved(Point::new(360.0, 140.0));
        assert_eq!(d.let_go(size), Some(Point::new(360.0, 140.0)), "pulled down off the bar, over the page");
        d.moved(Point::new(60.0, 30.0));
        assert_eq!(d.let_go(size), None, "back on the bar, though near the rail: a tab is not a rail place");
        d.moved(Point::new(500.0, -90.0));
        assert!(d.let_go(size).is_some(), "above the window, over the desktop");
    }

    #[test]
    fn lattices_keys_and_esc_follow_the_window_that_shows_its_chat() {
        use crate::lattice::Tab;
        let mut a = app();
        let main = a.window_id.unwrap();
        let _ = a.update(Message::Go(Section::Lattice));
        let id = tab_out(&mut a, Tab::Chat);
        let _ = a.update(Message::Windows(Msg::Focused(id, true)));
        assert_eq!(a.lattice_tab_in(a.windows.focus), Some(Tab::Chat), "the chat's own window has the keyboard");
        let _ = a.update(Message::Windows(Msg::Focused(id, false)));
        let _ = a.update(Message::Windows(Msg::Focused(main, true)));
        assert_ne!(a.lattice_tab_in(a.windows.focus), Some(Tab::Chat), "the main window shows another of Lattice's tabs");
        let _ = a.update(Message::Go(Section::Fleet));
        assert_eq!(a.lattice_tab_in(Some(main)), None, "Lattice is not in the main window now");
        assert_eq!(a.lattice_tab_in(Some(id)), Some(Tab::Chat));
    }

    #[test]
    fn a_tab_is_dragged_from_whichever_window_its_bar_is_in() {
        let mut a = app();
        let _ = a.update(Message::Go(Section::Fleet));
        let lattice = out(&mut a, Section::Lattice);
        let part = What::Part(Part::Lattice(crate::lattice::Tab::Models));
        // Lattice's bar in the page's own window: the pointer over a tab there is followed there
        let _ = a.update(Message::In(lattice, Box::new(Message::Lattice(crate::lattice::Msg::Window(crate::lattice::TabWindow::Hover(crate::lattice::Tab::Models, true))))));
        assert_eq!(a.windows.hover, Some((part, lattice)));
        let _ = a.update(Message::Windows(Msg::Pointer(a.window_id.unwrap(), Pointer::Pressed)));
        assert!(a.windows.drag.is_none(), "a press in another window is not a press on it");
        let _ = a.update(Message::Windows(Msg::Pointer(lattice, Pointer::Pressed)));
        assert_eq!(a.windows.drag.map(|d| (d.what, d.window)), Some((part, lattice)));
    }

    #[test]
    fn a_transcription_tab_popped_out_moves_and_its_library_goes_on_being_watched() {
        use crate::app::Tab;
        let mut a = app();
        let _ = a.update(Message::Go(Section::Transcription));
        let _ = a.update(Message::Tab(Tab::Library));
        let library = What::Part(Part::Words(Tab::Library));
        let _ = a.update(Message::Windows(Msg::PopOutAt(library, None)));
        let id = a.windows.of_part(Part::Words(Tab::Library)).expect("in a window of its own").id;
        assert_eq!(a.words_out, [Tab::Library]);
        assert_ne!(a.tab, Tab::Library, "the page shows another of its tabs");
        assert_eq!(a.title(id), "Transcription \u{00B7} Library \u{00B7} Alelyon");
        // the main window goes elsewhere: the library in its own window is still on show, and watched
        let _ = a.update(Message::Go(Section::Fleet));
        assert!(a.on_show(Section::Transcription) && a.words_shown(Tab::Library));
        // the page's Library tab, asked for, brings its window forward rather than showing it twice
        let before = a.tab;
        let _ = a.update(Message::Tab(Tab::Library));
        assert_eq!(a.tab, before);
        // Back: Transcription shows it, in the main window
        let _ = a.update(Message::Windows(Msg::Back(id)));
        assert_eq!((a.section, a.tab), (Section::Transcription, Tab::Library));
        assert!(a.words_out.is_empty());
    }

    #[test]
    fn a_compute_tab_out_runs_on_its_own_and_the_page_it_left_does_not() {
        use crate::compute::Tab;
        let mut a = app();
        let _ = a.update(Message::Go(Section::Fleet));
        let part = Part::Compute(Tab::Training);
        let _ = a.update(Message::Windows(Msg::PopOutAt(What::Part(part), None)));
        let id = a.windows.of_part(part).expect("in a window of its own").id;
        assert_eq!(a.compute.out, [Tab::Training]);
        assert_eq!(a.title(id), "Compute and simulation \u{00B7} Training \u{00B7} Alelyon");
        assert!(a.compute.tab_shown(Tab::Training), "its stream is read while its window shows it");
        assert!(!a.compute.tab_shown(Tab::Simulator) && !a.compute.simulating(), "the page shows nowhere: its simulator does not run");
        // asking the page for that tab brings its window forward
        let _ = a.update(Message::Compute(crate::compute::Msg::Tab(Tab::Training)));
        assert_ne!(a.compute.tab, Tab::Training);
        assert!(a.compute.raise_out.is_none(), "the window took the ask");
        let _ = a.update(Message::Windows(Msg::CloseRequested(id)));
        assert!(a.compute.out.is_empty() && !a.compute.tab_shown(Tab::Training));
    }

    #[test]
    fn a_page_stops_when_the_page_shows_nowhere_though_one_of_its_tabs_is_out() {
        let mut a = app();
        let _ = a.update(Message::Go(Section::Lattice));
        let _ = a.update(Message::Windows(Msg::PopOutAt(What::Part(Part::Lattice(crate::lattice::Tab::Runs)), None)));
        let _ = a.update(Message::Go(Section::Fleet));
        assert!(a.on_show(Section::Lattice), "its tab's window keeps Lattice's work going");
        assert!(!a.page_on_show(Section::Lattice), "the page itself shows nowhere");
    }

    #[test]
    fn a_page_in_a_window_of_its_own_lays_its_columns_out_by_that_windows_width() {
        let mut a = app();
        let _ = a.update(Message::Go(Section::Fleet));
        let id = out(&mut a, Section::Trust);
        let _ = a.update(Message::Windows(Msg::Resized(id, Size::new(700.0, 500.0))));
        assert_eq!(a.width_for(Section::Trust), 700.0);
        assert_eq!(a.width_for(Section::Fleet), a.window_width(), "the main window's pages, by the main window's");
    }
}
