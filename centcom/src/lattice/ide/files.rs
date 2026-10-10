//! The person's own reads and saves of the attached folder's files: the IDE's editor (it saves directly,
//! guarded).
//!
//! Every path goes through lattice-core's path rules first (`PathRules::resolve`, WP1-WP11: inside the folder and not
//! the folder, never through a link that leaves it, not git's own directory, not Lattice's state, not an ignored file,
//! no device names or 8.3 aliases), decided on the derived long-name path, the rules the agent's tools are held to.
//! So the editor opens and writes exactly the files the agent could read, and the Explorer lists only those
//! ([`Folder::listing`] is the core's own listing).
//!
//! A save is refused, and nothing written, when:
//! - the file on disk is no longer the one the editor opened (its SHA-256 differs): "changed on disk since you opened
//!   it" ([`SaveError::Changed`]);
//! - it is an authority file (the core's ST4: `.github/`, `AGENTS.md`, `Cargo.toml`, scripts, ...) and the person has
//!   not confirmed this save ([`SaveError::NeedsConfirm`]);
//! - the path is refused by a rule, or is not a file.
//!
//! The write: a temporary beside the file (the core's own temporary name, `fsx::temporary_for`), written and synced,
//! then `ReplaceFileW` over the file, which keeps the file's identity, attributes and permissions; a sharing violation
//! is retried 6 times, 20 ms apart, as the core retries one. On a failure the temporary is removed (the one removal
//! here, of this module's own temporary) and the file is left as it was. Afterwards the file is hashed again, and a
//! difference from what was written is said ("written, then changed by another program"), not hidden.
//!
//! What this does not exclude: a program of the same user changing the file, or a folder above it, in the moment
//! between the check and the replace (the core's Keep has the same window). No writer lease is taken: the lease keeps
//! two AGENTS from writing one checkout at once, and this is the person, not an agent; an agent's change waiting on a
//! file the person saved is re-applied or becomes a conflict when it is kept, by the core's own rule.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use lattice_core::git::runner::GitRunner;
use lattice_core::policy::PathClass;
use lattice_core::tools::read::{FileListing, NoOverlay, ReadContext, SearchArgs, SearchReport};
use lattice_core::workspace::Workspace;
use lattice_core::workspace::paths::{PathError, Want};
use lattice_core::{Env, StateRoot};

use super::buffer::{self, Decoded, Undecodable};

/// A folder as the editor reads and writes it.
pub struct Folder {
    workspace: Workspace,
    runner: GitRunner,
}

impl std::fmt::Debug for Folder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Folder").field("id", &self.workspace.id).field("root", &self.workspace.root).finish()
    }
}

/// A file opened for the editor.
#[derive(Clone, Debug)]
pub struct Opened {
    /// The derived path: relative, forward slashes, the file system's own spelling.
    pub path: String,
    /// SHA-256 of the bytes read, the base a save is checked against.
    pub sha256: String,
    pub authority: bool,
    /// The text, or why it is shown and not edited.
    pub text: Result<Decoded, Undecodable>,
}

/// What a save did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Saved {
    pub path: String,
    pub sha256: String,
    pub size: u64,
    /// The file's bytes were not the ones written when it was hashed again.
    pub changed_after: bool,
}

/// Why nothing was written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SaveError {
    Changed,
    NeedsConfirm,
    Exists,
    Refused(String),
    Failed(String),
}

impl SaveError {
    pub fn sentence(&self) -> String {
        match self {
            SaveError::Changed => {
                "The file changed on disk since you opened it, so nothing was written: reload it (your edits are kept \
                 until you choose) or copy them out first."
                    .to_string()
            }
            SaveError::NeedsConfirm => "This is an authority file: saving it needs your confirmation.".to_string(),
            SaveError::Exists => "A file of that name is there already, so nothing was written.".to_string(),
            SaveError::Refused(why) => format!("Nothing was written: {why}"),
            SaveError::Failed(why) => format!("Nothing was written: {why}"),
        }
    }
}

/// The most bytes read for the editor; a larger file is shown a page at a time through the core.
const READ_LIMIT: u64 = buffer::EDIT_LIMIT as u64;
const RETRIES: u32 = 6;
const RETRY_PAUSE: Duration = Duration::from_millis(20);

fn path_sentence(error: PathError) -> String {
    error.sentence()
}

