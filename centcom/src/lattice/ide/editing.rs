//! The editor's editing comforts, as an editor for code has them: Enter keeps the line's indentation (one more after
//! an opening bracket, and a bracket pair opened onto three lines); Tab and Shift+Tab indent and unindent by the
//! file's own unit (its spaces or its tabs: iced's editor would indent to eight-column stops); Ctrl+/ comments lines
//! out and back in, as the language writes comments; Alt+Up and Alt+Down move lines, Shift+Alt+Up and Shift+Alt+Down
//! copy them, Ctrl+Shift+K deletes them; Home goes to the first character that is not a space, then to the line's
//! start.
//!
//! Pure: lines in, lines (or what to type) out. The page applies each as one edit, which the editor's undo takes back.

use super::highlight::Lang;

/// The most lines read to learn a file's indentation.
const SAMPLE: usize = 2000;

/// A line's indentation: its leading spaces and tabs.
pub fn leading(line: &str) -> &str {
    &line[..line.len() - line.trim_start_matches([' ', '\t']).len()]
}

/// The file's indentation unit: a tab when more of its indented lines start with one, else the spaces its
/// indentation steps by (two or four; eight only when every indented line is a multiple of eight), else the
/// language's custom (Go indents with tabs; the rest with four spaces).
pub fn unit<'a>(lines: impl IntoIterator<Item = &'a str>, lang: Lang) -> String {
    let (mut tabs, mut spaced) = (0usize, Vec::new());
    for line in lines.into_iter().take(SAMPLE) {
        if line.trim().is_empty() {
            continue;
        }
        if line.starts_with('\t') {
            tabs += 1;
        } else if line.starts_with(' ') {
            spaced.push(line.len() - line.trim_start_matches(' ').len());
        }
    }
    if tabs > spaced.len() {
        return "\t".to_string();
    }
    if spaced.is_empty() {
        return if lang == Lang::Go { "\t".to_string() } else { "    ".to_string() };
    }
    let fits = |n: usize| spaced.iter().filter(|&&w| w % n == 0).count() * 10 >= spaced.len() * 9;
    let width = if fits(8) && spaced.iter().all(|&w| w % 8 == 0) {
        8
    } else if fits(4) {
        4
    } else if fits(2) {
        2
    } else {
        4
    };
    " ".repeat(width)
}

/// What Enter types at byte `column` of `line`: a line end and the indentation the new line takes, and where the
/// cursor goes (lines down from the line, and the byte in that line). After an opening bracket (or a colon in Python
/// and YAML) the new line is indented one unit more; between a bracket and its closing one, the closing one goes onto
/// a line of its own.
pub fn newline(line: &str, column: usize, lang: Lang, unit: &str) -> (String, (usize, usize)) {
    let column = column.min(line.len());
    let before = line[..column].trim_end();
    let after = line[column..].trim_start();
    let indent = leading(line);
    // An indentation typed so far but not ended is kept as it is, not doubled.
    let indent = if line[..column].trim().is_empty() { &line[..column] } else { indent };
    let opens = before.ends_with(['{', '(', '['])
        || (matches!(lang, Lang::Python | Lang::Yaml) && before.ends_with(':'));
    let pair = matches!((before.chars().last(), after.chars().next()), (Some('{'), Some('}')) | (Some('('), Some(')')) | (Some('['), Some(']')));
    if pair {
        let inner = format!("{indent}{unit}");
        let text = format!("\n{inner}\n{indent}");
        return (text, (1, inner.len()));
    }
    let next = if opens { format!("{indent}{unit}") } else { indent.to_string() };
    let at = next.len();
    (format!("\n{next}"), (1, at))
}

/// `lines` indented one unit more (blank lines left as they are).
pub fn indent(lines: &[&str], unit: &str) -> Vec<String> {
    lines.iter().map(|l| if l.trim().is_empty() { l.to_string() } else { format!("{unit}{l}") }).collect()
}

