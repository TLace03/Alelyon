//! What the Lattice page draws: a slim bar with its tabs, then the tab. The Chat and IDE fills the window to its edges
//! (`ide/view.rs`); the other tabs scroll inside their margins. It reads only what `update` built.

use iced::widget::{
    Column, Row, button, center, column, container, markdown, opaque, pick_list, row, scrollable, space, stack, text,
    text_input, tooltip,
};
use iced::{Alignment, Element, Length, padding};

use crate::theme::{self, fonts};
use lattice_protocol::Locality;
use lattice_protocol::chat::RuntimeState;
use lattice_protocol::conversation::ReviewResult;

use super::system::Level;
use super::{Msg, State, Tab};
use crate::ui::{self, chip, fact, label, mono, note, strong};

type El<'a> = Element<'a, Msg>;

const PURPOSE: &str = "Chat with a model about your code, with tools: it reads the folder you attach, proposes changes \
                       you review before anything is written, and runs commands only after you confirm each one. \
                       Local and Auto never leave this computer. Below: agent runs with their traces, and the models.";

/// The widest the tabs other than the Chat and IDE run, as the window's own pages do: lines do not run edge to edge
/// on a large screen.
pub const MAX_WIDTH: f32 = 1640.0;

/// The page, for a window `width` wide (the rail included): the tabs lay their parts in columns from a laptop's width.
pub fn view(state: &State, phase: f32, width: f32) -> El<'_> {
    let mut page = Column::new().width(Length::Fill).height(Length::Fill);
    page = page.push(top_bar(state, phase));
    if let Some(problem) = &state.problem {
        page = page.push(
            container(
                row![label(problem.as_str(), 12.5, theme::TEXT), space().width(Length::Fill), ui::secondary("Dismiss", Some(Msg::Dismiss))]
                    .spacing(10)
                    .align_y(Alignment::Center),
            )
            .padding([6.0, 14.0])
            .width(Length::Fill)
            .style(theme::notice(theme::CAUTION)),
        );
    }
    let body: El<'_> = if let Some(why) = &state.failed {
        container(ui::notice(format!("Lattice could not start: {why}"), theme::DANGER)).padding([16.0, 24.0]).into()
    } else if state.starting {
        container(ui::working(phase, "Starting Lattice: the chat core, the local model server's manager and the runs…"))
            .padding([16.0, 24.0])
            .into()
    } else {
        tab_body(state, state.tab, phase, width)
    };
    page = page.push(container(body).height(Length::Fill));
    let page: El<'_> = page.into();
    match &state.dialog {
        Some(dialog) => stack![page, opaque(center(dialog_card(dialog)).style(theme::scrim))].into(),
        None => page,
    }
}

/// One tab in a window of its own (`windows`): the tab as the page shows it, without the page's bar.
pub fn view_tab(state: &State, tab: Tab, phase: f32, width: f32) -> El<'_> {
    let body: El<'_> = if let Some(why) = &state.failed {
        container(ui::notice(format!("Lattice could not start: {why}"), theme::DANGER)).padding([16.0, 24.0]).into()
    } else if state.starting {
        container(ui::working(phase, "Starting Lattice: the chat core, the local model server's manager and the runs…"))
            .padding([16.0, 24.0])
            .into()
    } else {
        tab_body(state, tab, phase, width)
    };
    container(body).width(Length::Fill).height(Length::Fill).into()
}

/// A tab's body: the Chat and IDE to the window's edges, any other within the page's margins.
fn tab_body(state: &State, tab: Tab, phase: f32, width: f32) -> El<'_> {
    let content: El<'_> = match tab {
        Tab::Chat => super::ide::view::view(state, phase),
        Tab::Runs => runs_tab(state, phase),
        Tab::Models => models_tab(state, width),
        Tab::Training => training_tab(state, phase, width),
        Tab::Morphometry => super::morphometry::view(&state.morphometry, phase).map(Msg::Morpho),
        Tab::Foundry => super::foundry::view(&state.foundry, phase, width).map(Msg::Foundry),
        Tab::System => system_tab(state, phase),
    };
    if tab == Tab::Chat {
        content
    } else {
        container(container(content).max_width(MAX_WIDTH).width(Length::Fill).height(Length::Fill))
            .padding([16.0, 24.0])
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
}

/// The page's bar: its name (what it does on hover), its tabs, and, outside the IDE (which has a status bar), its
/// chips.
fn top_bar(state: &State, phase: f32) -> El<'_> {
    let mut line = Row::new().spacing(6).align_y(Alignment::Center);
    let title = row![label("\u{25C7}", 15.0, theme::GOLD), strong("Lattice", 14.5, theme::TEXT)].spacing(6).align_y(Alignment::Center);
    line = line.push(tooltip(
        container(title).padding([0.0, 6.0]),
        container(label(PURPOSE, 12.0, theme::TEXT)).max_width(460).padding([8.0, 10.0]).style(theme::card),
        tooltip::Position::Bottom,
    ));
    line = line.push(space().width(8));
    for tab in Tab::ALL {
        use super::TabWindow;
        // a tab in a window of its own is marked, and a press on it brings that window forward (`windows`)
        let out = state.out.contains(&tab);
        let on = state.tab == tab && !out;
        let color = if on {
            theme::GOLD
        } else if out {
            theme::TEXT_FAINT
        } else {
            theme::TEXT_DIM
        };
        let mut words = Row::new().spacing(5).align_y(Alignment::Center).push(label(tab.title(), 12.5, color));
        if out {
            words = words.push(label(crate::windows::OUT_MARK, 10.5, theme::GOLD));
        }
        // the pointer over the tab, followed, so a press on it can be dragged off the bar to a window of its own
        let face = iced::widget::mouse_area(container(words).padding([4.0, 10.0]))
            .on_enter(Msg::Window(TabWindow::Hover(tab, true)))
            .on_exit(Msg::Window(TabWindow::Hover(tab, false)))
            .on_middle_press(Msg::Window(TabWindow::PopOut(tab)));
        let press = if out { Msg::Window(TabWindow::Raise(tab)) } else { Msg::Tab(tab) };
        line = line.push(button(face).padding(0).style(theme::segment_button(on)).on_press(press));
    }
    line = line.push(space().width(Length::Fill));
    if state.tab != Tab::Chat {
        line = line.push(strip(state, phase));
    }
    column![
        // on the page's own black, as the rail is, so the title bar above runs into it unbroken
        container(line.wrap()).padding([7.0, 12.0]).width(Length::Fill),
        container(space().height(1)).width(Length::Fill).style(theme::line),
    ]
    .into()
}

