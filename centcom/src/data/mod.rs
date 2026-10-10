//! The Data page: the project's databases, read-only, as tables a person can browse.
//!
//! Which databases appear is fixed (chosen 2026-10-04 and 2026-10-05): the fleet and development stores (the
//! agent bus, the PR relay with its receipts, the session intents, the model ledger, the universal ledger, Lattice's
//! workspaces) and Sinai's memory and experience, its private conversations, read-only on this PC. Markets data
//! (the trading app's stores) and the identity stores (account password hashes, keys, tokens) are never listed, and
//! no other file can be opened from here: the list is closed (`stores::STORES`).
//!
//! Every read goes through `sqlite_ro`, so nothing on disk changes: no side file, no write, no lock held longer
//! than one short read. A column whose name says it holds a secret is never read at all (`stores::hidden`).
//!
//! The page is live (`live`): while it is open, every listed file is watched (its size and how it can be read are
//! looked at again when it changes), and the database on show is watched too: when it changes, its tables are counted
//! again and the page of rows on show is read again, with its filter and its place kept. Nothing is read on a timer,
//! and there is no Look again.

pub mod stores;
mod view;

use std::time::Duration;

use iced::{Subscription, Task};

use crate::checkout::Places;
use crate::live::{Freshness, Seen, Source, Watch};
use stores::{Overview, Page, STORES, Shelf};

pub use view::view;

/// Rows a page shows.
pub const PAGE: i64 = 50;
/// The shortest time between two readings of the database on show that its changes ask for: counting its tables'
/// rows is the dearest read here, and the bus is written often.
pub const STORE_GAP: Duration = Duration::from_secs(2);

pub struct State {
    pub places: Result<Places, String>,
    /// Each store's file, as last looked at, in the order of `STORES`.
    pub shelf: Vec<Shelf>,
    pub store: Option<usize>,
    pub overview: Option<Result<Overview, String>>,
    pub table: Option<String>,
    pub filter: String,
    /// The filter the page on show was read with.
    pub filtered: String,
    pub offset: i64,
    pub page: Option<Result<Page, String>>,
    /// A row opened whole, by its place on the page.
    pub detail: Option<usize>,
    pub loading: bool,
    /// When the list of files, and the database on show, last changed.
    pub shelf_fresh: Freshness,
    pub store_fresh: Freshness,
    asked: u64,
    /// A store (and table) asked for at start (`--tab relay/receipt`), opened once the files are looked at.
    want: Option<(usize, Option<String>)>,
}

#[derive(Clone, Debug)]
pub enum Msg {
    /// Look at every store's file again (the page opened).
    Refresh,
    /// A listed file changed (`live`).
    Live(Seen),
    /// The database on show changed (`live`): its tables and the rows on show are read again.
    StoreLive(Seen),
    /// That reading: the overview, and the page of rows on show (when a table is open), for the reading asked.
    Reread(u64, Result<Overview, String>, Option<Result<Page, String>>),
    Shelved(Vec<Shelf>),
    Store(usize),
    Overview(u64, Result<Overview, String>),
    Table(String),
    Filter(String),
    Search,
    Older,
    Newer,
    Rows(u64, Result<Page, String>),
    Detail(Option<usize>),
}

impl State {
    pub fn new() -> State {
        State {
            places: Places::find(),
            shelf: Vec::new(),
            store: None,
            overview: None,
            table: None,
            filter: String::new(),
            filtered: String::new(),
            offset: 0,
            page: None,
            detail: None,
            loading: false,
            shelf_fresh: Freshness::waiting(),
            store_fresh: Freshness::waiting(),
            asked: 0,
            want: None,
        }
    }

    /// Open this store (and table) once the page has looked at the files: `--tab` at start.
    pub fn want(&mut self, name: &str) {
        self.want = stores::named(name);
    }

    /// The page opened: look at the files (sizes and how each can be read; nothing is opened yet).
    pub fn open(&mut self) -> Task<Msg> {
        let Ok(places) = self.places.clone() else { return Task::none() };
        Task::perform(off_thread(move || stores::shelf(&places)), |shelf| Msg::Shelved(shelf.unwrap_or_default()))
    }

    pub fn busy(&self) -> bool {
        self.loading
    }

    fn ask(&mut self) -> u64 {
        self.asked += 1;
        self.loading = true;
        self.asked
    }

