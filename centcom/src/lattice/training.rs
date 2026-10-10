//! The Training Studio's data: a port of the Python Training Studio's workspace module (opening a workspace
//! folder, its latest revision and its saved plans; creating a workspace, adding and removing documents and saving a
//! plan) and the whole of `metrics.py` (the poll-based reader of a trainer's append-only JSONL telemetry).
//!
//! Nothing here starts a trainer, deletes a file or holds one open between calls. Writes are Python's: each file is
//! published new (`open("xb")`, written, fsynced) and never replaced, so a second writer is refused rather than
//! overwritten, and a partial file is left for inspection. The refusal wording is the Python's.
//! Where the port cannot be exact, the difference is named at the place it happens; in short:
//!
//! * JSON is parsed by a small parser of this file's own, not serde_json, because Python's rules have to be kept:
//!   a duplicate key is refused (serde_json keeps the last one silently), `NaN`/`Infinity`/`-Infinity` are refused
//!   as the Python `parse_constant` hook refuses them, an integer token stays an integer and a token with `.`/`e` is a
//!   float (Python's `type(x) is int`), and errors are raised in the order Python's scanner meets them (a duplicate
//!   is found when its object closes, a nonfinite constant when it is read).
//! * A lone UTF-16 surrogate escape (`"\ud800"`), which Python keeps in a `str`, cannot live in a Rust `String`: in
//!   telemetry it becomes U+FFFD (duplicate-key detection still compares the original UTF-16 units); in workspace
//!   metadata it is refused as unreadable (Python refuses such metadata too, but later and with another message).
//! * Unicode NFC (Python's `unicodedata.normalize("NFC", ...)`) is Windows' own `NormalizeString`, as std has no
//!   normalisation tables and no crate for them is linked: CPython 3.12 carries Unicode 15.0's data and Windows its
//!   own; normalisation is stable for every assigned character, so the two agree on text both know (pinned against
//!   CPython for nine inputs below). Off Windows, a document cannot be added, and a snapshot's NFC is not
//!   re-checked; its SHA-256, byte count, UTF-8, line endings, surrounding whitespace and NUL rules always are.
//! * Python's `uuid.uuid4()` names (a workspace's id, a plan's file) take their 16 random bytes from Windows'
//!   system RNG (`BCryptGenRandom`), version and variant bits set as `uuid4` sets them.
//! * On Windows, a file's identity (Python's `st_dev`, `st_ino`) is the volume serial number and the 64-bit file
//!   index from `GetFileInformationByHandle`, called through a two-line FFI declaration (std keeps these behind an
//!   unstable feature). Python 3.12 may use the 128-bit `FileIdInfo` id; the 64-bit index is unique on NTFS but not
//!   guaranteed on ReFS.

use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::time::SystemTime;


// ---------------------------------------------------------------------------------------------------------------
// Limits (workspace.py).

/// The largest document snapshot.
pub const MAX_DOCUMENT_BYTES: u64 = 4 << 20;
/// The largest document pool.
pub const MAX_POOL_BYTES: u64 = 32 << 20;
/// The most documents in one revision.
pub const MAX_DOCUMENTS: usize = 1024;
/// The largest metadata file (workspace, revision or plan).
pub const MAX_METADATA_BYTES: u64 = 2 << 20;
/// The most revisions a workspace may hold.
pub const MAX_REVISIONS: u64 = 10000;
/// Python's json scanner raises RecursionError near the interpreter's limit of 1000 frames; this is a fixed bound
/// below it (the exact Python depth depends on the caller's stack, so it is not reproducible).
const MAX_DEPTH: usize = 900;
/// Python 3.12's `int()` refuses decimal strings longer than 4300 digits (`sys.int_info.default_max_str_digits`).
const MAX_INT_DIGITS: usize = 4300;

/// A model geometry from `train_moe_live.PRESETS`, as `workspace.PRESETS` declares it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Preset {
    pub key: &'static str,
    pub label: &'static str,
    pub layers: u32,
    pub experts: u32,
    pub active: u32,
    pub width: u32,
}

/// `workspace.PRESETS`, in its order (s, m, l).
pub const PRESETS: [Preset; 3] = [
    Preset {
        key: "s",
        label: "Small",
        layers: 4,
        experts: 8,
        active: 2,
        width: 128,
    },
    Preset {
        key: "m",
        label: "Medium",
        layers: 6,
        experts: 16,
        active: 2,
        width: 256,
    },
    Preset {
        key: "l",
        label: "Large",
        layers: 8,
        experts: 32,
        active: 2,
        width: 384,
    },
];

/// The preset with this key.
pub fn preset(key: &str) -> Option<&'static Preset> {
    PRESETS.iter().find(|preset| preset.key == key)
}

// ---------------------------------------------------------------------------------------------------------------
// Refusals.

/// A Python exception: a `StudioError` with its message, or any other exception (OSError, ValueError, ...), which
/// the Python callers either reword (metrics: "unreadable or malformed") or let escape (workspace).
#[derive(Clone, Debug, PartialEq, Eq)]
enum Fail {
    Studio(String),
    Other,
}

type Res<T> = Result<T, Fail>;

fn studio(message: &str) -> Fail {
    Fail::Studio(message.to_string())
}

fn require(condition: bool, message: &str) -> Res<()> {
    if condition { Ok(()) } else { Err(studio(message)) }
}

/// What a workspace refusal reads when Python would have raised something other than a StudioError (a raw OSError
/// from a stat, a ValueError from an out-of-range float): Python shows its own exception text; this port has one
/// bounded sentence instead.
const WORKSPACE_OTHER: &str = "The workspace could not be read.";

