//! The Research page: the research archive (the `alelyon-research` crate). A person
//! names a subject, its searches and its concepts; the archive gathers every paper OpenAlex and arXiv connect to it,
//! follows citations until a round accepts nothing new, and keeps each paper with the reason it belongs.
//!
//! OpenAlex answers keyless requests from a small budget shared by everyone behind one internet address, so the
//! page leads with its key: a button to OpenAlex's free key page (an easy way to
//! sign in there and get a key), a field that keeps a pasted key in Windows Credential Manager (a key typed into
//! Alelyon is kept there, never in a plain-text file), and the key's
//! budget for today as OpenAlex reports it. The key is looked up as every Alelyon key is (lattice_core::keys: the
//! environment, Credential Manager, then FAMEnvironment.env), is never shown, and goes to api.openalex.org alone,
//! as a bearer header.
//!
//! A harvest takes minutes and runs on a thread of its own; the page polls its progress while it runs and can stop
//! it. Every read of the archive is a short read on a thread of its own.
//!
//! Each subject has a knowledge graph (built by the `alelyon-research` crate), rebuilt after every run: its threads of
//! related work, influence, foundations, bridges and frontier. The Map draws it; a paper chosen there shows its
//! citations and nearest neighbours. Models and agents read the same graphs through `centcom --research-mcp`, a
//! read-only MCP server (so the knowledge graph is usable by humans and AI models and
//! agents alike); the page copies the entry an MCP client needs.
//!
//! The page opens on a hub (personalised to the person from their prior queries, to give them ideas of what to work
//! on): the gaps of every subject (gaps.rs: threads that rarely
//! talk, forgotten foundations, untried combinations, young and stalled threads, blind spots), ranked toward what the
//! person searches, opens and saves here, with papers to read next, the newest frontier and subjects to start. The
//! activity it learns from stays in the archive on this PC.

mod hub_view;
mod map;
pub mod pursue;
mod view;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use alelyon_research::openalex::{Budget, KEY_HELP, KEY_PAGE};
use alelyon_research::snowball::{Policy, Topic};
use alelyon_research::hub::{Hub, Proposal};
use alelyon_research::store::{Activity, Hit, Member, Neighbourhood, Store, StoredGap, StoredGraph, TopicRow, Watch, words_query};
use iced::widget::canvas::Cache;
use iced::{Subscription, Task};

pub use view::view;

/// The key's name, as every Alelyon key is named.
pub const KEY_NAME: &str = "OPENALEX_API_KEY";
/// Papers a subject's list shows before "Show more".
pub const SHOWN: usize = 200;
const POLL_EVERY: Duration = Duration::from_secs(1);
/// A job that shows no heartbeat this long after it was started or found has ended without finishing.
const JOB_GRACE_SECS: f64 = 45.0;
/// How often watched subjects are checked for a due update while the window is open.
const WATCH_EVERY: Duration = Duration::from_secs(10 * 60);
/// Requests an update may make (an update reads only what is new; a free key allows about 10,000 credits a day).
pub const UPDATE_CALLS: usize = 300;

/// Which of a subject's papers the list shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Filter {
    All,
    /// Accepted on their own text: the core.
    Text,
    /// Accepted on citations, in or out: the papers keyword search alone misses.
    Links,
    /// Reached only by following citations.
    OnlyCitations,
    /// Kept on the edge of the subject: few citers in a large subject, other words.
    Edge,
}

impl Filter {
    pub const ALL: [Filter; 5] = [Filter::All, Filter::Text, Filter::Links, Filter::OnlyCitations, Filter::Edge];

    pub fn title(self) -> &'static str {
        match self {
            Filter::All => "All",
            Filter::Text => "Matched on text",
            Filter::Links => "Matched on citations",
            Filter::OnlyCitations => "Found only through citations",
            Filter::Edge => "On the edge",
        }
    }

    pub fn keeps(self, m: &Member) -> bool {
        match self {
            Filter::All => true,
            Filter::Text => m.reason.starts_with("text:"),
            Filter::Links => !m.reason.starts_with("text:") && !m.reason.starts_with("peripheral:"),
            Filter::OnlyCitations => m.via == "citation",
            Filter::Edge => m.reason.starts_with("peripheral:"),
        }
    }
}

/// What a subject's page shows below its facts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    Papers,
    Map,
    Gaps,
}

/// Where the key stands, without its value.
#[derive(Clone, Debug, PartialEq)]
pub enum KeyState {
    Looking,
    Missing,
    /// Found, and where: "Credential Manager", "the environment", or a file.
    Present(String),
}

/// The new-subject form, as typed.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Form {
    pub name: String,
    /// Searches, separated by `;`.
    pub queries: String,
    /// Concepts, separated by `;`; a concept's spellings by `|`.
    pub concepts: String,
    /// Index calls the run may make.
    pub budget: String,
}

/// A harvest in flight.
pub struct Running {
    pub topic: String,
    /// An update of a saved subject rather than a full gathering.
    pub update: bool,
    /// The job's files (written by the `alelyon-research` crate): the run is a process of its own.
    pub files: alelyon_research::jobs::Files,
    /// The progress lines as last read.
    pub lines: Vec<String>,
    /// Stop was asked for.
    pub stopping: bool,
    /// When this window started or found it (Unix seconds): a job given no heartbeat soon after is reported.
    since: f64,
}

pub struct State {
    db: PathBuf,
    pub key: KeyState,
    /// The pasted key, until it is saved (then cleared). Never drawn: the field is a secure one.
    pub key_input: String,
    /// Show the paste field even though a key is present (to replace it).
    pub replacing: bool,
    pub budget: Option<Result<Budget, String>>,
    pub checking: bool,
    pub topics: Option<Result<Vec<TopicRow>, String>>,
    pub chosen: Option<String>,
    pub creating: bool,
    pub form: Form,
    pub members: Option<Result<Vec<Member>, String>>,
    pub filter: Filter,
    pub shown: usize,
    pub find: String,
    pub hits: Option<Result<Vec<Hit>, String>>,
    pub detail: Option<usize>,
    pub running: Option<Running>,
    pub view: View,
    /// The chosen subject's graph, as drawn.
    pub drawn: Option<Result<map::Drawn, String>>,
    cache: Cache,
    /// The paper chosen on the Map (its archive row), and its connections.
    pub selected: Option<i64>,
    pub near: Option<Result<Option<Neighbourhood>, String>>,
    /// The thread lit on the Map.
    pub thread: Option<usize>,
    graph_asked: u64,
    /// The MCP entry was just copied.
    pub copied: bool,
    /// The hub is shown instead of a subject (the page's landing view).
    pub hub_open: bool,
    pub hub: Option<Result<Hub, String>>,
    /// The chosen subject's gaps.
    pub gaps: Option<Result<Vec<StoredGap>, String>>,
    /// Papers the person saved.
    pub saved_ids: std::collections::HashSet<i64>,
    /// The last harvest's outcome, or why it could not run.
    pub outcome: Option<Result<String, String>>,
    pub error: Option<String>,
    pub loading: bool,
    asked: u64,
}

