//! The Tools page and the Tools view: the agent's own tools, and the MCP servers it may use (alongside Projects,
//! plugins, tools and MCPs, since 2026-10-08).
//!
//! The servers are lattice-core's: declared in the reader's own MCP settings (Cursor's
//! `mcpServers` shape) or, in a trusted folder, in its `.lattice/mcp.json`, `.mcp.json` or `.cursor/mcp.json`; enabled
//! only through the core's own dialog, pinned to the entry's exact text (any change asks again); started at the
//! agent's first turn that offers their tools, or here on Start; each call asks unless the reader allowed that tool
//! always. This page only shows what the core reports and asks the core to act: it starts and decides nothing itself.
//!
//! A server is added or edited as JSON, as its instructions give it; an edited entry shows each environment value as
//! `<kept>` (the core keeps the value the file has), so a key never appears here.
//!
//! The agent's own browser is switched on and off here, shown (to sign in to a site yourself, or to watch) and
//! stopped; the core starts it at the agent's first browser action and asks before posting, buying, deleting and
//! account changes.

use std::collections::HashSet;

use iced::widget::{Column, Row, button, column, container, row, scrollable, space, text_editor, text_input};
use iced::{Alignment, Element, Length, Task};

use crate::theme::{self, fonts};
use lattice_core::browser::BrowserStatus;
use lattice_core::mcp::approvals::WHOLE_SERVER;
use lattice_core::mcp::config::{self as mcp_config, Scope, ServerKey};
use lattice_core::mcp::hub::{Overview, ServerStatus, ServerView, ToolView};

use super::{IdeMsg, TabKind};
use crate::lattice::{Msg, STOPPED, State, on};
use crate::ui::{self, label, mono, note, strong};

type El<'a> = Element<'a, Msg>;

/// What a new server's entry starts as.
pub const TEMPLATE: &str = "{\n  \"command\": \"\",\n  \"args\": [],\n  \"env\": {}\n}\n";

/// The agent's own tools, as the page lists them: their names, what they do, and in which modes.
pub const BUILT_IN: [(&str, &str, &str); 6] = [
    ("list_dir, glob, read_file, grep", "Read the folder as the chat sees it (your staged changes included).", "Ask and Agent"),
    ("ask_question", "Asks you, and waits for your answer.", "Ask and Agent"),
    ("use_skill, read_skill_file", "Read a skill's instructions and its files, when there is a skill (below).", "Ask and Agent"),
    ("edit_file, write_file, delete_file", "Stage changes; nothing is written until you keep them in Changes.", "Agent"),
    ("run_command", "One Windows PowerShell command, after you approve it.", "Agent"),
    (
        "browser_look, browser_open, browser_click, browser_type, browser_key, browser_scroll, browser_back",
        "Use the agent's own web browser as a person does; posting, buying, deleting and account changes ask you first.",
        "Agent, with the browser on",
    ),
];

/// What the Plugins section says.
pub const PLUGINS_ABOUT: &str = "A plugin is a folder in Claude Code's plugin layout (a .claude-plugin/plugin.json beside its commands, skills, agents, hooks and MCP servers). Add one from its folder: while it is on, its commands (/plugin:name) and skills join yours in every chat. Its MCP servers are only listed until you copy them into your settings, where each asks before it first starts. Its agents are not used, and its hooks never run.";

/// What a plugin brings, in a line.
pub fn plugin_parts(view: &lattice_core::plugins::PluginView) -> String {
    let count = |n: usize, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
    let mut parts = vec![
        count(view.commands.len(), "command", "commands"),
        count(view.skills.len(), "skill", "skills"),
    ];
    if !view.mcp_servers.is_empty() {
        parts.push(format!("MCP: {}", view.mcp_servers.join(", ")));
    }
    if !view.agents.is_empty() {
        parts.push(format!("{} (not used)", count(view.agents.len(), "agent", "agents")));
    }
    if view.hooks {
        parts.push("hooks (never run)".to_string());
    }
    parts.join(" \u{00B7} ")
}

/// What copying a plugin's MCP servers did, in words.
pub fn copied_words(copied: &lattice_core::plugins::Copied) -> String {
    let mut words = Vec::new();
    if !copied.enabled.is_empty() {
        words.push(format!("Copied to your MCP settings and enabled: {}.", copied.enabled.join(", ")));
    }
    if !copied.not_enabled.is_empty() {
        words.push(format!("Copied, not enabled: {}.", copied.not_enabled.join(", ")));
    }
    if !copied.kept.is_empty() {
        words.push(format!("Your settings already hold {}, left as you have them.", copied.kept.join(", ")));
    }
    words.join(" ")
}

/// What the Skills section says.
pub const SKILLS_ABOUT: &str = "A skill is a folder holding a SKILL.md: instructions for a kind of task, with a name and a description. Every agent turn lists them, and the agent reads one (use_skill) when a task calls for it. A skill is text: it never widens what the agent may do.";

/// What the Commands section says.
pub const COMMANDS_ABOUT: &str = "A command is a Markdown file of text you run from the composer: type / and its name, and its text goes into the composer, what you type after the name filling in its arguments ($ARGUMENTS, $1 to $9), for you to read before you send it. A command runs nothing; settings such as allowed-tools are set aside.";

/// What the browser section says it is for.
pub const BROWSER_ABOUT: &str = "A web browser of the agent's own (Microsoft Edge, or Chrome, with a profile of its own). Switched on, the agent can open websites, look at them, and click, type and scroll as a person does, in Agent mode with a model that can see images. Posting, messaging, buying, deleting and account or consent changes ask you first, and it never types a password or card details: sign in to a site yourself in its window.";

/// What the browser section says about what it keeps and sends.
pub const BROWSER_PRIVACY: &str = "Its sign-ins stay in its own profile on this PC, kept by the browser as Edge keeps yours; Lattice never reads them. Its screenshots go to the model you chose: with a model off this PC, they leave it.";

/// The add or edit form.
#[derive(Debug)]
pub struct Form {
    /// The server's name (fixed when editing).
    pub name: String,
    /// The name of the server being edited, if any.
    pub editing: Option<String>,
    pub json: text_editor::Content,
    pub error: Option<String>,
    pub saving: bool,
}

/// The page's state.
#[derive(Debug, Default)]
pub struct Tools {
    /// What the core reported last.
    pub overview: Option<Overview>,
    pub reading: bool,
    /// Servers shown open (by key id).
    pub expanded: HashSet<String>,
    /// Servers with an action in flight (by key id).
    pub busy: HashSet<String>,
    pub form: Option<Form>,
    /// The last action's sentence, and whether it warns.
    pub said: Option<(String, bool)>,
    /// The agent's browser as the core reported it last: switched on, its state, and its profile folder.
    pub browser: Option<(bool, BrowserStatus, String)>,
    /// A browser action is in flight.
    pub browser_busy: bool,
    /// Auto mode as the core reported it last: on, and its stop keys armed.
    pub auto: Option<(bool, bool)>,
    /// The skills and commands, as the core read them last.
    pub skills: Option<lattice_core::skills::Skills>,
    pub commands: Option<lattice_core::commands::Commands>,
    /// The plugins, as the core read them last.
    pub plugins: Option<Vec<lattice_core::plugins::PluginView>>,
    /// A plugin action (or Windows' picker) is in flight.
    pub plugin_busy: bool,
    /// The labs' agents as the core reported them: each, installed, and Node found.
    pub agents: Option<Vec<(lattice_core::acp::Agent, bool, bool)>>,
    /// The agent being installed.
    pub agent_busy: Option<lattice_core::acp::Agent>,
    /// Each agent's offered models and the reader's pick, as last read.
    pub agent_models: Vec<(lattice_core::acp::Agent, lattice_core::acp::models::Offered, Option<String>)>,
    /// The agent whose models are shown to choose from.
    pub choosing: Option<lattice_core::acp::Agent>,
    /// The agent whose models are being checked (it is started to read them).
    pub checking: Option<lattice_core::acp::Agent>,
    /// The open folder (its id) and the agent's notes about it; `None` before they are read.
    pub memory: Option<(Option<String>, Vec<lattice_core::convo::memory::Note>)>,
}

