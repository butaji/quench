//! The operating-system side of the code zone: reserving a region, keeping it W^X, releasing
//! a span's pages, and making freshly written code visible to instruction fetch.
//!
//! Linux maps each region twice over the same shared pages: a read+write view the compiler
//! writes through and a read+execute view the code runs from, so no page is ever re-protected
//! while another thread may be running code on it. Darwin maps one `MAP_JIT` region and flips
//! only the writing thread to write access for the duration of a fill.

/// One reserved region: where code runs from and where the compiler writes it.
#[derive(Clone, Copy)]
pub(super) struct Views {
    pub(super) exec: *mut u8,
    pub(super) write: *mut u8,
}

impl Views {
    /// The writable address of `exec + offset`.
    pub(super) fn writer(&self, offset: usize) -> *mut u8 {
        self.write.wrapping_add(offset)
    }
}

#[cfg(target_os = "linux")]
impl Views {
    /// Reserves `bytes` (a multiple of the page size) as two views of one shared region.
    /// Pages are committed by first touch and charged nothing before it.
    pub(super) fn reserve(bytes: usize) -> Result<Self, String> {
        // SAFETY: a fresh shared anonymous mapping; the result is checked.
        let write = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                bytes,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED | libc::MAP_ANONYMOUS | libc::MAP_NORESERVE,
                -1,
                0,
            )
        };
        if write == libc::MAP_FAILED {
            return Err("code region reservation failed".into());
        }
        // SAFETY: an old size of zero on a shared mapping maps the same pages a second time.
        let exec = unsafe { libc::mremap(write, 0, bytes, libc::MREMAP_MAYMOVE) };
        // SAFETY: the alias just created, `bytes` long.
        if exec == libc::MAP_FAILED
            || unsafe { libc::mprotect(exec, bytes, libc::PROT_READ | libc::PROT_EXEC) } != 0
        {
            // SAFETY: the mapping created above, not yet shared.
            unsafe { libc::munmap(write, bytes) };
            return Err("code region alias failed".into());
        }
        Ok(Self {
            exec: exec.cast(),
            write: write.cast(),
        })
    }

    /// Returns the pages of `[offset, offset + bytes)` to the system; both views read zero.
    pub(super) fn release(&self, offset: usize, bytes: usize) {
        // SAFETY: a page-aligned range of this region's shared pages, which no live code uses.
        unsafe { libc::madvise(self.writer(offset).cast(), bytes, libc::MADV_REMOVE) };
    }

    /// Runs `fill` with write access to the writable view.
    pub(super) fn writing<T>(fill: impl FnOnce() -> T) -> T {
        fill()
    }

    /// Unmaps the written pages from the writable view once code is published, so each
    /// resident code page is counted once (through the executable view). The shared pages
    /// keep their contents.
    pub(super) fn retire_writer(&self, offset: usize, bytes: usize) {
        let (start, length) = page_span(offset, bytes);
        // SAFETY: page-aligned writable-view range; `DONTNEED` on shared pages drops only
        // this view's page-table entries.
        unsafe { libc::madvise(self.writer(start).cast(), length, libc::MADV_DONTNEED) };
    }
}

#[cfg(target_vendor = "apple")]
impl Views {
    /// Reserves `bytes` as one `MAP_JIT` region; each thread sees it executable until it
    /// opens a write window.
    pub(super) fn reserve(bytes: usize) -> Result<Self, String> {
        // SAFETY: a fresh private JIT mapping; the result is checked.
        let exec = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                bytes,
                libc::PROT_READ | libc::PROT_WRITE | libc::PROT_EXEC,
                libc::MAP_PRIVATE | libc::MAP_ANON | libc::MAP_JIT,
                -1,
                0,
            )
        };
        if exec == libc::MAP_FAILED {
            return Err("code region reservation failed".into());
        }
        Ok(Self {
            exec: exec.cast(),
            write: exec.cast(),
        })
    }

    pub(super) fn release(&self, offset: usize, bytes: usize) {
        // SAFETY: a page-aligned range of this region, which no live code uses.
        unsafe { libc::madvise(self.writer(offset).cast(), bytes, libc::MADV_FREE) };
    }

    /// Runs `fill` with this thread's view of JIT memory writable; other threads keep
    /// executing.
    pub(super) fn writing<T>(fill: impl FnOnce() -> T) -> T {
        // SAFETY: toggles only this thread's JIT write protection, restored below.
        unsafe { libc::pthread_jit_write_protect_np(0) };
        let result = fill();
        // SAFETY: as above.
        unsafe { libc::pthread_jit_write_protect_np(1) };
        result
    }

    pub(super) fn retire_writer(&self, _: usize, _: usize) {}
}