/// The line of chips beside the tabs: where chats are saved, and the local server.
fn strip(state: &State, phase: f32) -> El<'_> {
    let mut chips = Row::new().spacing(8).align_y(Alignment::Center);
    if state.busy() {
        chips = chips.push(crate::spinner::spinner(phase, 14.0));
    }
    match state.shared_writes() {
        Some(true) => chips = chips.push(chip("Chats saved with the web Lattice's", theme::POSITIVE)),
        Some(false) => chips = chips.push(chip("Read only: chats are not saved yet", theme::CAUTION)),
        None => {}
    }
    if let Some(local) = &state.local {
        let (words, color) = local_words(local.state);
        chips = chips.push(chip(words, color));
    }
    chips.into()
}

/// A review's result in words.
pub fn review_words(result: &ReviewResult) -> String {
    match result {
        ReviewResult::Kept { .. } => "Kept".to_string(),
        ReviewResult::PartlyKept { .. } => "Partly kept".to_string(),
        ReviewResult::Undone => "Undone".to_string(),
        ReviewResult::Rebased => "Re-applied to the file as it is now; review it again".to_string(),
        ReviewResult::Conflict { reason, .. } => format!("Conflict: {reason}"),
        ReviewResult::Skipped { reason } => format!("Left as it was: {reason}"),
        ReviewResult::Failed { reason } => format!("Failed: {reason}"),
    }
}

// ------------------------------------------------------------------ the dialog

fn dialog_card(dialog: &super::Dialog) -> El<'_> {
    let facts = &dialog.facts;
    let mut col = Column::new().spacing(8).push(strong(facts.title.as_str(), 17.0, theme::TEXT));
    for line in &facts.lines {
        col = col.push(label(line.as_str(), 13.0, theme::TEXT));
    }
    let ready = dialog.ready();
    // The refusing button comes first and is the default; both take input only once the dialog is ready.
    col = col.push(
        row![
            ui::primary(facts.refuse, ready.then_some(Msg::DialogAnswer(false))),
            ui::secondary(facts.confirm, ready.then_some(Msg::DialogAnswer(true))),
        ]
        .spacing(10),
    );
    col = col.push(note("Lattice asks this itself; nothing in a chat can answer it for you."));
    container(col).padding(22).max_width(620).style(theme::card).into()
}

// ------------------------------------------------------------------ runs

fn runs_tab(state: &State, phase: f32) -> El<'_> {
    let runs = &state.runs;
    let mut left = Column::new().spacing(8);
    left = left.push(strong("Start a run", 14.5, theme::TEXT));
    left = left.push(text_input("What should the agent do?", &runs.task).on_input(Msg::Task).on_submit(Msg::StartRun).padding(8).style(ui::input_style));
    let mut agents = Row::new().spacing(6);
    for a in &runs.agents {
        let on = runs.agent.as_deref() == Some(a.id.as_str());
        agents = agents.push(button(label(a.label.as_str(), 12.0, if on { theme::GOLD } else { theme::TEXT_DIM })).padding([4.0, 8.0]).style(theme::segment_button(on)).on_press(Msg::Agent(a.id.clone())));
    }
    left = left.push(agents.wrap());
    let mut models = Row::new().spacing(6);
    for m in &runs.models {
        let on = runs.model.as_deref() == Some(m.id.as_str());
        let color = if !m.ready { theme::TEXT_FAINT } else if on { theme::GOLD } else { theme::TEXT_DIM };
        models = models.push(button(label(m.label.as_str(), 12.0, color)).padding([4.0, 8.0]).style(theme::segment_button(on)).on_press_maybe(m.ready.then(|| Msg::Model(m.id.clone()))));
    }
    left = left.push(models.wrap());
    if runs.starting {
        left = left.push(ui::working(phase, "Starting the run…"));
    } else {
        left = left.push(ui::primary("Start", (!runs.task.trim().is_empty() && runs.agent.is_some() && runs.model.is_some()).then_some(Msg::StartRun)));
    }
    left = left.push(strong("Runs", 14.5, theme::TEXT));
    let mut list = Column::new().spacing(4);
    if runs.list.is_empty() {
        list = list.push(note(if runs.reading { "Reading the runs…" } else { "No runs yet." }));
    }
    for r in &runs.list {
        let on = runs.selected.as_deref() == Some(r.id.as_str());
        let line = row![ui::dot(theme::status_color(r.status)), label(ui::cut(&r.task, 34), 12.5, theme::TEXT)].spacing(6).align_y(Alignment::Center);
        let sub = label(format!("{} · {} · {}", r.agent_label, r.model_label, theme::status_word(r.status)), 11.0, theme::TEXT_FAINT);
        list = list.push(button(column![line, sub].spacing(2)).width(Length::Fill).padding([6.0, 8.0]).style(theme::list_row(on)).on_press(Msg::SelectRun(r.id.clone())));
    }
    left = left.push(scrollable(list).height(Length::Fill).style(theme::scrollbars));
    let left = container(left).width(320).height(Length::Fill);

    let right: El<'_> = match &runs.detail {
        None => container(note("Choose a run to see its trace.")).padding(14).width(Length::Fill).height(Length::Fill).style(theme::panel).into(),
        Some(run) => run_detail(run, &runs.rows),
    };
    row![left, right].spacing(14).height(Length::Fill).into()
}

fn run_detail<'a>(run: &'a lattice_app::runstate::RunState, rows: &'a [lattice_app::wfmodel::WfRow]) -> El<'a> {
    let s = &run.summary;
    let mut col = Column::new().spacing(10);
    let mut top = Row::new().spacing(10).align_y(Alignment::Center);
    top = top.push(strong(ui::cut(&s.task, 70), 15.0, theme::TEXT));
    top = top.push(chip(theme::status_word(run.status), theme::status_color(run.status)));
    top = top.push(space().width(Length::Fill));
    if run.status.is_active() {
        top = top.push(ui::secondary("Stop", Some(Msg::StopRun(s.id.clone()))));
    }
    col = col.push(top);
    col = col.push(fact("Agent and model", label(format!("{} · {}", s.agent_label, s.model_label), 12.5, theme::TEXT)));
    if let Some(usage) = &run.usage {
        col = col.push(fact("Tokens", label(format!("{} in, {} out, {} requests", usage.input_tokens, usage.output_tokens, usage.requests), 12.5, theme::TEXT)));
    } else {
        col = col.push(fact("Tokens", label("not reported", 12.5, theme::TEXT_FAINT)));
    }
    if let Some(error) = &run.error {
        col = col.push(ui::notice(error.as_str(), theme::DANGER));
    }
    col = col.push(strong("Trace", 13.5, theme::TEXT));
    col = col.push(waterfall(run, rows));
    if let Some((text, _)) = run.output_text() {
        col = col.push(strong("Output", 13.5, theme::TEXT));
        col = col.push(text_block(text));
    }
    container(scrollable(col.padding(padding::right(12))).height(Length::Fill).style(theme::scrollbars))
        .padding(14)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(theme::panel)
        .into()
}

fn text_block<'a>(t: String) -> El<'a> {
    text(t).size(13).color(theme::TEXT).font(fonts().ui).into()
}