#[derive(Clone, Debug)]
pub enum Msg {
    Topics(Result<Vec<TopicRow>, String>),
    KeyLooked(KeyState),
    GetKey,
    KeyHelp,
    KeyInput(String),
    SaveKey,
    KeySaved(Result<(), String>),
    ReplaceKey,
    ForgetKey,
    KeyForgotten(Result<bool, String>),
    CheckBudget,
    Budget(Result<Budget, String>),
    NewSubject,
    FormName(String),
    FormQueries(String),
    FormConcepts(String),
    FormBudget(String),
    Gather,
    GatherAgain,
    Poll,
    Harvested(String, Result<alelyon_research::jobs::Outcome, String>),
    /// A run started earlier (by this window before it closed, or another) found still going.
    Attached(Option<(alelyon_research::jobs::Spec, alelyon_research::jobs::Files)>),
    StopHarvest,
    Pick(String),
    Members(u64, Result<Vec<Member>, String>),
    Show(Filter),
    ShowMore,
    Find(String),
    FindGo,
    Found(u64, Result<Vec<Hit>, String>),
    Detail(Option<usize>),
    OpenLink(String),
    Forget,
    Forgotten(Result<(), String>),
    ShowView(View),
    Graph(u64, Result<StoredGraph, String>),
    /// A paper chosen on the Map or in a list (its archive row), or none.
    Select(Option<i64>),
    Near(i64, Result<Option<Neighbourhood>, String>),
    Thread(Option<usize>),
    /// From the papers list: show this paper on the Map.
    ShowOnMap(i64),
    CopyMcp,
    OpenHub,
    HubRead(Result<Hub, String>),
    GapsRead(String, Result<Vec<StoredGap>, String>),
    /// Hide a gap from the hub and the subject's gaps (subject, headline).
    Dismiss(String, String),
    /// Fill the new-subject form from a proposal or a gap.
    StartFrom(Proposal),
    StartFromGap(StoredGap),
    /// Have Sinai work on a gap. The app sends the brief (`pursue::brief`) to whoever answers on Sinai's page and opens
    /// that page; this page marks the gap as handed over and keeps it in the person's history.
    Pursue(StoredGap),
    /// Sinai kept a finding beside one of this subject's gaps: read the gaps (and the hub) again to show it.
    FindingKept(String),
    /// Save a paper (its row, and the subject it was seen in).
    Save(i64, String),
    Unsave(i64),
    Logged(Result<(), String>),
    /// Open a subject on the Map at this paper.
    GoTo(String, i64),
    SetWatch(Watch),
    WatchSet(Result<(), String>),
    /// Update the chosen subject now.
    UpdateNow,
    /// Time to look for watched subjects that are due.
    WatchTick,
    Due(Vec<String>),
}

/// The archive's file: `CENTCOM_RESEARCH_DB`, else `~/.alelyon/research/archive.sqlite`.
pub fn db_path() -> PathBuf {
    // The same file the MCP server reads.
    alelyon_research::mcp::default_db()
}

/// The form as a subject and its policy, or what is missing.
pub fn read_form(form: &Form) -> Result<(Topic, Policy), String> {
    let split = |s: &str| s.split(';').map(str::trim).filter(|x| !x.is_empty()).map(str::to_string).collect::<Vec<_>>();
    let name = form.name.trim().to_string();
    if name.is_empty() {
        return Err("Name the subject.".into());
    }
    let queries = split(&form.queries);
    if queries.is_empty() {
        return Err("Give at least one search.".into());
    }
    let phrases: Vec<String> = split(&form.concepts)
        .into_iter()
        .map(|c| c.split('|').map(str::trim).filter(|v| !v.is_empty()).collect::<Vec<_>>().join("|"))
        .filter(|c| !c.is_empty())
        .collect();
    if phrases.is_empty() {
        return Err("Give at least one concept: the words a relevant paper's title or abstract would use.".into());
    }
    let topic = Topic { name, queries, phrases };
    let mut policy = Policy::for_topic(&topic);
    let budget = form.budget.trim();
    if !budget.is_empty() {
        policy.max_calls = budget.parse::<usize>().ok().filter(|n| (1..=1_000_000).contains(n)).ok_or("The budget is a number of requests, 1 or more.")?;
    }
    Ok((topic, policy))
}

/// The address a paper opens at: its open copy when that is a web address, else its DOI, arXiv or OpenAlex page.
pub fn link_of(m: &Member) -> Option<String> {
    use alelyon_research::ident::Id;
    if let Some(u) = &m.work.open_url
        && (u.starts_with("https://") || u.starts_with("http://"))
    {
        return Some(u.clone());
    }
    let ids = &m.work.ids;
    ids.iter()
        .find_map(|i| if let Id::Doi(d) = i { Some(format!("https://doi.org/{d}")) } else { None })
        .or_else(|| ids.iter().find_map(|i| if let Id::Arxiv(a) = i { Some(format!("https://arxiv.org/abs/{a}")) } else { None }))
        .or_else(|| ids.iter().find_map(|i| if let Id::OpenAlex(w) = i { Some(format!("https://openalex.org/{w}")) } else { None }))
}

fn look_up_key() -> KeyState {
    use lattice_core::keys::KeySource;
    let env: Arc<dyn lattice_core::Env> = Arc::new(lattice_core::ProcessEnv);
    match lattice_core::KeyStore::new(env, &lattice_core::state::resolve()).source(KEY_NAME) {
        None => KeyState::Missing,
        Some(KeySource::Environment) => KeyState::Present("the environment".into()),
        Some(KeySource::CredentialManager) => KeyState::Present("Credential Manager".into()),
        Some(KeySource::File(p)) => KeyState::Present(format!("a plain-text file, {}", p.display())),
    }
}

