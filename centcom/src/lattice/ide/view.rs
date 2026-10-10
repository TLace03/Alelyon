//! What the IDE draws: the activity bar, the side bar (Explorer, Chats, Changes), the editor and its tabs, the bottom
//! panel, the agent on the right ([`super::agent`]), the status bar, and the quick-open box and confirmations over
//! them. It reads only what `update` built.

use iced::widget::{
    Column, Row, button, center, column, container, mouse_area, opaque, responsive, rich_text, row, scrollable, space,
    span, stack, text, text_editor, text_input, tooltip,
};
use iced::{Alignment, Background, Border, Color, Element, Font, Length, Padding, mouse, padding};

use crate::theme::{self, fonts};
use lattice_protocol::conversation::{ChangeKind, ChangeState, DiffLineKind, HunkState, Lines, ReviewOp};

use super::highlight::{self, Lang};
use super::{
    Body, Confirm, EditorTab, PanelTab, Side, Splitter, TabKind, CODE_PAD, CODE_SIZE, LINE_HEIGHT,
    PAGE_LINES, file_name, tree,
};
use crate::lattice::ide::IdeMsg;
use crate::lattice::{Msg, State};
use crate::ui::{self, label, mono, note, strong};

type El<'a> = Element<'a, Msg>;

/// The activity bar's width, the splitters' grip, and the narrowest the editor may get before the side bar folds away.
const ACTIVITY: f32 = 46.0;
const GRIP: f32 = 5.0;
const MIN_EDITOR: f32 = 300.0;
const STATUS_HEIGHT: f32 = 26.0;

fn go(msg: IdeMsg) -> Msg {
    Msg::Ide(msg)
}

// ------------------------------------------------------------------------------------------------- pieces

/// A small button with only a glyph, and what it does on hover.
pub fn icon<'a>(glyph: &'a str, tip: &'a str, msg: Option<Msg>) -> El<'a> {
    let b = button(label(glyph, 14.0, if msg.is_some() { theme::TEXT_DIM } else { theme::TEXT_FAINT }))
        .padding([3.0, 7.0])
        .style(theme::ghost_button)
        .on_press_maybe(msg);
    tooltip(b, container(label(tip, 12.0, theme::TEXT)).padding([4.0, 8.0]).style(theme::card), tooltip::Position::Bottom)
        .into()
}

/// A plain, quiet button.
pub fn ghost<'a>(words: impl text::IntoFragment<'a>, msg: Option<Msg>) -> El<'a> {
    button(text(words).size(12.5).font(fonts().ui)).padding([3.0, 8.0]).style(theme::ghost_button).on_press_maybe(msg).into()
}

/// A side bar's title, in small capitals, with its actions.
pub(super) fn section_title<'a>(title: &'a str, actions: Vec<El<'a>>) -> El<'a> {
    let mut line = Row::new().spacing(2).align_y(Alignment::Center).push(
        container(text(title.to_uppercase()).size(11).font(fonts().ui_strong).color(theme::TEXT_FAINT)).padding([0.0, 4.0]),
    );
    line = line.push(space().width(Length::Fill));
    for a in actions {
        line = line.push(a);
    }
    container(line).padding([6.0, 8.0]).width(Length::Fill).into()
}

fn rule<'a>() -> El<'a> {
    container(space().height(1)).width(Length::Fill).style(theme::line).into()
}

fn vrule<'a>() -> El<'a> {
    container(space().width(1)).height(Length::Fill).style(theme::line).into()
}

/// A border that resizes what is beside it when dragged.
fn grip<'a>(which: Splitter, dragging: bool) -> El<'a> {
    let line_color = if dragging { theme::GOLD_DIM } else { theme::LINE };
    let (bar, cursor): (El<'a>, mouse::Interaction) = if which == Splitter::Panel {
        (
            container(container(space().height(1)).width(Length::Fill).style(move |_| solid(line_color)))
                .height(GRIP)
                .width(Length::Fill)
                .center_y(GRIP)
                .into(),
            mouse::Interaction::ResizingVertically,
        )
    } else {
        (
            container(container(space().width(1)).height(Length::Fill).style(move |_| solid(line_color)))
                .width(GRIP)
                .height(Length::Fill)
                .center_x(GRIP)
                .into(),
            mouse::Interaction::ResizingHorizontally,
        )
    };
    mouse_area(bar).on_press(go(IdeMsg::DragStart(which))).interaction(cursor).into()
}

fn solid(color: Color) -> container::Style {
    container::Style { background: Some(color.into()), ..container::Style::default() }
}

fn bar_style(_: &iced::Theme) -> container::Style {
    solid(theme::SIDEBAR)
}

fn canvas(_: &iced::Theme) -> container::Style {
    solid(theme::CANVAS)
}

/// The colour a file's dot takes, by its language: enough to tell kinds apart at a glance.
pub fn lang_color(lang: Lang) -> Color {
    match lang {
        Lang::Rust => Color::from_rgb8(0xde, 0xa5, 0x84),
        Lang::Python => Color::from_rgb8(0x8f, 0xb4, 0xe0),
        Lang::Js => Color::from_rgb8(0xe8, 0xd4, 0x6a),
        Lang::C | Lang::CSharp | Lang::Go | Lang::Java => Color::from_rgb8(0x9f, 0xb4, 0xd9),
        Lang::Shader => Color::from_rgb8(0xc9, 0xa8, 0xdc),
        Lang::Json | Lang::Toml | Lang::Yaml | Lang::Ini => Color::from_rgb8(0xd9, 0xa0, 0x7a),
        Lang::Markdown => theme::TEXT,
        Lang::Shell | Lang::PowerShell | Lang::Batch => theme::POSITIVE,
        Lang::Sql => Color::from_rgb8(0xe3, 0xa6, 0xa6),
        Lang::Html | Lang::Css => Color::from_rgb8(0xe3, 0xa6, 0xa6),
        Lang::Plain => theme::TEXT_FAINT,
    }
}

pub(super) fn file_dot<'a>(path: &str) -> El<'a> {
    let color = lang_color(Lang::of(path));
    container(space().width(7).height(7)).style(theme::dot(color)).into()
}

// ------------------------------------------------------------------------------------------------- the frame

/// The whole IDE (the Lattice page's Chat tab).
pub fn view<'a>(state: &'a State, phase: f32) -> El<'a> {
    let body = responsive(move |size| frame(state, phase, size.width));
    let page: El<'a> = column![container(body).height(Length::Fill), status_bar(state, phase)].into();
    let mut layers: Vec<El<'a>> = vec![page];
    if let Some(q) = &state.ide.quick {
        layers.push(quick_open(state, q));
    }
    if let Some(typed) = &state.ide.goto {
        layers.push(goto_box(state, typed));
    }
    if let Some((confirm, _)) = &state.ide.confirm {
        layers.push(opaque(center(confirm_card(state, confirm)).style(theme::scrim)).into());
    }
    if layers.len() == 1 { layers.pop().expect("one layer") } else { stack(layers).into() }
}

fn frame<'a>(state: &'a State, phase: f32, width: f32) -> El<'a> {
    let ide = &state.ide;
    let dragging = ide.drag.map(|(w, _)| w);
    // The agent panel keeps its width; the side bar folds away when the editor would get too narrow.
    let agent_w = ide.agent_width.min((width - ACTIVITY - MIN_EDITOR).max(super::AGENT_RANGE.0));
    let side_fits = width - ACTIVITY - agent_w - ide.side_width - 3.0 * GRIP >= MIN_EDITOR;
    let side_open = ide.side_open && side_fits;
    let mut line = Row::new().height(Length::Fill).push(activity_bar(state));
    if side_open {
        line = line.push(container(side_bar(state, phase)).width(ide.side_width).height(Length::Fill).style(bar_style));
        line = line.push(grip(Splitter::Side, dragging == Some(Splitter::Side)));
    } else {
        line = line.push(vrule());
    }
    line = line.push(center_column(state, phase));
    line = line.push(grip(Splitter::Agent, dragging == Some(Splitter::Agent)));
    line = line.push(container(super::agent::panel(state, phase)).width(agent_w).height(Length::Fill).style(|_| solid(theme::SURFACE)));
    line.into()
}