/// The spans as bars on one time axis: each row its title, indented by depth, and a bar from its start to its end (an
/// open span reaches the run's end so far).
fn waterfall<'a>(run: &'a lattice_app::runstate::RunState, rows: &'a [lattice_app::wfmodel::WfRow]) -> El<'a> {
    if rows.is_empty() {
        return note("No spans recorded.");
    }
    let start = rows.iter().map(|r| r.start).fold(f64::INFINITY, f64::min);
    let end = rows
        .iter()
        .map(|r| r.end.unwrap_or(r.start))
        .chain(run.ended_at)
        .chain(std::iter::once(run.summary.updated_at))
        .fold(start, f64::max);
    let total = (end - start).max(1e-3);
    let mut col = Column::new().spacing(3);
    for r in rows {
        let (a, b) = bar_portions(r.start, r.end.unwrap_or(end), start, total);
        let color = theme::kind_color(r.kind, r.triggered);
        let color = if r.error { theme::DANGER } else { color };
        let bar = row![
            space().width(Length::FillPortion(a)),
            container(space().height(10)).width(Length::FillPortion(b)).style(move |_| container::Style {
                background: Some(color.into()),
                border: iced::Border { radius: 3.0.into(), ..iced::Border::default() },
                ..container::Style::default()
            }),
            space().width(Length::FillPortion(1000u16.saturating_sub(a + b).max(1))),
        ]
        .align_y(Alignment::Center);
        let dur = r.end.map(|e| format!("{:.2} s", e - r.start)).unwrap_or_else(|| "open".to_string());
        col = col.push(
            row![
                container(row![space().width(f32::from(r.depth) * 12.0), label(ui::cut(&r.title, 34), 12.0, theme::TEXT)]).width(280),
                container(bar).width(Length::Fill),
                container(label(dur, 11.0, theme::TEXT_FAINT)).width(70),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        );
    }
    col.into()
}

/// A bar's offset and length on a 1000-part axis: never zero long, never past the end.
pub fn bar_portions(from: f64, to: f64, start: f64, total: f64) -> (u16, u16) {
    let a = (((from - start) / total) * 1000.0).clamp(0.0, 999.0) as u16;
    let b = ((((to - from).max(0.0)) / total) * 1000.0).round().clamp(1.0, f64::from(1000 - a)) as u16;
    (a, b)
}

/// Where model endpoints are kept and how their keys are held, said once on hover beside "Add a model endpoint".
const ENDPOINTS_HELP: &str = "Endpoints are written to the same model_endpoints.json the Python and web Lattice read. A \
                              key you type is kept in Windows Credential Manager (as Alelyon/<KEY NAME>), never in a \
                              plain-text file; the native Lattice and Alelyon read it from there, and the Python and web \
                              Lattice will once they are taught to. Of a key, only whether it is present is read. The \
                              Python Lattice reads the same registry, the same choice of local model and the same \
                              Credential Manager entries.";

/// The endpoint form while one is open, and a removal waiting for Confirm; nothing otherwise (the button that opens a
/// form sits on the endpoints' own panel).
fn endpoint_editor(state: &State) -> Option<El<'_>> {
    use super::{FormField as F, FormFlag, RegistryAct};
    let mut col = Column::new().spacing(8);
    if let Some(act) = &state.pending_act {
        let words = match act {
            RegistryAct::Remove { id } => format!(
                "Remove {id} from the model registry? A built-in endpoint is disabled instead, as the Python Lattice does; \
                 the web Lattice reads the same file."
            ),
            RegistryAct::ForgetKey { name } => format!(
                "Forget {name} from Windows Credential Manager? Anything that used it will need it again."
            ),
            RegistryAct::Save { .. } => String::new(),
        };
        col = col.push(
            container(
                column![
                    label(words, 13.0, theme::TEXT),
                    row![ui::primary("Confirm", Some(Msg::ConfirmAct)), ui::secondary("Cancel", Some(Msg::CancelAct))].spacing(8),
                ]
                .spacing(8),
            )
            .padding(12)
            .width(Length::Fill)
            .style(theme::notice(theme::GOLD)),
        );
    }
    let Some(f) = &state.form else {
        return state.pending_act.is_some().then(|| col.into());
    };
    let managed = f.editing.as_deref() == Some("llamacpp-local");
    let field = |name: &'static str, value: &str, which: F, hint: &'static str| -> El<'_> {
        fact(
            name,
            text_input(hint, value).on_input(move |v| Msg::Form(which, v)).padding(6).size(13).style(ui::input_style),
        )
    };
    if f.editing.is_none() {
        col = col.push(field("Id", &f.id, F::Id, "a short id, e.g. my-vllm"));
    } else {
        col = col.push(fact("Id", mono(f.id.as_str(), 12.5, theme::TEXT)));
    }
    col = col.push(field("Label", &f.label, F::Label, "what the model list shows"));
    if !managed {
        let kind = row![
            button(label("OpenAI-compatible", 12.0, if f.anthropic { theme::TEXT_DIM } else { theme::GOLD }))
                .padding([4.0, 8.0])
                .style(theme::segment_button(!f.anthropic))
                .on_press(Msg::FormFlag(FormFlag::Anthropic, false)),
            button(label("Anthropic", 12.0, if f.anthropic { theme::GOLD } else { theme::TEXT_DIM }))
                .padding([4.0, 8.0])
                .style(theme::segment_button(f.anthropic))
                .on_press(Msg::FormFlag(FormFlag::Anthropic, true)),
        ]
        .spacing(6);
        col = col.push(fact("Kind", kind));
        col = col.push(field("Address", &f.base_url, F::BaseUrl, "https://host/v1 (http only on this computer)"));
    }
    col = col.push(field("Model", &f.model, F::Model, "the model name the server knows"));
    if !managed {
        col = col.push(field("Key name", &f.key_name, F::KeyName, "e.g. MY_SERVICE_API_KEY (empty: no key)"));
        col = col.push(fact(
            "Key",
            text_input("type a key to keep it in Credential Manager (left empty: unchanged)", &f.key)
                .on_input(|v| Msg::Form(F::Key, v))
                .secure(true)
                .padding(6)
                .size(13)
                .style(ui::input_style),
        ));
    }
    col = col.push(field("Note", &f.note, F::Note, "optional"));
    col = col.push(fact(
        "Enabled",
        row![
            button(label("On", 12.0, if f.enabled { theme::GOLD } else { theme::TEXT_DIM }))
                .padding([4.0, 8.0])
                .style(theme::segment_button(f.enabled))
                .on_press(Msg::FormFlag(FormFlag::Enabled, true)),
            button(label("Off", 12.0, if f.enabled { theme::TEXT_DIM } else { theme::GOLD }))
                .padding([4.0, 8.0])
                .style(theme::segment_button(!f.enabled))
                .on_press(Msg::FormFlag(FormFlag::Enabled, false)),
        ]
        .spacing(6),
    ));
    if f.confirming {
        let mut words = format!(
            "This writes {} to the model registry (model_endpoints.json, shared with the Python and web Lattice).",
            if f.label.trim().is_empty() { "this endpoint" } else { f.label.trim() }
        );
        if !f.key.trim().is_empty() {
            words.push_str(&format!(
                " The key goes to Windows Credential Manager as Alelyon/{}, replacing any kept there.",
                f.key_name.trim()
            ));
        }
        col = col.push(
            container(label(words, 13.0, theme::TEXT)).padding(10).width(Length::Fill).style(theme::notice(theme::GOLD)),
        );
    }
    col = col.push(
        row![
            ui::primary(if f.confirming { "Confirm and save" } else { "Save…" }, (!state.acting).then_some(Msg::SaveEndpoint)),
            ui::secondary("Cancel", Some(Msg::CancelForm)),
        ]
        .spacing(8),
    );
    Some(ui::card(if f.editing.is_some() { "Edit endpoint" } else { "New endpoint" }, col))
}

