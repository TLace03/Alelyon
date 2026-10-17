//! The Chats side, on one chat surface: the groups (`super::groups`), then every topic, newest first, to jump to. The
//! chart of what is working now (each topic that works or waits for the reader, the tasks started from it drawn
//! beneath it, and the topic shown; a task opens in the Tasks view) is drawn on the board (`super::board`).

use std::collections::{HashMap, HashSet};

use iced::widget::{Column, Row, button, container, scrollable, space, text_input};
use iced::{Alignment, Element, Length, Padding};
use lattice_core::convo::task_runs::Origin;
use lattice_protocol::conversation::ConversationSummary;

use super::IdeMsg;
use crate::lattice::{Msg, State};
use crate::theme;
use crate::ui::{self, label, mono, note};

type El<'a> = Element<'a, Msg>;

fn go(msg: IdeMsg) -> Msg {
    Msg::Ide(msg)
}

/// The topics listed before "Show all", with no search.
pub const TOPICS_SHOWN: usize = 30;
/// The most steps followed from a task back to the topic it came from.
const MAX_DEPTH: usize = 8;

/// One topic on the chart, with the tasks started from it.
#[derive(Debug, PartialEq)]
pub struct Node<'a> {
    pub row: &'a ConversationSummary,
    /// Its task's title, when it was started from a chip or moved on from another topic.
    pub task: Option<&'a str>,
    /// The agent moved the work here from the topic above (`switch_project`).
    pub moved: bool,
    pub children: Vec<Node<'a>>,
}

/// The chart: each topic that works or waits for the reader (`calling` too), the topic shown (`current`), and the
/// topics each came from; a task beneath the topic it came from, the rest side by side, newest first.
pub fn chart<'a>(
    rows: &'a [ConversationSummary],
    origins: &'a HashMap<String, Origin>,
    current: Option<&str>,
    calling: &HashSet<String>,
) -> Vec<Node<'a>> {
    let listed: HashMap<&str, &ConversationSummary> = rows.iter().map(|r| (r.id.as_str(), r)).collect();
    let mut on: HashSet<&str> = rows
        .iter()
        .filter(|r| r.running || r.needs_you || calling.contains(&r.id) || current == Some(r.id.as_str()))
        .map(|r| r.id.as_str())
        .collect();
    // The topics the active ones came from, so a task is drawn beneath its topic.
    for id in on.clone() {
        let mut at = id;
        for _ in 0..MAX_DEPTH {
            let Some(from) = origins.get(at).map(|o| o.from.as_str()).filter(|f| listed.contains_key(f)) else { break };
            on.insert(from);
            at = from;
        }
    }
    // The topic a task is drawn beneath; none for a loop of topics that name each other, drawn side by side.
    let parent = |id: &str| -> Option<&str> {
        let from = origins.get(id).map(|o| o.from.as_str()).filter(|f| on.contains(f) && *f != id)?;
        let mut at = from;
        for _ in 0..MAX_DEPTH {
            match origins.get(at).map(|o| o.from.as_str()) {
                Some(up) if up == id => return None,
                Some(up) => at = up,
                None => break,
            }
        }
        Some(from)
    };
    fn build<'a>(
        id: &str,
        depth: usize,
        listed: &HashMap<&str, &'a ConversationSummary>,
        origins: &'a HashMap<String, Origin>,
        kids: &HashMap<&str, Vec<&'a str>>,
    ) -> Node<'a> {
        let row = listed[id];
        let mut children: Vec<Node<'a>> = Vec::new();
        if depth < MAX_DEPTH {
            for child in kids.get(id).map(Vec::as_slice).unwrap_or_default() {
                children.push(build(child, depth + 1, listed, origins, kids));
            }
        }
        let origin = origins.get(id);
        Node { row, task: origin.map(|o| o.title.as_str()), moved: origin.is_some_and(|o| o.switch), children }
    }
    let mut kids: HashMap<&str, Vec<&'a str>> = HashMap::new();
    let mut roots: Vec<&'a str> = Vec::new();
    for row in rows.iter().filter(|r| on.contains(r.id.as_str())) {
        match parent(&row.id) {
            Some(from) => kids.entry(from).or_default().push(row.id.as_str()),
            None => roots.push(row.id.as_str()),
        }
    }
    for list in kids.values_mut() {
        list.sort_by(|a, b| origins[*a].at.total_cmp(&origins[*b].at));
    }
    roots.sort_by(|a, b| listed[b].updated.total_cmp(&listed[a].updated));
    roots.into_iter().map(|id| build(id, 0, &listed, origins, &kids)).collect()
}