fn activity_bar<'a>(state: &'a State) -> El<'a> {
    let ide = &state.ide;
    let mut col = Column::new().spacing(4).align_x(Alignment::Center).padding([8.0, 0.0]);
    for side in Side::ALL {
        let on = ide.side_open && ide.side == side;
        let mut glyph = Row::new().align_y(Alignment::Center).push(label(side.glyph(), 18.0, if on { theme::GOLD } else { theme::TEXT_DIM }));
        // A mark on Changes when the agent's changes wait, and on Chats when a chat waits for the reader.
        let waiting = match side {
            Side::Changes => state.changes.as_ref().is_some_and(|c| c.waiting > 0),
            Side::Chats => state.list.as_ref().is_some_and(|l| l.conversations.iter().any(|c| c.needs_you)) || !state.calling.is_empty(),
            // A mark on Tools when an MCP server stopped with an error.
            Side::Tools => state.ide.tools.overview.as_ref().is_some_and(|o| {
                o.servers.iter().any(|s| matches!(s.status, lattice_core::mcp::ServerStatus::Failed(_)))
            }),
            Side::Explorer | Side::Search => false,
        };
        if waiting {
            glyph = glyph.push(container(space().width(6).height(6)).style(theme::dot(theme::GOLD)));
        }
        let b = button(container(glyph).center_x(Length::Fill))
            .width(ACTIVITY - 8.0)
            .padding([8.0, 0.0])
            .style(theme::rail_button(on))
            .on_press(go(IdeMsg::Side(side)));
        col = col.push(tooltip(
            b,
            container(label(side.title(), 12.0, theme::TEXT)).padding([4.0, 8.0]).style(theme::card),
            tooltip::Position::Right,
        ));
    }
    col = col.push(space().height(Length::Fill));
    let quick = button(container(label("\u{21E5}", 18.0, theme::TEXT_DIM)).center_x(Length::Fill))
        .width(ACTIVITY - 8.0)
        .padding([8.0, 0.0])
        .style(theme::rail_button(false))
        .on_press(go(IdeMsg::QuickOpen));
    col = col.push(tooltip(quick, container(label("Go to file (Ctrl+P)", 12.0, theme::TEXT)).padding([4.0, 8.0]).style(theme::card), tooltip::Position::Right));
    let term = button(container(mono(">_", 13.0, if state.ide.panel_open { theme::GOLD } else { theme::TEXT_DIM })).center_x(Length::Fill))
        .width(ACTIVITY - 8.0)
        .padding([8.0, 0.0])
        .style(theme::rail_button(state.ide.panel_open))
        .on_press(go(IdeMsg::TogglePanel));
    col = col.push(tooltip(term, container(label("Commands and terminal (Ctrl+J)", 12.0, theme::TEXT)).padding([4.0, 8.0]).style(theme::card), tooltip::Position::Right));
    container(col).width(ACTIVITY).height(Length::Fill).style(bar_style).into()
}

// ------------------------------------------------------------------------------------------------- side bar

fn side_bar<'a>(state: &'a State, phase: f32) -> El<'a> {
    match state.ide.side {
        Side::Explorer => explorer(state, phase),
        Side::Search => super::search::view(state, phase),
        Side::Chats => super::agent::chats(state, phase),
        Side::Changes => changes_view(state),
        Side::Tools => super::tools::side(state),
    }
}

fn explorer<'a>(state: &'a State, phase: f32) -> El<'a> {
    let ide = &state.ide;
    let Some(folder) = &ide.folder else {
        return open_folder_form(state, phase);
    };
    let mut col = Column::new();
    col = col.push(section_title(
        "Explorer",
        vec![
            icon("+", "New file", Some(go(IdeMsg::NewFile))),
            icon("\u{27F3}", "Read the folder again", (!ide.listing_busy).then(|| go(IdeMsg::Refresh))),
            icon("\u{229F}", "Collapse folders", Some(go(IdeMsg::CollapseAll))),
        ],
    ));
    let mut head = Row::new().spacing(6).align_y(Alignment::Center).padding([0.0, 12.0]);
    head = head.push(strong(ui::cut(folder.name(), 26), 12.5, theme::TEXT));
    if let Some(listing) = ide.files() {
        head = head.push(label(format!("{} files", tree::files_in(&listing.tree)), 11.0, theme::TEXT_FAINT));
    }
    let trusted = state.folder.view.as_ref().map(|v| v.workspace.trusted).or_else(|| {
        state.conversation().and_then(|c| c.summary.workspace.as_ref().filter(|w| w.id == folder.id()).map(|w| w.trusted))
    });
    head = head.push(match trusted {
        Some(true) => ui::chip("trusted", theme::POSITIVE),
        Some(false) => ui::chip("not trusted", theme::GOLD_DIM),
        None => space().width(0).into(),
    });
    col = col.push(head);
    col = col.push(
        container(
            text_input("Filter files", &ide.filter)
                .on_input(|t| go(IdeMsg::Filter(t)))
                .padding([5.0, 8.0])
                .size(12.5)
                .style(ui::input_style),
        )
        .padding([6.0, 10.0]),
    );
    if let Some(name) = &ide.new_file {
        col = col.push(
            container(
                text_input("new file, e.g. src/notes.md", name)
                    .on_input(|t| go(IdeMsg::NewFileName(t)))
                    .on_submit(go(IdeMsg::CreateFile))
                    .padding([5.0, 8.0])
                    .size(12.5)
                    .style(ui::input_style),
            )
            .padding(Padding { top: 0.0, right: 10.0, bottom: 6.0, left: 10.0 }),
        );
    }
    let mut rows = Column::new().spacing(0);
    match &ide.listing {
        None => rows = rows.push(container(ui::working(phase, "Listing the folder…")).padding(12)),
        Some(Err(why)) => rows = rows.push(container(ui::notice(why.as_str(), theme::CAUTION)).padding(8)),
        Some(Ok(listing)) => {
            let active = ide.active_tab().and_then(EditorTab::path);
            let changed: Vec<(&str, &ChangeState)> =
                state.changes.as_ref().map(|s| s.changes.iter().map(|c| (c.path.as_str(), &c.state)).collect()).unwrap_or_default();
            // A folder can list tens of thousands of files: only the rows on show (and a margin) are built.
            if ide.filter.trim().is_empty() {
                let tree_rows = tree::rows(&listing.tree, &ide.expanded);
                rows = ui::virtual_rows(&vec![TREE_ROW_H; tree_rows.len()], ide.explorer_at, |i| tree_row(tree_rows[i].clone(), active, &changed));
            } else if ide.filtered.is_empty() {
                rows = rows.push(container(note("No file fits.")).padding(12));
            } else {
                rows = ui::virtual_rows(&vec![FOUND_ROW_H; ide.filtered.len()], ide.explorer_at, |i| {
                    let f = &ide.filtered[i];
                    let on = active == Some(f.path.as_str());
                    button(
                        row![file_dot(&f.path), column![label(file_name(&f.path).to_string(), 12.5, theme::TEXT), label(f.path.clone(), 11.0, theme::TEXT_FAINT)]]
                            .spacing(8)
                            .align_y(Alignment::Center),
                    )
                    .width(Length::Fill)
                    .padding([3.0, 12.0])
                    .style(theme::list_row(on))
                    .on_press(go(IdeMsg::Open(f.path.clone())))
                    .into()
                });
            }
            if listing.truncated {
                rows = rows.push(container(note("The listing was cut short: use Go to file (Ctrl+P) for the rest.")).padding(10));
            }
            for w in &listing.withheld {
                let place = if w.is_empty() { "the whole folder".to_string() } else { w.clone() };
                rows = rows.push(container(note(format!("Left out: {place} (git cannot read its ignore rules)."))).padding(10));
            }
        }
    }
    col = col.push(
        scrollable(rows)
            .on_scroll(|v| go(IdeMsg::ExplorerScrolled(ui::Scrolled::of(v))))
            .height(Length::Fill)
            .style(theme::scrollbars),
    );
    col.into()
}

/// An Explorer row's height: a 12.5 line and the button's 2 above and below.
const TREE_ROW_H: f32 = 12.5 * 1.3 + 4.0;
/// A filtered file's: its name (12.5) over its path (11) and the button's 3 above and below.
const FOUND_ROW_H: f32 = 12.5 * 1.3 + 11.0 * 1.3 + 6.0;

fn tree_row<'a>(r: tree::Row, active: Option<&str>, changed: &[(&str, &ChangeState)]) -> El<'a> {
    let indent = space().width(10.0 + f32::from(r.depth) * 14.0);
    let on = !r.is_dir && active == Some(r.path.as_str());
    let mut line = Row::new().spacing(6).align_y(Alignment::Center).push(indent);
    if r.is_dir {
        line = line.push(label(if r.open { "\u{25BE}" } else { "\u{25B8}" }, 11.0, theme::TEXT_FAINT));
        line = line.push(label(r.name.clone(), 12.5, theme::TEXT_DIM));
    } else {
        line = line.push(file_dot(&r.path));
        line = line.push(label(r.name.clone(), 12.5, if on { theme::GOLD } else { theme::TEXT }));
    }
    line = line.push(space().width(Length::Fill));
    // A file the agent's change waits on, as git's letters would mark it.
    if let Some((_, st)) = changed.iter().find(|(p, _)| *p == r.path || (r.is_dir && p.starts_with(&format!("{}/", r.path)))) {
        let (mark, color) = match st {
            ChangeState::Pending | ChangeState::Rebased => ("M", theme::GOLD),
            ChangeState::Conflict { .. } => ("!", theme::CAUTION),
            ChangeState::OnDisk => ("C", theme::CAUTION),
            _ => ("", theme::TEXT_FAINT),
        };
        if !mark.is_empty() {
            line = line.push(container(label(if r.is_dir { "\u{2022}" } else { mark }, 11.0, color)).padding([0.0, 8.0]));
        }
    }
    let msg = if r.is_dir { IdeMsg::Toggle(r.path.clone()) } else { IdeMsg::Open(r.path.clone()) };
    button(line).width(Length::Fill).padding([2.0, 0.0]).style(theme::list_row(on)).on_press(go(msg)).into()
}

