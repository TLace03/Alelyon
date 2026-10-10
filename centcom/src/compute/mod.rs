//! The Compute and simulation page: Sinai's simulator, watched as it runs on the processor (real frames from the simulator's own
//! renderer, since 2026-10-05), and a trainer's metrics stream. A build's plug-ins add tabs between the two (`Slot::ComputeTab`).
//!
//! The simulator runs on a thread of its own only while its tab is shown and not paused (`sim::Runner`); a plug-in's
//! tab is told when it comes on show and when it leaves (`open`, `close`). Leaving the page stops all of it.

pub mod sim;
// Asks Windows for a file's identity (volume and index) through its handle, so a replaced stream is told apart.
#[allow(unsafe_code)]
pub mod train;
mod view;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use alelyon_plugin_api::{self as plugin, Registry, Slot, TrainingDefault};
use iced::{Subscription, Task};

pub use view::view;

/// A tab of the page: the simulator, a plug-in's tab (its place in the page's list), or Training.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Simulator,
    Plugin(usize),
    Training,
}

impl Tab {
    /// The built-in tab `--tab` names: `simulator` or `training` (a plug-in's tab is named by its id).
    pub fn from_name(name: &str) -> Option<Tab> {
        match name {
            "simulator" => Some(Tab::Simulator),
            "training" => Some(Tab::Training),
            _ => None,
        }
    }
}

/// A plug-in's tab on the page: its id, its title, and its page.
pub struct Mounted {
    pub id: &'static str,
    pub title: &'static str,
    pub page: Box<dyn plugin::Section>,
}

/// The scene `--tab` names by its key (`humanoid`, `pile`, ...): its place in `sim::SCENES`.
pub fn scene_named(name: &str) -> Option<usize> {
    sim::SCENES.iter().position(|s| s.key == name)
}

/// The frame size the simulator draws.
pub const FRAME_W: u32 = 448;
pub const FRAME_H: u32 = 336;

/// Why the simulator has no scenes when Alelyon does not run from a checkout: the page shows the capability's empty
/// state for it (`capability::absent(SimulatorScenes)`), not this as an error.
pub const NO_CHECKOUT: &str = "Alelyon is not running from a checkout of the repository, so the simulator's scenes are not here.";

/// The checkout Alelyon runs from: the first folder above `start` holding `.git` (a worktree is its own checkout
/// here, unlike the issuer's home: the scenes read are the ones this build came with).
pub fn checkout_of(start: &Path) -> Option<PathBuf> {
    start.ancestors().find(|dir| dir.join(".git").exists()).map(Path::to_path_buf)
}

/// Threads the simulator's frames are drawn on: a few, so a frame is quick without taking the machine.
pub fn draw_threads() -> usize {
    std::thread::available_parallelism().map(|n| (n.get() / 8).clamp(1, 4)).unwrap_or(2)
}

pub struct State {
    pub repo: Result<PathBuf, String>,
    pub tab: Tab,
    /// The page is on show: nothing runs while it is not.
    shown: bool,
    pub scene: usize,
    /// The person wants the simulator running (it starts paused only when they paused it).
    pub playing: bool,
    runner: Option<sim::Runner>,
    pub frame: Option<Arc<sim::Frame>>,
    /// The newest frame as an image, made once per frame: a handle is a new texture each time one is made.
    pub picture: Option<iced::widget::image::Handle>,
    pub loaded: Option<sim::Loaded>,
    pub sim_failed: Option<String>,
    /// The plug-ins' tabs, in the registry's order.
    pub plugins: Vec<Mounted>,
    /// The catalogue rows the plug-ins bring to this page.
    pub features: Vec<crate::catalogue::Feature>,
    /// Where Training looks for streams, when the build names a place.
    pub training: Option<TrainingDefault>,
    /// The metrics streams on offer, the one being watched, and a path being typed.
    pub train_offered: Vec<train::Offered>,
    pub stream: Option<train::Stream>,
    pub train_typed: String,
}

#[derive(Clone, Debug)]
pub enum Msg {
    Tab(Tab),
    Scene(usize),
    Play,
    Pause,
    Restart,
    /// Take the simulator's newest frame.
    Pull,
    /// A message for a plug-in's tab (its place in the list).
    Plugin(usize, plugin::Message),
    TrainChoose(PathBuf),
    TrainTyped(String),
    TrainOpenTyped,
    TrainPoll,
}