/// The Chats side: the groups, then the topics.
pub fn view<'a>(state: &'a State, phase: f32) -> El<'a> {
    let ide = &state.ide;
    let mut col = Column::new().spacing(2);
    let Some(list) = &state.list else {
        return container(ui::working(phase, "Reading the topics…")).padding(12).into();
    };
    col = super::groups::section(state, col, phase);
    col = projects_section(state, col);
    // Every topic, to jump to.
    let search = ide.chat_search.trim().to_lowercase();
    // In an open group, its topics only.
    let group = ide.groups.shown();
    let in_group = |c: &ConversationSummary| group.is_none_or(|g| g.topics.contains(&c.id));
    let shown: Vec<&ConversationSummary> = list
        .conversations
        .iter()
        .filter(|c| in_group(c) && (search.is_empty() || c.title.to_lowercase().contains(&search)))
        .collect();
    let heading = match group {
        Some(g) => format!("Topics in {} \u{00B7} {}", ui::cut(ide.groups.name(&g.id).unwrap_or(""), 28), g.topics.len()),
        None => format!("Topics \u{00B7} {}", list.conversations.len()),
    };
    // Under a head that folds them away.
    let folded = ide.side_folds.contains("topics");
    col = col.push(super::groups::section_head(&heading, "topics", folded, None));
    if folded {
        return scrollable(col).height(Length::Fill).style(theme::scrollbars).into();
    }
    col = col.push(
        container(text_input("Find a topic", &ide.chat_search).on_input(|t| go(IdeMsg::ChatSearch(t))).padding([5.0, 8.0]).size(12.5).style(ui::input_style))
            .padding(Padding { top: 0.0, right: 10.0, bottom: 4.0, left: 10.0 }),
    );
    let all = ide.chat_more.contains("") || !search.is_empty();
    for c in shown.iter().take(if all { shown.len() } else { TOPICS_SHOWN }) {
        col = col.push(topic_row(state, c, phase));
    }
    if shown.is_empty() {
        col = col.push(container(note(if search.is_empty() { "No topics yet." } else { "No topic's title has that." })).padding(12));
    }
    let hidden = shown.len().saturating_sub(TOPICS_SHOWN);
    if hidden > 0 && search.is_empty() {
        let words = if all { "Show fewer".to_string() } else { format!("Show {hidden} more") };
        col = col.push(
            button(label(words, 11.5, theme::TEXT_FAINT))
                .padding(Padding { top: 3.0, bottom: 3.0, left: 34.0, right: 8.0 })
                .style(theme::ghost_button)
                .on_press(go(IdeMsg::ChatMore(String::new(), !all))),
        );
    }
    if !list.archived.is_empty() {
        col = col.push(container(note(format!("{} archived (never deleted)", list.archived.len()))).padding(12));
    }
    scrollable(col).height(Length::Fill).style(theme::scrollbars).into()
}

/// A project's topics as the store lists them (newest first): the chats the project names.
pub(super) fn project_topics<'a>(rows: &'a [ConversationSummary], project: &lattice_core::projects::Project) -> Vec<&'a ConversationSummary> {
    rows.iter().filter(|r| project.chats.iter().any(|chat| *chat == r.id)).collect()
}