fn key_value() -> Option<String> {
    lattice_core::keys::get_key(KEY_NAME).map(|s| s.expose().to_string())
}

impl State {
    pub fn new() -> State {
        State::at(db_path())
    }

    fn at(db: PathBuf) -> State {
        State {
            db,
            key: KeyState::Looking,
            key_input: String::new(),
            replacing: false,
            budget: None,
            checking: false,
            topics: None,
            chosen: None,
            creating: false,
            form: Form { budget: "2000".into(), ..Form::default() },
            members: None,
            filter: Filter::All,
            shown: SHOWN,
            find: String::new(),
            hits: None,
            detail: None,
            running: None,
            view: View::Papers,
            drawn: None,
            cache: Cache::new(),
            selected: None,
            near: None,
            thread: None,
            graph_asked: 0,
            copied: false,
            hub_open: true,
            hub: None,
            gaps: None,
            saved_ids: std::collections::HashSet::new(),
            outcome: None,
            error: None,
            loading: false,
            asked: 0,
        }
    }

    /// The page opened: look up the key (where, not what), read the saved subjects and the hub.
    pub fn open(&mut self) -> Task<Msg> {
        Task::batch([
            self.read_topics(),
            self.read_hub(),
            self.find_running(),
            Task::perform(off_thread(look_up_key), |k| Msg::KeyLooked(k.unwrap_or(KeyState::Missing))),
        ])
    }

    fn read_hub(&self) -> Task<Msg> {
        let db = self.db.clone();
        Task::perform(
            off_thread(move || {
                if !db.is_file() {
                    return Ok(Hub::default());
                }
                Store::open(&db).and_then(|s| alelyon_research::hub::build(&s, crate::utc::now() as i64)).map_err(|e| e.to_string())
            }),
            |r| Msg::HubRead(r.unwrap_or_else(|| Err("the reading thread ended without an answer".into()))),
        )
    }

    fn read_gaps(&mut self) -> Task<Msg> {
        let Some(name) = self.chosen.clone() else { return Task::none() };
        self.gaps = None;
        let db = self.db.clone();
        Task::perform(off_thread({ let name = name.clone(); move || Store::open(&db).and_then(|s| s.gaps(Some(&name))).map_err(|e| e.to_string()) }), move |r| {
            Msg::GapsRead(name.clone(), r.unwrap_or_else(|| Err("the reading thread ended without an answer".into())))
        })
    }

    /// Keep one thing the person did, for the hub. Local only.
    fn log(&self, kind: &str, subject: Option<String>, work: Option<i64>, text: Option<String>) -> Task<Msg> {
        let db = self.db.clone();
        let a = Activity { at: crate::utc::full(crate::utc::now()), kind: kind.to_string(), subject, work, text };
        Task::perform(off_thread(move || Store::open(&db).and_then(|mut s| s.log(&a)).map_err(|e| e.to_string())), |r| {
            Msg::Logged(r.unwrap_or_else(|| Err("the thread ended without an answer".into())))
        })
    }

    fn prefill(&mut self, name: String, queries: Vec<String>, concepts: Vec<String>) {
        self.form = Form { name, queries: queries.join("; "), concepts: concepts.join("; "), budget: self.form.budget.clone() };
        self.creating = true;
        self.hub_open = false;
        self.detail = None;
    }

    pub fn busy(&self) -> bool {
        // A run is not "busy": it is a process of its own and can take an hour. Counting it turned the whole window over
        // 30 times a second for the run's whole length (the 2026-10-08 crash in the graphics driver came 18 minutes into
        // one); its progress is read once a second instead.
        self.loading || self.checking || self.drawn.is_none() && self.view == View::Map && self.chosen.is_some()
    }

    /// Read the chosen subject's graph, rebuilding it first for a subject saved before graphs existed.
    fn read_graph(&mut self) -> Task<Msg> {
        let Some(name) = self.chosen.clone() else { return Task::none() };
        self.graph_asked += 1;
        let asked = self.graph_asked;
        self.drawn = None;
        self.cache.clear();
        let db = self.db.clone();
        Task::perform(
            off_thread(move || {
                let mut store = Store::open(&db).map_err(|e| e.to_string())?;
                let g = store.graph(&name).map_err(|e| e.to_string())?;
                if g.nodes.is_empty() && store.members(&name).map(|m| !m.is_empty()).unwrap_or(false) {
                    store.rebuild_graph(&name).map_err(|e| e.to_string())?;
                    return store.graph(&name).map_err(|e| e.to_string());
                }
                Ok(g)
            }),
            move |r| Msg::Graph(asked, r.unwrap_or_else(|| Err("the reading thread ended without an answer".into()))),
        )
    }

    fn read_near(&mut self, work: i64) -> Task<Msg> {
        let Some(name) = self.chosen.clone() else { return Task::none() };
        self.selected = Some(work);
        self.near = None;
        let db = self.db.clone();
        Task::perform(off_thread(move || Store::open(&db).and_then(|s| s.neighbourhood(&name, work)).map_err(|e| e.to_string())), move |r| {
            Msg::Near(work, r.unwrap_or_else(|| Err("the reading thread ended without an answer".into())))
        })
    }

    /// The entry an MCP client (Lattice's Tools page, Claude Code, …) needs to read the archive through this program.
    pub fn mcp_entry() -> String {
        let exe = std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_else(|_| "centcom".into());
        serde_json::to_string_pretty(&serde_json::json!({
            "mcpServers": {"alelyon-research": {"command": exe, "args": ["--research-mcp"]}}
        }))
        .unwrap_or_default()
    }

    pub fn subscription(&self) -> Subscription<Msg> {
        let watch = iced::time::every(WATCH_EVERY).map(|_| Msg::WatchTick);
        if self.running.is_some() { Subscription::batch([watch, iced::time::every(POLL_EVERY).map(|_| Msg::Poll)]) } else { watch }
    }

    /// Look for a run that is still going (this window may have closed or crashed while it ran).
    fn find_running(&self) -> Task<Msg> {
        let dir = alelyon_research::jobs::dir_for(&self.db);
        Task::perform(off_thread(move || alelyon_research::jobs::running(&dir).into_iter().next()), |found| Msg::Attached(found.flatten()))
    }

