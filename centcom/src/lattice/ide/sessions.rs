//! A session in the open group: the topic doing part of its work, as a status view, not a chat. Its row says what it
//! does now; opened, its card shows its steps, what it did last, and whether it waits for the reader, with one small
//! reply box for answering it or guiding it when its work is vague. The group's chat stays the place to talk to the
//! work; the reply box is the exception, never the main thing. While the session works, a reply steers it (taken
//! before its next model call); while it is idle, a reply is its next message. The facts are the core's
//! (`convo::fleet`).

use iced::widget::{Column, Row, container, row, text_input};
use iced::{Alignment, Element, Length, Padding, Task};
use lattice_core::convo::fleet::TopicView;
use lattice_protocol::conversation::TodoStatus;

use super::groups::{GroupsMsg, go, small};
use crate::lattice::{Msg, STOPPED, State, on};
use crate::theme;
use crate::ui::{self, label};

type El<'a> = Element<'a, Msg>;

/// How a reply reaches a session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reply {
    /// It works now: the reply steers the running turn.
    Steer,
    /// It is idle: the reply is its next message.
    Send,
}

/// How a reply reaches `topic`.
pub fn reply_kind(topic: &TopicView) -> Reply {
    if topic.running { Reply::Steer } else { Reply::Send }
}

/// What a session does now, in a few words: what it waits for, the step under way, or where its steps stand.
pub fn status_line(topic: &TopicView) -> String {
    let steps = (topic.todos_total > 0).then(|| format!("{} of {} steps", topic.todos_done, topic.todos_total));
    if topic.waiting > 0 {
        return if topic.waiting == 1 { "Waits for you".to_string() } else { format!("Waits for you ({})", topic.waiting) };
    }
    match (topic.running, &topic.doing, steps) {
        (true, Some(step), _) => format!("Working: {step}"),
        (true, None, _) => "Working".to_string(),
        (false, _, Some(steps)) if topic.todos_done == topic.todos_total => format!("Done ({steps})"),
        (false, _, Some(steps)) => format!("Idle, {steps}"),
        (false, _, None) => "Idle".to_string(),
    }
}

/// The colour of a status line: gold while it waits for the reader, the text's while it works, faint otherwise.
fn tone(topic: &TopicView) -> iced::Color {
    if topic.waiting > 0 {
        theme::GOLD
    } else if topic.running {
        theme::TEXT
    } else {
        theme::TEXT_FAINT
    }
}

/// The status line under a session's row, with the toggle that opens its card.
pub fn status_row<'a>(state: &'a State, topic: &'a TopicView) -> El<'a> {
    let open = state.ide.groups.session.as_deref() == Some(topic.id.as_str());
    let toggle = small(if open { "Hide" } else { "Status" }, Some(go(GroupsMsg::Session((!open).then(|| topic.id.clone())))));
    container(
        row![container(label(status_line(topic), 10.5, tone(topic))).width(Length::Fill).clip(true), toggle]
            .spacing(6)
            .align_y(Alignment::Center),
    )
    .padding(Padding { top: 0.0, right: 10.0, bottom: 2.0, left: 30.0 })
    .into()
}

/// A session's card: its steps, what it did last, what it waits for, and the reply box.
pub fn card<'a>(state: &'a State, topic: &'a TopicView) -> El<'a> {
    let groups = &state.ide.groups;
    let mut col = Column::new().spacing(6).width(Length::Fill);
    if !topic.todos.is_empty() {
        col = col.push(label("Steps", 10.5, theme::TEXT_FAINT));
        for step in &topic.todos {
            let (mark, colour) = match step.status {
                TodoStatus::Done => ("\u{2713}", theme::TEXT_FAINT),
                TodoStatus::InProgress => ("\u{25b8}", theme::GOLD),
                TodoStatus::Pending => ("\u{25cb}", theme::TEXT_DIM),
            };
            col = col.push(Row::new().spacing(6).push(label(mark, 11.5, colour)).push(label(step.content.as_str(), 11.5, colour)));
        }
    }
    if !topic.recent.is_empty() {
        col = col.push(label("Did last", 10.5, theme::TEXT_FAINT));
        for action in &topic.recent {
            col = col.push(label(ui::cut(action, 90), 11.0, theme::TEXT_DIM));
        }
    }
    if topic.waiting > 0 {
        col = col.push(label("Its questions are in the group's chat, to answer there or here.", 11.0, theme::GOLD));
    }
    let hint = match reply_kind(topic) {
        Reply::Steer => "Guide it while it works\u{2026}",
        Reply::Send => "Answer or guide this session\u{2026}",
    };
    let sendable = !groups.reply.trim().is_empty() && !groups.replying;
    col = col.push(
        row![
            text_input(hint, &groups.reply)
                .on_input(|t| go(GroupsMsg::Reply(t)))
                .on_submit(go(GroupsMsg::SendReply))
                .padding([4.0, 6.0])
                .size(11.5)
                .style(ui::input_style),
            small("Send", sendable.then(|| go(GroupsMsg::SendReply))),
        ]
        .spacing(6)
        .align_y(Alignment::Center),
    );
    container(col).padding(Padding { top: 4.0, right: 10.0, bottom: 8.0, left: 30.0 }).width(Length::Fill).into()
}