/// What the page asks.
#[derive(Debug, Clone)]
pub enum ToolsMsg {
    /// Read the servers again.
    Refresh,
    Read(Option<Overview>),
    /// The core says a server changed.
    Changed,
    /// Open (or show) the Tools page, a server open on it.
    Open(Option<String>),
    /// These servers are shown open (a photograph's start).
    Opened(Vec<String>),
    /// Open the Tools page on the add form.
    OpenAdd,
    Expand(String),
    Enable(ServerKey),
    Disable(ServerKey),
    Start(ServerKey),
    Stop(ServerKey),
    Restart(ServerKey),
    /// A tool (or the whole server, `*`) on or off.
    Switch(ServerKey, String, bool),
    Allow(ServerKey, String),
    Disallow(ServerKey, String),
    Remove(String),
    /// An action ended: the server it was about, and what to say.
    Acted(String, Result<Option<String>, String>),
    // The form.
    Add,
    Edit(String),
    Masked(String, Option<serde_json::Value>),
    Name(String),
    Json(text_editor::Action),
    Save,
    Saved(Result<String, String>),
    Cancel,
    // The agent's browser.
    /// The core says the browser changed.
    BrowserChanged,
    BrowserRead(bool, BrowserStatus, String),
    /// Switch it on or off.
    BrowserSet(bool),
    /// Show its window (started now if it is not).
    BrowserShow,
    /// Show it at this address: a service's sign-in page (Connections).
    BrowserShowAt(String),
    /// Open (or show) the Connections page.
    OpenConnections,
    BrowserStop,
    BrowserActed(Result<Option<String>, String>),
    // The labs' agents (Claude Code, Codex) on the reader's own subscriptions.
    AgentsRead(Vec<(lattice_core::acp::Agent, bool, bool)>),
    // The model each agent answers with.
    AgentModelsRead(Vec<(lattice_core::acp::Agent, lattice_core::acp::models::Offered, Option<String>)>),
    ChooseModel(lattice_core::acp::Agent),
    CheckModels(lattice_core::acp::Agent),
    ModelsChecked(lattice_core::acp::Agent, Result<(), String>),
    PickModel(lattice_core::acp::Agent, Option<String>),
    CloseModels,
    // What the agent remembers about the open folder.
    MemoryRead(Option<String>, Vec<lattice_core::convo::memory::Note>),
    Forget(String, String),
    Forgot(Result<bool, String>),
    AgentInstall(lattice_core::acp::Agent),
    AgentInstalled(lattice_core::acp::Agent, Result<(), String>),
    // Auto mode.
    AutoRead(bool, bool),
    /// Switch it on (the core asks in its own dialog) or off.
    AutoSet(bool),
    AutoActed(Result<bool, String>),
    // Skills, commands and plugins.
    ExtrasRead(
        Box<(
            lattice_core::commands::Commands,
            lattice_core::skills::Skills,
            Vec<lattice_core::plugins::PluginView>,
        )>,
    ),
    /// Choose a plugin's folder in Windows' picker.
    PluginBrowse,
    PluginPicked(Result<Option<std::path::PathBuf>, String>),
    PluginSet(String, bool),
    PluginRemove(String),
    /// Copy a plugin's MCP servers into the reader's settings.
    PluginMcp(String),
    PluginActed(Result<Option<String>, String>),
}

fn go(msg: ToolsMsg) -> Msg {
    Msg::Ide(IdeMsg::Tools(msg))
}

/// A status's dot colour and words.
pub fn status_words(view: &ServerView) -> (iced::Color, String) {
    if !view.enabled {
        return (theme::TEXT_FAINT, "Not enabled".to_string());
    }
    if view.off {
        return (theme::TEXT_FAINT, "Switched off".to_string());
    }
    match &view.status {
        ServerStatus::Stopped => (theme::TEXT_DIM, "Starts at the agent's next turn".to_string()),
        ServerStatus::Starting => (theme::GOLD, "Starting\u{2026}".to_string()),
        ServerStatus::Running => {
            let on = view.tools.iter().filter(|t| t.on).count();
            (theme::POSITIVE, format!("Running \u{00B7} {on} of {} tools on", view.tools.len()))
        }
        ServerStatus::Failed(_) => (theme::DANGER, "Stopped with an error".to_string()),
    }
}

/// The browser's dot colour and words.
pub fn browser_words(on: bool, status: &BrowserStatus) -> (iced::Color, String) {
    match status {
        BrowserStatus::Starting => (theme::GOLD, "Starting\u{2026}".to_string()),
        BrowserStatus::Running { url, title } => {
            let page = if title.is_empty() { url.clone() } else { title.clone() };
            (theme::POSITIVE, format!("Open \u{00B7} {}", ui::cut(&page, 80)))
        }
        BrowserStatus::Failed(_) => (theme::DANGER, "Stopped with an error".to_string()),
        BrowserStatus::Stopped if on => (theme::TEXT_DIM, "On \u{00B7} starts at the agent's first browser action".to_string()),
        BrowserStatus::Stopped => (theme::TEXT_FAINT, "Off".to_string()),
    }
}

/// Where a server is declared, as the page says it.
pub fn source_words(view: &ServerView) -> String {
    match &view.key.scope {
        Scope::User => "Your MCP settings".to_string(),
        Scope::Folder { .. } => format!("This folder's {}", view.file),
    }
}