impl State {
    /// The page with the Compute tabs `registry` carries (those the capabilities allow) and its training default.
    pub fn with(registry: &Registry, context: &crate::capability::Context<'_>) -> State {
        let mut state = State::new();
        for p in registry.in_slot(Slot::ComputeTab).filter(|p| crate::capability::plugin_shown(p.id, context)) {
            state.plugins.push(Mounted { id: p.id, title: p.title, page: p.make() });
            state.features.extend(p.feature.as_ref().map(crate::catalogue::Feature::from_plugin));
        }
        state.training = registry.training_default();
        state
    }

    /// The tab `--tab` names: a built-in one, or a plug-in's by its id.
    pub fn tab_named(&self, name: &str) -> Option<Tab> {
        Tab::from_name(name).or_else(|| self.plugins.iter().position(|p| p.id == name).map(Tab::Plugin))
    }

    /// The tabs in the order they are shown: the simulator, the plug-ins', then Training.
    pub fn tabs(&self) -> Vec<Tab> {
        let mut tabs = vec![Tab::Simulator];
        tabs.extend((0..self.plugins.len()).map(Tab::Plugin));
        tabs.push(Tab::Training);
        tabs
    }

    pub fn title(&self, tab: Tab) -> String {
        match tab {
            Tab::Simulator => "The simulator".into(),
            Tab::Plugin(i) => self.plugins.get(i).map(|p| p.title).unwrap_or("?").into(),
            Tab::Training => "Training".into(),
        }
    }

    /// The folder of streams Training offers, when the build names one.
    pub fn results(&self) -> Option<PathBuf> {
        Some(self.repo.as_ref().ok()?.join(self.training?.folder))
    }

    pub fn new() -> State {
        let repo = std::env::current_exe().map_err(|e| format!("where Alelyon runs from is unknown: {e}")).and_then(|exe| {
            checkout_of(&exe).ok_or_else(|| NO_CHECKOUT.to_string())
        });
        State {
            repo,
            tab: Tab::Simulator,
            shown: false,
            scene: 0,
            playing: true,
            runner: None,
            frame: None,
            picture: None,
            loaded: None,
            sim_failed: None,
            plugins: Vec::new(),
            features: Vec::new(),
            training: None,
            train_offered: Vec::new(),
            stream: None,
            train_typed: String::new(),
        }
    }

    /// The page opened: start what its tab shows.
    pub fn open(&mut self) -> Task<Msg> {
        self.shown = true;
        self.start_if_wanted();
        self.read_for_tab()
    }

    /// What a tab reads when it comes on show: a plug-in's tab is told it opened; Training reads the streams on offer
    /// and the watched stream at once.
    fn read_for_tab(&mut self) -> Task<Msg> {
        match self.tab {
            Tab::Plugin(i) => match self.plugins.get_mut(i) {
                Some(p) => p.page.open().map(move |m| Msg::Plugin(i, m)),
                None => Task::none(),
            },
            Tab::Training => {
                if self.repo.is_err() {
                    return Task::none();
                }
                self.train_offered = self.results().map(|dir| train::offered(&dir)).unwrap_or_default();
                match self.stream.as_mut() {
                    Some(s) => s.poll(),
                    // Nothing chosen yet: watch the newest stream on offer (reading a file is all it does).
                    None => {
                        if let Some(newest) = self.train_offered.first() {
                            let mut s = train::Stream::new(newest.path.clone());
                            s.poll();
                            self.stream = Some(s);
                        }
                    }
                }
                Task::none()
            }
            _ => Task::none(),
        }
    }

    /// The page closed: stop the simulator (its thread ends after the frame it is drawing), and tell a plug-in's tab
    /// on show that it left.
    pub fn close(&mut self) {
        self.shown = false;
        self.runner = None;
        self.close_plugin_tab();
    }

    fn close_plugin_tab(&mut self) {
        if let Tab::Plugin(i) = self.tab {
            if let Some(p) = self.plugins.get_mut(i) {
                p.page.close();
            }
        }
    }