    /// Update a saved subject with what is new.
    fn start_update(&mut self, name: String) -> Task<Msg> {
        if self.running.is_some() {
            return Task::none();
        }
        let spec = alelyon_research::jobs::Spec {
            id: alelyon_research::jobs::new_id(),
            db: self.db.clone(),
            kind: alelyon_research::jobs::Kind::Update { subject: name.clone(), max_calls: UPDATE_CALLS },
        };
        self.launch(spec, format!("Updating \"{name}\" with what is new…"));
        Task::none()
    }

    fn ask(&mut self) -> u64 {
        self.asked += 1;
        self.loading = true;
        self.asked
    }

    fn read_topics(&self) -> Task<Msg> {
        let db = self.db.clone();
        Task::perform(off_thread(move || Store::open(&db).and_then(|s| s.topics()).map_err(|e| e.to_string())), |r| {
            Msg::Topics(r.unwrap_or_else(|| Err("the reading thread ended without an answer".into())))
        })
    }

    fn read_members(&mut self) -> Task<Msg> {
        let Some(name) = self.chosen.clone() else { return Task::none() };
        let asked = self.ask();
        let db = self.db.clone();
        Task::perform(off_thread(move || Store::open(&db).and_then(|s| s.members(&name)).map_err(|e| e.to_string())), move |r| {
            Msg::Members(asked, r.unwrap_or_else(|| Err("the reading thread ended without an answer".into())))
        })
    }

    fn check_budget(&mut self) -> Task<Msg> {
        self.checking = true;
        Task::perform(
            off_thread(|| {
                let key = key_value();
                if key.is_none() {
                    return Err("No key is set.".to_string());
                }
                let mut fetch = alelyon_research::net::HttpFetch::new().map_err(|e| e.to_string())?.with_bearer("api.openalex.org", key);
                fetch.openalex_budget().map_err(|e| e.to_string())
            }),
            |r| Msg::Budget(r.unwrap_or_else(|| Err("the checking thread ended without an answer".into()))),
        )
    }

    fn start(&mut self, topic: Topic, policy: Policy) -> Task<Msg> {
        if self.running.is_some() {
            return Task::none();
        }
        let name = topic.name.clone();
        let spec = alelyon_research::jobs::Spec {
            id: alelyon_research::jobs::new_id(),
            db: self.db.clone(),
            kind: alelyon_research::jobs::Kind::Full { topic, max_calls: policy.max_calls },
        };
        self.launch(spec, format!("Searching OpenAlex and arXiv for \"{name}\"…"));
        self.creating = false;
        self.chosen = Some(name);
        self.members = None;
        Task::none()
    }

    /// Start a job in a process of its own (`centcom --research-run <spec>`), outside any job object of this
    /// window, so a frozen, crashed or closed window does not end the run. The OpenAlex key goes in the process's
    /// environment, never its command line.
    fn launch(&mut self, spec: alelyon_research::jobs::Spec, first_line: String) {
        let update = spec.is_update();
        let subject = spec.subject().to_string();
        let files = match alelyon_research::jobs::create(&spec) {
            Ok(f) => f,
            Err(e) => {
                self.outcome = Some(Err(e.to_string()));
                return;
            }
        };
        match spawn_job(&files.spec) {
            Ok(()) => {
                self.outcome = None;
                self.running = Some(Running { topic: subject, update, files, lines: vec![first_line], stopping: false, since: crate::utc::now() });
            }
            Err(why) => self.outcome = Some(Err(format!("The run could not be started: {why}"))),
        }
    }

