//! The fleet, as the agent bridge answers it for the person's own agent (`alelyon-cli`'s fleet tools): every group's
//! card in every project, one group's topics, and everything waiting for the person. The reads are the core's
//! (`convo::fleet`); each is answered off the window's thread and changes nothing in the window. Answering a question
//! is the core's `answer`, as the window's own card answers it; an approval is never decided here.

use std::sync::Arc;

use iced::Task;
use lattice_core::convo::agent::AgentChat;
use lattice_protocol::conversation::AgentChatService;
use serde_json::{Value, json};

use super::{Msg, State, on};
use crate::bridge::Ask;

/// What the bridge answers: a status and a body.
type Reply = (u16, Value);

/// The fleet's reads.
pub const READS: [&str; 6] = ["fleet_groups", "fleet_group", "fleet_waiting", "fleet_record", "fleet_watch", "fleet_search"];

/// The longest a watch waits for a change (under the bridge's own wait for an answer), and how often it looks.
pub const WATCH_MOST: std::time::Duration = std::time::Duration::from_secs(25);
pub const WATCH_EVERY: std::time::Duration = std::time::Duration::from_secs(2);

/// What a read answers before Lattice has started in this window.
fn off() -> Reply {
    (409, json!({"error": "lattice-off", "message":
        "Lattice has not started in the Alelyon window yet. Open Lattice there once (or send go with section \"lattice\"), then try again."}))
}

fn read(chat: &AgentChat, command: &str, group: Option<&str>) -> Reply {
    match command {
        "fleet_groups" => {
            let groups: Vec<Value> = chat
                .fleet_groups()
                .into_iter()
                .map(|(project, card)| {
                    let mut card = serde_json::to_value(card).unwrap_or(Value::Null);
                    card["project"] = json!(project);
                    card
                })
                .collect();
            (200, json!({"groups": groups}))
        }
        "fleet_group" => match group.and_then(|id| chat.fleet_topics(id)) {
            Some(topics) => (200, json!({"group": group, "topics": topics})),
            None => (404, json!({"error": "unknown-group", "message": "There is no such group; see fleet_groups."})),
        },
        "fleet_search" => match group {
            Some(query) => (200, json!({"sessions": chat.search_sessions(query, 20)})),
            None => (400, json!({"error": "missing-field", "message": "fleet_search needs the words to find."})),
        },
        "fleet_record" => match group.and_then(|id| chat.thread_record(id)) {
            Some(record) => (200, json!({"record": record})),
            None => (404, json!({"error": "unknown-group", "message": "There is no such group; see fleet_groups."})),
        },
        _ => (200, json!({"waiting": chat.fleet_waiting()})),
    }
}

impl State {
    /// Answer the fleet read `ask`, off the window's thread.
    pub(crate) fn fleet_read(&self, ask: &Arc<Ask>) -> Task<Msg> {
        let Some(services) = self.services.clone() else {
            let (status, body) = off();
            ask.answer(status, body);
            return Task::none();
        };
        let group = ask.fields.get(if ask.command == "fleet_search" { "query" } else { "id" }).and_then(Value::as_str).map(str::to_owned);
        if matches!(ask.command.as_str(), "fleet_group" | "fleet_record" | "fleet_search") && group.is_none() {
            ask.answer(400, json!({"error": "missing-field", "message": "This needs the group's id."}));
            return Task::none();
        }
        let (chat, ask) = (services.chat.clone(), ask.clone());
        if ask.command == "fleet_watch" {
            // Answered when the fleet's digest differs from the one the watcher last saw, or when the wait is over.
            let since = ask.fields.get("since").and_then(Value::as_str).map(str::to_owned);
            let wait = ask
                .fields
                .get("wait_s")
                .and_then(Value::as_u64)
                .map_or(WATCH_MOST, |s| std::time::Duration::from_secs(s.clamp(1, WATCH_MOST.as_secs())));
            return Task::future(on(&services, async move {
                let until = tokio::time::Instant::now() + wait;
                loop {
                    let state = chat.fleet_state();
                    let changed = since.as_deref() != Some(state.digest.as_str());
                    if changed || tokio::time::Instant::now() + WATCH_EVERY > until {
                        ask.answer(200, json!({"changed": changed, "fleet": state}));
                        return;
                    }
                    tokio::time::sleep(WATCH_EVERY).await;
                }
            }))
            .discard();
        }
        Task::future(on(&services, async move {
            let (status, body) = read(&chat, &ask.command, group.as_deref());
            ask.answer(status, body);
        }))
        .discard()
    }

