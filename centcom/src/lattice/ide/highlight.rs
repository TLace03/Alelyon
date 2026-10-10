//! Syntax colours for the IDE: one small lexer per family of languages, run a line at a time.
//!
//! No grammar library is in CENTCOM's lockfile (iced's `highlighter` feature would bring syntect, which is not there),
//! and nothing is downloaded without the person's say-so, so this module colours the languages the project is written in
//! and those it reads by itself: keywords, types, names being declared or called, strings, numbers, comments,
//! attributes, keys and headings. It never parses, and a wrong colour is the worst it can do: the text is never
//! changed.
//!
//! A line's colours depend only on the line and on what the line before it left open ([`Carry`]: a block comment, a
//! string that runs on, a Markdown code fence, an HTML tag split over lines). [`Highlighter`] keeps the carry at the
//! start of every line it has coloured, so after an edit iced asks again from the changed line down.
//!
//! Every range returned is non-empty, in order, does not overlap the next, and starts and ends on a character boundary
//! (the lexers only ever cut at ASCII bytes, and treat every other byte as part of the word or string it is in).

use std::ops::Range;

use iced::Color;

/// A file's language, as far as its colours go.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Lang {
    Rust,
    Python,
    /// JavaScript and TypeScript, with JSX.
    Js,
    /// C and C++.
    C,
    CSharp,
    Go,
    /// Java and Kotlin.
    Java,
    /// WGSL, GLSL and HLSL.
    Shader,
    Json,
    Toml,
    Yaml,
    Markdown,
    Shell,
    PowerShell,
    Batch,
    Sql,
    /// HTML, XML and SVG.
    Html,
    Css,
    Ini,
    #[default]
    Plain,
}

impl Lang {
    /// The language a path's name says (case is ignored).
    pub fn of(path: &str) -> Lang {
        let name = path.rsplit(['/', '\\']).next().unwrap_or(path).to_ascii_lowercase();
        match name.as_str() {
            "cargo.lock" | "pipfile" | "poetry.lock" => return Lang::Toml,
            "dockerfile" | "makefile" | ".bashrc" | ".profile" | ".gitignore" | ".gitattributes" | ".latticeignore"
            | ".dockerignore" => return Lang::Shell,
            ".editorconfig" | ".npmrc" | ".gitconfig" => return Lang::Ini,
            _ => {}
        }
        let Some((_, ext)) = name.rsplit_once('.') else { return Lang::Plain };
        match ext {
            "rs" => Lang::Rust,
            "py" | "pyi" | "pyw" => Lang::Python,
            "js" | "jsx" | "ts" | "tsx" | "mjs" | "cjs" | "mts" | "cts" | "astro" | "vue" | "svelte" => Lang::Js,
            "c" | "h" | "cc" | "cpp" | "cxx" | "hpp" | "hh" | "hxx" | "inl" | "ino" | "m" | "mm" => Lang::C,
            "cs" | "csx" => Lang::CSharp,
            "go" => Lang::Go,
            "java" | "kt" | "kts" | "scala" | "groovy" | "gradle" => Lang::Java,
            "wgsl" | "glsl" | "hlsl" | "vert" | "frag" | "comp" | "geom" | "tesc" | "tese" | "spv" | "metal" => {
                Lang::Shader
            }
            "json" | "jsonc" | "jsonl" | "ndjson" | "json5" | "ipynb" | "geojson" | "webmanifest" => Lang::Json,
            "toml" => Lang::Toml,
            "yml" | "yaml" => Lang::Yaml,
            "md" | "markdown" | "mdx" => Lang::Markdown,
            "sh" | "bash" | "zsh" | "fish" | "ksh" | "env" => Lang::Shell,
            "ps1" | "psm1" | "psd1" => Lang::PowerShell,
            "bat" | "cmd" => Lang::Batch,
            "sql" => Lang::Sql,
            "html" | "htm" | "xml" | "svg" | "xaml" | "xsd" | "xsl" | "plist" | "csproj" | "props" | "targets"
            | "vcxproj" | "resx" => Lang::Html,
            "css" | "scss" | "sass" | "less" => Lang::Css,
            "ini" | "cfg" | "conf" | "properties" | "inf" | "reg" => Lang::Ini,
            _ => Lang::Plain,
        }
    }

    /// The language's name, as the status bar says it.
    pub fn name(self) -> &'static str {
        match self {
            Lang::Rust => "Rust",
            Lang::Python => "Python",
            Lang::Js => "JavaScript / TypeScript",
            Lang::C => "C / C++",
            Lang::CSharp => "C#",
            Lang::Go => "Go",
            Lang::Java => "Java / Kotlin",
            Lang::Shader => "Shader",
            Lang::Json => "JSON",
            Lang::Toml => "TOML",
            Lang::Yaml => "YAML",
            Lang::Markdown => "Markdown",
            Lang::Shell => "Shell",
            Lang::PowerShell => "PowerShell",
            Lang::Batch => "Batch",
            Lang::Sql => "SQL",
            Lang::Html => "HTML / XML",
            Lang::Css => "CSS",
            Lang::Ini => "INI",
            Lang::Plain => "Plain text",
        }
    }
}

/// What a coloured piece of a line is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Keyword,
    Type,
    /// A function or method being declared or called.
    Function,
    String,
    Number,
    /// `true`, `None`, `null`, an ALL_CAPS name.
    Constant,
    Comment,
    /// `#[derive]`, `@decorator`, a Rust lifetime, a YAML anchor.
    Attribute,
    /// A Rust macro call, a C preprocessor line.
    Macro,
    /// A key in JSON, TOML, YAML or INI, an HTML attribute, a CSS property.
    Key,
    /// A Markdown heading, a TOML or INI section.
    Heading,
    /// An HTML or XML tag.
    Tag,
    /// A shell or PowerShell variable.
    Variable,
}

/// Each kind's colour on the editor's black (and on the panels' surface): every one holds 4.5:1 there (a test
/// asserts it with the theme's own contrast formula). Gold stays the brand's: keywords take it.
pub fn color(kind: Kind) -> Color {
    match kind {
        Kind::Keyword => lattice_app::theme::GOLD,
        Kind::Type => Color::from_rgb8(0x8f, 0xc7, 0xd6),
        Kind::Function => Color::from_rgb8(0xe8, 0xd9, 0xb0),
        Kind::String => Color::from_rgb8(0xa7, 0xc8, 0x8a),
        Kind::Number | Kind::Constant => Color::from_rgb8(0xd9, 0xa0, 0x7a),
        Kind::Comment => lattice_app::theme::TEXT_FAINT,
        Kind::Attribute | Kind::Macro => Color::from_rgb8(0xc9, 0xa8, 0xdc),
        Kind::Key => lattice_app::theme::MODEL,
        Kind::Heading => lattice_app::theme::GOLD_STRONG,
        Kind::Tag => Color::from_rgb8(0xe3, 0xa6, 0xa6),
        Kind::Variable => Color::from_rgb8(0x9f, 0xd4, 0xc4),
    }
}

/// What a line leaves open for the next.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Carry {
    #[default]
    Code,
    /// Inside a block comment (`/* */`, `<# #>`, `<!-- -->`), nested `depth` deep (Rust nests them).
    Block { depth: u8 },
    /// Inside a string that runs on (`"…"` in Rust, `` `…` `` in JavaScript and Go, `r#"…"#`): its quote, the
    /// hashes that close it, and whether a backslash escapes.
    Str { quote: u8, hashes: u8, raw: bool },
    /// Inside `"""…"""` or `'''…'''`.
    Triple { quote: u8, raw: bool },
    /// Inside a Markdown code fence.
    Fence,
    /// Inside a PowerShell here-string, closed by a line that starts with `quote` then `@`.
    Here { quote: u8 },
    /// Inside an HTML or XML tag that goes on to the next line.
    Tag,
}

/// The coloured pieces of one line.
pub type Spans = Vec<(Range<usize>, Kind)>;

/// The most bytes of a line coloured; the rest of a longer line keeps the plain colour.
pub const LINE_LIMIT: usize = 4096;

/// Colour one line, given what the line before left open.
pub fn line(lang: Lang, text: &str, carry: Carry) -> (Spans, Carry) {
    let cut = if text.len() > LINE_LIMIT {
        // The last character boundary at or before the limit.
        (0..=LINE_LIMIT).rev().find(|&i| text.is_char_boundary(i)).unwrap_or(0)
    } else {
        text.len()
    };
    let mut lx = Lexer { b: &text.as_bytes()[..cut], i: 0, out: Vec::new() };
    let next = match lang {
        Lang::Plain => carry,
        Lang::Json => {
            json(&mut lx, carry);
            Carry::Code
        }
        Lang::Toml => toml(&mut lx, carry),
        Lang::Yaml => {
            yaml(&mut lx);
            Carry::Code
        }
        Lang::Markdown => markdown(&mut lx, carry),
        Lang::Batch => {
            batch(&mut lx);
            Carry::Code
        }
        Lang::Html => html(&mut lx, carry),
        Lang::Css => css(&mut lx, carry),
        Lang::Ini => {
            ini(&mut lx);
            Carry::Code
        }
        other => code(&mut lx, syntax(other), carry),
    };
    (lx.out, next)
}