// ------------------------------------------------------------------ training

/// What a Training Studio workspace is, said once on hover beside the tab's title.
const TRAINING_HELP: &str = "A Training Studio workspace, as the Python Studio keeps it: a pool of local documents (each \
                             its normalised text, named by its SHA-256, in an append-only history of revisions) and \
                             saved plans binding a revision to a preset and its experts. Every file is written new and \
                             none is replaced or deleted; a window that is not looking at the latest revision is refused. \
                             Nothing here starts a trainer. A trainer's live metrics (loss, steps, expert use) are on the \
                             Compute and simulation page, under Training.";

fn training_tab(state: &State, phase: f32, width: f32) -> El<'_> {
    use super::training::PRESETS;
    let t = &state.training;
    let mut col = Column::new().spacing(14);

    // At a glance: which workspace is open and what it holds.
    let (words, color) = match &t.workspace {
        None => ("No workspace open".to_string(), theme::TEXT_FAINT),
        Some(Err(_)) => ("The workspace could not be read".to_string(), theme::CAUTION),
        Some(Ok(w)) => (
            format!(
                "{} · revision {} · {} documents · {} saved plan(s)",
                ui::cut(&w.id, 40),
                w.revision.number,
                w.revision.documents.len(),
                w.plans.len()
            ),
            theme::POSITIVE,
        ),
    };
    col = col.push(
        row![ui::subheading("Training Studio"), ui::help(TRAINING_HELP), space().width(8), ui::dot(color), label(words, 13.0, theme::TEXT)]
            .spacing(8)
            .align_y(Alignment::Center),
    );
    let busy = t.writing || t.opening;
    if t.writing {
        col = col.push(ui::working(phase, "Writing to the workspace…"));
    }
    match &t.said {
        Some(Ok(words)) => col = col.push(ui::notice(words.as_str(), theme::POSITIVE)),
        Some(Err(why)) => col = col.push(ui::notice(why.as_str(), theme::CAUTION)),
        None => {}
    }

    // Open a workspace, or make a new one, side by side.
    let mut open = Column::new().spacing(6);
    open = open.push(
        row![
            text_input("A Training Studio workspace folder", &t.workspace_path)
                .on_input(Msg::WorkspacePath)
                .on_submit(Msg::OpenWorkspace)
                .padding(8)
                .style(ui::input_style),
            ui::primary("Open", (!t.opening && !t.workspace_path.trim().is_empty()).then_some(Msg::OpenWorkspace)),
        ]
        .spacing(8),
    );
    if t.opening {
        open = open.push(ui::working(phase, "Reading the workspace and checking each document against its digest…"));
    }
    match &t.workspace {
        None => {}
        Some(Err(why)) => open = open.push(ui::notice(why.as_str(), theme::CAUTION)),
        Some(Ok(w)) => {
            open = open.push(fact("Workspace", mono(w.id.as_str(), 12.0, theme::TEXT)));
            open = open.push(fact(
                "Revision",
                label(format!("{} · {} documents · manifest {}", w.revision.number, w.revision.documents.len(), ui::cut(&w.revision.sha256, 16)), 12.5, theme::TEXT),
            ));
        }
    }
    let name_ok = super::simple_folder_name(t.new_name.trim());
    let can_create = !busy && !t.new_parent.trim().is_empty() && name_ok;
    let mut create = Column::new().spacing(6);
    create = create.push(
        row![
            text_input("The folder it goes in", &t.new_parent).on_input(Msg::NewParent).padding(8).style(ui::input_style),
            ui::secondary("Choose…", (!busy).then_some(Msg::ChooseNewParent)),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    );
    create = create.push(
        row![
            text_input("Its name", &t.new_name).on_input(Msg::NewName).padding(8).style(ui::input_style),
            ui::secondary(
                "Create",
                can_create.then(|| {
                    Msg::Train(super::TrainingAct::Create(std::path::Path::new(t.new_parent.trim()).join(t.new_name.trim())))
                })
            ),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    );
    if !t.new_name.trim().is_empty() && !name_ok {
        create = create.push(note("Choose a simple folder name without path separators."));
    }
    col = col.push(two(width, vec![panel("Workspace", "", None, open)], vec![panel("New workspace", "", None, create)]));

    let Some(Ok(w)) = &t.workspace else {
        return scrollable(col.padding(padding::right(12))).height(Length::Fill).style(theme::scrollbars).into();
    };

    // The document pool: one line each, and the line that adds one.
    let mut docs = Column::new().spacing(4);
    if w.revision.documents.is_empty() {
        docs = docs.push(note("No documents yet."));
    } else {
        docs = docs.push(table_head(&[("Name", 8), ("Source", 6), ("License", 4), ("Group", 3), ("Size", 2), ("", 2)]));
    }
    for d in &w.revision.documents {
        docs = docs.push(
            row![
                container(label(ui::cut(&d.name, 40), 12.5, theme::TEXT)).width(Length::FillPortion(8)),
                container(label(ui::cut(&d.source, 30), 12.0, theme::TEXT_DIM)).width(Length::FillPortion(6)),
                container(label(ui::cut(&d.license, 20), 12.0, theme::TEXT_DIM)).width(Length::FillPortion(4)),
                container(label(ui::cut(&d.group, 16), 12.0, theme::TEXT_DIM)).width(Length::FillPortion(3)),
                container(label(ui::bytes(d.bytes), 12.0, theme::TEXT_DIM)).width(Length::FillPortion(2)),
                container(ui::secondary("Remove", (!busy).then(|| Msg::Train(super::TrainingAct::Remove(d.id.clone())))))
                    .width(Length::FillPortion(2))
                    .align_x(iced::alignment::Horizontal::Right),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        );
    }
    // Adding a document: a UTF-8 .txt or .md file, with where it came from and the permission to use it.
    let attributed = !t.doc_source.trim().is_empty() && !t.doc_license.trim().is_empty();
    let can_add = !busy && !t.doc_path.trim().is_empty() && attributed;
    docs = docs.push(
        row![
            text_input("Add a UTF-8 .txt or .md file", &t.doc_path).on_input(Msg::DocPath).padding(8).style(ui::input_style),
            container(text_input("Source ID", &t.doc_source).on_input(Msg::DocSource).padding(8).style(ui::input_style)).width(180),
            container(text_input("License or permission", &t.doc_license).on_input(Msg::DocLicense).padding(8).style(ui::input_style)).width(200),
            container(text_input("Group (optional)", &t.doc_group).on_input(Msg::DocGroup).padding(8).style(ui::input_style)).width(150),
            ui::primary(
                "Add",
                can_add.then(|| {
                    Msg::Train(super::TrainingAct::Add {
                        file: std::path::PathBuf::from(t.doc_path.trim()),
                        source: t.doc_source.trim().to_string(),
                        license: t.doc_license.trim().to_string(),
                        group: t.doc_group.trim().to_string(),
                    })
                })
            ),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    );
    col = col.push(panel(
        "Documents",
        "Every document needs a source ID (letters, numbers or . _ : / @ + -) and a license or permission. Its text \
             is stored normalised (UTF-8, LF line ends, NFC, trimmed), at most 4 MiB, 32 MiB in the pool; the file \
             itself is only read.",
        Some(label(format!("{} in revision {}", w.revision.documents.len(), w.revision.number), 12.0, theme::TEXT_DIM).into()),
        docs,
    ));

    // The saved plans and the export, side by side.
    let mut plans = Column::new().spacing(4);
    if w.plans.is_empty() {
        plans = plans.push(note("None saved."));
    }
    for (index, p) in w.plans.iter().enumerate() {
        let name = p.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        plans = plans.push(match &p.plan {
            Ok(plan) => row![
                label(
                    format!(
                        "{name}: preset {} ({} layers, {} experts, {} active, width {}), {} experts chosen, revision {}",
                        plan.preset.label, plan.preset.layers, plan.preset.experts, plan.preset.active,
                        plan.preset.width, plan.selected_experts.len(), plan.revision
                    ),
                    12.0,
                    theme::TEXT,
                )
                .width(Length::Fill),
                ui::secondary("Use", (!busy).then_some(Msg::UsePlan(index))),
            ]
            .spacing(8)
            .align_y(Alignment::Center)
            .into(),
            Err(why) => El::from(label(format!("{name}: not usable: {why}"), 12.0, theme::CAUTION)),
        });
    }
    let can_export = !busy && !w.revision.documents.is_empty() && !t.export_path.trim().is_empty();
    let export = Column::new().push(
        row![
            text_input("A new .jsonl file", &t.export_path).on_input(Msg::ExportPath).padding(8).style(ui::input_style),
            ui::secondary(
                "Export",
                can_export.then(|| Msg::Train(super::TrainingAct::Export(std::path::PathBuf::from(t.export_path.trim()))))
            ),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    );
    col = col.push(two(
        width,
        vec![panel("Saved plans", "", None, plans)],
        vec![panel(
            "Export a document snapshot",
            "One line per document: its normalised text, attribution and provenance, marked not ready to train. A \
                 file that exists is never overwritten.",
            None,
            export,
        )],
    ));

    // A plan: a preset and the experts it trains, bound to the latest revision when saved.
    let mut plan = Column::new().spacing(6);
    let keys: Vec<&'static str> = PRESETS.iter().map(|p| p.key).collect();
    plan = plan.push(ui::tabs(
        &keys,
        t.preset,
        |key| {
            let p = super::training::preset(key).expect("a preset's own key");
            format!("{} · {} layers × {} experts ({} active), width {}", p.label, p.layers, p.experts, p.active, p.width)
        },
        Msg::PlanPreset,
    ));
    if let Some(shape) = super::training::preset(t.preset) {
        for layer in 0..shape.layers {
            let mut line = Row::new().spacing(3).push(container(label(format!("Layer {layer}"), 12.0, theme::TEXT_DIM)).width(70));
            for expert in 0..shape.experts {
                let key = format!("{layer}:{expert}");
                let on = t.experts.contains(&key);
                line = line.push(
                    button(label(expert.to_string(), 12.0, if on { theme::GOLD } else { theme::TEXT_DIM }))
                        .padding([4.0, 7.0])
                        .on_press(Msg::ToggleExpert(key))
                        .style(theme::segment_button(on)),
                );
            }
            plan = plan.push(line.wrap());
        }
    }
    let can_save = !busy && !t.experts.is_empty() && !w.revision.documents.is_empty();
    let save = row![
        label(format!("{} experts chosen", t.experts.len()), 12.5, theme::TEXT),
        ui::primary(
            "Save training plan",
            can_save.then(|| Msg::Train(super::TrainingAct::SavePlan { preset: t.preset, experts: t.experts.iter().cloned().collect() }))
        ),
    ]
    .spacing(10)
    .align_y(Alignment::Center);
    col = col.push(panel(
        "Training plan",
        "A plan binds this revision's documents by their digests and the manifest's SHA-256; it is never marked ready \
             to train. Saving needs a document in the pool and at least one expert.",
        Some(save.into()),
        plan,
    ));
    scrollable(col.padding(padding::right(12))).height(Length::Fill).style(theme::scrollbars).into()
}

// ------------------------------------------------------------------ system

/// The System tab's closed-by-default part, by name (`State::open_parts`).
const STACK_GAPS: &str = "lattice.system.gaps";

fn system_tab(state: &State, phase: f32) -> El<'_> {
    let Some(sys) = &state.system else {
        return ui::working(phase, "Reading the stack and the Action Gate's ledger…");
    };
    let mut col = Column::new().spacing(14);
    // The stack: one line a layer, what it reached in words and colour; how it is wired on hover.
    let mut stack = Column::new().spacing(4);
    for r in &sys.stack {
        let (word, color) = match r.level {
            Level::Present => ("PRESENT", theme::POSITIVE),
            Level::Gated => ("GATED", theme::GOLD),
            Level::Absent => ("ABSENT", theme::TEXT_FAINT),
        };
        stack = stack.push(
            row![
                container(row![strong(r.name, 13.0, theme::TEXT), ui::help(r.detail)].spacing(6).align_y(Alignment::Center)).width(260),
                container(chip(word, color)).width(90),
                mono(r.headline.as_str(), 12.0, theme::TEXT_DIM).width(Length::Fill),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        );
    }
    let mut gaps = Column::new().spacing(3);
    for gap in super::system::STANDING_GAPS {
        gaps = gaps.push(label(format!("• {gap}"), 12.0, theme::TEXT_DIM));
    }
    stack = stack.push(ui::disclosure(
        "What this stack does not establish",
        label(format!("{} standing gaps", super::system::STANDING_GAPS.len()), 12.0, theme::CAUTION),
        state.open_parts.contains(STACK_GAPS),
        Msg::Disclose(STACK_GAPS),
        gaps,
    ));
    col = col.push(panel(
        "Stack: what sits underneath an answer",
        "Each row reports what this build could actually reach. Linked is not published, published is not \
             externally verified, and none of them is a statement that a number is right.",
        None,
        stack,
    ));

    let mut gate = Column::new().spacing(6);
    match &sys.ledger {
        Err(why) => gate = gate.push(ui::notice(why.as_str(), theme::CAUTION)),
        Ok(l) => {
            gate = gate.push(fact("Ledger", mono(l.path.display().to_string(), 12.0, theme::TEXT_DIM)));
            if l.absent {
                gate = gate.push(ui::notice(
                    "No ledger here: nothing was recorded, or it was deleted. The two look alike, so this is not a clean bill.",
                    theme::GOLD_DIM,
                ));
            } else {
                let v = &l.verdict;
                if v.ok {
                    gate = gate.push(
                        row![
                            ui::notice(format!("The chain re-derives: {} records, each link from its own content.", v.checked), theme::POSITIVE),
                            ui::help(
                                "It cannot show that later records were not cut off; compare the head below with one kept elsewhere."
                            ),
                        ]
                        .spacing(8)
                        .align_y(Alignment::Center),
                    );
                } else {
                    gate = gate.push(ui::notice(format!("The chain does not re-derive after {} records: {}", v.checked, v.reason), theme::DANGER));
                }
                if v.unparseable > 0 {
                    gate = gate.push(note(format!("{} line(s) are not valid JSON.", v.unparseable)));
                }
                let (count, head, seq) = l.head();
                gate = gate.push(fact("Head", mono(format!("{count} records, seq {seq}, {head}"), 11.5, theme::TEXT)));
                let kinds: Vec<String> = l.kinds.iter().map(|(k, n)| format!("{k} {n}")).collect();
                gate = gate.push(fact("By kind", label(kinds.join(", "), 12.5, theme::TEXT)));
                gate = gate.push(fact("Sessions", label(l.sessions.len().to_string(), 12.5, theme::TEXT)));
                for r in l.records.iter().rev().take(super::system::SHOWN) {
                    let payload = ui::cut(&r.payload.to_string(), 140);
                    gate = gate.push(
                        row![
                            container(mono(format!("#{}", r.seq), 11.5, theme::TEXT_FAINT)).width(60),
                            container(label(crate::utc::stamp(r.ts as f64), 11.5, theme::TEXT_DIM)).width(150),
                            container(label(r.kind.as_str(), 11.5, theme::GOLD)).width(100),
                            container(mono(ui::cut(&r.session, 12), 11.5, theme::TEXT_DIM)).width(110),
                            mono(payload, 11.5, theme::TEXT),
                        ]
                        .spacing(8),
                    );
                }
            }
            if !sys.evidence.is_empty() {
                gate = gate.push(
                    row![
                        strong("What each session was observed to do", 13.0, theme::TEXT),
                        ui::help(
                            "Replayed from the records the harness wrote when each tool call happened; a session's own \
                             say-so counts only for its falsifier. As the Python view does, any read of AGENTS.md holds \
                             P1 here (a changed policy is not compared)."
                        ),
                    ]
                    .spacing(8)
                    .align_y(Alignment::Center),
                );
            }
            for (session, e) in &sys.evidence {
                let mut line =
                    Row::new().spacing(6).align_y(Alignment::Center).push(mono(ui::cut(session, 14), 12.0, theme::TEXT));
                if e.held.is_empty() {
                    line = line.push(label("nothing observed", 11.5, theme::TEXT_FAINT));
                }
                // Each obligation held, its meaning on hover.
                for h in &e.held {
                    line = line.push(ui::tip(chip(h.as_str(), theme::POSITIVE), format!("{h}: {}", super::obligations::meaning(h))));
                }
                let mut block = Column::new().spacing(2).push(line.wrap());
                if !e.falsifier.is_empty() {
                    block = block.push(label(format!("Falsifier: {}", ui::cut(&e.falsifier, 200)), 11.5, theme::TEXT_DIM));
                }
                for g in &e.grants {
                    block = block.push(label(format!("Owner grant: {g}"), 11.5, theme::GOLD));
                }
                for n in &e.notes {
                    block = block.push(label(n.as_str(), 11.5, theme::CAUTION));
                }
                gate = gate.push(block);
            }
        }
    }
    col = col.push(panel(
        "Action Gate: what agents were allowed or refused",
        "Not read here: the trading gate's decisions (gate_decisions.jsonl) and the intent ledger (fam.db) belong \
             to the Financial Markets platform, which Alelyon leaves out for now.",
        None,
        gate,
    ));
    scrollable(col.padding(padding::right(12))).height(Length::Fill).style(theme::scrollbars).into()
}

// ------------------------------------------------------------------ models

fn models_tab(state: &State, width: f32) -> El<'_> {
    let mut col = Column::new().spacing(14);
    col = col.push(ui::grid(models_glance(state), 3, 12.0));

    // The endpoints: one line each, what they are and whether they answer, with their actions at the right.
    let mut reg = Column::new().spacing(4);
    match &state.endpoints {
        None => reg = reg.push(note("Reading the model registry…")),
        Some(e) => {
            reg = reg.push(fact("Registry", mono(e.path.as_str(), 12.0, theme::TEXT_DIM)));
            for issue in &e.issues {
                reg = reg.push(ui::notice(format!("Part of the registry could not be read: {issue}"), theme::CAUTION));
            }
            reg = reg.push(table_head(&[("Name", 5), ("Kind", 2), ("Where", 2), ("State", 3), ("", 3)]));
            let free = !state.acting && state.pending_act.is_none();
            for r in &e.rows {
                reg = reg.push(endpoint_line(r, free));
            }
        }
    }
    let add = ui::primary("Add a model endpoint", (!state.acting).then_some(Msg::NewEndpoint));
    col = col.push(panel("Model endpoints", ENDPOINTS_HELP, Some(add), reg));
    if let Some(editor) = endpoint_editor(state) {
        col = col.push(editor);
    }

    // What the chat can use, and the local server it may start.
    let mut choices = Column::new().spacing(4);
    if state.choices.is_empty() {
        choices = choices.push(note("Reading the model choices…"));
    }
    for c in &state.choices {
        let local = c.locality == Locality::Local;
        let mut line = Row::new().spacing(8).align_y(Alignment::Center);
        line = line.push(strong(c.label.as_str(), 13.0, theme::TEXT));
        if !c.detail.is_empty() {
            line = line.push(ui::help(c.detail.as_str()));
        }
        line = line.push(space().width(Length::Fill));
        if c.id == state.choice {
            line = line.push(chip("In use", theme::GOLD));
        }
        line = line.push(chip(if local { "This computer" } else { "Off this computer" }, if local { theme::POSITIVE } else { theme::CAUTION }));
        line = line.push(chip(if c.ready { "Ready" } else { "Not ready" }, if c.ready { theme::POSITIVE } else { theme::TEXT_FAINT }));
        let mut item = Column::new().spacing(2).push(line);
        if let Some(r) = &c.refusal {
            item = item.push(label(r.as_str(), 12.0, theme::CAUTION));
        }
        choices = choices.push(item);
    }
    let choices = panel("Chat's model choices", "", None, choices);
    col = col.push(two(width, vec![choices], vec![local_server(state)]));

    // What is read once in a while: the chat core, and the agent runs' models.
    let mut core = Column::new().spacing(6);
    let mut core_summary = String::from("not read yet");
    if let Some(s) = &state.status {
        core_summary = format!("{} · {} agent turn(s) running", s.runtime, s.agent_turns);
        core = core.push(fact("Runtime", label(s.runtime.as_str(), 13.0, theme::TEXT)));
        core = core.push(fact("Agent turns running", label(s.agent_turns.to_string(), 13.0, theme::TEXT)));
        core = core.push(fact("Chats saved", label(if s.shared_writes { "yes, with the web Lattice's" } else { "no: read only until the gate opens" }, 13.0, theme::TEXT)));
        if let Some(r) = &s.refusal {
            core = core.push(ui::notice(r.as_str(), theme::CAUTION));
        }
    }
    if let Some(l) = &state.list {
        core = core.push(fact("Chat store", mono(l.store_dir.as_str(), 12.0, theme::TEXT_DIM)));
        core = core.push(fact("Chats kept listed", label(format!("{} (older ones are archived, never deleted)", l.limit), 12.5, theme::TEXT_DIM)));
    }
    if state.status.is_none() && state.list.is_none() {
        core = core.push(note("Reading the chat core…"));
    }
    col = col.push(ui::disclosure(
        "The chat core",
        label(core_summary, 12.0, theme::TEXT_DIM),
        state.open_parts.contains(CHAT_CORE),
        Msg::Disclose(CHAT_CORE),
        core,
    ));
    let mut runs = Column::new().spacing(4);
    for m in &state.runs.models {
        runs = runs.push(row![strong(m.label.as_str(), 13.0, theme::TEXT), label(if m.ready { "ready" } else { m.refusal.as_deref().unwrap_or("not ready") }, 12.0, theme::TEXT_DIM)].spacing(10));
    }
    let runs_summary = if state.runs.models.is_empty() {
        runs = runs.push(note("Open the runs tab to read the agent runs' models."));
        "not read yet: open the Runs tab".to_string()
    } else {
        format!("{} of {} ready", state.runs.models.iter().filter(|m| m.ready).count(), state.runs.models.len())
    };
    col = col.push(ui::disclosure(
        "Agent runs' models",
        label(runs_summary, 12.0, theme::TEXT_DIM),
        state.open_parts.contains(RUNS_MODELS),
        Msg::Disclose(RUNS_MODELS),
        runs,
    ));
    scrollable(col.padding(padding::right(12))).height(Length::Fill).style(theme::scrollbars).into()
}

/// The Models tab's closed-by-default parts, by name (`State::open_parts`).
const CHAT_CORE: &str = "lattice.models.core";
const RUNS_MODELS: &str = "lattice.models.runs";

/// The Models tab at a glance: the chat's model, the local server and the endpoints, each its state in words.
fn models_glance(state: &State) -> Vec<El<'_>> {
    let chat = match state.choices.iter().find(|c| c.id == state.choice) {
        None if state.choices.is_empty() => tile("Chat model", "Reading…", theme::TEXT_FAINT, String::new()),
        None => tile("Chat model", "None chosen", theme::CAUTION, "choose one in the chat"),
        Some(c) => {
            let where_ = if c.locality == Locality::Local { "This computer" } else { "Off this computer" };
            let more = match &c.refusal {
                Some(r) if !c.ready => ui::cut(r, 80),
                _ => format!("{where_} · {}", if c.ready { "ready" } else { "not ready" }),
            };
            tile("Chat model", c.label.as_str(), if c.ready { theme::POSITIVE } else { theme::CAUTION }, more)
        }
    };
    let local = match &state.local {
        None => tile("Local server", "Reading…", theme::TEXT_FAINT, String::new()),
        Some(l) => {
            let (words, color) = local_words(l.state);
            tile("Local server", words, color, if l.model.is_empty() { "no model chosen".to_string() } else { ui::cut(&l.model, 60) })
        }
    };
    let endpoints = match &state.endpoints {
        None => tile("Endpoints", "Reading…", theme::TEXT_FAINT, String::new()),
        Some(e) => {
            let ready = e.rows.iter().filter(|r| r.ready).count();
            let more = if e.issues.is_empty() {
                format!("{} on this computer", e.rows.iter().filter(|r| r.local).count())
            } else {
                format!("{} part(s) of the registry unread", e.issues.len())
            };
            let color = if !e.issues.is_empty() || ready == 0 { theme::CAUTION } else { theme::POSITIVE };
            tile("Endpoints", format!("{ready} of {} ready", e.rows.len()), color, more)
        }
    };
    vec![chat, local, endpoints]
}

/// One endpoint as a line of the table: its name and model (the rest of what is known about it on hover), kind,
/// where it runs, its state, and what can be done to it.
fn endpoint_line(r: &super::EndpointRow, free: bool) -> El<'_> {
    let mut name = Row::new().spacing(8).align_y(Alignment::Center).push(strong(r.label.as_str(), 13.0, theme::TEXT));
    if !r.model.is_empty() {
        name = name.push(mono(ui::cut(&r.model, 30), 11.5, theme::TEXT_DIM));
    }
    let mut more = Vec::new();
    if r.builtin {
        more.push("Built in.".to_string());
    }
    if !r.key_name.is_empty() {
        more.push(format!("Key: {} (only whether it is present is read).", r.key_name));
    }
    if let Some(source) = &r.key_source {
        more.push(format!("Key found in {source}."));
    }
    if !more.is_empty() {
        name = name.push(ui::help(more.join(" ")));
    }
    let mut acts = Row::new().spacing(6).push(ui::secondary("Edit", free.then(|| Msg::EditEndpoint(r.saved.id.clone()))));
    if r.saved.id != "llamacpp-local" {
        acts = acts.push(ui::secondary(if r.builtin { "Disable" } else { "Remove" }, free.then(|| Msg::AskRemove(r.saved.id.clone()))));
    }
    if r.key_source.as_deref() == Some("Credential Manager") {
        acts = acts.push(ui::secondary("Forget key", free.then(|| Msg::AskForgetKey(r.key_name.clone()))));
    }
    row![
        container(name).width(Length::FillPortion(5)),
        container(label(r.kind, 12.0, theme::TEXT_DIM)).width(Length::FillPortion(2)),
        container(chip(if r.local { "This computer" } else { "Off this computer" }, if r.local { theme::POSITIVE } else { theme::CAUTION }))
            .width(Length::FillPortion(2)),
        container(label(r.status.as_str(), 12.0, if r.ready { theme::POSITIVE } else { theme::TEXT_DIM })).width(Length::FillPortion(3)),
        container(acts).width(Length::FillPortion(3)).align_x(iced::alignment::Horizontal::Right),
    ]
    .spacing(10)
    .align_y(Alignment::Center)
    .into()
}

/// The local model server: its state, the model it runs (chosen here from the GGUF files installed, as the Python
/// model bar chooses it) and what is installed.
fn local_server(state: &State) -> El<'_> {
    let mut local = Column::new().spacing(6);
    match &state.local {
        None => local = local.push(note("Reading the local server…")),
        Some(l) => {
            local = local.push(fact("State", label(l.headline.as_str(), 13.0, theme::TEXT)));
            if !l.detail.is_empty() {
                local = local.push(note(l.detail.as_str()));
            }
            let chosen: Element<'_, Msg> = if l.installed.is_empty() {
                label(if l.model.is_empty() { "none" } else { l.model.as_str() }, 13.0, theme::TEXT).into()
            } else {
                let selected = (!l.model.is_empty()).then(|| l.model.clone());
                pick_list(l.installed.clone(), selected, Msg::ChooseLocal)
                    .placeholder("Choose a model")
                    .padding(6)
                    .text_size(13.0)
                    .font(fonts().ui)
                    .style(theme::picker)
                    .menu_style(theme::picker_menu)
                    .into()
            };
            local = local.push(fact("Chosen model", chosen));
            local = local.push(fact("Installed", label(if l.installed.is_empty() { "none found".to_string() } else { l.installed.join(", ") }, 12.5, theme::TEXT_DIM)));
            if !l.model.is_empty() && !l.installed.contains(&l.model) {
                local = local.push(note("The chosen model is not among the GGUF files in the models folder; choose one that is."));
            }
        }
    }
    panel(
        "The local model server",
        "Local and Auto use only this server, on this computer: llama.cpp, started when a chat first needs it and stopped when idle.",
        None,
        local,
    )
}

