//! A thread whose stack sits at one fixed address in every run of one binary (`run_fixed`).
//! The C library places a thread's control block and static thread-local storage at the top of
//! a stack the caller supplies, so every thread-local the runtime keeps on that thread has the
//! same address in each run: a state saved from one run that names those addresses (`dump`)
//! stays valid in the next.

/// The stack's address: below the dump arena (`dump::DUMP_BASE`), above a no-PIE executable.
pub const STACK_BASE: usize = 0x0e00_0000_0000;
/// The guard below the stack, never mapped readable, so an overflow faults.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
const GUARD_BYTES: usize = 64 << 10;

#[cfg(target_os = "linux")]
const MAP_FIXED_NOREPLACE: libc::c_int = 0x100000;

/// Runs `body` on a new thread named `name` whose `stack_bytes` stack lies at the fixed
/// address, and waits for it; `body` comes back unrun where that range cannot be had or the
/// thread cannot start, so the caller can run it on a thread of its own. Only one such
/// thread runs at a time; a second concurrent call gets `body` back. `Ok(false)`: `body`
/// panicked (its hook reported it).
pub fn run_fixed(name: &std::ffi::CStr, stack_bytes: usize, body: Box<dyn FnOnce() + Send>) -> Result<bool, Box<dyn FnOnce() + Send>> {
    #[cfg(target_os = "linux")]
    {
        let length = GUARD_BYTES + stack_bytes.next_multiple_of(4096);
        // SAFETY: `MAP_FIXED_NOREPLACE` maps only where nothing is mapped.
        let mapped = unsafe {
            libc::mmap(STACK_BASE as *mut libc::c_void, length, libc::PROT_NONE,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS | libc::MAP_NORESERVE | libc::MAP_STACK | MAP_FIXED_NOREPLACE, -1, 0)
        };
        if mapped as usize != STACK_BASE {
            if mapped != libc::MAP_FAILED {
                // SAFETY: the mapping just made, which nothing names yet.
                unsafe { libc::munmap(mapped, length) };
            }
            return Err(body);
        }
        let stack = (STACK_BASE + GUARD_BYTES) as *mut libc::c_void;
        let unmap = || {
            // SAFETY: the stack mapping made above; its thread has ended or never started.
            unsafe { libc::munmap(STACK_BASE as *mut libc::c_void, length) };
        };
        // SAFETY: the range is this mapping's, above its guard.
        if unsafe { libc::mprotect(stack, length - GUARD_BYTES, libc::PROT_READ | libc::PROT_WRITE) } != 0 {
            unmap();
            return Err(body);
        }
        extern "C" fn start(argument: *mut libc::c_void) -> *mut libc::c_void {
            // SAFETY: `run_fixed` passes a leaked `Box<Box<dyn FnOnce() + Send>>` and keeps
            // nothing of it.
            let body = unsafe { Box::from_raw(argument.cast::<Box<dyn FnOnce() + Send>>()) };
            // A panic must not unwind into the C library's frames.
            let finished = std::panic::catch_unwind(std::panic::AssertUnwindSafe(*body)).is_ok();
            finished as usize as *mut libc::c_void
        }
        let argument = Box::into_raw(Box::new(body)).cast::<libc::c_void>();
        // SAFETY: the attribute lives across the create; the stack outlives the thread, which
        // is joined before the stack is unmapped.
        unsafe {
            let mut attributes: libc::pthread_attr_t = std::mem::zeroed();
            libc::pthread_attr_init(&mut attributes);
            libc::pthread_attr_setstack(&mut attributes, stack, length - GUARD_BYTES);
            let mut thread: libc::pthread_t = 0;
            let created = libc::pthread_create(&mut thread, &attributes, start, argument);
            libc::pthread_attr_destroy(&mut attributes);
            if created != 0 {
                let body = Box::from_raw(argument.cast::<Box<dyn FnOnce() + Send>>());
                unmap();
                return Err(*body);
            }
            libc::pthread_setname_np(thread, name.as_ptr());
            let mut finished: *mut libc::c_void = std::ptr::null_mut();
            libc::pthread_join(thread, &mut finished);
            unmap();
            Ok(!finished.is_null())
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (name, stack_bytes);
        Err(body)
    }
}