/// Colour lines in order from `carry` (a page read from the middle of a file starts from [`Carry::Code`], so a page
/// that begins inside a block comment is coloured as code until the comment's end).
pub fn lines<'a>(lang: Lang, lines: impl IntoIterator<Item = &'a str>, carry: Carry) -> Vec<Spans> {
    let mut carry = carry;
    lines
        .into_iter()
        .map(|text| {
            let (spans, next) = line(lang, text, carry);
            carry = next;
            spans
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------- the lexer

struct Lexer<'a> {
    b: &'a [u8],
    i: usize,
    out: Spans,
}

fn is_ident_start(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'_' || c >= 0x80
}

fn is_ident(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || c >= 0x80
}

impl Lexer<'_> {
    fn len(&self) -> usize {
        self.b.len()
    }

    fn at(&self, i: usize) -> u8 {
        self.b.get(i).copied().unwrap_or(0)
    }

    fn starts(&self, i: usize, s: &str) -> bool {
        self.b.get(i..i + s.len()) == Some(s.as_bytes())
    }

    fn starts_ci(&self, i: usize, s: &str) -> bool {
        self.b.get(i..i + s.len()).is_some_and(|w| w.eq_ignore_ascii_case(s.as_bytes()))
    }

    /// Record `start..end` as `kind`, joined to the piece before when it is the same kind and touches it.
    fn push(&mut self, start: usize, end: usize, kind: Kind) {
        let end = end.min(self.len());
        if start >= end {
            return;
        }
        if let Some((last, k)) = self.out.last_mut()
            && *k == kind
            && last.end == start
        {
            last.end = end;
            return;
        }
        debug_assert!(self.out.last().is_none_or(|(r, _)| r.end <= start));
        self.out.push((start..end, kind));
    }

    fn ident_end(&self, mut i: usize) -> usize {
        while i < self.len() && is_ident(self.b[i]) {
            i += 1;
        }
        i
    }

    fn word(&self, from: usize, to: usize) -> &str {
        std::str::from_utf8(&self.b[from..to]).unwrap_or("")
    }

    fn skip_spaces(&self, mut i: usize) -> usize {
        while matches!(self.at(i), b' ' | b'\t') {
            i += 1;
        }
        i
    }

    /// Only spaces before `i` on this line.
    fn at_line_start(&self, i: usize) -> bool {
        self.b[..i].iter().all(|&c| c == b' ' || c == b'\t')
    }

    /// A string from `start` whose body begins at `body`; returns what it leaves open.
    fn string(&mut self, start: usize, body: usize, quote: u8, hashes: u8, escapes: bool, runs_on: bool) -> Carry {
        let mut i = body;
        while i < self.len() {
            let c = self.b[i];
            if escapes && c == b'\\' {
                i += 2;
                continue;
            }
            if c == quote && (0..hashes as usize).all(|h| self.at(i + 1 + h) == b'#') {
                let end = i + 1 + hashes as usize;
                self.push(start, end, Kind::String);
                self.i = end;
                return Carry::Code;
            }
            i += 1;
        }
        self.push(start, self.len(), Kind::String);
        self.i = self.len();
        if runs_on { Carry::Str { quote, hashes, raw: !escapes } } else { Carry::Code }
    }

    /// `"""…"""` or `'''…'''` from `start`, its body from `body`.
    fn triple(&mut self, start: usize, body: usize, quote: u8, raw: bool) -> Carry {
        let close = [quote; 3];
        let mut i = body;
        while i < self.len() {
            if !raw && self.b[i] == b'\\' {
                i += 2;
                continue;
            }
            if self.b.get(i..i + 3) == Some(&close[..]) {
                self.push(start, i + 3, Kind::String);
                self.i = i + 3;
                return Carry::Code;
            }
            i += 1;
        }
        self.push(start, self.len(), Kind::String);
        self.i = self.len();
        Carry::Triple { quote, raw }
    }

    /// A block comment from `start`, its body from `body`, `depth` deep.
    fn block(&mut self, start: usize, body: usize, open: &str, close: &str, nests: bool, mut depth: u8) -> Carry {
        let mut i = body;
        while i < self.len() {
            if self.starts(i, close) {
                i += close.len();
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    self.push(start, i, Kind::Comment);
                    self.i = i;
                    return Carry::Code;
                }
                continue;
            }
            if nests && self.starts(i, open) {
                i += open.len();
                depth = depth.saturating_add(1);
                continue;
            }
            i += 1;
        }
        self.push(start, self.len(), Kind::Comment);
        self.i = self.len();
        Carry::Block { depth: depth.max(1) }
    }

    /// A number at `self.i`: hexadecimal, binary, octal or decimal with a fraction and exponent, and any suffix.
    fn number(&mut self) {
        let start = self.i;
        let mut i = start;
        let radix = if self.at(i) == b'0' { self.at(i + 1).to_ascii_lowercase() } else { 0 };
        if matches!(radix, b'x' | b'b' | b'o') {
            i += 2;
            while is_ident(self.at(i)) {
                i += 1;
            }
        } else {
            while self.at(i).is_ascii_digit() || self.at(i) == b'_' {
                i += 1;
            }
            if self.at(i) == b'.' && self.at(i + 1).is_ascii_digit() {
                i += 1;
                while self.at(i).is_ascii_digit() || self.at(i) == b'_' {
                    i += 1;
                }
            }
            if matches!(self.at(i), b'e' | b'E')
                && (self.at(i + 1).is_ascii_digit()
                    || (matches!(self.at(i + 1), b'+' | b'-') && self.at(i + 2).is_ascii_digit()))
            {
                i += 2;
                while self.at(i).is_ascii_digit() || self.at(i) == b'_' {
                    i += 1;
                }
            }
            // A suffix (`u8`, `f32`, `L`, `n`), never a `.` that starts a range or a method call.
            while is_ident(self.at(i)) {
                i += 1;
            }
        }
        self.push(start, i, Kind::Number);
        self.i = i.max(start + 1);
    }
}

// ---------------------------------------------------------------------------------- the C-like family

#[derive(Clone, Copy, PartialEq, Eq)]
enum Backtick {
    No,
    /// JavaScript's template: escapes, runs on.
    Template,
    /// Go's raw string: no escapes, runs on.
    Raw,
}

struct Syntax {
    keywords: &'static [&'static str],
    types: &'static [&'static str],
    constants: &'static [&'static str],
    /// Keywords whose next name is a function being declared.
    declares_fn: &'static [&'static str],
    /// Keywords whose next name is a type being declared.
    declares_type: &'static [&'static str],
    line_comments: &'static [&'static str],
    block: Option<(&'static str, &'static str, bool)>,
    /// `#` starts a comment only where a word could start (shells).
    hash_comment_at_word: bool,
    rust_attributes: bool,
    preprocessor: bool,
    decorators: bool,
    bang_macros: bool,
    lifetimes: bool,
    /// `'…'` is a string, not a character.
    single_quote_strings: bool,
    /// Escapes inside `'…'` (not in the shells' and SQL's).
    single_quote_escapes: bool,
    /// A `"…"` string runs on to the next line.
    double_runs_on: bool,
    single_runs_on: bool,
    backtick: Backtick,
    triple_quotes: bool,
    /// Letters that may start a string (`r"…"`, `b'…'`, `f"…"`, `@"…"`).
    string_prefixes: &'static [u8],
    ignore_case: bool,
    dollar_variables: bool,
    /// PowerShell: `Verb-Noun` names, `-Parameter`s and `-eq` operators.
    powershell: bool,
}

const fn base() -> Syntax {
    Syntax {
        keywords: &[],
        types: &[],
        constants: &[],
        declares_fn: &[],
        declares_type: &[],
        line_comments: &["//"],
        block: Some(("/*", "*/", false)),
        hash_comment_at_word: false,
        rust_attributes: false,
        preprocessor: false,
        decorators: false,
        bang_macros: false,
        lifetimes: false,
        single_quote_strings: false,
        single_quote_escapes: true,
        double_runs_on: false,
        single_runs_on: false,
        backtick: Backtick::No,
        triple_quotes: false,
        string_prefixes: &[],
        ignore_case: false,
        dollar_variables: false,
        powershell: false,
    }
}

const RUST: Syntax = Syntax {
    keywords: &[
        "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern", "fn", "for",
        "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref", "return", "self", "Self",
        "static", "struct", "super", "trait", "type", "unsafe", "use", "where", "while", "yield", "union",
        "macro_rules", "gen", "try",
    ],
    types: &[
        "i8", "i16", "i32", "i64", "i128", "isize", "u8", "u16", "u32", "u64", "u128", "usize", "f32", "f64", "bool",
        "char", "str",
    ],
    constants: &["true", "false"],
    declares_fn: &["fn"],
    declares_type: &["struct", "enum", "trait", "type", "union"],
    block: Some(("/*", "*/", true)),
    rust_attributes: true,
    bang_macros: true,
    lifetimes: true,
    double_runs_on: true,
    string_prefixes: b"brc",
    ..base()
};

const PYTHON: Syntax = Syntax {
    keywords: &[
        "and", "as", "assert", "async", "await", "break", "class", "continue", "def", "del", "elif", "else", "except",
        "finally", "for", "from", "global", "if", "import", "in", "is", "lambda", "nonlocal", "not", "or", "pass",
        "raise", "return", "try", "while", "with", "yield", "match", "case", "self", "cls",
    ],
    types: &[
        "int", "str", "float", "bool", "bytes", "bytearray", "list", "dict", "set", "frozenset", "tuple", "object",
        "type", "complex",
    ],
    constants: &["True", "False", "None", "NotImplemented", "Ellipsis", "__name__", "__file__"],
    declares_fn: &["def"],
    declares_type: &["class"],
    line_comments: &["#"],
    block: None,
    decorators: true,
    single_quote_strings: true,
    triple_quotes: true,
    string_prefixes: b"rbuf",
    ..base()
};

const JS: Syntax = Syntax {
    keywords: &[
        "break", "case", "catch", "class", "const", "continue", "debugger", "default", "delete", "do", "else",
        "export", "extends", "finally", "for", "function", "if", "import", "in", "instanceof", "new", "return",
        "super", "switch", "this", "throw", "try", "typeof", "var", "void", "while", "with", "yield", "let", "static",
        "async", "await", "of", "from", "as", "interface", "type", "enum", "implements", "namespace", "declare",
        "abstract", "readonly", "private", "protected", "public", "keyof", "infer", "satisfies", "is", "get", "set",
    ],
    types: &["string", "number", "boolean", "object", "symbol", "bigint", "any", "unknown", "never"],
    constants: &["true", "false", "null", "undefined", "NaN", "Infinity"],
    declares_fn: &["function"],
    declares_type: &["class", "interface", "type", "enum", "namespace"],
    decorators: true,
    single_quote_strings: true,
    backtick: Backtick::Template,
    ..base()
};

