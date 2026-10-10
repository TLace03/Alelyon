//! An open file's text as the editor holds it: how it was decoded (UTF-8, with or without a byte-order mark, and its
//! line ends), how it is encoded again on Save so nothing but the edit changes, and an undo history (iced's editor
//! keeps none).
//!
//! Only UTF-8 text is edited (a binary file, or one that is not UTF-8, is refused): a
//! NUL byte or a byte sequence that is not UTF-8 makes the file read-only, with the reason said.

/// The most bytes a file may have to be edited; a larger one opens read-only, a page at a time.
pub const EDIT_LIMIT: usize = 1 << 20;
/// The most lines a file may have to be edited.
pub const EDIT_LINES: usize = 20_000;

/// How a file ends its lines.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Ending {
    #[default]
    Lf,
    CrLf,
    /// Both: the text is written back with each line's own end, as the editor keeps it.
    Mixed,
}

impl Ending {
    pub fn name(self) -> &'static str {
        match self {
            Ending::Lf => "LF",
            Ending::CrLf => "CRLF",
            Ending::Mixed => "Mixed",
        }
    }
}

/// A file's bytes, decoded for editing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decoded {
    /// The text without its byte-order mark.
    pub text: String,
    pub bom: bool,
    pub ending: Ending,
    pub lines: usize,
}

/// Why a file is not edited.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Undecodable {
    Binary,
    NotUtf8,
    TooLarge,
    TooManyLines,
}

impl Undecodable {
    pub fn sentence(self) -> &'static str {
        match self {
            Undecodable::Binary => "It looks like a binary file (it holds a NUL byte), so it is shown, not edited.",
            Undecodable::NotUtf8 => "It is not UTF-8 text, so it is shown, not edited.",
            Undecodable::TooLarge => "It is over 1 MiB, so it is shown a page at a time, not edited.",
            Undecodable::TooManyLines => "It has over 20,000 lines, so it is shown a page at a time, not edited.",
        }
    }
}

/// Decode `bytes` for editing.
pub fn decode(bytes: &[u8]) -> Result<Decoded, Undecodable> {
    if bytes.len() > EDIT_LIMIT {
        return Err(Undecodable::TooLarge);
    }
    if bytes.contains(&0) {
        return Err(Undecodable::Binary);
    }
    let (bom, body) = match bytes.strip_prefix(b"\xEF\xBB\xBF") {
        Some(body) => (true, body),
        None => (false, bytes),
    };
    let text = std::str::from_utf8(body).map_err(|_| Undecodable::NotUtf8)?.to_string();
    let lines = text.matches('\n').count() + 1;
    if lines > EDIT_LINES {
        return Err(Undecodable::TooManyLines);
    }
    Ok(Decoded { ending: ending_of(&text), text, bom, lines })
}

/// The line ends `text` uses: LF when it has none.
pub fn ending_of(text: &str) -> Ending {
    let crlf = text.matches("\r\n").count();
    let lf = text.matches('\n').count() - crlf;
    match (crlf, lf) {
        (0, _) => Ending::Lf,
        (_, 0) => Ending::CrLf,
        _ => Ending::Mixed,
    }
}

/// The bytes to write for `text`: every line end made the file's own when it used one kind (a line the editor added
/// takes the editor's end, which may differ), the byte-order mark put back when it had one.
pub fn encode(text: &str, bom: bool, ending: Ending) -> Vec<u8> {
    let body = match ending {
        Ending::Mixed => text.to_string(),
        Ending::Lf => text.replace("\r\n", "\n"),
        Ending::CrLf => text.replace("\r\n", "\n").replace('\n', "\r\n"),
    };
    let mut out = Vec::with_capacity(body.len() + 3);
    if bom {
        out.extend_from_slice(b"\xEF\xBB\xBF");
    }
    out.extend_from_slice(body.as_bytes());
    out
}

/// Where the cursor was, to put it back with a snapshot.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Caret {
    pub line: usize,
    pub column: usize,
}

/// What kind of edit began a step of the history, so typing a word is one step to undo and not one per letter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditKind {
    /// Letters, digits and `_`.
    Word,
    /// Spaces between words.
    Space,
    /// A new line, a paste, a deletion, an indent: always a step of its own.
    Other,
}

/// The undo history: snapshots of the whole text before each step, within a budget of bytes.
#[derive(Debug, Default)]
pub struct History {
    done: Vec<(String, Caret)>,
    undone: Vec<(String, Caret)>,
    /// The kind of the last edit, while the cursor has not moved since: an edit of the same kind joins its step.
    last: Option<EditKind>,
    bytes: usize,
}

/// The most bytes the history keeps; the oldest steps go first.
pub const HISTORY_BYTES: usize = 32 << 20;
/// The most steps it keeps.
pub const HISTORY_STEPS: usize = 500;

impl History {
    /// An edit of `kind` is about to change `before` (with the cursor at `caret`): keep a snapshot when it starts a
    /// new step.
    pub fn record(&mut self, before: &str, caret: Caret, kind: EditKind) {
        let joins = kind != EditKind::Other && self.last == Some(kind);
        // A space after a word keeps the word's step open, so "two words" is one step per word.
        let joins = joins || (kind == EditKind::Space && self.last == Some(EditKind::Word));
        self.last = Some(kind);
        self.undone.clear();
        if joins {
            return;
        }
        self.bytes += before.len();
        self.done.push((before.to_string(), caret));
        while (self.bytes > HISTORY_BYTES || self.done.len() > HISTORY_STEPS) && !self.done.is_empty() {
            let (gone, _) = self.done.remove(0);
            self.bytes -= gone.len();
        }
    }