/// The projects, under a head that folds them away (with New project at its end): each with how many topics it holds,
/// the one worked in marked gold. A press works in it, as the composer's project chip does; its arrow shows its topics
/// under it, to open one, without moving the chat.
fn projects_section<'a>(state: &'a State, mut col: Column<'a, Msg>) -> Column<'a, Msg> {
    let Some(projects) = state.ide.projects.list.as_deref() else { return col };
    let shown: Vec<&lattice_core::projects::Project> = projects.iter().filter(|p| !p.archived).collect();
    let folded = state.ide.side_folds.contains("projects");
    let new = super::groups::small("New project", Some(go(IdeMsg::Projects(super::projects::ProjectsMsg::Create))));
    col = col.push(super::groups::section_head(&format!("Projects \u{00B7} {}", shown.len()), "projects", folded, Some(new)));
    if folded {
        return col;
    }
    let rows: &[ConversationSummary] = state.list.as_ref().map(|l| l.conversations.as_slice()).unwrap_or_default();
    for project in shown.iter().copied() {
        let key = format!("project:{}", project.id);
        let open = state.ide.side_folds.contains(&key);
        let picked = state.ide.project.as_deref() == Some(project.id.as_str());
        let members = project_topics(rows, project);
        let arrow = button(label(if open { "\u{25BE}" } else { "\u{25B8}" }, 11.0, theme::TEXT_FAINT))
            .padding([0.0, 2.0])
            .style(theme::ghost_button)
            .on_press(go(IdeMsg::SideFold(key)));
        let n = members.len();
        let line = Row::new()
            .spacing(8)
            .align_y(Alignment::Center)
            .push(arrow)
            .push(container(space().width(7).height(7)).style(theme::dot(if picked { theme::GOLD } else { theme::LINE })))
            .push(
                container(label(project.name.as_str(), 12.5, if picked { theme::GOLD } else { theme::TEXT }).wrapping(iced::widget::text::Wrapping::None))
                    .width(Length::Fill)
                    .clip(true),
            )
            .push(label(format!("{n} topic{}", if n == 1 { "" } else { "s" }), 10.5, theme::TEXT_FAINT));
        col = col.push(
            button(line).width(Length::Fill).padding([4.0, 10.0]).style(theme::list_row(picked)).on_press(Msg::WorkIn(Some(project.id.clone()))),
        );
        if open {
            col = super::groups::topic_rows(state, &members, col, false);
        }
    }
    if shown.is_empty() {
        col = col.push(container(note("No projects yet. New project makes one; the composer's project chip picks it.")).padding(Padding { top: 2.0, right: 10.0, bottom: 4.0, left: 12.0 }));
    }
    col
}

/// What is working now, as a chart: each topic that works or waits for the reader, the tasks started from it beneath
/// it, and the topic shown.
pub fn working<'a>(state: &'a State, mut col: Column<'a, Msg>, phase: f32) -> Column<'a, Msg> {
    let Some(list) = &state.list else { return col };
    let current = state.conversation().map(|c| c.id());
    let nodes = chart(&list.conversations, &state.task_origins, current, &state.calling);
    if !nodes.iter().any(|n| busy(state, n)) {
        col = col.push(note("Nothing is working now. A task you start from a chip runs beside the chat."));
    }
    for node in &nodes {
        col = node_lines(state, node, String::new(), None, col, phase);
    }
    col
}

fn needs(state: &State, row: &ConversationSummary) -> bool {
    row.needs_you || state.calling.contains(&row.id)
}

fn busy(state: &State, node: &Node) -> bool {
    node.row.running || needs(state, node.row) || node.children.iter().any(|n| busy(state, n))
}

/// What a topic's line says it is doing.
fn status(state: &State, row: &ConversationSummary, current: bool) -> (&'static str, iced::Color) {
    if needs(state, row) {
        ("needs you", theme::GOLD)
    } else if row.running {
        ("working", theme::GOLD_DIM)
    } else if current {
        ("shown", theme::TEXT_FAINT)
    } else {
        ("done", theme::TEXT_FAINT)
    }
}