    pub fn update(&mut self, msg: Msg) -> Task<Msg> {
        match msg {
            Msg::Topics(r) => {
                if let (Ok(list), None) = (&r, &self.chosen) {
                    self.chosen = list.first().map(|t| t.name.clone());
                    self.topics = Some(r);
                    let view = match self.view {
                        View::Map => self.read_graph(),
                        View::Gaps => self.read_gaps(),
                        View::Papers => Task::none(),
                    };
                    return Task::batch([self.read_members(), view]);
                }
                self.topics = Some(r);
            }
            Msg::KeyLooked(k) => {
                let present = matches!(k, KeyState::Present(_));
                self.key = k;
                if present && self.budget.is_none() {
                    return self.check_budget();
                }
            }
            Msg::GetKey => return self.open_link(KEY_PAGE.to_string()),
            Msg::KeyHelp => return self.open_link(KEY_HELP.to_string()),
            Msg::KeyInput(k) => self.key_input = k,
            Msg::SaveKey => {
                let key = std::mem::take(&mut self.key_input);
                if key.trim().is_empty() {
                    return Task::none();
                }
                return Task::perform(
                    off_thread(move || lattice_core::keys::vault_store(KEY_NAME, &lattice_core::keys::SecretString::new(key))),
                    |r| Msg::KeySaved(r.unwrap_or_else(|| Err("the saving thread ended without an answer".into()))),
                );
            }
            Msg::KeySaved(r) => match r {
                Ok(()) => {
                    self.replacing = false;
                    self.budget = None;
                    self.error = None;
                    return Task::perform(off_thread(look_up_key), |k| Msg::KeyLooked(k.unwrap_or(KeyState::Missing)));
                }
                Err(why) => self.error = Some(why),
            },
            Msg::ReplaceKey => self.replacing = !self.replacing,
            Msg::ForgetKey => {
                return Task::perform(off_thread(|| lattice_core::keys::vault_remove(KEY_NAME)), |r| {
                    Msg::KeyForgotten(r.unwrap_or_else(|| Err("the thread ended without an answer".into())))
                });
            }
            Msg::KeyForgotten(r) => {
                if let Err(why) = r {
                    self.error = Some(why);
                }
                self.budget = None;
                return Task::perform(off_thread(look_up_key), |k| Msg::KeyLooked(k.unwrap_or(KeyState::Missing)));
            }
            Msg::CheckBudget => return self.check_budget(),
            Msg::Budget(r) => {
                self.checking = false;
                self.budget = Some(r);
            }
            Msg::NewSubject => {
                self.creating = true;
                self.hub_open = false;
                self.detail = None;
            }
            Msg::FormName(v) => self.form.name = v,
            Msg::FormQueries(v) => self.form.queries = v,
            Msg::FormConcepts(v) => self.form.concepts = v,
            Msg::FormBudget(v) => self.form.budget = v,
            Msg::Gather => match read_form(&self.form) {
                Ok((topic, policy)) => {
                    self.error = None;
                    let note = self.log("gather", Some(topic.name.clone()), None, Some(topic.queries.join("; ")));
                    return Task::batch([self.start(topic, policy), note]);
                }
                Err(why) => self.error = Some(why),
            },
            Msg::GatherAgain => {
                let Some(Ok(topics)) = &self.topics else { return Task::none() };
                let Some(t) = topics.iter().find(|t| Some(&t.name) == self.chosen.as_ref()) else { return Task::none() };
                let topic = Topic { name: t.name.clone(), queries: t.queries.clone(), phrases: t.phrases.clone() };
                let mut policy = Policy::for_topic(&topic);
                if let Ok(n) = self.form.budget.trim().parse::<usize>() {
                    policy.max_calls = n.max(1);
                }
                return self.start(topic, policy);
            }
            Msg::Poll => {
                let Some(r) = &mut self.running else { return Task::none() };
                let lines = alelyon_research::jobs::progress(&r.files);
                if !lines.is_empty() {
                    r.lines = lines;
                }
                if let Some(o) = alelyon_research::jobs::outcome(&r.files) {
                    let name = r.topic.clone();
                    return self.update(Msg::Harvested(name, if o.ok { Ok(o) } else { Err(o.error.unwrap_or_else(|| "the run failed".into())) }));
                }
                // No heartbeat and no outcome, well after it started: the job's process ended without finishing.
                if !alelyon_research::jobs::alive(&r.files) && crate::utc::now() - r.since > JOB_GRACE_SECS {
                    let name = r.topic.clone();
                    return self.update(Msg::Harvested(
                        name,
                        Err("The run's process ended without an outcome. What it gathered up to its last round is saved.".into()),
                    ));
                }
            }
            Msg::StopHarvest => {
                if let Some(r) = &mut self.running {
                    match alelyon_research::jobs::request_stop(&r.files) {
                        Ok(()) => r.stopping = true,
                        Err(e) => self.error = Some(e.to_string()),
                    }
                }
            }
            Msg::Attached(found) => {
                if self.running.is_none()
                    && let Some((spec, files)) = found
                {
                    let lines = alelyon_research::jobs::progress(&files);
                    self.running = Some(Running {
                        topic: spec.subject().to_string(),
                        update: spec.is_update(),
                        files,
                        lines,
                        stopping: false,
                        since: crate::utc::now(),
                    });
                }
            }
            Msg::Harvested(name, r) => {
                self.running = None;
                self.outcome = Some(r.map(|o| {
                    format!(
                        "{}: {} papers accepted of {} read in {} requests. {}",
                        stop_words(o.stop.as_deref().unwrap_or("")),
                        o.accepted,
                        o.read,
                        o.calls,
                        o.halt_reason.map(|h| format!("({h})")).unwrap_or_default()
                    )
                }));
                self.chosen = Some(name);
                self.budget = None;
                let refresh = if matches!(self.key, KeyState::Present(_)) { self.check_budget() } else { Task::none() };
                self.selected = None;
                self.near = None;
                let graph = if self.view == View::Map { self.read_graph() } else { Task::none() };
                // A new run brings new gaps: the hub and the subject's gaps are read again.
                self.hub_open = false;
                let gaps = self.read_gaps();
                return Task::batch([self.read_topics(), self.read_members(), refresh, graph, gaps, self.read_hub()]);
            }
            Msg::Pick(name) => {
                self.chosen = Some(name);
                self.hub_open = false;
                self.gaps = None;
                self.creating = false;
                self.filter = Filter::All;
                self.shown = SHOWN;
                self.detail = None;
                self.hits = None;
                self.find.clear();
                self.selected = None;
                self.near = None;
                self.thread = None;
                self.drawn = None;
                let graph = match self.view {
                    View::Map => self.read_graph(),
                    View::Gaps => self.read_gaps(),
                    View::Papers => Task::none(),
                };
                return Task::batch([self.read_members(), graph]);
            }
            Msg::ShowView(v) => {
                self.view = v;
                if v == View::Map && self.drawn.is_none() {
                    return self.read_graph();
                }
                if v == View::Gaps {
                    return self.read_gaps();
                }
            }
            Msg::Graph(asked, r) => {
                if asked == self.graph_asked {
                    self.drawn = Some(r.map(map::Drawn::new));
                    self.cache.clear();
                }
            }
            Msg::Select(None) => {
                self.selected = None;
                self.near = None;
            }
            Msg::Select(Some(work)) => return self.read_near(work),
            Msg::Near(work, r) => {
                if self.selected == Some(work) {
                    self.near = Some(r);
                }
            }
            Msg::Thread(t) => {
                self.thread = if self.thread == t { None } else { t };
                self.cache.clear();
            }
            Msg::ShowOnMap(work) => {
                self.view = View::Map;
                let graph = if self.drawn.is_none() { self.read_graph() } else { Task::none() };
                return Task::batch([graph, self.read_near(work)]);
            }
            Msg::CopyMcp => {
                self.copied = true;
                return iced::clipboard::write(State::mcp_entry());
            }
            Msg::Members(asked, r) => {
                if asked == self.asked {
                    self.loading = false;
                    self.members = Some(r);
                }
            }
            Msg::Show(f) => {
                self.filter = f;
                self.shown = SHOWN;
                self.detail = None;
            }
            Msg::ShowMore => self.shown += SHOWN,
            Msg::Find(words) => {
                self.find = words;
                if self.find.trim().is_empty() {
                    self.hits = None;
                }
            }
            Msg::FindGo => {
                let Some(query) = words_query(&self.find) else {
                    self.hits = None;
                    return Task::none();
                };
                let asked = self.ask();
                let db = self.db.clone();
                let topic = self.chosen.clone();
                let note = self.log("search", topic.clone(), None, Some(self.find.trim().to_string()));
                return Task::batch([
                    Task::perform(
                        off_thread(move || Store::open(&db).and_then(|s| s.search(&query, topic.as_deref(), 100)).map_err(|e| e.to_string())),
                        move |r| Msg::Found(asked, r.unwrap_or_else(|| Err("the reading thread ended without an answer".into()))),
                    ),
                    note,
                ]);
            }
            Msg::Found(asked, r) => {
                if asked == self.asked {
                    self.loading = false;
                    self.hits = Some(r);
                }
            }
            Msg::Detail(i) => {
                self.detail = i;
                let work = match (&self.members, i) {
                    (Some(Ok(m)), Some(i)) => m.get(i).map(|m| m.id),
                    _ => None,
                };
                if let Some(w) = work {
                    return self.log("open", self.chosen.clone(), Some(w), None);
                }
            }
            Msg::OpenHub => {
                self.hub_open = true;
                self.creating = false;
                return self.read_hub();
            }
            Msg::HubRead(r) => {
                if let Ok(h) = &r {
                    self.saved_ids = h.saved.iter().map(|p| p.work).collect();
                }
                self.hub = Some(r);
            }
            Msg::GapsRead(name, r) => {
                if self.chosen.as_deref() == Some(name.as_str()) {
                    self.gaps = Some(r);
                }
            }
            Msg::Dismiss(subject, headline) => {
                if let Some(Ok(h)) = &mut self.hub {
                    h.ideas.retain(|i| !(i.gap.subject == subject && i.gap.headline == headline));
                }
                if let Some(Ok(list)) = &mut self.gaps {
                    for g in list.iter_mut().filter(|g| g.subject == subject && g.headline == headline) {
                        g.dismissed = true;
                    }
                }
                return self.log("dismiss_gap", Some(subject), None, Some(headline));
            }
            Msg::Pursue(g) => {
                let at = crate::utc::full(crate::utc::now());
                let same = |x: &StoredGap| x.subject == g.subject && x.headline == g.headline;
                if let Some(Ok(h)) = &mut self.hub {
                    h.ideas.iter_mut().filter(|i| same(&i.gap)).for_each(|i| i.gap.pursued = Some(at.clone()));
                }
                if let Some(Ok(list)) = &mut self.gaps {
                    list.iter_mut().filter(|x| same(x)).for_each(|x| x.pursued = Some(at.clone()));
                }
                return self.log("pursue_gap", Some(g.subject), None, Some(g.headline));
            }
            Msg::FindingKept(subject) => {
                let mut tasks = Vec::new();
                if self.hub_open {
                    tasks.push(self.read_hub());
                }
                if self.chosen.as_deref() == Some(subject.as_str()) && self.view == View::Gaps {
                    tasks.push(self.read_gaps());
                }
                return Task::batch(tasks);
            }
            Msg::StartFrom(p) => self.prefill(p.name, p.queries, p.concepts),
            Msg::StartFromGap(g) => {
                let terms: Vec<String> = g.terms.iter().filter(|t| !t.is_empty()).take(4).cloned().collect();
                let name = terms.iter().take(2).cloned().collect::<Vec<_>>().join(" × ");
                self.prefill(name, vec![terms.join(" ")], terms);
            }
            Msg::Save(work, subject) => {
                self.saved_ids.insert(work);
                return self.log("save", (!subject.is_empty()).then_some(subject), Some(work), None);
            }
            Msg::Unsave(work) => {
                self.saved_ids.remove(&work);
                if let Some(Ok(h)) = &mut self.hub {
                    h.saved.retain(|p| p.work != work);
                }
                return self.log("unsave", None, Some(work), None);
            }
            Msg::Logged(r) => {
                if let Err(why) = r {
                    self.error = Some(format!("Could not keep that in your history: {why}"));
                }
            }
            Msg::SetWatch(w) => {
                let Some(name) = self.chosen.clone() else { return Task::none() };
                if let Some(Ok(list)) = &mut self.topics
                    && let Some(t) = list.iter_mut().find(|t| t.name == name)
                {
                    t.watch = w;
                }
                let db = self.db.clone();
                return Task::perform(off_thread(move || Store::open(&db).and_then(|mut s| s.set_watch(&name, w)).map_err(|e| e.to_string())), |r| {
                    Msg::WatchSet(r.unwrap_or_else(|| Err("the thread ended without an answer".into())))
                });
            }
            Msg::WatchSet(r) => {
                if let Err(why) = r {
                    self.error = Some(why);
                    return self.read_topics();
                }
            }
            Msg::UpdateNow => {
                if let Some(name) = self.chosen.clone() {
                    return self.start_update(name);
                }
            }
            Msg::WatchTick => {
                // Only with a key (keyless requests share a tiny budget) and nothing else running.
                if self.running.is_some() || !matches!(self.key, KeyState::Present(_)) || !self.db.is_file() {
                    return Task::none();
                }
                let db = self.db.clone();
                return Task::perform(
                    off_thread(move || {
                        if !alelyon_research::jobs::running(&alelyon_research::jobs::dir_for(&db)).is_empty() {
                            return Vec::new();
                        }
                        Store::open(&db).and_then(|s| s.due(crate::utc::now() as i64)).unwrap_or_default()
                    }),
                    |due| Msg::Due(due.unwrap_or_default()),
                );
            }
            Msg::Due(due) => {
                if let Some(name) = due.into_iter().next() {
                    return self.start_update(name);
                }
            }
            Msg::GoTo(subject, work) => {
                if subject.is_empty() {
                    return Task::none();
                }
                let changed = self.chosen.as_deref() != Some(subject.as_str());
                self.hub_open = false;
                self.creating = false;
                self.view = View::Map;
                let mut tasks = Vec::new();
                if changed {
                    self.chosen = Some(subject);
                    self.drawn = None;
                    self.thread = None;
                    tasks.push(self.read_members());
                }
                if self.drawn.is_none() {
                    tasks.push(self.read_graph());
                }
                tasks.push(self.read_near(work));
                tasks.push(self.log("open", self.chosen.clone(), Some(work), None));
                return Task::batch(tasks);
            }
            Msg::OpenLink(url) => return self.open_link(url),
            Msg::Forget => {
                let (Some(name), None) = (self.chosen.clone(), &self.running) else { return Task::none() };
                let db = self.db.clone();
                return Task::perform(off_thread(move || Store::open(&db).and_then(|mut s| s.delete_topic(&name)).map_err(|e| e.to_string())), |r| {
                    Msg::Forgotten(r.unwrap_or_else(|| Err("the thread ended without an answer".into())))
                });
            }
            Msg::Forgotten(r) => {
                if let Err(why) = r {
                    self.error = Some(why);
                }
                self.chosen = None;
                self.members = None;
                self.drawn = None;
                self.selected = None;
                self.near = None;
                return self.read_topics();
            }
        }
        Task::none()
    }