    fn start_if_wanted(&mut self) {
        if !(self.shown && self.tab == Tab::Simulator && self.playing) || self.runner.is_some() {
            return;
        }
        let Ok(repo) = &self.repo else { return };
        self.sim_failed = None;
        match sim::Runner::start(repo.join(sim::SCENES[self.scene].path), FRAME_W, FRAME_H, draw_threads()) {
            Ok(runner) => self.runner = Some(runner),
            Err(why) => self.sim_failed = Some(why),
        }
    }

    fn restart(&mut self) {
        self.runner = None;
        self.frame = None;
        self.picture = None;
        self.loaded = None;
        self.start_if_wanted();
    }

    /// Loading a scene, or a plug-in's tab at work: the spinners turn.
    pub fn busy(&self) -> bool {
        self.plugins.iter().any(|p| p.page.busy()) || (self.runner.is_some() && self.frame.is_none() && self.sim_failed.is_none())
    }

    pub fn update(&mut self, msg: Msg) -> Task<Msg> {
        match msg {
            Msg::Tab(tab) => {
                if tab != self.tab && self.shown {
                    self.close_plugin_tab();
                }
                self.tab = tab;
                if tab == Tab::Simulator {
                    self.start_if_wanted();
                } else {
                    self.runner = None;
                }
                return self.read_for_tab();
            }
            Msg::Scene(i) if i < sim::SCENES.len() => {
                self.scene = i;
                self.playing = true;
                self.restart();
            }
            Msg::Scene(_) => {}
            Msg::Play => {
                self.playing = true;
                self.restart();
            }
            Msg::Pause => {
                self.playing = false;
                self.runner = None;
            }
            Msg::Restart => {
                self.playing = true;
                self.restart();
            }
            Msg::Pull => {
                if let Some(runner) = &self.runner {
                    if let Some(frame) = runner.latest() {
                        if !self.frame.as_ref().is_some_and(|f| Arc::ptr_eq(f, &frame)) {
                            self.picture = Some(iced::widget::image::Handle::from_rgba(frame.width, frame.height, frame.rgba.clone()));
                            self.frame = Some(frame);
                        }
                    }
                    if self.loaded.is_none() {
                        self.loaded = runner.loaded();
                    }
                    if let Some(why) = runner.failed() {
                        self.sim_failed = Some(why);
                        self.runner = None;
                    }
                }
            }
            Msg::Plugin(i, message) => {
                if let Some(p) = self.plugins.get_mut(i) {
                    return p.page.update(message).map(move |m| Msg::Plugin(i, m));
                }
            }
            Msg::TrainChoose(path) => {
                let mut stream = train::Stream::new(path);
                stream.poll();
                self.stream = Some(stream);
            }
            Msg::TrainTyped(words) => self.train_typed = words,
            Msg::TrainOpenTyped => {
                let path = PathBuf::from(self.train_typed.trim().trim_matches('"'));
                if !path.as_os_str().is_empty() {
                    return self.update(Msg::TrainChoose(path));
                }
            }
            Msg::TrainPoll => {
                if let Some(s) = self.stream.as_mut() {
                    s.poll();
                }
            }
        }
        Task::none()
    }

    /// The page's timers, held only while it is on show and something plays.
    pub fn subscription(&self) -> Subscription<Msg> {
        let mut subs = Vec::new();
        if self.shown && self.runner.is_some() {
            subs.push(iced::time::every(sim::FRAME_EVERY).map(|_| Msg::Pull));
        }
        // A plug-in's tab holds its timers only while it needs them (it is told when it is on show).
        for (i, p) in self.plugins.iter().enumerate() {
            subs.push(p.page.subscription().with(i).map(|(i, m)| Msg::Plugin(i, m)));
        }
        // A stream is read again only while its tab is on show and it may still grow, and only when its file changed
        // (`live`): a trainer that pauses makes no message (a two-second timer redrew the page for nothing).
        let growing = self.stream.as_ref().filter(|s| matches!(s.status, train::Status::Waiting | train::Status::Running | train::Status::Incomplete));
        if self.shown
            && self.tab == Tab::Training
            && let Some(stream) = growing
        {
            let watch = crate::live::Watch::new("training-stream", vec![("the stream".into(), crate::live::Source::File(stream.path.clone()))])
                .gap(train::POLL_EVERY);
            subs.push(watch.subscription().map(|_| Msg::TrainPoll));
        }
        Subscription::batch(subs)
    }
}

