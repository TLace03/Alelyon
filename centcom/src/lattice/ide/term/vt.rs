//! A terminal's screen: the bytes a Windows pseudoconsole writes (VT, xterm's subset, as ConPTY speaks it) turned
//! into rows of styled cells, a bounded scrollback, a cursor and a title. Pure: no window, no process, no clock.
//!
//! What it understands: printable text (UTF-8, read across chunk boundaries; East Asian wide characters take two
//! cells, zero-width marks are dropped), the C0 controls a shell uses (BS, HT, LF, VT, FF, CR), the escape and CSI
//! sequences ConPTY sends (cursor moves and positions, erase in line and display, insert and delete of characters
//! and lines, scroll up and down, the scroll region, save and restore of the cursor, SGR colours and attributes in
//! 16, 256 and 24-bit colour, the modes for the cursor's visibility, application cursor keys, autowrap, the
//! alternate screen and bracketed paste), the device status and attribute queries (answered through
//! [`Screen::take_replies`], for the caller to write back), and OSC 0 and 2 (the title). Everything else is read
//! and ignored, never shown as text.
//!
//! Bounded against output that means harm or is merely large: at most [`SCROLLBACK`] lines are kept, parameters are
//! capped in count and size, a count never moves past the screen, an OSC string is cut at [`MAX_OSC`] bytes, and the
//! size is capped at [`MAX_COLS`] by [`MAX_ROWS`].

use std::collections::VecDeque;

/// The most lines kept above the screen.
pub const SCROLLBACK: usize = 5000;
pub const MAX_COLS: u16 = 500;
pub const MAX_ROWS: u16 = 300;
const MAX_PARAMS: usize = 32;
/// The most bytes of an OSC string kept.
pub const MAX_OSC: usize = 4096;
/// The most characters of a title kept.
const MAX_TITLE: usize = 256;

/// A colour as the program asked for it; the window maps it to its palette.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Colour {
    Default,
    /// One of the 256 indexed colours (0-15 the named ones).
    Index(u8),
    Rgb(u8, u8, u8),
}

/// How a cell is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Pen {
    pub fg: Colour,
    pub bg: Colour,
    pub bold: bool,
    pub dim: bool,
    pub italic: bool,
    pub underline: bool,
    pub inverse: bool,
}

impl Default for Pen {
    fn default() -> Pen {
        Pen { fg: Colour::Default, bg: Colour::Default, bold: false, dim: false, italic: false, underline: false, inverse: false }
    }
}

/// One cell: its character (`'\0'` for the second half of a wide character) and its pen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    pub ch: char,
    pub pen: Pen,
}

/// The second half of a wide character.
pub const WIDE_TAIL: char = '\0';

impl Default for Cell {
    fn default() -> Cell {
        Cell { ch: ' ', pen: Pen::default() }
    }
}

pub type Row = Vec<Cell>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Ground,
    Escape,
    /// An escape whose next byte is consumed and ignored (`ESC (`, `ESC #`, ...).
    EscapeSkip,
    Csi,
    Osc,
    OscEscape,
    /// A DCS, SOS, PM or APC string: ignored to its terminator.
    Ignore,
    IgnoreEscape,
}

/// The cells' width: 2 for East Asian wide and fullwidth characters and the common emoji, 0 for marks that combine
/// with the character before, 1 for everything else.
pub fn width(c: char) -> usize {
    let u = c as u32;
    if matches!(u, 0x0300..=0x036F | 0x0483..=0x0489 | 0x0591..=0x05BD | 0x0610..=0x061A | 0x064B..=0x065F
        | 0x200B..=0x200F | 0x20D0..=0x20FF | 0xFE00..=0xFE0F | 0xFE20..=0xFE2F | 0xE0100..=0xE01EF)
    {
        return 0;
    }
    if matches!(u, 0x1100..=0x115F | 0x2E80..=0x303E | 0x3041..=0x33FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF
        | 0xA000..=0xA4CF | 0xAC00..=0xD7A3 | 0xF900..=0xFAFF | 0xFE30..=0xFE4F | 0xFF00..=0xFF60 | 0xFFE0..=0xFFE6
        | 0x1F300..=0x1F64F | 0x1F900..=0x1F9FF | 0x20000..=0x2FFFD | 0x30000..=0x3FFFD)
    {
        return 2;
    }
    1
}

pub struct Screen {
    cols: u16,
    rows: u16,
    grid: Vec<Row>,
    scrollback: VecDeque<Row>,
    row: u16,
    col: u16,
    /// The cursor stands past the last column: the next character wraps first (autowrap's deferred wrap).
    wrap_pending: bool,
    saved: (u16, u16, Pen),
    pen: Pen,
    /// The scroll region, rows `top..=bottom`.
    top: u16,
    bottom: u16,
    pub cursor_visible: bool,
    pub application_cursor: bool,
    pub bracketed_paste: bool,
    autowrap: bool,
    /// The main screen's rows and cursor while the alternate screen is on.
    alternate: Option<(Vec<Row>, u16, u16)>,
    pub title: String,
    state: State,
    params: Vec<u32>,
    param: Option<u32>,
    private: Option<u8>,
    intermediate: Option<u8>,
    osc: Vec<u8>,
    utf8: Vec<u8>,
    utf8_need: usize,
    replies: Vec<u8>,
    /// Bumped whenever what is drawn changes, so a window draws again only then.
    pub generation: u64,
    /// Lines that ever went to the scrollback, those since dropped included: a line's number (see [`Screen::line`])
    /// stays the same while it scrolls, so a selection holds to its text.
    scrolled: u64,
}