/// The local server's state in words and its colour (the bar's chip and the glance's tile say the same).
fn local_words(state: RuntimeState) -> (&'static str, iced::Color) {
    match state {
        RuntimeState::Ready => ("Local model ready", theme::POSITIVE),
        RuntimeState::NoModel => ("No local model chosen", theme::CAUTION),
        RuntimeState::Offline => ("Local server not running", theme::TEXT_FAINT),
        RuntimeState::Error => ("Local server failed", theme::DANGER),
    }
}

// ------------------------------------------------------------------ the parts the tabs share

/// A panel: its title in small capitals, what it is for on hover (`ui::help`; none when `help` is empty), and at its
/// right `aside` (a button, a count). An explanation is read once, so it waits on hover rather than standing on the page (UX-PRINCIPLES B2).
pub(super) fn panel<'a, M: 'a>(
    title: impl text::IntoFragment<'a>,
    help: impl text::IntoFragment<'a>,
    aside: Option<Element<'a, M>>,
    body: Column<'a, M>,
) -> Element<'a, M> {
    let mut head = Row::new().spacing(8).align_y(Alignment::Center).push(ui::subheading(title));
    let help = help.into_fragment();
    if !help.is_empty() {
        head = head.push(ui::help(help));
    }
    head = head.push(space().width(Length::Fill));
    if let Some(aside) = aside {
        head = head.push(aside);
    }
    container(column![head, body.spacing(6)].spacing(10)).padding([12.0, 14.0]).width(Length::Fill).style(theme::panel).into()
}