fn workspace_message(fail: Fail) -> String {
    match fail {
        Fail::Studio(message) => message,
        Fail::Other => WORKSPACE_OTHER.to_string(),
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Python values and their JSON.

/// A decoded JSON value with Python's types: `Int` and `Float` stay apart (`type(x) is int`), `Bool` is not an
/// `Int` for type checks, and objects keep their insertion order (Python dicts do; serde_json's map is sorted).
#[derive(Clone, Debug)]
enum Py {
    Null,
    Bool(bool),
    Int(PyInt),
    Float(f64),
    Str(String),
    List(Vec<Py>),
    Dict(Vec<(String, Py)>),
}

/// A JSON integer: its canonical decimal text (Python's `str(int)`: `-0` becomes `0`) and its value when it fits.
#[derive(Clone, Debug)]
struct PyInt {
    text: String,
    small: Option<i128>,
}

impl Py {
    fn get(&self, key: &str) -> Option<&Py> {
        match self {
            Py::Dict(pairs) => pairs.iter().find(|(name, _)| name == key).map(|(_, value)| value),
            _ => None,
        }
    }

    fn has_keys(&self, keys: &[&str]) -> bool {
        match self {
            Py::Dict(pairs) => pairs.len() == keys.len() && keys.iter().all(|key| self.get(key).is_some()),
            _ => false,
        }
    }

    fn str(&self) -> Option<&str> {
        if let Py::Str(text) = self { Some(text) } else { None }
    }

    /// The value of a JSON integer (not a bool, not a float) that fits in i128.
    fn int(&self) -> Option<i128> {
        if let Py::Int(int) = self { int.small } else { None }
    }

    fn is_str(&self, expected: &str) -> bool {
        self.str() == Some(expected)
    }
}

enum NumRef<'a> {
    Int(&'a str),
    Float(f64),
}

fn numeric(value: &Py) -> Option<NumRef<'_>> {
    match value {
        Py::Bool(true) => Some(NumRef::Int("1")),
        Py::Bool(false) => Some(NumRef::Int("0")),
        Py::Int(int) => Some(NumRef::Int(&int.text)),
        Py::Float(number) => Some(NumRef::Float(*number)),
        _ => None,
    }
}

/// Python's `==` between decoded JSON values: `True == 1 == 1.0`, ints and floats compare exactly, dicts by key set.
fn py_eq(left: &Py, right: &Py) -> bool {
    match (left, right) {
        (Py::Null, Py::Null) => true,
        (Py::Str(a), Py::Str(b)) => a == b,
        (Py::List(a), Py::List(b)) => a.len() == b.len() && a.iter().zip(b).all(|(x, y)| py_eq(x, y)),
        (Py::Dict(a), Py::Dict(b)) => {
            a.len() == b.len()
                && a.iter()
                    .all(|(key, value)| right.get(key).is_some_and(|other| py_eq(value, other)))
        }
        _ => match (numeric(left), numeric(right)) {
            (Some(NumRef::Int(a)), Some(NumRef::Int(b))) => a == b,
            (Some(NumRef::Float(a)), Some(NumRef::Float(b))) => a == b,
            (Some(NumRef::Int(a)), Some(NumRef::Float(b))) | (Some(NumRef::Float(b)), Some(NumRef::Int(a))) => {
                // Rust prints a float's exact integer value; `+ 0.0` turns -0.0 into 0.0.
                b.is_finite() && b.fract() == 0.0 && format!("{:.0}", b + 0.0) == a
            }
            _ => false,
        },
    }
}

/// Python's `str.isspace`, which `str.split()` and `str.strip()` use (it includes U+001C..U+001F, which Rust's
/// `char::is_whitespace` does not).
fn py_isspace(c: char) -> bool {
    matches!(c, '\t'..='\r' | '\u{1c}'..='\u{20}' | '\u{85}' | '\u{a0}' | '\u{1680}' | '\u{2000}'..='\u{200a}'
        | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}')
}

fn py_strip(text: &str) -> &str {
    text.trim_matches(py_isspace)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum JsonErr {
    Syntax,
    Duplicate,
    Nonfinite,
    TooDeep,
    IntTooLong,
}

/// Python's `json.loads` (C scanner, strict) with an `object_pairs_hook` that refuses duplicates and a
/// `parse_constant` that refuses NaN and the infinities.
struct Parser<'a> {
    s: &'a [u8],
    i: usize,
    depth: usize,
    lone_surrogate: bool,
}

impl<'a> Parser<'a> {
    /// Parses one document; the flag says whether a lone surrogate escape was replaced by U+FFFD.
    fn parse(text: &'a str) -> (Result<Py, JsonErr>, bool) {
        let mut parser = Parser {
            s: text.as_bytes(),
            i: 0,
            depth: 0,
            lone_surrogate: false,
        };
        let result = parser.document();
        (result, parser.lone_surrogate)
    }

    fn document(&mut self) -> Result<Py, JsonErr> {
        self.ws();
        let value = self.value()?;
        self.ws();
        if self.i == self.s.len() {
            Ok(value)
        } else {
            Err(JsonErr::Syntax)
        }
    }

    fn peek(&self) -> Option<u8> {
        self.s.get(self.i).copied()
    }

    fn ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.i += 1;
        }
    }

    fn word(&mut self, word: &[u8]) -> bool {
        if self.s[self.i..].starts_with(word) {
            self.i += word.len();
            true
        } else {
            false
        }
    }

    fn value(&mut self) -> Result<Py, JsonErr> {
        match self.peek() {
            Some(b'{') => self.object(),
            Some(b'[') => self.array(),
            Some(b'"') => {
                self.i += 1;
                self.string(None).map(Py::Str)
            }
            Some(b'n') if self.word(b"null") => Ok(Py::Null),
            Some(b't') if self.word(b"true") => Ok(Py::Bool(true)),
            Some(b'f') if self.word(b"false") => Ok(Py::Bool(false)),
            Some(b'N') if self.word(b"NaN") => Err(JsonErr::Nonfinite),
            Some(b'I') if self.word(b"Infinity") => Err(JsonErr::Nonfinite),
            Some(b'-') if self.s[self.i + 1..].starts_with(b"Infinity") => Err(JsonErr::Nonfinite),
            Some(b'-' | b'0'..=b'9') => self.number(),
            _ => Err(JsonErr::Syntax),
        }
    }

    fn enter(&mut self) -> Result<(), JsonErr> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            Err(JsonErr::TooDeep)
        } else {
            Ok(())
        }
    }

    fn object(&mut self) -> Result<Py, JsonErr> {
        self.enter()?;
        self.i += 1;
        self.ws();
        let mut pairs: Vec<(Vec<u16>, String, Py)> = Vec::new();
        if self.peek() == Some(b'}') {
            self.i += 1;
        } else {
            loop {
                if self.peek() != Some(b'"') {
                    return Err(JsonErr::Syntax);
                }
                self.i += 1;
                let mut units = Vec::new();
                let key = self.string(Some(&mut units))?;
                self.ws();
                if self.peek() != Some(b':') {
                    return Err(JsonErr::Syntax);
                }
                self.i += 1;
                self.ws();
                let value = self.value()?;
                pairs.push((units, key, value));
                self.ws();
                match self.peek() {
                    Some(b',') => {
                        self.i += 1;
                        self.ws();
                    }
                    Some(b'}') => {
                        self.i += 1;
                        break;
                    }
                    _ => return Err(JsonErr::Syntax),
                }
            }
        }
        // The hook sees the pairs only once the object has closed, as Python's does.
        let mut seen = HashSet::new();
        for (units, _, _) in &pairs {
            if !seen.insert(units.as_slice()) {
                return Err(JsonErr::Duplicate);
            }
        }
        self.depth -= 1;
        Ok(Py::Dict(
            pairs.into_iter().map(|(_, key, value)| (key, value)).collect(),
        ))
    }

    fn array(&mut self) -> Result<Py, JsonErr> {
        self.enter()?;
        self.i += 1;
        self.ws();
        let mut items = Vec::new();
        if self.peek() == Some(b']') {
            self.i += 1;
        } else {
            loop {
                items.push(self.value()?);
                self.ws();
                match self.peek() {
                    Some(b',') => {
                        self.i += 1;
                        self.ws();
                    }
                    Some(b']') => {
                        self.i += 1;
                        break;
                    }
                    _ => return Err(JsonErr::Syntax),
                }
            }
        }
        self.depth -= 1;
        Ok(Py::List(items))
    }

    fn digits(&mut self) {
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.i += 1;
        }
    }

    fn number(&mut self) -> Result<Py, JsonErr> {
        let start = self.i;
        let negative = self.peek() == Some(b'-');
        if negative {
            self.i += 1;
        }
        match self.peek() {
            Some(b'0') => self.i += 1,
            Some(b'1'..=b'9') => self.digits(),
            _ => return Err(JsonErr::Syntax),
        }
        let integer_end = self.i;
        let mut float = false;
        if self.peek() == Some(b'.') && matches!(self.s.get(self.i + 1), Some(b'0'..=b'9')) {
            self.i += 1;
            self.digits();
            float = true;
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            let mut next = self.i + 1;
            if matches!(self.s.get(next), Some(b'+' | b'-')) {
                next += 1;
            }
            if matches!(self.s.get(next), Some(b'0'..=b'9')) {
                self.i = next;
                self.digits();
                float = true;
            }
        }
        // ASCII by construction.
        let text = std::str::from_utf8(&self.s[start..self.i]).map_err(|_| JsonErr::Syntax)?;
        if float {
            // Correctly rounded, as Python's float(); an overflow is infinity, as in Python (1e999 -> inf).
            return text.parse::<f64>().map(Py::Float).map_err(|_| JsonErr::Syntax);
        }
        if integer_end - start - usize::from(negative) > MAX_INT_DIGITS {
            return Err(JsonErr::IntTooLong);
        }
        let small = text.parse::<i128>().ok();
        let text = if small == Some(0) {
            "0".to_string()
        } else {
            text.to_string()
        };
        Ok(Py::Int(PyInt { text, small }))
    }

    fn hex4(&mut self) -> Result<u32, JsonErr> {
        let digits = self.s.get(self.i..self.i + 4).ok_or(JsonErr::Syntax)?;
        let mut value = 0;
        for digit in digits {
            value = value * 16 + (*digit as char).to_digit(16).ok_or(JsonErr::Syntax)?;
        }
        self.i += 4;
        Ok(value)
    }

    /// A string body after its opening quote. `units` collects the UTF-16 code units (lone surrogates included) so
    /// that keys compare as Python's `str` would.
    fn string(&mut self, mut units: Option<&mut Vec<u16>>) -> Result<String, JsonErr> {
        let mut out = String::new();
        loop {
            let start = self.i;
            while let Some(byte) = self.peek() {
                if byte == b'"' || byte == b'\\' || byte < 0x20 {
                    break;
                }
                self.i += 1;
            }
            // The input is valid UTF-8 and the stops are ASCII, so the run is too.
            let run = std::str::from_utf8(&self.s[start..self.i]).map_err(|_| JsonErr::Syntax)?;
            out.push_str(run);
            if let Some(units) = units.as_deref_mut() {
                units.extend(run.encode_utf16());
            }
            match self.peek() {
                Some(b'"') => {
                    self.i += 1;
                    return Ok(out);
                }
                Some(b'\\') => self.i += 1,
                _ => return Err(JsonErr::Syntax), // end of input or a control character (strict mode)
            }
            let simple = match self.peek() {
                Some(b'"') => Some('"'),
                Some(b'\\') => Some('\\'),
                Some(b'/') => Some('/'),
                Some(b'b') => Some('\u{8}'),
                Some(b'f') => Some('\u{c}'),
                Some(b'n') => Some('\n'),
                Some(b'r') => Some('\r'),
                Some(b't') => Some('\t'),
                Some(b'u') => None,
                _ => return Err(JsonErr::Syntax),
            };
            self.i += 1;
            if let Some(c) = simple {
                out.push(c);
                if let Some(units) = units.as_deref_mut() {
                    units.push(c as u16);
                }
                continue;
            }
            let mut code = self.hex4()?;
            // As CPython's scanstring: a high surrogate joins a following \u low surrogate; any other following
            // escape is read again on its own.
            if (0xd800..0xdc00).contains(&code) && self.s[self.i..].starts_with(b"\\u") && self.i + 6 < self.s.len() {
                let save = self.i;
                self.i += 2;
                let low = self.hex4()?;
                if (0xdc00..0xe000).contains(&low) {
                    code = 0x10000 + ((code - 0xd800) << 10) + (low - 0xdc00);
                } else {
                    self.i = save;
                }
            }
            match char::from_u32(code) {
                Some(c) => {
                    out.push(c);
                    if let Some(units) = units.as_deref_mut() {
                        let mut buffer = [0u16; 2];
                        units.extend_from_slice(c.encode_utf16(&mut buffer));
                    }
                }
                None => {
                    self.lone_surrogate = true;
                    out.push('\u{fffd}');
                    if let Some(units) = units.as_deref_mut() {
                        units.push(code as u16);
                    }
                }
            }
        }
    }
}

/// Python's `repr(float)` (shortest round-trip digits; scientific outside 1e-4 <= |x| < 1e16).
fn py_float_repr(number: f64) -> String {
    if number == 0.0 {
        return if number.is_sign_negative() { "-0.0" } else { "0.0" }.to_string();
    }
    let formatted = format!("{number:e}");
    let (mantissa, exponent) = formatted.split_once('e').unwrap_or((&formatted, "0"));
    let exponent: i32 = exponent.parse().unwrap_or(0);
    let sign = if mantissa.starts_with('-') { "-" } else { "" };
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    if (-4..16).contains(&exponent) {
        let point = exponent + 1;
        if point <= 0 {
            format!("{sign}0.{}{digits}", "0".repeat((-point) as usize))
        } else if point as usize >= digits.len() {
            format!("{sign}{digits}{}.0", "0".repeat(point as usize - digits.len()))
        } else {
            let point = point as usize;
            format!("{sign}{}.{}", &digits[..point], &digits[point..])
        }
    } else {
        let rest = if digits.len() > 1 {
            format!(".{}", &digits[1..])
        } else {
            String::new()
        };
        let exponent_sign = if exponent < 0 { '-' } else { '+' };
        format!("{sign}{}{rest}e{exponent_sign}{:02}", &digits[..1], exponent.abs())
    }
}

/// `workspace._canonical`: `json.dumps(sort_keys=True, separators=(",", ":"), ensure_ascii=True, allow_nan=False)`
/// plus a newline. None where Python raises (a nonfinite float).
fn canonical(value: &Py) -> Option<Vec<u8>> {
    let mut out = String::new();
    write_canonical(value, &mut out)?;
    out.push('\n');
    Some(out.into_bytes())
}

fn write_canonical(value: &Py, out: &mut String) -> Option<()> {
    match value {
        Py::Null => out.push_str("null"),
        Py::Bool(flag) => out.push_str(if *flag { "true" } else { "false" }),
        Py::Int(int) => out.push_str(&int.text),
        Py::Float(number) => {
            if !number.is_finite() {
                return None;
            }
            out.push_str(&py_float_repr(*number));
        }
        Py::Str(text) => write_ascii_string(text, out),
        Py::List(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_canonical(item, out)?;
            }
            out.push(']');
        }
        Py::Dict(pairs) => {
            // Python sorts str keys by code point, which is UTF-8 byte order.
            let mut sorted: Vec<&(String, Py)> = pairs.iter().collect();
            sorted.sort_by(|a, b| a.0.cmp(&b.0));
            out.push('{');
            for (index, (key, item)) in sorted.into_iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_ascii_string(key, out);
                out.push(':');
                write_canonical(item, out)?;
            }
            out.push('}');
        }
    }
    Some(())
}

fn write_ascii_string(text: &str, out: &mut String) {
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            ' '..='~' => out.push(c),
            _ => {
                let mut buffer = [0u16; 2];
                for unit in c.encode_utf16(&mut buffer) {
                    out.push_str(&format!("\\u{unit:04x}"));
                }
            }
        }
    }
    out.push('"');
}

// ---------------------------------------------------------------------------------------------------------------
// SHA-256: the one the chat core already links.

/// SHA-256 as lowercase hex: lattice-core's, which the chat core already links.
fn sha256_hex(data: &[u8]) -> String {
    lattice_core::sha::sha256_hex(data)
}

// ---------------------------------------------------------------------------------------------------------------
// Local paths: `workspace._safe` and `_identity`.

#[derive(Clone, Copy, PartialEq, Eq)]
enum Want {
    File,
    Dir,
    New,
}

/// `st_mode` is a link, or (Windows) any reparse point: symlinks, junctions, mount points and also cloud or
/// deduplication placeholders, exactly as Python's `FILE_ATTRIBUTE_REPARSE_POINT` test.
fn is_link(info: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        info.file_type().is_symlink() || info.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    {
        info.file_type().is_symlink()
    }
}