impl Screen {
    pub fn new(cols: u16, rows: u16) -> Screen {
        let (cols, rows) = (cols.clamp(1, MAX_COLS), rows.clamp(1, MAX_ROWS));
        Screen {
            cols,
            rows,
            grid: vec![vec![Cell::default(); cols as usize]; rows as usize],
            scrollback: VecDeque::new(),
            row: 0,
            col: 0,
            wrap_pending: false,
            saved: (0, 0, Pen::default()),
            pen: Pen::default(),
            top: 0,
            bottom: rows - 1,
            cursor_visible: true,
            application_cursor: false,
            bracketed_paste: false,
            autowrap: true,
            alternate: None,
            title: String::new(),
            state: State::Ground,
            params: Vec::new(),
            param: None,
            private: None,
            intermediate: None,
            osc: Vec::new(),
            utf8: Vec::new(),
            utf8_need: 0,
            replies: Vec::new(),
            generation: 0,
            scrolled: 0,
        }
    }

    pub fn size(&self) -> (u16, u16) {
        (self.cols, self.rows)
    }

    pub fn cursor(&self) -> (u16, u16) {
        (self.row, self.col)
    }

    pub fn on_alternate_screen(&self) -> bool {
        self.alternate.is_some()
    }

    /// What the program asked to be told (cursor position reports, device attributes): write it back to its input.
    pub fn take_replies(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.replies)
    }

    /// The number of lines above the screen.
    pub fn scrollback_len(&self) -> usize {
        self.scrollback.len()
    }

    /// `count` lines ending `back` lines above the screen's last line (0: the screen itself), oldest first.
    pub fn lines(&self, back: usize, count: usize) -> Vec<&Row> {
        let (start, end) = self.window(back, count);
        (start..end)
            .map(|i| if i < self.scrollback.len() { &self.scrollback[i] } else { &self.grid[i - self.scrollback.len()] })
            .collect()
    }

    /// Where `lines(back, count)` starts and ends among the scrollback's lines and then the screen's.
    fn window(&self, back: usize, count: usize) -> (usize, usize) {
        let total = self.scrollback.len() + self.grid.len();
        let end = total.saturating_sub(back.min(self.scrollback.len()));
        (end.saturating_sub(count), end)
    }

    /// Lines that ever went to the scrollback (a count that only grows).
    pub fn scrolled(&self) -> u64 {
        self.scrolled
    }

    /// The number of the first line `lines(back, count)` returns: lines are numbered from the first ever written,
    /// and a line keeps its number while it scrolls.
    pub fn number_of_first(&self, back: usize, count: usize) -> u64 {
        self.first_kept() + self.window(back, count).0 as u64
    }

    /// The number of the oldest line kept.
    fn first_kept(&self) -> u64 {
        self.scrolled.saturating_sub(self.scrollback.len() as u64)
    }

    /// The line numbered `n`, while it is kept.
    pub fn line(&self, n: u64) -> Option<&Row> {
        let i = usize::try_from(n.checked_sub(self.first_kept())?).ok()?;
        if i < self.scrollback.len() { self.scrollback.get(i) } else { self.grid.get(i - self.scrollback.len()) }
    }

    /// The text from cell `from` to cell `to`, both included, as (line number, column) in either order: each line's
    /// trailing blanks cut, lines joined by CR LF (as Windows' clipboard takes them); lines no longer kept are left
    /// out.
    pub fn text_between(&self, from: (u64, u16), to: (u64, u16)) -> String {
        let (a, b) = if from <= to { (from, to) } else { (to, from) };
        let mut out: Vec<String> = Vec::new();
        for n in a.0..=b.0 {
            let Some(row) = self.line(n) else { continue };
            let start = if n == a.0 { usize::from(a.1).min(row.len()) } else { 0 };
            let end = if n == b.0 { (usize::from(b.1) + 1).min(row.len()) } else { row.len() };
            let piece: String = row[start..end.max(start)].iter().filter(|c| c.ch != WIDE_TAIL).map(|c| c.ch).collect();
            out.push(piece.trim_end().to_string());
        }
        out.join("\r\n")
    }

    /// Every line's text, the scrollback's then the screen's, trailing blanks cut and trailing empty lines dropped.
    #[cfg(test)]
    pub fn text(&self) -> String {
        let mut lines: Vec<String> = self.scrollback.iter().chain(self.grid.iter()).map(|r| row_text(r)).collect();
        while lines.last().is_some_and(|l| l.is_empty()) {
            lines.pop();
        }
        lines.join("\n")
    }

    /// The screen's size changed: rows and columns are cut or added (no reflow), the cursor stays on the screen, rows
    /// cut from the top go to the scrollback.
    pub fn resize(&mut self, cols: u16, rows: u16) {
        let (cols, rows) = (cols.clamp(1, MAX_COLS), rows.clamp(1, MAX_ROWS));
        if (cols, rows) == (self.cols, self.rows) {
            return;
        }
        for r in &mut self.grid {
            r.resize(cols as usize, Cell::default());
            fix_cut_wide(r);
        }
        while self.grid.len() > rows as usize {
            // Rows above the cursor leave from the top; below it, from the bottom.
            if self.row > 0 {
                let gone = self.grid.remove(0);
                self.push_scrollback(gone);
                self.row -= 1;
            } else {
                self.grid.pop();
            }
        }
        while self.grid.len() < rows as usize {
            self.grid.push(vec![Cell::default(); cols as usize]);
        }
        if let Some((main, _, _)) = &mut self.alternate {
            for r in main.iter_mut() {
                r.resize(cols as usize, Cell::default());
            }
            main.resize(rows as usize, vec![Cell::default(); cols as usize]);
        }
        self.cols = cols;
        self.rows = rows;
        self.top = 0;
        self.bottom = rows - 1;
        self.row = self.row.min(rows - 1);
        self.col = self.col.min(cols - 1);
        self.wrap_pending = false;
        self.generation += 1;
    }

    /// Read what the program wrote.
    pub fn feed(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        for &b in bytes {
            self.byte(b);
        }
        self.generation += 1;
    }

    fn push_scrollback(&mut self, row: Row) {
        if self.alternate.is_some() {
            return;
        }
        self.scrollback.push_back(row);
        self.scrolled += 1;
        while self.scrollback.len() > SCROLLBACK {
            self.scrollback.pop_front();
        }
    }

    fn blank(&self) -> Cell {
        // Erased cells keep the pen's background, as xterm erases.
        Cell { ch: ' ', pen: Pen { bg: self.pen.bg, ..Pen::default() } }
    }

    fn blank_row(&self) -> Row {
        vec![self.blank(); self.cols as usize]
    }

    fn byte(&mut self, b: u8) {
        match self.state {
            State::Ground => self.ground(b),
            State::Escape => self.escape(b),
            State::EscapeSkip => self.state = State::Ground,
            State::Csi => self.csi(b),
            State::Osc => match b {
                0x07 => self.end_osc(),
                0x1B => self.state = State::OscEscape,
                0x18 | 0x1A => self.state = State::Ground,
                _ => {
                    if self.osc.len() < MAX_OSC {
                        self.osc.push(b);
                    }
                }
            },
            State::OscEscape => {
                if b == b'\\' {
                    self.end_osc();
                } else {
                    self.state = State::Escape;
                    self.osc.clear();
                    self.escape(b);
                }
            }
            State::Ignore => match b {
                0x1B => self.state = State::IgnoreEscape,
                0x07 | 0x18 | 0x1A => self.state = State::Ground,
                _ => {}
            },
            State::IgnoreEscape => self.state = if b == b'\\' { State::Ground } else { State::Ignore },
        }
    }

    fn ground(&mut self, b: u8) {
        if self.utf8_need > 0 {
            if b & 0xC0 == 0x80 {
                self.utf8.push(b);
                if self.utf8.len() == self.utf8_need {
                    let c = std::str::from_utf8(&self.utf8).ok().and_then(|s| s.chars().next()).unwrap_or('\u{FFFD}');
                    self.utf8.clear();
                    self.utf8_need = 0;
                    self.print(c);
                }
                return;
            }
            // A sequence cut short: what came is one replacement character, and this byte is read afresh.
            self.utf8.clear();
            self.utf8_need = 0;
            self.print('\u{FFFD}');
        }
        match b {
            0x1B => {
                self.state = State::Escape;
            }
            0x00..=0x1F | 0x7F => self.control(b),
            0x20..=0x7E => self.print(b as char),
            0xC2..=0xDF => self.start_utf8(b, 2),
            0xE0..=0xEF => self.start_utf8(b, 3),
            0xF0..=0xF4 => self.start_utf8(b, 4),
            _ => self.print('\u{FFFD}'),
        }
    }

    fn start_utf8(&mut self, b: u8, need: usize) {
        self.utf8.clear();
        self.utf8.push(b);
        self.utf8_need = need;
    }

    fn control(&mut self, b: u8) {
        match b {
            0x08 => {
                self.wrap_pending = false;
                self.col = self.col.saturating_sub(1);
            }
            0x09 => {
                self.wrap_pending = false;
                self.col = ((self.col / 8 + 1) * 8).min(self.cols - 1);
            }
            0x0A..=0x0C => self.line_feed(),
            0x0D => {
                self.wrap_pending = false;
                self.col = 0;
            }
            _ => {}
        }
    }

    fn print(&mut self, c: char) {
        let w = width(c);
        if w == 0 {
            return;
        }
        if self.wrap_pending && self.autowrap {
            self.col = 0;
            self.line_feed();
        }
        self.wrap_pending = false;
        if w == 2 && self.col + 1 >= self.cols {
            // A wide character does not fit at the line's end: it goes to the next line.
            if self.autowrap {
                let pad = self.blank();
                self.grid[self.row as usize][self.col as usize] = pad;
                self.col = 0;
                self.line_feed();
            } else {
                return;
            }
        }
        let pen = self.pen;
        let (r, c0) = (self.row as usize, self.col as usize);
        {
            let row = &mut self.grid[r];
            // Writing over half of a wide character blanks its other half.
            if row[c0].ch == WIDE_TAIL && c0 > 0 {
                row[c0 - 1].ch = ' ';
            }
            if c0 + 1 < row.len() && row[c0 + 1].ch == WIDE_TAIL && w == 1 {
                row[c0 + 1].ch = ' ';
            }
            row[c0] = Cell { ch: c, pen };
            if w == 2 {
                row[c0 + 1] = Cell { ch: WIDE_TAIL, pen };
            }
        }
        let next = self.col + w as u16;
        if next >= self.cols {
            self.col = self.cols - 1;
            self.wrap_pending = self.autowrap;
        } else {
            self.col = next;
        }
    }

    fn line_feed(&mut self) {
        self.wrap_pending = false;
        if self.row == self.bottom {
            self.scroll_up(1);
        } else if self.row + 1 < self.rows {
            self.row += 1;
        }
    }

    fn reverse_index(&mut self) {
        self.wrap_pending = false;
        if self.row == self.top {
            self.scroll_down(1);
        } else {
            self.row = self.row.saturating_sub(1);
        }
    }

    /// Scroll the region up by `n`: its top rows leave (to the scrollback when the region is the screen's top).
    fn scroll_up(&mut self, n: u16) {
        let (top, bottom) = (self.top as usize, self.bottom as usize);
        let n = (n as usize).min(bottom + 1 - top);
        for _ in 0..n {
            let gone = self.grid.remove(top);
            if top == 0 {
                self.push_scrollback(gone);
            }
            let fill = self.blank_row();
            self.grid.insert(bottom, fill);
        }
    }

    fn scroll_down(&mut self, n: u16) {
        let (top, bottom) = (self.top as usize, self.bottom as usize);
        let n = (n as usize).min(bottom + 1 - top);
        for _ in 0..n {
            self.grid.remove(bottom);
            let fill = self.blank_row();
            self.grid.insert(top, fill);
        }
    }

    fn escape(&mut self, b: u8) {
        self.state = State::Ground;
        match b {
            b'[' => {
                self.state = State::Csi;
                self.params.clear();
                self.param = None;
                self.private = None;
                self.intermediate = None;
            }
            b']' => {
                self.state = State::Osc;
                self.osc.clear();
            }
            b'P' | b'X' | b'^' | b'_' => self.state = State::Ignore,
            b'7' => self.saved = (self.row, self.col, self.pen),
            b'8' => {
                let (r, c, p) = self.saved;
                self.row = r.min(self.rows - 1);
                self.col = c.min(self.cols - 1);
                self.pen = p;
                self.wrap_pending = false;
            }
            b'D' => self.line_feed(),
            b'E' => {
                self.col = 0;
                self.line_feed();
            }
            b'M' => self.reverse_index(),
            b'c' => {
                let (cols, rows, generation, scrolled) = (self.cols, self.rows, self.generation, self.scrolled);
                let scrollback = std::mem::take(&mut self.scrollback);
                *self = Screen::new(cols, rows);
                self.scrollback = scrollback;
                // Counts that only grow keep growing: a window caches by the one, a selection holds by the other.
                self.generation = generation;
                self.scrolled = scrolled;
            }
            b'(' | b')' | b'*' | b'+' | b'-' | b'.' | b'/' | b'#' | b'%' | b' ' => self.state = State::EscapeSkip,
            0x1B => self.state = State::Escape,
            _ => {}
        }
    }

    fn csi(&mut self, b: u8) {
        match b {
            b'0'..=b'9' => {
                let d = u32::from(b - b'0');
                self.param = Some((self.param.unwrap_or(0) * 10 + d).min(99_999));
            }
            b';' | b':' => {
                if self.params.len() < MAX_PARAMS {
                    self.params.push(self.param.unwrap_or(0));
                }
                self.param = None;
            }
            b'<'..=b'?' => {
                if self.params.is_empty() && self.param.is_none() {
                    self.private = Some(b);
                }
            }
            0x20..=0x2F => self.intermediate = Some(b),
            0x40..=0x7E => {
                if self.param.is_some() || !self.params.is_empty() {
                    if self.params.len() < MAX_PARAMS {
                        self.params.push(self.param.unwrap_or(0));
                    }
                }
                self.param = None;
                self.state = State::Ground;
                self.dispatch(b);
            }
            0x1B => self.state = State::Escape,
            0x18 | 0x1A => self.state = State::Ground,
            0x00..=0x1F => self.control(b),
            _ => {}
        }
    }

    /// The `i`th parameter, `default` when missing or zero (as most sequences read a zero).
    fn arg(&self, i: usize, default: u32) -> u32 {
        match self.params.get(i).copied() {
            None | Some(0) => default,
            Some(v) => v,
        }
    }

    fn dispatch(&mut self, f: u8) {
        if self.intermediate.is_some() {
            // DECSCUSR (cursor style), DECSTR and the like: nothing drawn changes.
            if self.intermediate == Some(b'!') && f == b'p' {
                self.soft_reset();
            }
            return;
        }
        if let Some(p) = self.private {
            match (p, f) {
                (b'?', b'h') | (b'?', b'l') => self.private_mode(f == b'h'),
                (b'>', b'c') => self.replies.extend_from_slice(b"\x1b[>0;0;0c"),
                _ => {}
            }
            return;
        }
        let rows = u32::from(self.rows);
        let cols = u32::from(self.cols);
        match f {
            b'A' => self.move_to(self.row.saturating_sub(self.arg(0, 1).min(rows) as u16), self.col),
            b'B' | b'e' => self.move_to(self.row.saturating_add(self.arg(0, 1).min(rows) as u16), self.col),
            b'C' | b'a' => self.move_to(self.row, self.col.saturating_add(self.arg(0, 1).min(cols) as u16)),
            b'D' => self.move_to(self.row, self.col.saturating_sub(self.arg(0, 1).min(cols) as u16)),
            b'E' => self.move_to(self.row.saturating_add(self.arg(0, 1).min(rows) as u16), 0),
            b'F' => self.move_to(self.row.saturating_sub(self.arg(0, 1).min(rows) as u16), 0),
            b'G' | b'`' => self.move_to(self.row, (self.arg(0, 1).min(cols) - 1) as u16),
            b'H' | b'f' => self.move_to((self.arg(0, 1).min(rows) - 1) as u16, (self.arg(1, 1).min(cols) - 1) as u16),
            b'd' => self.move_to((self.arg(0, 1).min(rows) - 1) as u16, self.col),
            b'J' => self.erase_display(self.params.first().copied().unwrap_or(0)),
            b'K' => self.erase_line(self.params.first().copied().unwrap_or(0)),
            b'@' => self.insert_chars(self.arg(0, 1).min(cols) as usize),
            b'P' => self.delete_chars(self.arg(0, 1).min(cols) as usize),
            b'X' => self.erase_chars(self.arg(0, 1).min(cols) as usize),
            b'L' => self.insert_lines(self.arg(0, 1).min(rows) as usize),
            b'M' => self.delete_lines(self.arg(0, 1).min(rows) as usize),
            b'S' => self.scroll_up(self.arg(0, 1).min(rows) as u16),
            b'T' => self.scroll_down(self.arg(0, 1).min(rows) as u16),
            b'm' => self.sgr(),
            b'r' => {
                let top = self.arg(0, 1).min(rows) - 1;
                let bottom = self.arg(1, rows).min(rows) - 1;
                if top < bottom {
                    self.top = top as u16;
                    self.bottom = bottom as u16;
                    self.move_to(0, 0);
                }
            }
            b's' => self.saved = (self.row, self.col, self.pen),
            b'u' => {
                let (r, c, p) = self.saved;
                self.row = r.min(self.rows - 1);
                self.col = c.min(self.cols - 1);
                self.pen = p;
                self.wrap_pending = false;
            }
            b'n' => match self.params.first().copied().unwrap_or(0) {
                5 => self.replies.extend_from_slice(b"\x1b[0n"),
                6 => self.replies.extend_from_slice(format!("\x1b[{};{}R", self.row + 1, self.col + 1).as_bytes()),
                _ => {}
            },
            b'c' => self.replies.extend_from_slice(b"\x1b[?1;0c"),
            _ => {}
        }
    }

    fn soft_reset(&mut self) {
        self.pen = Pen::default();
        self.top = 0;
        self.bottom = self.rows - 1;
        self.cursor_visible = true;
        self.application_cursor = false;
        self.autowrap = true;
        self.wrap_pending = false;
    }

    fn private_mode(&mut self, on: bool) {
        let modes = if self.params.is_empty() { vec![0] } else { self.params.clone() };
        for mode in modes {
            match mode {
                1 => self.application_cursor = on,
                7 => {
                    self.autowrap = on;
                    self.wrap_pending = false;
                }
                25 => self.cursor_visible = on,
                47 | 1047 | 1049 => self.alternate_screen(on, mode == 1049),
                2004 => self.bracketed_paste = on,
                _ => {}
            }
        }
    }

    fn alternate_screen(&mut self, on: bool, cursor: bool) {
        match (on, self.alternate.is_some()) {
            (true, false) => {
                if cursor {
                    self.saved = (self.row, self.col, self.pen);
                }
                let fresh = vec![vec![Cell::default(); self.cols as usize]; self.rows as usize];
                let main = std::mem::replace(&mut self.grid, fresh);
                self.alternate = Some((main, self.row, self.col));
                self.top = 0;
                self.bottom = self.rows - 1;
            }
            (false, true) => {
                if let Some((main, r, c)) = self.alternate.take() {
                    self.grid = main;
                    self.row = r.min(self.rows - 1);
                    self.col = c.min(self.cols - 1);
                }
                if cursor {
                    let (r, c, p) = self.saved;
                    self.row = r.min(self.rows - 1);
                    self.col = c.min(self.cols - 1);
                    self.pen = p;
                }
                self.top = 0;
                self.bottom = self.rows - 1;
            }
            _ => {}
        }
        self.wrap_pending = false;
    }

    fn move_to(&mut self, row: u16, col: u16) {
        self.row = row.min(self.rows - 1);
        self.col = col.min(self.cols - 1);
        self.wrap_pending = false;
    }

    fn erase_display(&mut self, how: u32) {
        let blank = self.blank();
        let (r, c) = (self.row as usize, self.col as usize);
        match how {
            0 => {
                for cell in &mut self.grid[r][c..] {
                    *cell = blank;
                }
                for row in &mut self.grid[r + 1..] {
                    row.fill(blank);
                }
            }
            1 => {
                for row in &mut self.grid[..r] {
                    row.fill(blank);
                }
                for cell in &mut self.grid[r][..=c] {
                    *cell = blank;
                }
            }
            2 => {
                for row in &mut self.grid {
                    row.fill(blank);
                }
            }
            3 => self.scrollback.clear(),
            _ => {}
        }
        self.wrap_pending = false;
    }

    fn erase_line(&mut self, how: u32) {
        let blank = self.blank();
        let c = self.col as usize;
        let row = &mut self.grid[self.row as usize];
        match how {
            0 => row[c..].fill(blank),
            1 => row[..=c].fill(blank),
            2 => row.fill(blank),
            _ => {}
        }
        fix_cut_wide(row);
        self.wrap_pending = false;
    }

    fn insert_chars(&mut self, n: usize) {
        let blank = self.blank();
        let c = self.col as usize;
        let row = &mut self.grid[self.row as usize];
        let n = n.min(row.len() - c);
        row.truncate(row.len() - n);
        for _ in 0..n {
            row.insert(c, blank);
        }
        fix_cut_wide(row);
        self.wrap_pending = false;
    }

    fn delete_chars(&mut self, n: usize) {
        let blank = self.blank();
        let c = self.col as usize;
        let row = &mut self.grid[self.row as usize];
        let n = n.min(row.len() - c);
        row.drain(c..c + n);
        row.resize(self.cols as usize, blank);
        fix_cut_wide(row);
        self.wrap_pending = false;
    }

    fn erase_chars(&mut self, n: usize) {
        let blank = self.blank();
        let c = self.col as usize;
        let row = &mut self.grid[self.row as usize];
        let end = (c + n).min(row.len());
        row[c..end].fill(blank);
        fix_cut_wide(row);
        self.wrap_pending = false;
    }

    fn insert_lines(&mut self, n: usize) {
        if self.row < self.top || self.row > self.bottom {
            return;
        }
        let (r, bottom) = (self.row as usize, self.bottom as usize);
        let n = n.min(bottom + 1 - r);
        for _ in 0..n {
            self.grid.remove(bottom);
            let fill = self.blank_row();
            self.grid.insert(r, fill);
        }
        self.col = 0;
        self.wrap_pending = false;
    }

    fn delete_lines(&mut self, n: usize) {
        if self.row < self.top || self.row > self.bottom {
            return;
        }
        let (r, bottom) = (self.row as usize, self.bottom as usize);
        let n = n.min(bottom + 1 - r);
        for _ in 0..n {
            self.grid.remove(r);
            let fill = self.blank_row();
            self.grid.insert(bottom, fill);
        }
        self.col = 0;
        self.wrap_pending = false;
    }

    fn sgr(&mut self) {
        if self.params.is_empty() {
            self.pen = Pen::default();
            return;
        }
        let p = self.params.clone();
        let mut i = 0;
        while i < p.len() {
            match p[i] {
                0 => self.pen = Pen::default(),
                1 => self.pen.bold = true,
                2 => self.pen.dim = true,
                3 => self.pen.italic = true,
                4 => self.pen.underline = true,
                7 => self.pen.inverse = true,
                21 => self.pen.underline = true,
                22 => {
                    self.pen.bold = false;
                    self.pen.dim = false;
                }
                23 => self.pen.italic = false,
                24 => self.pen.underline = false,
                27 => self.pen.inverse = false,
                30..=37 => self.pen.fg = Colour::Index((p[i] - 30) as u8),
                39 => self.pen.fg = Colour::Default,
                40..=47 => self.pen.bg = Colour::Index((p[i] - 40) as u8),
                49 => self.pen.bg = Colour::Default,
                90..=97 => self.pen.fg = Colour::Index((p[i] - 90 + 8) as u8),
                100..=107 => self.pen.bg = Colour::Index((p[i] - 100 + 8) as u8),
                38 | 48 => {
                    // 38;5;n and 38;2;r;g;b (48 for the background); a form not understood ends the sequence.
                    let fg = p[i] == 38;
                    let (colour, used) = match p.get(i + 1) {
                        Some(5) => (p.get(i + 2).map(|&v| Colour::Index(v.min(255) as u8)), 2),
                        Some(2) => match (p.get(i + 2), p.get(i + 3), p.get(i + 4)) {
                            (Some(&r), Some(&g), Some(&b)) => {
                                (Some(Colour::Rgb(r.min(255) as u8, g.min(255) as u8, b.min(255) as u8)), 4)
                            }
                            _ => (None, p.len()),
                        },
                        _ => (None, p.len()),
                    };
                    if let Some(colour) = colour {
                        if fg {
                            self.pen.fg = colour;
                        } else {
                            self.pen.bg = colour;
                        }
                    }
                    i = i.saturating_add(used);
                }
                _ => {}
            }
            i += 1;
        }
    }

    fn end_osc(&mut self) {
        self.state = State::Ground;
        let text = String::from_utf8_lossy(&self.osc).into_owned();
        self.osc.clear();
        if let Some((kind, rest)) = text.split_once(';')
            && matches!(kind, "0" | "2")
        {
            self.title = rest.chars().filter(|c| !c.is_control()).take(MAX_TITLE).collect();
        }
    }
}