/// No folder yet: the path box, and the folders the chats worked in.
fn open_folder_form<'a>(state: &'a State, phase: f32) -> El<'a> {
    let ide = &state.ide;
    let mut col = Column::new().spacing(10);
    col = col.push(section_title("Explorer", vec![]));
    let mut form = Column::new().spacing(8).padding([0.0, 12.0]);
    form = form.push(label("No folder is open.", 13.0, theme::TEXT));
    form = form.push(note("Open the folder to work in: the editor shows it and the agent works in it."));
    if ide.opening {
        form = form.push(ui::working(phase, "Opening the folder…"));
    } else if ide.picking {
        form = form.push(ui::working(phase, "Choose a folder in Windows' picker…"));
    } else {
        form = form.push(ui::primary("Open folder…", state.services_ready().then(|| go(IdeMsg::Browse))));
    }
    form = form.push(note("Or type its path (Lattice asks you to confirm a typed path):"));
    form = form.push(
        row![
            text_input("D:\\Projects\\demo", &ide.folder_path)
                .on_input(|t| go(IdeMsg::FolderPath(t)))
                .on_submit(go(IdeMsg::OpenFolder))
                .padding([6.0, 8.0])
                .size(12.5)
                .style(ui::input_style),
            ui::secondary("Open", (!ide.folder_path.trim().is_empty() && state.services_ready() && !ide.opening).then(|| go(IdeMsg::OpenFolder))),
        ]
        .spacing(6)
        .align_y(Alignment::Center),
    );
    let recent = recent_folders(state);
    if !recent.is_empty() {
        form = form.push(space().height(6));
        form = form.push(text("RECENT").size(11).font(fonts().ui_strong).color(theme::TEXT_FAINT));
        for (name, path) in recent {
            form = form.push(
                button(column![label(name, 12.5, theme::TEXT), label(ui::cut(&path, 40), 11.0, theme::TEXT_FAINT)].spacing(1))
                    .width(Length::Fill)
                    .padding([4.0, 8.0])
                    .style(theme::list_row(false))
                    .on_press(go(IdeMsg::OpenRecent(path))),
            );
        }
    }
    col = col.push(form);
    col.into()
}

/// The folders the chats worked in, newest first, once each.
pub fn recent_folders(state: &State) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    if let Some(list) = &state.list {
        for c in &list.conversations {
            if let Some(w) = &c.workspace
                && !out.iter().any(|(_, p)| p.eq_ignore_ascii_case(&w.path))
            {
                out.push((w.name.clone(), w.path.clone()));
            }
        }
    }
    out.truncate(6);
    out
}

fn changes_view<'a>(state: &'a State) -> El<'a> {
    let ide = &state.ide;
    let mut col = Column::new();
    let waiting = state.changes.as_ref().map(|s| s.waiting).unwrap_or(0);
    let free = !state.reviewing && !state.changes.as_ref().is_some_and(|s| s.command_running);
    col = col.push(section_title(
        "Changes",
        vec![
            icon("\u{27F3}", "Read again", Some(go(IdeMsg::ReadHistory))),
            icon("\u{21B6}", "Undo all", (waiting > 0 && !state.reviewing).then_some(Msg::Review(ReviewOp::UndoAll { note: None }))),
            icon("\u{2713}", "Keep all (not authority files)", (waiting > 0 && free).then_some(Msg::Review(ReviewOp::KeepAll))),
        ],
    ));
    let mut body = Column::new().spacing(2);
    match &state.changes {
        None if state.conversation().is_none() => body = body.push(container(note("Open a chat to see what its agent proposed.")).padding(12)),
        None => body = body.push(container(note("No changes.")).padding(12)),
        Some(set) => {
            if set.changes.is_empty() {
                body = body.push(container(note("The agent has proposed no change. Nothing is written to the folder until you keep it.")).padding(12));
            }
            for change in &set.changes {
                let (letter, color) = match change.kind {
                    ChangeKind::Create => ("A", theme::POSITIVE),
                    ChangeKind::Delete => ("D", theme::DANGER),
                    ChangeKind::Restore => ("R", theme::GOLD),
                    ChangeKind::CommandUndo => ("U", theme::GOLD),
                    ChangeKind::Edit | ChangeKind::Overwrite => ("M", theme::GOLD),
                };
                let (state_words, state_color) = change_state_words(&change.state);
                let selected = state.diff.as_ref().is_some_and(|d| d.change == change.id);
                let line = row![
                    container(mono(letter, 11.5, color)).width(14),
                    column![
                        label(file_name(&change.path).to_string(), 12.5, theme::TEXT),
                        label(ui::cut(&change.path, 40), 11.0, theme::TEXT_FAINT),
                    ]
                    .spacing(1)
                    .width(Length::Fill),
                    column![
                        rich_text::<(), _, _, _>([
                            span(format!("+{}", change.added)).color(theme::POSITIVE).font(fonts().mono).size(11.0),
                            span(" "),
                            span(format!("\u{2212}{}", change.removed)).color(theme::DANGER).font(fonts().mono).size(11.0),
                        ]),
                        label(state_words, 10.5, state_color),
                    ]
                    .spacing(1)
                    .align_x(Alignment::End),
                ]
                .spacing(6)
                .align_y(Alignment::Center);
                body = body.push(button(line).width(Length::Fill).padding([4.0, 10.0]).style(theme::list_row(selected)).on_press(Msg::ShowDiff(change.id.clone())));
            }
            if set.changes.iter().any(|c| c.authority) {
                body = body.push(container(note("Keep all leaves authority files out: keep each on its own.")).padding([4.0, 12.0]));
            }
            if set.command_running {
                body = body.push(container(note("A command is running: keeping waits for it.")).padding([4.0, 12.0]));
            }
        }
    }
    // Checkpoints: a restore is staged as changes, never written by itself.
    body = body.push(space().height(8));
    body = body.push(section_title("Checkpoints", vec![]));
    match &ide.checkpoints {
        None => body = body.push(container(note("Read when the chat has a folder.")).padding([2.0, 12.0])),
        Some(Err(why)) => body = body.push(container(note(why.as_str())).padding([2.0, 12.0])),
        Some(Ok(points)) if points.is_empty() => body = body.push(container(note("None yet: one is taken before each keep and around each command.")).padding([2.0, 12.0])),
        Some(Ok(points)) => {
            for p in points.iter().rev() {
                let why = match &p.reason {
                    lattice_protocol::conversation::CheckpointReason::BeforeKeep => "before a keep",
                    lattice_protocol::conversation::CheckpointReason::BeforeCommand { .. } => "before a command",
                    lattice_protocol::conversation::CheckpointReason::AfterCommand { .. } => "after a command",
                    lattice_protocol::conversation::CheckpointReason::BeforeRestoreKeep => "before a restore",
                };
                let mut sub = format!("#{} {why}", p.id);
                if p.exposed {
                    sub.push_str(" · incomplete (a command ran unseen)");
                }
                body = body.push(
                    row![
                        label(sub, 12.0, theme::TEXT_DIM).width(Length::Fill),
                        ghost("Restore…", Some(go(IdeMsg::Restore(p.id)))),
                    ]
                    .padding([1.0, 10.0])
                    .align_y(Alignment::Center),
                );
            }
            body = body.push(container(note("Restore stages the folder as it was at that point, as changes to review.")).padding([2.0, 12.0]));
        }
    }
    body = body.push(space().height(8));
    body = body.push(section_title("Standing approvals", vec![]));
    match &ide.permissions {
        None => body = body.push(container(note("Read when a folder is attached.")).padding([2.0, 12.0])),
        Some(Err(why)) => body = body.push(container(note(why.as_str())).padding([2.0, 12.0])),
        Some(Ok(entries)) if entries.is_empty() => body = body.push(container(note("None: every command asks.")).padding([2.0, 12.0])),
        Some(Ok(entries)) => {
            for e in entries {
                let cmd = e.argv.join(" ");
                let revoked = e.revoked_at.is_some();
                let mut line = Row::new().spacing(6).align_y(Alignment::Center).padding([1.0, 10.0]);
                line = line.push(column![mono(ui::cut(&cmd, 38), 11.5, if revoked { theme::TEXT_FAINT } else { theme::TEXT }), label(if e.cwd.is_empty() { "in the folder's top".to_string() } else { format!("in {}", e.cwd) }, 10.5, theme::TEXT_FAINT)].width(Length::Fill));
                line = line.push(if revoked { El::from(label("revoked", 11.0, theme::TEXT_FAINT)) } else { ghost("Revoke", Some(go(IdeMsg::Revoke(e.id.clone())))) });
                body = body.push(line);
            }
        }
    }
    col = col.push(scrollable(body).height(Length::Fill).style(theme::scrollbars));
    col.into()
}

pub fn change_state_words(state: &ChangeState) -> (&'static str, Color) {
    match state {
        ChangeState::Pending => ("waiting", theme::GOLD),
        ChangeState::Rebased => ("re-applied, waiting", theme::GOLD),
        ChangeState::Conflict { .. } => ("conflict", theme::CAUTION),
        ChangeState::Kept => ("kept", theme::POSITIVE),
        ChangeState::Undone => ("undone", theme::TEXT_FAINT),
        ChangeState::PartlyKept => ("partly kept", theme::GOLD_DIM),
        ChangeState::OnDisk => ("a command wrote it", theme::CAUTION),
    }
}

// ------------------------------------------------------------------------------------------------- the editor

fn center_column<'a>(state: &'a State, phase: f32) -> El<'a> {
    let ide = &state.ide;
    let mut col = Column::new().width(Length::Fill).height(Length::Fill);
    col = col.push(tabs_bar(state));
    col = col.push(container(editor_area(state, phase)).height(Length::Fill).width(Length::Fill).style(canvas));
    if ide.panel_open {
        col = col.push(grip(Splitter::Panel, ide.drag.map(|(w, _)| w) == Some(Splitter::Panel)));
        col = col.push(container(bottom_panel(state, phase)).height(ide.panel_height).width(Length::Fill).style(bar_style));
    }
    col.into()
}

