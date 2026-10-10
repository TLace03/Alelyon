//! Where a chat is in its work, as its title banner shows it: the stages a change goes through (plan, build,
//! review, commit, pull request), which it has passed and which it is at, and the step of the agent's own task list
//! it is on. Read from what the chat recorded (its plan card, its edits and commands, the changes waiting for review,
//! its commits and pull requests, its to-do list); nothing here asks the model.

use lattice_protocol::conversation::{ChangeSet, TodoStatus};

use super::chat::{Conversation, Item};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Stage {
    Plan,
    Build,
    Review,
    Commit,
    PullRequest,
}

impl Stage {
    pub const ALL: [Stage; 5] = [Stage::Plan, Stage::Build, Stage::Review, Stage::Commit, Stage::PullRequest];

    pub fn words(self) -> &'static str {
        match self {
            Stage::Plan => "Plan",
            Stage::Build => "Build",
            Stage::Review => "Review",
            Stage::Commit => "Commit",
            Stage::PullRequest => "Pull request",
        }
    }
}

/// A stage as the banner marks it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    /// Not reached.
    Ahead,
    /// Passed.
    Done,
    /// Where the chat is now.
    Now,
}

/// The agent's task list: how many steps are done of how many, and the one in progress.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tasks {
    pub done: usize,
    pub total: usize,
    pub current: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Progress {
    pub stages: [(Stage, Mark); 5],
    pub tasks: Option<Tasks>,
}

impl Progress {
    /// The stage the chat is at, if it has begun any.
    pub fn now(&self) -> Option<Stage> {
        self.stages.iter().find(|(_, m)| *m == Mark::Now).map(|(s, _)| *s)
    }
}

/// The stage a step of a turn belongs to, if any.
fn stage_of(item: &Item) -> Option<Stage> {
    match item {
        Item::Plan { .. } => Some(Stage::Plan),
        Item::Staged { result: None, .. } => Some(Stage::Build),
        Item::Staged { result: Some(_), .. } => Some(Stage::Review),
        Item::Tool(t) => match t.tool.as_str() {
            "edit_file" | "write_file" | "delete_file" | "run_command" => Some(Stage::Build),
            "git_commit" => Some(Stage::Commit),
            "git_push_pr" => Some(Stage::PullRequest),
            _ => None,
        },
        _ => None,
    }
}

/// Where `c` is: every stage it reached is passed, and the one its newest step belongs to is where it is now, unless
/// changes wait for review (`changes`), which puts it at Review whatever came before.
pub fn progress(c: &Conversation, changes: Option<&ChangeSet>) -> Progress {
    let mut reached = [false; 5];
    let mut latest: Option<Stage> = None;
    if !c.todos.is_empty() {
        reached[Stage::Plan as usize] = true;
        latest = Some(Stage::Plan);
    }
    for activity in &c.activity {
        for item in &activity.items {
            if let Some(stage) = stage_of(item) {
                reached[stage as usize] = true;
                latest = Some(stage);
            }
        }
    }
    if changes.is_some_and(|s| s.waiting > 0) {
        reached[Stage::Review as usize] = true;
        latest = Some(Stage::Review);
    }
    let stages = Stage::ALL.map(|s| {
        let mark = if latest == Some(s) {
            Mark::Now
        } else if reached[s as usize] {
            Mark::Done
        } else {
            Mark::Ahead
        };
        (s, mark)
    });
    let tasks = (!c.todos.is_empty()).then(|| Tasks {
        done: c.todos.iter().filter(|t| t.status == TodoStatus::Done).count(),
        total: c.todos.len(),
        current: c.todos.iter().find(|t| t.status == TodoStatus::InProgress).map(|t| t.content.clone()),
    });
    Progress { stages, tasks }
}

#[cfg(test)]
mod tests {
    use lattice_protocol::conversation::{ConversationSummary, Mode, Origin, Snapshot, TodoItem};

    use super::super::chat::{Activity, Tool};
    use super::*;

    fn chat() -> Conversation {
        let conversation = ConversationSummary {
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
        };
        Conversation::from_snapshot(Snapshot { conversation, turns: vec![], events: vec![], last_seq: 0, queued: vec![], answering_elsewhere: false })
    }

    fn tool(name: &str) -> Item {
        Item::Tool(Tool {
            call: "k".into(),
            tool: name.into(),
            summary: name.into(),
            target: None,
            output: None,
            withheld: false,
            truncated: false,
            progress: None,
            exit: None,
            effect: vec![],
        })
    }

    fn steps(c: &mut Conversation, items: Vec<Item>) {
        c.activity.push(Activity { turn: "t".into(), label: String::new(), local: true, items, stage: None, status: None, started: None });
    }

    fn marks(p: &Progress) -> Vec<Mark> {
        p.stages.iter().map(|(_, m)| *m).collect()
    }

    #[test]
    fn a_chat_that_has_done_nothing_has_reached_no_stage() {
        let p = progress(&chat(), None);
        assert_eq!(p.now(), None);
        assert!(marks(&p).iter().all(|m| *m == Mark::Ahead));
        assert_eq!(p.tasks, None);
    }

    #[test]
    fn the_newest_step_is_where_it_is_and_what_came_before_is_passed() {
        let mut c = chat();
        steps(&mut c, vec![tool("read_file"), tool("edit_file"), tool("run_command")]);
        assert_eq!(progress(&c, None).now(), Some(Stage::Build));
        steps(&mut c, vec![tool("git_commit")]);
        let p = progress(&c, None);
        assert_eq!(marks(&p), [Mark::Ahead, Mark::Done, Mark::Ahead, Mark::Now, Mark::Ahead], "no plan, built, committed");
        steps(&mut c, vec![tool("git_push_pr")]);
        assert_eq!(progress(&c, None).now(), Some(Stage::PullRequest));
    }

    #[test]
    fn changes_waiting_for_review_put_it_at_review() {
        let mut c = chat();
        steps(&mut c, vec![tool("edit_file")]);
        let waiting = ChangeSet { changes: vec![], waiting: 2, command_running: false };
        let p = progress(&c, Some(&waiting));
        assert_eq!(p.now(), Some(Stage::Review));
        assert_eq!(p.stages[Stage::Build as usize].1, Mark::Done);
    }

    #[test]
    fn the_task_list_says_how_far_along_and_what_is_in_progress() {
        let mut c = chat();
        c.todos = vec![
            TodoItem { content: "Read the sorter".into(), status: TodoStatus::Done },
            TodoItem { content: "Write the test".into(), status: TodoStatus::InProgress },
            TodoItem { content: "Fix it".into(), status: TodoStatus::Pending },
        ];
        let p = progress(&c, None);
        assert_eq!(p.tasks, Some(Tasks { done: 1, total: 3, current: Some("Write the test".into()) }));
        assert_eq!(p.now(), Some(Stage::Plan), "a task list is a plan begun");
    }
}
