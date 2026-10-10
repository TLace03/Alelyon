//! A program on a Windows pseudoconsole (ConPTY): the person's own terminal in the IDE's bottom panel (a real
//! interactive terminal).
//!
//! The program is started suspended, put in the job object the caller gives (so it and everything it starts end
//! with Alelyon, a crash included), and only then let run. Its console is the pseudoconsole: what it writes arrives
//! as VT on the output pipe (read on a thread of its own, [`read_all`], so the pipe never fills), and what the person
//! types goes in as VT on the input pipe ([`Pty::write`]). Nothing else writes to the input: no model, no tool.
//!
//! A pseudoconsole outlives its program, so a thread waits for the program to end and then closes it: what the
//! program last wrote is flushed and the output pipe ends, which is how the reader learns of the end. Closing it
//! ([`Drop`]) closes the input, then the pseudoconsole on a thread of its own (closing can wait until the output is
//! drained, which the reader keeps doing), and ends the program if it still runs; the caller's job object ends
//! whatever the program started. Nothing here waits on the window's thread.

use std::fs::File;
use std::io::{Read, Write};
use std::os::windows::io::FromRawHandle;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use windows::Win32::Foundation::{CloseHandle, DUPLICATE_SAME_ACCESS, DuplicateHandle, HANDLE, WAIT_OBJECT_0};
use windows::Win32::System::Console::{COORD, ClosePseudoConsole, CreatePseudoConsole, HPCON, ResizePseudoConsole};
use windows::Win32::System::Pipes::CreatePipe;
use windows::Win32::System::Threading::{
    CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT, CreateProcessW, DeleteProcThreadAttributeList,
    EXTENDED_STARTUPINFO_PRESENT, GetCurrentProcess, GetExitCodeProcess, INFINITE, InitializeProcThreadAttributeList,
    LPPROC_THREAD_ATTRIBUTE_LIST, PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE, PROCESS_INFORMATION, ResumeThread,
    STARTF_USESTDHANDLES, STARTUPINFOEXW, TerminateProcess, UpdateProcThreadAttribute, WaitForSingleObject,
};
use windows::core::{PCWSTR, PWSTR};

/// A pseudoconsole's handle: closed once, by whichever comes first of the program's end and the terminal's.
struct Console(HPCON);
// SAFETY: the handle is used by one thread at a time (it is taken out of its slot under a lock to be closed).
unsafe impl Send for Console {}

type Slot = Arc<Mutex<Option<Console>>>;

/// Close the pseudoconsole in `slot`, if it is still open.
fn close(slot: &Slot) {
    let console = slot.lock().unwrap_or_else(|p| p.into_inner()).take();
    if let Some(console) = console {
        // SAFETY: taken out of the slot, so closed exactly once, here.
        unsafe { ClosePseudoConsole(console.0) };
    }
}

/// A handle of the program's process that the waiting thread owns.
struct Waiting(HANDLE);
// SAFETY: owned by the waiting thread alone, and closed once, in Drop.
unsafe impl Send for Waiting {}