/// `workspace._safe`: no `..`; neither the path nor any ancestor is a link, junction or other reparse point; then
/// the right type (or, for `New`, nothing there yet and an existing parent). Returns the absolute path.
fn safe(path: &Path, want: Want) -> Res<PathBuf> {
    require(
        !path
            .components()
            .any(|part| matches!(part, Component::ParentDir) || part.as_os_str() == ".."),
        "Invalid local path.",
    )?;
    // Python's absolute() does not raise inside _safe's try; an unusable working directory is "other".
    let absolute = std::path::absolute(path).map_err(|_| Fail::Other)?;
    for part in absolute.ancestors() {
        match fs::symlink_metadata(part) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                require(
                    want == Want::New && part == absolute,
                    "The parent folder must already exist.",
                )?;
            }
            Err(_) => return Err(studio("The local path is unavailable.")),
            Ok(info) => require(!is_link(&info), "Links and junctions are not supported.")?,
        }
    }
    match want {
        Want::New => match absolute.try_exists() {
            Ok(exists) => require(!exists, "The destination already exists.")?,
            Err(_) => return Err(studio("The local path is unavailable.")),
        },
        Want::Dir => require(absolute.is_dir(), "The local path has the wrong type.")?,
        Want::File => require(absolute.is_file(), "The local path has the wrong type.")?,
    }
    Ok(absolute)
}

/// What `os.stat` tells: (st_dev, st_ino) as `id`, and the size and modification time `_identity` adds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Stat {
    id: (u64, u64),
    size: u64,
    modified: Option<SystemTime>,
}

#[cfg(windows)]
#[allow(unsafe_code)]
mod win {
    //! `GetFileInformationByHandle`, for the volume serial number and file index std keeps unstable.

    use std::fs::File;
    use std::io;
    use std::os::windows::io::AsRawHandle;

    #[repr(C)]
    #[derive(Default)]
    struct ByHandleFileInformation {
        attributes: u32,
        creation: [u32; 2],
        access: [u32; 2],
        write: [u32; 2],
        volume_serial_number: u32,
        size_high: u32,
        size_low: u32,
        links: u32,
        index_high: u32,
        index_low: u32,
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        // Declared as compute/train.rs declares it (a handle as isize), so the crate's two declarations agree.
        fn GetFileInformationByHandle(file: isize, info: *mut ByHandleFileInformation) -> i32;
    }

    #[link(name = "normaliz")]
    unsafe extern "system" {
        fn NormalizeString(form: i32, source: *const u16, source_len: i32, destination: *mut u16, destination_len: i32)
        -> i32;
    }

    #[link(name = "bcrypt")]
    unsafe extern "system" {
        fn BCryptGenRandom(algorithm: *mut core::ffi::c_void, buffer: *mut u8, length: u32, flags: u32) -> i32;
    }

    /// `unicodedata.normalize("NFC", text)` through Windows' `NormalizeString(NormalizationC, ...)`. None where
    /// Windows refuses the text.
    pub fn nfc(text: &str) -> Option<String> {
        const NORMALIZATION_C: i32 = 1;
        const ERROR_INSUFFICIENT_BUFFER: i32 = 122;
        if text.is_empty() {
            return Some(String::new());
        }
        let source: Vec<u16> = text.encode_utf16().collect();
        let length = i32::try_from(source.len()).ok()?;
        // SAFETY: `source` is a valid UTF-16 buffer of `length` units; a null destination of length 0 asks only for
        // an estimate.
        let mut estimate = unsafe { NormalizeString(NORMALIZATION_C, source.as_ptr(), length, std::ptr::null_mut(), 0) };
        for _ in 0..8 {
            if estimate <= 0 {
                return None;
            }
            let mut out = vec![0u16; estimate as usize];
            // SAFETY: `out` is writable for `estimate` units and the function writes at most that many.
            let written = unsafe { NormalizeString(NORMALIZATION_C, source.as_ptr(), length, out.as_mut_ptr(), estimate) };
            if written > 0 {
                out.truncate(written as usize);
                return String::from_utf16(&out).ok();
            }
            if io::Error::last_os_error().raw_os_error() != Some(ERROR_INSUFFICIENT_BUFFER) {
                return None;
            }
            // A negative result is the negated new estimate.
            estimate = written.checked_neg()?.max(estimate.saturating_mul(2));
        }
        None
    }

    /// `n` bytes from the system's preferred random number generator.
    pub fn random<const N: usize>() -> io::Result<[u8; N]> {
        const BCRYPT_USE_SYSTEM_PREFERRED_RNG: u32 = 0x2;
        let mut bytes = [0u8; N];
        // SAFETY: `bytes` is writable for N bytes; no algorithm handle is passed with the system-preferred flag.
        let status =
            unsafe { BCryptGenRandom(std::ptr::null_mut(), bytes.as_mut_ptr(), N as u32, BCRYPT_USE_SYSTEM_PREFERRED_RNG) };
        if status != 0 {
            return Err(io::Error::other(format!("BCryptGenRandom returned {status:#x}")));
        }
        Ok(bytes)
    }

    /// (volume serial number, 64-bit file index) of an open handle.
    pub fn identity(file: &File) -> io::Result<(u64, u64)> {
        let mut info = ByHandleFileInformation::default();
        // SAFETY: the handle is open for the duration of the call and `info` is a valid, writable
        // BY_HANDLE_FILE_INFORMATION; the function writes only into it.
        let ok = unsafe { GetFileInformationByHandle(file.as_raw_handle() as isize, &mut info) };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok((
            u64::from(info.volume_serial_number),
            (u64::from(info.index_high) << 32) | u64::from(info.index_low),
        ))
    }
}

/// `os.fstat` of an open file.
fn stat_file(file: &File) -> io::Result<Stat> {
    let meta = file.metadata()?;
    #[cfg(windows)]
    let id = win::identity(file)?;
    #[cfg(unix)]
    let id = {
        use std::os::unix::fs::MetadataExt;
        (meta.dev(), meta.ino())
    };
    #[cfg(not(any(windows, unix)))]
    let id = (0, 0);
    Ok(Stat {
        id,
        size: meta.len(),
        modified: meta.modified().ok(),
    })
}

/// `Path.stat()` (follows links; callers have refused them already). On Windows it opens the path for attributes
/// only, sharing read, write and delete, as CPython's stat does, and so works for folders too.
fn stat_path(path: &Path) -> io::Result<Stat> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_READ_ATTRIBUTES: u32 = 0x80;
        const FILE_SHARE_ALL: u32 = 0x1 | 0x2 | 0x4;
        const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
        let file = fs::OpenOptions::new()
            .access_mode(FILE_READ_ATTRIBUTES)
            .share_mode(FILE_SHARE_ALL)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
            .open(path)?;
        stat_file(&file)
    }
    #[cfg(not(windows))]
    {
        let meta = fs::metadata(path)?;
        #[cfg(unix)]
        let id = {
            use std::os::unix::fs::MetadataExt;
            (meta.dev(), meta.ino())
        };
        #[cfg(not(unix))]
        let id = (0, 0);
        Ok(Stat {
            id,
            size: meta.len(),
            modified: meta.modified().ok(),
        })
    }
}

/// `workspace._read`: a bounded read of a file that must not change identity, size or mtime while it is read.
fn read_bounded(path: &Path, maximum: u64) -> Res<Vec<u8>> {
    let path = safe(path, Want::File)?;
    let unreadable = |_: io::Error| studio("Could not read the local file.");
    let before = stat_path(&path).map_err(unreadable)?;
    require(before.size <= maximum, "File exceeds the supported size limit.")?;
    let mut payload = Vec::new();
    {
        let handle = File::open(&path).map_err(unreadable)?;
        let opened = stat_file(&handle).map_err(unreadable)?;
        require(opened.id == before.id, "File changed while opening.")?;
        handle.take(maximum + 1).read_to_end(&mut payload).map_err(unreadable)?;
    }
    let same =
        payload.len() as u64 == before.size && stat_path(&safe(&path, Want::File)?).map_err(unreadable)? == before;
    require(same, "File changed while reading.")?;
    Ok(payload)
}

// ---------------------------------------------------------------------------------------------------------------
// Workspace (read side of workspace.py).

/// One document of a revision's inventory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Document {
    pub id: String,
    pub name: String,
    pub source: String,
    pub license: String,
    pub group: String,
    pub sha256: String,
    pub bytes: u64,
}

/// The latest revision: its number, the SHA-256 of its manifest file, and its documents.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Revision {
    pub number: u32,
    pub sha256: String,
    pub documents: Vec<Document>,
}

/// A saved, verified plan (`training_ready` is always false; a plan that says otherwise is refused).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    pub workspace_id: String,
    pub revision: u32,
    pub manifest_sha256: String,
    pub documents: Vec<Document>,
    pub preset: &'static Preset,
    pub selected_experts: Vec<String>,
}

/// One `plans/*.json` file and what loading it gave (`Workspace.load_plan`'s result or refusal).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlanEntry {
    pub path: PathBuf,
    pub plan: Result<Plan, String>,
}

/// An opened workspace: what `Workspace.open` verifies, plus every saved plan loaded as `load_plan` loads one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Workspace {
    pub path: PathBuf,
    pub id: String,
    pub revision: Revision,
    pub plans: Vec<PlanEntry>,
    /// What this open saw, for a later write's `_check_root`: the four folders' identities and workspace.json's bytes.
    roots: Vec<(&'static str, (u64, u64))>,
    metadata: Vec<u8>,
}

const DOCUMENT_KEYS: [&str; 7] = ["id", "name", "source", "license", "group", "sha256", "bytes"];