#[cfg(not(any(target_os = "linux", target_vendor = "apple")))]
compile_error!("the code zone needs a W^X mapping scheme for this operating system");

#[cfg(target_os = "linux")]
/// The page-aligned span covering `[offset, offset + bytes)`.
fn page_span(offset: usize, bytes: usize) -> (usize, usize) {
    let page = page_bytes();
    let start = offset - offset % page;
    (start, (offset + bytes).next_multiple_of(page) - start)
}

/// The system page size, which every region and chunk is a multiple of.
#[cfg(target_os = "linux")]
fn page_bytes() -> usize {
    static PAGE: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    // SAFETY: `sysconf` has no preconditions.
    *PAGE.get_or_init(|| {
        usize::try_from(unsafe { libc::sysconf(libc::_SC_PAGESIZE) }).expect("page size")
    })
}

/// Makes `bytes` just written at `code` visible to instruction fetch on this core. Data and
/// instruction caches are coherent on x86-64; on AArch64 a reused block may hold stale
/// instructions until its lines are cleaned to the point of unification and invalidated.
pub(super) fn flush_icache(code: *mut u8, bytes: usize) {
    #[cfg(all(target_arch = "aarch64", target_vendor = "apple"))]
    {
        unsafe extern "C" {
            fn sys_icache_invalidate(start: *mut std::ffi::c_void, len: usize);
        }
        // SAFETY: the executable range just filled.
        unsafe { sys_icache_invalidate(code.cast(), bytes) };
    }
    #[cfg(all(target_arch = "aarch64", not(target_vendor = "apple")))]
    aarch64::flush(code as usize, bytes);
    #[cfg(not(target_arch = "aarch64"))]
    let _ = (code, bytes);
}

#[cfg(all(target_arch = "aarch64", not(target_vendor = "apple")))]
mod aarch64 {
    /// `CTR_EL0` encodes each cache's line size as log2 of its words.
    const DMIN_LINE_SHIFT: u64 = 16;
    const LINE_FIELD_MASK: u64 = 0xf;
    const WORD_BYTES: usize = 4;

    pub(super) fn flush(start: usize, bytes: usize) {
        let ctr: u64;
        // SAFETY: `CTR_EL0` is readable at EL0 on Linux.
        unsafe { core::arch::asm!("mrs {}, ctr_el0", out(reg) ctr) };
        let data_line = WORD_BYTES << ((ctr >> DMIN_LINE_SHIFT) & LINE_FIELD_MASK);
        let instruction_line = WORD_BYTES << (ctr & LINE_FIELD_MASK);
        let end = start + bytes;
        for line in (start & !(data_line - 1)..end).step_by(data_line) {
            // SAFETY: cleans a readable line of the executable view.
            unsafe { core::arch::asm!("dc cvau, {}", in(reg) line) };
        }
        // SAFETY: barriers have no operands.
        unsafe { core::arch::asm!("dsb ish") };
        for line in (start & !(instruction_line - 1)..end).step_by(instruction_line) {
            // SAFETY: invalidates instruction lines of the executable view.
            unsafe { core::arch::asm!("ic ivau, {}", in(reg) line) };
        }
        // SAFETY: as above.
        unsafe { core::arch::asm!("dsb ish", "isb") };
    }
}