const C: Syntax = Syntax {
    keywords: &[
        "auto", "break", "case", "const", "continue", "default", "do", "else", "enum", "extern", "for", "goto", "if",
        "inline", "register", "restrict", "return", "sizeof", "static", "struct", "switch", "typedef", "union",
        "volatile", "while", "alignas", "alignof", "asm", "catch", "class", "constexpr", "consteval", "constinit",
        "const_cast", "co_await", "co_return", "co_yield", "decltype", "delete", "dynamic_cast", "explicit", "export",
        "friend", "mutable", "namespace", "new", "noexcept", "operator", "private", "protected", "public",
        "reinterpret_cast", "requires", "static_assert", "static_cast", "template", "this", "thread_local", "throw",
        "try", "typeid", "typename", "using", "virtual", "override", "final", "concept",
    ],
    types: &[
        "void", "char", "short", "int", "long", "float", "double", "signed", "unsigned", "bool", "wchar_t", "char8_t",
        "char16_t", "char32_t", "size_t", "ssize_t", "ptrdiff_t", "intptr_t", "uintptr_t", "int8_t", "int16_t",
        "int32_t", "int64_t", "uint8_t", "uint16_t", "uint32_t", "uint64_t", "std", "string", "vector",
    ],
    constants: &["true", "false", "NULL", "nullptr"],
    declares_type: &["struct", "class", "union", "enum", "namespace", "concept"],
    preprocessor: true,
    ..base()
};

const CSHARP: Syntax = Syntax {
    keywords: &[
        "abstract", "as", "base", "break", "case", "catch", "checked", "class", "const", "continue", "default",
        "delegate", "do", "else", "enum", "event", "explicit", "extern", "finally", "fixed", "for", "foreach", "goto",
        "if", "implicit", "in", "interface", "internal", "is", "lock", "namespace", "new", "operator", "out",
        "override", "params", "private", "protected", "public", "readonly", "ref", "return", "sealed", "sizeof",
        "stackalloc", "static", "struct", "switch", "this", "throw", "try", "typeof", "unchecked", "unsafe", "using",
        "virtual", "volatile", "while", "var", "async", "await", "get", "set", "value", "yield", "record", "init",
        "with", "when", "where", "nameof", "partial", "required",
    ],
    types: &[
        "bool", "byte", "sbyte", "char", "decimal", "double", "float", "int", "uint", "long", "ulong", "short",
        "ushort", "object", "string", "void", "dynamic", "nint", "nuint",
    ],
    constants: &["true", "false", "null"],
    declares_type: &["class", "struct", "interface", "enum", "record", "namespace"],
    preprocessor: true,
    triple_quotes: true,
    string_prefixes: b"@$",
    ..base()
};

const GO: Syntax = Syntax {
    keywords: &[
        "break", "case", "chan", "const", "continue", "default", "defer", "else", "fallthrough", "for", "func", "go",
        "goto", "if", "import", "interface", "map", "package", "range", "return", "select", "struct", "switch",
        "type", "var",
    ],
    types: &[
        "bool", "byte", "complex64", "complex128", "error", "float32", "float64", "int", "int8", "int16", "int32",
        "int64", "rune", "string", "uint", "uint8", "uint16", "uint32", "uint64", "uintptr", "any",
    ],
    constants: &["true", "false", "nil", "iota"],
    declares_fn: &["func"],
    declares_type: &["type"],
    backtick: Backtick::Raw,
    ..base()
};

const JAVA: Syntax = Syntax {
    keywords: &[
        "abstract", "assert", "break", "case", "catch", "class", "const", "continue", "default", "do", "else", "enum",
        "extends", "final", "finally", "for", "goto", "if", "implements", "import", "instanceof", "interface",
        "native", "new", "package", "private", "protected", "public", "return", "static", "strictfp", "super",
        "switch", "synchronized", "this", "throw", "throws", "transient", "try", "volatile", "while", "var", "record",
        "sealed", "permits", "yield", "fun", "val", "when", "object", "companion", "data", "override", "open",
        "internal", "lateinit", "inline", "suspend", "is", "in", "as",
    ],
    types: &["boolean", "byte", "char", "short", "int", "long", "float", "double", "void"],
    constants: &["true", "false", "null"],
    declares_fn: &["fun"],
    declares_type: &["class", "interface", "enum", "record", "object"],
    decorators: true,
    triple_quotes: true,
    ..base()
};

const SHADER: Syntax = Syntax {
    keywords: &[
        "fn", "let", "var", "const", "struct", "return", "if", "else", "for", "loop", "break", "continue", "switch",
        "case", "default", "discard", "override", "alias", "enable", "while", "do", "in", "out", "inout", "uniform",
        "layout", "attribute", "varying", "precision", "highp", "mediump", "lowp", "cbuffer", "register", "static",
        "continuing", "requires", "diagnostic",
    ],
    types: &[
        "f16", "f32", "i32", "u32", "bool", "void", "float", "int", "uint", "half", "double", "vec2", "vec3", "vec4",
        "ivec2", "ivec3", "ivec4", "uvec2", "uvec3", "uvec4", "bvec2", "bvec3", "bvec4", "mat2", "mat3", "mat4",
        "vec2f", "vec3f", "vec4f", "vec2i", "vec3i", "vec4i", "vec2u", "vec3u", "vec4u", "mat2x2", "mat3x3",
        "mat4x4", "mat4x4f", "mat3x3f", "array", "ptr", "atomic", "sampler", "sampler2D", "texture_2d",
        "texture_storage_2d", "float2", "float3", "float4", "float4x4",
    ],
    constants: &["true", "false"],
    declares_fn: &["fn"],
    declares_type: &["struct"],
    preprocessor: true,
    decorators: true,
    ..base()
};

const SHELL: Syntax = Syntax {
    keywords: &[
        "if", "then", "else", "elif", "fi", "for", "while", "until", "do", "done", "case", "esac", "in", "function",
        "select", "time", "return", "exit", "export", "local", "readonly", "declare", "unset", "source", "alias",
        "break", "continue", "shift", "trap", "set", "eval", "exec",
    ],
    constants: &["true", "false"],
    declares_fn: &["function"],
    line_comments: &["#"],
    block: None,
    hash_comment_at_word: true,
    single_quote_strings: true,
    single_quote_escapes: false,
    double_runs_on: true,
    single_runs_on: true,
    dollar_variables: true,
    ..base()
};

const POWERSHELL: Syntax = Syntax {
    keywords: &[
        "begin", "break", "catch", "class", "continue", "data", "define", "do", "dynamicparam", "else", "elseif",
        "end", "exit", "filter", "finally", "for", "foreach", "from", "function", "if", "in", "param", "process",
        "return", "switch", "throw", "trap", "try", "until", "using", "var", "while", "workflow", "enum",
    ],
    constants: &["$true", "$false", "$null"],
    declares_fn: &["function", "filter"],
    declares_type: &["class", "enum"],
    line_comments: &["#"],
    block: Some(("<#", "#>", false)),
    single_quote_strings: true,
    single_quote_escapes: false,
    double_runs_on: true,
    single_runs_on: true,
    ignore_case: true,
    dollar_variables: true,
    powershell: true,
    ..base()
};

const SQL: Syntax = Syntax {
    keywords: &[
        "select", "from", "where", "insert", "into", "values", "update", "set", "delete", "create", "table", "index",
        "view", "drop", "alter", "add", "column", "primary", "key", "foreign", "references", "not", "null", "unique",
        "default", "join", "left", "right", "inner", "outer", "cross", "on", "group", "by", "order", "having", "limit",
        "offset", "as", "and", "or", "in", "is", "like", "glob", "between", "case", "when", "then", "else", "end",
        "union", "all", "distinct", "exists", "begin", "commit", "rollback", "transaction", "pragma", "with",
        "recursive", "returning", "if", "trigger", "conflict", "replace", "ignore", "asc", "desc", "check",
        "constraint", "cascade", "virtual", "using", "without", "rowid", "strict",
    ],
    types: &[
        "integer", "int", "text", "real", "blob", "numeric", "varchar", "char", "boolean", "date", "datetime",
        "timestamp", "float", "double", "bigint", "smallint", "decimal",
    ],
    constants: &["true", "false"],
    line_comments: &["--"],
    single_quote_strings: true,
    single_quote_escapes: false,
    ignore_case: true,
    ..base()
};

fn syntax(lang: Lang) -> &'static Syntax {
    match lang {
        Lang::Rust => &RUST,
        Lang::Python => &PYTHON,
        Lang::Js => &JS,
        Lang::CSharp => &CSHARP,
        Lang::Go => &GO,
        Lang::Java => &JAVA,
        Lang::Shader => &SHADER,
        Lang::Shell => &SHELL,
        Lang::PowerShell => &POWERSHELL,
        Lang::Sql => &SQL,
        _ => &C,
    }
}

fn listed(list: &[&str], word: &str, ignore_case: bool) -> bool {
    if ignore_case { list.iter().any(|w| w.eq_ignore_ascii_case(word)) } else { list.contains(&word) }
}

const PS_OPERATORS: &[&str] = &[
    "eq", "ne", "gt", "lt", "ge", "le", "like", "notlike", "match", "notmatch", "contains", "notcontains", "in",
    "notin", "replace", "and", "or", "not", "xor", "band", "bor", "bxor", "bnot", "shl", "shr", "is", "isnot", "as",
    "f", "split", "join", "ceq", "cne", "ieq", "ine", "clike", "cmatch", "imatch",
];

/// Finish what the line before left open; `None` when the line ended inside it again.
fn resume(lx: &mut Lexer, syn: &Syntax, carry: Carry) -> Option<Carry> {
    match carry {
        Carry::Block { depth } => {
            let (open, close, nests) = syn.block.unwrap_or(("/*", "*/", false));
            match lx.block(0, 0, open, close, nests, depth) {
                Carry::Code => None,
                still => Some(still),
            }
        }
        Carry::Str { quote, hashes, raw } => match lx.string(0, 0, quote, hashes, !raw, true) {
            Carry::Code => None,
            still => Some(still),
        },
        Carry::Triple { quote, raw } => match lx.triple(0, 0, quote, raw) {
            Carry::Code => None,
            still => Some(still),
        },
        Carry::Here { quote } => {
            if lx.at(0) == quote && lx.at(1) == b'@' {
                lx.push(0, 2, Kind::String);
                lx.i = 2;
                None
            } else {
                lx.push(0, lx.len(), Kind::String);
                lx.i = lx.len();
                Some(carry)
            }
        }
        Carry::Code | Carry::Fence | Carry::Tag => None,
    }
}