impl State {
    /// The page's actions.
    pub(in crate::lattice) fn tools(&mut self, msg: ToolsMsg) -> Task<Msg> {
        match msg {
            ToolsMsg::Refresh | ToolsMsg::Changed => return self.read_tools(),
            ToolsMsg::Read(overview) => {
                self.ide.tools.reading = false;
                if let Some(overview) = overview {
                    self.ide.tools.overview = Some(overview);
                }
            }
            ToolsMsg::Open(server) => {
                if let Some(id) = server {
                    self.ide.tools.expanded.insert(id);
                }
                let existing = self.ide.tabs.iter().find(|t| matches!(t.kind, TabKind::Tools)).map(|t| t.id);
                let id = match existing {
                    Some(id) => id,
                    None => self.ide.push(TabKind::Tools),
                };
                self.ide.active = Some(id);
                return self.read_tools();
            }
            ToolsMsg::Expand(id) => {
                if !self.ide.tools.expanded.remove(&id) {
                    self.ide.tools.expanded.insert(id);
                }
            }
            ToolsMsg::Opened(ids) => {
                self.ide.tools.expanded.extend(ids);
                return self.read_tools();
            }
            ToolsMsg::OpenAdd => {
                let open = self.tools(ToolsMsg::Open(None));
                if self.ide.tools.form.is_none() {
                    let _ = self.tools(ToolsMsg::Add);
                }
                return open;
            }
            ToolsMsg::Enable(key) => {
                let workspace = self.chat_workspace();
                return self.mcp_act(key, move |chat, key| {
                    Box::pin(async move {
                        match chat.mcp_enable(key, workspace).await {
                            Ok(true) => Ok(Some("Enabled. It starts at the agent's next turn in Agent mode.".to_string())),
                            Ok(false) => Ok(Some("Not enabled.".to_string())),
                            Err(r) => Err(r.message),
                        }
                    })
                });
            }
            ToolsMsg::Disable(key) => {
                return self.mcp_act(key, |chat, key| {
                    Box::pin(async move {
                        chat.mcp_disable(key)
                            .await
                            .map(|()| Some("Disabled: it asks again before it next starts.".to_string()))
                            .map_err(|r| r.message)
                    })
                });
            }
            ToolsMsg::Start(key) => {
                let workspace = self.chat_workspace();
                return self.mcp_act(key, move |chat, key| {
                    Box::pin(async move { chat.mcp_start(key, workspace).await.map(|()| None).map_err(|r| r.message) })
                });
            }
            ToolsMsg::Stop(key) => {
                return self.mcp_act(key, |chat, key| {
                    Box::pin(async move {
                        chat.mcp_stop(key).await;
                        Ok(None)
                    })
                });
            }
            ToolsMsg::Restart(key) => {
                let workspace = self.chat_workspace();
                return self.mcp_act(key, move |chat, key| {
                    Box::pin(async move {
                        chat.mcp_stop(key.clone()).await;
                        chat.mcp_start(key, workspace).await.map(|()| None).map_err(|r| r.message)
                    })
                });
            }
            ToolsMsg::Switch(key, tool, on) => {
                return self.mcp_act(key, move |chat, key| {
                    Box::pin(async move { chat.mcp_switch(key, tool, on).await.map(|()| None).map_err(|r| r.message) })
                });
            }
            ToolsMsg::Allow(key, tool) => {
                let workspace = self.chat_workspace();
                return self.mcp_act(key, move |chat, key| {
                    Box::pin(async move {
                        match chat.mcp_allow(key, tool.clone(), workspace).await {
                            Ok(true) => Ok(Some(format!("{tool} runs without asking now."))),
                            Ok(false) => Ok(None),
                            Err(r) => Err(r.message),
                        }
                    })
                });
            }
            ToolsMsg::Disallow(key, tool) => {
                return self.mcp_act(key, move |chat, key| {
                    Box::pin(async move {
                        chat.mcp_disallow(key, tool.clone())
                            .await
                            .map(|()| Some(format!("{tool} asks before each call again.")))
                            .map_err(|r| r.message)
                    })
                });
            }
            ToolsMsg::Remove(name) => {
                return self.mcp_act(ServerKey::user(&name), |chat, key| {
                    Box::pin(async move {
                        chat.mcp_remove(key.name.clone())
                            .await
                            .map(|()| Some(format!("{} was taken out of your MCP settings.", key.name)))
                            .map_err(|r| r.message)
                    })
                });
            }
            ToolsMsg::Acted(id, result) => {
                self.ide.tools.busy.remove(&id);
                match result {
                    Ok(Some(words)) => self.ide.tools.said = Some((words, false)),
                    Ok(None) => {}
                    Err(why) => self.ide.tools.said = Some((why, true)),
                }
                return self.read_tools();
            }
            ToolsMsg::Add => {
                self.ide.tools.form = Some(Form {
                    name: String::new(),
                    editing: None,
                    json: text_editor::Content::with_text(TEMPLATE),
                    error: None,
                    saving: false,
                });
            }
            ToolsMsg::Edit(name) => {
                let Some(services) = self.services.clone() else { return Task::none() };
                let chat = services.chat.clone();
                let asked = name.clone();
                return Task::perform(on(&services, async move { chat.mcp_masked(asked).await }), move |entry| {
                    go(ToolsMsg::Masked(name.clone(), entry.flatten()))
                });
            }
            ToolsMsg::Masked(name, entry) => match entry {
                Some(entry) => {
                    let text = serde_json::to_string_pretty(&entry).unwrap_or_default();
                    self.ide.tools.form = Some(Form {
                        name: name.clone(),
                        editing: Some(name),
                        json: text_editor::Content::with_text(&text),
                        error: None,
                        saving: false,
                    });
                }
                None => self.ide.tools.said = Some(("That server is not in your MCP settings now.".to_string(), true)),
            },
            ToolsMsg::Name(name) => {
                if let Some(form) = &mut self.ide.tools.form
                    && form.editing.is_none()
                {
                    form.name = name;
                }
            }
            ToolsMsg::Json(action) => {
                if let Some(form) = &mut self.ide.tools.form {
                    form.json.perform(action);
                    form.error = None;
                }
            }
            ToolsMsg::Save => return self.save_form(),
            ToolsMsg::Saved(result) => match result {
                Ok(words) => {
                    self.ide.tools.form = None;
                    self.ide.tools.said = Some((words, false));
                    return self.read_tools();
                }
                Err(why) => {
                    if let Some(form) = &mut self.ide.tools.form {
                        form.saving = false;
                        form.error = Some(why);
                    }
                }
            },
            ToolsMsg::Cancel => self.ide.tools.form = None,
            ToolsMsg::BrowserChanged => return self.read_browser(),
            ToolsMsg::BrowserRead(on, status, profile) => self.ide.tools.browser = Some((on, status, profile)),
            ToolsMsg::AutoRead(on, armed) => self.ide.tools.auto = Some((on, armed)),
            ToolsMsg::AgentsRead(agents) => self.ide.tools.agents = Some(agents),
            ToolsMsg::AgentModelsRead(models) => self.ide.tools.agent_models = models,
            ToolsMsg::ChooseModel(agent) => {
                self.ide.tools.choosing = Some(agent);
                let known = self.ide.tools.agent_models.iter().any(|(a, offered, _)| *a == agent && !offered.models.is_empty());
                if !known {
                    return self.tools(ToolsMsg::CheckModels(agent));
                }
            }
            ToolsMsg::CheckModels(agent) => {
                let Some(services) = self.services.clone() else { return Task::none() };
                if self.ide.tools.checking.is_some() {
                    return Task::none();
                }
                self.ide.tools.checking = Some(agent);
                self.ide.tools.said = None;
                let chat = services.chat.clone();
                return Task::perform(on(&services, async move { chat.agent_check_models(agent).await.map(|_| ()).map_err(|r| r.message) }), move |r| {
                    go(ToolsMsg::ModelsChecked(agent, r.unwrap_or_else(|| Err(STOPPED.to_string()))))
                });
            }
            ToolsMsg::ModelsChecked(agent, result) => {
                self.ide.tools.checking = None;
                if let Err(why) = result {
                    self.ide.tools.said = Some((format!("{}'s models could not be read: {why}", agent.label()), true));
                    self.ide.tools.choosing = None;
                }
                return self.read_browser();
            }
            ToolsMsg::PickModel(agent, model) => {
                let Some(services) = self.services.clone() else { return Task::none() };
                match services.chat.agent_pick_model(agent, model.as_deref()) {
                    Ok(()) => {
                        self.ide.tools.choosing = None;
                        self.ide.tools.said = Some((
                            format!("{}'s new chats use {}.", agent.label(), model.as_deref().unwrap_or("its own default")),
                            false,
                        ));
                    }
                    Err(why) => self.ide.tools.said = Some((why, true)),
                }
                return self.read_browser();
            }
            ToolsMsg::CloseModels => self.ide.tools.choosing = None,
            ToolsMsg::MemoryRead(workspace, notes) => self.ide.tools.memory = Some((workspace, notes)),
            ToolsMsg::Forget(workspace, id) => {
                let Some(services) = self.services.clone() else { return Task::none() };
                let chat = services.chat.clone();
                return Task::perform(on(&services, async move { chat.forget_memory(&workspace, &id) }), |r| {
                    go(ToolsMsg::Forgot(r.unwrap_or_else(|| Err(STOPPED.to_string()))))
                });
            }
            ToolsMsg::Forgot(result) => {
                match result {
                    Ok(true) => self.ide.tools.said = Some(("Forgotten: no later chat starts with it.".to_string(), false)),
                    Ok(false) => self.ide.tools.said = Some(("That note was already gone.".to_string(), false)),
                    Err(why) => self.ide.tools.said = Some((why, true)),
                }
                return self.read_browser();
            }
            ToolsMsg::AgentInstall(agent) => {
                let Some(services) = self.services.clone() else { return Task::none() };
                if self.ide.tools.agent_busy.is_some() {
                    return Task::none();
                }
                self.ide.tools.agent_busy = Some(agent);
                self.ide.tools.said = None;
                let chat = services.chat.clone();
                return Task::perform(on(&services, async move { chat.agent_install(agent).await.map_err(|r| r.message) }), move |result| {
                    go(ToolsMsg::AgentInstalled(agent, result.unwrap_or_else(|| Err(STOPPED.to_string()))))
                });
            }
            ToolsMsg::AgentInstalled(agent, result) => {
                self.ide.tools.agent_busy = None;
                match result {
                    Ok(()) => {
                        self.ide.tools.said = Some((
                            format!("{} is installed: choose it in the composer's model list.", agent.label()),
                            false,
                        ))
                    }
                    Err(why) => self.ide.tools.said = Some((why, true)),
                }
                // The model list shows it ready now.
                let Some(services) = self.services.clone() else { return Task::none() };
                let chat = services.chat.clone();
                let choices = Task::perform(on(&services, async move { lattice_protocol::conversation::AgentChatService::choices(chat.as_ref()).await }), |r| Msg::Choices(r.unwrap_or_default()));
                return Task::batch([choices, self.read_browser()]);
            }
            ToolsMsg::PluginBrowse => {
                if self.ide.tools.plugin_busy {
                    return Task::none();
                }
                self.ide.tools.plugin_busy = true;
                self.ide.tools.said = None;
                return Task::perform(crate::lattice::off_thread(super::picker::pick_folder), |r| {
                    go(ToolsMsg::PluginPicked(r.unwrap_or_else(|| Err(STOPPED.to_string()))))
                });
            }
            ToolsMsg::PluginPicked(result) => match result {
                // A folder chosen in Windows' picker: the core adds it with no dialog, as it attaches one.
                Ok(Some(folder)) => {
                    return self.plugin_act(move |chat| {
                        Box::pin(async move {
                            chat.plugin_add_native(folder)
                                .await
                                .map(|view| Some(format!("{} is on: its commands and skills join yours in every chat.", view.name)))
                                .map_err(|r| r.message)
                        })
                    });
                }
                Ok(None) => self.ide.tools.plugin_busy = false,
                Err(why) => {
                    self.ide.tools.plugin_busy = false;
                    self.ide.tools.said = Some((why, true));
                }
            },
            ToolsMsg::PluginSet(name, on) => {
                return self.plugin_act(move |chat| {
                    Box::pin(async move { chat.plugin_set_on(name, on).await.map(|()| None).map_err(|r| r.message) })
                });
            }
            ToolsMsg::PluginRemove(name) => {
                return self.plugin_act(move |chat| {
                    Box::pin(async move {
                        chat.plugin_remove(name.clone())
                            .await
                            .map(|()| Some(format!("{name} is off the list; its folder is as it was.")))
                            .map_err(|r| r.message)
                    })
                });
            }
            ToolsMsg::PluginMcp(name) => {
                let workspace = self.chat_workspace();
                return self.plugin_act(move |chat| {
                    Box::pin(async move {
                        chat.plugin_mcp_copy(name, workspace).await.map(|copied| Some(copied_words(&copied))).map_err(|r| r.message)
                    })
                });
            }
            ToolsMsg::PluginActed(result) => {
                self.ide.tools.plugin_busy = false;
                match result {
                    Ok(Some(words)) => self.ide.tools.said = Some((words, false)),
                    Ok(None) => {}
                    Err(why) => self.ide.tools.said = Some((why, true)),
                }
                return self.read_tools();
            }
            ToolsMsg::ExtrasRead(read) => {
                let (commands, skills, plugins) = *read;
                self.ide.tools.commands = Some(commands);
                self.ide.tools.skills = Some(skills);
                self.ide.tools.plugins = Some(plugins);
                // A debug build's photograph of the page's end, the skills and commands (`CENTCOM_LATTICE_TOOLS=extras`),
                // or of a point part-way down (`at:0.6`; a release build never reads this).
                #[cfg(debug_assertions)]
                if let Ok(value) = std::env::var("CENTCOM_LATTICE_TOOLS") {
                    let y = if value == "extras" { Some(1.0) } else { value.strip_prefix("at:").and_then(|y| y.parse::<f32>().ok()) };
                    if let Some(y) = y {
                        return iced::widget::operation::snap_to(page_id(), iced::widget::scrollable::RelativeOffset { x: 0.0, y: y.clamp(0.0, 1.0) });
                    }
                }
            }
            ToolsMsg::AutoSet(wanted) => {
                let Some(services) = self.services.clone() else { return Task::none() };
                self.ide.tools.browser_busy = true;
                self.ide.tools.said = None;
                let chat = services.chat.clone();
                return Task::perform(
                    on(&services, async move { chat.auto_mode_set(wanted).await.map_err(|r| r.message) }),
                    |result| go(ToolsMsg::AutoActed(result.unwrap_or_else(|| Err(STOPPED.to_string())))),
                );
            }
            ToolsMsg::AutoActed(result) => {
                self.ide.tools.browser_busy = false;
                match result {
                    Ok(true) => {
                        self.ide.tools.said = Some((
                            format!("Auto mode is on: in Agent mode the agent can use your whole desktop. {} stops it at once.", lattice_core::desktop::STOP_KEYS),
                            false,
                        ))
                    }
                    Ok(false) => {}
                    Err(why) => self.ide.tools.said = Some((why, true)),
                }
                return self.read_browser();
            }
            ToolsMsg::BrowserSet(on) => {
                return self.browser_act(move |chat| {
                    Box::pin(async move {
                        chat.browser_set_on(on)
                            .await
                            .map(|()| {
                                Some(if on {
                                    "On. In Agent mode, with a model that can see, the agent can use its browser; it asks you before posting, buying, deleting or changing an account.".to_string()
                                } else {
                                    "Off. Its window is closed, and no turn offers it.".to_string()
                                })
                            })
                            .map_err(|r| r.message)
                    })
                });
            }
            ToolsMsg::BrowserShow => {
                return self.browser_act(|chat| {
                    Box::pin(async move {
                        chat.browser_show(None)
                            .await
                            .map(|()| Some("Its window is open: sign in to a site there yourself; the agent never types a password.".to_string()))
                            .map_err(|r| r.message)
                    })
                });
            }
            ToolsMsg::BrowserShowAt(url) => {
                return self.browser_act(move |chat| {
                    Box::pin(async move {
                        chat.browser_show(Some(url))
                            .await
                            .map(|()| Some("The agent's browser is open there: sign in yourself; the agent never types a password.".to_string()))
                            .map_err(|r| r.message)
                    })
                });
            }
            ToolsMsg::OpenConnections => {
                let existing = self.ide.tabs.iter().find(|t| matches!(t.kind, TabKind::Connections)).map(|t| t.id);
                let id = match existing {
                    Some(id) => id,
                    None => self.ide.push(TabKind::Connections),
                };
                self.ide.active = Some(id);
                return self.read_browser();
            }
            ToolsMsg::BrowserStop => {
                return self.browser_act(|chat| {
                    Box::pin(async move {
                        chat.browser_stop().await;
                        Ok(None)
                    })
                });
            }
            ToolsMsg::BrowserActed(result) => {
                self.ide.tools.browser_busy = false;
                match result {
                    Ok(Some(words)) => self.ide.tools.said = Some((words, false)),
                    Ok(None) => {}
                    Err(why) => self.ide.tools.said = Some((why, true)),
                }
                return self.read_browser();
            }
        }
        Task::none()
    }