impl State {
    /// Send the reply box's text to the open session: a steer while it works, else its next message, in its own
    /// folder. The window stays where it is.
    pub(in crate::lattice) fn session_reply(&mut self) -> Task<Msg> {
        let groups = &self.ide.groups;
        let Some(topic) = groups.session.as_ref().and_then(|id| groups.topics.iter().find(|t| t.id == *id)).cloned() else {
            return Task::none();
        };
        let text = groups.reply.trim().to_string();
        if text.is_empty() || groups.replying {
            return Task::none();
        }
        if self.agent().is_none() {
            return Task::none();
        }
        self.ide.groups.replying = true;
        let replied = |r: Option<Result<(), String>>| go(GroupsMsg::Replied(r.unwrap_or_else(|| Err(STOPPED.to_string()))));
        self.reply_to(&topic.id, reply_kind(&topic), text, replied)
    }

    /// `text` to the topic `id`, as `how` says: a steer of its running turn, or its next message in its own folder.
    /// The window stays where it is; `then` is what the window is told of how it went.
    pub(in crate::lattice) fn reply_to(
        &self,
        id: &str,
        how: Reply,
        text: String,
        then: impl Fn(Option<Result<(), String>>) -> Msg + Send + 'static,
    ) -> Task<Msg> {
        let Some((services, agent)) = self.agent() else { return Task::none() };
        let id = id.to_owned();
        match how {
            Reply::Steer => Task::perform(on(&services, async move { agent.steer(&id, text).await.map_err(|r| r.message) }), then),
            Reply::Send => {
                let mut request = self.send_request(text, self.mode, None);
                request.workspace = self
                    .list
                    .as_ref()
                    .and_then(|l| l.conversations.iter().find(|c| c.id == id))
                    .and_then(|c| c.workspace.as_ref().map(|w| w.id.clone()));
                request.conversation = Some(id);
                request.project = None;
                request.images.clear();
                Task::perform(on(&services, async move { agent.send(request).await.map(drop).map_err(|r| r.message) }), then)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lattice_protocol::conversation::TodoItem;

    fn topic(running: bool, waiting: usize, steps: &[TodoStatus]) -> TopicView {
        let todos: Vec<TodoItem> = steps
            .iter()
            .enumerate()
            .map(|(i, s)| TodoItem { content: format!("Step {i}"), status: *s, decision: None, chosen: None })
            .collect();
        TopicView {
            id: "t".into(),
            title: "Parser".into(),
            running,
            doing: todos.iter().find(|t| t.status == TodoStatus::InProgress).map(|t| t.content.clone()),
            todos_done: steps.iter().filter(|s| **s == TodoStatus::Done).count(),
            todos_total: steps.len(),
            waiting,
            updated: 0.0,
            todos,
            recent: Vec::new(),
        }
    }

    /// Mutants: a waiting session not saying so first; a working one not naming its step; finished steps read as idle.
    #[test]
    fn a_sessions_status_line_says_what_it_does_now() {
        use TodoStatus::*;
        assert_eq!(status_line(&topic(true, 1, &[Done, InProgress])), "Waits for you");
        assert_eq!(status_line(&topic(false, 2, &[])), "Waits for you (2)");
        assert_eq!(status_line(&topic(true, 0, &[Done, InProgress, Pending])), "Working: Step 1");
        assert_eq!(status_line(&topic(true, 0, &[])), "Working");
        assert_eq!(status_line(&topic(false, 0, &[Done, Done])), "Done (2 of 2 steps)");
        assert_eq!(status_line(&topic(false, 0, &[Done, Pending])), "Idle, 1 of 2 steps");
        assert_eq!(status_line(&topic(false, 0, &[])), "Idle");
    }

    /// A reply steers a working session and is the next message of an idle one; opening another session's card
    /// clears a reply half typed for the last one, so it never goes to the wrong session.
    #[test]
    fn a_reply_steers_a_working_session_and_is_kept_to_its_own_card() {
        assert_eq!(reply_kind(&topic(true, 0, &[])), Reply::Steer);
        assert_eq!(reply_kind(&topic(false, 1, &[])), Reply::Send);
        let mut state = State::new();
        let _ = state.update(go(GroupsMsg::Session(Some("a".into()))));
        let _ = state.update(go(GroupsMsg::Reply("Use CSV".into())));
        let _ = state.update(go(GroupsMsg::Session(Some("a".into()))));
        assert_eq!(state.ide.groups.reply, "Use CSV", "the same card keeps it");
        let _ = state.update(go(GroupsMsg::Session(Some("b".into()))));
        assert!(state.ide.groups.reply.is_empty(), "another card starts empty");
        let _ = state.update(go(GroupsMsg::Replied(Err("That chat is closed.".into()))));
        assert_eq!(state.ide.groups.said.as_deref(), Some("That chat is closed."));
        let _ = state.session_reply();
        assert!(!state.ide.groups.replying, "nothing is sent for a session the window has not read");
    }
}
