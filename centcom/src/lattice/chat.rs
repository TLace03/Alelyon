//! One open conversation as the page shows it: the shared transcript's turns, what each agent turn did (its tool
//! calls, commands, approvals, questions and staged changes), and the answer being written now.
//!
//! Everything is built here, in `update`, from the core's snapshot and its event batches; the view only reads it. An
//! event is applied once: a batch whose events are at or below the last sequence number seen is skipped, so a
//! follow that starts again from `last_seq` never doubles a card.

use iced::widget::markdown;

use lattice_protocol::chat::{ChatTurn, Role, Stage};
use lattice_protocol::conversation::{
    ApprovalDetail, ChangedPath, ConversationEvent, ConversationEventKind as Ev, ConversationSummary, DecidedBy,
    ArtifactKind, ExitReason, PlanOutcome, QueuedMessage, ReviewResult, Snapshot, TaskOutcome, TodoItem, TurnStatus,
};

/// What one tool call did, as its card shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct Tool {
    pub call: String,
    pub tool: String,
    pub summary: String,
    pub target: Option<String>,
    /// At most 2 KiB of what it returned.
    pub output: Option<String>,
    pub withheld: bool,
    pub truncated: bool,
    /// A command's progress: bytes and lines written, the last of it.
    pub progress: Option<(u64, u64, String)>,
    /// A command's end: its exit code, how long it ran, why it ended.
    pub exit: Option<(Option<i32>, u64, ExitReason)>,
    /// The files a command added, changed or deleted.
    pub effect: Vec<ChangedPath>,
}

/// One thing an agent turn did, in order.
#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    Tool(Tool),
    /// A call waiting for the reader (a command, an MCP tool), and how it was decided.
    Approval { call: String, detail: ApprovalDetail, allow_always: bool, decided: Option<(bool, DecidedBy)> },
    /// The agent's question, and whether it was answered.
    Question { call: String, text: String, options: Vec<String>, answered: bool },
    /// A task the agent suggested (a chip): nothing runs until the reader starts it, as a new chat; and what became
    /// of it.
    Task { task: String, title: String, summary: String, prompt: String, outcome: Option<TaskOutcome> },
    /// A plan the agent proposed in Ask mode: nothing changes until the reader approves it, and the chat goes on in
    /// Agent mode; and what became of it.
    Plan { plan: String, title: String, text: String, outcome: Option<PlanOutcome> },
    /// A version of a document the agent saved beside the chat (an artifact); its text is read when it is opened.
    Artifact { name: String, title: String, kind: ArtifactKind, version: u32, bytes: u64 },
    /// A change staged for review; nothing is written until it is kept.
    Staged { change: String, path: String, added: u32, removed: u32, authority: bool, result: Option<ReviewResult> },
    Conflict { path: String, reason: String },
    Steered(String),
    Notice(String),
    Error(String),
}

/// One turn's record: what it was asked of, and what it did.
#[derive(Clone, Debug, PartialEq)]
pub struct Activity {
    /// The turn it belongs to (the core's id for it).
    pub turn: String,
    pub label: String,
    pub local: bool,
    pub items: Vec<Item>,
    pub stage: Option<String>,
    pub status: Option<TurnStatus>,
    /// When the turn started (seconds since the epoch, the event's own time), for how long it has been working.
    pub started: Option<f64>,
}

pub struct Conversation {
    pub summary: ConversationSummary,
    pub turns: Vec<ChatTurn>,
    /// Each assistant turn's text, parsed once (index = the turn's place in `turns`; `None` for a user turn).
    rendered: Vec<Option<markdown::Content>>,
    pub activity: Vec<Activity>,
    /// The answer being written: its text so far, parsed as it arrives.
    pub live: Option<markdown::Content>,
    pub live_text: String,
    pub dropped: u64,
    pub last_seq: u64,
    pub queued: Vec<QueuedMessage>,
    /// Another window (the web Lattice, or another native one) is answering this conversation.
    pub elsewhere: bool,
    /// A turn is running: from its start (or the send that started it) to its end.
    pub running: bool,
    /// The agent's to-do list as it last wrote it (empty when it has none, or cleared it).
    pub todos: Vec<TodoItem>,
    /// How many images the reader attached to each user turn, by the turn's id.
    pub images: std::collections::HashMap<String, u32>,
}