/// A row cut through a wide character keeps no half of one.
fn fix_cut_wide(row: &mut Row) {
    for i in 0..row.len() {
        if row[i].ch == WIDE_TAIL && (i == 0 || width(row[i - 1].ch) != 2) {
            row[i].ch = ' ';
        }
        if width(row[i].ch) == 2 && row.get(i + 1).is_none_or(|n| n.ch != WIDE_TAIL) {
            row[i].ch = ' ';
        }
    }
}

/// A row's text, the second halves of wide characters left out and trailing blanks cut.
#[cfg(test)]
pub fn row_text(row: &Row) -> String {
    let s: String = row.iter().filter(|c| c.ch != WIDE_TAIL).map(|c| c.ch).collect();
    s.trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen(cols: u16, rows: u16, input: &str) -> Screen {
        let mut s = Screen::new(cols, rows);
        s.feed(input.as_bytes());
        s
    }

    fn shown(s: &Screen) -> Vec<String> {
        s.lines(0, s.size().1 as usize).into_iter().map(row_text).collect()
    }

    #[test]
    fn text_wraps_at_the_edge_and_a_line_feed_scrolls_into_the_scrollback() {
        let s = screen(5, 2, "abcdefg\r\nxy\r\nz");
        assert_eq!(shown(&s), vec!["xy", "z"]);
        assert_eq!(s.scrollback_len(), 2);
        assert_eq!(s.text(), "abcde\nfg\nxy\nz");
        assert_eq!(s.cursor(), (1, 1));
    }

    #[test]
    fn the_deferred_wrap_waits_for_the_next_character() {
        let mut s = screen(3, 2, "abc");
        assert_eq!(s.cursor(), (0, 2));
        s.feed(b"\r\n");
        assert_eq!(shown(&s), vec!["abc", ""], "a CR LF after a full line is one line, not two");
    }

    #[test]
    fn cursor_moves_positions_and_erases_as_conpty_sends_them() {
        let s = screen(10, 3, "hello\x1b[1;3HXY\x1b[2;1Hworld\x1b[3;5H!\x1b[1;4H\x1b[K");
        assert_eq!(shown(&s), vec!["heX", "world", "    !"]);
        let s = screen(10, 2, "abcdef\x1b[1;3H\x1b[2P");
        assert_eq!(shown(&s)[0], "abef");
        let s = screen(10, 2, "abcdef\x1b[1;3H\x1b[2@");
        assert_eq!(shown(&s)[0], "ab  cdef");
        let s = screen(10, 2, "abcdef\x1b[1;2H\x1b[3X");
        assert_eq!(shown(&s)[0], "a   ef");
        let s = screen(4, 3, "a\r\nb\r\nc\x1b[2J");
        assert_eq!(shown(&s), vec!["", "", ""]);
    }

    #[test]
    fn colours_in_16_256_and_24_bits_and_attributes_set_the_pen() {
        let s = screen(20, 1, "\x1b[1;31mA\x1b[0;38;5;208mB\x1b[48;2;1;2;3mC\x1b[7;4mD\x1b[mE\x1b[92mF");
        let row = s.lines(0, 1)[0];
        assert_eq!((row[0].pen.fg, row[0].pen.bold), (Colour::Index(1), true));
        assert_eq!(row[1].pen.fg, Colour::Index(208));
        assert_eq!((row[2].pen.fg, row[2].pen.bg), (Colour::Index(208), Colour::Rgb(1, 2, 3)));
        assert!(row[3].pen.inverse && row[3].pen.underline);
        assert_eq!(row[4].pen, Pen::default());
        assert_eq!(row[5].pen.fg, Colour::Index(10));
    }

    #[test]
    fn the_scroll_region_and_inserted_and_deleted_lines_stay_inside_it() {
        let mut s = screen(4, 4, "1\r\n2\r\n3\r\n4");
        s.feed(b"\x1b[2;3r\x1b[3;1H\n");
        assert_eq!(shown(&s), vec!["1", "3", "", "4"], "the region scrolled, not the screen");
        assert_eq!(s.scrollback_len(), 0, "a region below the top keeps no scrollback");
        let mut s = screen(4, 4, "1\r\n2\r\n3\r\n4\x1b[2;1H\x1b[L");
        assert_eq!(shown(&s), vec!["1", "", "2", "3"]);
        s.feed(b"\x1b[M");
        assert_eq!(shown(&s), vec!["1", "2", "3", ""]);
    }

    #[test]
    fn the_alternate_screen_keeps_the_main_one_and_gives_it_back() {
        let mut s = screen(6, 2, "shell\r\n$ ");
        s.feed(b"\x1b[?1049h\x1b[Hfull");
        assert!(s.on_alternate_screen());
        assert_eq!(shown(&s)[0], "full");
        s.feed(b"\x1b[?1049l");
        assert_eq!(shown(&s), vec!["shell", "$"]);
        assert_eq!(s.cursor(), (1, 2));
    }

    #[test]
    fn utf8_split_across_reads_wide_characters_and_bad_bytes() {
        let mut s = Screen::new(10, 1);
        let bytes = "é日".as_bytes();
        s.feed(&bytes[..1]);
        s.feed(&bytes[1..3]);
        s.feed(&bytes[3..]);
        assert_eq!(shown(&s)[0], "é日");
        assert_eq!(s.cursor(), (0, 3), "the wide character took two cells");
        s.feed(b"\xFF\xC3x");
        assert_eq!(shown(&s)[0], "é日\u{FFFD}\u{FFFD}x");
        let s = screen(3, 2, "ab日");
        assert_eq!(shown(&s), vec!["ab", "日"], "a wide character that does not fit wraps whole");
    }

    #[test]
    fn queries_are_answered_and_titles_and_modes_read() {
        let mut s = screen(10, 3, "ab\x1b[6n\x1b[5n\x1b[c");
        assert_eq!(s.take_replies(), b"\x1b[1;3R\x1b[0n\x1b[?1;0c".to_vec());
        assert!(s.take_replies().is_empty());
        let s = screen(10, 1, "\x1b]0;PowerShell 7\x07\x1b]2;C:\\work\x1b\\x");
        assert_eq!(s.title, "C:\\work");
        assert_eq!(shown(&s)[0], "x", "neither title is shown as text");
        let s = screen(10, 1, "\x1b[?25l\x1b[?1h\x1b[?2004h");
        assert!(!s.cursor_visible && s.application_cursor && s.bracketed_paste);
        let s = screen(10, 1, "\x1bP1$r0m\x1b\\ok\x1b[?2026h\x1b[3 q");
        assert_eq!(shown(&s)[0], "ok", "a DCS string, an unknown mode and a cursor style are not text");
    }

    #[test]
    fn hostile_counts_and_sizes_are_bounded() {
        let mut s = screen(10, 3, "\x1b[99999999999999@\x1b[99999;99999H\x1b[99999L\x1b[99999S");
        assert_eq!(s.cursor(), (2, 0));
        let mut long = String::from("\x1b]0;");
        long.push_str(&"x".repeat(100_000));
        long.push('\x07');
        s.feed(long.as_bytes());
        assert!(s.title.len() <= 256);
        for _ in 0..(SCROLLBACK + 50) {
            s.feed(b"line\r\n");
        }
        assert_eq!(s.scrollback_len(), SCROLLBACK);
        let s = Screen::new(60_000, 60_000);
        assert_eq!(s.size(), (MAX_COLS, MAX_ROWS));
        let mut many = String::from("\x1b[");
        many.push_str(&"1;".repeat(10_000));
        many.push('m');
        let mut s = Screen::new(10, 1);
        s.feed(many.as_bytes());
        s.feed(b"ok");
        assert_eq!(shown(&s)[0], "ok");
    }

    #[test]
    fn a_resize_keeps_the_cursor_on_the_screen_and_the_lines_above_it() {
        let mut s = screen(10, 4, "1\r\n2\r\n3\r\n4");
        s.resize(5, 2);
        assert_eq!(shown(&s), vec!["3", "4"]);
        assert_eq!(s.scrollback_len(), 2);
        assert_eq!(s.cursor(), (1, 1));
        s.resize(5, 3);
        assert_eq!(shown(&s), vec!["3", "4", ""]);
        let g = s.generation;
        s.resize(5, 3);
        assert_eq!(s.generation, g, "the same size changes nothing");
    }

    #[test]
    fn a_full_reset_clears_the_screen_and_keeps_the_scrollback() {
        let mut s = screen(4, 2, "a\r\nb\r\nc");
        assert_eq!(s.scrollback_len(), 1);
        s.feed(b"\x1b[31m\x1bc");
        assert_eq!(shown(&s), vec!["", ""]);
        assert_eq!(s.scrollback_len(), 1);
        s.feed(b"x");
        assert_eq!(s.lines(0, 1)[0][0].pen, Pen::default());
    }

    #[test]
    fn lines_are_read_from_any_depth_of_the_scrollback() {
        let mut s = Screen::new(4, 2);
        for n in 0..10 {
            s.feed(format!("{n}\r\n").as_bytes());
        }
        let text = |back, count| s.lines(back, count).into_iter().map(row_text).collect::<Vec<_>>();
        assert_eq!(text(0, 2), vec!["9", ""]);
        assert_eq!(text(3, 2), vec!["6", "7"]);
        assert_eq!(text(100, 2), vec!["0", "1"], "past the top, the oldest lines");
    }

    #[test]
    fn a_line_keeps_its_number_while_it_scrolls_and_a_selection_reads_across_lines() {
        let mut s = Screen::new(6, 2);
        s.feed(b"ab\r\ncdef\r\n");
        // "ab" scrolled off: line 0; "cdef" is on the screen's first row: line 1.
        assert_eq!(s.number_of_first(0, 2), 1);
        assert_eq!(s.line(0).map(row_text).as_deref(), Some("ab"));
        assert_eq!(s.text_between((1, 2), (0, 1)), "b\r\ncde", "either order, both ends included");
        let generation = s.generation;
        s.feed(b"g\r\nh\x1bc");
        assert_eq!(s.line(1).map(row_text).as_deref(), Some("cdef"), "the same number after scrolling and a reset");
        assert!(s.generation > generation, "a reset does not count the generation back");
        for _ in 0..SCROLLBACK + 10 {
            s.feed(b"x\r\n");
        }
        assert_eq!(s.line(0), None, "a dropped line is gone");
        assert_eq!(s.text_between((0, 0), (2, 5)), "", "and is left out of a selection");
        assert!(s.line(s.scrolled()).is_some(), "the screen's first row");
    }
}
