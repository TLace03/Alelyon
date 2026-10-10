//! The plug-in registry of the Alelyon desktop app.
//!
//! The app reads a [`Registry`] when it starts. Each [`Plugin`] in it names a [`Slot`] (a tab of the Compute page, a
//! card on the Trust page, or a whole section of the window), a title, the catalogue row it brings, and how to make its
//! [`Section`]: a state with its own messages, drawn with the app's toolkit. The public app runs with [`Registry::default`], which holds nothing; a
//! wrapper crate that links further sections in builds its own registry and passes it to the app's `run`.
//!
//! A plug-in writes its page as a [`Page`], with a message type of its own; the app holds it as a `Box<dyn Section>`
//! and carries its messages type-erased ([`Message`]), so the app never names a plug-in's types. There is no dynamic
//! loading: what a build carries is decided when it is linked.
//!
//! A build can also carry what is not a page: the receipt verifier's replay substrate ([`Registry::replay`]), and
//! where the programs the public app does not ship are found ([`Processes`]: Sinai's mind, voice enrolment's tools),
//! or its own rule for one the app finds itself (the speech engine). Each is a plug-in id: [`Registry::ids`].

use std::any::Any;
use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;

pub use alelyon_verify::ReplayKernel;
pub use iced;
use iced::{Element, Subscription, Task};

/// A plug-in's message, type-erased so the app can carry it without naming the plug-in's type. Cloning is cheap (the
/// value is shared).
#[derive(Clone)]
pub struct Message(Arc<dyn Any + Send + Sync>);

impl Message {
    pub fn new<T: Any + Send + Sync>(message: T) -> Message {
        Message(Arc::new(message))
    }

    /// The message as `T`, when it is one.
    pub fn get<T: Any>(&self) -> Option<&T> {
        self.0.downcast_ref::<T>()
    }
}

impl fmt::Debug for Message {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("plug-in message")
    }
}

/// A plug-in's page, as the app holds it: driven by type-erased messages. Write a [`Page`] instead; every `Page` is a
/// `Section`.
pub trait Section {
    /// The page came on show (its tab was chosen, or its page opened): start what it shows.
    fn open(&mut self) -> Task<Message>;
    /// The window started on this page (`--section`): start what it shows. Unless the page says otherwise, as
    /// [`open`](Section::open).
    fn start(&mut self) -> Task<Message> {
        self.open()
    }
    /// The page was asked to show its tab called `tab` (`--tab`); whether it has one.
    fn select(&mut self, tab: &str) -> bool {
        let _ = tab;
        false
    }
    /// The page left the screen: stop what runs only while it is shown.
    fn close(&mut self);
    /// A message for this page; one of another type is ignored.
    fn update(&mut self, message: Message) -> Task<Message>;
    /// The page, drawn. `phase` is the spinners' angle.
    fn view(&self, phase: f32) -> Element<'_, Message>;
    /// The page's timers and streams. The app asks for them whatever is on show, so a page holds a timer only while
    /// it needs one (between `open` and `close`, or while work it started runs).
    fn subscription(&self) -> Subscription<Message>;
    /// Work is under way: the app's spinners turn.
    fn busy(&self) -> bool;
}

/// A plug-in's page with its own message type.
pub trait Page: 'static {
    type Msg: Clone + fmt::Debug + Send + Sync + 'static;

    fn open(&mut self) -> Task<Self::Msg> {
        Task::none()
    }
    /// The window started on this page; [`Page::open`] unless the page says otherwise.
    fn start(&mut self) -> Task<Self::Msg> {
        self.open()
    }
    /// Show the tab called `tab`, if the page has one.
    fn select(&mut self, tab: &str) -> bool {
        let _ = tab;
        false
    }
    fn close(&mut self) {}
    fn update(&mut self, message: Self::Msg) -> Task<Self::Msg>;
    fn view(&self, phase: f32) -> Element<'_, Self::Msg>;
    fn subscription(&self) -> Subscription<Self::Msg> {
        Subscription::none()
    }
    fn busy(&self) -> bool {
        false
    }
}