fn tabs_bar<'a>(state: &'a State) -> El<'a> {
    let ide = &state.ide;
    let mut tabs = Row::new().spacing(0).align_y(Alignment::End);
    for t in &ide.tabs {
        let on = ide.active == Some(t.id);
        let mut inner = Row::new().spacing(6).align_y(Alignment::Center);
        inner = inner.push(match &t.kind {
            TabKind::File { path, .. } => file_dot(path),
            TabKind::Diff { .. } => label("\u{00B1}", 12.0, theme::GOLD).into(),
            TabKind::Output { .. } => mono(">_", 10.5, theme::POSITIVE).into(),
            TabKind::Tools => label(Side::Tools.glyph(), 12.0, theme::GOLD).into(),
            TabKind::Connections => label("\u{21C4}", 12.0, theme::GOLD).into(),
            TabKind::Project { .. } => label("\u{25A3}", 12.0, theme::GOLD).into(),
            TabKind::Artifact { view, .. } => {
                let glyph = match view {
                    Some(Ok(v)) => super::agent::kind_glyph(v.kind),
                    _ => "\u{2630}",
                };
                label(glyph, 12.0, theme::GOLD).into()
            }
        });
        inner = inner.push(label(t.title(), 12.5, if on { theme::TEXT } else { theme::TEXT_DIM }));
        let close: El<'a> = if t.dirty() {
            button(label("\u{25CF}", 10.0, theme::GOLD)).padding([0.0, 4.0]).style(theme::ghost_button).on_press(go(IdeMsg::Close(t.id))).into()
        } else {
            button(label("\u{00D7}", 13.0, theme::TEXT_FAINT)).padding([0.0, 4.0]).style(theme::ghost_button).on_press(go(IdeMsg::Close(t.id))).into()
        };
        inner = inner.push(close);
        let accent = container(space().height(2)).width(Length::Fill).style(move |_| solid(if on { theme::GOLD } else { Color::TRANSPARENT }));
        let tab = column![accent, container(inner).padding([6.0, 10.0])].width(Length::Shrink);
        tabs = tabs.push(
            button(tab)
                .padding(0)
                .style(move |_, status| {
                    let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
                    button::Style {
                        background: Some(Background::Color(if on { theme::CANVAS } else if hovered { theme::HOVER } else { theme::SIDEBAR })),
                        text_color: theme::TEXT,
                        border: Border { color: theme::LINE_SOFT, width: 0.0, radius: 0.0.into() },
                        ..button::Style::default()
                    }
                })
                .on_press(go(IdeMsg::Activate(t.id))),
        );
        tabs = tabs.push(vrule());
    }
    let strip = scrollable(tabs).direction(scrollable::Direction::Horizontal(scrollable::Scrollbar::new().width(3).scroller_width(3))).style(theme::scrollbars);
    let mut line = Row::new().align_y(Alignment::Center).push(container(strip).width(Length::Fill));
    if let Some(t) = ide.active_tab()
        && let TabKind::File { body: Body::Editing(e), .. } = &t.kind
    {
        line = line.push(icon("\u{21B6}", "Undo (Ctrl+Z)", e.history.can_undo().then(|| go(IdeMsg::Undo))));
        line = line.push(icon("\u{21B7}", "Redo (Ctrl+Y)", e.history.can_redo().then(|| go(IdeMsg::Redo))));
        line = line.push(icon("@", "Mention this file in the chat", Some(go(IdeMsg::Mention))));
        line = line.push(
            container(ui::primary(if e.saving { "Saving…" } else { "Save" }, (e.dirty && !e.saving).then(|| go(IdeMsg::Save))))
                .padding([0.0, 8.0]),
        );
    }
    column![container(line).height(36).width(Length::Fill).style(bar_style), rule()].into()
}

fn editor_area<'a>(state: &'a State, phase: f32) -> El<'a> {
    let ide = &state.ide;
    let Some(tab) = ide.active_tab() else { return welcome(state) };
    match &tab.kind {
        TabKind::File { path, lang, body, note } => {
            let mut col = Column::new().height(Length::Fill);
            col = col.push(breadcrumb(path, note.as_ref(), tab, state));
            col = col.push(match body {
                Body::Loading => container(ui::working(phase, "Reading the file…")).padding(16).into(),
                Body::Failed(why) => container(ui::notice(format!("{path}: {why}"), theme::CAUTION)).padding(16).into(),
                Body::Paged { why, lines, reading, colours } => paged(tab.id, why, lines.as_ref(), colours, *reading, phase, false),
                Body::Editing(e) => {
                    let find = ide.find.as_ref().filter(|f| f.tab == tab.id);
                    let band = ide.inline.as_ref().filter(|i| i.tab == tab.id).map(|i| (i.from, i.to));
                    let editor = code_editor(tab, *lang, e, find, band);
                    let mut layers: Vec<El<'a>> = vec![editor];
                    if let Some(f) = find {
                        layers.push(container(find_bar(f)).align_right(Length::Fill).padding(Padding { top: 6.0, right: 18.0, bottom: 0.0, left: 0.0 }).into());
                    }
                    if let Some(i) = ide.inline.as_ref().filter(|i| i.tab == tab.id) {
                        layers.push(container(inline_box(state, i)).center_x(Length::Fill).padding(Padding { top: 8.0, right: 14.0, bottom: 0.0, left: 14.0 }).into());
                    }
                    if layers.len() == 1 { layers.pop().expect("the editor") } else { stack(layers).into() }
                }
            });
            col.into()
        }
        TabKind::Diff { path, .. } => diff_view(state, path, phase),
        TabKind::Tools => super::tools::page(state, phase),
        TabKind::Connections => super::connections::page(state, phase),
        TabKind::Project { id, .. } => super::projects::page(state, id),
        TabKind::Artifact { view, rendered, source, .. } => artifact_view(tab.id, view.as_ref(), rendered.as_ref(), *source, phase),
        TabKind::Output { label: l, lines, reading, .. } => {
            let mut col = Column::new().height(Length::Fill);
            col = col.push(container(row![mono(">_", 12.0, theme::POSITIVE), label(format!("The whole output of: {l}"), 12.5, theme::TEXT_DIM)].spacing(8)).padding([6.0, 14.0]));
            col = col.push(paged(tab.id, "", lines.as_ref(), &[], *reading, phase, true));
            col.into()
        }
    }
}

/// An artifact's tab: its title, which version of how many (with the versions either side a click away), Formatted
/// or Source for Markdown, Preview for a page or a picture, Copy; then the text, formatted or as it is.
fn artifact_view<'a>(
    tab: u64,
    view: Option<&'a Result<lattice_protocol::conversation::ArtifactView, String>>,
    rendered: Option<&'a iced::widget::markdown::Content>,
    source: bool,
    phase: f32,
) -> El<'a> {
    use lattice_protocol::conversation::ArtifactKind;
    let v = match view {
        None => return container(ui::working(phase, "Reading the artifact\u{2026}")).padding(16).into(),
        Some(Err(why)) => return container(ui::notice(why.as_str(), theme::CAUTION)).padding(16).into(),
        Some(Ok(v)) => v,
    };
    let mut bar = Row::new().spacing(8).align_y(Alignment::Center);
    bar = bar.push(label(super::agent::kind_glyph(v.kind), 13.0, theme::GOLD));
    bar = bar.push(strong(v.title.as_str(), 13.5, theme::TEXT));
    bar = bar.push(ghost("\u{2039}", (v.version > 1).then(|| go(IdeMsg::ArtifactVersion(tab, v.version - 1)))));
    bar = bar.push(label(format!("version {} of {}", v.version, v.versions), 12.0, theme::TEXT_DIM));
    bar = bar.push(ghost("\u{203A}", (v.version < v.versions).then(|| go(IdeMsg::ArtifactVersion(tab, v.version + 1)))));
    bar = bar.push(space().width(Length::Fill));
    if v.kind == ArtifactKind::Markdown {
        bar = bar.push(
            button(label("Formatted", 12.0, theme::TEXT)).padding([3.0, 8.0]).style(theme::segment_button(!source)).on_press(go(IdeMsg::ArtifactSource(tab))),
        );
        bar = bar.push(
            button(label("Source", 12.0, theme::TEXT)).padding([3.0, 8.0]).style(theme::segment_button(source)).on_press(go(IdeMsg::ArtifactSource(tab))),
        );
    }
    if matches!(v.kind, ArtifactKind::Html | ArtifactKind::Svg) {
        bar = bar.push(ghost("Preview", Some(go(IdeMsg::ArtifactPreview(v.name.clone(), v.version)))));
    }
    bar = bar.push(ghost("Copy", Some(go(IdeMsg::ArtifactCopy(tab)))));
    let body: El<'a> = match rendered {
        Some(content) if !source => {
            iced::widget::markdown::view(content.items(), crate::lattice::view::settings()).map(|_| Msg::Link)
        }
        _ => mono(v.text.as_str(), CODE_SIZE, theme::TEXT).into(),
    };
    let mut col = Column::new().height(Length::Fill);
    col = col.push(container(bar).padding([6.0, 14.0]));
    col = col.push(container(space().height(1)).width(Length::Fill).style(theme::line));
    col = col.push(scrollable(container(body).padding([12.0, 18.0]).width(Length::Fill)).height(Length::Fill));
    col.into()
}

