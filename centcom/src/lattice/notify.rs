//! What the person is told while they are in another window: a session asked them something (a question, an
//! approval, a decision), or a session that was working finished. Found at each read of the chat list, from the core's
//! attention calls and the chats' running flags; the app tells the person (`crate::notify`) only when none of its
//! windows has the focus.

use std::collections::HashSet;

use lattice_protocol::conversation::ChatList;

/// Why the person is told.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// It waits for them.
    Asks,
    /// It was working and finished.
    Done,
}

/// One thing to tell the person.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Note {
    pub conversation: String,
    pub title: String,
    pub kind: Kind,
}

impl Note {
    /// The notification's heading and line.
    pub fn words(&self) -> (String, String) {
        let title = if self.title.is_empty() { "A session" } else { self.title.as_str() };
        match self.kind {
            Kind::Asks => (format!("\u{201c}{title}\u{201d} needs you"), "It asks you something in Lattice.".to_string()),
            Kind::Done => (format!("\u{201c}{title}\u{201d} is done"), "It finished its work in Lattice.".to_string()),
        }
    }
}

/// The notes between two reads of the list: the conversations that asked (`asked`, from the core's attention), and
/// those running in the last read (`running_before`) that are not running now. One that asked is not also told as done.
pub fn notes(running_before: &HashSet<String>, list: &ChatList, asked: &[String]) -> Vec<Note> {
    let title = |id: &str| list.conversations.iter().find(|c| c.id == id).map(|c| c.title.clone()).unwrap_or_default();
    let mut out: Vec<Note> =
        asked.iter().map(|id| Note { conversation: id.clone(), title: title(id), kind: Kind::Asks }).collect();
    for c in &list.conversations {
        if !c.running && running_before.contains(&c.id) && !asked.contains(&c.id) {
            out.push(Note { conversation: c.id.clone(), title: c.title.clone(), kind: Kind::Done });
        }
    }
    out
}

/// The conversations running in `list`, for the next read.
pub fn running(list: &ChatList) -> HashSet<String> {
    list.conversations.iter().filter(|c| c.running).map(|c| c.id.clone()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use lattice_protocol::conversation::{ConversationSummary, Mode, Origin};

    fn chat(id: &str, running: bool) -> ConversationSummary {
        ConversationSummary {
            id: id.into(),
            title: format!("Topic {id}"),
            created: 0.0,
            updated: 0.0,
            turns: 2,
            pinned_provider: String::new(),
            workspace: None,
            mode: Mode::Agent,
            origin: Origin::Native,
            running,
            needs_you: false,
        }
    }

    fn list(chats: Vec<ConversationSummary>) -> ChatList {
        ChatList {
            conversations: chats,
            archived: vec![],
            store_dir: String::new(),
            installed: true,
            index_unreadable: false,
            archive_unreadable: false,
            shared_writes: true,
            limit: 60,
        }
    }

    /// Mutants: a finished chat not told; a chat that asked also told as done; a chat idle at the first read told
    /// as done; a chat still running told.
    #[test]
    fn who_asked_and_who_finished_between_two_reads() {
        let first = list(vec![chat("a", true), chat("b", true), chat("c", false), chat("d", true)]);
        assert!(notes(&HashSet::new(), &first, &[]).is_empty(), "nothing finished before the first read");
        let before = running(&first);
        let now = list(vec![chat("a", false), chat("b", false), chat("c", false), chat("d", true)]);
        let told = notes(&before, &now, &["b".to_string()]);
        assert_eq!(
            told.iter().map(|n| (n.conversation.as_str(), n.kind)).collect::<Vec<_>>(),
            [("b", Kind::Asks), ("a", Kind::Done)]
        );
        assert_eq!(told[0].words().0, "\u{201c}Topic b\u{201d} needs you");
        assert_eq!(told[1].words().0, "\u{201c}Topic a\u{201d} is done");
    }
}