    fn open_link(&mut self, url: String) -> Task<Msg> {
        // Only web addresses: a record's link comes from an index, not from this program.
        if !(url.starts_with("https://") || url.starts_with("http://")) {
            return Task::none();
        }
        if let Err(why) = crate::signin::loopback::open_in_browser(&url) {
            self.error = Some(why);
        }
        Task::none()
    }

    /// The chosen subject's papers under the filter, as indexes into `members`.
    /// The chosen paper's index in the drawn graph.
    pub fn selected_index(&self) -> Option<usize> {
        let (Some(Ok(d)), Some(w)) = (&self.drawn, self.selected) else { return None };
        d.index.get(&w).copied()
    }

    pub fn map(&self) -> Option<iced::widget::Canvas<map::Map<'_>, Msg>> {
        let Some(Ok(drawn)) = &self.drawn else { return None };
        Some(iced::widget::canvas(map::Map { drawn, cache: &self.cache, selected: self.selected_index(), thread: self.thread }))
    }

    pub fn visible(&self) -> Vec<usize> {
        let Some(Ok(members)) = &self.members else { return Vec::new() };
        members.iter().enumerate().filter(|(_, m)| self.filter.keeps(m)).map(|(i, _)| i).collect()
    }
}

impl Default for State {
    fn default() -> State {
        State::new()
    }
}