    /// Read whether the agent's browser is on, and its state.
    pub(in crate::lattice) fn read_browser(&mut self) -> Task<Msg> {
        let Some(services) = self.services.clone() else { return Task::none() };
        if self.fixture {
            return Task::none();
        }
        let chat = services.chat.clone();
        let browser = Task::perform(
            on(&services, async move {
                let browser = chat.browser();
                (chat.browser_on(), browser.status(), browser.profile().display().to_string())
            }),
            |read| match read {
                Some((on, status, profile)) => go(ToolsMsg::BrowserRead(on, status, profile)),
                None => go(ToolsMsg::BrowserActed(Err(STOPPED.to_string()))),
            },
        );
        let chat = services.chat.clone();
        let auto = Task::perform(on(&services, async move { (chat.auto_mode_on(), chat.auto_mode_armed()) }), |read| {
            let (on, armed) = read.unwrap_or((false, false));
            go(ToolsMsg::AutoRead(on, armed))
        });
        let chat = services.chat.clone();
        let agents = Task::perform(on(&services, async move { chat.agents() }), |read| go(ToolsMsg::AgentsRead(read.unwrap_or_default())));
        let chat = services.chat.clone();
        let models = Task::perform(
            on(&services, async move {
                lattice_core::acp::Agent::ALL
                    .into_iter()
                    .map(|agent| {
                        let (offered, picked) = chat.agent_models(agent);
                        (agent, offered, picked)
                    })
                    .collect::<Vec<_>>()
            }),
            |read| go(ToolsMsg::AgentModelsRead(read.unwrap_or_default())),
        );
        let chat = services.chat.clone();
        let workspace = self.chat_workspace();
        let memory = Task::perform(
            on(&services, async move {
                let notes = workspace.as_deref().map(|w| chat.memory(w)).unwrap_or_default();
                (workspace, notes)
            }),
            |read| {
                let (workspace, notes) = read.unwrap_or((None, Vec::new()));
                go(ToolsMsg::MemoryRead(workspace, notes))
            },
        );
        Task::batch([browser, auto, agents, models, memory])
    }

    /// Run one action on the plugins, the section busy until it ends.
    fn plugin_act(
        &mut self,
        act: impl FnOnce(
            std::sync::Arc<lattice_core::convo::agent::AgentChat>,
        ) -> futures::future::BoxFuture<'static, Result<Option<String>, String>>,
    ) -> Task<Msg> {
        let Some(services) = self.services.clone() else {
            self.ide.tools.plugin_busy = false;
            return Task::none();
        };
        self.ide.tools.plugin_busy = true;
        self.ide.tools.said = None;
        let work = act(services.chat.clone());
        Task::perform(on(&services, work), |result| {
            go(ToolsMsg::PluginActed(result.unwrap_or_else(|| Err(STOPPED.to_string()))))
        })
    }

    /// Run one action on the browser, the section busy until it ends.
    fn browser_act(
        &mut self,
        act: impl FnOnce(
            std::sync::Arc<lattice_core::convo::agent::AgentChat>,
        ) -> futures::future::BoxFuture<'static, Result<Option<String>, String>>,
    ) -> Task<Msg> {
        let Some(services) = self.services.clone() else { return Task::none() };
        self.ide.tools.browser_busy = true;
        self.ide.tools.said = None;
        let work = act(services.chat.clone());
        Task::perform(on(&services, work), |result| {
            go(ToolsMsg::BrowserActed(result.unwrap_or_else(|| Err(STOPPED.to_string()))))
        })
    }

    /// A debug build's photograph (`CENTCOM_LATTICE_TOOLS`): the Tools view and page open, each server of the
    /// reader's own settings open on it, and each enabled one started.
    #[cfg(debug_assertions)]
    pub(in crate::lattice) fn photograph_tools(&mut self) -> Task<Msg> {
        let Some(services) = self.services.clone() else { return Task::none() };
        self.ide.side = super::Side::Tools;
        self.ide.side_open = true;
        // `CENTCOM_LATTICE_TOOLS=connections` photographs the Connections page.
        let open = if std::env::var("CENTCOM_LATTICE_TOOLS").is_ok_and(|v| v == "connections") {
            self.tools(ToolsMsg::OpenConnections)
        } else {
            self.tools(ToolsMsg::Open(None))
        };
        let chat = services.chat.clone();
        let start = Task::perform(
            on(&services, async move {
                let overview = chat.mcp_overview(None).await;
                // `CENTCOM_LATTICE_TOOLS=compact` shows every server closed.
                let compact = std::env::var("CENTCOM_LATTICE_TOOLS").is_ok_and(|v| v == "compact");
                let mut ids = Vec::new();
                for server in overview.servers {
                    if !compact {
                        ids.push(server.key.id());
                    }
                    if server.enabled && !server.off {
                        let _ = chat.mcp_start(server.key, None).await;
                    }
                }
                ids
            }),
            |ids| Msg::Ide(IdeMsg::Tools(ToolsMsg::Opened(ids.unwrap_or_default()))),
        );
        Task::batch([open, start])
    }

    /// Read the servers the reader can see now: their own, and the chat's folder's when it is trusted.
    pub(in crate::lattice) fn read_tools(&mut self) -> Task<Msg> {
        let Some(services) = self.services.clone() else { return Task::none() };
        if self.fixture {
            return Task::none();
        }
        self.ide.tools.reading = true;
        let chat = services.chat.clone();
        let workspace = self.chat_workspace();
        let servers = Task::perform(on(&services, async move { chat.mcp_overview(workspace).await }), |overview| {
            go(ToolsMsg::Read(overview))
        });
        // The skills and commands, the folder's when it is trusted.
        let chat = services.chat.clone();
        let workspace = self.chat_workspace();
        let extras = Task::perform(
            on(&services, async move {
                let plugins = chat.plugins().await.unwrap_or_default();
                (chat.commands(workspace.clone()).await, chat.skills(workspace).await, plugins)
            }),
            |read| go(ToolsMsg::ExtrasRead(Box::new(read.unwrap_or_default()))),
        );
        Task::batch([servers, extras, self.read_browser()])
    }

    /// Run one action on a server, marking it busy until it ends.
    fn mcp_act(
        &mut self,
        key: ServerKey,
        act: impl FnOnce(
            std::sync::Arc<lattice_core::convo::agent::AgentChat>,
            ServerKey,
        ) -> futures::future::BoxFuture<'static, Result<Option<String>, String>>,
    ) -> Task<Msg> {
        let Some(services) = self.services.clone() else { return Task::none() };
        let id = key.id();
        self.ide.tools.busy.insert(id.clone());
        self.ide.tools.said = None;
        let work = act(services.chat.clone(), key);
        Task::perform(on(&services, work), move |result| {
            go(ToolsMsg::Acted(id.clone(), result.unwrap_or_else(|| Err(STOPPED.to_string()))))
        })
    }