impl Default for State {
    fn default() -> State {
        State::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_runs_while_the_page_is_not_shown_or_its_simulator_is_paused() {
        let mut state = State::new();
        assert!(state.runner.is_none(), "a new page starts nothing");
        let _ = state.open();
        assert!(state.runner.is_some(), "the simulator starts when its tab is shown");
        let _ = state.update(Msg::Pause);
        assert!(state.runner.is_none() && !state.playing);
        let _ = state.update(Msg::Play);
        assert!(state.runner.is_some());
        let _ = state.update(Msg::Tab(Tab::Training));
        assert!(state.runner.is_none(), "another tab stops the simulator");
        let _ = state.update(Msg::Tab(Tab::Simulator));
        assert!(state.runner.is_some());
        state.close();
        assert!(state.runner.is_none(), "leaving the page stops it");
        assert!(state.subscription_is_quiet());
    }

    /// A plug-in's tab that counts what it is told.
    #[derive(Default)]
    struct Probe {
        shown: bool,
        opened: u32,
        added: u32,
    }

    #[derive(Clone, Debug)]
    struct Add(u32);

    impl plugin::Page for Probe {
        type Msg = Add;

        fn open(&mut self) -> Task<Add> {
            self.shown = true;
            self.opened += 1;
            Task::none()
        }

        fn close(&mut self) {
            self.shown = false;
        }

        fn update(&mut self, Add(n): Add) -> Task<Add> {
            self.added += n;
            Task::none()
        }

        fn view(&self, _: f32) -> iced::Element<'_, Add> {
            iced::widget::space().into()
        }

        fn busy(&self) -> bool {
            self.shown
        }
    }

    #[test]
    fn the_public_page_has_the_simulator_and_training_and_no_training_folder() {
        let public = State::with(&Registry::default(), &crate::capability::Context { signed_in: false, plugins: &[], dev_checkout: true });
        assert_eq!(public.tabs(), [Tab::Simulator, Tab::Training]);
        assert!(public.plugins.is_empty() && public.features.is_empty() && public.training.is_none() && public.results().is_none());
        assert_eq!(public.tab_named("training"), Some(Tab::Training));
        assert_eq!(public.tab_named("emulator"), None, "a plug-in's tab is absent from a build without it");
    }

    #[test]
    fn a_plugins_tab_sits_between_the_simulator_and_training_and_is_told_when_it_is_shown() {
        let registry = alelyon_plugin_api::Registry::new()
            .with(plugin::Plugin::new("probe", "Probe", Slot::ComputeTab, || Box::new(Probe::default())))
            .with(plugin::Plugin::new("card", "Card", Slot::TrustCard, || Box::new(Probe::default())));
        let ids = crate::capability::plugin_ids(&registry);
        let mut state = State::with(&registry, &crate::capability::Context { signed_in: false, plugins: &ids, dev_checkout: false });
        assert_eq!(state.tabs(), [Tab::Simulator, Tab::Plugin(0), Tab::Training], "a Trust card is not a Compute tab");
        assert_eq!((state.tab_named("probe"), state.title(Tab::Plugin(0)).as_str()), (Some(Tab::Plugin(0)), "Probe"));
        let _ = state.open();
        assert!(!state.plugins[0].page.busy(), "a tab not on show is not told it opened");
        let _ = state.update(Msg::Tab(Tab::Plugin(0)));
        assert!(state.plugins[0].page.busy() && state.runner.is_none(), "chosen, it opens; the simulator stops");
        let _ = state.update(Msg::Plugin(0, plugin::Message::new(Add(2))));
        let _ = state.update(Msg::Plugin(0, plugin::Message::new("another page's message")));
        let _ = state.update(Msg::Plugin(7, plugin::Message::new(Add(1))));
        let _ = state.update(Msg::Tab(Tab::Training));
        assert!(!state.plugins[0].page.busy(), "another tab closes it");
        let _ = state.update(Msg::Tab(Tab::Plugin(0)));
        state.close();
        assert!(!state.plugins[0].page.busy(), "leaving the page closes it");
    }

    impl State {
        fn subscription_is_quiet(&self) -> bool {
            !self.shown && self.runner.is_none()
        }
    }
}