fn code(lx: &mut Lexer, syn: &Syntax, carry: Carry) -> Carry {
    if let Some(still) = resume(lx, syn, carry) {
        return still;
    }
    // What the last keyword said the next name is.
    let mut declaring: Option<Kind> = None;
    while lx.i < lx.len() {
        let i = lx.i;
        let c = lx.b[i];
        if c == b' ' || c == b'\t' {
            lx.i += 1;
            continue;
        }
        // Comments.
        if let Some(lc) = syn.line_comments.iter().find(|lc| lx.starts(i, lc)) {
            let at_word = !syn.hash_comment_at_word || i == 0 || matches!(lx.at(i - 1), b' ' | b'\t' | b';');
            if at_word && !(*lc == "#" && syn.powershell && lx.at(i + 1) == b'>') {
                lx.push(i, lx.len(), Kind::Comment);
                return Carry::Code;
            }
        }
        if let Some((open, close, nests)) = syn.block
            && lx.starts(i, open)
        {
            match lx.block(i, i + open.len(), open, close, nests, 1) {
                Carry::Code => continue,
                still => return still,
            }
        }
        // Attributes, preprocessor lines, decorators.
        if syn.rust_attributes && c == b'#' && (lx.at(i + 1) == b'[' || (lx.at(i + 1) == b'!' && lx.at(i + 2) == b'[')) {
            let mut j = i + 1;
            let mut depth = 0i32;
            while j < lx.len() {
                match lx.b[j] {
                    b'[' => depth += 1,
                    b']' => {
                        depth -= 1;
                        if depth == 0 {
                            j += 1;
                            break;
                        }
                    }
                    _ => {}
                }
                j += 1;
            }
            lx.push(i, j, Kind::Attribute);
            lx.i = j;
            continue;
        }
        if syn.preprocessor && c == b'#' && lx.at_line_start(i) {
            lx.push(i, lx.len(), Kind::Macro);
            return Carry::Code;
        }
        if syn.decorators && c == b'@' && is_ident_start(lx.at(i + 1)) {
            let end = lx.ident_end(i + 1);
            lx.push(i, end, Kind::Attribute);
            lx.i = end;
            continue;
        }
        // PowerShell's here-strings: `@"` or `@'` closing the line.
        if syn.powershell && c == b'@' && matches!(lx.at(i + 1), b'"' | b'\'') && lx.i + 2 >= lx.len() {
            lx.push(i, lx.len(), Kind::String);
            return Carry::Here { quote: lx.at(i + 1) };
        }
        // Strings, with their prefixes.
        if is_ident_start(c) || c == b'@' || c == b'$' {
            let mut j = i;
            while j < lx.len() && j - i < 3 && syn.string_prefixes.contains(&lx.b[j].to_ascii_lowercase()) {
                j += 1;
            }
            if j > i && (j - i < 3 || syn.string_prefixes.contains(&b'$')) {
                // Rust's raw strings: r#"…"#.
                let mut hashes = 0u8;
                let mut k = j;
                if syn.lifetimes {
                    while lx.at(k) == b'#' && hashes < 255 {
                        hashes += 1;
                        k += 1;
                    }
                }
                let q = lx.at(k);
                let prefix = lx.word(i, j).to_ascii_lowercase();
                let raw = prefix.contains('r') || (prefix.contains('@') && syn.string_prefixes.contains(&b'@'));
                // In a language whose '…' is a character, only `b'x'` (a byte) takes a prefix.
                let byte_char = q == b'\'' && !syn.single_quote_strings && prefix == "b" && hashes == 0;
                if q == b'"' || (q == b'\'' && syn.single_quote_strings) || byte_char {
                    if syn.triple_quotes && lx.at(k + 1) == q && lx.at(k + 2) == q {
                        match lx.triple(i, k + 3, q, raw) {
                            Carry::Code => continue,
                            still => return still,
                        }
                    }
                    if q == b'\'' && !syn.single_quote_strings {
                        // A byte character, b'x'.
                        if let Some(end) = char_literal(lx, k) {
                            lx.push(i, end, Kind::String);
                            lx.i = end;
                            continue;
                        }
                    } else {
                        let runs_on = syn.double_runs_on || raw;
                        match lx.string(i, k + 1, q, hashes, !raw, runs_on) {
                            Carry::Code => continue,
                            still => return still,
                        }
                    }
                }
            }
        }
        if c == b'"' {
            if syn.triple_quotes && lx.starts(i, "\"\"\"") {
                match lx.triple(i, i + 3, b'"', false) {
                    Carry::Code => continue,
                    still => return still,
                }
            }
            match lx.string(i, i + 1, b'"', 0, !syn.powershell, syn.double_runs_on) {
                Carry::Code => continue,
                still => return still,
            }
        }
        if c == b'\'' {
            if syn.single_quote_strings {
                if syn.triple_quotes && lx.starts(i, "'''") {
                    match lx.triple(i, i + 3, b'\'', false) {
                        Carry::Code => continue,
                        still => return still,
                    }
                }
                match lx.string(i, i + 1, b'\'', 0, syn.single_quote_escapes, syn.single_runs_on) {
                    Carry::Code => continue,
                    still => return still,
                }
            }
            if syn.lifetimes && is_ident_start(lx.at(i + 1)) {
                let end = lx.ident_end(i + 1);
                if lx.at(end) != b'\'' {
                    lx.push(i, end, Kind::Attribute);
                    lx.i = end;
                    continue;
                }
            }
            if let Some(end) = char_literal(lx, i) {
                lx.push(i, end, Kind::String);
                lx.i = end;
                continue;
            }
            lx.i += 1;
            continue;
        }
        if c == b'`' && syn.backtick != Backtick::No {
            match lx.string(i, i + 1, b'`', 0, syn.backtick == Backtick::Template, true) {
                Carry::Code => continue,
                still => return still,
            }
        }
        // Numbers.
        if c.is_ascii_digit() && (i == 0 || !is_ident(lx.at(i - 1))) {
            lx.number();
            continue;
        }
        // Variables.
        if syn.dollar_variables && c == b'$' {
            let end = if lx.at(i + 1) == b'{' {
                let mut j = i + 2;
                while j < lx.len() && lx.b[j] != b'}' {
                    j += 1;
                }
                (j + 1).min(lx.len())
            } else if is_ident_start(lx.at(i + 1)) || lx.at(i + 1).is_ascii_digit() {
                let mut end = lx.ident_end(i + 1);
                // $env:NAME, $script:x.
                if syn.powershell && lx.at(end) == b':' && is_ident_start(lx.at(end + 1)) {
                    end = lx.ident_end(end + 1);
                }
                end
            } else if matches!(lx.at(i + 1), b'?' | b'#' | b'@' | b'*' | b'!' | b'$' | b'_') {
                i + 2
            } else {
                i + 1
            };
            let word = lx.word(i, end);
            let kind = if listed(syn.constants, word, true) { Kind::Constant } else { Kind::Variable };
            lx.push(i, end, kind);
            lx.i = end.max(i + 1);
            continue;
        }
        // PowerShell's -Parameter and -eq.
        if syn.powershell && c == b'-' && is_ident_start(lx.at(i + 1)) && (i == 0 || matches!(lx.at(i - 1), b' ' | b'\t' | b'(')) {
            let end = lx.ident_end(i + 1);
            let word = lx.word(i + 1, end);
            lx.push(i, end, if listed(PS_OPERATORS, word, true) { Kind::Keyword } else { Kind::Key });
            lx.i = end;
            continue;
        }
        // Names.
        if is_ident_start(c) {
            let mut end = lx.ident_end(i);
            if syn.powershell {
                // Verb-Noun.
                while lx.at(end) == b'-' && lx.at(end + 1).is_ascii_alphabetic() {
                    end = lx.ident_end(end + 1);
                }
            }
            let word = lx.word(i, end);
            let after = lx.skip_spaces(end);
            let kind = if let Some(kind) = declaring.take() {
                Some(kind)
            } else if listed(syn.keywords, word, syn.ignore_case) {
                if listed(syn.declares_fn, word, syn.ignore_case) {
                    declaring = Some(Kind::Function);
                } else if listed(syn.declares_type, word, syn.ignore_case) {
                    declaring = Some(Kind::Type);
                }
                Some(Kind::Keyword)
            } else if listed(syn.constants, word, syn.ignore_case) {
                Some(Kind::Constant)
            } else if listed(syn.types, word, syn.ignore_case) {
                Some(Kind::Type)
            } else if syn.bang_macros && lx.at(end) == b'!' && lx.at(end + 1) != b'=' {
                end += 1;
                Some(Kind::Macro)
            } else if syn.powershell && word.contains('-') {
                Some(Kind::Function)
            } else if lx.at(after) == b'(' {
                Some(Kind::Function)
            } else if word.len() > 1
                && word.bytes().any(|b| b.is_ascii_uppercase())
                && !word.bytes().any(|b| b.is_ascii_lowercase())
                && word.bytes().all(|b| b.is_ascii())
            {
                Some(Kind::Constant)
            } else if word.as_bytes()[0].is_ascii_uppercase() && !syn.ignore_case {
                Some(Kind::Type)
            } else {
                None
            };
            if let Some(kind) = kind {
                lx.push(i, end, kind);
            }
            lx.i = end;
            continue;
        }
        lx.i += 1;
    }
    Carry::Code
}