impl<P: Page> Section for P {
    fn open(&mut self) -> Task<Message> {
        Page::open(self).map(Message::new)
    }

    fn start(&mut self) -> Task<Message> {
        Page::start(self).map(Message::new)
    }

    fn select(&mut self, tab: &str) -> bool {
        Page::select(self, tab)
    }

    fn close(&mut self) {
        Page::close(self)
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message.get::<P::Msg>() {
            Some(m) => Page::update(self, m.clone()).map(Message::new),
            None => Task::none(),
        }
    }

    fn view(&self, phase: f32) -> Element<'_, Message> {
        Page::view(self, phase).map(Message::new)
    }

    fn subscription(&self) -> Subscription<Message> {
        Page::subscription(self).map(Message::new::<P::Msg>)
    }

    fn busy(&self) -> bool {
        Page::busy(self)
    }
}

/// Where a plug-in's page goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Slot {
    /// A tab of the Compute page, after the simulator's and before Training.
    ComputeTab,
    /// A card on the Trust page, after Verify.
    TrustCard,
    /// A whole section of the window: the one its plug-in's id names (the section's short name, such as `fleet`). A
    /// build without one shows that section's empty state.
    SectionPage,
}

/// How far a feature has moved into the app, as the app's catalogue says it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Here {
    Now,
    InPart,
    Later,
}

/// A row of the app's catalogue of features that a plug-in brings: shown on its section's page with the rest.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Feature {
    pub id: &'static str,
    /// The section's key in the catalogue: `"trust"`, `"compute"`, ...
    pub section: &'static str,
    pub name: &'static str,
    pub what: &'static str,
    pub today: &'static str,
    pub no_screen: bool,
    pub plan: &'static str,
    pub step: &'static str,
    pub here: Here,
    /// The built-in row this one is listed before; None lists it last.
    pub before: Option<&'static str>,
}

/// One plug-in: what it is called, where it goes, and how its page is made.
#[derive(Clone)]
pub struct Plugin {
    /// A stable name: the capability it unlocks and, for a Compute tab, the `--tab` that opens it.
    pub id: &'static str,
    pub title: &'static str,
    pub slot: Slot,
    pub feature: Option<Feature>,
    /// The tabs of its page that `--tab` may name (see [`Section::select`]).
    pub tabs: &'static [&'static str],
    make: Arc<dyn Fn() -> Box<dyn Section> + Send + Sync>,
}

impl Plugin {
    pub fn new(id: &'static str, title: &'static str, slot: Slot, make: impl Fn() -> Box<dyn Section> + Send + Sync + 'static) -> Plugin {
        Plugin { id, title, slot, feature: None, tabs: &[], make: Arc::new(make) }
    }

    pub fn with_feature(mut self, feature: Feature) -> Plugin {
        self.feature = Some(feature);
        self
    }

    /// The tabs of its page that `--tab` may name.
    pub fn with_tabs(mut self, tabs: &'static [&'static str]) -> Plugin {
        self.tabs = tabs;
        self
    }

    /// A new page for this plug-in (the app makes one when it starts).
    pub fn make(&self) -> Box<dyn Section> {
        (self.make)()
    }
}

impl fmt::Debug for Plugin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Plugin").field("id", &self.id).field("title", &self.title).field("slot", &self.slot).finish_non_exhaustive()
    }
}

/// Where the Training tab looks for metrics streams when nothing else is chosen, and the trainer that writes them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TrainingDefault {
    /// A folder under the checkout the app runs from.
    pub folder: &'static str,
    /// The trainer that writes the streams, named in the tab's note.
    pub trainer: &'static str,
}