    /// Save the form: each server it declares into the reader's file, then ask the core to enable each (its dialog
    /// shows the program it would start).
    fn save_form(&mut self) -> Task<Msg> {
        let Some(services) = self.services.clone() else { return Task::none() };
        let Some(form) = &mut self.ide.tools.form else { return Task::none() };
        let named = match mcp_config::pasted(&form.json.text(), form.name.trim()) {
            Ok(named) => named,
            Err(why) => {
                form.error = Some(why);
                return Task::none();
            }
        };
        if let Some(editing) = &form.editing
            && (named.len() != 1 || named[0].0 != *editing)
        {
            form.error = Some(format!("Edit only {editing}'s entry here; add other servers with Add a server."));
            return Task::none();
        }
        form.saving = true;
        let chat = services.chat.clone();
        let workspace = self.chat_workspace();
        Task::perform(
            on(&services, async move {
                let mut enabled = Vec::new();
                for (name, entry) in named {
                    chat.mcp_put(name.clone(), entry).await.map_err(|r| format!("{name}: {}", r.message))?;
                    match chat.mcp_enable(ServerKey::user(&name), workspace.clone()).await {
                        Ok(true) => enabled.push(name),
                        Ok(false) => {}
                        Err(r) => return Err(format!("{name} was saved but not enabled: {}", r.message)),
                    }
                }
                Ok(if enabled.is_empty() {
                    "Saved. Enable it when you want the agent to use it.".to_string()
                } else {
                    format!("Saved and enabled: {}. It starts at the agent's next turn in Agent mode.", enabled.join(", "))
                })
            }),
            |r| go(ToolsMsg::Saved(r.unwrap_or_else(|| Err(STOPPED.to_string())))),
        )
    }
}

// ------------------------------------------------------------------------------------------------- views

fn small<'a>(words: impl iced::widget::text::IntoFragment<'a>, msg: Option<Msg>) -> El<'a> {
    button(iced::widget::text(words).size(12.0).font(fonts().ui))
        .padding([3.0, 9.0])
        .style(theme::secondary_button)
        .on_press_maybe(msg)
        .into()
}

fn quiet<'a>(words: impl iced::widget::text::IntoFragment<'a>, msg: Option<Msg>) -> El<'a> {
    button(iced::widget::text(words).size(12.0).font(fonts().ui))
        .padding([3.0, 8.0])
        .style(theme::ghost_button)
        .on_press_maybe(msg)
        .into()
}

/// The side bar's Tools view: each server's state, and the way to the page.
pub fn side<'a>(state: &'a State) -> El<'a> {
    let t = &state.ide.tools;
    let mut col = Column::new();
    col = col.push(super::view::section_title(
        "Tools",
        vec![
            super::view::icon("\u{27F3}", "Read again", Some(go(ToolsMsg::Refresh))),
            super::view::icon("+", "Add an MCP server", Some(go(ToolsMsg::OpenAdd))),
        ],
    ));
    let mut body = Column::new().spacing(2);
    if let Some((on, status, _)) = &t.browser {
        let (color, words) = browser_words(*on, status);
        let line = row![
            ui::dot(color),
            column![label("The agent's browser", 12.5, theme::TEXT), label(words, 11.0, theme::TEXT_FAINT)].spacing(1).width(Length::Fill),
        ]
        .spacing(8)
        .align_y(Alignment::Center);
        body = body.push(
            button(line).width(Length::Fill).padding([5.0, 10.0]).style(theme::list_row(false)).on_press(go(ToolsMsg::Open(None))),
        );
    }
    match &t.overview {
        None if t.reading => body = body.push(container(note("Reading the servers\u{2026}")).padding(12)),
        None => body = body.push(container(note("Open the Tools page to see the agent's tools and MCP servers.")).padding(12)),
        Some(o) => {
            if o.servers.is_empty() {
                body = body.push(
                    container(note("No MCP servers yet. Add one on the Tools page: the agent can then use its tools in Agent mode, asking you before each call.")).padding(12),
                );
            }
            for s in &o.servers {
                let (color, words) = status_words(s);
                let line = row![
                    ui::dot(color),
                    column![
                        label(s.key.name.clone(), 12.5, theme::TEXT),
                        label(words, 11.0, theme::TEXT_FAINT),
                    ]
                    .spacing(1)
                    .width(Length::Fill),
                ]
                .spacing(8)
                .align_y(Alignment::Center);
                body = body.push(
                    button(line)
                        .width(Length::Fill)
                        .padding([5.0, 10.0])
                        .style(theme::list_row(false))
                        .on_press(go(ToolsMsg::Open(Some(s.key.id())))),
                );
            }
        }
    }
    col = col.push(scrollable(body).height(Length::Fill).style(theme::scrollbars));
    col = col.push(
        container(
            row![
                ui::secondary("Open the Tools page", Some(go(ToolsMsg::Open(None)))),
                ui::secondary("Connections", Some(go(ToolsMsg::OpenConnections))),
            ]
            .spacing(6),
        )
        .padding(10),
    );
    col.into()
}

/// The Tools page, in the editor area.
pub fn page<'a>(state: &'a State, phase: f32) -> El<'a> {
    let t = &state.ide.tools;
    let mut col = Column::new().spacing(14).padding([18.0, 24.0]).max_width(980);
    col = col.push(
        row![
            column![
                strong("Tools", 20.0, theme::TEXT),
                label(
                    "What the agent can use. Its own tools work in every chat with a folder; an MCP server's tools join in Agent mode, in a trusted folder, and each call asks you first unless you allow that tool always. Its browser joins Agent mode too, once you switch it on.",
                    12.5,
                    theme::TEXT_DIM,
                ),
            ]
            .spacing(4)
            .width(Length::Fill),
            ui::primary("Add a server", (t.form.is_none()).then(|| go(ToolsMsg::Add))),
        ]
        .spacing(12)
        .align_y(Alignment::Start),
    );
    if let Some((words, warn)) = &t.said {
        col = col.push(ui::notice(words.clone(), if *warn { theme::CAUTION } else { theme::POSITIVE }));
    }
    if let Some(form) = &t.form {
        col = col.push(form_view(form));
    }
    col = col.push(
        container(label(
            "A tool runs only with a model that can call tools. A model on a named endpoint can. The local model (Auto and Local) is checked once, at its first Agent-mode turn: if it calls tools well it gets them, and if not it answers in plain text.",
            12.0,
            theme::TEXT_FAINT,
        ))
        .padding([2.0, 0.0]),
    );
    col = col.push(agents_card(t));
    col = col.push(memory_card(t));
    col = col.push(browser_card(t));
    col = col.push(auto_card(t));
    // The servers.
    col = col.push(row![ui::subheading("MCP servers"), space().width(Length::Fill), quiet("Read again", Some(go(ToolsMsg::Refresh)))].align_y(Alignment::Center));
    match &t.overview {
        None => col = col.push(if t.reading { ui::working(phase, "Reading the servers\u{2026}") } else { note("Not read yet.") }),
        Some(o) => {
            col = col.push(label(format!("Your MCP settings: {}", o.user_file), 11.5, theme::TEXT_FAINT));
            if o.servers.is_empty() {
                col = col.push(note("None yet. Add a server with the entry its instructions give (\"command\", \"args\", \"env\"), or paste a whole \"mcpServers\" block. A trusted folder's .lattice/mcp.json, .mcp.json or .cursor/mcp.json shows here too."));
            }
            for s in &o.servers {
                col = col.push(server_card(state, s));
            }
            for p in &o.problems {
                let what = if p.name.is_empty() { p.file.clone() } else { format!("{} ({})", p.name, p.file) };
                col = col.push(ui::notice(format!("{what}: {}", p.sentence), theme::CAUTION));
            }
        }
    }
    // Skills and commands (the plugins feature).
    let yours = |dir: std::path::PathBuf| dir.display().to_string();
    let (skills_dir, commands_dir) = match &state.services {
        Some(services) => (
            yours(lattice_core::skills::user_skills_dir(&services.state)),
            yours(lattice_core::commands::user_commands_dir(&services.state)),
        ),
        None => (String::new(), String::new()),
    };
    col = col.push(
        row![
            ui::subheading("Plugins"),
            space().width(Length::Fill),
            ui::secondary(if t.plugin_busy { "Working\u{2026}" } else { "Add a plugin\u{2026}" }, (!t.plugin_busy).then(|| go(ToolsMsg::PluginBrowse))),
        ]
        .align_y(Alignment::Center),
    );
    col = col.push(label(PLUGINS_ABOUT, 12.0, theme::TEXT_DIM));
    col = col.push(match &t.plugins {
        None => note("Not read yet."),
        Some(plugins) if plugins.is_empty() => note("None yet. Add one from its folder: the folder that holds its .claude-plugin."),
        Some(plugins) => {
            let mut list = Column::new().spacing(8);
            for plugin in plugins {
                list = list.push(plugin_card(plugin, t.plugin_busy));
            }
            list.into()
        }
    });
    col = col.push(ui::subheading("Skills"));
    col = col.push(label(SKILLS_ABOUT, 12.0, theme::TEXT_DIM));
    col = col.push(label(
        format!("Yours: a folder each in {skills_dir}. A trusted folder's: .lattice/skills or .claude/skills."),
        11.5,
        theme::TEXT_FAINT,
    ));
    col = col.push(match &t.skills {
        None => note("Not read yet."),
        Some(found) if found.skills.is_empty() && found.notices.is_empty() => note("None yet."),
        Some(found) => extras(
            found.skills.iter().map(|skill| (skill.name.clone(), skill.description.clone(), skill.path.clone())).collect(),
            &found.notices,
        ),
    });
    col = col.push(ui::subheading("Commands"));
    col = col.push(label(COMMANDS_ABOUT, 12.0, theme::TEXT_DIM));
    col = col.push(label(
        format!("Yours: Markdown files in {commands_dir}. A trusted folder's: .lattice/commands, .claude/commands or .cursor/commands."),
        11.5,
        theme::TEXT_FAINT,
    ));
    col = col.push(match &t.commands {
        None => note("Not read yet."),
        Some(found) if found.commands.is_empty() && found.notices.is_empty() => note("None yet."),
        Some(found) => extras(
            found
                .commands
                .iter()
                .map(|command| (format!("/{}", command.name), command.description.clone(), command.path.clone()))
                .collect(),
            &found.notices,
        ),
    });
    // The agent's own tools.
    col = col.push(ui::subheading("Built in"));
    let mut built = Column::new().spacing(6);
    for (names, what, modes) in BUILT_IN {
        built = built.push(
            row![
                container(mono(names, 12.0, theme::GOLD)).width(Length::FillPortion(2)),
                container(label(what, 12.0, theme::TEXT_DIM)).width(Length::FillPortion(3)),
                container(label(modes, 11.5, theme::TEXT_FAINT)).width(Length::FillPortion(1)),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        );
    }
    col = col.push(container(built).padding([10.0, 12.0]).width(Length::Fill).style(theme::well));
    scrollable(container(col).center_x(Length::Fill)).id(page_id()).height(Length::Fill).style(theme::scrollbars).into()
}