impl std::fmt::Debug for Conversation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Conversation")
            .field("id", &self.summary.id)
            .field("turns", &self.turns.len())
            .field("last_seq", &self.last_seq)
            .field("running", &self.running)
            .finish()
    }
}

fn parse(turn: &ChatTurn) -> Option<markdown::Content> {
    (turn.role == Role::Assistant).then(|| markdown::Content::parse(&turn.text))
}

impl Conversation {
    pub fn from_snapshot(snapshot: Snapshot) -> Conversation {
        let Snapshot { conversation, turns, events, last_seq: _, queued, answering_elsewhere } = snapshot;
        let running = conversation.running;
        let rendered = turns.iter().map(parse).collect();
        let mut c = Conversation {
            summary: conversation,
            turns,
            rendered,
            activity: Vec::new(),
            live: None,
            live_text: String::new(),
            dropped: 0,
            last_seq: 0,
            queued,
            elsewhere: answering_elsewhere,
            running,
            todos: Vec::new(),
            images: std::collections::HashMap::new(),
        };
        // The snapshot's record is applied like a batch, so a follow from `last_seq` continues exactly where it ends.
        c.apply(&events);
        c
    }

    pub fn id(&self) -> &str {
        &self.summary.id
    }

    /// The title of the task `task` the agent suggested here, as its chip shows it.
    pub fn task_title(&self, task: &str) -> Option<&str> {
        self.activity.iter().flat_map(|a| a.items.iter()).find_map(|item| match item {
            Item::Task { task: id, title, .. } if id == task => Some(title.as_str()),
            _ => None,
        })
    }

    /// The parsed text of `turns[index]`, when it is an answer.
    pub fn rendered(&self, index: usize) -> Option<&markdown::Content> {
        self.rendered.get(index).and_then(Option::as_ref)
    }

    /// A question this window sent was accepted: show it at once.
    pub fn asked(&mut self, turn: ChatTurn) {
        if !self.turns.iter().any(|t| t.id == turn.id) {
            self.rendered.push(parse(&turn));
            self.turns.push(turn);
        }
        self.running = true;
    }

    /// Apply a batch of events, skipping any already applied.
    pub fn apply(&mut self, events: &[ConversationEvent]) {
        for event in events {
            if event.seq <= self.last_seq {
                continue;
            }
            self.last_seq = event.seq;
            self.one(&event.kind, event.at);
        }
    }

    fn current(&mut self) -> &mut Activity {
        if self.activity.is_empty() {
            self.activity.push(Activity {
                turn: String::new(),
                label: String::new(),
                local: true,
                items: Vec::new(),
                stage: None,
                status: None,
                started: None,
            });
        }
        self.activity.last_mut().expect("just made")
    }

    fn tool(&mut self, call: &str) -> Option<&mut Tool> {
        self.activity.iter_mut().rev().flat_map(|a| a.items.iter_mut().rev()).find_map(|item| match item {
            Item::Tool(t) if t.call == call => Some(t),
            _ => None,
        })
    }

