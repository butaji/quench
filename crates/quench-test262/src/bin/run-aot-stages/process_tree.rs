use serde_json::{Value, json};
use std::{collections::BTreeSet, sync::{Arc, atomic::{AtomicBool, Ordering}}, thread, time::{Duration, Instant}};

/// Memory sampling runs separately from unobserved timing. A sampled resident
/// sum is a lower bound on the instantaneous process-tree peak, not an exact
/// high-water mark. The time wrapper and this observer are excluded.
const SAMPLE_INTERVAL: Duration = Duration::from_millis(1);

pub struct Monitor {
    stop: Arc<AtomicBool>,
    worker: thread::JoinHandle<Value>,
}
impl Monitor {
    pub fn start(wrapper: u32) -> Result<Self,String> {
        let stop = Arc::new(AtomicBool::new(false));
        let requested = stop.clone();
        let worker = thread::Builder::new().name("aot-memory-observer".into()).spawn(move || {
            let mut samples = 0u64;
            let mut observed_samples = 0u64;
            let mut peak = 0u64;
            let mut peak_processes = 0usize;
            let mut unreadable = 0u64;
            let mut maximum_gap = Duration::ZERO;
            let mut previous = Instant::now();
            while !requested.load(Ordering::Relaxed) {
                let now = Instant::now();
                maximum_gap = maximum_gap.max(now.duration_since(previous));
                previous = now;
                let mut pending = vec![wrapper];
                let mut seen = BTreeSet::new();
                let mut resident = 0u64;
                let mut processes = 0usize;
                while let Some(parent) = pending.pop() {
                    match platform::children(parent) {
                        Ok(children) => for child in children {
                            if !seen.insert(child) { continue; }
                            match platform::resident(child,parent) {
                                Some(bytes) => { resident = resident.saturating_add(bytes); processes += 1; pending.push(child); },
                                None => unreadable += 1,
                            }
                        },
                        Err(_) => unreadable += 1,
                    }
                }
                samples += 1;
                if processes > 0 { observed_samples += 1; }
                peak = peak.max(resident);
                peak_processes = peak_processes.max(processes);
                thread::sleep(SAMPLE_INTERVAL);
            }
            json!({"sampled_peak_resident_bytes":peak,"maximum_observed_processes":peak_processes,"samples":samples,
                "observed_samples":observed_samples,"unreadable_process_observations":unreadable,
                "requested_interval_nanoseconds":SAMPLE_INTERVAL.as_nanos(),"maximum_sample_gap_nanoseconds":maximum_gap.as_nanos(),
                "scope":"Sampled resident-byte sum of descendants of the time wrapper; wrapper/observer excluded. Lower bound; short-lived allocations or children between samples can be missed. RSS shared pages may be counted in multiple processes."})
        }).map_err(|error|error.to_string())?;
        Ok(Self { stop, worker })
    }
    pub fn finish(self) -> Result<Value,String> {
        self.stop.store(true,Ordering::Relaxed);
        self.worker.join().map_err(|_|"memory observer panicked".into())
    }
}

#[cfg(target_os = "macos")]
mod platform {
    pub fn children(pid:u32)->Result<Vec<u32>,String> {
        // libproc returns PID counts; its buffer argument is sized in bytes.
        let count = unsafe { libc::proc_listchildpids(pid as i32,std::ptr::null_mut(),0) };
        if count < 0 { return Err(std::io::Error::last_os_error().to_string()); }
        let mut pids = vec![0i32;usize::try_from(count).map_err(|e|e.to_string())? + 1];
        let bytes = i32::try_from(std::mem::size_of_val(pids.as_slice())).map_err(|e|e.to_string())?;
        let written = unsafe { libc::proc_listchildpids(pid as i32,pids.as_mut_ptr().cast(),bytes) };
        if written < 0 || written as usize >= pids.len() { return Err("child list changed beyond sampled buffer".into()); }
        pids.truncate(written as usize);
        Ok(pids.into_iter().filter_map(|pid|u32::try_from(pid).ok()).filter(|pid|*pid>0).collect())
    }
    fn info<T>(pid:u32,flavor:i32)->Option<T> {
        let mut info = std::mem::MaybeUninit::<T>::uninit();
        let size = i32::try_from(std::mem::size_of::<T>()).ok()?;
        // SAFETY: the fixed libproc flavor writes exactly the matching C struct;
        // all callers below pair the flavor and type from libc's declarations.
        let written = unsafe { libc::proc_pidinfo(pid as i32,flavor,0,info.as_mut_ptr().cast(),size) };
        (written == size).then(||unsafe { info.assume_init() })
    }
    pub fn resident(pid:u32,parent:u32)->Option<u64> {
        let before:libc::proc_bsdinfo = info(pid,libc::PROC_PIDTBSDINFO)?;
        if before.pbi_ppid != parent { return None; }
        let task:libc::proc_taskinfo = info(pid,libc::PROC_PIDTASKINFO)?;
        let after:libc::proc_bsdinfo = info(pid,libc::PROC_PIDTBSDINFO)?;
        if (before.pbi_pid,before.pbi_ppid,before.pbi_start_tvsec,before.pbi_start_tvusec)
            != (after.pbi_pid,after.pbi_ppid,after.pbi_start_tvsec,after.pbi_start_tvusec) { return None; }
        Some(task.pti_resident_size)
    }
}

#[cfg(target_os = "linux")]
mod platform {
    const STAT_PARENT_COLUMN:usize = 4 - 3;
    const STAT_START_COLUMN:usize = 22 - 3;
    const STATM_RESIDENT_COLUMN:usize = 1;
    pub fn children(pid:u32)->Result<Vec<u32>,String> {
        let mut children=std::collections::BTreeSet::new();
        for task in std::fs::read_dir(format!("/proc/{pid}/task")).map_err(|e|e.to_string())? {
            let task=task.map_err(|e|e.to_string())?;
            let Ok(text)=std::fs::read_to_string(task.path().join("children")) else { continue; };
            children.extend(text.split_whitespace().filter_map(|pid|pid.parse::<u32>().ok()));
        }
        Ok(children.into_iter().collect())
    }
    fn identity(pid:u32)->Option<(u32,u64)> {
        let text=std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        let fields=text.rsplit_once(')')?.1.split_whitespace().collect::<Vec<_>>();
        Some((fields.get(STAT_PARENT_COLUMN)?.parse().ok()?,fields.get(STAT_START_COLUMN)?.parse().ok()?))
    }
    pub fn resident(pid:u32,parent:u32)->Option<u64> {
        let before=identity(pid)?;
        if before.0 != parent { return None; }
        let text=std::fs::read_to_string(format!("/proc/{pid}/statm")).ok()?;
        let pages=text.split_whitespace().nth(STATM_RESIDENT_COLUMN)?.parse::<u64>().ok()?;
        let page_bytes=u64::try_from(unsafe { libc::sysconf(libc::_SC_PAGESIZE) }).ok()?;
        (identity(pid)?==before).then(||pages.checked_mul(page_bytes)).flatten()
    }
}

#[cfg(not(any(target_os = "macos",target_os = "linux")))]
mod platform {
    pub fn children(_:u32)->Result<Vec<u32>,String> { Err("unsupported process observation platform".into()) }
    pub fn resident(_:u32,_:u32)->Option<u64> { None }
}
