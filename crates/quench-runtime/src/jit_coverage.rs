//! `QUENCH_TRACE=jit-coverage`: how much of a run's time compiled code covers.
//!
//! A timer samples the JavaScript thread's program counter every `SAMPLE_MICROS`. A sample
//! whose counter lies in the code zone (`note_code_region`) is compiled code; any other is resolved against the executable's symbol table at exit and
//! sorted into the interpreter, the shared runtime (object model, builtins, collector) or the
//! code generator by the module that owns the symbol (`CATEGORIES`). The report is the share
//! of each, with compiled code's share of the time spent executing bytecode (`jit` over `jit +
//! interpreter`) as the coverage figure, and the hottest symbols outside compiled code.
//! With `jit-coverage=PATH` the report is appended to `PATH` as well as written to stderr.
//! Sampling starts when the first VM is made, on its thread (`sample_this_thread`).

use std::sync::atomic::{AtomicPtr, AtomicU64, AtomicUsize, Ordering};

/// The sampling period, in microseconds.
const SAMPLE_MICROS: i64 = 250;
/// Samples kept: about 17 minutes at the sampling period.
const SAMPLE_CAPACITY: usize = 1 << 22;
/// The code zone reserves few regions (64 MiB each); more are not sampled.
const REGION_CAPACITY: usize = 64;
/// Hottest symbols outside compiled code the report lists.
const REPORTED_SYMBOLS: usize = 30;
const NANOS_PER_MICRO: i64 = 1000;
use crate::units::PERCENT;

static SAMPLES: AtomicPtr<AtomicU64> = AtomicPtr::new(std::ptr::null_mut());
static TAKEN: AtomicUsize = AtomicUsize::new(0);
/// The code zone's regions, `[start, end)`, published before any code runs there.
static REGION_STARTS: [AtomicUsize; REGION_CAPACITY] = [const { AtomicUsize::new(0) }; REGION_CAPACITY];
static REGION_ENDS: [AtomicUsize; REGION_CAPACITY] = [const { AtomicUsize::new(0) }; REGION_CAPACITY];
static REGION_COUNT: AtomicUsize = AtomicUsize::new(0);

/// Records a code zone region, so samples inside it count as compiled code.
pub(crate) fn note_code_region(start: usize, bytes: usize) {
    let index = REGION_COUNT.load(Ordering::Relaxed);
    if index < REGION_CAPACITY {
        REGION_STARTS[index].store(start, Ordering::Relaxed);
        REGION_ENDS[index].store(start + bytes, Ordering::Relaxed);
        REGION_COUNT.store(index + 1, Ordering::Release);
    }
}

fn in_code_zone(pc: usize) -> bool {
    let count = REGION_COUNT.load(Ordering::Acquire);
    (0..count).any(|index| {
        (REGION_STARTS[index].load(Ordering::Relaxed)..REGION_ENDS[index].load(Ordering::Relaxed)).contains(&pc)
    })
}

#[cfg(all(target_os = "linux", any(target_arch = "x86_64", target_arch = "aarch64")))]
pub(crate) use sampler::sample_this_thread;
#[cfg(not(all(target_os = "linux", any(target_arch = "x86_64", target_arch = "aarch64"))))]
pub(crate) fn sample_this_thread() {}

#[cfg(all(target_os = "linux", any(target_arch = "x86_64", target_arch = "aarch64")))]
mod sampler {
    use super::*;

    static TIMER: AtomicUsize = AtomicUsize::new(0);

    /// Starts sampling the calling thread, the first to make a VM, if the channel is on.
    pub(crate) fn sample_this_thread() {
        static STARTED: std::sync::Once = std::sync::Once::new();
        if crate::trace::Channel::JitCoverage.on() {
            STARTED.call_once(start);
        }
    }