/// The Tools page's scrollable.
pub fn page_id() -> iced::widget::Id {
    iced::widget::Id::new("ide-tools-page")
}

/// One plugin: its name and version, what it is and brings, where it is, and its switches.
fn plugin_card<'a>(plugin: &'a lattice_core::plugins::PluginView, busy: bool) -> El<'a> {
    let mut head = Row::new().spacing(10).align_y(Alignment::Center);
    head = head.push(ui::dot(if plugin.on && plugin.problem.is_none() { theme::POSITIVE } else { theme::TEXT_FAINT }));
    let mut title = Row::new().spacing(8).align_y(Alignment::Center).push(strong(plugin.name.as_str(), 14.0, theme::TEXT));
    if !plugin.version.is_empty() {
        title = title.push(mono(format!("v{}", plugin.version), 11.0, theme::TEXT_FAINT));
    }
    if !plugin.author.is_empty() {
        title = title.push(label(format!("by {}", plugin.author), 11.5, theme::TEXT_FAINT));
    }
    head = head.push(column![title, label(if plugin.on { "On" } else { "Off" }, 11.5, theme::TEXT_FAINT)].spacing(1).width(Length::Fill));
    let act = |on: bool, msg: ToolsMsg| (on && !busy).then(|| go(msg));
    head = head.push(ui::secondary(
        if plugin.on { "Switch off" } else { "Switch on" },
        act(true, ToolsMsg::PluginSet(plugin.name.clone(), !plugin.on)),
    ));
    if !plugin.mcp_servers.is_empty() {
        head = head.push(ui::secondary("Copy its MCP servers", act(true, ToolsMsg::PluginMcp(plugin.name.clone()))));
    }
    head = head.push(ui::secondary("Take off the list", act(true, ToolsMsg::PluginRemove(plugin.name.clone()))));
    let mut col = Column::new().spacing(6).push(head);
    if !plugin.description.is_empty() {
        col = col.push(label(ui::cut(&plugin.description, 400), 12.0, theme::TEXT_DIM));
    }
    col = col.push(label(plugin_parts(plugin), 11.5, theme::TEXT_DIM));
    col = col.push(mono(ui::cut(&plugin.path, 120), 11.0, theme::TEXT_FAINT));
    if let Some(problem) = &plugin.problem {
        col = col.push(ui::notice(problem.clone(), theme::CAUTION));
    }
    container(col).padding([10.0, 14.0]).width(Length::Fill).style(theme::card).into()
}

/// A list of skills or commands: each one's name, what it is for and where it is, then what was left out.
fn extras<'a>(rows: Vec<(String, String, String)>, notices: &'a [String]) -> El<'a> {
    let mut list = Column::new().spacing(6);
    for (name, what, path) in rows {
        list = list.push(
            row![
                container(mono(ui::cut(&name, 40), 12.0, theme::GOLD)).width(Length::FillPortion(2)),
                container(label(ui::cut(&what, 160), 12.0, theme::TEXT_DIM)).width(Length::FillPortion(3)),
                container(label(ui::cut(&path, 60), 11.0, theme::TEXT_FAINT)).width(Length::FillPortion(2)),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        );
    }
    for notice in notices {
        list = list.push(label(notice.as_str(), 11.5, theme::CAUTION));
    }
    container(list).padding([10.0, 12.0]).width(Length::Fill).style(theme::well).into()
}

/// What the auto mode card says.
pub const AUTO_ABOUT: &str = "With auto mode on, the agent in Agent mode can use your whole desktop as you would: it sees your screen and uses your mouse and keyboard in any program. It posts, sends, edits and deletes without asking; a purchase, a payment, and a sign-in, security or account change still ask you. It never acts on Alelyon itself, a password manager or Windows' sign-in prompts, and never types a password. Pictures of your screen go to the model you choose.";

/// Auto mode's dot colour and words.
pub fn auto_words(on: bool, armed: bool) -> (iced::Color, String) {
    match (on, armed) {
        (false, _) => (theme::TEXT_FAINT, "Off: the agent uses only its own browser".to_string()),
        (true, true) => (theme::GOLD, format!("On \u{00B7} {} stops the agent", lattice_core::desktop::STOP_KEYS)),
        (true, false) => (theme::CAUTION, "On, but its stop keys are not armed: switch it off and on again".to_string()),
    }
}

/// What the agent remembers about the open folder: each note, and Forget.
fn memory_card<'a>(t: &'a Tools) -> El<'a> {
    let mut col = Column::new()
        .spacing(8)
        .push(strong("What the agent remembers", 14.0, theme::TEXT))
        .push(label(
            "Notes Lattice's agent kept about this folder with remember. Every new chat in the folder starts with them; forget any that is wrong, stale or should not be kept.",
            12.0,
            theme::TEXT_DIM,
        ));
    match &t.memory {
        None => col = col.push(note("Not read yet.")),
        Some((None, _)) => col = col.push(note("Open a folder or a chat in one to see what the agent remembers about it.")),
        Some((Some(_), notes)) if notes.is_empty() => col = col.push(note("Nothing yet: the agent keeps a note when something will matter again.")),
        Some((Some(workspace), notes)) => {
            for kept in notes {
                col = col.push(
                    Row::new()
                        .spacing(10)
                        .align_y(Alignment::Center)
                        .push(container(label(kept.text.as_str(), 12.5, theme::TEXT)).width(Length::Fill))
                        .push(small("Forget", Some(go(ToolsMsg::Forget(workspace.clone(), kept.id.clone()))))),
                );
            }
        }
    }
    container(col).padding([12.0, 14.0]).width(Length::Fill).style(theme::card).into()
}

/// What the agents' card says under its title.
const AGENTS_ABOUT: &str = "Claude Code and Codex answer in Lattice's chat on your own accounts, with their own tools, in the chat's folder: choose one in the composer's model list. They run off this machine, so your messages and the files they read go to Anthropic or OpenAI. Whatever they ask to do is asked of you first, once each time.";

/// How each agent signs in.
fn agent_sign_in(agent: lattice_core::acp::Agent) -> &'static str {
    match agent {
        lattice_core::acp::Agent::ClaudeCode => "Uses your claude login (run `claude /login` once in a terminal).",
        lattice_core::acp::Agent::Codex => "Uses your ChatGPT plan (run `codex login` once in a terminal).",
    }
}

/// The labs' agents: each installed or not, Install, and how it signs in.
fn agents_card<'a>(t: &'a Tools) -> El<'a> {
    let mut col = Column::new()
        .spacing(8)
        .push(strong("Claude Code and Codex, on your subscriptions", 14.0, theme::TEXT))
        .push(label(AGENTS_ABOUT, 12.0, theme::TEXT_DIM));
    match &t.agents {
        None => col = col.push(note("Not read yet.")),
        Some(agents) => {
            for (agent, installed, node) in agents {
                let mut line = Row::new().spacing(10).align_y(Alignment::Center);
                line = line.push(ui::dot(if *installed { theme::POSITIVE } else { theme::TEXT_FAINT }));
                line = line.push(
                    column![
                        strong(agent.label(), 13.0, theme::TEXT),
                        label(
                            if *installed {
                                agent_sign_in(*agent).to_string()
                            } else if !node && *agent == lattice_core::acp::Agent::ClaudeCode {
                                "Needs Node.js, which was not found.".to_string()
                            } else {
                                format!("Not installed: Install downloads {} (npm).", agent.package())
                            },
                            11.5,
                            theme::TEXT_DIM,
                        ),
                    ]
                    .spacing(2),
                );
                line = line.push(space().width(Length::Fill));
                if t.agent_busy == Some(*agent) {
                    line = line.push(label("Installing\u{2026}", 12.0, theme::TEXT_FAINT));
                } else if !installed {
                    line = line.push(small("Install", t.agent_busy.is_none().then(|| go(ToolsMsg::AgentInstall(*agent)))));
                } else {
                    line = line.push(label("Installed", 12.0, theme::POSITIVE));
                }
                col = col.push(line);
                if *installed {
                    col = col.push(agent_model(t, *agent));
                }
            }
        }
    }
    container(col).padding([12.0, 14.0]).width(Length::Fill).style(theme::card).into()
}