/// A character literal at `i` (`'a'`, `'\n'`, `'\u{1F600}'`, `'é'`): where it ends, if it is one.
fn char_literal(lx: &Lexer, i: usize) -> Option<usize> {
    if lx.at(i) != b'\'' {
        return None;
    }
    if lx.at(i + 1) == b'\\' {
        let mut j = i + 2;
        while j < lx.len() && j < i + 14 {
            if lx.b[j] == b'\'' {
                return Some(j + 1);
            }
            j += 1;
        }
        return None;
    }
    let text = std::str::from_utf8(&lx.b[i + 1..]).ok().or_else(|| {
        // The line was cut at a byte limit: decode what is whole.
        let valid = std::str::from_utf8(&lx.b[i + 1..]).err()?.valid_up_to();
        std::str::from_utf8(&lx.b[i + 1..i + 1 + valid]).ok()
    })?;
    let ch = text.chars().next()?;
    let end = i + 1 + ch.len_utf8();
    (ch != '\'' && lx.at(end) == b'\'').then_some(end + 1)
}

// ------------------------------------------------------------------------------- data and documents

fn json(lx: &mut Lexer, carry: Carry) {
    if let Carry::Block { depth } = carry
        && lx.block(0, 0, "/*", "*/", false, depth) != Carry::Code
    {
        return;
    }
    while lx.i < lx.len() {
        let i = lx.i;
        match lx.b[i] {
            b'"' => {
                let mut j = i + 1;
                while j < lx.len() && lx.b[j] != b'"' {
                    j += if lx.b[j] == b'\\' { 2 } else { 1 };
                }
                let end = (j + 1).min(lx.len());
                let kind = if lx.at(lx.skip_spaces(end)) == b':' { Kind::Key } else { Kind::String };
                lx.push(i, end, kind);
                lx.i = end;
            }
            b'/' if lx.at(i + 1) == b'/' => {
                lx.push(i, lx.len(), Kind::Comment);
                return;
            }
            b'/' if lx.at(i + 1) == b'*' => {
                if lx.block(i, i + 2, "/*", "*/", false, 1) != Carry::Code {
                    return;
                }
            }
            b'-' | b'0'..=b'9' if lx.at(i + 1).is_ascii_digit() || lx.b[i].is_ascii_digit() => {
                let start = i;
                lx.i = if lx.b[i] == b'-' { i + 1 } else { i };
                lx.number();
                let end = lx.i;
                lx.out.pop();
                lx.push(start, end, Kind::Number);
            }
            c if c.is_ascii_alphabetic() => {
                let end = lx.ident_end(i);
                if matches!(lx.word(i, end), "true" | "false" | "null") {
                    lx.push(i, end, Kind::Constant);
                }
                lx.i = end;
            }
            _ => lx.i += 1,
        }
    }
}

fn toml(lx: &mut Lexer, carry: Carry) -> Carry {
    if let Carry::Triple { quote, raw } = carry
        && lx.triple(0, 0, quote, raw) != Carry::Code
    {
        return carry;
    }
    let start = lx.skip_spaces(lx.i);
    if lx.i == 0 && lx.at(start) == b'[' {
        let mut j = start;
        while j < lx.len() && lx.b[j] != b'#' {
            j += 1;
        }
        let mut end = j;
        while end > start && matches!(lx.at(end - 1), b' ' | b'\t') {
            end -= 1;
        }
        lx.push(start, end, Kind::Heading);
        lx.i = end;
    }
    let mut key_allowed = lx.i == 0;
    while lx.i < lx.len() {
        let i = lx.i;
        let c = lx.b[i];
        match c {
            b' ' | b'\t' => lx.i += 1,
            b'#' => {
                lx.push(i, lx.len(), Kind::Comment);
                return Carry::Code;
            }
            b'{' | b',' => {
                key_allowed = true;
                lx.i += 1;
            }
            b'"' | b'\'' => {
                let raw = c == b'\'';
                if lx.at(i + 1) == c && lx.at(i + 2) == c {
                    match lx.triple(i, i + 3, c, raw) {
                        Carry::Code => {}
                        still => return still,
                    }
                } else {
                    let before = lx.out.len();
                    lx.string(i, i + 1, c, 0, !raw, false);
                    if key_allowed && lx.at(lx.skip_spaces(lx.i)) == b'=' && lx.out.len() > before {
                        let (range, _) = lx.out.pop().expect("just pushed");
                        lx.push(range.start, range.end, Kind::Key);
                    }
                }
                key_allowed = false;
            }
            b'=' => {
                key_allowed = false;
                lx.i += 1;
            }
            _ if key_allowed && (is_ident(c) || c == b'-' || c == b'.') => {
                let mut end = i;
                while end < lx.len() && (is_ident(lx.b[end]) || matches!(lx.b[end], b'-' | b'.')) {
                    end += 1;
                }
                let kind = if lx.at(lx.skip_spaces(end)) == b'=' { Kind::Key } else { Kind::String };
                if kind == Kind::Key {
                    lx.push(i, end, kind);
                }
                lx.i = end;
            }
            b'+' | b'-' | b'0'..=b'9' if c.is_ascii_digit() || lx.at(i + 1).is_ascii_digit() => {
                let mut end = i + 1;
                while end < lx.len() && (is_ident(lx.b[end]) || matches!(lx.b[end], b'-' | b':' | b'.' | b'+')) {
                    end += 1;
                }
                lx.push(i, end, Kind::Number);
                lx.i = end;
            }
            _ if c.is_ascii_alphabetic() => {
                let end = lx.ident_end(i);
                if matches!(lx.word(i, end), "true" | "false" | "inf" | "nan") {
                    lx.push(i, end, Kind::Constant);
                }
                lx.i = end;
            }
            _ => lx.i += 1,
        }
    }
    Carry::Code
}

fn yaml(lx: &mut Lexer) {
    let first = lx.skip_spaces(0);
    if first == 0 && (lx.starts(0, "---") || lx.starts(0, "...")) {
        lx.push(0, lx.len(), Kind::Comment);
        return;
    }
    let mut i = first;
    if lx.at(i) == b'-' && matches!(lx.at(i + 1), b' ' | 0) {
        i = lx.skip_spaces(i + 1);
    }
    // A key: up to a colon that ends the line or is followed by a space.
    let mut j = i;
    let quoted = matches!(lx.at(i), b'"' | b'\'');
    if quoted {
        let q = lx.at(i);
        j = i + 1;
        while j < lx.len() && lx.b[j] != q {
            j += 1;
        }
        j = (j + 1).min(lx.len());
    } else {
        while j < lx.len() && lx.b[j] != b':' && lx.b[j] != b'#' {
            j += 1;
        }
    }
    if lx.at(j) == b':' && matches!(lx.at(j + 1), b' ' | b'\t' | 0) && j > i && lx.at(i) != b'#' {
        lx.push(i, j, Kind::Key);
        lx.i = j + 1;
    } else {
        lx.i = i;
    }
    while lx.i < lx.len() {
        let i = lx.i;
        let c = lx.b[i];
        match c {
            b'#' if i == 0 || matches!(lx.at(i - 1), b' ' | b'\t') => {
                lx.push(i, lx.len(), Kind::Comment);
                return;
            }
            b'"' => {
                lx.string(i, i + 1, b'"', 0, true, false);
            }
            b'\'' => {
                lx.string(i, i + 1, b'\'', 0, false, false);
            }
            b'&' | b'*' if is_ident_start(lx.at(i + 1)) => {
                let end = lx.ident_end(i + 1);
                lx.push(i, end, Kind::Attribute);
                lx.i = end;
            }
            b'!' if is_ident_start(lx.at(i + 1)) || lx.at(i + 1) == b'!' => {
                let mut end = i + 1;
                while end < lx.len() && !matches!(lx.b[end], b' ' | b'\t') {
                    end += 1;
                }
                lx.push(i, end, Kind::Macro);
                lx.i = end;
            }
            _ if c.is_ascii_alphanumeric() || c == b'~' || c == b'-' || c == b'.' => {
                // A plain scalar: to the next comment.
                let mut end = i;
                while end < lx.len() && !(lx.b[end] == b'#' && matches!(lx.at(end.wrapping_sub(1)), b' ' | b'\t')) {
                    end += 1;
                }
                let mut trimmed = end;
                while trimmed > i && matches!(lx.at(trimmed - 1), b' ' | b'\t') {
                    trimmed -= 1;
                }
                let word = lx.word(i, trimmed);
                let kind = if matches!(
                    word.to_ascii_lowercase().as_str(),
                    "true" | "false" | "yes" | "no" | "on" | "off" | "null" | "~"
                ) {
                    Some(Kind::Constant)
                } else if word.parse::<f64>().is_ok() || (word.starts_with("0x") && word.len() > 2) {
                    Some(Kind::Number)
                } else {
                    None
                };
                if let Some(kind) = kind {
                    lx.push(i, trimmed, kind);
                }
                lx.i = end.max(i + 1);
            }
            _ => lx.i += 1,
        }
    }
}

fn markdown(lx: &mut Lexer, carry: Carry) -> Carry {
    let start = lx.skip_spaces(0);
    let fence = lx.starts(start, "```") || lx.starts(start, "~~~");
    if carry == Carry::Fence {
        lx.push(0, lx.len(), if fence { Kind::Comment } else { Kind::String });
        return if fence { Carry::Code } else { Carry::Fence };
    }
    if fence {
        lx.push(start, lx.len(), Kind::Comment);
        return Carry::Fence;
    }
    if lx.at(start) == b'#' {
        let mut hashes = start;
        while lx.at(hashes) == b'#' {
            hashes += 1;
        }
        if hashes - start <= 6 && matches!(lx.at(hashes), b' ' | 0) {
            lx.push(start, lx.len(), Kind::Heading);
            return Carry::Code;
        }
    }
    if lx.at(start) == b'>' {
        lx.push(start, lx.len(), Kind::Comment);
        return Carry::Code;
    }
    // A list's marker.
    let mut i = start;
    if matches!(lx.at(i), b'-' | b'*' | b'+') && lx.at(i + 1) == b' ' {
        lx.push(i, i + 1, Kind::Keyword);
        i += 2;
    } else {
        let mut j = i;
        while lx.at(j).is_ascii_digit() {
            j += 1;
        }
        if j > i && matches!(lx.at(j), b'.' | b')') && lx.at(j + 1) == b' ' {
            lx.push(i, j + 1, Kind::Keyword);
            i = j + 2;
        }
    }
    // Inline code.
    lx.i = i;
    while lx.i < lx.len() {
        let i = lx.i;
        if lx.b[i] == b'`' {
            let mut ticks = i;
            while lx.at(ticks) == b'`' {
                ticks += 1;
            }
            let n = ticks - i;
            let mut j = ticks;
            let mut end = None;
            while j < lx.len() {
                if lx.b[j] == b'`' {
                    let mut k = j;
                    while lx.at(k) == b'`' {
                        k += 1;
                    }
                    if k - j == n {
                        end = Some(k);
                        break;
                    }
                    j = k;
                    continue;
                }
                j += 1;
            }
            match end {
                Some(end) => {
                    lx.push(i, end, Kind::String);
                    lx.i = end;
                }
                None => lx.i = ticks,
            }
            continue;
        }
        lx.i += 1;
    }
    Carry::Code
}