    fn one(&mut self, kind: &Ev, at: f64) {
        use lattice_protocol::Locality;
        match kind {
            Ev::TurnStarted { turn, label, locality, .. } => {
                self.activity.push(Activity {
                    turn: turn.clone(),
                    label: label.clone(),
                    local: *locality == Locality::Local,
                    items: Vec::new(),
                    stage: None,
                    status: None,
                    started: Some(at),
                });
                self.running = true;
                self.live = None;
                self.live_text.clear();
                self.dropped = 0;
            }
            Ev::Stage { stage, detail } => {
                let words = if detail.is_empty() {
                    match stage {
                        Stage::Thinking => "Thinking",
                        Stage::Writing => "Writing",
                        Stage::Checking => "Checking",
                        Stage::Done => "Done",
                    }
                    .to_string()
                } else {
                    detail.clone()
                };
                self.current().stage = Some(words);
            }
            Ev::Delta { text } => {
                self.live_text.push_str(text);
                match &mut self.live {
                    Some(content) => content.push_str(text),
                    None => self.live = Some(markdown::Content::parse(&self.live_text)),
                }
            }
            Ev::DeltasDropped { n } => self.dropped += n,
            Ev::ToolCall { call_id, tool, summary, target } => self.current().items.push(Item::Tool(Tool {
                call: call_id.clone(),
                tool: tool.clone(),
                summary: summary.clone(),
                target: target.clone(),
                output: None,
                withheld: false,
                truncated: false,
                progress: None,
                exit: None,
                effect: Vec::new(),
            })),
            Ev::ToolOutput { call_id, preview, withheld, truncated } => {
                if let Some(t) = self.tool(call_id) {
                    t.output = Some(preview.clone());
                    t.withheld = *withheld;
                    t.truncated = *truncated;
                }
            }
            Ev::Withheld { call_id } => {
                if let Some(t) = self.tool(call_id) {
                    t.withheld = true;
                }
            }
            Ev::ApprovalRequested { call_id, detail, allow_always_offer, .. } => self.current().items.push(Item::Approval {
                call: call_id.clone(),
                detail: detail.clone(),
                allow_always: *allow_always_offer,
                decided: None,
            }),
            Ev::ApprovalResolved { call_id, approved, by } => {
                for a in self.activity.iter_mut().rev() {
                    for item in a.items.iter_mut() {
                        if let Item::Approval { call, decided, .. } = item
                            && call == call_id
                        {
                            *decided = Some((*approved, by.clone()));
                        }
                    }
                }
            }
            Ev::Question { call_id, text, options } => self.current().items.push(Item::Question {
                call: call_id.clone(),
                text: text.clone(),
                options: options.clone(),
                answered: false,
            }),
            Ev::TaskSuggested { task, title, summary, prompt } => self.current().items.push(Item::Task {
                task: task.clone(),
                title: title.clone(),
                summary: summary.clone(),
                prompt: prompt.clone(),
                outcome: None,
            }),
            Ev::ArtifactSaved { name, title, kind, version, bytes } => self.current().items.push(Item::Artifact {
                name: name.clone(),
                title: title.clone(),
                kind: *kind,
                version: *version,
                bytes: *bytes,
            }),
            Ev::PlanProposed { plan, title, text } => self.current().items.push(Item::Plan {
                plan: plan.clone(),
                title: title.clone(),
                text: text.clone(),
                outcome: None,
            }),
            Ev::PlanSettled { plan: id, outcome: settled } => {
                // A plan settles once: the first outcome stands.
                for a in self.activity.iter_mut() {
                    for item in a.items.iter_mut() {
                        if let Item::Plan { plan, outcome, .. } = item
                            && plan == id
                            && outcome.is_none()
                        {
                            *outcome = Some(settled.clone());
                        }
                    }
                }
            }
            Ev::ImagesAttached { turn, count, .. } => {
                *self.images.entry(turn.clone()).or_default() += *count;
            }
            // The whole list each time: the latest replaces the one before.
            Ev::TodosUpdated { items } => self.todos = items.clone(),
            Ev::TaskSettled { task: id, outcome: settled } => {
                // A task settles once: the first outcome stands.
                for a in self.activity.iter_mut() {
                    for item in a.items.iter_mut() {
                        if let Item::Task { task, outcome, .. } = item
                            && task == id
                            && outcome.is_none()
                        {
                            *outcome = Some(settled.clone());
                        }
                    }
                }
            }
            Ev::QuestionAnswered { call_id } => {
                for a in self.activity.iter_mut() {
                    for item in a.items.iter_mut() {
                        if let Item::Question { call, answered, .. } = item
                            && call == call_id
                        {
                            *answered = true;
                        }
                    }
                }
            }
            Ev::Staged { change, path, added, removed, authority } => self.current().items.push(Item::Staged {
                change: change.clone(),
                path: path.clone(),
                added: *added,
                removed: *removed,
                authority: *authority,
                result: None,
            }),
            Ev::Reviewed { change: id, result: r } => {
                for a in self.activity.iter_mut() {
                    for item in a.items.iter_mut() {
                        if let Item::Staged { change, result, .. } = item
                            && change == id
                        {
                            *result = Some(r.clone());
                        }
                    }
                }
            }
            Ev::Conflict { path, reason, .. } => {
                self.current().items.push(Item::Conflict { path: path.clone(), reason: reason.clone() })
            }
            Ev::Checkpoint { .. } | Ev::Queued { .. } | Ev::Dequeued { .. } => {}
            Ev::CommandStarted { call_id, .. } => {
                if let Some(t) = self.tool(call_id) {
                    t.progress = Some((0, 0, String::new()));
                }
            }
            Ev::CommandProgress { call_id, bytes, lines, tail_preview } => {
                if let Some(t) = self.tool(call_id) {
                    t.progress = Some((*bytes, *lines, tail_preview.clone()));
                }
            }
            Ev::CommandExited { call_id, code, duration_ms, reason } => {
                if let Some(t) = self.tool(call_id) {
                    t.exit = Some((*code, *duration_ms, *reason));
                }
            }
            Ev::CommandEffect { call_id, files, .. } => {
                if let Some(t) = self.tool(call_id) {
                    t.effect = files.clone();
                }
            }
            Ev::Steered { text } => self.current().items.push(Item::Steered(text.clone())),
            Ev::Notice { text } => self.current().items.push(Item::Notice(text.clone())),
            Ev::Error { message } => self.current().items.push(Item::Error(message.clone())),
            Ev::TurnSaved { turn, .. } => {
                let turn = (**turn).clone();
                match self.turns.iter().position(|t| t.id == turn.id && !t.id.is_empty()) {
                    Some(at) => {
                        self.rendered[at] = parse(&turn);
                        self.turns[at] = turn;
                    }
                    None => {
                        self.rendered.push(parse(&turn));
                        self.turns.push(turn);
                    }
                }
                self.live = None;
                self.live_text.clear();
            }
            Ev::TurnEnded { status, .. } => {
                let current = self.current();
                current.status = Some(*status);
                current.stage = None;
                self.running = false;
                self.live = None;
                self.live_text.clear();
            }
        }
    }

