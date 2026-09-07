//! Windows process lifetime binding for the optional local preview worker.
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
};
use windows::Win32::{
    Foundation::HANDLE,
    System::{
        JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
            SetInformationJobObject, TerminateJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        },
        Threading::WaitForSingleObject,
    },
};
static STOPPING: AtomicBool = AtomicBool::new(false);
struct Active {
    job: usize,
    child: usize,
    directory: PathBuf,
}
static ACTIVE: Mutex<Option<Active>> = Mutex::new(None);
pub struct Lifetime(OwnedHandle);
impl Lifetime {
    pub fn attach(child: &std::process::Child, directory: &Path) -> Result<Self, String> {
        unsafe {
            let raw = CreateJobObjectW(None, None).map_err(|e| e.to_string())?;
            let owned = OwnedHandle::from_raw_handle(raw.0);
            let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            SetInformationJobObject(
                raw,
                JobObjectExtendedLimitInformation,
                &limits as *const _ as *const _,
                std::mem::size_of_val(&limits) as u32,
            )
            .map_err(|e| e.to_string())?;
            AssignProcessToJobObject(raw, HANDLE(child.as_raw_handle()))
                .map_err(|e| e.to_string())?;
            let mut active = ACTIVE.lock().unwrap_or_else(|e| e.into_inner());
            if STOPPING.load(Ordering::Acquire) {
                return Err("app is shutting down".into());
            }
            *active = Some(Active {
                job: raw.0 as usize,
                child: child.as_raw_handle() as usize,
                directory: directory.to_path_buf(),
            });
            Ok(Self(owned))
        }
    }
}
impl Drop for Lifetime {
    fn drop(&mut self) {
        let mut active = ACTIVE.lock().unwrap_or_else(|e| e.into_inner());
        if active
            .as_ref()
            .is_some_and(|a| a.job == self.0.as_raw_handle() as usize)
        {
            *active = None;
        }
        // OwnedHandle closes the kill-on-close job after unregistering it.
    }
}
pub fn shutdown() {
    STOPPING.store(true, Ordering::Release);
    let active = ACTIVE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(a) = active.as_ref() {
        unsafe {
            let _ = TerminateJobObject(HANDLE(a.job as *mut _), 1);
            let _ = WaitForSingleObject(HANDLE(a.child as *mut _), 5000);
        }
        let _ = std::fs::remove_dir_all(&a.directory);
    }
}
