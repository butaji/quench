//! OS signal resources and ordered notifications for the Node host.
// Delivery is implemented on Linux; other platforms preserve the previous path.
#[cfg(target_os = "linux")]
mod platform {
    use std::collections::{BTreeMap, HashMap, VecDeque};
    use std::io::{self, Read};
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
    use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, AtomicUsize, Ordering};
    use std::sync::Mutex;

    pub(crate) const SIGNALS: &[(&str, libc::c_int)] = &[
        ("SIGHUP", libc::SIGHUP),
        ("SIGINT", libc::SIGINT),
        ("SIGQUIT", libc::SIGQUIT),
        ("SIGILL", libc::SIGILL),
        ("SIGTRAP", libc::SIGTRAP),
        ("SIGABRT", libc::SIGABRT),
        ("SIGIOT", libc::SIGIOT),
        ("SIGBUS", libc::SIGBUS),
        ("SIGFPE", libc::SIGFPE),
        ("SIGKILL", libc::SIGKILL),
        ("SIGUSR1", libc::SIGUSR1),
        ("SIGSEGV", libc::SIGSEGV),
        ("SIGUSR2", libc::SIGUSR2),
        ("SIGPIPE", libc::SIGPIPE),
        ("SIGALRM", libc::SIGALRM),
        ("SIGTERM", libc::SIGTERM),
        ("SIGCHLD", libc::SIGCHLD),
        ("SIGSTKFLT", libc::SIGSTKFLT),
        ("SIGCONT", libc::SIGCONT),
        ("SIGSTOP", libc::SIGSTOP),
        ("SIGTSTP", libc::SIGTSTP),
        ("SIGTTIN", libc::SIGTTIN),
        ("SIGTTOU", libc::SIGTTOU),
        ("SIGURG", libc::SIGURG),
        ("SIGXCPU", libc::SIGXCPU),
        ("SIGXFSZ", libc::SIGXFSZ),
        ("SIGVTALRM", libc::SIGVTALRM),
        ("SIGPROF", libc::SIGPROF),
        ("SIGWINCH", libc::SIGWINCH),
        ("SIGIO", libc::SIGIO),
        ("SIGPOLL", libc::SIGPOLL),
        ("SIGPWR", libc::SIGPWR),
        ("SIGSYS", libc::SIGSYS),
    ];
    // Apply only at an executable's process boundary, before starting VM workers.
    // Node ResetSignalHandlers resets inherited ignores for standard POSIX signals;
    // genuine installed handlers survive. SIGPIPE and SIGXFSZ are ignored by policy.
    pub(crate) fn reset_cli_dispositions() -> std::io::Result<()> {
        for (index, (_, signal)) in SIGNALS.iter().enumerate() {
            if matches!(*signal, libc::SIGKILL | libc::SIGSTOP)
                || SIGNALS[..index].iter().any(|(_, prior)| prior == signal)
            {
                continue;
            }
            let ignore = matches!(*signal, libc::SIGPIPE | libc::SIGXFSZ);
            let mut previous: libc::sigaction = unsafe { std::mem::zeroed() };
            if unsafe { libc::sigaction(*signal, std::ptr::null(), &mut previous) } != 0 {
                return Err(std::io::Error::last_os_error());
            }
            if !ignore
                && (previous.sa_flags & libc::SA_SIGINFO != 0
                    || previous.sa_sigaction != libc::SIG_IGN)
            {
                continue;
            }
            let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
            unsafe {
                libc::sigemptyset(&mut action.sa_mask);
            }
            action.sa_sigaction = if ignore { libc::SIG_IGN } else { libc::SIG_DFL };
            if unsafe { libc::sigaction(*signal, &action, std::ptr::null_mut()) } != 0 {
                return Err(std::io::Error::last_os_error());
            }
        }
        Ok(())
    }

    const INVALID_FD: RawFd = -1;
    static WRITE_FD: AtomicI32 = AtomicI32::new(INVALID_FD);
    static ACTIVE_HANDLERS: AtomicUsize = AtomicUsize::new(0);
    // Standard signals may coalesce. If the OS pipe fills, preserve a pending
    // notification for each physical signal rather than blocking its handler.
    static OVERFLOW: [AtomicU64; SIGNALS.len()] = [const { AtomicU64::new(0) }; SIGNALS.len()];
    static SEQUENCE: AtomicU64 = AtomicU64::new(1);
    static SEQUENCE_EXHAUSTED: AtomicBool = AtomicBool::new(false);
    const SIGNAL_WIDTH: usize = std::mem::size_of::<libc::c_int>();
    const RECORD_WIDTH: usize = SIGNAL_WIDTH + std::mem::size_of::<u64>();
    const _: () = assert!(RECORD_WIDTH <= libc::PIPE_BUF);
    fn next_sequence() -> io::Result<u64> {
        SEQUENCE
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |value| {
                value.checked_add(1)
            })
            .map_err(|_| io::Error::other("signal arrival sequence exhausted"))
    }
    static HUB: Mutex<Option<Hub>> = Mutex::new(None);

    pub(crate) fn signal_number(name: &str) -> Option<libc::c_int> {
        SIGNALS
            .iter()
            .find_map(|(candidate, number)| (*candidate == name).then_some(*number))
    }

    extern "C" fn receive(signal: libc::c_int) {
        ACTIVE_HANDLERS.fetch_add(1, Ordering::AcqRel);
        // No allocation, lock, VM access, or guest callback occurs in this handler.
        unsafe {
            let errno = libc::__errno_location();
            let saved_errno = *errno;
            let fd = WRITE_FD.load(Ordering::Acquire);
            if fd != INVALID_FD {
                let sequence = SEQUENCE.try_update(Ordering::SeqCst, Ordering::SeqCst, |value| {
                    value.checked_add(1)
                });
                if let Ok(sequence) = sequence {
                    let mut record = [0; RECORD_WIDTH];
                    record[..SIGNAL_WIDTH].copy_from_slice(&signal.to_ne_bytes());
                    record[SIGNAL_WIDTH..].copy_from_slice(&sequence.to_ne_bytes());
                    loop {
                        let written = libc::write(fd, record.as_ptr().cast(), record.len());
                        if written == record.len() as isize {
                            break;
                        }
                        if written < 0 && *errno == libc::EINTR {
                            continue;
                        }
                        if let Some(index) =
                            SIGNALS.iter().position(|(_, number)| *number == signal)
                        {
                            OVERFLOW[index].fetch_max(sequence, Ordering::Relaxed);
                        }
                        break;
                    }
                } else {
                    SEQUENCE_EXHAUSTED.store(true, Ordering::Release);
                }
            }
            *errno = saved_errno;
        }
        ACTIVE_HANDLERS.fetch_sub(1, Ordering::Release);
    }

    struct Subscriber {
        names: Vec<(String, libc::c_int, u64)>,
        pending: VecDeque<String>,
    }
    struct Hub {
        reader: std::fs::File,
        _writer: OwnedFd,
        previous: HashMap<libc::c_int, libc::sigaction>,
        subscribers: BTreeMap<u64, Subscriber>,
        next_id: u64,
    }
    impl Hub {
        fn new() -> io::Result<Self> {
            let mut fds = [INVALID_FD; 2];
            if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC | libc::O_NONBLOCK) } != 0 {
                return Err(io::Error::last_os_error());
            }
            let reader = unsafe { std::fs::File::from_raw_fd(fds[0]) };
            let writer = unsafe { OwnedFd::from_raw_fd(fds[1]) };
            for pending in &OVERFLOW {
                pending.store(0, Ordering::Relaxed);
            }
            WRITE_FD.store(writer.as_raw_fd(), Ordering::Release);
            Ok(Self {
                reader,
                _writer: writer,
                previous: HashMap::new(),
                subscribers: BTreeMap::new(),
                next_id: 1,
            })
        }
        fn subscribe(&mut self) -> io::Result<u64> {
            let id = self.next_id;
            self.next_id = id
                .checked_add(1)
                .ok_or_else(|| io::Error::other("signal subscriber identifiers exhausted"))?;
            self.subscribers.insert(
                id,
                Subscriber {
                    names: Vec::new(),
                    pending: VecDeque::new(),
                },
            );
            Ok(id)
        }
        fn watch(&mut self, id: u64, name: &str, signal: libc::c_int) -> io::Result<()> {
            if self.subscribers[&id]
                .names
                .iter()
                .any(|(candidate, _, _)| candidate == name)
            {
                return Ok(());
            }
            let sequence = next_sequence()?;
            if !self.previous.contains_key(&signal) {
                let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
                let mut previous: libc::sigaction = unsafe { std::mem::zeroed() };
                action.sa_sigaction = receive as *const () as usize;
                action.sa_flags = libc::SA_RESTART;
                unsafe {
                    libc::sigemptyset(&mut action.sa_mask);
                }
                if unsafe { libc::sigaction(signal, &action, &mut previous) } != 0 {
                    return Err(io::Error::last_os_error());
                }
                self.previous.insert(signal, previous);
            }
            self.subscribers
                .get_mut(&id)
                .unwrap()
                .names
                .push((name.to_owned(), signal, sequence));
            Ok(())
        }
        fn restore_unused(&mut self) {
            let unused = self
                .previous
                .keys()
                .copied()
                .filter(|signal| {
                    !self.subscribers.values().any(|subscriber| {
                        subscriber
                            .names
                            .iter()
                            .any(|(_, number, _)| number == signal)
                    })
                })
                .collect::<Vec<_>>();
            for signal in unused {
                if let Some(previous) = self.previous.remove(&signal) {
                    unsafe {
                        libc::sigaction(signal, &previous, std::ptr::null_mut());
                    }
                }
            }
        }
        fn dispatch(&mut self, signal: libc::c_int, sequence: u64) {
            for subscriber in self.subscribers.values_mut() {
                for (name, number, start) in &subscriber.names {
                    if *number == signal && sequence >= *start {
                        subscriber.pending.push_back(name.clone());
                    }
                }
            }
        }
        fn poll(&mut self) -> io::Result<()> {
            if SEQUENCE_EXHAUSTED.load(Ordering::Acquire) {
                return Err(io::Error::other("signal arrival sequence exhausted"));
            }
            let mut record = [0; RECORD_WIDTH];
            loop {
                match self.reader.read(&mut record) {
                    Ok(length) if length == record.len() => {
                        let signal =
                            libc::c_int::from_ne_bytes(record[..SIGNAL_WIDTH].try_into().unwrap());
                        let sequence =
                            u64::from_ne_bytes(record[SIGNAL_WIDTH..].try_into().unwrap());
                        self.dispatch(signal, sequence);
                    }
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                    Err(error) => return Err(error),
                    _ => return Err(io::Error::other("incomplete internal signal record")),
                }
            }
            for (index, pending) in OVERFLOW.iter().enumerate() {
                let sequence = pending.swap(0, Ordering::Acquire);
                if sequence != 0 {
                    self.dispatch(SIGNALS[index].1, sequence);
                }
            }
            Ok(())
        }
    }
    impl Drop for Hub {
        fn drop(&mut self) {
            for (signal, previous) in self.previous.drain() {
                unsafe {
                    libc::sigaction(signal, &previous, std::ptr::null_mut());
                }
            }
            WRITE_FD.store(INVALID_FD, Ordering::Release);
            while ACTIVE_HANDLERS.load(Ordering::Acquire) != 0 {
                std::thread::yield_now();
            }
            // Owned pipe endpoints close after handlers can no longer use them.
        }
    }

    #[derive(Default)]
    pub(crate) struct SignalState {
        id: Option<u64>,
    }
    impl SignalState {
        pub(crate) fn watch(&mut self, name: &str) -> io::Result<()> {
            let Some(signal) = signal_number(name) else {
                return Ok(());
            };
            let mut guard = HUB
                .lock()
                .map_err(|_| io::Error::other("signal registry poisoned"))?;
            if guard.is_none() {
                *guard = Some(Hub::new()?);
            }
            let hub = guard.as_mut().unwrap();
            let id = match self.id {
                Some(id) => id,
                None => {
                    let id = hub.subscribe()?;
                    self.id = Some(id);
                    id
                }
            };
            hub.watch(id, name, signal)
        }
        pub(crate) fn unwatch(&mut self, name: Option<&str>) {
            let Some(id) = self.id else {
                return;
            };
            if let Ok(mut guard) = HUB.lock() {
                if let Some(hub) = guard.as_mut() {
                    if let Some(subscriber) = hub.subscribers.get_mut(&id) {
                        subscriber
                            .names
                            .retain(|(candidate, _, _)| name.is_some_and(|name| candidate != name));
                        subscriber
                            .pending
                            .retain(|candidate| name.is_some_and(|name| candidate != name));
                    }
                    hub.restore_unused();
                }
            }
        }
        pub(crate) fn take_pending(&mut self) -> io::Result<Vec<String>> {
            let Some(id) = self.id else {
                return Ok(Vec::new());
            };
            let mut guard = HUB
                .lock()
                .map_err(|_| io::Error::other("signal registry poisoned"))?;
            let Some(hub) = guard.as_mut() else {
                return Ok(Vec::new());
            };
            hub.poll()?;
            Ok(hub
                .subscribers
                .get_mut(&id)
                .unwrap()
                .pending
                .drain(..)
                .collect())
        }
    }
    impl Drop for SignalState {
        fn drop(&mut self) {
            let Some(id) = self.id else {
                return;
            };
            if let Ok(mut guard) = HUB.lock() {
                if let Some(hub) = guard.as_mut() {
                    hub.subscribers.remove(&id);
                    hub.restore_unused();
                    if hub.subscribers.is_empty() {
                        *guard = None;
                    }
                }
            }
        }
    }
}