fn breadcrumb<'a>(path: &'a str, note: Option<&'a (String, bool)>, tab: &'a EditorTab, state: &'a State) -> El<'a> {
    let mut crumbs = Row::new().spacing(4).align_y(Alignment::Center);
    let parts: Vec<&str> = path.split('/').collect();
    for (i, part) in parts.iter().enumerate() {
        if i > 0 {
            crumbs = crumbs.push(label("\u{203A}", 12.0, theme::TEXT_FAINT));
        }
        crumbs = crumbs.push(label(*part, 12.0, if i + 1 == parts.len() { theme::TEXT_DIM } else { theme::TEXT_FAINT }));
    }
    let mut line = Row::new().spacing(10).align_y(Alignment::Center).push(crumbs).push(space().width(Length::Fill));
    if let TabKind::File { body: Body::Editing(e), .. } = &tab.kind {
        if e.authority {
            line = line.push(ui::chip("authority file: saving asks first", theme::CAUTION));
        }
        // The agent's change waiting on this very file.
        if let Some(change) = state.changes.as_ref().and_then(|s| s.changes.iter().find(|c| c.path == path && matches!(c.state, ChangeState::Pending | ChangeState::Rebased))) {
            line = line.push(
                button(label("the agent proposed a change here: review", 11.5, theme::GOLD))
                    .padding([2.0, 8.0])
                    .style(theme::ghost_button)
                    .on_press(Msg::ShowDiff(change.id.clone())),
            );
        }
    }
    if let Some((words, warn)) = note {
        line = line.push(label(words.as_str(), 11.5, if *warn { theme::CAUTION } else { theme::TEXT_FAINT }));
    }
    if matches!(&tab.kind, TabKind::File { .. }) {
        line = line.push(ghost("Reload", Some(go(IdeMsg::Reload(tab.id)))));
    }
    container(line).padding([4.0, 14.0]).width(Length::Fill).into()
}

/// The editor: the line numbers and the text, side by side in one scrollable, so they scroll together; the text
/// coloured by [`highlight`], the current line's number in gold.
fn code_editor<'a>(
    tab: &'a EditorTab,
    lang: Lang,
    e: &'a super::Editing,
    find: Option<&'a super::find::Find>,
    band: Option<(usize, usize)>,
) -> El<'a> {
    let id = tab.id;
    // The numbers before the cursor's line, its own in gold, and those after, cut from the cached gutter text.
    let (numbers, starts) = &e.gutter;
    let n = starts.len().saturating_sub(1).max(1);
    let current = e.content.cursor().position.line.min(n - 1);
    let (from, to) = (starts[current], starts[current + 1]);
    let gutter = rich_text::<(), _, _, _>([
        span(&numbers[..from]).color(theme::TEXT_FAINT),
        span(&numbers[from..to]).color(theme::GOLD),
        span(&numbers[to..]).color(theme::TEXT_FAINT),
    ])
    .font(fonts().mono)
    .size(CODE_SIZE)
    .line_height(text::LineHeight::Absolute(LINE_HEIGHT.into()));
    let gutter = container(gutter).padding(Padding { top: CODE_PAD, bottom: CODE_PAD, left: 10.0, right: 12.0 });
    let width = (e.longest as f32 * CODE_SIZE * 0.62 + 2.0 * CODE_PAD + 40.0).max(400.0);
    let editor = text_editor(&e.content)
        .id(super::editor_id(id))
        .on_action(move |a| go(IdeMsg::Edit(id, a)))
        .highlight_with::<highlight::Highlighter>(lang, highlight::format)
        .font(fonts().mono)
        .size(CODE_SIZE)
        .line_height(text::LineHeight::Absolute(LINE_HEIGHT.into()))
        .wrapping(text::Wrapping::None)
        .padding(CODE_PAD)
        .width(width)
        .key_binding(move |press| code_keys(press))
        // Clear, so the find bar's matches drawn beneath show through (the editor area is the canvas).
        .style(|_, status| text_editor::Style {
            background: Background::Color(Color::TRANSPARENT),
            border: Border::default(),
            placeholder: theme::TEXT_FAINT,
            value: theme::TEXT,
            selection: theme::with_alpha(theme::GOLD, if matches!(status, text_editor::Status::Focused { .. }) { 0.28 } else { 0.16 }),
        });
    let hits = find.map_or(&[][..], |f| &f.hits[..]);
    let content: El<'a> = if hits.is_empty() && band.is_none() {
        row![gutter, editor].into()
    } else {
        row![gutter, stack![editor].push_under(super::find::marks(hits, find.and_then(|f| f.current), band))].into()
    };
    scrollable(content)
        .id(tab.scroll_id())
        .direction(scrollable::Direction::Both { vertical: scrollable::Scrollbar::new(), horizontal: scrollable::Scrollbar::new() })
        .on_scroll(move |v| go(IdeMsg::Scrolled(id, v)))
        .width(Length::Fill)
        .height(Length::Fill)
        .style(theme::scrollbars)
        .into()
}

/// The editor's own keys: Ctrl+S saves, Ctrl+Z undoes, Ctrl+Y and Ctrl+Shift+Z redo, Tab and Shift+Tab indent;
/// Ctrl+F finds, Ctrl+H replaces, Ctrl+G goes to a line, Ctrl+K asks the agent to edit the selected lines,
/// Ctrl+Shift+F searches the folder, F3 and Shift+F3 step through the matches, and Esc closes what is open over it;
/// Enter keeps the indentation, Ctrl+/ comments lines out and in, Alt+Up and Alt+Down move them, Shift+Alt+Up and
/// Shift+Alt+Down copy them, Ctrl+Shift+K deletes them, and Home goes to the first character that is not a space.
fn code_keys(press: text_editor::KeyPress) -> Option<text_editor::Binding<Msg>> {
    use iced::keyboard::Key;
    use iced::keyboard::key::Named;
    use text_editor::Binding;
    if !matches!(press.status, text_editor::Status::Focused { .. }) {
        return None;
    }
    let m = press.modifiers;
    if m.command() {
        match press.key.to_latin(press.physical_key) {
            Some('s') => return Some(Binding::Custom(go(IdeMsg::Save))),
            Some('z') if !m.shift() => return Some(Binding::Custom(go(IdeMsg::Undo))),
            Some('y') | Some('z') => return Some(Binding::Custom(go(IdeMsg::Redo))),
            Some('f') if m.shift() => return Some(Binding::Custom(go(IdeMsg::SearchShow))),
            Some('f') => return Some(Binding::Custom(go(IdeMsg::FindOpen(false)))),
            Some('h') => return Some(Binding::Custom(go(IdeMsg::FindOpen(true)))),
            Some('g') => return Some(Binding::Custom(go(IdeMsg::GotoOpen))),
            Some('k') if m.shift() => return Some(Binding::Custom(go(IdeMsg::DeleteLines))),
            Some('k') => return Some(Binding::Custom(go(IdeMsg::InlineOpen))),
            Some('/') => return Some(Binding::Custom(go(IdeMsg::ToggleComment))),
            _ => {}
        }
    }
    match press.key.as_ref() {
        Key::Named(Named::ArrowUp) | Key::Named(Named::ArrowDown) if m.alt() && !m.command() => {
            let up = press.key.as_ref() == Key::Named(Named::ArrowUp);
            return Some(Binding::Custom(go(if m.shift() { IdeMsg::CopyLines(up) } else { IdeMsg::MoveLines(up) })));
        }
        Key::Named(Named::Enter) if !m.command() && !m.alt() => return Some(Binding::Custom(go(IdeMsg::Newline))),
        Key::Named(Named::Home) if !m.command() && !m.alt() => return Some(Binding::Custom(go(IdeMsg::Home(m.shift())))),
        Key::Named(Named::Tab) => return Some(Binding::Custom(go(IdeMsg::Indent(!m.shift())))),
        Key::Named(Named::F3) => return Some(Binding::Custom(go(IdeMsg::FindStep(!m.shift())))),
        Key::Named(Named::Escape) => {
            return Some(Binding::Sequence(vec![Binding::Custom(go(IdeMsg::EditorEscape)), Binding::Unfocus]));
        }
        _ => {}
    }
    Binding::from_key_press(press)
}

/// The find bar (VS Code's place: the editor's top right): what to find, its options, how many there are and which,
/// the steps, and with Ctrl+H the replace row.
fn find_bar<'a>(f: &'a super::find::Find) -> El<'a> {
    let status = f.note.clone().unwrap_or_else(|| f.status());
    let warn = f.error.is_some();
    let some = !f.hits.is_empty();
    let find_row = row![
        icon(if f.replace.is_some() { "\u{25BE}" } else { "\u{25B8}" }, "Replace (Ctrl+H)", Some(go(IdeMsg::FindOpen(f.replace.is_none())))),
        text_input("Find", &f.query.text)
            .id(super::find_id())
            .on_input(|t| go(IdeMsg::FindQuery(t)))
            .on_submit(go(IdeMsg::FindStep(true)))
            .padding([5.0, 8.0])
            .size(13)
            .width(240)
            .style(ui::input_style),
        super::search::toggles(&f.query, |t| go(IdeMsg::FindToggle(t))),
        container(label(status, 11.5, if warn { theme::CAUTION } else { theme::TEXT_DIM })).width(92).padding([0.0, 4.0]),
        icon("\u{2191}", "Previous match (Shift+F3)", some.then(|| go(IdeMsg::FindStep(false)))),
        icon("\u{2193}", "Next match (Enter, F3)", some.then(|| go(IdeMsg::FindStep(true)))),
        icon("\u{00D7}", "Close (Esc)", Some(go(IdeMsg::FindClose))),
    ]
    .spacing(2)
    .align_y(Alignment::Center);
    let mut col = Column::new().spacing(4).push(find_row);
    if let Some(with) = &f.replace {
        col = col.push(
            row![
                space().width(26),
                text_input("Replace", with)
                    .id(super::replace_id())
                    .on_input(|t| go(IdeMsg::FindReplaceText(t)))
                    .on_submit(go(IdeMsg::ReplaceOne))
                    .padding([5.0, 8.0])
                    .size(13)
                    .width(240)
                    .style(ui::input_style),
                ghost("Replace", some.then(|| go(IdeMsg::ReplaceOne))),
                ghost("Replace all", some.then(|| go(IdeMsg::ReplaceAll))),
            ]
            .spacing(4)
            .align_y(Alignment::Center),
        );
    }
    container(col).padding(6).style(theme::card).into()
}