    fn start() {
        let samples: Box<[AtomicU64]> = (0..SAMPLE_CAPACITY).map(|_| AtomicU64::new(0)).collect();
        SAMPLES.store(Box::leak(samples).as_mut_ptr(), Ordering::Release);
        // SAFETY: installs a handler that only reads its context and stores to preallocated
        // atomics, then arms the sampling timer; `report` runs at normal exit.
        unsafe {
            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction = sample as *const () as usize;
            action.sa_flags = libc::SA_SIGINFO | libc::SA_RESTART;
            libc::sigaction(signal(), &action, std::ptr::null_mut());
            // A wall-clock timer aimed at this thread: CPU-time timers expire only at
            // scheduler ticks (4 ms at HZ=250), too coarse for a short run.
            let mut event: libc::sigevent = std::mem::zeroed();
            event.sigev_notify = libc::SIGEV_THREAD_ID;
            event.sigev_signo = signal();
            event.sigev_notify_thread_id = libc::gettid();
            let mut timer: libc::timer_t = std::mem::zeroed();
            let period = libc::timespec { tv_sec: 0, tv_nsec: SAMPLE_MICROS * NANOS_PER_MICRO };
            let schedule = libc::itimerspec { it_interval: period, it_value: period };
            if libc::timer_create(libc::CLOCK_MONOTONIC, &mut event, &mut timer) != 0
                || libc::timer_settime(timer, 0, &schedule, std::ptr::null_mut()) != 0
            {
                return;
            }
            TIMER.store(timer as usize, Ordering::Release);
            libc::atexit(report_at_exit);
        }
    }

    /// A real-time signal of its own, which no host mask or `process.on` handler claims.
    fn signal() -> libc::c_int {
        libc::SIGRTMAX() - 1
    }

    fn program_counter(context: *mut libc::c_void) -> u64 {
        // SAFETY: the kernel passes the interrupted thread's `ucontext_t`.
        let context = unsafe { &*context.cast::<libc::ucontext_t>() };
        #[cfg(target_arch = "x86_64")]
        return context.uc_mcontext.gregs[libc::REG_RIP as usize] as u64;
        #[cfg(target_arch = "aarch64")]
        return context.uc_mcontext.pc;
    }

    extern "C" fn sample(_: libc::c_int, _: *mut libc::siginfo_t, context: *mut libc::c_void) {
        let samples = SAMPLES.load(Ordering::Acquire);
        let index = TAKEN.fetch_add(1, Ordering::Relaxed);
        if samples.is_null() || index >= SAMPLE_CAPACITY {
            return;
        }
        // SAFETY: `index` is below the leaked buffer's length.
        unsafe { &*samples.add(index) }.store(program_counter(context), Ordering::Relaxed);
    }

    extern "C" fn report_at_exit() {
        // SAFETY: stops the timer `start` armed before the report reads the samples.
        unsafe { libc::timer_delete(TIMER.load(Ordering::Acquire) as libc::timer_t) };
        let samples = SAMPLES.load(Ordering::Acquire);
        let taken = TAKEN.load(Ordering::Relaxed).min(SAMPLE_CAPACITY);
        // SAFETY: the first `taken` entries were written by the stopped handler.
        let samples: Vec<u64> = (0..taken)
            .map(|index| unsafe { &*samples.add(index) }.load(Ordering::Relaxed))
            .collect();
        super::report(&samples);
    }
}

/// Where a sample's time went.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Category {
    Jit,
    Interpreter,
    Runtime,
    Codegen,
}

/// Symbol path fragments that place a symbol, first match wins; anything else is runtime.
const CATEGORIES: &[(&str, Category)] = &[
    ("cranelift", Category::Codegen),
    ("regalloc2", Category::Codegen),
    ("jit_backend", Category::Codegen),
    ("dispatch_fast", Category::Interpreter),
    ("dispatch_frame", Category::Interpreter),
    ("vm::dispatch::", Category::Interpreter),
    ("lane_", Category::Interpreter),
];

fn categorize(symbol: &str) -> Category {
    CATEGORIES
        .iter()
        .find(|(fragment, _)| symbol.contains(fragment))
        .map_or(Category::Runtime, |&(_, category)| category)
}

