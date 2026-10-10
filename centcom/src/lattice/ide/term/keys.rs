//! What a key sends to the program in the terminal: the bytes xterm sends, which ConPTY turns into the console's own
//! key events for a Windows program (PowerShell, cmd) and passes on unchanged to a VT one. Pure.

/// A key, as the window read it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key<'a> {
    /// The text a key produced (already with Shift applied), with Ctrl not held.
    Text(&'a str),
    /// A letter, digit or symbol pressed with Ctrl (the key's own character, without Shift).
    Ctrl(char),
    Enter,
    Backspace,
    Tab,
    Escape,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Insert,
    Delete,
    /// F1 to F12.
    F(u8),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mods {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
}

impl Mods {
    /// xterm's modifier parameter: 1 + Shift + 2·Alt + 4·Ctrl.
    fn param(self) -> u8 {
        1 + u8::from(self.shift) + 2 * u8::from(self.alt) + 4 * u8::from(self.ctrl)
    }

    fn any(self) -> bool {
        self.shift || self.ctrl || self.alt
    }
}

/// The bytes for `key`; `application_cursor` is the program's DECCKM mode (arrows as `ESC O A`).
pub fn encode(key: Key, mods: Mods, application_cursor: bool) -> Option<Vec<u8>> {
    let alt = |mut bytes: Vec<u8>| {
        if mods.alt {
            bytes.insert(0, 0x1B);
        }
        bytes
    };
    let cursor = |letter: u8| -> Vec<u8> {
        if mods.any() {
            format!("\x1b[1;{}{}", mods.param(), letter as char).into_bytes()
        } else if application_cursor {
            vec![0x1B, b'O', letter]
        } else {
            vec![0x1B, b'[', letter]
        }
    };
    let tilde = |n: u8| -> Vec<u8> {
        if mods.any() { format!("\x1b[{n};{}~", mods.param()).into_bytes() } else { format!("\x1b[{n}~").into_bytes() }
    };
    Some(match key {
        Key::Text(text) => {
            let clean: String = text.chars().filter(|c| !c.is_control()).collect();
            if clean.is_empty() {
                return None;
            }
            alt(clean.into_bytes())
        }
        Key::Ctrl(c) => {
            let code = match c.to_ascii_lowercase() {
                c @ 'a'..='z' => c as u8 - b'a' + 1,
                '@' | ' ' | '2' => 0x00,
                '[' | '3' => 0x1B,
                '\\' | '4' => 0x1C,
                ']' | '5' => 0x1D,
                '^' | '6' => 0x1E,
                '_' | '-' | '/' | '7' => 0x1F,
                '8' | '?' => 0x7F,
                _ => return None,
            };
            alt(vec![code])
        }
        Key::Enter => alt(vec![b'\r']),
        Key::Backspace => alt(vec![if mods.ctrl { 0x08 } else { 0x7F }]),
        Key::Tab if mods.shift => b"\x1b[Z".to_vec(),
        Key::Tab => alt(vec![b'\t']),
        Key::Escape => vec![0x1B],
        Key::Up => cursor(b'A'),
        Key::Down => cursor(b'B'),
        Key::Right => cursor(b'C'),
        Key::Left => cursor(b'D'),
        Key::Home => cursor(b'H'),
        Key::End => cursor(b'F'),
        Key::Insert => tilde(2),
        Key::Delete => tilde(3),
        Key::PageUp => tilde(5),
        Key::PageDown => tilde(6),
        Key::F(n @ 1..=4) => {
            let letter = b"PQRS"[usize::from(n - 1)];
            if mods.any() { format!("\x1b[1;{}{}", mods.param(), letter as char).into_bytes() } else { vec![0x1B, b'O', letter] }
        }
        Key::F(n @ 5..=12) => tilde([15, 17, 18, 19, 20, 21, 23, 24][usize::from(n - 5)]),
        Key::F(_) => return None,
    })
}

/// The bytes for pasting `text`: line ends as Enter (`\r`), other control characters left out (so a paste cannot
/// press keys it does not show, nor end a bracketed paste early), and the bracket markers around it when the program
/// asked for them.
pub fn paste(text: &str, bracketed: bool) -> Vec<u8> {
    let normal = text.replace("\r\n", "\n");
    let clean: String = normal.chars().filter(|c| *c == '\n' || *c == '\t' || !c.is_control()).map(|c| if c == '\n' { '\r' } else { c }).collect();
    let mut out = Vec::with_capacity(clean.len() + 12);
    if bracketed {
        out.extend_from_slice(b"\x1b[200~");
    }
    out.extend_from_slice(clean.as_bytes());
    if bracketed {
        out.extend_from_slice(b"\x1b[201~");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const NONE: Mods = Mods { shift: false, ctrl: false, alt: false };

    fn ctrl() -> Mods {
        Mods { ctrl: true, ..NONE }
    }

    #[test]
    fn text_control_keys_and_alt_send_what_xterm_sends() {
        assert_eq!(encode(Key::Text("aé"), NONE, false), Some("aé".as_bytes().to_vec()));
        assert_eq!(encode(Key::Text("x"), Mods { alt: true, ..NONE }, false), Some(b"\x1bx".to_vec()));
        assert_eq!(encode(Key::Ctrl('c'), ctrl(), false), Some(vec![0x03]));
        assert_eq!(encode(Key::Ctrl('Z'), ctrl(), false), Some(vec![0x1A]));
        assert_eq!(encode(Key::Ctrl('['), ctrl(), false), Some(vec![0x1B]));
        assert_eq!(encode(Key::Ctrl(' '), ctrl(), false), Some(vec![0x00]));
        assert_eq!(encode(Key::Ctrl('é'), ctrl(), false), None);
        assert_eq!(encode(Key::Enter, NONE, false), Some(b"\r".to_vec()));
        assert_eq!(encode(Key::Backspace, NONE, false), Some(vec![0x7F]));
        assert_eq!(encode(Key::Backspace, ctrl(), false), Some(vec![0x08]));
        assert_eq!(encode(Key::Tab, Mods { shift: true, ..NONE }, false), Some(b"\x1b[Z".to_vec()));
        assert_eq!(encode(Key::Text("\u{7}"), NONE, false), None, "a control character is never typed as text");
    }

    #[test]
    fn cursor_and_editing_keys_with_and_without_modifiers() {
        assert_eq!(encode(Key::Up, NONE, false), Some(b"\x1b[A".to_vec()));
        assert_eq!(encode(Key::Up, NONE, true), Some(b"\x1bOA".to_vec()));
        assert_eq!(encode(Key::Left, ctrl(), true), Some(b"\x1b[1;5D".to_vec()));
        assert_eq!(encode(Key::End, Mods { shift: true, ..NONE }, false), Some(b"\x1b[1;2F".to_vec()));
        assert_eq!(encode(Key::Delete, NONE, false), Some(b"\x1b[3~".to_vec()));
        assert_eq!(encode(Key::PageUp, Mods { alt: true, ..NONE }, false), Some(b"\x1b[5;3~".to_vec()));
        assert_eq!(encode(Key::F(1), NONE, false), Some(b"\x1bOP".to_vec()));
        assert_eq!(encode(Key::F(5), NONE, false), Some(b"\x1b[15~".to_vec()));
        assert_eq!(encode(Key::F(12), ctrl(), false), Some(b"\x1b[24;5~".to_vec()));
        assert_eq!(encode(Key::F(13), NONE, false), None);
    }

    #[test]
    fn a_paste_is_typed_as_shown_and_cannot_end_its_brackets_early() {
        assert_eq!(paste("a\r\nb\nc", false), b"a\rb\rc".to_vec());
        assert_eq!(paste("ls\x1b[201~; rm x", true), b"\x1b[200~ls[201~; rm x\x1b[201~".to_vec());
        assert_eq!(paste("x\u{8}\u{3}y\tz", false), b"xy\tz".to_vec());
    }
}