fn is_lower_hex(text: &str, length: usize) -> bool {
    text.len() == length && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// `_IDENTITY`: `[A-Za-z0-9][A-Za-z0-9._:/@+-]*`.
fn is_source_id(text: &str) -> bool {
    let mut bytes = text.bytes();
    bytes.next().is_some_and(|b| b.is_ascii_alphanumeric())
        && bytes.all(|b| b.is_ascii_alphanumeric() || b"._:/@+-".contains(&b))
}

/// `_text`.
fn text_field(value: &Py, label: &str, maximum: usize) -> Res<String> {
    let message = format!("Invalid {label}.");
    let text = value.str().filter(|text| !text.is_empty() && py_strip(text) == *text);
    let text = text.ok_or_else(|| studio(&message))?;
    require(
        text.len() <= maximum && !text.chars().any(|c| (c as u32) < 32 || c as u32 == 127),
        &message,
    )?;
    Ok(text.to_string())
}

/// `_documents`.
fn documents(value: &Py) -> Res<Vec<Document>> {
    let items = match value {
        Py::List(items) if items.len() <= MAX_DOCUMENTS => items,
        _ => return Err(studio("Invalid document inventory.")),
    };
    let mut seen = HashSet::new();
    let mut result = Vec::with_capacity(items.len());
    for doc in items {
        require(doc.has_keys(&DOCUMENT_KEYS), "Invalid document metadata.")?;
        let field = |key: &str| doc.get(key).unwrap_or(&Py::Null);
        let sha256 = field("sha256").str().filter(|text| is_lower_hex(text, 64));
        let sha256 = match sha256 {
            Some(sha256) if py_eq(field("id"), field("sha256")) => sha256.to_string(),
            _ => return Err(studio("Invalid document identity.")),
        };
        require(seen.insert(sha256.clone()), "Duplicate document membership.")?;
        let name = text_field(field("name"), "name", 2048)?;
        let source = text_field(field("source"), "source", 256)?;
        let license = text_field(field("license"), "license", 256)?;
        let group = text_field(field("group"), "group", 2048)?;
        require(
            is_source_id(&source),
            "Source ID must use letters, numbers or . _ : / @ + -.",
        )?;
        let bytes = field("bytes")
            .int()
            .filter(|bytes| (1..=i128::from(MAX_DOCUMENT_BYTES)).contains(bytes));
        let bytes = bytes.ok_or_else(|| studio("Invalid document byte count."))? as u64;
        result.push(Document {
            id: sha256.clone(),
            name,
            source,
            license,
            group,
            sha256,
            bytes,
        });
    }
    require(
        result.iter().map(|doc| doc.bytes).sum::<u64>() <= MAX_POOL_BYTES,
        "Document pool exceeds 32 MiB.",
    )?;
    Ok(result)
}

/// `_decode`: UTF-8, JSON without duplicates or nonfinite constants, and byte-for-byte canonical.
fn decode_metadata(payload: &[u8]) -> Res<Py> {
    let unreadable = || studio("Unreadable workspace metadata.");
    let text = std::str::from_utf8(payload).map_err(|_| unreadable())?;
    let (parsed, lone_surrogate) = Parser::parse(text);
    let value = match parsed {
        Ok(value) => value,
        Err(JsonErr::Duplicate) => return Err(studio("Duplicate metadata key.")),
        Err(JsonErr::Nonfinite) => return Err(studio("Nonfinite metadata.")),
        // Python: JSONDecodeError and RecursionError are reworded here; an over-long integer raises a raw
        // ValueError instead (this port rewords it too).
        Err(_) => return Err(unreadable()),
    };
    // Deviation: Python accepts the surrogate here and refuses the metadata later with another error.
    require(!lone_surrogate, "Unreadable workspace metadata.")?;
    // Python's dumps raises a raw ValueError for an overflowed float (1e999); reworded here.
    let canonical = canonical(&value).ok_or_else(unreadable)?;
    require(canonical == payload, "Metadata is not canonical JSON.")?;
    Ok(value)
}

/// `_selection`: a known preset and a non-empty, duplicate-free set of "layer:expert" names inside it, sorted.
fn selection(chosen: Option<&Py>, selected: Option<&Py>) -> Res<(&'static Preset, Vec<String>)> {
    let shape = chosen
        .and_then(Py::str)
        .and_then(preset)
        .ok_or_else(|| studio("Choose a supported model preset."))?;
    let items = match selected {
        Some(Py::List(items)) if !items.is_empty() && items.iter().all(|item| item.str().is_some()) => items,
        _ => return Err(studio("Select at least one expert.")),
    };
    let mut parsed = Vec::with_capacity(items.len());
    let mut seen = HashSet::new();
    let mut valid = true;
    for item in items {
        let text = item.str().unwrap_or_default();
        valid &= seen.insert(text);
        // f"{layer}:{expert}" exactly: no sign, no leading zero, no spaces.
        let pair = text
            .split_once(':')
            .and_then(|(layer, expert)| Some((layer.parse::<u32>().ok()?, expert.parse::<u32>().ok()?)));
        match pair {
            Some((layer, expert))
                if layer < shape.layers && expert < shape.experts && format!("{layer}:{expert}") == text =>
            {
                parsed.push((layer, expert));
            }
            _ => valid = false,
        }
    }
    require(valid, "Expert selection does not match the model preset.")?;
    parsed.sort_unstable();
    Ok((
        shape,
        parsed
            .into_iter()
            .map(|(layer, expert)| format!("{layer}:{expert}"))
            .collect(),
    ))
}

fn preset_value(shape: &Preset) -> Py {
    let int = |value: u32| {
        Py::Int(PyInt {
            text: value.to_string(),
            small: Some(i128::from(value)),
        })
    };
    Py::Dict(vec![
        ("label".to_string(), Py::Str(shape.label.to_string())),
        ("layers".to_string(), int(shape.layers)),
        ("experts".to_string(), int(shape.experts)),
        ("active".to_string(), int(shape.active)),
        ("width".to_string(), int(shape.width)),
    ])
}

/// The state `Workspace.open` keeps: the folders' identities and the identity file's bytes, re-checked before each
/// later read (`_check_root`), and the snapshots already verified during this open.
struct Opened {
    root: PathBuf,
    roots: Vec<(&'static str, (u64, u64))>,
    metadata: Vec<u8>,
    id: String,
    verified: HashMap<(String, u64), Res<()>>,
}

impl Opened {
    fn folder(&self, name: &str) -> PathBuf {
        if name == "." {
            self.root.clone()
        } else {
            self.root.join(name)
        }
    }

    /// `_check_root`. Python compares SHA-256 digests of workspace.json; the bytes are compared here, which is the
    /// same test without the collision caveat.
    fn check_root(&self) -> Res<()> {
        for (name, identity) in &self.roots {
            let now = stat_path(&safe(&self.folder(name), Want::Dir)?).map_err(|_| Fail::Other)?;
            require(now.id == *identity, "Workspace folder changed. Reopen the workspace.")?;
        }
        require(
            read_bounded(&self.root.join("workspace.json"), MAX_METADATA_BYTES)? == self.metadata,
            "Workspace identity changed.",
        )
    }

    /// `_revision_payload`.
    fn revision_payload(&self, number: u32) -> Res<(Py, Vec<Document>, String)> {
        let payload = read_bounded(
            &self.root.join("revisions").join(format!("{number:08}.json")),
            MAX_METADATA_BYTES,
        )?;
        let revision = decode_metadata(&payload)?;
        let valid = revision.has_keys(&["schema", "workspace_id", "revision", "documents"])
            && revision.get("schema").and_then(Py::int) == Some(1)
            && revision.get("workspace_id").is_some_and(|id| id.is_str(&self.id))
            && revision.get("revision").and_then(Py::int) == Some(i128::from(number));
        require(valid, "Invalid workspace revision.")?;
        let docs = documents(revision.get("documents").unwrap_or(&Py::Null))?;
        Ok((revision, docs, sha256_hex(&payload)))
    }

    /// `_latest`: every name in revisions/ is NNNNNNNN.json, the numbers run 0..n without a gap, the highest wins.
    fn latest(&self) -> Res<Revision> {
        self.check_root()?;
        let unreadable = |_: io::Error| studio("Could not read workspace revisions.");
        let mut numbers: Vec<u32> = Vec::new();
        for entry in fs::read_dir(self.root.join("revisions")).map_err(unreadable)? {
            let entry = entry.map_err(unreadable)?;
            let name = entry.file_name();
            let number = name.to_str().and_then(|name| {
                let stem = name.strip_suffix(".json")?;
                (stem.len() == 8 && stem.bytes().all(|b| b.is_ascii_digit())).then(|| stem.parse::<u32>().ok())?
            });
            let number = match number {
                Some(number) if (numbers.len() as u64) < MAX_REVISIONS => number,
                _ => return Err(studio("Invalid or exhausted revision inventory.")),
            };
            safe(&entry.path(), Want::File)?;
            numbers.push(number);
        }
        let mut sorted = numbers.clone();
        sorted.sort_unstable();
        let complete = !sorted.is_empty()
            && sorted
                .iter()
                .enumerate()
                .all(|(index, number)| *number as usize == index);
        require(complete, "Workspace revision history is incomplete.")?;
        let number = sorted.last().copied().unwrap_or(0);
        let (_, documents, sha256) = self.revision_payload(number)?;
        Ok(Revision {
            number,
            sha256,
            documents,
        })
    }

    /// `_document_text`'s checks (off Windows, without the NFC one).
    fn verify_document(&self, doc: &Document) -> Res<()> {
        let payload = read_bounded(
            &self.root.join("documents").join(format!("{}.txt", doc.sha256)),
            MAX_DOCUMENT_BYTES,
        )?;
        require(
            payload.len() as u64 == doc.bytes && sha256_hex(&payload) == doc.sha256,
            "A document snapshot changed or is corrupted.",
        )?;
        let text = std::str::from_utf8(&payload).map_err(|_| studio("Document snapshot is not UTF-8."))?;
        require(
            !text.is_empty() && !text.contains('\r') && py_strip(text) == text && !text.contains('\0'),
            "Document snapshot is not normalized text.",
        )?;
        #[cfg(windows)]
        require(win::nfc(text).as_deref() == Some(text), "Document snapshot is not normalized text.")?;
        Ok(())
    }

    /// `_verify_documents`. A snapshot is read and hashed once per open, however many plans name it (Python's
    /// `load_plan` re-reads them per plan; the result is the same unless a file changes during the open).
    fn verify_documents(&mut self, docs: &[Document]) -> Res<()> {
        for doc in docs {
            let key = (doc.sha256.clone(), doc.bytes);
            let result = match self.verified.get(&key) {
                Some(result) => result.clone(),
                None => {
                    let result = self.verify_document(doc);
                    self.verified.insert(key, result.clone());
                    result
                }
            };
            result?;
        }
        Ok(())
    }

    /// `Workspace.load_plan`.
    fn load_plan(&mut self, path: &Path) -> Res<Plan> {
        self.check_root()?;
        let plan = decode_metadata(&read_bounded(path, MAX_METADATA_BYTES)?)?;
        let keys = [
            "schema",
            "kind",
            "training_ready",
            "workspace_id",
            "revision",
            "manifest_sha256",
            "documents",
            "preset",
            "geometry",
            "selected_experts",
        ];
        let valid = plan.has_keys(&keys)
            && plan.get("schema").and_then(Py::int) == Some(1)
            && plan.get("kind").is_some_and(|kind| kind.is_str("training-studio-plan"))
            && matches!(plan.get("training_ready"), Some(Py::Bool(false)))
            && plan.get("workspace_id").is_some_and(|id| id.is_str(&self.id));
        require(valid, "Invalid training plan.")?;
        let (shape, selected) = selection(plan.get("preset"), plan.get("selected_experts"))?;
        let listed = match plan.get("selected_experts") {
            Some(Py::List(items)) => items
                .iter()
                .map(|item| item.str().unwrap_or_default())
                .eq(selected.iter().map(String::as_str)),
            _ => false,
        };
        let geometry = plan
            .get("geometry")
            .is_some_and(|geometry| py_eq(geometry, &preset_value(shape)));
        require(listed && geometry, "Plan geometry changed.")?;
        let revision = plan
            .get("revision")
            .and_then(Py::int)
            .filter(|r| (0..i128::from(MAX_REVISIONS)).contains(r));
        let revision = revision.ok_or_else(|| studio("Invalid plan revision."))? as u32;
        let (manifest, docs, digest) = self.revision_payload(revision)?;
        let bound = plan.get("manifest_sha256").is_some_and(|sha| sha.is_str(&digest))
            && plan
                .get("documents")
                .zip(manifest.get("documents"))
                .is_some_and(|(a, b)| py_eq(a, b))
            && !docs.is_empty();
        require(bound, "Plan document binding changed.")?;
        self.verify_documents(&docs)?;
        Ok(Plan {
            workspace_id: self.id.clone(),
            revision,
            manifest_sha256: digest,
            documents: docs,
            preset: shape,
            selected_experts: selected,
        })
    }
}

/// Opens an existing Training Studio workspace folder read-only, as `Workspace.open` does (folder identities,
/// canonical `workspace.json`, a gap-free revision history whose latest manifest binds every document snapshot by
/// SHA-256), then loads each `plans/*.json` as `Workspace.load_plan` does. A refused plan is listed with its
/// refusal and does not refuse the workspace (Python's `open` reads no plans; listing them is this port's addition,
/// sorted by file name, at most `MAX_REVISIONS` of them).
pub fn open_workspace(path: &Path) -> Result<Workspace, String> {
    open(path).map_err(workspace_message)
}

fn open(path: &Path) -> Res<Workspace> {
    let root = safe(path, Want::Dir)?;
    let mut roots = Vec::new();
    for name in [".", "documents", "revisions", "plans"] {
        let folder = if name == "." { root.clone() } else { root.join(name) };
        let stat = stat_path(&safe(&folder, Want::Dir)?).map_err(|_| Fail::Other)?;
        roots.push((name, stat.id));
    }
    let metadata = read_bounded(&root.join("workspace.json"), MAX_METADATA_BYTES)?;
    let value = decode_metadata(&metadata)?;
    let id = value.get("id").and_then(Py::str).unwrap_or_default().to_string();
    let valid = value.has_keys(&["schema", "kind", "id"])
        && value.get("schema").and_then(Py::int) == Some(1)
        && value.get("kind").is_some_and(|kind| kind.is_str("training-studio"))
        && is_lower_hex(&id, 32);
    require(valid, "Invalid workspace identity.")?;
    let mut opened = Opened {
        root: root.clone(),
        roots: roots.clone(),
        metadata: metadata.clone(),
        id: id.clone(),
        verified: HashMap::new(),
    };
    let revision = opened.latest()?;
    opened.verify_documents(&revision.documents)?;

    let unreadable = |_: io::Error| studio("Could not read workspace plans.");
    let mut paths = Vec::new();
    for entry in fs::read_dir(root.join("plans")).map_err(unreadable)? {
        let entry = entry.map_err(unreadable)?;
        if entry
            .file_name()
            .to_string_lossy()
            .to_ascii_lowercase()
            .ends_with(".json")
        {
            require((paths.len() as u64) < MAX_REVISIONS, "Too many saved plans.")?;
            paths.push(entry.path());
        }
    }
    paths.sort();
    let plans = paths
        .into_iter()
        .map(|path| {
            let plan = opened.load_plan(&path).map_err(workspace_message);
            PlanEntry { path, plan }
        })
        .collect();
    Ok(Workspace {
        path: root,
        id,
        revision,
        plans,
        roots,
        metadata,
    })
}

// ---------------------------------------------------------------------------------------------------------------
// Writing (the write side of workspace.py). Each write re-reads the workspace from disk first, as Python's
// `_current` does, and refuses when it is not the one this window last opened; afterwards the workspace is opened
// again, so what the window shows is what is on disk.

const REOPEN: &str = "Workspace changed in another editor. Reopen it before saving.";

fn py_int(value: u64) -> Py {
    Py::Int(PyInt {
        text: value.to_string(),
        small: Some(i128::from(value)),
    })
}

fn py_str(text: &str) -> Py {
    Py::Str(text.to_string())
}

fn document_value(doc: &Document) -> Py {
    Py::Dict(vec![
        ("id".to_string(), py_str(&doc.id)),
        ("name".to_string(), py_str(&doc.name)),
        ("source".to_string(), py_str(&doc.source)),
        ("license".to_string(), py_str(&doc.license)),
        ("group".to_string(), py_str(&doc.group)),
        ("sha256".to_string(), py_str(&doc.sha256)),
        ("bytes".to_string(), py_int(doc.bytes)),
    ])
}

fn documents_value(docs: &[Document]) -> Py {
    Py::List(docs.iter().map(document_value).collect())
}

/// `uuid.uuid4().hex`: 16 random bytes with the version (4) and variant (RFC 4122) bits set, as lowercase hex.
fn uuid4_hex() -> Res<String> {
    #[cfg(windows)]
    let mut bytes = win::random::<16>().map_err(|_| studio("No random identifier could be made."))?;
    #[cfg(not(windows))]
    let mut bytes = {
        let mut bytes = [0u8; 16];
        File::open("/dev/urandom")
            .and_then(|mut f| f.read_exact(&mut bytes))
            .map_err(|_| studio("No random identifier could be made."))?;
        bytes
    };
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// `workspace._write_new`: publish a new file, never replacing one. Written whole and fsynced; the parent folder must
/// be the same folder afterwards. A partial file left by a failure stays for inspection (the next open refuses it).
fn write_new(path: &Path, payload: &[u8]) -> Res<()> {
    let path = safe(path, Want::New)?;
    let parent = path.parent().ok_or(Fail::Other)?.to_path_buf();
    let before = stat_path(&safe(&parent, Want::Dir)?).map_err(|_| Fail::Other)?.id;
    let published = (|| -> io::Result<()> {
        let mut handle = fs::OpenOptions::new().write(true).create_new(true).open(&path)?;
        handle.write_all(payload)?;
        handle.flush()?;
        handle.sync_all()
    })();
    match published {
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            Err(studio("Another writer published first. Reopen the workspace."))
        }
        Err(_) => Err(studio("Publication failed; any partial file was retained for inspection.")),
        Ok(()) => {
            let after = stat_path(&safe(&parent, Want::Dir)?)
                .map_err(|_| studio("Publication failed; any partial file was retained for inspection."))?;
            require(after.id == before, "The destination folder changed during publication.")
        }
    }
}

/// `_current`: the workspace on disk is still the one `ws` describes (its folders, its identity file, and its latest
/// revision's number and manifest digest), every document re-verified. Returns the fresh reading.
fn current(ws: &Workspace) -> Res<Workspace> {
    let now = open(&ws.path)?;
    require(now.roots == ws.roots, "Workspace folder changed. Reopen the workspace.")?;
    require(now.metadata == ws.metadata, "Workspace identity changed.")?;
    require(
        now.revision.number == ws.revision.number && now.revision.sha256 == ws.revision.sha256,
        REOPEN,
    )?;
    Ok(now)
}

/// `_commit`: publish the next revision with `docs` as its inventory.
fn commit(ws: &Workspace, docs: &[Document]) -> Res<()> {
    let now = current(ws)?;
    let inventory = documents_value(docs);
    documents(&inventory)?;
    let number = u64::from(now.revision.number) + 1;
    require(number < MAX_REVISIONS, "Workspace revision limit reached.")?;
    let payload = canonical(&Py::Dict(vec![
        ("schema".to_string(), py_int(1)),
        ("workspace_id".to_string(), py_str(&now.id)),
        ("revision".to_string(), py_int(number)),
        ("documents".to_string(), inventory),
    ]))
    .ok_or(Fail::Other)?;
    require(payload.len() as u64 <= MAX_METADATA_BYTES, "Document metadata exceeds its size limit.")?;
    write_new(&now.path.join("revisions").join(format!("{number:08}.json")), &payload)
}

/// `Workspace.create`: a new folder (its parent must exist; it must not) with `documents/`, `revisions/` and
/// `plans/`, a new identity and an empty revision 0; then opened. Nothing is cleaned up after a failure.
pub fn create_workspace(new_root: &Path) -> Result<Workspace, String> {
    create(new_root).map_err(workspace_message)
}

fn create(new_root: &Path) -> Res<Workspace> {
    let root = safe(new_root, Want::New)?;
    let made = (|| -> io::Result<()> {
        fs::create_dir(&root)?;
        for name in ["documents", "revisions", "plans"] {
            fs::create_dir(root.join(name))?;
        }
        Ok(())
    })();
    made.map_err(|_| studio("Could not create a new workspace. Existing files were preserved."))?;
    let id = uuid4_hex()?;
    let identity = Py::Dict(vec![
        ("schema".to_string(), py_int(1)),
        ("kind".to_string(), py_str("training-studio")),
        ("id".to_string(), py_str(&id)),
    ]);
    write_new(&root.join("workspace.json"), &canonical(&identity).ok_or(Fail::Other)?)?;
    let first = Py::Dict(vec![
        ("schema".to_string(), py_int(1)),
        ("workspace_id".to_string(), py_str(&id)),
        ("revision".to_string(), py_int(0)),
        ("documents".to_string(), Py::List(Vec::new())),
    ]);
    write_new(&root.join("revisions").join("00000000.json"), &canonical(&first).ok_or(Fail::Other)?)?;
    open(&root)
}

/// What `add_document` stores for a file's bytes: decoded as `utf-8-sig` (one leading BOM dropped), CRLF and CR made
/// LF, NFC, then Python's `strip()`. None where it is not UTF-8 or cannot be normalised here.
fn normalised_document(payload: &[u8]) -> Option<String> {
    let payload = payload.strip_prefix(b"\xef\xbb\xbf").unwrap_or(payload);
    let text = std::str::from_utf8(payload).ok()?;
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    #[cfg(windows)]
    let text = win::nfc(&text)?;
    #[cfg(not(windows))]
    let text: String = return None;
    Some(py_strip(&text).to_string())
}

/// `Workspace.add_document`: a UTF-8 `.txt` or `.md` file (at most 4 MiB) copied into the pool as its normalised
/// text, named by its SHA-256, with a source ID, a licence and an optional group (else the digest). The same content
/// with the same attribution is already there and nothing is written; with another attribution it is refused. The
/// file itself is only read. Returns the reopened workspace and the document.
pub fn add_document(
    ws: &Workspace,
    file: &Path,
    source: &str,
    license: &str,
    group: &str,
) -> Result<(Workspace, Document), String> {
    add(ws, file, source, license, group).map_err(workspace_message)
}

fn add(ws: &Workspace, file: &Path, source: &str, license: &str, group: &str) -> Res<(Workspace, Document)> {
    let now = current(ws)?;
    let suffix = file.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase);
    require(
        file.file_name().is_some() && matches!(suffix.as_deref(), Some("txt" | "md")),
        "Choose a UTF-8 .txt or .md document.",
    )?;
    text_field(&py_str(source), "source", 256)?;
    require(is_source_id(source), "Source ID must use letters, numbers or . _ : / @ + -.")?;
    text_field(&py_str(license), "license", 256)?;
    let payload = read_bounded(file, MAX_DOCUMENT_BYTES)?;
    let content = normalised_document(&payload).ok_or_else(|| studio("The document must contain UTF-8 text."))?;
    require(!content.is_empty() && !content.contains('\0'), "The document is empty or contains binary content.")?;
    let stored = content.into_bytes();
    let digest = sha256_hex(&stored);
    let group = if group.is_empty() { digest.clone() } else { group.to_string() };
    if let Some(existing) = now.revision.documents.iter().find(|doc| doc.id == digest) {
        require(
            (existing.source.as_str(), existing.license.as_str(), existing.group.as_str())
                == (source, license, group.as_str()),
            "This content already has different attribution in the pool.",
        )?;
        let existing = existing.clone();
        return Ok((now, existing));
    }
    let name = file.file_name().and_then(|n| n.to_str()).ok_or_else(|| studio("Invalid name."))?;
    let doc = Document {
        id: digest.clone(),
        name: name.to_string(),
        source: source.to_string(),
        license: license.to_string(),
        group,
        sha256: digest.clone(),
        bytes: stored.len() as u64,
    };
    let mut docs = now.revision.documents.clone();
    docs.push(doc.clone());
    documents(&documents_value(&docs))?;
    let target = now.path.join("documents").join(format!("{digest}.txt"));
    if target.try_exists().map_err(|_| studio("The local path is unavailable."))? {
        require(
            read_bounded(&target, MAX_DOCUMENT_BYTES)? == stored,
            "A retained document snapshot is corrupted.",
        )?;
    } else {
        write_new(&target, &stored)?;
    }
    commit(&now, &docs)?;
    Ok((open(&now.path)?, doc))
}

/// `Workspace.remove_document`: a new revision without it. Its snapshot stays, as older plans bind it.
pub fn remove_document(ws: &Workspace, id: &str) -> Result<Workspace, String> {
    (|| {
        require(ws.revision.documents.iter().any(|doc| doc.id == id), "Document is not in this pool.")?;
        let docs: Vec<Document> = ws.revision.documents.iter().filter(|doc| doc.id != id).cloned().collect();
        commit(ws, &docs)?;
        open(&ws.path)
    })()
    .map_err(workspace_message)
}

/// `Workspace.save_plan`: a new `plans/<uuid4>.json` binding the latest revision (its number, manifest digest and
/// documents), the preset with its geometry and the chosen experts, sorted; never ready to train. Returns the
/// reopened workspace and the plan's path.
pub fn save_plan(ws: &Workspace, preset_key: &str, experts: &[String]) -> Result<(Workspace, PathBuf), String> {
    (|| {
        let now = current(ws)?;
        let chosen = Py::List(experts.iter().map(|e| py_str(e)).collect());
        let (shape, selected) = selection(Some(&py_str(preset_key)), Some(&chosen))?;
        require(!now.revision.documents.is_empty(), "Add a document before saving a training plan.")?;
        let plan = Py::Dict(vec![
            ("schema".to_string(), py_int(1)),
            ("kind".to_string(), py_str("training-studio-plan")),
            ("training_ready".to_string(), Py::Bool(false)),
            ("workspace_id".to_string(), py_str(&now.id)),
            ("revision".to_string(), py_int(u64::from(now.revision.number))),
            ("manifest_sha256".to_string(), py_str(&now.revision.sha256)),
            ("documents".to_string(), documents_value(&now.revision.documents)),
            ("preset".to_string(), py_str(shape.key)),
            ("geometry".to_string(), preset_value(shape)),
            ("selected_experts".to_string(), Py::List(selected.iter().map(|e| py_str(e)).collect())),
        ]);
        let path = now.path.join("plans").join(format!("{}.json", uuid4_hex()?));
        write_new(&path, &canonical(&plan).ok_or(Fail::Other)?)?;
        Ok((open(&now.path)?, path))
    })()
    .map_err(workspace_message)
}

/// `_document_text`: a snapshot's text, verified as an open verifies it.
fn document_text(root: &Path, doc: &Document) -> Res<String> {
    let payload = read_bounded(&root.join("documents").join(format!("{}.txt", doc.sha256)), MAX_DOCUMENT_BYTES)?;
    require(
        payload.len() as u64 == doc.bytes && sha256_hex(&payload) == doc.sha256,
        "A document snapshot changed or is corrupted.",
    )?;
    let text = String::from_utf8(payload).map_err(|_| studio("Document snapshot is not UTF-8."))?;
    #[cfg(windows)]
    let normal = win::nfc(&text).as_deref() == Some(text.as_str());
    #[cfg(not(windows))]
    let normal = true;
    require(
        !text.is_empty() && !text.contains('\r') && py_strip(&text) == text && !text.contains('\0') && normal,
        "Document snapshot is not normalized text.",
    )?;
    Ok(text)
}

/// `Workspace.export_corpus`: the pool's normalised texts as a new `.jsonl` file (which may be outside the
/// workspace; never one that exists), one canonical line per document with its attribution and a provenance string
/// that says it is a snapshot, not ready to train. Returns the reopened workspace and the file.
pub fn export_corpus(ws: &Workspace, new_file: &Path) -> Result<(Workspace, PathBuf), String> {
    (|| {
        let now = current(ws)?;
        require(!now.revision.documents.is_empty(), "Add a document before exporting a corpus snapshot.")?;
        let target = safe(new_file, Want::New)?;
        let suffix = target.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase);
        require(suffix.as_deref() == Some("jsonl"), "Choose a new .jsonl destination.")?;
        let mut rows = Vec::new();
        for doc in &now.revision.documents {
            let provenance = Py::Dict(vec![
                ("kind".to_string(), py_str("local-training-studio-snapshot")),
                ("name".to_string(), py_str(&doc.name)),
                ("workspace_id".to_string(), py_str(&now.id)),
                ("manifest_sha256".to_string(), py_str(&now.revision.sha256)),
                ("training_ready".to_string(), Py::Bool(false)),
            ]);
            // json.dumps(sort_keys=True, separators=(",", ":")): ensure_ascii is its default, so it is the canonical
            // text without the newline.
            let mut provenance_text = String::new();
            write_canonical(&provenance, &mut provenance_text).ok_or(Fail::Other)?;
            let row = Py::Dict(vec![
                ("id".to_string(), py_str(&doc.id)),
                ("source".to_string(), py_str(&doc.source)),
                ("revision".to_string(), py_str(&doc.sha256)),
                ("license".to_string(), py_str(&doc.license)),
                ("group".to_string(), py_str(&doc.group)),
                ("text".to_string(), Py::Str(document_text(&now.path, doc)?)),
                ("provenance".to_string(), Py::Str(provenance_text)),
            ]);
            rows.extend(canonical(&row).ok_or(Fail::Other)?);
        }
        let now = current(&now)?;
        write_new(&target, &rows)?;
        Ok((now, target))
    })()
    .map_err(workspace_message)
}