/// One tile of a tab's glance: what it is, its state in words beside a dot of its colour, and one line more.
pub(super) fn tile<'a, M: 'a>(
    name: &'a str,
    state: impl text::IntoFragment<'a>,
    color: iced::Color,
    more: impl text::IntoFragment<'a>,
) -> Element<'a, M> {
    container(
        column![
            ui::caps(name, 10.5, theme::TEXT_DIM),
            row![ui::dot(color), strong(state, 14.0, theme::TEXT)].spacing(8).align_y(Alignment::Center),
            label(more, 11.5, theme::TEXT_FAINT),
        ]
        .spacing(4),
    )
    .padding([10.0, 14.0])
    .width(Length::Fill)
    .style(theme::panel)
    .into()
}

/// Two columns of parts from a laptop's width (`ui::columns_for`), one after the other on a narrow window.
pub(super) fn two<'a, M: 'a>(width: f32, left: Vec<Element<'a, M>>, right: Vec<Element<'a, M>>) -> Element<'a, M> {
    let stack = |items: Vec<Element<'a, M>>| items.into_iter().fold(Column::new().spacing(14), |c, e| c.push(e));
    if ui::columns_for(width) >= 2 {
        row![stack(left).width(Length::FillPortion(1)), stack(right).width(Length::FillPortion(1))].spacing(14).into()
    } else {
        stack(left.into_iter().chain(right).collect()).into()
    }
}