    fn read_page(&mut self) -> Task<Msg> {
        let (Some(i), Some(table), Ok(places)) = (self.store, self.table.clone(), self.places.clone()) else { return Task::none() };
        let asked = self.ask();
        let (filter, offset) = (self.filter.trim().to_string(), self.offset);
        self.filtered = filter.clone();
        self.detail = None;
        Task::perform(off_thread(move || stores::page(&STORES[i], &places, &table, &filter, offset, PAGE)), move |page| {
            Msg::Rows(asked, page.unwrap_or_else(|| Err("the reading thread ended without an answer".into())))
        })
    }

    /// The database on show changed: count its tables again and read the rows on show again, keeping the table, the
    /// filter it was read with, the place and the row opened. No spinner: what is on show is replaced when it arrives.
    fn reread_store(&mut self) -> Task<Msg> {
        let (Some(i), Ok(places)) = (self.store, self.places.clone()) else { return Task::none() };
        self.asked += 1;
        let asked = self.asked;
        let (table, filter, offset) = (self.table.clone(), self.filtered.clone(), self.offset);
        Task::perform(
            off_thread(move || {
                let overview = stores::overview(&STORES[i], &places);
                let page = table.map(|t| stores::page(&STORES[i], &places, &t, &filter, offset, PAGE));
                (overview, page)
            }),
            move |r| match r {
                Some((overview, page)) => Msg::Reread(asked, overview, page),
                None => Msg::Reread(asked, Err("the reading thread ended without an answer".into()), None),
            },
        )
    }

    /// The page's watches, held only while it is open: every listed file, and the database on show.
    pub fn subscription(&self) -> Subscription<Msg> {
        let Ok(places) = &self.places else { return Subscription::none() };
        let files: Vec<(String, Source)> =
            STORES.iter().filter_map(|s| s.path(places).map(|p| (s.title.to_string(), Source::Sqlite(p)))).collect();
        let mut subs = vec![Watch::new("data-shelf", files).subscription().map(Msg::Live)];
        if let Some(path) = self.store.and_then(|i| STORES[i].path(places)) {
            let title = STORES[self.store.unwrap_or(0)].title.to_string();
            subs.push(Watch::new("data-store", vec![(title, Source::Sqlite(path))]).gap(STORE_GAP).subscription().map(Msg::StoreLive));
        }
        Subscription::batch(subs)
    }

    pub fn update(&mut self, msg: Msg) -> Task<Msg> {
        match msg {
            Msg::Refresh => return self.open(),
            Msg::Live(seen) => {
                self.shelf_fresh.saw(&seen);
                if !seen.first {
                    return self.open();
                }
            }
            Msg::StoreLive(seen) => {
                self.store_fresh.saw(&seen);
                if !seen.first {
                    return self.reread_store();
                }
            }
            Msg::Reread(asked, overview, page) => {
                if asked == self.asked {
                    self.loading = false;
                    // the table on show may have gone; then its rows go with it
                    let still = self.table.as_ref().is_some_and(|t| matches!(&overview, Ok(o) if o.tables.iter().any(|x| &x.name == t)));
                    self.overview = Some(overview);
                    if still {
                        if let Some(page) = page {
                            if self.detail.is_some_and(|d| !matches!(&page, Ok(p) if d < p.rows.len())) {
                                self.detail = None;
                            }
                            self.page = Some(page);
                        }
                    } else {
                        self.table = None;
                        self.page = None;
                        self.detail = None;
                    }
                }
            }
            Msg::Shelved(shelf) => {
                self.shelf = shelf;
                // Once: the store now, and its table when the store's overview arrives.
                if let Some((i, table)) = self.want.take() {
                    let task = self.update(Msg::Store(i));
                    self.want = table.map(|t| (i, Some(t)));
                    return task;
                }
            }
            Msg::Store(i) if i < STORES.len() => {
                let Ok(places) = self.places.clone() else { return Task::none() };
                self.store = Some(i);
                self.store_fresh = Freshness::waiting();
                self.overview = None;
                self.table = None;
                self.page = None;
                self.filter.clear();
                self.offset = 0;
                let asked = self.ask();
                return Task::perform(off_thread(move || stores::overview(&STORES[i], &places)), move |o| {
                    Msg::Overview(asked, o.unwrap_or_else(|| Err("the reading thread ended without an answer".into())))
                });
            }
            Msg::Store(_) => {}
            Msg::Overview(asked, overview) => {
                if asked == self.asked {
                    self.loading = false;
                    self.overview = Some(overview);
                    if let Some((_, Some(table))) = self.want.take() {
                        return self.update(Msg::Table(table));
                    }
                }
            }
            Msg::Table(name) => {
                // Only a table the store's own overview listed: the closed vocabulary of names a query may use.
                let known = matches!(&self.overview, Some(Ok(o)) if o.tables.iter().any(|t| t.name == name));
                if known {
                    self.table = Some(name);
                    self.filter.clear();
                    self.offset = 0;
                    return self.read_page();
                }
            }
            Msg::Filter(words) => self.filter = words,
            Msg::Search => {
                self.offset = 0;
                return self.read_page();
            }
            Msg::Older => {
                self.offset += PAGE;
                return self.read_page();
            }
            Msg::Newer => {
                self.offset = (self.offset - PAGE).max(0);
                return self.read_page();
            }
            Msg::Rows(asked, page) => {
                if asked == self.asked {
                    self.loading = false;
                    self.page = Some(page);
                }
            }
            Msg::Detail(row) => self.detail = row,
        }
        Task::none()
    }
}