/// `lines` with up to one unit of indentation taken off each (a tab, or the unit's spaces), and how many bytes each
/// lost.
pub fn unindent(lines: &[&str], unit: &str) -> (Vec<String>, Vec<usize>) {
    let width = if unit == "\t" { 1 } else { unit.len() };
    let mut lost = Vec::with_capacity(lines.len());
    let out = lines
        .iter()
        .map(|l| {
            let cut = if l.starts_with('\t') {
                1
            } else {
                l.bytes().take(width).take_while(|b| *b == b' ').count()
            };
            lost.push(cut);
            l[cut..].to_string()
        })
        .collect();
    (out, lost)
}

/// How a language writes a comment on one line: what opens it and what closes it (most close at the line's end).
pub fn comment(lang: Lang) -> Option<(&'static str, &'static str)> {
    match lang {
        Lang::Rust | Lang::C | Lang::CSharp | Lang::Go | Lang::Java | Lang::Js | Lang::Shader => Some(("//", "")),
        Lang::Python | Lang::Shell | Lang::PowerShell | Lang::Yaml | Lang::Toml => Some(("#", "")),
        Lang::Ini => Some((";", "")),
        Lang::Sql => Some(("--", "")),
        Lang::Batch => Some(("REM", "")),
        Lang::Css => Some(("/*", "*/")),
        Lang::Html | Lang::Markdown => Some(("<!--", "-->")),
        Lang::Json | Lang::Plain => None,
    }
}

/// `lines` commented out, or back in when every line that is not blank already is one, as `comment` writes it:
/// the mark goes at the shallowest indentation of the lines, then a space. The lines, and whether they were
/// commented out (`false`: back in).
pub fn toggle_comment(lines: &[&str], (open, close): (&str, &str)) -> (Vec<String>, bool) {
    let written: Vec<&&str> = lines.iter().filter(|l| !l.trim().is_empty()).collect();
    if written.is_empty() {
        return (lines.iter().map(|l| l.to_string()).collect(), false);
    }
    let all = written.iter().all(|l| {
        let t = l.trim();
        t.starts_with(open) && (close.is_empty() || t.ends_with(close))
    });
    if all {
        let out = lines
            .iter()
            .map(|l| {
                if l.trim().is_empty() {
                    return l.to_string();
                }
                let pad = leading(l);
                let mut body = &l[pad.len() + open.len()..];
                body = body.strip_prefix(' ').unwrap_or(body);
                if !close.is_empty() {
                    body = body.trim_end();
                    body = body.strip_suffix(close).unwrap_or(body);
                    body = body.strip_suffix(' ').unwrap_or(body);
                }
                format!("{pad}{body}")
            })
            .collect();
        return (out, false);
    }
    let column = written.iter().map(|l| leading(l).len()).min().unwrap_or(0);
    let out = lines
        .iter()
        .map(|l| {
            if l.trim().is_empty() {
                return l.to_string();
            }
            let (pad, body) = l.split_at(column.min(leading(l).len()));
            if close.is_empty() { format!("{pad}{open} {body}") } else { format!("{pad}{open} {body} {close}") }
        })
        .collect();
    (out, true)
}