impl Folder {
    /// Attach `path` as the core attaches a folder (the same refusals: a drive root, the Windows folder, Lattice's
    /// state, the profile itself, AppData, ...). Blocking: call it off the window's thread.
    pub fn attach(path: &Path, env: Arc<dyn Env>, state: &StateRoot) -> Result<Folder, String> {
        let runner = GitRunner::new(env.clone(), state);
        let workspace = lattice_core::workspace::attach::attach_path(path, env.as_ref(), state, &runner)
            .map_err(|refusal| refusal.sentence().to_string())?;
        Ok(Folder { workspace, runner })
    }

    /// The workspace id, the one the chat core gives the same folder.
    pub fn id(&self) -> &str {
        &self.workspace.id
    }

    /// The folder's path as people read it.
    pub fn shown_path(&self) -> String {
        lattice_core::workspace::shown_path(&self.workspace.root)
    }

    pub fn name(&self) -> &str {
        &self.workspace.name
    }

    /// Every file the read tools see, sorted (the core's listing).
    pub fn listing(&self) -> Result<FileListing, String> {
        let ctx = ReadContext { workspace: &self.workspace, runner: &self.runner, overlay: &NoOverlay };
        lattice_core::tools::read::listing(&ctx).map_err(|e| e.0)
    }

    /// The person's search across the folder (the Search view): the files the agent's grep reads, through the core's
    /// own rules, matched by `args` (the disk as it is: nothing staged).
    pub fn search(&self, args: &SearchArgs) -> Result<SearchReport, String> {
        let ctx = ReadContext { workspace: &self.workspace, runner: &self.runner, overlay: &NoOverlay };
        lattice_core::tools::read::search(&ctx, args).map_err(|e| e.0)
    }

    /// Open `path` (derived, as listed) for the editor.
    pub fn open(&self, path: &str) -> Result<Opened, String> {
        let resolved = self
            .workspace
            .with_rules(&self.runner, |rules| rules.resolve(path, Want::Existing))
            .map_err(|_| RULES_UNREADABLE.to_string())?
            .map_err(path_sentence)?;
        if resolved.is_dir {
            return Err("That is a folder, not a file.".to_string());
        }
        let authority = resolved.class == PathClass::Authority;
        let file = resolved.file.ok_or_else(|| "That file could not be opened.".to_string())?;
        let (bytes, size) = read_bounded(file)?;
        let sha256 = lattice_core::sha::sha256_hex(&bytes);
        let text = if size > READ_LIMIT { Err(Undecodable::TooLarge) } else { buffer::decode(&bytes) };
        Ok(Opened { path: resolved.derived, sha256, authority, text })
    }

    /// Write `bytes` over `path`, which the editor opened as `base_sha256` (see the module header).
    pub fn save(&self, path: &str, base_sha256: &str, bytes: &[u8], confirmed: bool) -> Result<Saved, SaveError> {
        let resolved = self
            .workspace
            .with_rules(&self.runner, |rules| rules.resolve(path, Want::Existing))
            .map_err(|_| SaveError::Refused(RULES_UNREADABLE.to_string()))?
            .map_err(|e| SaveError::Refused(path_sentence(e)))?;
        if resolved.is_dir {
            return Err(SaveError::Refused("that is a folder, not a file.".to_string()));
        }
        if resolved.class == PathClass::Authority && !confirmed {
            return Err(SaveError::NeedsConfirm);
        }
        let file = resolved.file.ok_or_else(|| SaveError::Failed("the file could not be opened.".to_string()))?;
        let (now, size) = read_bounded(file).map_err(SaveError::Failed)?;
        if size > READ_LIMIT || lattice_core::sha::sha256_hex(&now) != base_sha256 {
            return Err(SaveError::Changed);
        }
        replace(&resolved.final_path, bytes).map_err(SaveError::Failed)?;
        Ok(self.after(path, &resolved.derived, bytes))
    }

    /// Make the new file `path` holding `bytes`, never over a file that is there.
    pub fn create(&self, path: &str, bytes: &[u8], confirmed: bool) -> Result<Saved, SaveError> {
        let resolved = self
            .workspace
            .with_rules(&self.runner, |rules| rules.resolve(path, Want::MayCreate))
            .map_err(|_| SaveError::Refused(RULES_UNREADABLE.to_string()))?
            .map_err(|e| match e {
                PathError::Missing => SaveError::Refused("the folder it would go in does not exist.".to_string()),
                other => SaveError::Refused(path_sentence(other)),
            })?;
        if resolved.exists {
            return Err(SaveError::Exists);
        }
        if resolved.class == PathClass::Authority && !confirmed {
            return Err(SaveError::NeedsConfirm);
        }
        place_new(&resolved.final_path, bytes).map_err(|e| match e {
            Placed::Exists => SaveError::Exists,
            Placed::Failed(why) => SaveError::Failed(why),
        })?;
        Ok(self.after(path, &resolved.derived, bytes))
    }