fn batch(lx: &mut Lexer) {
    let start = lx.skip_spaces(0);
    let rest = lx.word(start, lx.len()).to_ascii_lowercase();
    if rest.starts_with("::") || rest == "rem" || rest.starts_with("rem ") || rest.starts_with("rem\t") || rest.starts_with("@rem ") {
        lx.push(start, lx.len(), Kind::Comment);
        return;
    }
    if lx.at(start) == b':' {
        lx.push(start, lx.len(), Kind::Function);
        return;
    }
    const WORDS: &[&str] = &[
        "set", "if", "else", "goto", "call", "echo", "for", "in", "do", "exit", "setlocal", "endlocal", "not",
        "exist", "defined", "errorlevel", "equ", "neq", "lss", "leq", "gtr", "geq", "shift", "pushd", "popd", "start",
        "cd", "off", "on", "title", "pause", "cls", "copy", "del", "move", "mkdir", "rmdir", "type", "choice",
    ];
    lx.i = start;
    while lx.i < lx.len() {
        let i = lx.i;
        let c = lx.b[i];
        match c {
            b'@' if i == start => {
                lx.push(i, i + 1, Kind::Keyword);
                lx.i += 1;
            }
            b'%' => {
                let end = if lx.at(i + 1) == b'%' {
                    // %%i in a for loop.
                    (i + 3).min(lx.len())
                } else if lx.at(i + 1) == b'~' || lx.at(i + 1).is_ascii_digit() {
                    let mut j = i + 1;
                    while j < lx.len() && !lx.b[j].is_ascii_digit() {
                        j += 1;
                    }
                    (j + 1).min(lx.len())
                } else {
                    let mut j = i + 1;
                    while j < lx.len() && lx.b[j] != b'%' && lx.b[j] != b' ' {
                        j += 1;
                    }
                    if lx.at(j) == b'%' { j + 1 } else { i + 1 }
                };
                if end > i + 1 {
                    lx.push(i, end, Kind::Variable);
                }
                lx.i = end.max(i + 1);
            }
            b'!' if is_ident_start(lx.at(i + 1)) => {
                let end = lx.ident_end(i + 1);
                if lx.at(end) == b'!' {
                    lx.push(i, end + 1, Kind::Variable);
                    lx.i = end + 1;
                } else {
                    lx.i += 1;
                }
            }
            b'"' => {
                lx.string(i, i + 1, b'"', 0, false, false);
            }
            b'0'..=b'9' if i == 0 || !is_ident(lx.at(i - 1)) => lx.number(),
            _ if is_ident_start(c) => {
                let end = lx.ident_end(i);
                if listed(WORDS, lx.word(i, end), true) {
                    lx.push(i, end, Kind::Keyword);
                }
                lx.i = end;
            }
            _ => lx.i += 1,
        }
    }
}

fn html(lx: &mut Lexer, carry: Carry) -> Carry {
    match carry {
        Carry::Block { depth } => {
            if lx.block(0, 0, "<!--", "-->", false, depth) != Carry::Code {
                return Carry::Block { depth: 1 };
            }
        }
        Carry::Tag => {
            if let Some(still) = tag_rest(lx) {
                return still;
            }
        }
        _ => {}
    }
    while lx.i < lx.len() {
        let i = lx.i;
        if lx.starts(i, "<!--") {
            if lx.block(i, i + 4, "<!--", "-->", false, 1) != Carry::Code {
                return Carry::Block { depth: 1 };
            }
            continue;
        }
        if lx.b[i] == b'<' && (is_ident_start(lx.at(i + 1)) || matches!(lx.at(i + 1), b'/' | b'!' | b'?')) {
            let mut end = i + 1;
            if matches!(lx.at(end), b'/' | b'!' | b'?') {
                end += 1;
            }
            while end < lx.len() && (is_ident(lx.b[end]) || matches!(lx.b[end], b'-' | b':' | b'.')) {
                end += 1;
            }
            lx.push(i, end, Kind::Tag);
            lx.i = end;
            if let Some(still) = tag_rest(lx) {
                return still;
            }
            continue;
        }
        if lx.b[i] == b'&' {
            let mut j = i + 1;
            while j < lx.len() && j < i + 12 && (lx.b[j].is_ascii_alphanumeric() || lx.b[j] == b'#') {
                j += 1;
            }
            if lx.at(j) == b';' && j > i + 1 {
                lx.push(i, j + 1, Kind::Constant);
                lx.i = j + 1;
                continue;
            }
        }
        lx.i += 1;
    }
    Carry::Code
}

/// The rest of a tag: attributes and strings up to its `>`; `Some(Tag)` when the line ends first.
fn tag_rest(lx: &mut Lexer) -> Option<Carry> {
    while lx.i < lx.len() {
        let i = lx.i;
        match lx.b[i] {
            b'>' => {
                lx.push(i, i + 1, Kind::Tag);
                lx.i = i + 1;
                return None;
            }
            b'/' | b'?' if lx.at(i + 1) == b'>' => {
                lx.push(i, i + 2, Kind::Tag);
                lx.i = i + 2;
                return None;
            }
            b'"' | b'\'' => {
                let q = lx.b[i];
                lx.string(i, i + 1, q, 0, false, false);
            }
            c if is_ident_start(c) => {
                let mut end = i;
                while end < lx.len() && (is_ident(lx.b[end]) || matches!(lx.b[end], b'-' | b':' | b'.')) {
                    end += 1;
                }
                lx.push(i, end, Kind::Key);
                lx.i = end;
            }
            _ => lx.i += 1,
        }
    }
    Some(Carry::Tag)
}

fn css(lx: &mut Lexer, carry: Carry) -> Carry {
    if let Carry::Block { depth } = carry
        && lx.block(0, 0, "/*", "*/", false, depth) != Carry::Code
    {
        return Carry::Block { depth: 1 };
    }
    while lx.i < lx.len() {
        let i = lx.i;
        let c = lx.b[i];
        if lx.starts(i, "/*") {
            if lx.block(i, i + 2, "/*", "*/", false, 1) != Carry::Code {
                return Carry::Block { depth: 1 };
            }
            continue;
        }
        if lx.starts(i, "//") {
            lx.push(i, lx.len(), Kind::Comment);
            return Carry::Code;
        }
        match c {
            b'"' | b'\'' => {
                lx.string(i, i + 1, c, 0, true, false);
            }
            b'@' if is_ident_start(lx.at(i + 1)) => {
                let end = lx.ident_end(i + 1);
                lx.push(i, end, Kind::Keyword);
                lx.i = end;
            }
            b'!' if lx.starts_ci(i + 1, "important") => {
                lx.push(i, i + 10, Kind::Keyword);
                lx.i = i + 10;
            }
            b'#' if lx.at(i + 1).is_ascii_hexdigit() => {
                let mut end = i + 1;
                while lx.at(end).is_ascii_hexdigit() {
                    end += 1;
                }
                if (4..=9).contains(&(end - i)) && !is_ident(lx.at(end)) {
                    lx.push(i, end, Kind::Number);
                    lx.i = end;
                } else {
                    lx.i = lx.ident_end(i + 1).max(i + 1);
                }
            }
            b'0'..=b'9' if i == 0 || !is_ident(lx.at(i - 1)) && lx.at(i - 1) != b'-' || lx.at(i - 1) == b' ' => {
                let mut end = i;
                while lx.at(end).is_ascii_digit() || lx.at(end) == b'.' {
                    end += 1;
                }
                while lx.at(end).is_ascii_alphabetic() || lx.at(end) == b'%' {
                    end += 1;
                }
                lx.push(i, end, Kind::Number);
                lx.i = end.max(i + 1);
            }
            _ if is_ident_start(c) || c == b'-' => {
                let mut end = i;
                while end < lx.len() && (is_ident(lx.b[end]) || lx.b[end] == b'-') {
                    end += 1;
                }
                let after = lx.skip_spaces(end);
                if lx.at(after) == b':' && !matches!(lx.at(after + 1), b':') && (lx.at(after + 1) == b' ' || after + 1 >= lx.len() || lx.at(after + 1) == b'\t') {
                    lx.push(i, end, Kind::Key);
                } else if lx.at(after) == b'(' {
                    lx.push(i, end, Kind::Function);
                }
                lx.i = end.max(i + 1);
            }
            b'.' if is_ident_start(lx.at(i + 1)) && (i == 0 || !lx.at(i - 1).is_ascii_digit()) => {
                let mut end = i + 1;
                while end < lx.len() && (is_ident(lx.b[end]) || lx.b[end] == b'-') {
                    end += 1;
                }
                lx.push(i, end, Kind::Type);
                lx.i = end;
            }
            _ => lx.i += 1,
        }
    }
    Carry::Code
}