/// A run's stop, in words.
pub fn stop_words(stop: &str) -> &'static str {
    match stop {
        "closed" => "Complete: every relevant paper's references and citations were read",
        "budget" => "Stopped at its request budget",
        "rounds" => "Stopped at its round limit",
        "throttled" => "Stopped: OpenAlex's daily budget is spent",
        "key_refused" => "Stopped: OpenAlex refused the key",
        "cancelled" => "Stopped by you",
        "in_progress" => "Ended before it finished (its process closed); what it gathered up to its last round is saved",
        _ => "Stopped",
    }
}

/// Start `centcom --research-run <spec>` as a process of its own: no console window, no inherited handles, and out of
/// this window's job object where Windows allows it, so it outlives the window.
fn spawn_job(spec: &std::path::Path) -> Result<(), String> {
    use std::process::{Command, Stdio};
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let make = || {
        let mut c = Command::new(&exe);
        c.arg("--research-run").arg(spec).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        if let Some(k) = key_value() {
            c.env("OPENALEX_API_KEY", k);
        }
        c
    };
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        // Breaking away fails when the window's job forbids it; then the run lives in the job like any child.
        if make().creation_flags(CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP | CREATE_BREAKAWAY_FROM_JOB).spawn().is_ok() {
            return Ok(());
        }
        make().creation_flags(CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP).spawn().map(|_| ()).map_err(|e| e.to_string())
    }
    #[cfg(not(windows))]
    {
        make().spawn().map(|_| ()).map_err(|e| e.to_string())
    }
}