// ---------------------------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);

    /// A fresh folder under the system temp directory, removed on drop.
    struct Temp(PathBuf);

    impl Temp {
        fn new() -> Temp {
            let unique = format!(
                "centcom-training-{}-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed),
                SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .map(|d| d.as_nanos())
                    .unwrap_or(0)
            );
            let path = std::env::temp_dir().join(unique);
            fs::create_dir(&path).unwrap();
            Temp(path)
        }
    }

    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn json(value: &Value) -> Vec<u8> {
        let text = serde_json::to_string(value).unwrap();
        let (parsed, _) = Parser::parse(&text);
        canonical(&parsed.unwrap()).unwrap()
    }

    // --- JSON, canonical form and SHA-256 -------------------------------------------------------------------

    #[test]
    fn sha256_matches_published_vectors() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            sha256_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
        assert_eq!(
            sha256_hex(&vec![b'a'; 1_000_000]),
            "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
        );
    }

    #[test]
    fn parser_keeps_python_types_and_refuses_duplicates_and_constants() {
        let (value, _) = Parser::parse(r#"{"a":1,"b":1.0,"c":true,"d":-0,"e":[null]}"#);
        let value = value.unwrap();
        assert!(matches!(value.get("a"), Some(Py::Int(_))));
        assert!(matches!(value.get("b"), Some(Py::Float(_))));
        assert_eq!(value.get("c").and_then(Py::int), None); // bool is not int
        assert_eq!(
            canonical(&value).unwrap(),
            b"{\"a\":1,\"b\":1.0,\"c\":true,\"d\":0,\"e\":[null]}\n"
        );
        assert_eq!(Parser::parse(r#"{"a":1,"a":2}"#).0.unwrap_err(), JsonErr::Duplicate);
        // An escaped key equal to a literal one is a duplicate, as in Python.
        assert_eq!(Parser::parse(r#"{"é":1,"é":2}"#).0.unwrap_err(), JsonErr::Duplicate);
        // A constant met before the object closes wins over its duplicate.
        assert_eq!(Parser::parse(r#"{"a":1,"a":NaN}"#).0.unwrap_err(), JsonErr::Nonfinite);
        assert_eq!(
            Parser::parse(r#"{"a":{"b":1,"b":2},"c":NaN}"#).0.unwrap_err(),
            JsonErr::Duplicate
        );
        for text in ["NaN", "Infinity", "-Infinity", "[1,-Infinity]"] {
            assert_eq!(Parser::parse(text).0.unwrap_err(), JsonErr::Nonfinite, "{text}");
        }
        for text in [
            "",
            "01",
            "1.",
            "[1,]",
            "{\"a\":1,}",
            "\"\u{1}\"",
            "-",
            "nul",
            "\u{feff}{}",
            "{} x",
            "-NaN",
        ] {
            assert_eq!(Parser::parse(text).0.unwrap_err(), JsonErr::Syntax, "{text:?}");
        }
        assert!(matches!(Parser::parse("1e999").0, Ok(Py::Float(f)) if f.is_infinite()));
        assert_eq!(
            Parser::parse(&"[".repeat(MAX_DEPTH + 1)).0.unwrap_err(),
            JsonErr::TooDeep
        );
        assert_eq!(Parser::parse(&"9".repeat(4301)).0.unwrap_err(), JsonErr::IntTooLong);
        let (pair, lone) = Parser::parse(r#""😀\ud800""#);
        assert!(lone);
        assert_eq!(pair.unwrap().str(), Some("\u{1f600}\u{fffd}"));
    }

    #[test]
    fn float_repr_matches_python() {
        let cases = [
            (0.1, "0.1"),
            (1.0, "1.0"),
            (-0.0, "-0.0"),
            (1e16, "1e+16"),
            (1234567890123456.0, "1234567890123456.0"),
            (1e-5, "1e-05"),
            (0.0001, "0.0001"),
            (1.5e-7, "1.5e-07"),
            (123.456, "123.456"),
            (1e300, "1e+300"),
            (-2.5e22, "-2.5e+22"),
        ];
        for (number, text) in cases {
            assert_eq!(py_float_repr(number), text);
        }
    }

    // --- Workspace ----------------------------------------------------------------------------------------------

    const ID: &str = "0123456789abcdef0123456789abcdef";

    struct Studio {
        temp: Temp,
        root: PathBuf,
        docs: Vec<Value>,
    }

    /// A workspace laid out as Workspace.create, add_document and save_plan write it.
    fn studio(texts: &[&str]) -> Studio {
        let temp = Temp::new();
        let root = temp.0.join("studio");
        for name in ["", "documents", "revisions", "plans"] {
            fs::create_dir(root.join(name)).unwrap();
        }
        fs::write(
            root.join("workspace.json"),
            json(&serde_json::json!({"schema":1,"kind":"training-studio","id":ID})),
        )
        .unwrap();
        let empty = serde_json::json!({"schema":1,"workspace_id":ID,"revision":0,"documents":[]});
        fs::write(root.join("revisions/00000000.json"), json(&empty)).unwrap();
        let mut docs = Vec::new();
        for (index, text) in texts.iter().enumerate() {
            let sha = sha256_hex(text.as_bytes());
            fs::write(root.join(format!("documents/{sha}.txt")), text).unwrap();
            docs.push(
                serde_json::json!({"id":sha,"name":format!("doc{index}.md"),"source":"local",
                "license":"Owner-provided","group":sha,"sha256":sha,"bytes":text.len()}),
            );
            let revision = serde_json::json!({"schema":1,"workspace_id":ID,"revision":index+1,"documents":docs});
            fs::write(root.join(format!("revisions/{:08}.json", index + 1)), json(&revision)).unwrap();
        }
        Studio { temp, root, docs }
    }

    fn plan_value(studio: &Studio, preset: &str, selected: &[&str]) -> Value {
        let revision = studio.docs.len();
        let manifest = fs::read(studio.root.join(format!("revisions/{revision:08}.json"))).unwrap();
        let shape = super::preset(preset).unwrap();
        serde_json::json!({"schema":1,"kind":"training-studio-plan","training_ready":false,"workspace_id":ID,
            "revision":revision,"manifest_sha256":sha256_hex(&manifest),"documents":studio.docs,"preset":preset,
            "geometry":{"label":shape.label,"layers":shape.layers,"experts":shape.experts,"active":shape.active,
            "width":shape.width},"selected_experts":selected})
    }

    fn save_plan(studio: &Studio, name: &str, plan: &Value) -> PathBuf {
        let path = studio.root.join("plans").join(name);
        fs::write(&path, json(plan)).unwrap();
        path
    }

    #[test]
    fn a_valid_workspace_opens_with_its_latest_revision_and_plans() {
        let studio = studio(&["Café\nA local document.", "The second document."]);
        let first = save_plan(&studio, "aaaa.json", &plan_value(&studio, "s", &["0:0", "3:7"]));
        save_plan(&studio, "bbbb.json", &plan_value(&studio, "m", &["5:15"]));
        fs::write(studio.root.join("plans/notes.txt"), "not a plan").unwrap();
        let workspace = open_workspace(&studio.root).unwrap();
        assert_eq!(workspace.id, ID);
        assert_eq!(workspace.revision.number, 2);
        assert_eq!(workspace.revision.documents.len(), 2);
        assert_eq!(workspace.revision.documents[0].name, "doc0.md");
        assert_eq!(
            workspace.revision.documents[0].bytes,
            "Café\nA local document.".len() as u64
        );
        let manifest = fs::read(studio.root.join("revisions/00000002.json")).unwrap();
        assert_eq!(workspace.revision.sha256, sha256_hex(&manifest));
        assert_eq!(workspace.plans.len(), 2);
        assert_eq!(workspace.plans[0].path, std::path::absolute(&first).unwrap());
        let plan = workspace.plans[0].plan.as_ref().unwrap();
        assert_eq!(plan.selected_experts, ["0:0", "3:7"]);
        assert_eq!(plan.preset.key, "s");
        assert_eq!(plan.revision, 2);
        assert_eq!(workspace.plans[1].plan.as_ref().unwrap().preset.key, "m");
    }

    #[test]
    fn plan_refusals_are_listed_without_refusing_the_workspace() {
        let studio = studio(&["A document for the plans."]);
        // Unsorted selection: Python's _selection sorts, then the stored list must equal it.
        save_plan(&studio, "a.json", &plan_value(&studio, "s", &["3:7", "0:0"]));
        let mut tampered = plan_value(&studio, "s", &["0:0"]);
        tampered["documents"][0]["source"] = Value::from("changed");
        save_plan(&studio, "b.json", &tampered);
        let mut ready = plan_value(&studio, "s", &["0:0"]);
        ready["training_ready"] = Value::from(true);
        save_plan(&studio, "c.json", &ready);
        let mut unknown = plan_value(&studio, "s", &["0:0"]);
        unknown["preset"] = Value::from("unknown");
        save_plan(&studio, "d.json", &unknown);
        save_plan(&studio, "e.json", &plan_value(&studio, "s", &["00:0"]));
        save_plan(&studio, "f.json", &plan_value(&studio, "s", &["0:0", "0:0"]));
        save_plan(&studio, "g.json", &plan_value(&studio, "s", &["4:0"]));
        let mut floats = plan_value(&studio, "s", &["0:0"]);
        floats["geometry"]["layers"] = serde_json::json!(4.0);
        save_plan(&studio, "h.json", &floats); // Python: 4.0 == 4, so the geometry matches.
        fs::write(studio.root.join("plans/i.json"), b"{\"schema\":1}").unwrap(); // not canonical (no newline)
        let workspace = open_workspace(&studio.root).unwrap();
        let refusals: Vec<_> = workspace
            .plans
            .iter()
            .map(|entry| entry.plan.as_ref().err().cloned())
            .collect();
        assert_eq!(
            refusals,
            [
                Some("Plan geometry changed.".to_string()),
                Some("Plan document binding changed.".to_string()),
                Some("Invalid training plan.".to_string()),
                Some("Choose a supported model preset.".to_string()),
                Some("Expert selection does not match the model preset.".to_string()),
                Some("Expert selection does not match the model preset.".to_string()),
                Some("Expert selection does not match the model preset.".to_string()),
                None,
                Some("Metadata is not canonical JSON.".to_string()),
            ]
        );
    }

    #[test]
    fn mutated_or_incomplete_workspaces_refuse() {
        type Mutation = (&'static str, fn(&Studio), &'static str);
        let cases: [Mutation; 7] = [
            (
                "content",
                |s| {
                    fs::write(
                        s.root.join(format!("documents/{}.txt", sha256_hex(b"Some text."))),
                        "x".repeat(10),
                    )
                    .unwrap()
                },
                "A document snapshot changed or is corrupted.",
            ),
            (
                "missing",
                |s| fs::remove_file(s.root.join(format!("documents/{}.txt", sha256_hex(b"Some text.")))).unwrap(),
                "The parent folder must already exist.",
            ),
            (
                "manifest",
                |s| fs::write(s.root.join("revisions/00000001.json"), "{}\n").unwrap(),
                "Invalid workspace revision.",
            ),
            (
                "partial",
                |s| fs::write(s.root.join("revisions/00000002.json"), "{\"schema\":").unwrap(),
                "Unreadable workspace metadata.",
            ),
            (
                "gap",
                |s| {
                    fs::rename(
                        s.root.join("revisions/00000001.json"),
                        s.root.join("revisions/00000003.json"),
                    )
                    .unwrap()
                },
                "Workspace revision history is incomplete.",
            ),
            (
                "stray",
                |s| fs::write(s.root.join("revisions/notes.txt"), "x").unwrap(),
                "Invalid or exhausted revision inventory.",
            ),
            (
                "identity",
                |s| {
                    fs::write(
                        s.root.join("workspace.json"),
                        "{\"id\":\"x\",\"kind\":\"training-studio\",\"schema\":1}\n",
                    )
                    .unwrap()
                },
                "Invalid workspace identity.",
            ),
        ];
        for (name, mutate, message) in cases {
            let studio = studio(&["Some text."]);
            assert!(open_workspace(&studio.root).is_ok(), "{name}: control opens");
            mutate(&studio);
            assert_eq!(open_workspace(&studio.root).unwrap_err(), message, "{name}");
        }
    }

    #[test]
    fn duplicate_keys_nonfinite_and_unnormalized_metadata_refuse() {
        let studio = studio(&[]);
        let revision = studio.root.join("revisions/00000000.json");
        fs::write(
            &revision,
            format!("{{\"documents\":[],\"revision\":0,\"revision\":0,\"schema\":1,\"workspace_id\":\"{ID}\"}}\n"),
        )
        .unwrap();
        assert_eq!(open_workspace(&studio.root).unwrap_err(), "Duplicate metadata key.");
        fs::write(&revision, "{\"documents\":NaN}\n").unwrap();
        assert_eq!(open_workspace(&studio.root).unwrap_err(), "Nonfinite metadata.");
        fs::write(
            &revision,
            format!("{{\"schema\":1,\"workspace_id\":\"{ID}\",\"revision\":0,\"documents\":[]}}\n"),
        )
        .unwrap();
        assert_eq!(
            open_workspace(&studio.root).unwrap_err(),
            "Metadata is not canonical JSON."
        );
        let unnormalized = studio_with_raw_document(" padded text");
        assert_eq!(
            open_workspace(&unnormalized.root).unwrap_err(),
            "Document snapshot is not normalized text."
        );
        let crlf = studio_with_raw_document("one\r\ntwo");
        assert_eq!(
            open_workspace(&crlf.root).unwrap_err(),
            "Document snapshot is not normalized text."
        );
    }

    /// A one-document workspace whose snapshot is written as given (Python's add_document would have normalised it).
    fn studio_with_raw_document(text: &str) -> Studio {
        studio(&[text])
    }

    #[test]
    fn parent_folders_and_dot_dot_are_refused() {
        let studio = studio(&[]);
        let dotted = studio.root.join("plans").join("..");
        assert_eq!(open_workspace(&dotted).unwrap_err(), "Invalid local path.");
        assert_eq!(
            open_workspace(&studio.temp.0.join("absent")).unwrap_err(),
            "The parent folder must already exist."
        );
        assert_eq!(
            open_workspace(&studio.root.join("workspace.json")).unwrap_err(),
            "The local path has the wrong type."
        );
    }

    /// A junction (Windows, no privilege needed) or a symlink (Unix) to the workspace, and a link inside it.
    #[test]
    fn links_and_junctions_are_refused() {
        let studio = studio(&["Some text."]);
        let alias = studio.temp.0.join("alias");
        if !make_dir_link(&studio.root, &alias) {
            eprintln!("SKIPPED links_and_junctions_are_refused: could not create a directory link or junction here");
            return;
        }
        assert_eq!(
            open_workspace(&alias).unwrap_err(),
            "Links and junctions are not supported."
        );
        // A link inside the workspace (the plans folder replaced by a link to another folder).
        let elsewhere = studio.temp.0.join("elsewhere");
        fs::create_dir(&elsewhere).unwrap();
        fs::remove_dir(studio.root.join("plans")).unwrap();
        assert!(make_dir_link(&elsewhere, &studio.root.join("plans")));
        assert_eq!(
            open_workspace(&studio.root).unwrap_err(),
            "Links and junctions are not supported."
        );
    }

    #[cfg(windows)]
    fn make_dir_link(target: &Path, link: &Path) -> bool {
        // mklink /J creates a junction without the symlink privilege.
        std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .output()
            .is_ok_and(|output| output.status.success())
    }

    #[cfg(unix)]
    fn make_dir_link(target: &Path, link: &Path) -> bool {
        std::os::unix::fs::symlink(target, link).is_ok()
    }

    #[cfg(not(any(windows, unix)))]
    fn make_dir_link(_: &Path, _: &Path) -> bool {
        false
    }

    #[test]
    fn presets_match_python() {
        let keys: Vec<_> = PRESETS
            .iter()
            .map(|p| (p.key, p.label, p.layers, p.experts, p.active, p.width))
            .collect();
        assert_eq!(
            keys,
            [
                ("s", "Small", 4, 8, 2, 128),
                ("m", "Medium", 6, 16, 2, 256),
                ("l", "Large", 8, 32, 2, 384)
            ]
        );
    }

    // --- Writing ----------------------------------------------------------------------------------------------

    fn created() -> (Temp, Workspace) {
        let temp = Temp::new();
        let ws = create_workspace(&temp.0.join("studio")).unwrap();
        (temp, ws)
    }

    fn file(temp: &Temp, name: &str, bytes: &[u8]) -> PathBuf {
        let path = temp.0.join(name);
        fs::write(&path, bytes).unwrap();
        path
    }

    fn add_owned(ws: &Workspace, path: &Path) -> Result<(Workspace, Document), String> {
        add_document(ws, path, "local", "Owner-provided", "")
    }

    fn listing(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> =
            fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        names
    }

    /// `json.dumps(..., ensure_ascii=False)`'s inputs and what workspace.add_document stores for each (utf-8-sig, LF,
    /// NFC, strip), with its SHA-256, from CPython 3.12.10 / Unicode 15.0.0 (scratch script golden_nfc.py,
    /// 2026-10-07), as hex.
    const PYTHON_DOCUMENTS: [(&str, &str, &str); 9] = [
        ("202043616665cc810d0a41206c6f63616c20646f63756d656e742e2020", "436166c3a90a41206c6f63616c20646f63756d656e742e", "0dc2b97a23f249883849771119a5d0d8d1c20f360559a979dd189d2499360a8f"),
        ("efbbbf424f4d207468656e20746578740d0d74776f20435273", "424f4d207468656e20746578740a0a74776f20435273", "5b1d5dfcd1d996ed48a92bf23a73fbc96b54aba5c1b3519c164436f2431fb5df"),
        ("e284ab20616e677374726f6d207369676e20616e6420e284a6206f686d", "c38520616e677374726f6d207369676e20616e6420cea9206f686d", "efea9c92740df47963b221013efcfce73420b84228f0fcc49dadd6d484870a85"),
        ("e18480e185a1e186a82048616e67756c206a616d6f", "eab0812048616e67756c206a616d6f", "6b4110bdeaf541e02b072a3d4fb3ea84f42396e1204966bad0140dd14b105d9a"),
        ("65cca3cc8220616e6420e1bb8720616e642071cc87cca3", "e1bb8720616e6420e1bb8720616e642071cca3cc87", "6debeb986476fdc6fb1a5a456b992310dc422d55a358894bd01e7b323edb0e26"),
        ("cd8420e0a59820e2ab9c20efac9d", "cc88cc8120e0a495e0a4bc20e2ab9dccb820d799d6b4", "9a7af88dbced2c2897ed1dd2365591bb25f58a069d9079624a154dfde1ed9a71"),
        ("66616d696c7920f09f91a8e2808df09f91a9e2808df09f91a720efb88fe283a32078f09d859e", "66616d696c7920f09f91a8e2808df09f91a9e2808df09f91a720efb88fe283a32078f09d8597f09d85a5", "5f502012fc504abcee5ace30b77d23add2a9add4533ac583ee27e206121959de"),
        ("efbca1efac8120636f6d7061746962696c697479206973204e4f5420666f6c646564206279204e4643", "efbca1efac8120636f6d7061746962696c697479206973204e4f5420666f6c646564206279204e4643", "4ce15e669a9b21cce838de5e329c528007af261cb9d95ab8a16e0a3babfe1aaf"),
        ("e38080e280a8206964656f6772617068696320737061636520616e64206c696e6520736570617261746f722073747269707065643f20c285", "6964656f6772617068696320737061636520616e64206c696e6520736570617261746f722073747269707065643f", "397c5e6b9f61592740c7e3dc8d73803bf2b5d249e5ee18ba3fede969c5741003"),
    ];

    fn unhex(text: &str) -> Vec<u8> {
        (0..text.len()).step_by(2).map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap()).collect()
    }

    #[cfg(windows)]
    #[test]
    fn a_document_is_stored_as_python_stores_it() {
        for (raw, stored, digest) in PYTHON_DOCUMENTS {
            let text = normalised_document(&unhex(raw)).expect("normalised");
            assert_eq!(text.as_bytes(), unhex(stored).as_slice(), "{raw}");
            assert_eq!(sha256_hex(text.as_bytes()), digest);
        }
        // Not UTF-8 is refused, as Python's decode refuses it.
        assert_eq!(normalised_document(b"\xff"), None);
    }

    #[cfg(windows)]
    #[test]
    fn a_new_workspace_pools_normalised_documents_and_binds_plans() {
        let (temp, ws) = created();
        assert!(is_lower_hex(&ws.id, 32) && ws.id.as_bytes()[12] == b'4', "a uuid4: {}", ws.id);
        assert!(matches!(ws.id.as_bytes()[16], b'8' | b'9' | b'a' | b'b'));
        assert_eq!((ws.revision.number, ws.revision.documents.len(), ws.plans.len()), (0, 0, 0));
        let path = file(&temp, "notes.md", "  Cafe\u{301}\r\nA local document.  ".as_bytes());
        let original = fs::read(&path).unwrap();
        let (ws, doc) = add_owned(&ws, &path).unwrap();
        assert_eq!(fs::read(&path).unwrap(), original, "the chosen file is only read");
        let expected = "Caf\u{e9}\nA local document.";
        let blob = ws.path.join("documents").join(format!("{}.txt", doc.sha256));
        assert_eq!(fs::read_to_string(&blob).unwrap(), expected);
        assert_eq!(doc.bytes, expected.len() as u64);
        assert_eq!((doc.name.as_str(), doc.group.as_str()), ("notes.md", doc.sha256.as_str()));
        assert_eq!(open_workspace(&ws.path).unwrap().revision.documents, vec![doc.clone()]);

        let (ws, plan_path) = super::save_plan(&ws, "s", &["3:7".to_string(), "0:0".to_string()]).unwrap();
        let plan = ws.plans.iter().find(|p| p.path == plan_path).unwrap().plan.clone().unwrap();
        assert_eq!(plan.selected_experts, ["0:0", "3:7"]);
        assert_eq!((plan.revision, plan.manifest_sha256.as_str()), (1, ws.revision.sha256.as_str()));

        fs::write(&path, b"The original was changed after import.").unwrap();
        let ws = remove_document(&ws, &doc.id).unwrap();
        assert_eq!((ws.revision.number, ws.revision.documents.len()), (2, 0));
        let kept = ws.plans.iter().find(|p| p.path == plan_path).unwrap();
        assert_eq!(kept.plan.as_ref().unwrap(), &plan, "removing a document keeps the plans that bind it");
        assert!(blob.is_file(), "its snapshot stays");
        assert_eq!(listing(&ws.path.join("revisions")), ["00000000.json", "00000001.json", "00000002.json"]);
    }

    #[cfg(windows)]
    #[test]
    fn the_same_content_is_one_document_and_nothing_is_overwritten() {
        let (temp, ws) = created();
        let path = file(&temp, "notes.md", b"The local document explains several useful concepts.");
        let (ws, doc) = add_owned(&ws, &path).unwrap();
        let (again, same) = add_owned(&ws, &path).unwrap();
        assert_eq!((same, again.revision.number), (doc.clone(), 1), "the same attribution writes nothing");
        let other = add_document(&again, &path, "other", "Owner-provided", "");
        assert!(other.unwrap_err().contains("attribution"));
        assert!(create_workspace(&again.path).unwrap_err().contains("already exists"));
        // A publication never replaces a file.
        let existing = file(&temp, "existing.txt", b"owner content");
        assert_eq!(write_new(&existing, b"new"), Err(super::studio("The destination already exists.")));
        assert_eq!(fs::read(&existing).unwrap(), b"owner content");
        assert_eq!(open_workspace(&again.path).unwrap().revision.documents, vec![doc]);
    }

    #[cfg(windows)]
    #[test]
    fn a_stale_window_cannot_write_over_another_editor() {
        let (temp, ws) = created();
        let stale = ws.clone();
        let (ws, doc) = add_owned(&ws, &file(&temp, "a.txt", b"The first writer's document.")).unwrap();
        let refused = add_owned(&stale, &file(&temp, "b.txt", b"The other writer has different content."));
        assert!(refused.unwrap_err().contains("another editor"));
        assert!(remove_document(&stale, &doc.id).is_err(), "it is not in the stale pool");
        assert!(super::save_plan(&stale, "s", &["0:0".to_string()]).unwrap_err().contains("another editor"));
        assert_eq!(open_workspace(&ws.path).unwrap().revision.documents, vec![doc]);
        assert_eq!(listing(&ws.path.join("documents")).len(), 1);
    }

    #[cfg(windows)]
    #[test]
    fn an_invalid_selection_or_input_changes_nothing() {
        let (temp, ws) = created();
        assert!(super::save_plan(&ws, "s", &["0:0".to_string()]).unwrap_err().contains("Add a document"));
        let (ws, _) = add_owned(&ws, &file(&temp, "notes.md", b"A document.")).unwrap();
        for (preset, chosen) in [
            ("unknown", vec!["0:0"]),
            ("s", vec![]),
            ("s", vec!["0:0", "0:0"]),
            ("s", vec!["4:0"]),
            ("s", vec!["0:8"]),
            ("s", vec!["00:0"]),
        ] {
            let chosen: Vec<String> = chosen.into_iter().map(String::from).collect();
            assert!(super::save_plan(&ws, preset, &chosen).is_err(), "{preset} {chosen:?}");
        }
        assert!(listing(&ws.path.join("plans")).is_empty());
        let big = vec![b'x'; MAX_DOCUMENT_BYTES as usize + 1];
        for (name, payload) in [
            ("bad.txt", &b"\xff"[..]),
            ("bad.txt", &b"\x00binary"[..]),
            ("bad.txt", &b"    "[..]),
            ("bad.pdf", &b"data"[..]),
            ("large.txt", &big[..]),
        ] {
            assert!(add_owned(&ws, &file(&temp, name, payload)).is_err(), "{name}");
        }
        for (source, license) in [("", "x"), (" local", "x"), ("-local", "x"), ("local", ""), ("local", "a\u{7f}")] {
            let path = file(&temp, "ok.txt", b"Another document.");
            assert!(add_document(&ws, &path, source, license, "").is_err(), "{source:?} {license:?}");
        }
        let now = open_workspace(&ws.path).unwrap();
        assert_eq!((now.revision.number, listing(&ws.path.join("documents")).len()), (1, 1));
    }

    #[cfg(windows)]
    #[test]
    fn an_export_is_new_jsonl_and_never_overwrites() {
        let (temp, ws) = created();
        assert!(export_corpus(&ws, &temp.0.join("empty.jsonl")).unwrap_err().contains("Add a document"));
        let (ws, doc) = add_owned(&ws, &file(&temp, "notes.md", b"Exported text.")).unwrap();
        assert!(export_corpus(&ws, &temp.0.join("out.txt")).unwrap_err().contains(".jsonl"));
        let existing = file(&temp, "existing.jsonl", b"owner content");
        assert!(export_corpus(&ws, &existing).unwrap_err().contains("already exists"));
        assert_eq!(fs::read(&existing).unwrap(), b"owner content");
        let (_, written) = export_corpus(&ws, &temp.0.join("snapshot.jsonl")).unwrap();
        let line: Value = serde_json::from_slice(&fs::read(&written).unwrap()).unwrap();
        assert_eq!((line["text"].as_str(), line["revision"].as_str()), (Some("Exported text."), Some(doc.sha256.as_str())));
        let provenance: Value = serde_json::from_str(line["provenance"].as_str().unwrap()).unwrap();
        assert_eq!(provenance["training_ready"], Value::Bool(false));
    }

    /// Run by hand against a workspace another program wrote (the Python Studio, say): it opens, and each plan loads.
    #[test]
    #[ignore = "reads the workspace TRAINING_STUDIO_WORKSPACE names: run by hand"]
    fn a_workspace_named_by_hand_opens() {
        let path = std::env::var("TRAINING_STUDIO_WORKSPACE").expect("TRAINING_STUDIO_WORKSPACE");
        let ws = open_workspace(Path::new(&path)).unwrap();
        println!("revision {} with {:?}", ws.revision.number, ws.revision.documents.iter().map(|d| &d.name).collect::<Vec<_>>());
        for entry in &ws.plans {
            println!("{}: {:?}", entry.path.display(), entry.plan.as_ref().map(|p| (&p.selected_experts, p.revision)));
            assert!(entry.plan.is_ok());
        }
        // With TRAINING_STUDIO_EXPORT named, the pool is exported there too, to compare with Python's export.
        if let Ok(target) = std::env::var("TRAINING_STUDIO_EXPORT") {
            let (_, written) = export_corpus(&ws, Path::new(&target)).unwrap();
            println!("exported {}", written.display());
        }
    }

    // --- Telemetry ------------------------------------------------------------------------------------------

}
