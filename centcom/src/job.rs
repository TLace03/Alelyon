//! A Windows job object that kills what it holds when its last handle closes: when Alelyon (CENTCOM) exits for
//! any reason, a crash included, what it started goes with it: the speech engine and its recogniser, the issuer.

use std::os::windows::io::AsRawHandle;

use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_BASIC_ACCOUNTING_INFORMATION, JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectBasicAccountingInformation,
    JobObjectExtendedLimitInformation, QueryInformationJobObject, SetInformationJobObject,
};
use windows::core::PCWSTR;

pub struct Job(HANDLE);

// SAFETY: the handle is only used for the calls below and closed once, in Drop.
unsafe impl Send for Job {}

impl Job {
    pub fn new() -> Result<Job, String> {
        // SAFETY: a fresh unnamed job; the information struct outlives the call.
        unsafe {
            let job = CreateJobObjectW(None, PCWSTR::null()).map_err(|e| format!("no job object: {e}"))?;
            let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let set = SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const core::ffi::c_void,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );
            if let Err(e) = set {
                let _ = CloseHandle(job);
                return Err(format!("the job object refused its limit: {e}"));
            }
            Ok(Job(job))
        }
    }

    /// Tie `child` (`what` names it in an error) to this job.
    pub fn adopt(&self, child: &std::process::Child, what: &str) -> Result<(), String> {
        // SAFETY: both handles are live: the job made above, the child just started.
        unsafe { AssignProcessToJobObject(self.0, HANDLE(child.as_raw_handle())) }
            .map_err(|e| format!("could not tie {what} to Alelyon: {e}"))
    }

    /// Tie the process `process` (a live handle the caller owns: the terminal's shell, started suspended) to this job.
    pub fn adopt_handle(&self, process: HANDLE, what: &str) -> Result<(), String> {
        // SAFETY: both handles are live: the job made above, the process the caller holds.
        unsafe { AssignProcessToJobObject(self.0, process) }.map_err(|e| format!("could not tie {what} to Alelyon: {e}"))
    }

    /// How many processes in the job are running now (a terminal's shell, and what it started).
    pub fn running(&self) -> Option<u32> {
        let mut info = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
        // SAFETY: the job is live; the struct outlives the call and its size is the one passed.
        unsafe {
            QueryInformationJobObject(
                Some(self.0),
                JobObjectBasicAccountingInformation,
                &mut info as *mut _ as *mut core::ffi::c_void,
                std::mem::size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
                None,
            )
        }
        .ok()
        .map(|_| info.ActiveProcesses)
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        // SAFETY: closed exactly once, here.
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}
