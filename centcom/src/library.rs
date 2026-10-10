//! The transcript library: transcripts a person chose to keep, on this PC, until they delete them.
//!
//! Where and how long (decided 2026-10-03): `~/.alelyon/angel/transcripts`, this PC only, kept until
//! deleted. Nothing is saved without a click: live captions and dictation stay on screen unless someone presses
//! Save, and a file's transcript joins the library only when they choose to keep it there.
//!
//! Each transcript is one plain-text file whose name says when it was saved and what it is
//! (`2026-10-03 20.41.07 Captions.txt`), so the folder reads well in Explorer too. Deleting moves a file into
//! `Deleted/`, from which it can be restored by hand; emptying that folder is the only permanent step, and the
//! window asks twice before it.

use std::path::{Path, PathBuf};

/// The kinds a transcript can be, as its file name ends.
pub const KINDS: [&str; 4] = ["Captions", "PC audio", "Dictation", "File"];

pub fn dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE").map(|home| PathBuf::from(home).join(".alelyon").join("angel").join("transcripts"))
}

fn deleted_dir(root: &Path) -> PathBuf {
    root.join("Deleted")
}

/// One kept transcript.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub path: PathBuf,
    /// The file name without `.txt`.
    pub name: String,
    pub kind: String,
    pub bytes: u64,
    pub text: String,
}

impl Entry {
    /// Does it contain every word of `query` (in its name or its text), ignoring case?
    pub fn matches(&self, query: &str) -> bool {
        let hay = format!("{}\n{}", self.name, self.text).to_lowercase();
        query.split_whitespace().all(|word| hay.contains(&word.to_lowercase()))
    }

    /// The first words, for the list.
    pub fn preview(&self) -> String {
        let flat = self.text.split_whitespace().collect::<Vec<_>>().join(" ");
        match flat.char_indices().nth(140) {
            Some((at, _)) => format!("{}…", &flat[..at]),
            None => flat,
        }
    }
}

/// Every kept transcript in `root`, newest first (names begin with the time they were saved).
pub fn list_in(root: &Path) -> Result<Vec<Entry>, String> {
    let read = match std::fs::read_dir(root) {
        Ok(read) => read,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("{}: {e}", root.display())),
    };
    let mut out = Vec::new();
    for item in read.flatten() {
        let path = item.path();
        if !path.is_file() || path.extension().and_then(|e| e.to_str()) != Some("txt") {
            continue;
        }
        let name = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        let kind = kind_of(&name);
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        let bytes = item.metadata().map(|m| m.len()).unwrap_or(0);
        out.push(Entry { path, name, kind, bytes, text });
    }
    out.sort_by(|a, b| b.name.cmp(&a.name));
    Ok(out)
}

/// The kind a name says, right after its time (`2026-10-03 20.41.07 File - interview`).
fn kind_of(name: &str) -> String {
    let rest = name.get(20..).unwrap_or("");
    KINDS.iter().find(|k| rest.starts_with(*k)).map(|k| k.to_string()).unwrap_or_else(|| "Transcript".into())
}