/// Where Home goes from byte `column` of `line`: the first character that is not a space, or, from there, the line's
/// start.
pub fn home(line: &str, column: usize) -> usize {
    let first = leading(line).len();
    if column == first { 0 } else { first }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_indentation_unit_is_the_files_own() {
        assert_eq!(unit(["fn a() {", "    let x = 1;", "        x", "}"], Lang::Rust), "    ");
        assert_eq!(unit(["a:", "  b: 1", "  c:", "    d: 2"], Lang::Yaml), "  ");
        assert_eq!(unit(["func a() {", "\tx := 1", "\tif x {", "\t\ty()", "\t}", "}"], Lang::Go), "\t");
        assert_eq!(unit(["package main"], Lang::Go), "\t", "Go's custom with nothing indented yet");
        assert_eq!(unit(["x = 1"], Lang::Python), "    ");
        assert_eq!(unit(["a", "        b", "                c"], Lang::C), " ".repeat(8));
        assert_eq!(unit(["a", "    b", "     c (one stray)", "        d", "    e", "    f", "    g", "    h", "    i", "    j", "    k"], Lang::C), "    ");
    }

    #[test]
    fn enter_keeps_the_indentation_and_opens_a_bracket_one_deeper() {
        let u = "    ";
        assert_eq!(newline("    let x = 1;", 14, Lang::Rust, u), ("\n    ".to_string(), (1, 4)));
        assert_eq!(newline("    if x {", 10, Lang::Rust, u), ("\n        ".to_string(), (1, 8)));
        assert_eq!(newline("    f(a, {}", 10, Lang::Rust, u), ("\n        \n    ".to_string(), (1, 8)), "between a pair");
        assert_eq!(newline("def f():", 8, Lang::Python, u), ("\n    ".to_string(), (1, 4)));
        assert_eq!(newline("x: 1", 4, Lang::Rust, u), ("\n".to_string(), (1, 0)), "a colon opens nothing in Rust");
        assert_eq!(newline("  ", 2, Lang::Rust, u), ("\n  ".to_string(), (1, 2)), "an indentation typed so far");
        assert_eq!(newline("\tx {", 4, Lang::Go, "\t"), ("\n\t\t".to_string(), (1, 2)));
        assert_eq!(newline("    ab", 5, Lang::Rust, u), ("\n    ".to_string(), (1, 4)), "the line split in its middle");
    }

    #[test]
    fn indent_and_unindent_by_the_unit_and_leave_blank_lines_alone() {
        assert_eq!(indent(&["a", "", "  b"], "    "), ["    a", "", "      b"]);
        assert_eq!(indent(&["a"], "\t"), ["\ta"]);
        let (out, lost) = unindent(&["      a", "  b", "c", "\td"], "    ");
        assert_eq!(out, ["  a", "b", "c", "d"]);
        assert_eq!(lost, [4, 2, 0, 1]);
    }

    #[test]
    fn a_comment_toggles_at_the_shallowest_indentation_as_the_language_writes_it() {
        let rust = comment(Lang::Rust).unwrap();
        let (out, added) = toggle_comment(&["    let a = 1;", "", "        b();"], rust);
        assert!(added);
        assert_eq!(out, ["    // let a = 1;", "", "    //     b();"]);
        let back: Vec<&str> = out.iter().map(String::as_str).collect();
        let (again, added) = toggle_comment(&back, rust);
        assert!(!added);
        assert_eq!(again, ["    let a = 1;", "", "        b();"], "back as it was");
        let (mixed, added) = toggle_comment(&["// a", "b"], rust);
        assert!(added, "not every line is a comment: comment them all");
        assert_eq!(mixed, ["// // a", "// b"]);
        let css = comment(Lang::Css).unwrap();
        assert_eq!(toggle_comment(&["a { b: c; }"], css).0, ["/* a { b: c; } */"]);
        assert_eq!(toggle_comment(&["/* a { b: c; } */"], css).0, ["a { b: c; }"]);
        assert_eq!(toggle_comment(&["x = 1"], comment(Lang::Python).unwrap()).0, ["# x = 1"]);
        assert_eq!(comment(Lang::Json), None, "JSON has no comments");
        assert_eq!(toggle_comment(&["", "  "], rust).0, ["", "  "], "blank lines only: nothing changes");
    }

    #[test]
    fn home_goes_to_the_first_character_then_the_lines_start() {
        assert_eq!(home("    let x", 9), 4);
        assert_eq!(home("    let x", 4), 0);
        assert_eq!(home("    let x", 0), 4);
        assert_eq!(home("x", 1), 0);
        assert_eq!(home("", 0), 0);
    }
}
