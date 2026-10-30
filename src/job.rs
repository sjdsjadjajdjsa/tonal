//! Windows has no parent-death signal, so every helper process Tonal starts (mpv,
//! yt-dlp) joins one Job Object that kills its members when the last handle to it
//! closes. The handle is never closed by hand: Windows closes it when Tonal exits,
//! however that happens, and takes the children with it. Processes those helpers
//! start themselves (yt-dlp under mpv) join the job automatically.

use std::os::windows::io::RawHandle;
use std::sync::OnceLock;
use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    SetInformationJobObject,
};
use windows_core::PCWSTR;

/// The job handle as an address, since HANDLE is not Sync. Lives for the whole app.
static JOB: OnceLock<Option<usize>> = OnceLock::new();

fn job() -> Option<HANDLE> {
    let handle = JOB.get_or_init(|| unsafe {
        let job = CreateJobObjectW(None, PCWSTR::null())
            .inspect_err(|e| log::warn!("could not create a job object: {e}"))
            .ok()?;
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        if let Err(e) = SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            (&raw const limits).cast(),
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        ) {
            log::warn!("could not configure the job object: {e}");
            return None;
        }
        Some(job.0 as usize)
    });
    handle.map(|h| HANDLE(h as *mut _))
}

/// Ties a freshly spawned child to Tonal's lifetime. Failures only log.
pub fn adopt(process: Option<RawHandle>) {
    let (Some(job), Some(process)) = (job(), process) else {
        return;
    };
    if let Err(e) = unsafe { AssignProcessToJobObject(job, HANDLE(process)) } {
        log::warn!("could not add a child process to the job object: {e}");
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn child_joins_the_job() {
        let mut child = std::process::Command::new("cmd")
            .args(["/C", "exit 0"])
            .spawn()
            .expect("cmd starts");
        use std::os::windows::io::AsRawHandle;
        super::adopt(Some(child.as_raw_handle()));
        assert!(super::job().is_some());
        let _ = child.wait();
    }
}