/// `text` kept as a `kind` transcript in `root`, named for `stamp` (local time, `YYYY-MM-DD HH.MM.SS`) and, for a
/// file, its `title`; a transcript never overwrites another.
pub fn save_in(root: &Path, stamp: &str, kind: &str, title: Option<&str>, text: &str) -> Result<PathBuf, String> {
    if text.trim().is_empty() {
        return Err("there is nothing to save yet".into());
    }
    std::fs::create_dir_all(root).map_err(|e| format!("{}: {e}", root.display()))?;
    // a title may carry characters a file name cannot
    let title: Option<String> = title.map(|t| t.chars().map(|c| if r#"<>:"/\|?*"#.contains(c) || c.is_control() { '_' } else { c }).collect());
    let stem = match title {
        Some(t) if !t.trim().is_empty() => format!("{stamp} {kind} - {}", t.trim()),
        _ => format!("{stamp} {kind}"),
    };
    let path = unique(root, &stem);
    std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(path)
}

/// Move a transcript into `Deleted/`, from which it can be restored.
pub fn delete_in(root: &Path, path: &Path) -> Result<(), String> {
    if path.parent() != Some(root) {
        return Err(format!("{} is not in the library", path.display()));
    }
    let bin = deleted_dir(root);
    std::fs::create_dir_all(&bin).map_err(|e| format!("{}: {e}", bin.display()))?;
    let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "transcript".into());
    std::fs::rename(path, unique(&bin, &stem)).map_err(|e| format!("{}: {e}", path.display()))
}

/// How many transcripts are in `Deleted/`.
pub fn deleted_count_in(root: &Path) -> usize {
    std::fs::read_dir(deleted_dir(root)).map(|r| r.flatten().filter(|i| i.path().is_file()).count()).unwrap_or(0)
}

/// Remove what is in `Deleted/` for good: the one permanent step, taken only on a person's second click.
pub fn empty_deleted_in(root: &Path) -> Result<usize, String> {
    let bin = deleted_dir(root);
    let mut gone = 0;
    for item in std::fs::read_dir(&bin).map_err(|e| format!("{}: {e}", bin.display()))?.flatten() {
        if item.path().is_file() {
            std::fs::remove_file(item.path()).map_err(|e| format!("{}: {e}", item.path().display()))?;
            gone += 1;
        }
    }
    Ok(gone)
}

/// `dir/stem.txt`, or `stem (2).txt` and so on when that exists.
fn unique(dir: &Path, stem: &str) -> PathBuf {
    let first = dir.join(format!("{stem}.txt"));
    if !first.exists() {
        return first;
    }
    (2..).map(|n| dir.join(format!("{stem} ({n}).txt"))).find(|p| !p.exists()).expect("an unused name exists")
}

/// The local time now as `YYYY-MM-DD HH.MM.SS`, for a file name (dots, because a name may not hold a colon).
pub fn stamp_now() -> String {
    #[cfg(windows)]
    {
        // SAFETY: GetLocalTime only writes the struct it is given.
        #[allow(unsafe_code)]
        let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
        format!("{:04}-{:02}-{:02} {:02}.{:02}.{:02}", t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond)
    }
    #[cfg(not(windows))]
    {
        let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let (days, rem) = (secs / 86_400, secs % 86_400);
        let (y, m, d) = civil(days as i64);
        format!("{y:04}-{m:02}-{d:02} {:02}.{:02}.{:02}", rem / 3600, rem % 3600 / 60, rem % 60)
    }
}

/// Days since 1970-01-01 as a calendar date (Howard Hinnant's algorithm).
#[cfg(not(windows))]
fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("centcom-library-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn a_kept_transcript_is_listed_newest_first_and_never_overwrites_another() {
        let root = temp("keep");
        assert!(list_in(&root).unwrap().is_empty(), "no folder yet is an empty library");
        save_in(&root, "2026-10-03 20.41.07", "Captions", None, "[0:01] Hello there.").unwrap();
        save_in(&root, "2026-10-03 21.00.00", "Dictation", None, "Dear Sam, thank you.").unwrap();
        let again = save_in(&root, "2026-10-03 21.00.00", "Dictation", None, "A second note.").unwrap();
        assert!(again.ends_with("2026-10-03 21.00.00 Dictation (2).txt"), "{}", again.display());
        let file = save_in(&root, "2026-10-03 21.05.00", "File", Some("interview: part 1"), "Q. A.").unwrap();
        assert!(file.ends_with("2026-10-03 21.05.00 File - interview_ part 1.txt"), "{}", file.display());
        let all = list_in(&root).unwrap();
        assert_eq!(all.iter().map(|e| e.kind.as_str()).collect::<Vec<_>>(), ["File", "Dictation", "Dictation", "Captions"]);
        assert!(save_in(&root, "x", "Captions", None, "  ").is_err(), "nothing to save is said so");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn search_finds_every_word_in_the_name_or_the_text() {
        let e = Entry { path: "a.txt".into(), name: "2026-10-03 20.41.07 Captions".into(), kind: "Captions".into(), bytes: 0,
            text: "We agreed to ship the ears on Friday.".into() };
        assert!(e.matches("ears friday"));
        assert!(e.matches("CAPTIONS 2026-10-03"));
        assert!(!e.matches("ears monday"));
        assert!(e.matches(""), "an empty search shows everything");
    }

    #[test]
    fn deleting_moves_to_deleted_and_only_emptying_is_for_good() {
        let root = temp("delete");
        let kept = save_in(&root, "2026-10-03 20.41.07", "Captions", None, "words").unwrap();
        delete_in(&root, &kept).unwrap();
        assert!(list_in(&root).unwrap().is_empty());
        assert_eq!(deleted_count_in(&root), 1, "restorable from Deleted");
        assert!(delete_in(&root, Path::new("C:/Windows/win.ini")).is_err(), "nothing outside the library is touched");
        assert_eq!(empty_deleted_in(&root).unwrap(), 1);
        assert_eq!(deleted_count_in(&root), 0);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_long_transcript_is_previewed_by_its_first_words() {
        let e = Entry { path: "a.txt".into(), name: "n".into(), kind: "File".into(), bytes: 0, text: "word ".repeat(100) };
        assert!(e.preview().ends_with('…'));
        assert!(e.preview().chars().count() <= 141);
    }
}