    /// Hash the file again after a write.
    fn after(&self, path: &str, derived: &str, bytes: &[u8]) -> Saved {
        let written = lattice_core::sha::sha256_hex(bytes);
        let now = self
            .workspace
            .with_rules(&self.runner, |rules| rules.resolve(path, Want::Existing))
            .ok()
            .and_then(Result::ok)
            .and_then(|r| r.file)
            .and_then(|f| read_bounded(f).ok())
            .map(|(b, _)| lattice_core::sha::sha256_hex(&b));
        Saved {
            path: derived.to_string(),
            changed_after: now.as_deref() != Some(written.as_str()),
            sha256: written,
            size: bytes.len() as u64,
        }
    }
}

const RULES_UNREADABLE: &str = "Lattice could not read this folder's .latticeignore, so nothing is shown.";

/// Up to [`READ_LIMIT`] + 1 bytes of `file`, and its length.
fn read_bounded(file: std::fs::File) -> Result<(Vec<u8>, u64), String> {
    let size = file.metadata().map(|m| m.len()).map_err(|e| format!("the file could not be read: {e}"))?;
    let mut bytes = Vec::with_capacity(size.min(READ_LIMIT + 1) as usize);
    file.take(READ_LIMIT + 1).read_to_end(&mut bytes).map_err(|e| format!("the file could not be read: {e}"))?;
    let read = bytes.len() as u64;
    Ok((bytes, size.max(read)))
}

/// The temporary beside `target`, written and synced.
fn temporary(target: &Path, bytes: &[u8]) -> Result<PathBuf, String> {
    let temporary = lattice_core::fsx::temporary_for(target).map_err(|e| format!("no temporary name: {e}"))?;
    let written = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .and_then(|mut f| f.write_all(bytes).and_then(|()| f.sync_all()));
    match written {
        Ok(()) => Ok(temporary),
        Err(e) => {
            let _ = lattice_core::fsx::remove_own_temporary(&temporary);
            Err(format!("the new bytes could not be written beside the file: {e}"))
        }
    }
}

fn wide(path: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    path.as_os_str().encode_wide().chain(std::iter::once(0)).collect()
}

fn retryable(code: u32) -> bool {
    use windows::Win32::Foundation::{ERROR_ACCESS_DENIED, ERROR_LOCK_VIOLATION, ERROR_SHARING_VIOLATION};
    code == ERROR_SHARING_VIOLATION.0 || code == ERROR_LOCK_VIOLATION.0 || code == ERROR_ACCESS_DENIED.0
}

fn win32_code(e: &windows::core::Error) -> u32 {
    (e.code().0 as u32) & 0xFFFF
}

/// `bytes` over `target` through a temporary and `ReplaceFileW`; the target is unchanged on failure.
fn replace(target: &Path, bytes: &[u8]) -> Result<(), String> {
    use windows::Win32::Storage::FileSystem::{REPLACEFILE_WRITE_THROUGH, ReplaceFileW};
    use windows::core::PCWSTR;
    let temporary = temporary(target, bytes)?;
    let (to, from) = (wide(target), wide(&temporary));
    let mut tries = 0;
    let result = loop {
        // SAFETY: both strings are NUL-terminated and outlive the call; no backup file, no reserved arguments.
        let done = unsafe {
            ReplaceFileW(PCWSTR(to.as_ptr()), PCWSTR(from.as_ptr()), PCWSTR::null(), REPLACEFILE_WRITE_THROUGH, None, None)
        };
        match done {
            Ok(()) => break Ok(()),
            Err(e) if retryable(win32_code(&e)) && tries < RETRIES => {
                tries += 1;
                std::thread::sleep(RETRY_PAUSE);
            }
            Err(e) => break Err(format!("Windows would not replace the file ({e})")),
        }
    };
    if result.is_err() {
        let _ = lattice_core::fsx::remove_own_temporary(&temporary);
    }
    result
}

enum Placed {
    Exists,
    Failed(String),
}