/// A table's header: each column's name over its share of the width, as the rows below share it.
pub(super) fn table_head<'a, M: 'a>(columns: &[(&'a str, u16)]) -> Element<'a, M> {
    let mut line = Row::new().spacing(10);
    for &(name, share) in columns {
        line = line.push(container(label(name, 11.0, theme::TEXT_FAINT)).width(Length::FillPortion(share)));
    }
    line.into()
}

/// How an answer's markdown is drawn (the IDE's agent panel uses it too).
pub(crate) fn settings() -> markdown::Settings {
    settings_sized(14.0)
}

/// How an answer's markdown is drawn, its body text `size` pixels high (the chat's answers are set larger, to read
/// at length).
pub(crate) fn settings_sized(size: f32) -> markdown::Settings {
    let f = fonts();
    markdown::Settings::with_text_size(
        size,
        markdown::Style {
            font: f.ui,
            inline_code_highlight: markdown::Highlight { background: theme::RAISED.into(), border: iced::border::rounded(4) },
            inline_code_padding: padding::left(2).right(2),
            inline_code_color: theme::TEXT,
            inline_code_font: f.mono,
            code_block_font: f.mono,
            link_color: theme::GOLD,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bar_is_never_empty_and_never_runs_past_the_axis() {
        assert_eq!(bar_portions(0.0, 10.0, 0.0, 10.0), (0, 1000));
        assert_eq!(bar_portions(5.0, 5.0, 0.0, 10.0), (500, 1));
        let (a, b) = bar_portions(9.999, 10.0, 0.0, 10.0);
        assert!(a + b <= 1000 && b >= 1);
    }

    #[test]
    fn a_failed_review_says_why() {
        assert_eq!(review_words(&ReviewResult::Failed { reason: "the disk is full".into() }), "Failed: the disk is full");
    }
}