/// The plug-in id of the receipt verifier's replay substrate.
pub const REPLAY: &str = "replay";
/// The plug-in id of where Sinai's mind (its loop) is reached.
pub const SINAI_MIND: &str = "sinai-mind";
/// The plug-in id of where the speech engine's program is.
pub const EARS: &str = "ears";
/// The plug-in id of where voice enrolment's tools are.
pub const VOICE: &str = "voice";
/// The plug-in id of the Fleet section's page ([`Slot::SectionPage`]).
pub const FLEET: &str = "fleet";

/// How a build finds something on this PC, asked when it is needed: what was found, or why not, in words.
pub type Locate<T> = Arc<dyn Fn() -> Result<T, String> + Send + Sync>;

/// Voice enrolment's tools as a build finds them: Sinai's state home, the interpreter that runs the probe's scripts,
/// the scripts' folder, the synthetic voices' models and the speaker model.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VoiceTools {
    pub home: PathBuf,
    pub python: PathBuf,
    pub scripts: PathBuf,
    pub models: PathBuf,
    pub speaker_model: PathBuf,
}

/// Where the programs the public app does not ship are found. The app's clients for them (Sinai's page, voice
/// enrolment) are public; a build without a locator shows that client's empty state. The speech engine is open and
/// the app finds it itself (beside its own program); a locator here replaces that rule for one build.
#[derive(Clone, Default)]
pub struct Processes {
    /// Sinai's mind: the loop's WebSocket the window connects to.
    pub sinai_mind: Option<Locate<String>>,
    /// The speech engine: the program the window starts with `serve`, when a build finds it by a rule of its own.
    pub ears: Option<Locate<PathBuf>>,
    /// Voice enrolment's tools.
    pub voice: Option<Locate<VoiceTools>>,
}

impl fmt::Debug for Processes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Processes")
            .field("sinai_mind", &self.sinai_mind.is_some())
            .field("ears", &self.ears.is_some())
            .field("voice", &self.voice.is_some())
            .finish()
    }
}

/// Everything a build adds to the app. The default holds nothing: the public app.
#[derive(Clone, Default)]
pub struct Registry {
    plugins: Vec<Plugin>,
    owner: bool,
    training: Option<TrainingDefault>,
    replay: Option<Arc<dyn ReplayKernel>>,
    processes: Processes,
}

impl fmt::Debug for Registry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Registry")
            .field("plugins", &self.plugins)
            .field("owner", &self.owner)
            .field("training", &self.training)
            .field("replay", &self.replay.as_ref().map(|k| k.id().to_owned()))
            .field("processes", &self.processes)
            .finish()
    }
}

impl Registry {
    pub fn new() -> Registry {
        Registry::default()
    }

    /// Adds a plug-in. Two with one id are a mistake in the build, refused at once, as is a page that takes an id
    /// kept for what is not a page.
    pub fn with(mut self, plugin: Plugin) -> Registry {
        assert!(!self.has(plugin.id), "two plug-ins are named {:?}", plugin.id);
        assert!(![REPLAY, SINAI_MIND, EARS, VOICE].contains(&plugin.id), "{:?} is kept for what is not a page", plugin.id);
        self.plugins.push(plugin);
        self
    }

    /// Marks the build as a developer's own build, never handed to users.
    pub fn owner(mut self, owner: bool) -> Registry {
        self.owner = owner;
        self
    }

    pub fn training(mut self, training: TrainingDefault) -> Registry {
        self.training = Some(training);
        self
    }

    /// The substrate the receipt verifier replays on. Without one, receipts are checked but not replayed: their
    /// replay checks are reported not performed, and no receipt passes.
    pub fn replay(mut self, kernel: Arc<dyn ReplayKernel>) -> Registry {
        self.replay = Some(kernel);
        self
    }

    /// Where Sinai's mind is reached.
    pub fn sinai_mind(mut self, locate: impl Fn() -> Result<String, String> + Send + Sync + 'static) -> Registry {
        self.processes.sinai_mind = Some(Arc::new(locate));
        self
    }