impl Default for State {
    fn default() -> State {
        State::new()
    }
}

/// `work` on a thread of its own, awaited without holding the window's.
async fn off_thread<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    let (tx, rx) = futures::channel::oneshot::channel();
    let _ = std::thread::Builder::new().name("centcom-data".into()).spawn(move || {
        let _ = tx.send(work());
    });
    rx.await.ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use stores::TableInfo;

    #[test]
    fn only_a_table_the_overview_listed_can_be_asked_for() {
        let mut state = State::new();
        state.store = Some(0);
        state.overview = Some(Ok(Overview { access: None, tables: vec![TableInfo { name: "finding".into(), rows: 3, columns: vec![] }] }));
        let _ = state.update(Msg::Table("sqlite_master; DROP TABLE finding".into()));
        assert!(state.table.is_none(), "a name the overview did not list is ignored");
        let _ = state.update(Msg::Table("finding".into()));
        assert_eq!(state.table.as_deref(), Some("finding"));
    }

    #[test]
    fn a_change_to_the_database_on_show_reads_it_again_keeping_the_table_and_a_table_gone_closes() {
        let mut state = State::new();
        state.store = Some(0);
        state.table = Some("finding".into());
        state.offset = 50;
        state.detail = Some(3);
        let seen = |first| Seen { at: 0.0, first, missing: vec![], of: 1 };
        let _ = state.update(Msg::StoreLive(seen(true)));
        assert_eq!(state.asked, 0, "the first word is what the page already read");
        let _ = state.update(Msg::StoreLive(seen(false)));
        assert_eq!(state.asked, 1, "a change asks for one reading");
        let page = Page {
            table: "finding".into(),
            columns: vec!["a".into()],
            rows: vec![vec![]; 5],
            offset: 50,
            matching: None,
            newest_first: true,
            access: crate::sqlite_ro::Access::ReadOnly,
        };
        let listed = Overview { access: None, tables: vec![TableInfo { name: "finding".into(), rows: 55, columns: vec![] }] };
        let _ = state.update(Msg::Reread(0, Ok(listed.clone()), Some(Ok(page.clone()))));
        assert!(state.page.is_none(), "an older reading is dropped");
        let _ = state.update(Msg::Reread(1, Ok(listed), Some(Ok(page))));
        assert_eq!((state.table.as_deref(), state.offset, state.detail), (Some("finding"), 50, Some(3)), "kept in place");
        assert!(state.page.is_some());
        let _ = state.update(Msg::StoreLive(seen(false)));
        let _ = state.update(Msg::Reread(2, Ok(Overview { access: None, tables: vec![] }), None));
        assert!(state.table.is_none() && state.page.is_none() && state.detail.is_none(), "the table went: its rows go too");
    }

    #[test]
    fn paging_never_goes_before_the_newest_rows() {
        let mut state = State::new();
        let _ = state.update(Msg::Newer);
        assert_eq!(state.offset, 0);
        state.offset = 10;
        let _ = state.update(Msg::Newer);
        assert_eq!(state.offset, 0);
    }
}