fn report(samples: &[u64]) {
    let symbols = symbols::Table::load();
    let mut counts = [0u64; Category::Codegen as usize + 1];
    let mut by_symbol: rustc_hash::FxHashMap<(Category, &str), u64> = rustc_hash::FxHashMap::default();
    for &sample in samples {
        let pc = sample as usize;
        let (category, name) = if in_code_zone(pc) {
            (Category::Jit, "<compiled code>")
        } else {
            let name = symbols.as_ref().and_then(|table| table.name(pc)).unwrap_or("<unknown>");
            (categorize(name), name)
        };
        counts[category as usize] += 1;
        *by_symbol.entry((category, name)).or_default() += 1;
    }
    let foreground = samples.len() as u64;
    let share = |count: u64| PERCENT * count as f64 / foreground.max(1) as f64;
    let executing = counts[Category::Jit as usize] + counts[Category::Interpreter as usize];
    let mut text = format!(
        "jit-coverage: {:.1}% of bytecode time compiled; {foreground} samples: jit {:.1}%, \
         interpreter {:.1}%, runtime {:.1}%, codegen {:.1}%\n",
        PERCENT * counts[Category::Jit as usize] as f64 / executing.max(1) as f64,
        share(counts[Category::Jit as usize]),
        share(counts[Category::Interpreter as usize]),
        share(counts[Category::Runtime as usize]),
        share(counts[Category::Codegen as usize]),
    );
    let mut hottest: Vec<((Category, &str), u64)> = by_symbol.into_iter().collect();
    hottest.sort_unstable_by_key(|&(_, count)| std::cmp::Reverse(count));
    for ((category, name), count) in hottest.into_iter().take(REPORTED_SYMBOLS) {
        text += &format!("  {:5.1}% {category:?} {name}\n", share(count));
    }
    eprint!("{text}");
    if let Some(path) = crate::trace::Channel::JitCoverage.value() {
        use std::io::Write;
        if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
            let _ = file.write_all(text.as_bytes());
        }
    }
}

/// The running executable's function symbols, demangled, by address.
mod symbols {
    pub(super) struct Table {
        /// `(start, end, name)` sorted by start, at run-time addresses.
        functions: Vec<(usize, usize, String)>,
    }

    impl Table {
        pub(super) fn load() -> Option<Self> {
            let image = std::fs::read("/proc/self/exe").ok()?;
            let bias = load_bias(&image)?;
            let mut functions = elf_functions(&image)?
                .into_iter()
                .map(|(start, size, name)| (start + bias, start + bias + size.max(1), demangle(name)))
                .collect::<Vec<_>>();
            functions.sort_unstable_by_key(|function| function.0);
            Some(Self { functions })
        }

        pub(super) fn name(&self, pc: usize) -> Option<&str> {
            let index = self.functions.partition_point(|function| function.0 <= pc).checked_sub(1)?;
            let (_, end, name) = &self.functions[index];
            (pc < *end).then_some(name.as_str())
        }
    }

    /// ELF64 layout: header fields, section header and symbol fields, as `(offset, bytes)`.
    const ELF_TYPE: (usize, usize) = (0x10, 2);
    const ELF_SECTIONS: (usize, usize) = (0x28, 8);
    const ELF_SECTION_COUNT: (usize, usize) = (0x3c, 2);
    const SECTION_HEADER_BYTES: usize = 64;
    const SECTION_TYPE: (usize, usize) = (0x04, 4);
    const SECTION_OFFSET: (usize, usize) = (0x18, 8);
    const SECTION_SIZE: (usize, usize) = (0x20, 8);
    const SECTION_LINK: (usize, usize) = (0x28, 4);
    const SYMBOL_BYTES: usize = 24;
    const SYMBOL_NAME: (usize, usize) = (0x00, 4);
    const SYMBOL_INFO: (usize, usize) = (0x04, 1);
    const SYMBOL_VALUE: (usize, usize) = (0x08, 8);
    const SYMBOL_SIZE: (usize, usize) = (0x10, 8);
    const SYMBOL_TYPE_MASK: usize = 0xf;
    const SHT_SYMTAB: usize = 2;
    const STT_FUNC: usize = 2;
    /// A position-independent image, mapped at a load bias.
    const ET_DYN: usize = 3;
    const HEX: u32 = 16;
    /// `/proc/self/maps` columns: the address range, then permissions, then the file offset.
    const MAPS_OFFSET_COLUMN: usize = 2;