/// A node's line and its tasks' beneath it, joined by lines: `lead` is what the line above drew at this depth, and
/// `last` whether this node is the last of its topic's tasks (`None` for a topic of its own).
fn node_lines<'a>(state: &'a State, node: &Node<'a>, lead: String, last: Option<bool>, mut col: Column<'a, Msg>, phase: f32) -> Column<'a, Msg> {
    let current = state.conversation().is_some_and(|c| c.id() == node.row.id);
    let branch = match last {
        None => String::new(),
        Some(true) => format!("{lead}\u{2514}\u{2500} "),
        Some(false) => format!("{lead}\u{251C}\u{2500} "),
    };
    let (words, colour) = status(state, node.row, current);
    let mut line = Row::new().spacing(6).align_y(Alignment::Center);
    if !branch.is_empty() {
        line = line.push(mono(branch, 12.0, theme::LINE));
    }
    line = line.push(if node.row.running {
        crate::spinner::spinner(phase, 10.0)
    } else {
        container(space().width(7).height(7)).style(theme::dot(if words == "needs you" { theme::GOLD } else { theme::LINE })).into()
    });
    let title = if node.row.title.is_empty() { "Untitled" } else { node.row.title.as_str() };
    let mut words_col = Column::new().push(
        label(title, 12.5, if current { theme::GOLD } else { theme::TEXT }).wrapping(iced::widget::text::Wrapping::None),
    );
    if let Some(task) = node.task.filter(|t| *t != title) {
        let words = if node.moved { task.to_string() } else { format!("Task: {task}") };
        words_col = words_col.push(label(words, 10.5, theme::TEXT_FAINT).wrapping(iced::widget::text::Wrapping::None));
    }
    line = line.push(container(words_col).width(Length::Fill).clip(true));
    line = line.push(label(words, 10.5, colour));
    col = col.push(button(line).width(Length::Fill).padding([4.0, 10.0]).style(theme::list_row(current)).on_press(Msg::Open(node.row.id.clone())));
    let lead = match last {
        None => lead,
        Some(true) => format!("{lead}   "),
        Some(false) => format!("{lead}\u{2502}  "),
    };
    let n = node.children.len();
    for (i, child) in node.children.iter().enumerate() {
        col = node_lines(state, child, lead.clone(), Some(i + 1 == n), col, phase);
    }
    col
}