    /// Where the speech engine's program is, by this build's own rule (the app has a built-in one).
    pub fn ears(mut self, locate: impl Fn() -> Result<PathBuf, String> + Send + Sync + 'static) -> Registry {
        self.processes.ears = Some(Arc::new(locate));
        self
    }

    /// Where voice enrolment's tools are.
    pub fn voice(mut self, locate: impl Fn() -> Result<VoiceTools, String> + Send + Sync + 'static) -> Registry {
        self.processes.voice = Some(Arc::new(locate));
        self
    }

    pub fn replay_kernel(&self) -> Option<Arc<dyn ReplayKernel>> {
        self.replay.clone()
    }

    pub fn processes(&self) -> &Processes {
        &self.processes
    }

    /// Every plug-in id this build carries: its pages' and, for what it adds that is not a page, [`REPLAY`],
    /// [`SINAI_MIND`], [`EARS`] and [`VOICE`].
    pub fn ids(&self) -> Vec<&'static str> {
        let mut ids: Vec<&'static str> = self.plugins.iter().map(|p| p.id).collect();
        let p = &self.processes;
        for (id, carried) in
            [(REPLAY, self.replay.is_some()), (SINAI_MIND, p.sinai_mind.is_some()), (EARS, p.ears.is_some()), (VOICE, p.voice.is_some())]
        {
            if carried {
                ids.push(id);
            }
        }
        ids
    }

    pub fn plugins(&self) -> &[Plugin] {
        &self.plugins
    }

    pub fn in_slot(&self, slot: Slot) -> impl Iterator<Item = &Plugin> {
        self.plugins.iter().filter(move |p| p.slot == slot)
    }

    pub fn has(&self, id: &str) -> bool {
        self.plugins.iter().any(|p| p.id == id)
    }

    pub fn is_owner(&self) -> bool {
        self.owner
    }

    pub fn training_default(&self) -> Option<TrainingDefault> {
        self.training
    }

    /// Nothing added: the public app.
    pub fn is_empty(&self) -> bool {
        self.plugins.is_empty() && !self.owner && self.training.is_none() && self.ids().is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Counter(u32);

    #[derive(Clone, Debug)]
    enum Msg {
        Add(u32),
    }

    impl Page for Counter {
        type Msg = Msg;

        fn update(&mut self, message: Msg) -> Task<Msg> {
            let Msg::Add(n) = message;
            self.0 += n;
            Task::none()
        }

        fn view(&self, _: f32) -> Element<'_, Msg> {
            iced::widget::space().into()
        }
    }

    #[test]
    fn the_default_registry_is_the_public_app_and_holds_nothing() {
        let r = Registry::default();
        assert!(r.is_empty() && r.plugins().is_empty() && !r.is_owner() && r.training_default().is_none());
        assert!(r.replay_kernel().is_none() && r.ids().is_empty());
        let p = r.processes();
        assert!(p.sinai_mind.is_none() && p.ears.is_none() && p.voice.is_none());
    }

    struct Fixture;

    impl ReplayKernel for Fixture {
        fn id(&self) -> &str {
            "test-fixture/0"
        }
        fn sum(&self, values: &[f64]) -> f64 {
            values.iter().sum()
        }
        fn mean(&self, values: &[f64]) -> f64 {
            self.sum(values) / values.len() as f64
        }
        fn dither(&self, _: u64, _: u64) -> Box<dyn alelyon_verify::DitherStream + '_> {
            unimplemented!("not replayed here")
        }
    }

    #[test]
    fn what_is_not_a_page_is_carried_and_named_by_its_id() {
        let r = Registry::new()
            .replay(Arc::new(Fixture))
            .sinai_mind(|| Ok("ws://127.0.0.1:1/ws".into()))
            .ears(|| Err("not on this PC".into()))
            .voice(|| Err("no checkout".into()));
        assert_eq!(r.ids(), [REPLAY, SINAI_MIND, EARS, VOICE]);
        assert!(!r.is_empty());
        assert_eq!(r.replay_kernel().map(|k| k.id().to_owned()).as_deref(), Some("test-fixture/0"));
        let p = r.processes();
        assert_eq!((p.sinai_mind.as_ref().unwrap())().as_deref(), Ok("ws://127.0.0.1:1/ws"));
        assert_eq!((p.ears.as_ref().unwrap())(), Err("not on this PC".into()));
        assert!(format!("{r:?}").contains("test-fixture/0"));
        // a registry with only the substrate is not the public app
        assert!(!Registry::new().replay(Arc::new(Fixture)).is_empty());
    }

    #[test]
    #[should_panic(expected = "kept for what is not a page")]
    fn a_page_may_not_take_a_process_id() {
        let _ = Registry::new().with(Plugin::new(EARS, "Ears", Slot::ComputeTab, || Box::new(Counter::default())));
    }

    #[test]
    fn a_page_takes_its_own_messages_and_ignores_others() {
        let mut page: Box<dyn Section> = Box::new(Counter::default());
        let _ = page.update(Message::new(Msg::Add(3)));
        let _ = page.update(Message::new("not a counter's message"));
        let _ = page.update(Message::new(Msg::Add(4)));
        assert!(!page.busy());
        let plugin = Plugin::new("counter", "Counter", Slot::ComputeTab, || Box::new(Counter::default()));
        let r = Registry::new().with(plugin).owner(true);
        assert!(r.has("counter") && !r.has("other") && r.is_owner() && !r.is_empty());
        assert_eq!(r.in_slot(Slot::ComputeTab).count(), 1);
        assert_eq!(r.in_slot(Slot::TrustCard).count(), 0);
        assert_eq!(format!("{:?}", Message::new(1u8)), "plug-in message");
        assert_eq!(Message::new(7u32).get::<u32>(), Some(&7));
    }

    #[derive(Default)]
    struct Tabbed {
        tab: &'static str,
        started: bool,
    }

    impl Page for Tabbed {
        type Msg = Msg;

        fn start(&mut self) -> Task<Msg> {
            self.started = true;
            Task::none()
        }

        fn select(&mut self, tab: &str) -> bool {
            match ["one", "two"].into_iter().find(|t| *t == tab) {
                Some(t) => {
                    self.tab = t;
                    true
                }
                None => false,
            }
        }

        fn update(&mut self, _: Msg) -> Task<Msg> {
            Task::none()
        }

        fn view(&self, _: f32) -> Element<'_, Msg> {
            iced::widget::space().into()
        }
    }

    #[test]
    fn a_section_page_names_its_tabs_and_is_started_and_told_its_tab() {
        let plugin = Plugin::new("fleet", "Fleet", Slot::SectionPage, || Box::new(Tabbed::default())).with_tabs(&["one", "two"]);
        assert_eq!(plugin.tabs, ["one", "two"]);
        let r = Registry::new().with(plugin);
        assert_eq!(r.in_slot(Slot::SectionPage).map(|p| p.id).collect::<Vec<_>>(), ["fleet"]);
        assert!(r.in_slot(Slot::ComputeTab).next().is_none());
        let mut page = r.plugins()[0].make();
        assert!(page.select("two") && !page.select("three"));
        let _ = page.start();
        // a page that does not say how it starts starts as it opens, and has no tabs to select
        let mut plain: Box<dyn Section> = Box::new(Counter::default());
        let _ = plain.start();
        assert!(!plain.select("one"));
        assert!(Plugin::new("x", "X", Slot::TrustCard, || Box::new(Counter::default())).tabs.is_empty());
    }

    #[test]
    #[should_panic(expected = "two plug-ins")]
    fn two_plugins_with_one_id_are_refused() {
        let make = || Box::new(Counter::default()) as Box<dyn Section>;
        let _ = Registry::new().with(Plugin::new("x", "X", Slot::TrustCard, make)).with(Plugin::new("x", "Y", Slot::TrustCard, make));
    }
}