#[cfg(not(target_os = "linux"))]
mod platform {
    #[derive(Default)]
    pub(crate) struct SignalState;
    impl SignalState {
        pub(crate) fn watch(&mut self, _: &str) -> std::io::Result<()> {
            Ok(())
        }
        pub(crate) fn unwatch(&mut self, _: Option<&str>) {}
        pub(crate) fn take_pending(&mut self) -> std::io::Result<Vec<String>> {
            Ok(Vec::new())
        }
    }
}

pub(crate) use platform::SignalState;
#[cfg(target_os = "linux")]
pub(crate) use platform::SIGNALS;

/// The VM runs on a stack-owning worker. Keep process-directed signals from
/// being delivered to its waiting caller, while preserving the caller's mask
/// in the execution thread and restoring it when execution finishes.
#[cfg(target_os = "linux")]
pub(crate) struct ParentSignalMask(libc::sigset_t);
#[cfg(target_os = "linux")]
pub(crate) struct WorkerSignalMask(libc::sigset_t);
#[cfg(target_os = "linux")]
impl ParentSignalMask {
    pub(crate) fn block_for_worker() -> std::io::Result<(Self, WorkerSignalMask)> {
        let mut set: libc::sigset_t = unsafe { std::mem::zeroed() };
        let mut previous: libc::sigset_t = unsafe { std::mem::zeroed() };
        unsafe {
            libc::sigemptyset(&mut set);
        }
        for (_, signal) in SIGNALS {
            unsafe {
                libc::sigaddset(&mut set, *signal);
            }
        }
        let code = unsafe { libc::pthread_sigmask(libc::SIG_BLOCK, &set, &mut previous) };
        if code != 0 {
            return Err(std::io::Error::from_raw_os_error(code));
        }
        Ok((Self(previous), WorkerSignalMask(previous)))
    }
}
#[cfg(target_os = "linux")]
impl WorkerSignalMask {
    pub(crate) fn restore(self) -> std::io::Result<()> {
        let code =
            unsafe { libc::pthread_sigmask(libc::SIG_SETMASK, &self.0, std::ptr::null_mut()) };
        if code == 0 {
            Ok(())
        } else {
            Err(std::io::Error::from_raw_os_error(code))
        }
    }
}
#[cfg(target_os = "linux")]
impl Drop for ParentSignalMask {
    fn drop(&mut self) {
        unsafe {
            libc::pthread_sigmask(libc::SIG_SETMASK, &self.0, std::ptr::null_mut());
        }
    }
}

#[cfg(target_os = "linux")]
pub(crate) use platform::reset_cli_dispositions;