/// One topic in the list: what it is doing, its title, and how long ago.
fn topic_row<'a>(state: &'a State, c: &'a ConversationSummary, phase: f32) -> El<'a> {
    let current = state.conversation().is_some_and(|o| o.id() == c.id);
    let mut line = Row::new().spacing(8).align_y(Alignment::Center);
    line = line.push(if c.running {
        crate::spinner::spinner(phase, 10.0)
    } else {
        container(space().width(7).height(7)).style(theme::dot(if needs(state, c) { theme::GOLD } else { theme::LINE })).into()
    });
    let title = if c.title.is_empty() { "Untitled" } else { c.title.as_str() };
    line = line.push(container(label(title, 12.5, if current { theme::GOLD } else { theme::TEXT }).wrapping(iced::widget::text::Wrapping::None)).width(Length::Fill).clip(true));
    line = line.push(label(super::ago(crate::utc::now() - c.updated), 10.5, theme::TEXT_FAINT));
    button(line).width(Length::Fill).padding([5.0, 10.0]).style(theme::list_row(current)).on_press(Msg::Open(c.id.clone())).into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use lattice_protocol::conversation::{Mode, Origin as RowOrigin};

    fn row(id: &str, updated: f64, running: bool, needs_you: bool) -> ConversationSummary {
        ConversationSummary {
            id: id.into(),
            title: id.to_uppercase(),
            created: 0.0,
            updated,
            turns: 2,
            pinned_provider: String::new(),
            workspace: None,
            mode: Mode::Agent,
            origin: RowOrigin::Native,
            running,
            needs_you,
        }
    }

    fn origin(from: &str, title: &str, at: f64) -> Origin {
        Origin { from: from.into(), title: title.into(), at, ..Origin::default() }
    }

    fn ids(nodes: &[Node]) -> Vec<String> {
        nodes.iter().map(|n| format!("{}{:?}", n.row.id, n.children.iter().map(|c| c.row.id.clone()).collect::<Vec<_>>())).collect()
    }

    #[test]
    fn the_chart_draws_working_tasks_beneath_their_topic_and_leaves_quiet_topics_out() {
        let rows = vec![
            row("a", 9.0, false, false),
            row("t1", 8.0, true, false),
            row("t2", 7.0, false, true),
            row("t3", 6.0, false, false),
            row("b", 5.0, true, false),
            row("quiet", 4.0, false, false),
            row("t4", 3.0, true, false),
        ];
        let origins: HashMap<String, Origin> = [
            ("t1".to_string(), origin("a", "Fix the badge", 2.0)),
            ("t2".to_string(), origin("a", "Write the test", 1.0)),
            ("t3".to_string(), origin("a", "Done one", 3.0)),
            // A task of a topic no longer listed stands alone.
            ("t4".to_string(), origin("gone", "Orphan", 1.0)),
        ]
        .into_iter()
        .collect();
        let nodes = chart(&rows, &origins, None, &HashSet::new());
        // `a` is drawn for its tasks though it is quiet; a finished task and a quiet topic are left out.
        assert_eq!(ids(&nodes), vec!["a[\"t2\", \"t1\"]", "b[]", "t4[]"]);
        assert_eq!(nodes[0].children[1].task, Some("Fix the badge"));
        // The topic shown is always drawn, and a topic calling for the reader counts as working.
        let calling: HashSet<String> = ["quiet".to_string()].into_iter().collect();
        let nodes = chart(&rows, &origins, Some("t3"), &calling);
        assert_eq!(ids(&nodes), vec!["a[\"t2\", \"t1\", \"t3\"]", "b[]", "quiet[]", "t4[]"]);
    }

    #[test]
    fn a_task_of_a_task_is_drawn_beneath_it_and_a_loop_is_drawn_side_by_side() {
        let rows = vec![row("a", 9.0, false, false), row("t1", 8.0, false, false), row("t2", 7.0, true, false)];
        let origins: HashMap<String, Origin> = [
            ("t1".to_string(), origin("a", "First", 1.0)),
            ("t2".to_string(), origin("t1", "Second", 2.0)),
        ]
        .into_iter()
        .collect();
        let nodes = chart(&rows, &origins, None, &HashSet::new());
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].children[0].children[0].row.id, "t2");
        // Two topics that name each other: neither is beneath the other forever.
        let looped: HashMap<String, Origin> =
            [("a".to_string(), origin("t1", "x", 1.0)), ("t1".to_string(), origin("a", "y", 2.0))].into_iter().collect();
        let rows = vec![row("a", 9.0, true, false), row("t1", 8.0, false, false)];
        let nodes = chart(&rows, &looped, None, &HashSet::new());
        assert_eq!(ids(&nodes), vec!["a[]", "t1[]"], "a loop is drawn as topics side by side, never left out");
    }
    #[test]
    fn a_projects_topics_are_the_chats_it_names_in_the_lists_order() {
        let rows = vec![row("a", 9.0, false, false), row("b", 8.0, false, false), row("c", 7.0, false, false)];
        let project = lattice_core::projects::Project {
            id: "p".into(),
            name: "App".into(),
            instructions: String::new(),
            folder: None,
            more_folders: vec![],
            files: vec![],
            choice: None,
            chats: vec!["c".into(), "a".into(), "gone".into()],
            created: 0.0,
            updated: 0.0,
            archived: false,
            allow_all: false,
        };
        let ids: Vec<&str> = project_topics(&rows, &project).iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, ["a", "c"], "newest first, as the list has them; a chat no longer listed is left out");
    }
}