/// An installed agent's model: the reader's pick or its default, Choose
/// model…, and when choosing, each model it offers.
fn agent_model<'a>(t: &'a Tools, agent: lattice_core::acp::Agent) -> El<'a> {
    let (offered, picked) = t
        .agent_models
        .iter()
        .find(|(a, _, _)| *a == agent)
        .map(|(_, offered, picked)| (Some(offered), picked.as_deref()))
        .unwrap_or((None, None));
    let name = |id: &str| {
        offered
            .and_then(|o| o.models.iter().find(|m| m.id == id))
            .map(|m| m.name.clone())
            .unwrap_or_else(|| id.to_string())
    };
    let now = match picked {
        Some(id) => format!("Model: {} (new chats)", name(id)),
        None => "Model: its own default".to_string(),
    };
    let mut head = Row::new().spacing(10).align_y(Alignment::Center).push(space().width(18));
    head = head.push(label(now, 12.0, theme::TEXT_DIM)).push(space().width(Length::Fill));
    if t.checking == Some(agent) {
        head = head.push(label("Checking its models\u{2026}", 12.0, theme::TEXT_FAINT));
    } else if t.choosing == Some(agent) {
        head = head
            .push(small("Check again", t.checking.is_none().then(|| go(ToolsMsg::CheckModels(agent)))))
            .push(small("Done", Some(go(ToolsMsg::CloseModels))));
    } else {
        head = head.push(small("Choose model\u{2026}", t.checking.is_none().then(|| go(ToolsMsg::ChooseModel(agent)))));
    }
    let mut col = Column::new().spacing(6).push(head);
    if t.choosing == Some(agent)
        && let Some(offered) = offered
        && !offered.models.is_empty()
    {
        let mut list = Column::new().spacing(4);
        let default = Row::new()
            .spacing(10)
            .align_y(Alignment::Center)
            .push(container(label("Its own default", 12.0, theme::TEXT)).width(Length::Fill))
            .push(if picked.is_none() {
                Element::from(label("in use", 11.5, theme::POSITIVE))
            } else {
                small("Use", Some(go(ToolsMsg::PickModel(agent, None))))
            });
        list = list.push(default);
        for model in &offered.models {
            let mut words = Column::new().spacing(1).push(label(model.name.as_str(), 12.0, theme::TEXT));
            if !model.description.is_empty() {
                words = words.push(label(model.description.as_str(), 11.0, theme::TEXT_FAINT));
            }
            list = list.push(
                Row::new()
                    .spacing(10)
                    .align_y(Alignment::Center)
                    .push(container(words).width(Length::Fill))
                    .push(if picked == Some(model.id.as_str()) {
                        Element::from(label("in use", 11.5, theme::POSITIVE))
                    } else {
                        small("Use", Some(go(ToolsMsg::PickModel(agent, Some(model.id.clone())))))
                    }),
            );
        }
        col = col.push(container(list).padding([8.0, 10.0]).width(Length::Fill).style(theme::well));
        col = col.push(note("A chat already open with it keeps the model it began with."));
    }
    col.into()
}

/// Auto mode: off or on, and its stop keys.
fn auto_card<'a>(t: &'a Tools) -> El<'a> {
    let (on, armed) = t.auto.unwrap_or((false, false));
    let (color, words) = auto_words(on, armed);
    let mut head = Row::new().spacing(10).align_y(Alignment::Center);
    head = head.push(ui::dot(color));
    head = head.push(column![strong("Auto mode: the whole desktop", 14.0, theme::TEXT), label(words, 12.0, theme::TEXT_DIM)].spacing(2));
    head = head.push(space().width(Length::Fill));
    if t.browser_busy {
        head = head.push(label("Working\u{2026}", 12.0, theme::TEXT_FAINT));
    } else if on {
        head = head.push(small("Switch off", Some(go(ToolsMsg::AutoSet(false)))));
    } else {
        head = head.push(small("Switch on\u{2026}", Some(go(ToolsMsg::AutoSet(true)))));
    }
    let col = Column::new().spacing(8).push(head).push(label(AUTO_ABOUT, 12.0, theme::TEXT_DIM));
    container(col).padding([12.0, 14.0]).width(Length::Fill).style(theme::card).into()
}

/// The agent's browser: on or off, its state, Show and Stop.
fn browser_card<'a>(t: &'a Tools) -> El<'a> {
    let (on, status, profile) = match &t.browser {
        Some((on, status, profile)) => (*on, status.clone(), profile.as_str()),
        None => (false, BrowserStatus::Stopped, ""),
    };
    let (color, words) = browser_words(on, &status);
    let mut head = Row::new().spacing(10).align_y(Alignment::Center);
    head = head.push(ui::dot(color));
    head = head.push(column![strong("The agent's browser", 14.0, theme::TEXT), label(words, 12.0, theme::TEXT_DIM)].spacing(2));
    head = head.push(space().width(Length::Fill));
    if t.browser_busy {
        head = head.push(label("Working\u{2026}", 12.0, theme::TEXT_FAINT));
    } else {
        head = head.push(small(if on { "Switch off" } else { "Switch on" }, Some(go(ToolsMsg::BrowserSet(!on)))));
        head = head.push(small("Show", Some(go(ToolsMsg::BrowserShow))));
        head = head.push(small("Connections\u{2026}", Some(go(ToolsMsg::OpenConnections))));
        if matches!(status, BrowserStatus::Running { .. } | BrowserStatus::Starting) {
            head = head.push(small("Stop", Some(go(ToolsMsg::BrowserStop))));
        }
    }
    let mut col = Column::new().spacing(8).push(head);
    col = col.push(label(BROWSER_ABOUT, 12.0, theme::TEXT_DIM));
    if let BrowserStatus::Failed(why) = &status {
        col = col.push(ui::notice(why.clone(), theme::DANGER));
    }
    col = col.push(label(BROWSER_PRIVACY, 11.5, theme::TEXT_FAINT));
    if !profile.is_empty() {
        col = col.push(label(format!("Its profile: {profile}"), 11.0, theme::TEXT_FAINT));
    }
    container(col).padding([12.0, 14.0]).width(Length::Fill).style(theme::card).into()
}

fn form_view<'a>(form: &'a Form) -> El<'a> {
    let mut col = Column::new().spacing(8);
    col = col.push(strong(if form.editing.is_some() { "Edit a server" } else { "Add a server" }, 14.0, theme::GOLD));
    let name: El<'a> = if let Some(editing) = &form.editing {
        label(format!("Name: {editing}"), 12.5, theme::TEXT).into()
    } else {
        text_input("Its name, e.g. files (not needed for a whole mcpServers block)", &form.name)
            .on_input(|t| go(ToolsMsg::Name(t)))
            .padding([6.0, 8.0])
            .size(12.5)
            .style(ui::input_style)
            .into()
    };
    col = col.push(name);
    col = col.push(
        text_editor(&form.json)
            .on_action(|a| go(ToolsMsg::Json(a)))
            .font(fonts().mono)
            .size(12.5)
            .height(Length::Fixed(190.0))
            .padding(8)
            .style(theme::editor),
    );
    col = col.push(label(
        "Its entry, as its instructions give it: \"command\" (a program on this PC, or a bare name such as npx or uvx found on your PATH), \"args\", \"env\" (variable names and values; they stay in your MCP settings), and \"cwd\". A value shown as <kept> keeps what is saved. Servers that are a web address are not supported: they send data off this machine.",
        11.5,
        theme::TEXT_FAINT,
    ));
    if let Some(why) = &form.error {
        col = col.push(ui::notice(why.clone(), theme::CAUTION));
    }
    col = col.push(
        row![
            ui::primary(if form.saving { "Saving\u{2026}" } else { "Save and enable\u{2026}" }, (!form.saving).then(|| go(ToolsMsg::Save))),
            ui::secondary("Cancel", (!form.saving).then(|| go(ToolsMsg::Cancel))),
        ]
        .spacing(8),
    );
    container(col).padding(14).width(Length::Fill).style(theme::notice(theme::GOLD)).into()
}