fn ini(lx: &mut Lexer) {
    let start = lx.skip_spaces(0);
    match lx.at(start) {
        b';' | b'#' => lx.push(start, lx.len(), Kind::Comment),
        b'[' => {
            let mut end = start;
            while end < lx.len() && lx.b[end] != b']' {
                end += 1;
            }
            lx.push(start, (end + 1).min(lx.len()), Kind::Heading);
        }
        _ => {
            let mut end = start;
            while end < lx.len() && !matches!(lx.b[end], b'=' | b':') {
                end += 1;
            }
            if end < lx.len() {
                let mut key_end = end;
                while key_end > start && matches!(lx.at(key_end - 1), b' ' | b'\t') {
                    key_end -= 1;
                }
                lx.push(start, key_end, Kind::Key);
                let value = lx.skip_spaces(end + 1);
                let word = lx.word(value, lx.len()).trim_end();
                if word.parse::<f64>().is_ok() {
                    lx.push(value, value + word.len(), Kind::Number);
                } else if matches!(word.to_ascii_lowercase().as_str(), "true" | "false" | "yes" | "no" | "on" | "off") {
                    lx.push(value, value + word.len(), Kind::Constant);
                } else if word.starts_with('"') {
                    lx.push(value, value + word.len(), Kind::String);
                }
            }
        }
    }
}

// --------------------------------------------------------------------------------- iced's highlighter

/// iced's highlighter for the editor: [`line`] per line, with the carry kept at the start of every line.
pub struct Highlighter {
    lang: Lang,
    /// `carries[n]` is what line `n` starts with; always at least one entry.
    carries: Vec<Carry>,
    current: usize,
}

impl iced::advanced::text::Highlighter for Highlighter {
    type Settings = Lang;
    type Highlight = Kind;
    type Iterator<'a> = std::vec::IntoIter<(Range<usize>, Kind)>;

    fn new(settings: &Lang) -> Self {
        Highlighter { lang: *settings, carries: vec![Carry::Code], current: 0 }
    }

    fn update(&mut self, new_settings: &Lang) {
        if *new_settings != self.lang {
            self.lang = *new_settings;
            self.carries.truncate(1);
            self.current = 0;
        }
    }

    fn change_line(&mut self, line: usize) {
        self.current = self.current.min(line);
        self.carries.truncate(self.current + 1);
    }

    fn highlight_line(&mut self, text: &str) -> Self::Iterator<'_> {
        let carry = self.carries.get(self.current).copied().unwrap_or_default();
        let (spans, next) = line(self.lang, text, carry);
        self.current += 1;
        if self.carries.len() > self.current {
            self.carries[self.current] = next;
        } else {
            self.carries.push(next);
        }
        spans.into_iter()
    }

    fn current_line(&self) -> usize {
        self.current
    }
}

