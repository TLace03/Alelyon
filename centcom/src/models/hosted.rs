//! Hosted open-weight models in the Models panel (easy access to hosted open-weight
//! models, in the manner of OpenRouter and HuggingFace), on lattice-core's `hosted`.
//!
//! Each provider is a card: connected or not, and how to connect. OpenRouter signs in in the person's own browser
//! (they approve a key for Lattice there; it comes back to this PC, `hosted::openrouter`); the others take a key made
//! on the provider's page and pasted once. Either way the key goes to the panel's keeper (Windows Credential Manager
//! as `Alelyon/<NAME>`), never to a file, a log or the screen, and the provider's registry row is switched on.
//! Browse models reads the provider's list (open-weight chat models only); Use for Lattice chat or Use for Sinai sets
//! the row's model, then switches as every endpoint of the panel switches.
//!
//! Nothing here is contacted until the person presses a button. No timer: a sign-in waits until the browser comes
//! back or Cancel.

use std::collections::HashMap;

use futures::future::{AbortHandle, Abortable};
use iced::Task;

use lattice_core::hosted::{self, HostedModel, PROVIDERS, Provider, SignIn};
use lattice_core::keys::SecretString;

use super::{Catalog, Pick, Sources, Surface, off_thread, read};

/// The panel's hosted part.
#[derive(Debug, Default)]
pub struct Hosted {
    /// A key being typed, by provider; forgotten once it is kept.
    typed: HashMap<&'static str, String>,
    /// The provider whose models are shown, and its list as read.
    pub browsing: Option<&'static str>,
    pub models: HashMap<&'static str, Result<Vec<HostedModel>, String>>,
    /// Shows only models whose id or name holds this.
    pub filter: String,
    /// A provider being connected or listed.
    pub busy: Option<&'static str>,
    /// OpenRouter's sign-in under way: Cancel ends it.
    signing_in: Option<AbortHandle>,
}