    /// The activity recorded for `turn`, when there is any.
    pub fn activity_for(&self, turn: &str) -> impl Iterator<Item = &Activity> {
        self.activity.iter().filter(move |a| !a.turn.is_empty() && a.turn == turn)
    }

    /// Activity whose turn is not among the visible turns (an older turn, or one not saved yet): shown at the end.
    pub fn unplaced(&self) -> impl Iterator<Item = &Activity> {
        self.activity.iter().filter(|a| a.turn.is_empty() || !self.turns.iter().any(|t| t.id == a.turn))
    }

    /// Calls waiting for the reader: approvals not yet decided, questions not yet answered.
    pub fn waiting(&self) -> usize {
        self.activity
            .iter()
            .flat_map(|a| a.items.iter())
            .filter(|item| {
                matches!(item, Item::Approval { decided: None, .. } | Item::Question { answered: false, .. })
            })
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lattice_protocol::Locality;
    use lattice_protocol::conversation::{Mode, Origin, TurnKind};

    fn summary() -> ConversationSummary {
        ConversationSummary {
            id: "c1".into(),
            title: "t".into(),
            created: 1.0,
            updated: 1.0,
            turns: 0,
            pinned_provider: String::new(),
            workspace: None,
            mode: Mode::Agent,
            origin: Origin::Native,
            running: false,
            needs_you: false,
        }
    }

    fn turn(id: &str, role: Role, text: &str) -> ChatTurn {
        ChatTurn {
            id: id.into(),
            ts: 1.0,
            role,
            text: text.into(),
            tools: vec![],
            facts: vec![],
            unsupported: vec![],
            provider: String::new(),
            error: String::new(),
            constrained: false,
            truncated: false,
            cancelled: false,
            prompt_tokens: None,
            completion_tokens: None,
        }
    }

    fn ev(seq: u64, kind: Ev) -> ConversationEvent {
        ConversationEvent { seq, at: seq as f64, kind }
    }

    fn snapshot(events: Vec<ConversationEvent>) -> Snapshot {
        Snapshot {
            conversation: summary(),
            turns: vec![turn("u1", Role::User, "read a.txt")],
            last_seq: events.last().map(|e| e.seq).unwrap_or(0),
            events,
            queued: vec![],
            answering_elsewhere: false,
        }
    }

    fn started(seq: u64) -> ConversationEvent {
        ev(
            seq,
            Ev::TurnStarted {
                turn: "u1".into(),
                kind: TurnKind::Agent,
                mode: Mode::Agent,
                label: "Local".into(),
                locality: Locality::Local,
            },
        )
    }

    #[test]
    fn a_turn_streams_calls_a_tool_saves_and_ends() {
        let mut c = Conversation::from_snapshot(snapshot(vec![]));
        c.apply(&[
            started(1),
            ev(2, Ev::ToolCall { call_id: "k".into(), tool: "read_file".into(), summary: "Read a.txt".into(), target: Some("a.txt".into()) }),
            ev(3, Ev::ToolOutput { call_id: "k".into(), preview: "alpha".into(), withheld: false, truncated: false }),
            ev(4, Ev::Delta { text: "It says ".into() }),
            ev(5, Ev::Delta { text: "alpha.".into() }),
        ]);
        assert!(c.running);
        assert_eq!(c.live_text, "It says alpha.");
        let tool = match &c.activity_for("u1").next().unwrap().items[0] {
            Item::Tool(t) => t.clone(),
            other => panic!("{other:?}"),
        };
        assert_eq!((tool.tool.as_str(), tool.output.as_deref()), ("read_file", Some("alpha")));
        c.apply(&[
            ev(6, Ev::TurnSaved { turn: Box::new(turn("a1", Role::Assistant, "It says alpha.")), saved: true }),
            ev(7, Ev::TurnEnded { turn: "u1".into(), status: TurnStatus::Completed }),
        ]);
        assert!(!c.running && c.live.is_none() && c.live_text.is_empty());
        assert_eq!(c.turns.len(), 2);
        assert!(c.rendered(1).is_some() && c.rendered(0).is_none());
        assert_eq!(c.activity_for("u1").next().unwrap().status, Some(TurnStatus::Completed));
        // The turn keeps the time its start event carried.
        assert_eq!(c.activity_for("u1").next().unwrap().started, Some(1.0));
    }

    #[test]
    fn a_batch_seen_before_is_not_applied_twice() {
        let first = vec![started(1), ev(2, Ev::Notice { text: "once".into() })];
        let mut c = Conversation::from_snapshot(snapshot(first.clone()));
        c.apply(&first);
        c.apply(&[ev(2, Ev::Notice { text: "once".into() }), ev(3, Ev::Notice { text: "twice".into() })]);
        let notices: Vec<_> = c.activity.iter().flat_map(|a| a.items.iter()).collect();
        assert_eq!(notices, vec![&Item::Notice("once".into()), &Item::Notice("twice".into())]);
        assert_eq!(c.last_seq, 3);
    }

    #[test]
    fn an_approval_waits_until_it_is_decided_and_a_question_until_it_is_answered() {
        let mut c = Conversation::from_snapshot(snapshot(vec![started(1)]));
        c.apply(&[
            ev(
                2,
                Ev::ApprovalRequested {
                    call_id: "r".into(),
                    kind: lattice_protocol::conversation::ApprovalKind::Command,
                    detail: ApprovalDetail::Command {
                        text: "git status".into(),
                        cwd: String::new(),
                        mode: lattice_protocol::conversation::CommandMode::Direct,
                        timeout_s: 60,
                        remote: None,
                        staged_waiting: 0,
                        background: false,
                    },
                    allow_always_offer: true,
                },
            ),
            ev(3, Ev::Question { call_id: "q".into(), text: "Which file?".into(), options: vec![] }),
        ]);
        assert_eq!(c.waiting(), 2);
        c.apply(&[ev(4, Ev::ApprovalResolved { call_id: "r".into(), approved: true, by: DecidedBy::Reader })]);
        assert_eq!(c.waiting(), 1);
        c.apply(&[ev(5, Ev::QuestionAnswered { call_id: "q".into() })]);
        assert_eq!(c.waiting(), 0);
    }

    #[test]
    fn a_command_reports_progress_and_exit_on_its_tool_card() {
        let mut c = Conversation::from_snapshot(snapshot(vec![started(1)]));
        c.apply(&[
            ev(2, Ev::ToolCall { call_id: "x".into(), tool: "run_command".into(), summary: "git status".into(), target: None }),
            ev(3, Ev::CommandStarted { call_id: "x".into(), mode: lattice_protocol::conversation::CommandMode::Direct }),
            ev(4, Ev::CommandProgress { call_id: "x".into(), bytes: 40, lines: 2, tail_preview: "clean".into() }),
            ev(5, Ev::CommandExited { call_id: "x".into(), code: Some(0), duration_ms: 120, reason: ExitReason::Exited }),
        ]);
        let Item::Tool(t) = &c.activity[0].items[0] else { panic!() };
        assert_eq!(t.progress, Some((40, 2, "clean".into())));
        assert_eq!(t.exit, Some((Some(0), 120, ExitReason::Exited)));
    }

    #[test]
    fn a_suggested_task_waits_on_its_chip_and_settles_once() {
        let task = "task_0011223344556677";
        let mut c = Conversation::from_snapshot(snapshot(vec![started(1)]));
        c.apply(&[ev(
            2,
            Ev::TaskSuggested {
                task: task.into(),
                title: "Fix the stale README badge".into(),
                summary: "The badge names the old workflow.".into(),
                prompt: "Point the badge in README.md at build.yml.".into(),
            },
        )]);
        let chip = |c: &Conversation| match &c.activity[0].items[0] {
            Item::Task { task, title, prompt, outcome, .. } => (task.clone(), title.clone(), prompt.clone(), outcome.clone()),
            other => panic!("{other:?}"),
        };
        assert_eq!(
            chip(&c),
            (task.into(), "Fix the stale README badge".into(), "Point the badge in README.md at build.yml.".into(), None)
        );
        assert_eq!(c.waiting(), 0, "a chip holds the agent up for nothing");
        // Settled after its turn ended (Start is pressed later): the first outcome stands.
        let started = TaskOutcome::Started { conversation: "c2".into() };
        c.apply(&[
            ev(3, Ev::TurnEnded { turn: "u1".into(), status: TurnStatus::Completed }),
            ev(4, Ev::TaskSettled { task: task.into(), outcome: started.clone() }),
            ev(5, Ev::TaskSettled { task: task.into(), outcome: TaskOutcome::Dismissed }),
            ev(6, Ev::TaskSettled { task: "task_ffffffffffffffff".into(), outcome: TaskOutcome::Dismissed }),
        ]);
        assert_eq!(chip(&c).3, Some(started));
        assert_eq!(c.activity.iter().map(|a| a.items.len()).sum::<usize>(), 1);
    }

    #[test]
    fn each_saved_artifact_version_is_a_card_of_its_turn() {
        let mut c = Conversation::from_snapshot(snapshot(vec![started(1)]));
        for (seq, version) in [(2, 1), (3, 2)] {
            c.apply(&[ev(
                seq,
                Ev::ArtifactSaved {
                    name: "plan".into(),
                    title: "The plan".into(),
                    kind: ArtifactKind::Markdown,
                    version,
                    bytes: 19,
                },
            )]);
        }
        let cards: Vec<(String, u32)> = c.activity[0]
            .items
            .iter()
            .filter_map(|item| match item {
                Item::Artifact { name, version, .. } => Some((name.clone(), *version)),
                _ => None,
            })
            .collect();
        assert_eq!(cards, [("plan".to_string(), 1), ("plan".to_string(), 2)]);
    }

    #[test]
    fn the_latest_todo_list_replaces_the_one_before_and_an_empty_one_clears_it() {
        use lattice_protocol::conversation::TodoStatus;
        let step = |content: &str, status| TodoItem { content: content.into(), status };
        let mut c = Conversation::from_snapshot(snapshot(vec![started(1)]));
        assert!(c.todos.is_empty());
        c.apply(&[ev(2, Ev::TodosUpdated { items: vec![step("Read", TodoStatus::InProgress), step("Fix", TodoStatus::Pending)] })]);
        c.apply(&[ev(3, Ev::TodosUpdated { items: vec![step("Read", TodoStatus::Done), step("Fix", TodoStatus::InProgress)] })]);
        assert_eq!(c.todos, [step("Read", TodoStatus::Done), step("Fix", TodoStatus::InProgress)]);
        c.apply(&[ev(4, Ev::TodosUpdated { items: vec![] })]);
        assert!(c.todos.is_empty(), "an empty list clears it");
    }

    #[test]
    fn a_proposed_plan_waits_on_its_card_and_settles_once() {
        let mut c = Conversation::from_snapshot(snapshot(vec![started(1)]));
        let plan = "plan_0011223344556677".to_string();
        c.apply(&[ev(2, Ev::PlanProposed { plan: plan.clone(), title: "The plan".into(), text: "1. Read.".into() })]);
        let outcome = |c: &Conversation| {
            c.activity[0].items.iter().find_map(|item| match item {
                Item::Plan { outcome, .. } => Some(outcome.clone()),
                _ => None,
            })
        };
        assert_eq!(outcome(&c), Some(None), "it waits");
        c.apply(&[ev(3, Ev::PlanSettled { plan: plan.clone(), outcome: PlanOutcome::Approved })]);
        c.apply(&[ev(4, Ev::PlanSettled { plan, outcome: PlanOutcome::KeptPlanning })]);
        assert_eq!(outcome(&c), Some(Some(PlanOutcome::Approved)), "the first outcome stands");
    }

    #[test]
    fn the_images_attached_to_a_turn_are_counted_with_it() {
        let mut c = Conversation::from_snapshot(snapshot(vec![started(1)]));
        c.apply(&[ev(2, Ev::ImagesAttached { turn: "u1".into(), count: 2, bytes: 900 })]);
        assert_eq!(c.images.get("u1"), Some(&2));
        assert_eq!(c.images.get("u2"), None);
    }

    #[test]
    fn a_question_sent_here_shows_at_once_and_only_once() {
        let mut c = Conversation::from_snapshot(snapshot(vec![]));
        c.asked(turn("u2", Role::User, "and b.txt?"));
        c.asked(turn("u2", Role::User, "and b.txt?"));
        assert_eq!(c.turns.len(), 2);
        assert!(c.running);
    }
}