    /// The cursor moved, or the file was saved: the next edit starts a step of its own.
    pub fn break_step(&mut self) {
        self.last = None;
    }

    /// Undo: the text to show instead of `now` (which can be redone).
    pub fn undo(&mut self, now: &str, caret: Caret) -> Option<(String, Caret)> {
        let (text, at) = self.done.pop()?;
        self.bytes -= text.len();
        self.undone.push((now.to_string(), caret));
        self.last = None;
        Some((text, at))
    }

    pub fn redo(&mut self, now: &str, caret: Caret) -> Option<(String, Caret)> {
        let (text, at) = self.undone.pop()?;
        self.bytes += now.len();
        self.done.push((now.to_string(), caret));
        self.last = None;
        Some((text, at))
    }

    pub fn can_undo(&self) -> bool {
        !self.done.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.undone.is_empty()
    }
}

/// The kind of edit an action is, for the history.
pub fn kind_of(edit: &iced::widget::text_editor::Edit) -> EditKind {
    use iced::widget::text_editor::Edit;
    match edit {
        Edit::Insert(c) if c.is_alphanumeric() || *c == '_' => EditKind::Word,
        Edit::Insert(' ') => EditKind::Space,
        _ => EditKind::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_is_decoded_and_encoded_back_byte_for_byte() {
        for bytes in [
            &b"fn main() {}\n"[..],
            b"a\r\nb\r\n",
            b"\xEF\xBB\xBFwith a mark\r\n",
            b"mixed\r\nends\nhere",
            b"",
            "ünïcödé ✓\n".as_bytes(),
        ] {
            let d = decode(bytes).unwrap();
            assert_eq!(encode(&d.text, d.bom, d.ending), bytes, "{bytes:?}");
        }
        let d = decode(b"\xEF\xBB\xBFx").unwrap();
        assert!(d.bom && d.text == "x");
    }

    #[test]
    fn a_line_the_editor_adds_takes_the_files_own_end() {
        let d = decode(b"one\r\ntwo\r\n").unwrap();
        assert_eq!(d.ending, Ending::CrLf);
        // The editor added "new" with a bare LF.
        assert_eq!(encode("one\r\nnew\ntwo\r\n", d.bom, d.ending), b"one\r\nnew\r\ntwo\r\n");
        assert_eq!(encode("a\r\nb\n", false, Ending::Lf), b"a\nb\n");
        assert_eq!(encode("a\r\nb\n", false, Ending::Mixed), b"a\r\nb\n");
    }

    #[test]
    fn binary_foreign_and_oversized_files_are_refused_for_editing() {
        assert_eq!(decode(b"PNG\0\x01"), Err(Undecodable::Binary));
        assert_eq!(decode(b"caf\xE9"), Err(Undecodable::NotUtf8));
        assert_eq!(decode(&vec![b'a'; EDIT_LIMIT + 1]), Err(Undecodable::TooLarge));
        assert_eq!(decode("\n".repeat(EDIT_LINES).as_bytes()), Err(Undecodable::TooManyLines));
        assert!(decode("\n".repeat(EDIT_LINES - 1).as_bytes()).is_ok());
        for refusal in [Undecodable::Binary, Undecodable::NotUtf8, Undecodable::TooLarge, Undecodable::TooManyLines] {
            assert!(refusal.sentence().ends_with('.'));
        }
    }

    #[test]
    fn typing_a_word_is_one_step_and_a_new_line_is_its_own() {
        let mut h = History::default();
        let c = Caret::default();
        h.record("", c, EditKind::Word);
        h.record("a", c, EditKind::Word);
        h.record("ab", c, EditKind::Space);
        h.record("ab ", c, EditKind::Word);
        h.record("ab c", c, EditKind::Other);
        assert_eq!(h.undo("ab c\n", c).unwrap().0, "ab c");
        assert_eq!(h.undo("ab c", c).unwrap().0, "ab ");
        assert_eq!(h.undo("ab ", c).unwrap().0, "");
        assert!(h.undo("", c).is_none());
        assert_eq!(h.redo("", c).unwrap().0, "ab ");
        // A new edit drops what could be redone.
        h.record("ab ", c, EditKind::Other);
        assert!(!h.can_redo());
    }

    #[test]
    fn a_moved_cursor_starts_a_new_step_and_the_budget_drops_the_oldest() {
        let mut h = History::default();
        let c = Caret::default();
        h.record("", c, EditKind::Word);
        h.break_step();
        h.record("x", c, EditKind::Word);
        assert_eq!(h.undo("xy", c).unwrap().0, "x");
        let mut h = History::default();
        for n in 0..HISTORY_STEPS + 10 {
            h.record(&n.to_string(), c, EditKind::Other);
        }
        assert_eq!(h.done.len(), HISTORY_STEPS);
        assert_eq!(h.done[0].0, "10");
    }
}