#[derive(Clone, Debug)]
pub enum Msg {
    Typed(&'static str, String),
    /// Open the provider's page for making a key, in the person's own browser.
    KeyPage(&'static str),
    Connect(&'static str),
    SignIn,
    CancelSignIn,
    /// A connect or sign-in ended: the catalogue read again, or why not.
    Connected(&'static str, Result<Box<Catalog>, String>),
    Browse(&'static str),
    Listed(&'static str, Result<Vec<HostedModel>, String>),
    Filter(String),
    /// Use this model of the provider for a surface.
    Use(&'static str, String, Surface),
    /// The row's model is set and the catalogue read again: switch as the panel switches.
    Chosen(Surface, &'static str, Result<Box<Catalog>, String>),
}

/// What the hosted part asks of the panel after a message.
pub enum Then {
    Nothing,
    Task(Task<Msg>),
    /// Say this on the panel (a plain sentence; true: an error).
    Say(String, bool),
    /// The catalogue as read again, with a sentence to say.
    Catalogued(Box<Catalog>, String),
    /// The catalogue as read again, then this switch.
    Switch(Box<Catalog>, Surface, Pick),
}

/// The provider with this id (only the catalogue's ids are ever sent).
fn provider(id: &str) -> &'static Provider {
    hosted::provider(id).expect("a provider of the catalogue")
}

/// Keep the typed key and switch the provider's row on; the catalogue read again.
pub fn connect(sources: &Sources, provider: &Provider, key: &SecretString) -> Result<Catalog, String> {
    if key.expose().trim().is_empty() {
        return Err("Paste the key first.".into());
    }
    sources.keeper.keep(provider.key_name, key)?;
    if let Err(why) = hosted::enable(&sources.registry, provider) {
        let _ = sources.keeper.forget(provider.key_name);
        return Err(why);
    }
    Ok(read(sources))
}

/// Set the provider's model on its row; the catalogue read again.
pub fn choose(sources: &Sources, provider: &Provider, model: &str) -> Result<Catalog, String> {
    hosted::choose(&sources.registry, provider, model)?;
    Ok(read(sources))
}

/// A runtime of its own for one request off the window's thread.
fn block_on<T>(work: impl std::future::Future<Output = Result<T, String>>) -> Result<T, String> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| "Lattice could not start the request.".to_string())?
        .block_on(work)
}

/// The models of a list that hold the filter, in its order.
pub fn shown<'a>(models: &'a [HostedModel], filter: &str) -> Vec<&'a HostedModel> {
    let filter = filter.trim().to_lowercase();
    models
        .iter()
        .filter(|m| filter.is_empty() || m.id.to_lowercase().contains(&filter) || m.name.to_lowercase().contains(&filter))
        .collect()
}

impl Hosted {
    pub fn typed(&self, id: &str) -> &str {
        self.typed.get(id).map(String::as_str).unwrap_or("")
    }

    pub fn signing_in(&self) -> bool {
        self.signing_in.is_some()
    }

    pub fn working(&self) -> bool {
        self.busy.is_some()
    }

    pub fn update(&mut self, msg: Msg, sources: &Sources) -> Then {
        match msg {
            Msg::Typed(id, value) => {
                self.typed.insert(id, value);
            }
            Msg::KeyPage(id) => {
                if let Err(why) = crate::signin::web::open_yours(provider(id).key_page) {
                    return Then::Say(why, true);
                }
            }
            Msg::Connect(id) => {
                if self.busy.is_some() {
                    return Then::Nothing;
                }
                let key = SecretString::from(self.typed.get(id).map(|k| k.trim().to_string()).unwrap_or_default());
                if key.expose().is_empty() {
                    return Then::Say(format!("Paste your {} key first.", provider(id).label), true);
                }
                self.busy = Some(id);
                let sources = sources.clone();
                return Then::Task(Task::perform(off_thread(move || connect(&sources, provider(id), &key)), move |r| {
                    Msg::Connected(id, r.unwrap_or_else(|| Err("It was not connected: its thread stopped.".into())).map(Box::new))
                }));
            }
            Msg::SignIn => {
                if self.busy.is_some() {
                    return Then::Nothing;
                }
                let id = "openrouter";
                let (handle, registration) = AbortHandle::new_pair();
                self.busy = Some(id);
                self.signing_in = Some(handle);
                let sources = sources.clone();
                let job = move || -> Result<Catalog, String> {
                    let key = block_on(async move {
                        let pending = hosted::openrouter::begin().await?;
                        crate::signin::web::open_yours(pending.url())?;
                        let web = hosted::Web::new()?;
                        Abortable::new(pending.finish(&web), registration)
                            .await
                            .unwrap_or_else(|_| Err("The sign-in was cancelled.".to_string()))
                    })?;
                    connect(&sources, provider(id), &key)
                };
                return Then::Task(Task::perform(off_thread(job), move |r| {
                    Msg::Connected(id, r.unwrap_or_else(|| Err("The sign-in stopped.".into())).map(Box::new))
                }));
            }
            Msg::CancelSignIn => {
                if let Some(handle) = self.signing_in.take() {
                    handle.abort();
                }
            }
            Msg::Connected(id, result) => {
                self.busy = None;
                self.signing_in = None;
                let label = provider(id).label;
                return match result {
                    Ok(catalog) => {
                        // the key typed is forgotten here
                        self.typed.remove(id);
                        Then::Catalogued(catalog, format!("{label} is connected: Browse models to choose one."))
                    }
                    Err(why) => Then::Say(why, true),
                };
            }
            Msg::Browse(id) => {
                if self.browsing == Some(id) && self.models.get(id).is_some_and(Result::is_ok) {
                    // pressed again: closes the list
                    self.browsing = None;
                    return Then::Nothing;
                }
                if self.busy.is_some() {
                    return Then::Nothing;
                }
                self.browsing = Some(id);
                self.filter.clear();
                self.busy = Some(id);
                let key = sources.keys().get(provider(id).key_name);
                return Then::Task(Task::perform(
                    off_thread(move || {
                        block_on(async move {
                            let web = hosted::Web::new()?;
                            hosted::list_models(&web, provider(id), key).await
                        })
                    }),
                    move |r| Msg::Listed(id, r.unwrap_or_else(|| Err("The list was not read: its thread stopped.".into()))),
                ));
            }
            Msg::Listed(id, result) => {
                self.busy = None;
                self.models.insert(id, result);
            }
            Msg::Filter(value) => self.filter = value,
            Msg::Use(id, model, surface) => {
                if self.busy.is_some() {
                    return Then::Nothing;
                }
                self.busy = Some(id);
                let sources = sources.clone();
                return Then::Task(Task::perform(off_thread(move || choose(&sources, provider(id), &model)), move |r| {
                    Msg::Chosen(surface, id, r.unwrap_or_else(|| Err("The model was not chosen: its thread stopped.".into())).map(Box::new))
                }));
            }
            Msg::Chosen(surface, id, result) => {
                self.busy = None;
                return match result {
                    Ok(catalog) => Then::Switch(catalog, surface, Pick::Endpoint(id.to_string())),
                    Err(why) => Then::Say(why, true),
                };
            }
        }
        Then::Nothing
    }
}

/// The providers in the order they are offered.
pub fn providers() -> &'static [Provider] {
    PROVIDERS
}

/// How the provider connects, for its card.
pub fn signs_in(provider: &Provider) -> bool {
    provider.sign_in == SignIn::OpenRouter
}