/// `bytes` as the new file `target`: a temporary moved into place without replacing anything.
fn place_new(target: &Path, bytes: &[u8]) -> Result<(), Placed> {
    use windows::Win32::Foundation::ERROR_ALREADY_EXISTS;
    use windows::Win32::Foundation::ERROR_FILE_EXISTS;
    use windows::Win32::Storage::FileSystem::{MOVEFILE_WRITE_THROUGH, MoveFileExW};
    use windows::core::PCWSTR;
    let temporary = temporary(target, bytes).map_err(Placed::Failed)?;
    let (to, from) = (wide(target), wide(&temporary));
    let mut tries = 0;
    let result = loop {
        // SAFETY: both strings are NUL-terminated and outlive the call; without MOVEFILE_REPLACE_EXISTING the move
        // never replaces a file.
        let done = unsafe { MoveFileExW(PCWSTR(from.as_ptr()), PCWSTR(to.as_ptr()), MOVEFILE_WRITE_THROUGH) };
        match done {
            Ok(()) => break Ok(()),
            Err(e) if matches!(win32_code(&e), c if c == ERROR_ALREADY_EXISTS.0 || c == ERROR_FILE_EXISTS.0) => {
                break Err(Placed::Exists);
            }
            Err(e) if retryable(win32_code(&e)) && tries < RETRIES => {
                tries += 1;
                std::thread::sleep(RETRY_PAUSE);
            }
            Err(e) => break Err(Placed::Failed(format!("Windows would not place the file ({e})"))),
        }
    };
    if result.is_err() {
        let _ = lattice_core::fsx::remove_own_temporary(&temporary);
    }
    result
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// A throwaway folder outside AppData (attach refuses AppData, where %TEMP% is): under the system drive's
    /// `tmp\centcom-ide-tests`, removed when dropped.
    pub struct Scratch {
        pub root: PathBuf,
        pub state: StateRoot,
    }

    impl Scratch {
        pub fn new(name: &str) -> Scratch {
            let base = std::env::var_os("SystemDrive")
                .map(|d| PathBuf::from(format!("{}\\", d.to_string_lossy())))
                .unwrap_or_else(|| PathBuf::from("C:\\"))
                .join("tmp")
                .join("centcom-ide-tests");
            let root = base.join(format!("{name}-{}-{}", std::process::id(), crate::utc::now() as u64));
            std::fs::create_dir_all(root.join("work")).unwrap();
            let state = StateRoot::at(root.join("state"));
            Scratch { root, state }
        }

        pub fn work(&self) -> PathBuf {
            self.root.join("work")
        }

        /// An environment with only what attaching needs: no profile, no AppData, so a test folder is never refused
        /// for being one, and no git on the path (the folder is not a repository).
        pub fn env(&self) -> Arc<dyn Env> {
            let system = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
            Arc::new(lattice_core::MapEnv::new().with("SystemRoot", system))
        }

        pub fn folder(&self) -> Folder {
            Folder::attach(&self.work(), self.env(), &self.state).expect("a scratch folder attaches")
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            // The test's own folder, made above.
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn a_file_is_opened_saved_and_its_new_bytes_are_hashed_again() {
        let s = Scratch::new("save");
        std::fs::create_dir_all(s.work().join("src")).unwrap();
        std::fs::write(s.work().join("src").join("a.rs"), b"fn a() {}\r\n").unwrap();
        let folder = s.folder();
        assert!(folder.listing().unwrap().paths.contains(&"src/a.rs".to_string()));
        let opened = folder.open("src/a.rs").unwrap();
        let text = opened.text.clone().unwrap();
        assert_eq!((text.ending, opened.authority), (buffer::Ending::CrLf, false));
        let bytes = buffer::encode("fn a() { b() }\n", text.bom, text.ending);
        let saved = folder.save("src/a.rs", &opened.sha256, &bytes, false).unwrap();
        assert!(!saved.changed_after);
        assert_eq!(std::fs::read(s.work().join("src").join("a.rs")).unwrap(), b"fn a() { b() }\r\n");
        // The next save is checked against the bytes just written.
        assert_eq!(folder.save("src/a.rs", &opened.sha256, b"x", false), Err(SaveError::Changed));
        assert!(folder.save("src/a.rs", &saved.sha256, b"y", false).is_ok());
        // No temporary is left beside it.
        let names: Vec<_> = std::fs::read_dir(s.work().join("src")).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(names.len(), 1, "{names:?}");
    }

    #[test]
    fn a_file_changed_on_disk_since_it_was_opened_is_not_written_over() {
        let s = Scratch::new("changed");
        std::fs::write(s.work().join("notes.txt"), b"one").unwrap();
        let folder = s.folder();
        let opened = folder.open("notes.txt").unwrap();
        std::fs::write(s.work().join("notes.txt"), b"another program's").unwrap();
        assert_eq!(folder.save("notes.txt", &opened.sha256, b"mine", false), Err(SaveError::Changed));
        assert_eq!(std::fs::read(s.work().join("notes.txt")).unwrap(), b"another program's");
        assert!(SaveError::Changed.sentence().contains("changed on disk"));
    }

    #[test]
    fn an_authority_file_is_written_only_once_confirmed() {
        let s = Scratch::new("authority");
        std::fs::create_dir_all(s.work().join(".github").join("workflows")).unwrap();
        std::fs::write(s.work().join(".github").join("workflows").join("ci.yml"), b"on: push\n").unwrap();
        std::fs::write(s.work().join("AGENTS.md"), b"rules\n").unwrap();
        let folder = s.folder();
        for path in [".github/workflows/ci.yml", "AGENTS.md"] {
            let opened = folder.open(path).unwrap();
            assert!(opened.authority, "{path}");
            assert_eq!(folder.save(path, &opened.sha256, b"changed\n", false), Err(SaveError::NeedsConfirm));
            assert_ne!(std::fs::read(s.work().join(path)).unwrap(), b"changed\n");
            assert!(folder.save(path, &opened.sha256, b"changed\n", true).is_ok());
            assert_eq!(std::fs::read(s.work().join(path)).unwrap(), b"changed\n");
        }
    }

    #[test]
    fn paths_the_rules_refuse_are_never_opened_or_written() {
        let s = Scratch::new("refused");
        std::fs::write(s.work().join("ok.txt"), b"ok").unwrap();
        std::fs::write(s.work().join(".env"), b"KEY=secret").unwrap();
        let outside = s.root.join("outside.txt");
        std::fs::write(&outside, b"outside").unwrap();
        let folder = s.folder();
        for path in ["../outside.txt", "C:/Windows/win.ini", ".env", "nul", "OK~1.TXT", ".git/config", "", "a\\b"] {
            assert!(folder.open(path).is_err(), "{path} opened");
            assert!(matches!(folder.save(path, "", b"x", true), Err(SaveError::Refused(_))), "{path} written");
        }
        assert_eq!(std::fs::read(&outside).unwrap(), b"outside");
        assert_eq!(std::fs::read(s.work().join(".env")).unwrap(), b"KEY=secret");
        // A link that leaves the folder is refused before it is followed.
        if std::os::windows::fs::symlink_file(&outside, s.work().join("link.txt")).is_ok() {
            assert!(folder.open("link.txt").is_err());
            assert!(folder.save("link.txt", "", b"x", true).is_err());
            assert_eq!(std::fs::read(&outside).unwrap(), b"outside");
        } else {
            println!("UNMEASURED: a symbolic link could not be made here; the link case did not run");
        }
        // The positive control: the same folder's ordinary file opens.
        assert!(folder.open("ok.txt").is_ok());
    }

    #[test]
    fn a_binary_or_foreign_file_opens_read_only() {
        let s = Scratch::new("binary");
        std::fs::write(s.work().join("a.bin"), b"\x89PNG\0\x01").unwrap();
        std::fs::write(s.work().join("latin.txt"), b"caf\xE9").unwrap();
        let folder = s.folder();
        assert_eq!(folder.open("a.bin").unwrap().text, Err(Undecodable::Binary));
        assert_eq!(folder.open("latin.txt").unwrap().text, Err(Undecodable::NotUtf8));
    }

    #[test]
    fn a_new_file_is_made_and_never_made_over_another() {
        let s = Scratch::new("create");
        std::fs::create_dir_all(s.work().join("src")).unwrap();
        std::fs::write(s.work().join("src").join("there.rs"), b"old").unwrap();
        let folder = s.folder();
        let made = folder.create("src/new.rs", b"fn n() {}\n", false).unwrap();
        assert_eq!((made.path.as_str(), made.size), ("src/new.rs", 10));
        assert_eq!(std::fs::read(s.work().join("src").join("new.rs")).unwrap(), b"fn n() {}\n");
        assert_eq!(folder.create("src/there.rs", b"new", false), Err(SaveError::Exists));
        assert_eq!(std::fs::read(s.work().join("src").join("there.rs")).unwrap(), b"old");
        assert!(matches!(folder.create("nowhere/x.rs", b"x", false), Err(SaveError::Refused(_))));
        assert_eq!(folder.create("run.ps1", b"x", false), Err(SaveError::NeedsConfirm));
        assert!(!s.work().join("run.ps1").exists());
    }
}