/// The inline edit's box (Ctrl+K), over the editor: the lines it is about, what to do, and that the agent's change
/// waits for review.
fn inline_box<'a>(state: &'a State, i: &'a super::find::Inline) -> El<'a> {
    let lines = if i.from == i.to { format!("line {}", i.from + 1) } else { format!("lines {}-{}", i.from + 1, i.to + 1) };
    let ready = !i.prompt.trim().is_empty() && !state.sending;
    let prompt = text_input("What should change? (Enter sends, Esc closes)", &i.prompt)
        .id(super::inline_id())
        .on_input(|t| go(IdeMsg::InlinePrompt(t)))
        .on_submit(go(IdeMsg::InlineSend))
        .padding([7.0, 10.0])
        .size(13)
        .style(ui::input_style);
    container(
        column![
            row![
                label("\u{25C7}", 13.0, theme::GOLD),
                label(format!("Edit {lines} of {} with the agent", file_name(&i.path)), 12.5, theme::TEXT),
                space().width(Length::Fill),
                icon("\u{00D7}", "Close (Esc)", Some(go(IdeMsg::InlineClose))),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
            row![prompt, ui::secondary("Send", ready.then(|| go(IdeMsg::InlineSend)))].spacing(8).align_y(Alignment::Center),
            note("The agent proposes the change in Agent mode; nothing is written before you keep it in the review."),
        ]
        .spacing(8),
    )
    .padding(12)
    .width(Length::Fill)
    .max_width(620)
    .style(theme::card)
    .into()
}

/// Go to line's box (Ctrl+G), pinned near the top as Go to file's is.
fn goto_box<'a>(state: &'a State, typed: &'a str) -> El<'a> {
    let lines = state.ide.active_tab().and_then(|t| match &t.kind {
        TabKind::File { body: Body::Editing(e), .. } => Some(e.lines),
        _ => None,
    });
    let hint = match lines {
        Some(n) => format!("Go to line 1 to {n} (line:column for a column too)"),
        None => "Go to line".to_string(),
    };
    let card = container(
        column![
            text_input(&hint, typed)
                .id(super::goto_id())
                .on_input(|t| go(IdeMsg::GotoText(t)))
                .on_submit(go(IdeMsg::GotoGo))
                .padding([8.0, 10.0])
                .size(14)
                .style(ui::input_style),
            note("Enter goes there, Esc closes."),
        ]
        .spacing(8),
    )
    .padding(10)
    .width(460)
    .style(theme::card);
    let backdrop = mouse_area(container(space()).width(Length::Fill).height(Length::Fill)).on_press(go(IdeMsg::GotoClose));
    stack![backdrop, container(card).center_x(Length::Fill).padding(padding::top(70))].into()
}

/// Lines read a page at a time (a file shown, not edited; a command's output), coloured, with their numbers.
fn paged<'a>(
    id: u64,
    why: &'a str,
    lines: Option<&'a Lines>,
    colours: &'a [highlight::Spans],
    reading: bool,
    phase: f32,
    output: bool,
) -> El<'a> {
    let mut col = Column::new().spacing(6).height(Length::Fill);
    let mut top = Row::new().spacing(8).align_y(Alignment::Center).padding([0.0, 14.0]);
    if !why.is_empty() {
        top = top.push(label(why, 12.0, theme::CAUTION));
    }
    top = top.push(space().width(Length::Fill));
    if let Some(l) = lines {
        let last = (l.from + l.lines.len() as u32).saturating_sub(1).max(l.from);
        top = top.push(label(format!("lines {}–{} of {}", l.from, last, l.total), 11.5, theme::TEXT_DIM));
        let page = |from: u32| if output { IdeMsg::OutputPage(id, from) } else { IdeMsg::Page(id, from) };
        top = top.push(ghost("Earlier", (l.from > 1).then(|| go(page(l.from.saturating_sub(PAGE_LINES).max(1))))));
        top = top.push(ghost("Later", (last < l.total).then(|| go(page(last + 1)))));
    }
    col = col.push(top);
    match lines {
        None if reading => col = col.push(container(ui::working(phase, "Reading…")).padding(16)),
        None => {}
        Some(l) => {
            let digits = (l.from as usize + l.lines.len()).to_string().len().max(3);
            let none = highlight::Spans::new();
            let mut body = Column::new().spacing(0).padding([0.0, 8.0]);
            for (i, line) in l.lines.iter().enumerate() {
                let spans = colours.get(i).unwrap_or(&none);
                body = body.push(
                    row![
                        container(mono(format!("{:>digits$}", l.from as usize + i), CODE_SIZE, theme::TEXT_FAINT)).padding(padding::right(12)),
                        coloured(line, spans, None, false),
                    ]
                    .height(LINE_HEIGHT),
                );
            }
            col = col.push(
                scrollable(body)
                    .direction(scrollable::Direction::Both { vertical: scrollable::Scrollbar::new(), horizontal: scrollable::Scrollbar::new() })
                    .height(Length::Fill)
                    .width(Length::Fill)
                    .style(theme::scrollbars),
            );
            if l.truncated {
                col = col.push(container(note("A line was cut at its length cap.")).padding([0.0, 14.0]));
            }
        }
    }
    col.into()
}

/// One line of code in its colours (`base` for the pieces no lexer coloured); `wrap`: a long line breaks onto the next
/// rows instead of running on to the right.
fn coloured<'a>(line: &str, spans: &highlight::Spans, base: Option<Color>, wrap: bool) -> El<'a> {
    let base = base.unwrap_or(theme::TEXT);
    let mut pieces: Vec<text::Span<'a, (), Font>> = Vec::new();
    let mut at = 0;
    for (range, kind) in spans {
        if range.start > at {
            pieces.push(span(line[at..range.start].to_string()).color(base));
        }
        pieces.push(span(line[range.clone()].to_string()).color(highlight::color(*kind)));
        at = range.end;
    }
    if at < line.len() {
        pieces.push(span(line[at..].to_string()).color(base));
    }
    if pieces.is_empty() {
        pieces.push(span(" ".to_string()));
    }
    rich_text(pieces)
        .font(fonts().mono)
        .size(CODE_SIZE)
        .line_height(text::LineHeight::Absolute(LINE_HEIGHT.into()))
        .wrapping(if wrap { text::Wrapping::WordOrGlyph } else { text::Wrapping::None })
        .into()
}