fn server_card<'a>(state: &'a State, s: &'a ServerView) -> El<'a> {
    let t = &state.ide.tools;
    let id = s.key.id();
    let open = t.expanded.contains(&id);
    let busy = t.busy.contains(&id);
    let (color, words) = status_words(s);
    let mut head = Row::new().spacing(10).align_y(Alignment::Center);
    head = head.push(ui::dot(color));
    head = head.push(
        button(
            column![
                row![strong(s.key.name.clone(), 14.0, theme::TEXT), label(source_words(s), 11.5, theme::TEXT_FAINT)].spacing(8).align_y(Alignment::Center),
                label(words, 12.0, theme::TEXT_DIM),
            ]
            .spacing(2),
        )
        .padding(0)
        .style(theme::ghost_button)
        .on_press(go(ToolsMsg::Expand(id.clone()))),
    );
    head = head.push(space().width(Length::Fill));
    let key = || s.key.clone();
    if busy {
        head = head.push(label("Working\u{2026}", 12.0, theme::TEXT_FAINT));
    } else if !s.enabled {
        head = head.push(small("Enable\u{2026}", Some(go(ToolsMsg::Enable(key())))));
    } else if s.off {
        head = head.push(small("Switch on", Some(go(ToolsMsg::Switch(key(), WHOLE_SERVER.to_string(), true)))));
    } else {
        match &s.status {
            ServerStatus::Running => {
                head = head.push(small("Stop", Some(go(ToolsMsg::Stop(key())))));
                head = head.push(small("Restart", Some(go(ToolsMsg::Restart(key())))));
            }
            ServerStatus::Starting => {}
            ServerStatus::Stopped | ServerStatus::Failed(_) => {
                head = head.push(small("Start", Some(go(ToolsMsg::Start(key())))));
            }
        }
    }
    head = head.push(quiet(if open { "\u{25BE}" } else { "\u{25B8}" }, Some(go(ToolsMsg::Expand(id.clone())))));
    let mut col = Column::new().spacing(8).push(head);
    col = col.push(container(mono(ui::cut(&s.command_line, 240), 11.5, theme::TEXT_DIM)).padding([6.0, 8.0]).width(Length::Fill).style(theme::well));
    if let ServerStatus::Failed(why) = &s.status {
        col = col.push(ui::notice(why.clone(), theme::DANGER));
    }
    if open {
        let mut facts = Vec::new();
        if let Some(server) = &s.server {
            facts.push(server.clone());
        }
        if !s.env_names.is_empty() {
            facts.push(format!("Variables: {}", s.env_names.join(", ")));
        }
        if let Some(cwd) = &s.cwd {
            facts.push(format!("Runs in {cwd}"));
        }
        for fact in facts {
            col = col.push(label(fact, 11.5, theme::TEXT_FAINT));
        }
        if let Some(words) = &s.instructions {
            col = col.push(label(format!("What it says about itself (not checked): {}", ui::cut(words, 400)), 11.5, theme::TEXT_FAINT));
        }
        if s.tools.is_empty() {
            let why = if s.status == ServerStatus::Running { "It lists no tools." } else { "Its tools show once it runs." };
            col = col.push(note(why));
        } else {
            let mut tools = Column::new().spacing(4);
            for tool in &s.tools {
                tools = tools.push(tool_row(s, tool, busy));
            }
            col = col.push(container(tools).padding([8.0, 10.0]).width(Length::Fill).style(theme::well));
        }
        for p in &s.problems {
            col = col.push(label(p.clone(), 11.5, theme::CAUTION));
        }
        if !s.log.is_empty() {
            let tail: Vec<&str> = s.log.iter().rev().take(14).rev().map(String::as_str).collect();
            col = col.push(label("Its log (last lines; never sent to a model)", 11.5, theme::TEXT_FAINT));
            col = col.push(container(mono(tail.join("\n"), 11.0, theme::TEXT_DIM)).padding([6.0, 8.0]).width(Length::Fill).style(theme::well));
        }
        let mut actions = Row::new().spacing(6);
        if s.enabled {
            actions = actions.push(quiet("Disable", (!busy).then(|| go(ToolsMsg::Disable(key())))));
        }
        if s.enabled && !s.off {
            actions = actions.push(quiet("Switch off", (!busy).then(|| go(ToolsMsg::Switch(key(), WHOLE_SERVER.to_string(), false)))));
        }
        if s.key.scope == Scope::User {
            actions = actions.push(quiet("Edit", (!busy).then(|| go(ToolsMsg::Edit(s.key.name.clone())))));
            actions = actions.push(quiet("Remove", (!busy).then(|| go(ToolsMsg::Remove(s.key.name.clone())))));
        }
        col = col.push(actions);
    }
    container(col).padding([12.0, 14.0]).width(Length::Fill).style(theme::card).into()
}

fn tool_row<'a>(s: &'a ServerView, tool: &'a ToolView, busy: bool) -> El<'a> {
    let key = s.key.clone();
    let mut hints = Vec::new();
    if tool.read_only == Some(true) {
        hints.push("says it only reads");
    }
    if tool.destructive == Some(true) {
        hints.push("says it may delete or overwrite");
    }
    if tool.open_world == Some(true) {
        hints.push("says it reaches outside this PC");
    }
    let first = tool.description.lines().next().unwrap_or("").to_string();
    let mut words = column![
        row![mono(tool.name.clone(), 12.0, if tool.on { theme::TEXT } else { theme::TEXT_FAINT }), label(ui::cut(&first, 120), 11.5, theme::TEXT_DIM)].spacing(8),
    ]
    .spacing(1)
    .width(Length::Fill);
    if !hints.is_empty() {
        words = words.push(label(format!("The server {}", hints.join(", ")), 10.5, theme::TEXT_FAINT));
    }
    let allow: El<'a> = if tool.allowed {
        row![label("Runs without asking", 11.0, theme::GOLD), quiet("Ask each time", (!busy).then(|| go(ToolsMsg::Disallow(key.clone(), tool.name.clone()))))]
            .spacing(4)
            .align_y(Alignment::Center)
            .into()
    } else {
        quiet("Allow always\u{2026}", (!busy && tool.on).then(|| go(ToolsMsg::Allow(key.clone(), tool.name.clone()))))
    };
    let switch = quiet(if tool.on { "On" } else { "Off" }, (!busy).then(|| go(ToolsMsg::Switch(key.clone(), tool.name.clone(), !tool.on))));
    row![words, allow, switch].spacing(8).align_y(Alignment::Center).into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use lattice_core::mcp::config::ServerKey;

    fn view(enabled: bool, off: bool, status: ServerStatus) -> ServerView {
        ServerView {
            key: ServerKey::user("files"),
            file: "your MCP settings".into(),
            command_line: "npx -y files".into(),
            env_names: vec![],
            cwd: None,
            off,
            enabled,
            status,
            server: None,
            instructions: None,
            tools: vec![],
            problems: vec![],
            log: vec![],
        }
    }

    #[test]
    fn a_servers_words_say_whether_it_is_enabled_on_and_running() {
        assert_eq!(status_words(&view(false, false, ServerStatus::Running)).1, "Not enabled");
        assert_eq!(status_words(&view(true, true, ServerStatus::Running)).1, "Switched off");
        assert_eq!(status_words(&view(true, false, ServerStatus::Stopped)).1, "Starts at the agent's next turn");
        assert_eq!(status_words(&view(true, false, ServerStatus::Running)).1, "Running \u{00B7} 0 of 0 tools on");
        assert_eq!(status_words(&view(true, false, ServerStatus::Failed("x".into()))).1, "Stopped with an error");
        assert_eq!(source_words(&view(true, false, ServerStatus::Stopped)), "Your MCP settings");
    }

    #[test]
    fn auto_modes_words_say_whether_it_is_off_on_or_on_without_its_stop_keys() {
        assert_eq!(auto_words(false, false).1, "Off: the agent uses only its own browser");
        assert_eq!(auto_words(true, true).1, "On \u{00B7} Ctrl+Alt+End stops the agent");
        assert!(auto_words(true, false).1.contains("not armed"));
    }

    #[test]
    fn the_browsers_words_say_whether_it_is_on_starting_open_or_stopped_with_an_error() {
        assert_eq!(browser_words(false, &BrowserStatus::Stopped).1, "Off");
        assert_eq!(browser_words(true, &BrowserStatus::Stopped).1, "On \u{00B7} starts at the agent's first browser action");
        assert_eq!(browser_words(true, &BrowserStatus::Starting).1, "Starting\u{2026}");
        let open = BrowserStatus::Running { url: "https://x.com/home".into(), title: "Home / X".into() };
        assert_eq!(browser_words(true, &open).1, "Open \u{00B7} Home / X");
        let blank = BrowserStatus::Running { url: "about:blank".into(), title: String::new() };
        assert_eq!(browser_words(true, &blank).1, "Open \u{00B7} about:blank");
        assert_eq!(browser_words(true, &BrowserStatus::Failed("x".into())).1, "Stopped with an error");
    }

    /// The new server's template is an entry the core would refuse only for its empty command: it is shown, not
    /// saved as it is.
    #[test]
    fn the_template_parses_and_needs_a_command() {
        let refused = mcp_config::pasted(TEMPLATE, "x").unwrap_err();
        assert!(refused.contains("no command"), "{refused}");
        let filled = TEMPLATE.replace("\"command\": \"\"", "\"command\": \"uvx\"");
        assert_eq!(mcp_config::pasted(&filled, "x").unwrap()[0].0, "x");
    }

    /// A plugin's line counts its commands and skills, names its MCP servers, and says its agents are not used and
    /// its hooks never run.
    #[test]
    fn a_plugins_line_counts_what_it_brings_and_names_what_is_not_used() {
        use lattice_core::plugins::PluginView;
        let view = PluginView {
            name: "alelyon".into(),
            commands: vec!["alelyon:alelyon".into()],
            skills: vec!["alelyon:alelyon".into(), "alelyon:cne-verifier".into()],
            ..PluginView::default()
        };
        assert_eq!(plugin_parts(&view), "1 command \u{00B7} 2 skills");
        let full = PluginView { agents: vec!["reviewer".into()], hooks: true, mcp_servers: vec!["evidence".into()], ..view };
        assert_eq!(
            plugin_parts(&full),
            "1 command \u{00B7} 2 skills \u{00B7} MCP: evidence \u{00B7} 1 agent (not used) \u{00B7} hooks (never run)"
        );
    }

    /// A pick given up in Windows' picker, and an action the core refused, leave the section ready, the refusal said.
    #[test]
    fn a_cancelled_pick_and_a_refused_action_leave_the_plugins_ready() {
        let mut state = State::new();
        state.ide.tools.plugin_busy = true;
        let _ = state.tools(ToolsMsg::PluginPicked(Ok(None)));
        assert!(!state.ide.tools.plugin_busy && state.ide.tools.said.is_none());
        state.ide.tools.plugin_busy = true;
        let _ = state.tools(ToolsMsg::PluginActed(Err("That plugin is on the list already.".into())));
        assert!(!state.ide.tools.plugin_busy);
        assert_eq!(state.ide.tools.said, Some(("That plugin is on the list already.".to_string(), true)));
    }

    /// Copying a plugin's MCP servers says what was enabled, what was not, and what the reader's settings kept.
    #[test]
    fn copying_a_plugins_servers_says_what_happened_to_each() {
        use lattice_core::plugins::Copied;
        let copied = Copied { enabled: vec!["evidence".into()], not_enabled: vec!["search".into()], kept: vec!["git".into()] };
        assert_eq!(
            copied_words(&copied),
            "Copied to your MCP settings and enabled: evidence. Copied, not enabled: search. Your settings already hold git, left as you have them."
        );
    }
}