/// `work` on a thread of its own, awaited without holding the window's. reqwest's blocking client needs this: it
/// must not run inside the window's async runtime.
async fn off_thread<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    let (tx, rx) = futures::channel::oneshot::channel();
    let _ = std::thread::Builder::new().name("centcom-research".into()).spawn(move || {
        let _ = tx.send(work());
    });
    rx.await.ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use alelyon_research::ident::Id;
    use alelyon_research::work::Work;

    fn member(reason: &str, via: &str) -> Member {
        Member { id: 0, work: Work::default(), reason: reason.into(), via: via.into() }
    }

    #[test]
    fn the_form_reads_concepts_and_spellings() {
        let form = Form {
            name: " Low-bit weights ".into(),
            queries: "binary neural network; 1-bit quantization ;".into(),
            concepts: "binarized | binary weights ; quantization|quantized;;".into(),
            budget: "500".into(),
        };
        let (t, p) = read_form(&form).unwrap();
        assert_eq!(t.name, "Low-bit weights");
        assert_eq!(t.queries, vec!["binary neural network", "1-bit quantization"]);
        assert_eq!(t.phrases, vec!["binarized|binary weights", "quantization|quantized"]);
        assert_eq!(p.max_calls, 500);
    }

    #[test]
    fn an_incomplete_form_says_what_is_missing() {
        let mut form = Form::default();
        assert!(read_form(&form).unwrap_err().contains("Name"));
        form.name = "x".into();
        assert!(read_form(&form).unwrap_err().contains("search"));
        form.queries = "q".into();
        assert!(read_form(&form).unwrap_err().contains("concept"));
        form.concepts = "c".into();
        form.budget = "lots".into();
        assert!(read_form(&form).unwrap_err().contains("budget"));
        form.budget = "0".into();
        assert!(read_form(&form).is_err());
    }

    #[test]
    fn a_saved_key_leaves_no_copy_in_the_page() {
        let mut s = State::at(PathBuf::from("unused.sqlite"));
        let _ = s.update(Msg::KeyInput("secret-key".into()));
        let _ = s.update(Msg::SaveKey);
        assert!(s.key_input.is_empty());
    }

    #[test]
    fn filters_pick_by_reason_and_route() {
        let all = [member("text:2", "search"), member("cited_by:3", "citation"), member("cites:2:1", "both"), member("peripheral:2", "citation")];
        let count = |f: Filter| all.iter().filter(|m| f.keeps(m)).count();
        assert_eq!(count(Filter::All), 4);
        assert_eq!(count(Filter::Text), 1);
        assert_eq!(count(Filter::Links), 2);
        assert_eq!(count(Filter::OnlyCitations), 2);
        assert_eq!(count(Filter::Edge), 1);
    }

    #[test]
    fn a_paper_opens_only_at_a_web_address() {
        let mut m = member("text:2", "search");
        m.work.open_url = Some("file:///C:/Windows/System32/calc.exe".into());
        m.work.ids.insert(Id::Doi("10.1/x".into()));
        assert_eq!(link_of(&m).as_deref(), Some("https://doi.org/10.1/x"));
        m.work.open_url = Some("https://arxiv.org/pdf/2106.09685".into());
        assert_eq!(link_of(&m).as_deref(), Some("https://arxiv.org/pdf/2106.09685"));
        let mut s = State::at(PathBuf::from("unused.sqlite"));
        let _ = s.update(Msg::OpenLink("file:///C:/x.exe".into()));
        assert!(s.error.is_none(), "a non-web address is ignored, not attempted");
    }

    #[test]
    fn a_late_answer_for_an_earlier_subject_is_dropped() {
        let mut s = State::at(PathBuf::from("unused.sqlite"));
        s.asked = 2;
        s.loading = true;
        let _ = s.update(Msg::Members(1, Ok(vec![member("text:2", "search")])));
        assert!(s.members.is_none());
        let _ = s.update(Msg::Members(2, Ok(vec![])));
        assert!(matches!(s.members, Some(Ok(_))));
        assert!(!s.loading);
    }

    #[test]
    fn the_mcp_entry_runs_this_program_as_the_research_server() {
        let v: serde_json::Value = serde_json::from_str(&State::mcp_entry()).unwrap();
        let server = &v["mcpServers"]["alelyon-research"];
        assert_eq!(server["args"][0], "--research-mcp");
        assert!(!server["command"].as_str().unwrap().is_empty());
    }

    #[test]
    fn a_late_graph_for_an_earlier_subject_is_dropped_and_a_thread_toggles() {
        let mut s = State::at(PathBuf::from("unused.sqlite"));
        s.graph_asked = 2;
        let _ = s.update(Msg::Graph(1, Ok(StoredGraph::default())));
        assert!(s.drawn.is_none());
        let _ = s.update(Msg::Graph(2, Ok(StoredGraph::default())));
        assert!(matches!(s.drawn, Some(Ok(_))));
        let _ = s.update(Msg::Thread(Some(3)));
        assert_eq!(s.thread, Some(3));
        let _ = s.update(Msg::Thread(Some(3)));
        assert_eq!(s.thread, None, "choosing the lit thread again unlights it");
        let _ = s.update(Msg::Near(7, Ok(None)));
        assert!(s.near.is_none(), "connections of a paper no longer chosen are dropped");
    }

    #[test]
    fn the_page_opens_on_the_hub_and_a_proposal_fills_the_form() {
        let mut s = State::at(PathBuf::from("unused.sqlite"));
        assert!(s.hub_open);
        let _ = s.update(Msg::StartFrom(Proposal {
            name: "fpga × analog".into(),
            queries: vec!["fpga analog".into()],
            concepts: vec!["fpga".into(), "analog".into()],
            why: "w".into(),
        }));
        assert!(s.creating && !s.hub_open);
        assert_eq!(s.form.concepts, "fpga; analog");
        assert_eq!(s.form.budget, "2000", "the budget the person set is kept");
        let (topic, _) = read_form(&s.form).unwrap();
        assert_eq!(topic.queries, vec!["fpga analog"]);
    }

    #[test]
    fn saving_and_dismissing_change_the_page_at_once() {
        let mut s = State::at(PathBuf::from("unused.sqlite"));
        let _ = s.update(Msg::Save(7, "x".into()));
        assert!(s.saved_ids.contains(&7));
        let _ = s.update(Msg::Unsave(7));
        assert!(!s.saved_ids.contains(&7));
        let gap = StoredGap {
            subject: "x".into(),
            kind: alelyon_research::gaps::Kind::Young,
            rank: 0,
            score: 1.0,
            headline: "h".into(),
            detail: "d".into(),
            terms: vec![],
            works: vec![],
            threads: vec![],
            dismissed: false,
            pursued: None,
            findings: vec![],
        };
        s.gaps = Some(Ok(vec![gap.clone()]));
        let _ = s.update(Msg::Pursue(gap));
        assert!(matches!(&s.gaps, Some(Ok(g)) if g[0].pursued.is_some()), "marked as handed to Sinai at once");
        let _ = s.update(Msg::Dismiss("x".into(), "h".into()));
        assert!(matches!(&s.gaps, Some(Ok(g)) if g[0].dismissed));
    }

    #[test]
    fn watched_updates_need_a_key_and_an_idle_page() {
        let mut s = State::at(PathBuf::from("unused.sqlite"));
        s.key = KeyState::Missing;
        let _ = s.update(Msg::WatchTick);
        let _ = s.update(Msg::Due(vec![]));
        assert!(s.running.is_none());
        // A second due subject waits for the run that is going.
        s.running = Some(Running {
            topic: "a".into(),
            update: true,
            files: alelyon_research::jobs::files(std::path::Path::new("unused"), "x"),
            lines: vec![],
            stopping: false,
            since: 0.0,
        });
        let _ = s.update(Msg::Due(vec!["b".into()]));
        assert!(matches!(&s.running, Some(r) if r.topic == "a"));
        assert!(!s.busy(), "a run alone does not keep the window redrawing");
    }

    #[test]
    fn a_job_that_ends_reports_its_outcome_and_a_job_that_dies_says_so() {
        let dir = std::env::temp_dir().join(format!("centcom-research-jobs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut s = State::at(dir.join("archive.sqlite"));
        let files = alelyon_research::jobs::files(&dir, "j1");
        s.running = Some(Running { topic: "t".into(), update: false, files: files.clone(), lines: vec![], stopping: false, since: crate::utc::now() });
        // Still starting: no heartbeat yet, inside the grace period: nothing is reported.
        let _ = s.update(Msg::Poll);
        assert!(s.running.is_some() && s.outcome.is_none());
        std::fs::write(&files.progress, "10 accepted (10 new this round), 20 papers read, 3 requests\n").unwrap();
        let _ = s.update(Msg::Poll);
        assert_eq!(s.running.as_ref().unwrap().lines.len(), 1);
        let _ = s.update(Msg::StopHarvest);
        assert!(files.stop.exists() && s.running.as_ref().unwrap().stopping);
        std::fs::write(&files.done, r#"{"ok":true,"stop":"cancelled","accepted":10,"read":20,"calls":3}"#).unwrap();
        let _ = s.update(Msg::Poll);
        assert!(s.running.is_none());
        assert!(matches!(&s.outcome, Some(Ok(w)) if w.contains("10 papers accepted of 20")));
        // A job whose process vanished long ago without an outcome is reported as ended, its saved rounds kept.
        let gone = alelyon_research::jobs::files(&dir, "j2");
        s.running = Some(Running { topic: "t".into(), update: false, files: gone, lines: vec![], stopping: false, since: 0.0 });
        let _ = s.update(Msg::Poll);
        assert!(s.running.is_none());
        assert!(matches!(&s.outcome, Some(Err(w)) if w.contains("last round is saved")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_key_page_is_openalexs_own() {
        assert!(KEY_PAGE.starts_with("https://openalex.org/"));
        assert!(KEY_HELP.starts_with("https://help.openalex.org/"));
    }
}