impl Drop for Waiting {
    fn drop(&mut self) {
        // SAFETY: this value's own duplicate, closed exactly once.
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

pub struct Pty {
    console: Slot,
    input: Option<File>,
    process: HANDLE,
    thread: HANDLE,
}

// SAFETY: the handles are owned by this value alone and used through `&mut self` or in Drop.
unsafe impl Send for Pty {}

/// A started program: its pseudoconsole, and the pipe its output arrives on.
pub struct Spawned {
    pub pty: Pty,
    pub output: File,
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// A command line's quoting for one argument, as `CommandLineToArgvW` reads it back.
pub fn quote(arg: &str) -> String {
    if !arg.is_empty() && !arg.contains([' ', '\t', '"']) {
        return arg.to_string();
    }
    let mut out = String::from("\"");
    let mut slashes = 0;
    for c in arg.chars() {
        match c {
            '\\' => slashes += 1,
            '"' => {
                out.push_str(&"\\".repeat(slashes * 2 + 1));
                out.push('"');
                slashes = 0;
            }
            _ => {
                out.push_str(&"\\".repeat(slashes));
                slashes = 0;
                out.push(c);
            }
        }
    }
    out.push_str(&"\\".repeat(slashes * 2));
    out.push('"');
    out
}

impl Pty {
    /// Start `program` (an absolute path) with `args` in `cwd`, on a pseudoconsole `cols` by `rows`; `adopt` puts the
    /// suspended process in the caller's job object before it runs (an error there ends it unrun).
    pub fn spawn(
        program: &Path,
        args: &[&str],
        cwd: &Path,
        cols: u16,
        rows: u16,
        adopt: impl FnOnce(HANDLE) -> Result<(), String>,
    ) -> Result<Spawned, String> {
        if !program.is_absolute() {
            return Err("The terminal's program must be named by its full path.".to_string());
        }
        // SAFETY: every handle made here is closed exactly once (on each failure path, or by `Pty`'s Drop); the
        // attribute list's buffer outlives its use and is deleted before it is freed; the strings are NUL-terminated
        // and outlive the calls.
        unsafe {
            let (mut in_read, mut in_write) = (HANDLE::default(), HANDLE::default());
            CreatePipe(&mut in_read, &mut in_write, None, 0).map_err(|e| format!("no input pipe: {e}"))?;
            let (mut out_read, mut out_write) = (HANDLE::default(), HANDLE::default());
            if let Err(e) = CreatePipe(&mut out_read, &mut out_write, None, 0) {
                let _ = CloseHandle(in_read);
                let _ = CloseHandle(in_write);
                return Err(format!("no output pipe: {e}"));
            }
            let size = COORD { X: cols.clamp(1, 500) as i16, Y: rows.clamp(1, 300) as i16 };
            let console = CreatePseudoConsole(size, in_read, out_write, 0);
            // The pseudoconsole holds its own copies of these two ends.
            let _ = CloseHandle(in_read);
            let _ = CloseHandle(out_write);
            let console = match console {
                Ok(c) => c,
                Err(e) => {
                    let _ = CloseHandle(in_write);
                    let _ = CloseHandle(out_read);
                    return Err(format!("Windows would not make a pseudoconsole ({e})."));
                }
            };
            let close_all = |why: String| {
                ClosePseudoConsole(console);
                let _ = CloseHandle(in_write);
                let _ = CloseHandle(out_read);
                why
            };
            let mut bytes = 0usize;
            let _ = InitializeProcThreadAttributeList(None, 1, None, &mut bytes);
            let mut buffer = vec![0u8; bytes.max(64)];
            let list = LPPROC_THREAD_ATTRIBUTE_LIST(buffer.as_mut_ptr().cast());
            if let Err(e) = InitializeProcThreadAttributeList(Some(list), 1, None, &mut bytes) {
                return Err(close_all(format!("no attribute list ({e})")));
            }
            let attached = UpdateProcThreadAttribute(
                list,
                0,
                PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE as usize,
                Some(console.0 as *const core::ffi::c_void),
                std::mem::size_of::<HPCON>(),
                None,
                None,
            );
            if let Err(e) = attached {
                DeleteProcThreadAttributeList(list);
                return Err(close_all(format!("the pseudoconsole could not be given to the program ({e})")));
            }
            let mut info = STARTUPINFOEXW::default();
            info.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
            // No standard handles of this process reach the program: its console is the pseudoconsole alone.
            info.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
            info.lpAttributeList = list;
            let mut line = quote(&program.display().to_string());
            for a in args {
                line.push(' ');
                line.push_str(&quote(a));
            }
            let mut line = wide(&line);
            let cwd = wide(&cwd.display().to_string());
            let mut process = PROCESS_INFORMATION::default();
            let made = CreateProcessW(
                PCWSTR::null(),
                Some(PWSTR(line.as_mut_ptr())),
                None,
                None,
                false,
                EXTENDED_STARTUPINFO_PRESENT | CREATE_UNICODE_ENVIRONMENT | CREATE_SUSPENDED,
                None,
                PCWSTR(cwd.as_ptr()),
                &info.StartupInfo,
                &mut process,
            );
            DeleteProcThreadAttributeList(list);
            drop(buffer);
            if let Err(e) = made {
                return Err(close_all(format!("{} could not be started ({e})", program.display())));
            }
            if let Err(why) = adopt(process.hProcess) {
                let _ = TerminateProcess(process.hProcess, 1);
                let _ = CloseHandle(process.hThread);
                let _ = CloseHandle(process.hProcess);
                return Err(close_all(why));
            }
            ResumeThread(process.hThread);
            let slot: Slot = Arc::new(Mutex::new(Some(Console(console))));
            // When the program ends, its pseudoconsole is closed, so the output pipe ends (on a thread that owns a
            // handle of its own, as the terminal may close its handle first).
            let mut waiting = HANDLE::default();
            let me = GetCurrentProcess();
            if DuplicateHandle(me, process.hProcess, me, &mut waiting, 0, false, DUPLICATE_SAME_ACCESS).is_ok() {
                let waiting = Waiting(waiting);
                let slot = slot.clone();
                // (Inside this block's unsafe: the duplicate is live until `waiting` drops, after the wait.)
                let _ = std::thread::Builder::new().name("terminal-wait".into()).spawn(move || {
                    WaitForSingleObject(waiting.0, INFINITE);
                    close(&slot);
                    drop(waiting);
                });
            }
            let pty = Pty {
                console: slot,
                input: Some(File::from_raw_handle(in_write.0)),
                process: process.hProcess,
                thread: process.hThread,
            };
            Ok(Spawned { pty, output: File::from_raw_handle(out_read.0) })
        }
    }

    /// Send what the person typed.
    pub fn write(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        match &mut self.input {
            Some(input) => input.write_all(bytes),
            None => Err(std::io::Error::new(std::io::ErrorKind::BrokenPipe, "the terminal is closed")),
        }
    }

    pub fn resize(&self, cols: u16, rows: u16) -> Result<(), String> {
        let slot = self.console.lock().unwrap_or_else(|p| p.into_inner());
        let Some(console) = slot.as_ref() else { return Ok(()) };
        let size = COORD { X: cols.clamp(1, 500) as i16, Y: rows.clamp(1, 300) as i16 };
        // SAFETY: the pseudoconsole is open while it is in its slot, and the lock keeps it there for this call.
        unsafe { ResizePseudoConsole(console.0, size) }.map_err(|e| format!("the terminal could not be resized ({e})"))
    }

    /// The program's exit code, once it has ended.
    pub fn exited(&self) -> Option<u32> {
        // SAFETY: the process handle is open until Drop.
        unsafe {
            if WaitForSingleObject(self.process, 0) != WAIT_OBJECT_0 {
                return None;
            }
            let mut code = 0u32;
            GetExitCodeProcess(self.process, &mut code).ok().map(|_| code)
        }
    }
}

impl Drop for Pty {
    fn drop(&mut self) {
        drop(self.input.take());
        if self.console.lock().unwrap_or_else(|p| p.into_inner()).is_some() {
            // Closing can wait for the output to be drained, which the reader thread goes on doing: off this thread.
            let slot = self.console.clone();
            let _ = std::thread::Builder::new().name("terminal-close".into()).spawn(move || close(&slot));
        }
        // SAFETY: the handles are this value's own, closed exactly once, here.
        unsafe {
            if WaitForSingleObject(self.process, 0) != WAIT_OBJECT_0 {
                let _ = TerminateProcess(self.process, 1);
            }
            let _ = CloseHandle(self.thread);
            let _ = CloseHandle(self.process);
        }
    }
}

/// Read everything the program writes, a piece at a time, until its pseudoconsole closes; `piece` is called with
/// each piece and stops the reading by returning false.
pub fn read_all(mut output: File, mut piece: impl FnMut(&[u8]) -> bool) {
    let mut buffer = vec![0u8; 16 * 1024];
    loop {
        match output.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                if !piece(&buffer[..n]) {
                    break;
                }
            }
        }
    }
}

/// The shell the terminal starts: PowerShell 7 where its installer puts it, else Windows PowerShell 5.1 under
/// `%SystemRoot%`; a name is never looked up on `PATH` or in the folder.
pub fn shell(env: impl Fn(&str) -> Option<String>) -> Option<(PathBuf, Vec<&'static str>)> {
    let program_files = env("ProgramFiles").unwrap_or_else(|| r"C:\Program Files".to_string());
    let pwsh = PathBuf::from(program_files).join("PowerShell").join("7").join("pwsh.exe");
    if pwsh.is_file() {
        return Some((pwsh, vec!["-NoLogo"]));
    }
    let root = env("SystemRoot").unwrap_or_else(|| r"C:\Windows".to_string());
    let windows_powershell =
        PathBuf::from(root).join("System32").join("WindowsPowerShell").join("v1.0").join("powershell.exe");
    windows_powershell.is_file().then(|| (windows_powershell, vec!["-NoLogo"]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    fn system32(name: &str) -> PathBuf {
        PathBuf::from(std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into())).join("System32").join(name)
    }

    /// Run `program args` on a pseudoconsole, read what it writes until it closes or `seconds` pass, and hand back
    /// the screen's text.
    fn run(program: &Path, args: &[&str], input: Option<&[u8]>, seconds: u64) -> (String, Option<u32>) {
        let spawned = Pty::spawn(program, args, &std::env::temp_dir(), 80, 24, |_| Ok(())).expect("it starts");
        let (tx, rx) = mpsc::channel::<Vec<u8>>();
        let output = spawned.output;
        let reader = std::thread::spawn(move || read_all(output, |b| tx.send(b.to_vec()).is_ok()));
        let mut pty = spawned.pty;
        if let Some(bytes) = input {
            pty.write(bytes).unwrap();
        }
        let mut screen = super::super::vt::Screen::new(80, 24);
        let deadline = std::time::Instant::now() + Duration::from_secs(seconds);
        while std::time::Instant::now() < deadline {
            match rx.recv_timeout(Duration::from_millis(100)) {
                Ok(bytes) => {
                    screen.feed(&bytes);
                    let replies = screen.take_replies();
                    if !replies.is_empty() {
                        let _ = pty.write(&replies);
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if pty.exited().is_some() && screen.text().len() > 0 {
                        break;
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        let code = pty.exited();
        drop(pty);
        let _ = reader.join();
        (screen.text(), code)
    }

    #[test]
    fn a_program_on_the_pseudoconsole_writes_to_the_screen_and_ends() {
        let (text, code) = run(&system32("cmd.exe"), &["/d", "/c", "echo", "hello from conpty"], None, 10);
        assert!(text.contains("hello from conpty"), "{text:?}");
        assert_eq!(code, Some(0));
    }

    #[test]
    fn what_is_typed_reaches_the_program() {
        let (text, _) = run(&system32("cmd.exe"), &["/d", "/q", "/k", "prompt $G"], Some(b"echo typed-%COMPUTERNAME:~0,0%here\rexit\r"), 15);
        assert!(text.contains("typed-here"), "{text:?}");
    }

    #[test]
    fn the_shell_is_found_by_its_full_path_only() {
        let found = shell(|name| std::env::var(name).ok()).expect("a PowerShell on this PC");
        assert!(found.0.is_absolute() && found.0.is_file(), "{found:?}");
        assert!(shell(|_| Some(r"Z:\nowhere".into())).is_none());
        assert!(Pty::spawn(Path::new("cmd.exe"), &[], Path::new("."), 80, 24, |_| Ok(())).is_err(), "a bare name is refused");
    }

    #[test]
    fn arguments_are_quoted_as_windows_reads_them_back() {
        assert_eq!(quote("plain"), "plain");
        assert_eq!(quote("two words"), "\"two words\"");
        assert_eq!(quote(r"C:\Program Files\x\"), r#""C:\Program Files\x\\""#);
        assert_eq!(quote(r#"say "hi""#), r#""say \"hi\"""#);
        assert_eq!(quote(""), "\"\"");
    }

    #[test]
    fn a_refused_adoption_ends_the_program_unrun() {
        let refused = Pty::spawn(&system32("cmd.exe"), &["/d", "/c", "echo", "never"], &std::env::temp_dir(), 80, 24, |_| Err("no job".into()));
        assert_eq!(refused.err().as_deref(), Some("no job"));
    }
}