    /// The person's agent answers the question `call` waiting in the topic `topic`.
    pub(crate) fn fleet_answer(&self, topic: &str, call: &str, text: String) -> Reply {
        let Some(services) = self.services.as_ref() else { return off() };
        match services.chat.answer(topic, call, text) {
            Ok(()) => (200, json!({"done": "answered"})),
            Err(why) => (409, json!({"error": "not-waiting", "message": why.message})),
        }
    }

    /// The person's agent starts a group of work: a group named `name` in the project worked in, with `text` as the
    /// first message of its chat.
    pub(crate) fn fleet_new_group(&self, name: &str, text: &str) -> Result<Task<Msg>, Reply> {
        if self.services.is_none() {
            return Err(off());
        }
        Ok(self.create_group(name.to_owned(), Some(text.to_owned())))
    }

    /// The person's agent guides one session, the topic `topic`, as its card's reply box does: a steer of its
    /// running turn, else its next message. A topic the window does not list is refused.
    pub(crate) fn fleet_reply(&self, topic: &str, text: &str) -> Result<Task<Msg>, Reply> {
        use super::ide::sessions::Reply as How;
        if self.services.is_none() {
            return Err(off());
        }
        let Some(listed) = self.list.as_ref().and_then(|l| l.conversations.iter().find(|c| c.id == topic)) else {
            return Err((404, json!({"error": "unknown-topic", "message": "The window does not list that topic; see fleet_group."})));
        };
        let how = if listed.running { How::Steer } else { How::Send };
        Ok(self.reply_to(topic, how, text.to_owned(), super::done))
    }

    /// The person's agent stops the topic `topic`'s running turn.
    pub(crate) fn fleet_stop(&self, topic: &str) -> Reply {
        let Some(services) = self.services.as_ref() else { return off() };
        (200, json!({"stopped": services.chat.stop(topic)}))
    }

    /// The person's agent records a decision on the group `id`'s record (`convo::record`), from outside Lattice.
    pub(crate) fn fleet_decide(&self, id: &str, decision: &str) -> Reply {
        let Some(services) = self.services.as_ref() else { return off() };
        match services.chat.record_decision(id, decision, "your agent") {
            Ok(d) => (200, json!({"done": "recorded", "id": d.id})),
            Err(why) => (400, json!({"error": "refused", "message": why})),
        }
    }

    /// The person's agent writes `text` in the group `id`'s chat: the group opens, and the message goes where the
    /// person's own message in that group would. A group the window has not read yet is refused, never sent somewhere
    /// else; the refusal comes with the read of the groups, so that a second try finds it.
    pub(crate) fn fleet_send(&mut self, id: &str, text: &str) -> Result<Task<Msg>, (Reply, Task<Msg>)> {
        use super::ide::groups::{GroupsMsg, go};
        if self.ide.groups.find(id).is_none_or(|g| g.archived) {
            let refused = (404, json!({"error": "unknown-group", "message":
                "The window does not show that group (yet). See fleet_groups, and try again in a moment."}));
            return Err((refused, self.read_groups()));
        }
        self.composer = iced::widget::text_editor::Content::with_text(text);
        Ok(Task::done(go(GroupsMsg::Open(Some(id.to_owned())))).chain(Task::done(Msg::Send)))
    }
}