/// An agent's change to a file, to review: its hunks, each kept or undone on its own, or the whole file.
fn diff_view<'a>(state: &'a State, path: &'a str, phase: f32) -> El<'a> {
    let Some(diff) = state.diff.as_ref().filter(|d| d.path == path) else {
        return container(ui::working(phase, "Reading the change…")).padding(16).into();
    };
    let busy = state.reviewing;
    let mut col = Column::new().spacing(10).padding(Padding { top: 10.0, bottom: 16.0, left: 14.0, right: 14.0 });
    let mut top = Row::new().spacing(8).align_y(Alignment::Center);
    top = top.push(label("\u{00B1}", 15.0, theme::GOLD));
    top = top.push(mono(path, 13.0, theme::TEXT));
    if let Some(change) = state.changes.as_ref().and_then(|s| s.changes.iter().find(|c| c.id == diff.change)) {
        let (words, color) = change_state_words(&change.state);
        top = top.push(ui::chip(words, color));
        if change.authority {
            top = top.push(ui::chip("authority file", theme::CAUTION));
        }
    }
    col = col.push(top.wrap());
    let mut acts = Row::new().spacing(8).align_y(Alignment::Center);
    acts = acts.push(ui::primary("Keep file", (!busy).then(|| Msg::Review(ReviewOp::Keep { change: diff.change.clone(), hunks: None }))));
    acts = acts.push(ui::secondary("Undo file", (!busy).then(|| Msg::Review(ReviewOp::Undo { change: diff.change.clone(), hunks: None, note: None }))));
    acts = acts.push(ghost("Open file", Some(go(IdeMsg::Open(path.to_string())))));
    col = col.push(acts);
    col = col.push(note("Nothing is written to the folder until you keep it; an authority file asks once more."));
    if diff.binary {
        col = col.push(note("A binary file: its bytes are not shown."));
    }
    let none: Vec<highlight::Spans> = Vec::new();
    for (n, hunk) in diff.hunks.iter().enumerate() {
        let mut h = Column::new().spacing(0);
        let mut head = Row::new().spacing(8).align_y(Alignment::Center).padding([4.0, 8.0]);
        // The hunk's place and section, cut at the width so its buttons always show.
        let place = format!("@@ \u{2212}{},{} +{},{} @@ {}", hunk.old_start, hunk.old_lines, hunk.new_start, hunk.new_lines, hunk.section);
        head = head.push(container(mono(place, 11.5, theme::TEXT_FAINT).wrapping(text::Wrapping::None)).width(Length::Fill).clip(true));
        match hunk.state {
            HunkState::Pending => {
                head = head.push(ghost("Undo", (!busy).then(|| Msg::Review(ReviewOp::Undo { change: diff.change.clone(), hunks: Some(vec![hunk.id.clone()]), note: None }))));
                head = head.push(ghost("Keep", (!busy).then(|| Msg::Review(ReviewOp::Keep { change: diff.change.clone(), hunks: Some(vec![hunk.id.clone()]) }))));
            }
            HunkState::Kept => head = head.push(label("kept", 11.5, theme::POSITIVE)),
            HunkState::Undone => head = head.push(label("undone", 11.5, theme::TEXT_FAINT)),
        }
        h = h.push(container(head).width(Length::Fill).style(|_| solid(theme::RAISED)));
        // Each line coloured as code (once, when the diff arrived), on its add or remove tint.
        let colours = state.diff_colours.get(n).unwrap_or(&none);
        let plain = highlight::Spans::new();
        let mut old_no = hunk.old_start;
        let mut new_no = hunk.new_start;
        for (i, line) in hunk.lines.iter().enumerate() {
            let spans = colours.get(i).unwrap_or(&plain);
            let (sign, tint, a, b) = match line.kind {
                DiffLineKind::Context => {
                    let r = (" ", None, Some(old_no), Some(new_no));
                    old_no += 1;
                    new_no += 1;
                    r
                }
                DiffLineKind::Add => {
                    let r = ("+", Some(theme::with_alpha(theme::POSITIVE, 0.13)), None, Some(new_no));
                    new_no += 1;
                    r
                }
                DiffLineKind::Remove => {
                    let r = ("\u{2212}", Some(theme::with_alpha(theme::DANGER, 0.13)), Some(old_no), None);
                    old_no += 1;
                    r
                }
            };
            let number = |n: Option<u32>| container(mono(n.map(|n| n.to_string()).unwrap_or_default(), 11.0, theme::TEXT_FAINT)).width(40).align_right(40);
            let sign_color = match line.kind {
                DiffLineKind::Add => theme::POSITIVE,
                DiffLineKind::Remove => theme::DANGER,
                DiffLineKind::Context => theme::TEXT_FAINT,
            };
            let mut c = container(
                row![
                    number(a),
                    number(b),
                    container(mono(sign, CODE_SIZE, sign_color)).width(18).center_x(18),
                    container(coloured(&line.text, spans, None, true)).width(Length::Fill),
                ]
                .spacing(4)
                .align_y(Alignment::Start),
            )
            .width(Length::Fill);
            if let Some(tint) = tint {
                c = c.style(move |_| solid(tint));
            }
            h = h.push(c);
        }
        col = col.push(container(h).width(Length::Fill).style(theme::well).clip(true));
    }
    if diff.truncated {
        col = col.push(note("The change is too long to show whole: it can be kept or undone only whole."));
    }
    scrollable(col.width(Length::Fill)).height(Length::Fill).width(Length::Fill).style(theme::scrollbars).into()
}

/// No tab open: what the IDE does, and where to start.
fn welcome<'a>(state: &'a State) -> El<'a> {
    let mut col = Column::new().spacing(14).max_width(560).align_x(Alignment::Center);
    col = col.push(label("\u{25C7}", 44.0, theme::GOLD));
    col = col.push(strong("Lattice", 22.0, theme::TEXT));
    col = col.push(label(
        "Chat with a model about your code. It reads the folder you open, proposes changes you review before anything \
         is written, and runs commands only after you confirm each one. Local and Auto never leave this computer.",
        13.0,
        theme::TEXT_DIM,
    ));
    let shortcut = |keys: &'a str, what: &'a str| row![container(mono(keys, 12.0, theme::GOLD)).width(110), label(what, 12.5, theme::TEXT_DIM)].spacing(10);
    let mut keys = Column::new().spacing(6);
    keys = keys.push(shortcut("Ctrl+P", "Go to a file"));
    keys = keys.push(shortcut("Ctrl+S", "Save the file you are editing"));
    keys = keys.push(shortcut("Ctrl+F, Ctrl+H", "Find, and replace, in the file"));
    keys = keys.push(shortcut("Ctrl+Shift+F", "Search the folder"));
    keys = keys.push(shortcut("Ctrl+K", "Ask the agent to edit the selected lines"));
    keys = keys.push(shortcut("Ctrl+L", "Ask the agent"));
    keys = keys.push(shortcut("Ctrl+B", "Show or hide the side bar"));
    keys = keys.push(shortcut("Ctrl+J", "The agent's commands and your terminal"));
    keys = keys.push(shortcut("Enter", "Send (Shift+Enter: a new line; Esc: stop the agent)"));
    col = col.push(container(keys).padding(16).style(theme::panel));
    if state.ide.folder.is_none() {
        col = col.push(ui::primary("Open folder…", (state.services_ready() && !state.ide.picking).then(|| go(IdeMsg::Browse))));
        col = col.push(note("Or drop a folder on the window."));
    } else {
        col = col.push(note("Choose a file in the Explorer, or press Ctrl+P."));
    }
    center(col).into()
}

// ------------------------------------------------------------------------------------------------- bottom panel

fn bottom_panel<'a>(state: &'a State, phase: f32) -> El<'a> {
    let ide = &state.ide;
    let mut head = Row::new().spacing(2).align_y(Alignment::Center).padding([2.0, 8.0]);
    for (tab, words) in [(PanelTab::Commands, "AGENT COMMANDS"), (PanelTab::Terminal, "TERMINAL")] {
        let on = ide.panel == tab;
        head = head.push(
            button(text(words).size(11).font(fonts().ui_strong).color(if on { theme::GOLD } else { theme::TEXT_FAINT }))
                .padding([5.0, 10.0])
                .style(theme::ghost_button)
                .on_press(go(IdeMsg::Panel(tab))),
        );
    }
    if ide.panel == PanelTab::Terminal {
        head = head.push(space().width(10));
        for (n, t) in ide.terms.iter().enumerate() {
            head = head.push(terminal_tab(n + 1, t, ide.term_active == Some(t.id)));
        }
        let room = !ide.term_starting && ide.terms.len() < super::term::MAX_TERMINALS;
        head = head.push(icon("+", "New terminal", room.then(|| go(IdeMsg::TermNew))));
    }
    head = head.push(space().width(Length::Fill));
    head = head.push(icon("\u{00D7}", "Hide the panel (Ctrl+J)", Some(go(IdeMsg::TogglePanel))));
    let body: El<'a> = match ide.panel {
        PanelTab::Commands => super::agent::commands(state, phase),
        PanelTab::Terminal => terminal(state, phase),
    };
    column![rule(), head, container(body).height(Length::Fill)].into()
}

/// A terminal's tab in the panel's header: its number and folder, gold while shown, and its close button.
fn terminal_tab<'a>(n: usize, t: &'a super::term::Term, on: bool) -> El<'a> {
    let folder = t.cwd.file_name().map_or_else(|| t.cwd.display().to_string(), |f| f.to_string_lossy().into_owned());
    let color = if t.ended.is_some() {
        theme::TEXT_FAINT
    } else if on {
        theme::GOLD
    } else {
        theme::TEXT_DIM
    };
    let words = if t.ended.is_some() { format!("{n} {folder} (ended)") } else { format!("{n} {folder}") };
    let glyph = mono(">_", 10.5, if t.ended.is_some() { theme::TEXT_FAINT } else { theme::POSITIVE });
    let tab = row![glyph, label(words, 12.0, color)].spacing(6).align_y(Alignment::Center);
    let tip = format!("{}, in {}", t.label, t.cwd.display());
    let tab = tooltip(
        button(tab).padding([4.0, 8.0]).style(theme::ghost_button).on_press(go(IdeMsg::TermShow(t.id))),
        container(label(tip, 12.0, theme::TEXT)).padding([4.0, 8.0]).style(theme::card),
        tooltip::Position::Top,
    );
    let close = button(label("\u{00D7}", 12.0, theme::TEXT_FAINT))
        .padding([2.0, 4.0])
        .style(theme::ghost_button)
        .on_press(go(IdeMsg::TermClose(t.id)));
    row![tab, close].spacing(0).align_y(Alignment::Center).into()
}

/// The terminals' body: the one on show, or how one starts.
fn terminal<'a>(state: &'a State, phase: f32) -> El<'a> {
    let ide = &state.ide;
    let problem = ide
        .term_problem
        .as_ref()
        .map(|why| -> El<'a> { container(label(why.as_str(), 12.5, theme::CAUTION)).padding([4.0, 12.0]).into() });
    if let Some(t) = ide.shown_term() {
        let id = t.id;
        // The core's own dialog, over the page, takes the keyboard from the terminal as well.
        let focused = ide.terminal_focused() && state.dialog.is_none();
        let grid: El<'a> = super::term::grid::grid(t, focused, move |input| go(IdeMsg::TermInput(id, input))).into();
        let mut col = Column::new().push(container(grid).width(Length::Fill).height(Length::Fill));
        if let Some(why) = &t.ended {
            col = col.push(
                container(
                    row![
                        label(why.as_str(), 12.5, theme::TEXT_DIM),
                        space().width(Length::Fill),
                        ghost("Start it again", Some(go(IdeMsg::TermRestart(id)))),
                        ghost("Close", Some(go(IdeMsg::TermClose(id)))),
                    ]
                    .spacing(8)
                    .align_y(Alignment::Center),
                )
                .padding([4.0, 12.0])
                .style(|_| solid(theme::RAISED)),
            );
        }
        if let Some(p) = problem {
            col = col.push(p);
        }
        return col.into();
    }
    let mut col = Column::new().spacing(10).padding([14.0, 16.0]);
    if ide.term_starting {
        col = col.push(
            row![crate::spinner::spinner(phase, 14.0), note("Starting PowerShell\u{2026}")]
                .spacing(10)
                .align_y(Alignment::Center),
        );
    } else {
        if let Some(p) = problem {
            col = col.push(p);
        }
        col = col.push(note(
            "Your own PowerShell, in the folder you have open. What runs in it is only what you type or paste; the \
             agent cannot type into it.",
        ));
        col = col.push(ui::secondary("New terminal", Some(go(IdeMsg::TermNew))));
    }
    col.into()
}