/// The editor's format for a kind (its colour; the font stays the editor's).
pub fn format(kind: &Kind, _theme: &iced::Theme) -> iced::advanced::text::highlighter::Format<iced::Font> {
    iced::advanced::text::highlighter::Format { color: Some(color(*kind)), font: None }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(lang: Lang, text: &str) -> Vec<(&str, Kind)> {
        let (spans, _) = line(lang, text, Carry::Code);
        spans.into_iter().map(|(r, k)| (&text[r], k)).collect()
    }

    fn has(lang: Lang, text: &str, piece: &str, kind: Kind) -> bool {
        kinds(lang, text).iter().any(|(p, k)| *p == piece && *k == kind)
    }

    #[test]
    fn languages_are_named_by_their_files() {
        assert_eq!(Lang::of("src/main.rs"), Lang::Rust);
        assert_eq!(Lang::of(r"C:\x\Tool.PY"), Lang::Python);
        assert_eq!(Lang::of("www/src/pages/index.astro"), Lang::Js);
        assert_eq!(Lang::of("a/b.tsx"), Lang::Js);
        assert_eq!(Lang::of("kernels/x.comp"), Lang::Shader);
        assert_eq!(Lang::of("Cargo.lock"), Lang::Toml);
        assert_eq!(Lang::of(".gitignore"), Lang::Shell);
        assert_eq!(Lang::of("tools/run.ps1"), Lang::PowerShell);
        assert_eq!(Lang::of("README"), Lang::Plain);
        assert_eq!(Lang::of("notes.unknownext"), Lang::Plain);
    }

    #[test]
    fn rust_keywords_types_strings_comments_macros_attributes_and_lifetimes() {
        let l = "    pub fn read<'a>(x: &'a str) -> Option<u8> { println!(\"{x}\"); 0x1F_u8 } // done";
        assert!(has(Lang::Rust, l, "pub", Kind::Keyword));
        assert!(has(Lang::Rust, l, "read", Kind::Function));
        assert!(has(Lang::Rust, l, "'a", Kind::Attribute));
        assert!(has(Lang::Rust, l, "str", Kind::Type));
        assert!(has(Lang::Rust, l, "Option", Kind::Type));
        assert!(has(Lang::Rust, l, "println!", Kind::Macro));
        assert!(has(Lang::Rust, l, "\"{x}\"", Kind::String));
        assert!(has(Lang::Rust, l, "0x1F_u8", Kind::Number));
        assert!(has(Lang::Rust, l, "// done", Kind::Comment));
        assert!(has(Lang::Rust, "#[derive(Clone, Debug)]", "#[derive(Clone, Debug)]", Kind::Attribute));
        assert!(has(Lang::Rust, "let c = 'x';", "'x'", Kind::String));
        assert!(has(Lang::Rust, "let c = b'\\n';", "b'\\n'", Kind::String));
        assert!(has(Lang::Rust, "const MAX_LEN: usize = 4;", "MAX_LEN", Kind::Constant));
        assert!(has(Lang::Rust, "for i in 0..10 {}", "0", Kind::Number));
        assert!(has(Lang::Rust, "for i in 0..10 {}", "10", Kind::Number));
        assert!(has(Lang::Rust, "struct Thing;", "Thing", Kind::Type));
    }

    #[test]
    fn a_rust_string_and_a_raw_string_run_on_to_the_next_line() {
        let (spans, carry) = line(Lang::Rust, "let s = \"one", Carry::Code);
        assert_eq!(carry, Carry::Str { quote: b'"', hashes: 0, raw: false });
        assert_eq!(spans.last().unwrap().1, Kind::String);
        let (spans, carry) = line(Lang::Rust, "two\" + 1", carry);
        assert_eq!(carry, Carry::Code);
        assert_eq!(spans[0], (0..4, Kind::String));
        let (_, carry) = line(Lang::Rust, "let r = r#\"a \" still", Carry::Code);
        assert_eq!(carry, Carry::Str { quote: b'"', hashes: 1, raw: true });
        let (spans, carry) = line(Lang::Rust, "end\"# ;", carry);
        assert_eq!((spans[0].clone(), carry), ((0..5, Kind::String), Carry::Code));
    }

    #[test]
    fn rust_block_comments_nest_and_carry() {
        let (_, carry) = line(Lang::Rust, "/* outer /* inner */ still", Carry::Code);
        assert_eq!(carry, Carry::Block { depth: 1 });
        let (spans, carry) = line(Lang::Rust, "end */ fn", carry);
        assert_eq!(carry, Carry::Code);
        assert_eq!(spans[0], (0..6, Kind::Comment));
        assert_eq!(spans[1].1, Kind::Keyword);
    }

    #[test]
    fn python_decorators_defs_triple_quotes_and_prefixed_strings() {
        assert!(has(Lang::Python, "@dataclass", "@dataclass", Kind::Attribute));
        assert!(has(Lang::Python, "def run(self, x=None):", "run", Kind::Function));
        assert!(has(Lang::Python, "def run(self, x=None):", "None", Kind::Constant));
        assert!(has(Lang::Python, "class Store:", "Store", Kind::Type));
        assert!(has(Lang::Python, "x = f'{a}' # note", "f'{a}'", Kind::String));
        assert!(has(Lang::Python, "x = f'{a}' # note", "# note", Kind::Comment));
        assert!(has(Lang::Python, "p = rb\"\\d\"", "rb\"\\d\"", Kind::String));
        let (_, carry) = line(Lang::Python, "    \"\"\"Docstring", Carry::Code);
        assert_eq!(carry, Carry::Triple { quote: b'"', raw: false });
        let (spans, carry) = line(Lang::Python, "ends here\"\"\" + 1", carry);
        assert_eq!((spans[0].clone(), carry), ((0..12, Kind::String), Carry::Code));
    }

    #[test]
    fn javascript_templates_run_on_and_keywords_are_found() {
        assert!(has(Lang::Js, "export async function load() {", "load", Kind::Function));
        assert!(has(Lang::Js, "const x: string = 'a';", "string", Kind::Type));
        assert!(has(Lang::Js, "if (a === undefined) return null;", "undefined", Kind::Constant));
        let (_, carry) = line(Lang::Js, "const t = `line ${a}", Carry::Code);
        assert_eq!(carry, Carry::Str { quote: b'`', hashes: 0, raw: false });
    }

    #[test]
    fn c_preprocessor_lines_and_types() {
        assert!(has(Lang::C, "#include <stdint.h>", "#include <stdint.h>", Kind::Macro));
        assert!(has(Lang::C, "  uint32_t n = sizeof(x);", "uint32_t", Kind::Type));
        assert!(has(Lang::C, "  uint32_t n = sizeof(x);", "sizeof", Kind::Keyword));
        assert!(has(Lang::C, "char c = '\\0';", "'\\0'", Kind::String));
    }

    #[test]
    fn shells_and_powershell() {
        assert!(has(Lang::Shell, "echo \"$HOME\" # where", "# where", Kind::Comment));
        assert!(has(Lang::Shell, "x=${PATH}:a", "${PATH}", Kind::Variable));
        assert!(!kinds(Lang::Shell, "echo a#b").iter().any(|(_, k)| *k == Kind::Comment));
        assert!(has(Lang::PowerShell, "Get-ChildItem -Path $env:TEMP -Recurse", "Get-ChildItem", Kind::Function));
        assert!(has(Lang::PowerShell, "Get-ChildItem -Path $env:TEMP -Recurse", "-Path", Kind::Key));
        assert!(has(Lang::PowerShell, "Get-ChildItem -Path $env:TEMP -Recurse", "$env:TEMP", Kind::Variable));
        assert!(has(Lang::PowerShell, "if ($a -eq $true) {", "-eq", Kind::Keyword));
        assert!(has(Lang::PowerShell, "if ($a -eq $true) {", "$true", Kind::Constant));
        let (_, carry) = line(Lang::PowerShell, "$s = @\"", Carry::Code);
        assert_eq!(carry, Carry::Here { quote: b'"' });
        let (_, carry) = line(Lang::PowerShell, "inside $x", carry);
        assert_eq!(carry, Carry::Here { quote: b'"' });
        let (spans, carry) = line(Lang::PowerShell, "\"@ | Out-File a", carry);
        assert_eq!((spans[0].clone(), carry), ((0..2, Kind::String), Carry::Code));
        let (_, carry) = line(Lang::PowerShell, "<# a block", Carry::Code);
        assert_eq!(carry, Carry::Block { depth: 1 });
    }

    #[test]
    fn json_keys_apart_from_values() {
        let l = r#"  "name": "x", "n": -1.5e3, "ok": true, "none": null"#;
        assert!(has(Lang::Json, l, "\"name\"", Kind::Key));
        assert!(has(Lang::Json, l, "\"x\"", Kind::String));
        assert!(has(Lang::Json, l, "-1.5e3", Kind::Number));
        assert!(has(Lang::Json, l, "true", Kind::Constant));
        assert!(has(Lang::Json, l, "null", Kind::Constant));
    }

    #[test]
    fn toml_sections_keys_and_multiline_strings() {
        assert!(has(Lang::Toml, "[dependencies.iced] # gui", "[dependencies.iced]", Kind::Heading));
        assert!(has(Lang::Toml, "[dependencies.iced] # gui", "# gui", Kind::Comment));
        assert!(has(Lang::Toml, "version = \"0.14.0\"", "version", Kind::Key));
        assert!(has(Lang::Toml, "iced = { version = \"0.14\", default-features = false }", "default-features", Kind::Key));
        assert!(has(Lang::Toml, "iced = { version = \"0.14\", default-features = false }", "false", Kind::Constant));
        let (_, carry) = line(Lang::Toml, "text = \"\"\"", Carry::Code);
        assert_eq!(carry, Carry::Triple { quote: b'"', raw: false });
    }

    #[test]
    fn yaml_markdown_html_css_ini_and_batch() {
        assert!(has(Lang::Yaml, "  - name: build # step", "name", Kind::Key));
        assert!(has(Lang::Yaml, "  - name: build # step", "# step", Kind::Comment));
        assert!(has(Lang::Yaml, "enabled: true", "true", Kind::Constant));
        assert!(has(Lang::Markdown, "## Running it", "## Running it", Kind::Heading));
        assert!(has(Lang::Markdown, "Run `cargo test` now", "`cargo test`", Kind::String));
        let (_, carry) = line(Lang::Markdown, "```rust", Carry::Code);
        assert_eq!(carry, Carry::Fence);
        let (spans, carry) = line(Lang::Markdown, "let x = 1;", carry);
        assert_eq!((spans[0].1, carry), (Kind::String, Carry::Fence));
        assert_eq!(line(Lang::Markdown, "```", Carry::Fence).1, Carry::Code);
        assert!(has(Lang::Html, "<div class=\"a\">x &amp; y</div>", "<div", Kind::Tag));
        assert!(has(Lang::Html, "<div class=\"a\">x &amp; y</div>", "class", Kind::Key));
        assert!(has(Lang::Html, "<div class=\"a\">x &amp; y</div>", "&amp;", Kind::Constant));
        assert_eq!(line(Lang::Html, "<a href=\"x\"", Carry::Code).1, Carry::Tag);
        assert_eq!(line(Lang::Html, "<!-- open", Carry::Code).1, Carry::Block { depth: 1 });
        assert!(has(Lang::Css, "  color: #e6c46a !important;", "color", Kind::Key));
        assert!(has(Lang::Css, "  color: #e6c46a !important;", "#e6c46a", Kind::Number));
        assert!(has(Lang::Css, "@media (max-width: 600px) {", "600px", Kind::Number));
        assert!(has(Lang::Ini, "[core]", "[core]", Kind::Heading));
        assert!(has(Lang::Ini, "autocrlf = true", "autocrlf", Kind::Key));
        // Two pieces of one kind that touch are one piece.
        assert!(has(Lang::Batch, "@echo off", "@echo", Kind::Keyword));
        assert!(has(Lang::Batch, "set X=%PATH%", "%PATH%", Kind::Variable));
        assert!(has(Lang::Batch, "rem a note", "rem a note", Kind::Comment));
    }

    /// The pieces of any line: in order, apart, non-empty and cut only at character boundaries, in every language,
    /// from every carry; and a line past the limit is coloured only up to it.
    #[test]
    fn every_piece_is_ordered_apart_and_on_character_boundaries() {
        let samples = [
            "", " ", "\t", "é", "\"", "'", "`", "/*", "*/", "<#", "#>", "<!--", "-->", "#[", "r#\"", "'''", "\"\"\"",
            "let ünïcödé = \"日本語\"; // コメント",
            "fn f<'a>(x: &'a [u8]) -> 'a' { 'é' }",
            "x = '\\u{1F600}' + \"\\\"\" + 1e-3 + .5",
            "@\"", "$", "${", "%", "%%i", "!x!", "<a b='c", "& ;", "&#123;", "#fff", "0x", "0b2", "1.2.3", "a.0",
            "name: \"unterminated", "- : :", "# heading #", "```", "~~~", "1) item", "* item `code",
            "SELECT * FROM t WHERE a = 'x' -- c", "Get-Item -Path \"$a`n\" # c", "<# x #> y", "\\", "\\\\\"",
            "🦀🦀 \"🦀\" '🦀' `🦀` // 🦀",
        ];
        let langs = [
            Lang::Rust, Lang::Python, Lang::Js, Lang::C, Lang::CSharp, Lang::Go, Lang::Java, Lang::Shader, Lang::Json,
            Lang::Toml, Lang::Yaml, Lang::Markdown, Lang::Shell, Lang::PowerShell, Lang::Batch, Lang::Sql, Lang::Html,
            Lang::Css, Lang::Ini, Lang::Plain,
        ];
        let carries = [
            Carry::Code,
            Carry::Block { depth: 2 },
            Carry::Str { quote: b'"', hashes: 1, raw: true },
            Carry::Str { quote: b'`', hashes: 0, raw: false },
            Carry::Triple { quote: b'\'', raw: false },
            Carry::Fence,
            Carry::Here { quote: b'\'' },
            Carry::Tag,
        ];
        for lang in langs {
            for carry in carries {
                for a in samples {
                    for b in samples {
                        let text = format!("{a}{b}");
                        let (spans, _) = line(lang, &text, carry);
                        let mut end = 0;
                        for (range, _) in &spans {
                            assert!(range.start < range.end, "{lang:?} {carry:?} {text:?}: empty {range:?}");
                            assert!(range.start >= end, "{lang:?} {carry:?} {text:?}: overlap at {range:?}");
                            assert!(range.end <= text.len(), "{lang:?} {text:?}: past the end");
                            assert!(text.is_char_boundary(range.start) && text.is_char_boundary(range.end), "{lang:?} {text:?}: {range:?}");
                            end = range.end;
                        }
                    }
                }
            }
        }
        let long = format!("\"{}", "é".repeat(LINE_LIMIT));
        let (spans, _) = line(Lang::Rust, &long, Carry::Code);
        assert!(spans.iter().all(|(r, _)| r.end <= LINE_LIMIT && long.is_char_boundary(r.end)));
    }

    #[test]
    fn the_highlighter_recolours_from_a_changed_line_with_the_carry_it_kept() {
        use iced::advanced::text::Highlighter as _;
        let mut h = Highlighter::new(&Lang::Rust);
        let first: Vec<_> = h.highlight_line("/* open").collect();
        let second: Vec<_> = h.highlight_line("still */ fn").collect();
        assert_eq!(first[0].1, Kind::Comment);
        assert_eq!(second[0], (0..8, Kind::Comment));
        assert_eq!(h.current_line(), 2);
        h.change_line(1);
        assert_eq!(h.current_line(), 1);
        let again: Vec<_> = h.highlight_line("still */ fn").collect();
        assert_eq!(again, second, "line 1 starts inside the comment line 0 left open");
        h.update(&Lang::Python);
        assert_eq!(h.current_line(), 0);
    }

    #[test]
    fn every_colour_holds_45_to_1_on_the_editor_and_the_panels() {
        use lattice_app::theme::{CANVAS, SURFACE, contrast};
        for kind in [
            Kind::Keyword, Kind::Type, Kind::Function, Kind::String, Kind::Number, Kind::Constant, Kind::Comment,
            Kind::Attribute, Kind::Macro, Kind::Key, Kind::Heading, Kind::Tag, Kind::Variable,
        ] {
            for bg in [CANVAS, SURFACE, lattice_app::theme::RAISED] {
                let ratio = contrast(color(kind), bg);
                assert!(ratio >= 4.5, "{kind:?} on {bg:?}: {ratio:.2}");
            }
        }
    }
}

#[cfg(test)]
mod real_sources {
    use super::*;

    /// The crate's own sources and documents, coloured whole with the carry kept line to line: every piece is in
    /// order, apart, non-empty and on character boundaries (real input, beside the synthetic lines above).
    #[test]
    fn the_crates_own_files_colour_cleanly() {
        let files: [(&str, &str); 6] = [
            ("tree.rs", include_str!("tree.rs")),
            ("highlight.rs", include_str!("highlight.rs")),
            ("buffer.rs", include_str!("buffer.rs")),
            ("view.rs", include_str!("view.rs")),
            ("README.md", include_str!("../../../README.md")),
            ("Cargo.toml", include_str!("../../../Cargo.toml")),
        ];
        for (name, text) in files {
            let lang = Lang::of(name);
            let mut carry = Carry::Code;
            for (n, l) in text.lines().enumerate() {
                let (spans, next) = line(lang, l, carry);
                let mut end = 0;
                for (range, _) in &spans {
                    assert!(range.start < range.end && range.start >= end && range.end <= l.len(), "{name}:{}: {range:?}", n + 1);
                    assert!(l.is_char_boundary(range.start) && l.is_char_boundary(range.end), "{name}:{}", n + 1);
                    end = range.end;
                }
                carry = next;
            }
            assert_eq!(carry, Carry::Code, "{name} ends inside something");
        }
    }
}