    /// Where the executable's first segment is mapped (zero for a fixed-address image).
    fn load_bias(image: &[u8]) -> Option<usize> {
        if field(image, 0, ELF_TYPE)? != ET_DYN {
            return Some(0);
        }
        let exe = std::fs::read_link("/proc/self/exe").ok()?;
        let maps = std::fs::read_to_string("/proc/self/maps").ok()?;
        let exe = exe.to_string_lossy();
        let line = maps.lines().find(|line| line.ends_with(exe.as_ref()))?;
        let start = usize::from_str_radix(line.split('-').next()?, HEX).ok()?;
        let offset = usize::from_str_radix(line.split_whitespace().nth(MAPS_OFFSET_COLUMN)?, HEX).ok()?;
        Some(start - offset)
    }

    /// The little-endian field `(offset, bytes)` of the record at `base`.
    fn field(image: &[u8], base: usize, (offset, bytes): (usize, usize)) -> Option<usize> {
        let at = base + offset;
        let field = image.get(at..at + bytes)?;
        Some(field.iter().rev().fold(0, |value, byte| value << u8::BITS | *byte as usize))
    }

    /// The `(value, size, name)` of each function symbol in an ELF64 image's symbol table.
    fn elf_functions(image: &[u8]) -> Option<Vec<(usize, usize, &str)>> {
        let sections = field(image, 0, ELF_SECTIONS)?;
        let header = |index: usize| sections + index * SECTION_HEADER_BYTES;
        let symtab = (0..field(image, 0, ELF_SECTION_COUNT)?)
            .map(header)
            .find(|&header| field(image, header, SECTION_TYPE) == Some(SHT_SYMTAB))?;
        let offset = field(image, symtab, SECTION_OFFSET)?;
        let size = field(image, symtab, SECTION_SIZE)?;
        let strings = field(image, header(field(image, symtab, SECTION_LINK)?), SECTION_OFFSET)?;
        Some(
            (offset..offset + size)
                .step_by(SYMBOL_BYTES)
                .filter(|&symbol| field(image, symbol, SYMBOL_INFO).is_some_and(|info| info & SYMBOL_TYPE_MASK == STT_FUNC))
                .filter_map(|symbol| {
                    let name = strings + field(image, symbol, SYMBOL_NAME)?;
                    let end = name + image.get(name..)?.iter().position(|byte| *byte == 0)?;
                    let name = std::str::from_utf8(&image[name..end]).ok()?;
                    Some((field(image, symbol, SYMBOL_VALUE)?, field(image, symbol, SYMBOL_SIZE)?, name))
                })
                .filter(|(value, _, _)| *value != 0)
                .collect(),
        )
    }

    /// A v0 Rust symbol's path (`_RNv..14quench_runtime2vm..`) as its identifiers joined by
    /// `::`, dropping disambiguators; others unchanged.
    fn demangle(symbol: &str) -> String {
        const DISAMBIGUATOR: &str = "s_";
        let Some(rest) = symbol.strip_prefix("_R") else {
            return symbol.to_owned();
        };
        let bytes = rest.as_bytes();
        let mut parts = Vec::new();
        let mut at = 0;
        while at < bytes.len() {
            let digits = bytes[at..].iter().take_while(|byte| byte.is_ascii_digit()).count();
            if digits == 0 {
                // A v0 disambiguator `s<base62>_` names no identifier.
                let base62 = bytes[at + 1..].iter().take_while(|byte| byte.is_ascii_alphanumeric()).count();
                at += if bytes[at] == b's' && bytes.get(at + 1 + base62) == Some(&b'_') { base62 + DISAMBIGUATOR.len() } else { 1 };
                continue;
            }
            let length: usize = rest[at..at + digits].parse().unwrap_or(0);
            at += digits + usize::from(bytes.get(at + digits) == Some(&b'_'));
            let Some(part) = rest.get(at..at + length) else { break };
            at += length;
            parts.push(part);
        }
        parts.join("::")
    }
}