// ------------------------------------------------------------------------------------------------- status bar

fn status_bar<'a>(state: &'a State, phase: f32) -> El<'a> {
    let ide = &state.ide;
    let item = |words: String, color: Color| -> El<'a> { container(label(words, 11.5, color)).padding([0.0, 8.0]).into() };
    let mut left = Row::new().spacing(2).align_y(Alignment::Center);
    match &ide.folder {
        Some(f) => {
            left = left.push(item(format!("\u{2750} {}", f.name()), theme::TEXT_DIM));
            if let Some(view) = &state.folder.view
                && view.git_top_level.is_some()
            {
                left = left.push(item("git".to_string(), theme::TEXT_FAINT));
            }
        }
        None => left = left.push(item("No folder".to_string(), theme::TEXT_FAINT)),
    }
    if let Some(set) = &state.changes
        && set.waiting > 0
    {
        left = left.push(
            button(label(format!("\u{00B1} {} waiting for review", set.waiting), 11.5, theme::GOLD))
                .padding([0.0, 8.0])
                .style(theme::ghost_button)
                .on_press(go(IdeMsg::Side(Side::Changes))),
        );
    }
    if let Some(c) = state.conversation() {
        match c.waiting() {
            0 => {}
            n => left = left.push(item(format!("{n} waiting for you"), theme::GOLD)),
        }
    }
    if state.busy() {
        left = left.push(container(crate::spinner::spinner(phase, 12.0)).padding([0.0, 6.0]));
    }
    let mut right = Row::new().spacing(2).align_y(Alignment::Center);
    if let Some(t) = ide.active_tab()
        && let TabKind::File { lang, body, .. } = &t.kind
    {
        if let Body::Editing(e) = body {
            let c = e.content.cursor().position;
            let column = e.content.line(c.line).map_or(c.column, |l| l.text.get(..c.column).map_or(c.column, |h| h.chars().count()));
            right = right.push(item(format!("Ln {}, Col {}", c.line + 1, column + 1), theme::TEXT_DIM));
            right = right.push(item(format!("UTF-8{}", if e.bom { " with BOM" } else { "" }), theme::TEXT_FAINT));
            right = right.push(item(e.ending.name().to_string(), theme::TEXT_FAINT));
        }
        right = right.push(item(lang.name().to_string(), theme::TEXT_FAINT));
    }
    if let Some(local) = &state.local {
        use lattice_protocol::chat::RuntimeState;
        let (words, color) = match local.state {
            RuntimeState::Ready => ("\u{25CF} Local model ready", theme::POSITIVE),
            RuntimeState::NoModel => ("No local model chosen", theme::CAUTION),
            RuntimeState::Offline => ("Local server off", theme::TEXT_FAINT),
            RuntimeState::Error => ("Local server failed", theme::DANGER),
        };
        right = right.push(item(words.to_string(), color));
    }
    match state.shared_writes() {
        Some(true) => right = right.push(item("Chats saved with the web Lattice's".to_string(), theme::TEXT_FAINT)),
        Some(false) => right = right.push(item("Chats read only".to_string(), theme::CAUTION)),
        None => {}
    }
    let line = row![left, space().width(Length::Fill), right].align_y(Alignment::Center).height(STATUS_HEIGHT);
    column![rule(), container(line).width(Length::Fill).style(bar_style)].into()
}

// ------------------------------------------------------------------------------------------------- overlays

fn quick_open<'a>(state: &'a State, q: &'a super::Quick) -> El<'a> {
    let mut list = Column::new().spacing(0);
    if state.ide.files().is_none() {
        list = list.push(container(note("Open a folder first.")).padding(10));
    } else if q.results.is_empty() {
        list = list.push(container(note("No file fits.")).padding(10));
    }
    for (i, r) in q.results.iter().enumerate() {
        let on = i == q.selected;
        list = list.push(
            button(
                row![
                    file_dot(&r.path),
                    label(file_name(&r.path).to_string(), 13.0, if on { theme::GOLD } else { theme::TEXT }),
                    label(r.path.clone(), 11.5, theme::TEXT_FAINT),
                ]
                .spacing(10)
                .align_y(Alignment::Center),
            )
            .width(Length::Fill)
            .padding([5.0, 12.0])
            .style(theme::list_row(on))
            .on_press(go(IdeMsg::QuickPick(Some(r.path.clone())))),
        );
    }
    let card = container(
        column![
            text_input("Go to file: type part of its name", &q.query)
                .id(super::quick_id())
                .on_input(|t| go(IdeMsg::QuickQuery(t)))
                .on_submit(go(IdeMsg::QuickPick(None)))
                .padding([8.0, 10.0])
                .size(14)
                .style(ui::input_style),
            scrollable(list).height(Length::Shrink).style(theme::scrollbars),
            note("Enter opens, the arrows choose, Esc closes."),
        ]
        .spacing(8),
    )
    .padding(10)
    .width(620)
    .max_height(460)
    .style(theme::card);
    // Pinned near the top, as an IDE's box is; a click outside closes it.
    let backdrop = mouse_area(container(space()).width(Length::Fill).height(Length::Fill)).on_press(go(IdeMsg::QuickClose));
    stack![backdrop, container(card).center_x(Length::Fill).padding(padding::top(70))].into()
}

fn confirm_card<'a>(state: &'a State, confirm: &'a Confirm) -> El<'a> {
    let ide = &state.ide;
    let name = |tab: &u64| ide.tab(*tab).and_then(EditorTab::path).unwrap_or("this file").to_string();
    let (title, words, yes): (String, String, &str) = match confirm {
        Confirm::SaveAuthority { tab } => (
            format!("Save {}?", name(tab)),
            "It is an authority file: it changes what tools, builds or agents are allowed or run (the core's own list). \
             Saving writes your text over it now."
                .to_string(),
            "Save it",
        ),
        Confirm::CreateAuthority { path } => (
            format!("Make {path}?"),
            "It would be an authority file: it changes what tools, builds or agents are allowed or run.".to_string(),
            "Make it",
        ),
        Confirm::Discard { tab } => (
            format!("Close {} without saving?", name(tab)),
            "Your edits to it are not saved and will be lost.".to_string(),
            "Close without saving",
        ),
        Confirm::Reload { tab } => (
            format!("Read {} again from disk?", name(tab)),
            "Your unsaved edits to it will be lost.".to_string(),
            "Drop my edits and reload",
        ),
        Confirm::SwitchFolder { path, .. } => (
            format!("Open {path}?"),
            "Files of the open folder have unsaved edits; opening another folder closes their tabs and loses them."
                .to_string(),
            "Open it and lose them",
        ),
        Confirm::Paste { text, runs, .. } => {
            let lines = text.lines().count();
            if *runs {
                let first = text.lines().next().unwrap_or_default();
                (
                    format!("Paste {lines} lines into the terminal?"),
                    format!(
                        "The shell runs each line as it arrives, as if Enter were pressed after it. The first line: {}",
                        ui::cut(first, 90)
                    ),
                    "Paste and run them",
                )
            } else {
                (
                    format!("Paste {} characters into the terminal?", text.chars().count()),
                    "It is a long paste: check that it is what you meant to paste.".to_string(),
                    "Paste it",
                )
            }
        }
        Confirm::CloseTerminal { .. } => (
            "Close the terminal?".to_string(),
            "A program started in it is still running: closing the terminal ends it, and everything it started."
                .to_string(),
            "Close and end it",
        ),
    };
    let ready = ide.confirm_ready();
    container(
        column![
            strong(title, 16.0, theme::TEXT),
            label(words, 13.0, theme::TEXT_DIM),
            // The refusing button comes first and is the default; neither takes input for the first half second.
            row![ui::primary("Cancel", ready.then(|| go(IdeMsg::Confirm(false)))), ui::secondary(yes, ready.then(|| go(IdeMsg::Confirm(true))))].spacing(10),
        ]
        .spacing(12),
    )
    .padding(22)
    .max_width(560)
    .style(theme::card)
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_language_has_a_colour_that_reads_on_the_side_bar() {
        use lattice_app::theme::{SIDEBAR, contrast};
        for lang in [
            Lang::Rust, Lang::Python, Lang::Js, Lang::C, Lang::CSharp, Lang::Go, Lang::Java, Lang::Shader, Lang::Json,
            Lang::Toml, Lang::Yaml, Lang::Markdown, Lang::Shell, Lang::PowerShell, Lang::Batch, Lang::Sql, Lang::Html,
            Lang::Css, Lang::Ini, Lang::Plain,
        ] {
            assert!(contrast(lang_color(lang), SIDEBAR) >= 4.5, "{lang:?}");
        }
    }
}
